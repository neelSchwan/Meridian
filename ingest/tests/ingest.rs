use core_types::{Patch, Role, Tier};
use engine_api::{Connection, balance_changes, init_db};
use ingest::{import_notes, to_record};
use riven::models::match_v5::Match;
use serde_json::{Value, json};

const PARTICIPANT_TEMPLATE: &str = include_str!("fixtures/participant.json");

fn participant(champion: &str, position: &str, won: bool) -> Value {
    let mut value: Value = serde_json::from_str(PARTICIPANT_TEMPLATE).unwrap();
    value["championName"] = json!(champion);
    value["teamPosition"] = json!(position);
    value["win"] = json!(won);
    value
}

fn objective() -> Value {
    json!({ "first": false, "kills": 0 })
}

fn team(id: u32, won: bool, bans: &[i32]) -> Value {
    let bans: Vec<Value> = bans
        .iter()
        .enumerate()
        .map(|(turn, champion)| json!({ "championId": champion, "pickTurn": turn + 1 }))
        .collect();
    json!({
        "bans": bans,
        "objectives": {
            "baron": objective(), "champion": objective(), "dragon": objective(),
            "inhibitor": objective(), "riftHerald": objective(), "tower": objective(),
        },
        "teamId": id,
        "win": won,
    })
}

/// A ranked solo game of patch 16.2 where Ahri (mid) and Jinx (bot) win against Zed and Lux.
fn ranked_game() -> Value {
    json!({
        "metadata": { "dataVersion": "2", "matchId": "NA1_100", "participants": [] },
        "info": {
            "gameCreation": 0,
            "gameDuration": 1800,
            "gameId": 100,
            "gameMode": "CLASSIC",
            "gameName": "test",
            "gameStartTimestamp": 1_760_000_000_000_i64,
            "gameType": "MATCHED_GAME",
            "gameVersion": "16.2.724.5821",
            "mapId": 11,
            "platformId": "NA1",
            "queueId": 420,
            "participants": [
                participant("Ahri", "MIDDLE", true),
                participant("Jinx", "BOTTOM", true),
                participant("Zed", "MIDDLE", false),
                participant("Lux", "UTILITY", false),
                participant("MonkeyKing", "", false),
            ],
            "teams": [team(100, true, &[103, -1]), team(200, false, &[238])],
        },
    })
}

/// Riot's enums deserialize from borrowed strings, so the JSON has to go through text.
fn parse(value: Value) -> Match {
    serde_json::from_str(&value.to_string()).unwrap()
}

#[test]
fn a_ranked_game_becomes_a_record() {
    let record = to_record(&parse(ranked_game()), "na1", Tier::Diamond)
        .unwrap()
        .unwrap();

    assert_eq!(record.match_id, "NA1_100");
    assert_eq!(record.patch, "16.2".parse::<Patch>().unwrap());
    assert_eq!(
        (record.region.as_str(), record.tier),
        ("na1", Tier::Diamond)
    );
    assert_eq!(record.started_at, 1_760_000_000);
    assert_eq!(record.duration_secs, 1800);

    let ahri = &record.participants[0];
    assert_eq!(
        (ahri.champion.as_str(), ahri.role, ahri.won),
        ("Ahri", Some(Role::Middle), true)
    );
    assert_eq!(record.participants[3].role, Some(Role::Support));
    assert_eq!(record.participants[4].role, None);
    assert_eq!(record.bans, ["Ahri", "Zed"]);
    record.validate().unwrap();
}

#[test]
fn other_queues_are_not_counted() {
    let mut game = ranked_game();
    game["info"]["queueId"] = json!(450);

    assert_eq!(to_record(&parse(game), "na1", Tier::Gold).unwrap(), None);
}

#[test]
fn remakes_and_early_surrenders_are_not_counted() {
    let mut short = ranked_game();
    short["info"]["gameDuration"] = json!(180);
    let mut surrendered = ranked_game();
    surrendered["info"]["participants"][0]["gameEndedInEarlySurrender"] = json!(true);

    assert_eq!(to_record(&parse(short), "na1", Tier::Gold).unwrap(), None);
    assert_eq!(
        to_record(&parse(surrendered), "na1", Tier::Gold).unwrap(),
        None
    );
}

#[test]
fn an_unreadable_game_version_is_an_error() {
    let mut game = ranked_game();
    game["info"]["gameVersion"] = json!("unknown");

    assert!(to_record(&parse(game), "na1", Tier::Gold).is_err());
}

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn
}

const NOTES: &str = r#"<div class="patch-change-block"><h3 class="change-title">
    <a href="https://www.leagueoflegends.com/en-us/champions/ahri/">Ahri</a></h3>
    <h4>Q - Orb of Deception</h4><ul><li><strong>Damage</strong>: 80 ⇒ <strong>75</strong></li></ul></div>"#;

#[test]
fn importing_notes_stores_the_changes_for_that_patch() {
    let conn = db();
    let patch: Patch = "16.2".parse().unwrap();

    let count = import_notes(&conn, patch, NOTES).unwrap();

    assert_eq!(count, 1);
    let stored = balance_changes(&conn, patch).unwrap();
    assert_eq!(
        (stored[0].champion.as_str(), stored[0].attribute.as_str()),
        ("Ahri", "Damage")
    );
}

#[test]
fn importing_a_page_without_champions_fails_and_keeps_earlier_changes() {
    let conn = db();
    let patch: Patch = "16.2".parse().unwrap();
    import_notes(&conn, patch, NOTES).unwrap();

    let error = import_notes(&conn, patch, "<html><body>moved</body></html>").unwrap_err();

    assert!(error.to_string().contains("layout may have changed"));
    assert_eq!(balance_changes(&conn, patch).unwrap().len(), 1);
}
