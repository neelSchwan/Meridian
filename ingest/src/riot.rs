use core_types::Tier;
use engine_api::{Connection, has_match, record_match};
use riven::RiotApi;
use riven::consts::{Division, PlatformRoute, Queue, QueueType};
use riven::reqwest::StatusCode;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::BoxError;
use crate::convert::to_record;

/// Which players to start from and how far back to look.
///
/// Riot has no "all matches" feed, so matches are found through players: the players of one tier
/// in one region, then each player's recent ranked games. A match is labelled with the tier of the
/// player it was found through, so choose a tier for each run and compare like with like.
pub struct Sampling {
    pub route: PlatformRoute,
    pub tier: Tier,
    /// Required below Master; ignored for Master and above, which have no divisions.
    pub division: Division,
    /// Pages of the league listing to read, 200 players each below Master.
    pub pages: i32,
    pub max_players: usize,
    pub matches_per_player: i32,
    /// Only matches started within this many days are fetched.
    pub since_days: i64,
}

/// What an ingestion run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub players: usize,
    pub stored: usize,
    pub already_stored: usize,
    /// Fetched but not counted: other queues, remakes.
    pub not_counted: usize,
    /// Requests that failed after Riot's client retried them.
    pub failed: usize,
}

/// Finds players, lists their recent ranked matches, and stores the ones not seen before.
///
/// The `riven` client waits for Riot's rate limits and retries throttled and failed requests
/// itself. A request that still fails is counted and skipped, and since a match is stored only
/// once complete, the next run picks it up again. An invalid API key stops the run.
pub async fn ingest_matches(
    api: &RiotApi,
    conn: &Connection,
    sampling: &Sampling,
) -> Result<Summary, BoxError> {
    let region = sampling.route.to_string().to_lowercase();
    let regional = sampling.route.to_regional();
    let since = unix_now()? - sampling.since_days * 24 * 60 * 60;
    let mut summary = Summary::default();

    let players = find_players(api, sampling).await?;
    summary.players = players.len();
    for puuid in &players {
        let ids = match api
            .match_v5()
            .get_match_ids_by_puuid(
                regional,
                puuid,
                Some(sampling.matches_per_player),
                None,
                Some(Queue::SUMMONERS_RIFT_5V5_RANKED_SOLO),
                Some(since),
                None,
                None,
            )
            .await
        {
            Ok(ids) => ids,
            Err(e) => {
                note_failure(&e)?;
                summary.failed += 1;
                continue;
            }
        };
        for id in ids {
            if has_match(conn, &id)? {
                summary.already_stored += 1;
                continue;
            }
            let game = match api.match_v5().get_match(regional, &id).await {
                Ok(Some(game)) => game,
                Ok(None) => {
                    summary.failed += 1;
                    continue;
                }
                Err(e) => {
                    note_failure(&e)?;
                    summary.failed += 1;
                    continue;
                }
            };
            match to_record(&game, &region, sampling.tier)? {
                Some(record) => {
                    if record_match(conn, &record)? {
                        summary.stored += 1;
                    }
                }
                None => summary.not_counted += 1,
            }
        }
    }
    Ok(summary)
}

/// The PUUIDs of up to `max_players` ranked solo players in the sampled tier. Riot returns Master
/// and above unordered, so a cap there picks an arbitrary subset.
async fn find_players(api: &RiotApi, sampling: &Sampling) -> Result<Vec<String>, BoxError> {
    let queue = QueueType::RANKED_SOLO_5x5;
    let tier = sampling.tier.as_str().parse()?;
    let mut puuids = Vec::new();
    if sampling.tier.is_apex() {
        let league = match sampling.tier {
            Tier::Master => {
                api.league_v4()
                    .get_master_league(sampling.route, queue)
                    .await?
            }
            Tier::Grandmaster => {
                api.league_v4()
                    .get_grandmaster_league(sampling.route, queue)
                    .await?
            }
            // The tier is apex, so only Challenger is left.
            _ => {
                api.league_v4()
                    .get_challenger_league(sampling.route, queue)
                    .await?
            }
        };
        puuids.extend(league.entries.into_iter().map(|entry| entry.puuid));
    } else {
        for page in 1..=sampling.pages {
            let entries = api
                .league_v4()
                .get_league_entries(
                    sampling.route,
                    queue.clone(),
                    tier,
                    sampling.division,
                    Some(page),
                )
                .await?
                .unwrap_or_default();
            if entries.is_empty() {
                break;
            }
            puuids.extend(entries.into_iter().map(|entry| entry.puuid));
        }
    }
    puuids.truncate(sampling.max_players);
    Ok(puuids)
}

/// Logs a failed request and lets the run continue, unless Riot is refusing the API key, which
/// no later request can fix.
fn note_failure(error: &riven::RiotApiError) -> Result<(), BoxError> {
    if matches!(
        error.status_code(),
        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    ) {
        return Err(
            "Riot rejected the API key (it may have expired; development keys last 24 hours)"
                .into(),
        );
    }
    eprintln!("warning: request failed: {error}");
    Ok(())
}

fn unix_now() -> Result<i64, BoxError> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(elapsed.as_secs() as i64)
}
