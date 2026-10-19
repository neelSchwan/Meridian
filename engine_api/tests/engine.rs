use core_types::{BalanceChange, ChangeKind, Filter, MatchRecord, Participant, Patch, Role, Tier};
use engine_api::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    init_db(&conn).unwrap();
    conn
}

fn patch(text: &str) -> Patch {
    text.parse().unwrap()
}

fn participant(champion: &str, role: Option<Role>, won: bool) -> Participant {
    Participant {
        champion: champion.to_string(),
        role,
        won,
    }
}

/// A match with a fresh id, so tests can add as many as they need.
fn game(patch_text: &str, region: &str, tier: Tier, participants: Vec<Participant>) -> MatchRecord {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    MatchRecord {
        match_id: format!("TEST_{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)),
        patch: patch(patch_text),
        region: region.to_string(),
        tier,
        started_at: 1_700_000_000,
        duration_secs: 1800,
        participants,
        bans: vec![],
    }
}

/// Records `wins + losses` matches in which only `champion` is played, so a champion's pick rate
/// is its share of the matches the test has recorded.
fn play(conn: &Connection, patch_text: &str, champion: &str, wins: u64, losses: u64) {
    play_in(conn, patch_text, "na1", Tier::Gold, champion, wins, losses);
}

fn play_in(
    conn: &Connection,
    patch_text: &str,
    region: &str,
    tier: Tier,
    champion: &str,
    wins: u64,
    losses: u64,
) {
    for won in
        std::iter::repeat_n(true, wins as usize).chain(std::iter::repeat_n(false, losses as usize))
    {
        let record = game(
            patch_text,
            region,
            tier,
            vec![participant(champion, None, won)],
        );
        assert!(record_match(conn, &record).unwrap());
    }
}

fn stats_of(conn: &Connection, patch_text: &str, champion: &str) -> core_types::ChampionStats {
    champion_stats(conn, patch(patch_text), &Filter::default())
        .unwrap()
        .into_iter()
        .find(|stats| stats.champion == champion)
        .unwrap()
}

fn change(champion: &str, kind: ChangeKind) -> BalanceChange {
    BalanceChange {
        champion: champion.to_string(),
        ability: "Q - Orb".to_string(),
        attribute: "Damage".to_string(),
        old_value: "80".to_string(),
        new_value: "75".to_string(),
        kind,
    }
}

#[test]
fn a_recorded_match_counts_toward_wins_picks_and_bans() {
    let conn = db();
    let mut record = game(
        "16.1",
        "na1",
        Tier::Gold,
        vec![
            participant("Ahri", Some(Role::Middle), true),
            participant("Jinx", Some(Role::Bottom), false),
        ],
    );
    record.bans = vec!["Ahri".to_string(), "Zed".to_string()];
    assert!(record_match(&conn, &record).unwrap());
    play(&conn, "16.1", "Lux", 0, 1);

    let ahri = stats_of(&conn, "16.1", "Ahri");
    assert_eq!(
        (ahri.games, ahri.wins, ahri.bans, ahri.total_matches),
        (1, 1, 1, 2)
    );
    assert_eq!(ahri.pick_rate(), 0.5);
    let zed = stats_of(&conn, "16.1", "Zed");
    assert_eq!((zed.games, zed.bans), (0, 1));
    assert_eq!(stats_of(&conn, "16.1", "Jinx").win_rate(), Some(0.0));
}

#[test]
fn recording_a_match_twice_changes_nothing() {
    let conn = db();
    let record = game(
        "16.1",
        "na1",
        Tier::Gold,
        vec![participant("Ahri", None, true)],
    );

    assert!(record_match(&conn, &record).unwrap());
    assert!(has_match(&conn, &record.match_id).unwrap());
    assert!(!record_match(&conn, &record).unwrap());

    assert_eq!(stats_of(&conn, "16.1", "Ahri").games, 1);
}

#[test]
fn an_invalid_match_stores_nothing() {
    let conn = db();
    let record = game(
        "16.1",
        "na1",
        Tier::Gold,
        vec![
            participant("Ahri", None, true),
            participant("ahri", None, false),
        ],
    );

    assert!(matches!(
        record_match(&conn, &record),
        Err(EngineError::Domain(_))
    ));

    assert!(!has_match(&conn, &record.match_id).unwrap());
    assert!(list_patches(&conn).unwrap().is_empty());
}

#[test]
fn the_same_champion_under_two_spellings_is_one_champion() {
    let conn = db();
    play(&conn, "16.1", "MonkeyKing", 3, 1);
    replace_balance_changes(&conn, patch("16.1"), &[change("Wukong", ChangeKind::Nerf)]).unwrap();

    assert_eq!(stats_of(&conn, "16.1", "MonkeyKing").games, 4);
    let history = champion_history(&conn, "wukong", &Filter::default()).unwrap();
    assert_eq!(history[0].stats.games, 4);
    assert_eq!(history[0].changes.len(), 1);
}

#[test]
fn filters_narrow_statistics_to_a_region_tier_or_role() {
    let conn = db();
    play_in(&conn, "16.1", "na1", Tier::Gold, "Ahri", 6, 4);
    play_in(&conn, "16.1", "euw1", Tier::Gold, "Ahri", 1, 1);
    play_in(&conn, "16.1", "na1", Tier::Diamond, "Ahri", 2, 0);
    let mut record = game(
        "16.1",
        "na1",
        Tier::Gold,
        vec![participant("Ahri", Some(Role::Support), true)],
    );
    record.bans = vec!["Ahri".to_string()];
    record_match(&conn, &record).unwrap();

    let all = champion_stats(&conn, patch("16.1"), &Filter::default()).unwrap();
    assert_eq!(all[0].games, 15);

    let na = Filter {
        region: Some("na1".to_string()),
        ..Filter::default()
    };
    assert_eq!(
        champion_stats(&conn, patch("16.1"), &na).unwrap()[0].games,
        13
    );

    let shouted = Filter {
        region: Some("NA1".to_string()),
        ..Filter::default()
    };
    assert_eq!(
        champion_stats(&conn, patch("16.1"), &shouted).unwrap()[0].games,
        13
    );

    let diamond = Filter {
        tier: Some(Tier::Diamond),
        ..Filter::default()
    };
    let diamond_stats = champion_stats(&conn, patch("16.1"), &diamond).unwrap();
    assert_eq!(
        (diamond_stats[0].games, diamond_stats[0].total_matches),
        (2, 2)
    );

    let support = Filter {
        role: Some(Role::Support),
        ..Filter::default()
    };
    let support_stats = champion_stats(&conn, patch("16.1"), &support).unwrap();
    assert_eq!(
        (
            support_stats[0].games,
            support_stats[0].bans,
            support_stats[0].total_matches
        ),
        (1, 1, 15)
    );
}

#[test]
fn a_filter_that_matches_nothing_is_reported_as_a_missing_patch() {
    let conn = db();
    play(&conn, "16.1", "Ahri", 1, 1);
    let filter = Filter {
        region: Some("kr".to_string()),
        ..Filter::default()
    };

    let error = champion_stats(&conn, patch("16.1"), &filter).unwrap_err();

    assert!(matches!(error, EngineError::PatchNotFound(_)));
}

#[test]
fn the_report_measures_a_buff_against_the_previous_patch() {
    let conn = db();
    play(&conn, "16.1", "Jinx", 490, 510);
    play(&conn, "16.2", "Jinx", 550, 450);
    replace_balance_changes(&conn, patch("16.2"), &[change("Jinx", ChangeKind::Buff)]).unwrap();

    let report = patch_report(&conn, patch("16.2"), &Filter::default()).unwrap();

    assert_eq!(report.previous, patch("16.1"));
    assert_eq!((report.matches_before, report.matches_after), (1000, 1000));
    assert_eq!(report.champions_changed, 1);
    let jinx = &report.impacts[0];
    assert!((jinx.win_rate_delta.unwrap() - 0.06).abs() < 1e-12);
    assert_eq!(jinx.moved_as_intended, Some(true));
    assert!(jinx.significant());
    assert_eq!(report.largest_gain.as_ref().unwrap().champion, "Jinx");
}

#[test]
fn the_same_gain_on_a_small_sample_is_not_significant() {
    let conn = db();
    play(&conn, "16.1", "Jinx", 25, 25);
    play(&conn, "16.2", "Jinx", 30, 20);
    replace_balance_changes(&conn, patch("16.2"), &[change("Jinx", ChangeKind::Buff)]).unwrap();

    let jinx = &patch_report(&conn, patch("16.2"), &Filter::default())
        .unwrap()
        .impacts[0];

    assert!((jinx.win_rate_delta.unwrap() - 0.10).abs() < 1e-12);
    assert!(!jinx.significant());
}

#[test]
fn a_report_needs_the_patch_and_an_earlier_one() {
    let conn = db();
    play(&conn, "16.2", "Jinx", 40, 40);
    replace_balance_changes(&conn, patch("16.1"), &[change("Jinx", ChangeKind::Buff)]).unwrap();

    let first = patch_report(&conn, patch("16.2"), &Filter::default()).unwrap_err();
    let notes_only = patch_report(&conn, patch("16.1"), &Filter::default()).unwrap_err();

    assert!(matches!(first, EngineError::NoPreviousPatch(_)));
    assert!(matches!(notes_only, EngineError::PatchNotFound(_)));
}

#[test]
fn a_filtered_report_compares_with_the_last_patch_that_has_data_in_that_slice() {
    let conn = db();
    play_in(&conn, "16.1", "na1", Tier::Gold, "Ahri", 50, 50);
    play_in(&conn, "16.2", "euw1", Tier::Gold, "Ahri", 50, 50);
    play_in(&conn, "16.3", "na1", Tier::Gold, "Ahri", 60, 40);
    let na = Filter {
        region: Some("na1".to_string()),
        ..Filter::default()
    };

    let report = patch_report(&conn, patch("16.3"), &na).unwrap();

    assert_eq!(report.previous, patch("16.1"));
}

#[test]
fn history_lists_each_patch_with_a_smoothed_win_rate() {
    let conn = db();
    play(&conn, "16.1", "Ahri", 40, 60);
    play(&conn, "16.2", "Ahri", 60, 40);
    play(&conn, "16.3", "Lux", 50, 50);
    play(&conn, "16.4", "Ahri", 50, 50);

    let history = champion_history(&conn, "Ahri", &Filter::default()).unwrap();

    let patches: Vec<_> = history
        .iter()
        .map(|point| point.patch.to_string())
        .collect();
    assert_eq!(patches, ["16.1", "16.2", "16.4"]);
    let smoothed: Vec<_> = history
        .iter()
        .map(|point| point.smoothed_win_rate.unwrap())
        .collect();
    assert!((smoothed[1] - 0.5).abs() < 1e-12);
    assert!((smoothed[2] - 0.5).abs() < 1e-12);
    assert!((smoothed[0] - 0.4).abs() < 1e-12);
}

#[test]
fn history_of_an_unknown_champion_is_an_error() {
    let conn = db();
    play(&conn, "16.1", "Ahri", 1, 1);

    let error = champion_history(&conn, "Nobody", &Filter::default()).unwrap_err();

    assert_eq!(error.to_string(), "champion 'Nobody' not found");
}

#[test]
fn rankings_put_the_best_win_rate_first_and_skip_small_samples() {
    let conn = db();
    play(&conn, "16.1", "Ahri", 55, 45);
    play(&conn, "16.1", "Jinx", 60, 40);
    play(&conn, "16.1", "Zed", 9, 0);

    let ranked = rankings(&conn, patch("16.1"), &Filter::default()).unwrap();

    let names: Vec<_> = ranked.iter().map(|stats| stats.champion.as_str()).collect();
    assert_eq!(names, ["Jinx", "Ahri"]);
}

#[test]
fn meta_shifts_compare_each_patch_with_the_one_before() {
    let conn = db();
    for (patch_text, wins) in [("16.1", 50), ("16.2", 60), ("16.3", 60)] {
        play(&conn, patch_text, "Ahri", wins, 100 - wins);
    }

    let shifts = meta_shifts(&conn, &Filter::default()).unwrap();

    assert_eq!(shifts.len(), 2);
    assert!((shifts[0].mean_abs_win_rate_delta - 0.10).abs() < 1e-12);
    assert_eq!(shifts[1].mean_abs_win_rate_delta, 0.0);
}

#[test]
fn effectiveness_counts_how_often_changes_moved_win_rates_as_intended() {
    let conn = db();
    play(&conn, "16.1", "Jinx", 500, 500);
    play(&conn, "16.1", "Ahri", 540, 460);
    play(&conn, "16.2", "Jinx", 560, 440);
    play(&conn, "16.2", "Ahri", 500, 500);
    replace_balance_changes(
        &conn,
        patch("16.2"),
        &[
            change("Jinx", ChangeKind::Buff),
            change("Ahri", ChangeKind::Nerf),
        ],
    )
    .unwrap();

    let result = balance_effectiveness(&conn, &Filter::default()).unwrap();

    assert_eq!(
        (
            result[0].kind,
            result[0].measured,
            result[0].in_intended_direction
        ),
        (ChangeKind::Buff, 1, 1)
    );
    assert_eq!(
        (result[1].kind, result[1].measured, result[1].toward_even),
        (ChangeKind::Nerf, 1, 1)
    );
}

#[test]
fn importing_notes_again_replaces_that_patchs_changes() {
    let conn = db();
    replace_balance_changes(
        &conn,
        patch("16.2"),
        &[
            change("Ahri", ChangeKind::Nerf),
            change("Jinx", ChangeKind::Buff),
        ],
    )
    .unwrap();
    replace_balance_changes(&conn, patch("16.2"), &[change("Lux", ChangeKind::Buff)]).unwrap();
    replace_balance_changes(&conn, patch("16.3"), &[change("Zed", ChangeKind::Nerf)]).unwrap();

    let stored = balance_changes(&conn, patch("16.2")).unwrap();

    assert_eq!(stored, [change("Lux", ChangeKind::Buff)]);
    assert_eq!(balance_changes(&conn, patch("16.3")).unwrap().len(), 1);
}

#[test]
fn patches_are_listed_oldest_first_with_their_counts() {
    let conn = db();
    play(&conn, "16.10", "Ahri", 1, 1);
    play(&conn, "16.9", "Ahri", 1, 0);
    replace_balance_changes(&conn, patch("16.10"), &[change("Ahri", ChangeKind::Buff)]).unwrap();

    let listed = list_patches(&conn).unwrap();

    let summary: Vec<_> = listed
        .iter()
        .map(|p| (p.patch.to_string(), p.matches, p.balance_changes))
        .collect();
    assert_eq!(
        summary,
        [("16.9".to_string(), 1, 0), ("16.10".to_string(), 2, 1)]
    );
}

#[test]
fn the_markdown_report_shows_the_evidence_for_each_changed_champion() {
    let conn = db();
    play(&conn, "16.1", "Jinx", 490, 510);
    play(&conn, "16.2", "Jinx", 550, 450);
    play(&conn, "16.1", "Lux", 500, 500);
    play(&conn, "16.2", "Lux", 440, 560);
    replace_balance_changes(&conn, patch("16.2"), &[change("Jinx", ChangeKind::Buff)]).unwrap();

    let markdown =
        render_markdown(&patch_report(&conn, patch("16.2"), &Filter::default()).unwrap());

    assert!(markdown.contains("# Patch 16.2 impact report"));
    assert!(markdown.contains("Largest gain: Jinx +6.0pp"));
    assert!(markdown.contains("| Jinx | buff (1) | 49.0% to 55.0% | +6.0pp |"));
    assert!(markdown.contains("moved as intended"));
    assert!(markdown.contains("## Significant movers Riot did not change"));
    assert!(markdown.contains("| Lux | 50.0% to 44.0% | -6.0pp |"));
}

#[test]
fn the_csv_report_has_a_row_per_champion_and_quotes_awkward_names() {
    let conn = db();
    play(&conn, "16.1", "Nunu & Willump, Jr", 50, 50);
    play(&conn, "16.2", "Nunu & Willump, Jr", 60, 40);

    let csv = render_csv(&patch_report(&conn, patch("16.2"), &Filter::default()).unwrap());

    let lines: Vec<_> = csv.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("champion,changed,kind,"));
    assert!(
        lines[1].starts_with("\"Nunu & Willump, Jr\",false,,100,100,0.500000,0.600000,0.100000,")
    );
}

