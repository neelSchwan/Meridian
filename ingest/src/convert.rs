use core_types::{DomainError, MatchRecord, Participant, Role, Tier};
use riven::consts::Queue;
use riven::models::match_v5::Match;

/// Games shorter than this are remakes, which say nothing about balance.
const MIN_DURATION_SECS: i64 = 300;

/// Turns a Riot match into a record, or `None` when it should not be counted: anything other
/// than a ranked solo queue game on Summoner's Rift, remakes, and games ended by early surrender.
///
/// `region` and `tier` describe how the match was found, because Riot's match data carries
/// neither.
pub fn to_record(
    game: &Match,
    region: &str,
    tier: Tier,
) -> Result<Option<MatchRecord>, DomainError> {
    let info = &game.info;
    let remade = info.game_duration < MIN_DURATION_SECS
        || info
            .participants
            .iter()
            .any(|participant| participant.game_ended_in_early_surrender);
    if info.queue_id != Queue::SUMMONERS_RIFT_5V5_RANKED_SOLO || remade {
        return Ok(None);
    }

    let participants = info
        .participants
        .iter()
        .map(|participant| Participant {
            champion: participant.champion_name.clone(),
            // Riot sends an empty position when it could not tell, which stays unknown.
            role: participant.team_position.parse::<Role>().ok(),
            won: participant.win,
        })
        .collect();
    // A ban slot left empty has champion id -1, which has no name.
    let bans = info
        .teams
        .iter()
        .flat_map(|team| &team.bans)
        .filter_map(|ban| ban.champion_id.identifier())
        .map(str::to_string)
        .collect();
    Ok(Some(MatchRecord {
        match_id: game.metadata.match_id.clone(),
        patch: info.game_version.parse()?,
        region: region.to_string(),
        tier,
        started_at: info.game_start_timestamp / 1000,
        duration_secs: info.game_duration,
        participants,
        bans,
    }))
}
