//! Exercises the HTTP API through `handle`, without a network socket.

use core_types::{BalanceChange, ChangeKind, MatchRecord, Participant, Tier};
use engine_api::{Connection, init_db, record_match, replace_balance_changes};
use meridian_server::{Request, handle};
use serde_json::Value;

/// Records `wins + losses` matches of one champion, enough games for comparisons to be tested.
fn play(conn: &Connection, patch: &str, region: &str, champion: &str, wins: u64, losses: u64) {
    for index in 0..wins + losses {
        let record = MatchRecord {
            match_id: format!("{patch}-{region}-{champion}-{index}"),
            patch: patch.parse().unwrap(),
            region: region.to_string(),
            tier: Tier::Gold,
            started_at: 1_700_000_000,
            duration_secs: 1800,
            participants: vec![Participant {
                champion: champion.to_string(),
                role: None,
                won: index < wins,
            }],
            bans: vec![],
        };
        record_match(conn, &record).unwrap();
    }
}

/// Two patches in which Jinx is buffed and her win rate rises from 49% to 55%.
fn seeded_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    play(&conn, "16.1", "na1", "Jinx", 490, 510);
    play(&conn, "16.2", "na1", "Jinx", 550, 450);
    play(&conn, "16.2", "euw1", "Kai'Sa", 40, 40);
    let buff = BalanceChange {
        champion: "Jinx".to_string(),
        ability: "Q".to_string(),
        attribute: "Damage".to_string(),
        old_value: "80".to_string(),
        new_value: "90".to_string(),
        kind: ChangeKind::Buff,
    };
    replace_balance_changes(&conn, "16.2".parse().unwrap(), &[buff]).unwrap();
    conn
}

fn get_from(
    conn: &Connection,
    host: Option<&str>,
    method: &str,
    url: &str,
) -> (u16, String, String) {
    let response = handle(conn, &Request { method, url, host });
    (
        response.status,
        response.content_type.to_string(),
        response.body,
    )
}

/// Sends a GET to localhost and returns (status, parsed JSON body).
fn get(conn: &Connection, url: &str) -> (u16, Value) {
    let (status, _, body) = get_from(conn, Some("localhost:7979"), "GET", url);
    (status, serde_json::from_str(&body).unwrap())
}

fn ok(conn: &Connection, url: &str) -> Value {
    let (status, value) = get(conn, url);
    assert_eq!(status, 200, "GET {url} -> {value}");
    value
}

#[test]
fn only_local_hosts_are_served() {
    let conn = seeded_db();
    for host in [None, Some("example.com"), Some("localhost.evil.com:7979")] {
        assert_eq!(
            get_from(&conn, host, "GET", "/api/patches").0,
            403,
            "{host:?}"
        );
    }
    for host in ["localhost", "127.0.0.1:7979", "[::1]:7979"] {
        assert_eq!(
            get_from(&conn, Some(host), "GET", "/api/patches").0,
            200,
            "{host}"
        );
    }
}

#[test]
fn only_get_requests_are_accepted() {
    let conn = seeded_db();
    let (status, _, _) = get_from(&conn, Some("localhost"), "POST", "/api/patches");
    assert_eq!(status, 405);
}

#[test]
fn the_root_serves_the_dashboard() {
    let conn = seeded_db();
    let (status, content_type, body) = get_from(&conn, Some("localhost"), "GET", "/");
    assert_eq!(status, 200);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains("<title>Meridian</title>"));
}

#[test]
fn patches_are_listed_with_their_counts() {
    let conn = seeded_db();
    let patches = ok(&conn, "/api/patches");
    assert_eq!(patches[0]["patch"], "16.1");
    assert_eq!(patches[1]["matches"], 1080);
    assert_eq!(patches[1]["balance_changes"], 1);
}

#[test]
fn the_report_shows_the_change_and_the_evidence() {
    let conn = seeded_db();
    let report = ok(&conn, "/api/patches/16.2/report?region=na1");
    assert_eq!(report["previous"], "16.1");
    assert_eq!(report["champions_changed"], 1);
    assert_eq!(report["largest_gain"]["champion"], "Jinx");
    let jinx = &report["impacts"][0];
    assert_eq!(jinx["kind"], "buff");
    assert_eq!(jinx["moved_as_intended"], true);
    assert!((jinx["win_rate_delta"].as_f64().unwrap() - 0.06).abs() < 1e-9);
    assert!(jinx["test"]["p_value"].as_f64().unwrap() < 0.05);
    assert_eq!(jinx["after"]["win_rate"], 0.55);
}

#[test]
fn the_report_is_also_available_as_csv() {
    let conn = seeded_db();
    let (status, content_type, body) = get_from(
        &conn,
        Some("localhost"),
        "GET",
        "/api/patches/16.2/report.csv?region=na1",
    );
    assert_eq!(status, 200);
    assert!(content_type.starts_with("text/csv"));
    assert!(body.starts_with("champion,changed,kind,"));
    assert!(body.contains("\nJinx,true,buff,1000,1000,"));
}

#[test]
fn filters_are_validated() {
    let conn = seeded_db();
    let (status, body) = get(&conn, "/api/patches/16.2/report?tier=wood");
    assert_eq!(status, 400);
    assert_eq!(body["error"], "unknown tier 'wood'");
    assert_eq!(get(&conn, "/api/patches/16.2/report?role=carry").0, 400);
    // A blank filter, as an unfilled form field sends, means no filter.
    assert_eq!(get(&conn, "/api/patches/16.2/report?region=&tier=").0, 200);
}

#[test]
fn missing_data_is_a_404_and_bad_patches_a_400() {
    let conn = seeded_db();
    assert_eq!(get(&conn, "/api/patches/16.1/report").0, 404);
    assert_eq!(get(&conn, "/api/patches/99.9/report").0, 404);
    assert_eq!(get(&conn, "/api/patches/latest/report").0, 400);
    assert_eq!(get(&conn, "/api/champions/Nobody/history").0, 404);
    assert_eq!(get(&conn, "/api/nothing").0, 404);
}

#[test]
fn champion_history_accepts_encoded_names() {
    let conn = seeded_db();
    let history = ok(&conn, "/api/champions/Kai%27Sa/history");
    assert_eq!(history[0]["patch"], "16.2");
    assert_eq!(history[0]["stats"]["games"], 80);
    let jinx = ok(&conn, "/api/champions/jinx/history");
    assert_eq!(jinx.as_array().unwrap().len(), 2);
    assert_eq!(jinx[1]["changes"][0]["attribute"], "Damage");
}

#[test]
fn rankings_list_champions_by_win_rate() {
    let conn = seeded_db();
    let ranked = ok(&conn, "/api/patches/16.2/rankings");
    assert_eq!(ranked[0]["champion"], "Jinx");
    assert_eq!(ranked[1]["champion"], "Kai'Sa");
}

#[test]
fn meta_endpoints_summarize_across_patches() {
    let conn = seeded_db();
    let shifts = ok(&conn, "/api/meta/shifts");
    assert_eq!(shifts[0]["patch"], "16.2");
    let effectiveness = ok(&conn, "/api/meta/effectiveness");
    assert_eq!(effectiveness[0]["kind"], "buff");
    assert_eq!(effectiveness[0]["in_intended_direction"], 1);
}