#[test]
fn every_tier_and_role_is_accepted_by_the_schema() {
    let conn = db();
    let tiers = [
        Tier::Iron,
        Tier::Bronze,
        Tier::Silver,
        Tier::Gold,
        Tier::Platinum,
        Tier::Emerald,
        Tier::Diamond,
        Tier::Master,
        Tier::Grandmaster,
        Tier::Challenger,
    ];
    let roles = [
        Role::Top,
        Role::Jungle,
        Role::Middle,
        Role::Bottom,
        Role::Support,
    ];
    for tier in tiers {
        let participants = roles
            .iter()
            .enumerate()
            .map(|(index, role)| participant(&format!("Champion{index}"), Some(*role), true))
            .collect();
        assert!(record_match(&conn, &game("16.1", "na1", tier, participants)).unwrap());
    }
}

#[test]
fn a_database_from_a_newer_version_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
        .unwrap();

    let error = init_db(&conn).unwrap_err();

    assert!(matches!(error, EngineError::UnsupportedSchemaVersion(_)));
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}

#[test]
fn initializing_twice_keeps_the_data() {
    let conn = db();
    play(&conn, "16.1", "Ahri", 1, 0);

    init_db(&conn).unwrap();

    assert_eq!(stats_of(&conn, "16.1", "Ahri").games, 1);
}

#[test]
fn two_connections_to_one_file_can_both_write() {
    let path = std::env::temp_dir().join(format!("meridian-test-{}.db", std::process::id()));
    let first = open_db(&path).unwrap();
    let second = open_db(&path).unwrap();

    play(&first, "16.1", "Ahri", 1, 0);
    play(&second, "16.1", "Jinx", 1, 0);

    assert_eq!(list_patches(&first).unwrap()[0].matches, 2);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
