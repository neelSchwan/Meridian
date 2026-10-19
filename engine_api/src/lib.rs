//! SQLite storage and analysis for Meridian.
//!
//! Matches are stored once and never changed. Every statistic is computed from them on demand,
//! so a report always reflects exactly the matches in the database. Balance changes parsed from
//! patch notes are the one derived table; importing a patch's notes again replaces its rows.

mod analysis;
mod error;
mod report;
mod schema;

use core_types::{
    BalanceChange, ChampionStats, ChangeKind, Filter, MatchRecord, Patch, PatchSummary,
    champion_key,
};
use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior};
use std::path::Path;

pub use analysis::{balance_effectiveness, champion_history, meta_shifts, patch_report, rankings};
pub use error::{EngineError, Result};
pub use report::{
    render_csv, render_effectiveness, render_history, render_markdown, render_patches,
    render_shifts, render_stats_table,
};
// Re-exported so frontends can hold a connection without depending on rusqlite directly.
pub use rusqlite::Connection;
pub use schema::SCHEMA_VERSION;

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// Opens (or creates) the database at `path` and brings its schema up to date.
pub fn open_db(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL lets the server read while an ingestion run is writing.
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    init_db(&conn)?;
    Ok(conn)
}

/// Creates all tables if they don't exist. Safe to call on every startup.
pub fn init_db(conn: &Connection) -> Result<()> {
    schema::migrate(conn)
}

/// Starts a write transaction that takes the write lock up front.
/// A deferred transaction that reads before writing fails immediately with "database is locked"
/// when another connection commits in between, and the busy timeout does not help.
fn write_transaction(conn: &Connection) -> Result<Transaction<'_>> {
    Ok(Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    )?)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Whether a match with this Riot id is already stored. Ingestion checks this before fetching a
/// match, so a run that was interrupted resumes where it stopped and nothing is downloaded twice.
pub fn has_match(conn: &Connection, match_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM matches WHERE riot_match_id = ?1)",
        [match_id],
        |row| row.get(0),
    )?)
}

/// Stores a match with its participants and bans in one transaction.
/// Returns `false`, changing nothing, when the match was already stored.
pub fn record_match(conn: &Connection, record: &MatchRecord) -> Result<bool> {
    record.validate()?;
    let tx = write_transaction(conn)?;
    if has_match(&tx, &record.match_id)? {
        return Ok(false);
    }
    let patch_id = upsert_patch(&tx, record.patch)?;
    tx.execute(
        "INSERT INTO matches (riot_match_id, patch_id, region, tier, started_at, duration_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (
            &record.match_id,
            patch_id,
            &record.region,
            record.tier.as_str(),
            record.started_at,
            record.duration_secs,
        ),
    )?;
    let match_row = tx.last_insert_rowid();
    for participant in &record.participants {
        let champion_id = upsert_champion(&tx, &participant.champion)?;
        tx.execute(
            "INSERT INTO participants (match_id, champion_id, role, won) VALUES (?1, ?2, ?3, ?4)",
            (
                match_row,
                champion_id,
                participant.role.map(|role| role.as_str()),
                participant.won,
            ),
        )?;
    }
    for ban in &record.bans {
        let champion_id = upsert_champion(&tx, ban)?;
        // Both teams could in principle ban the same champion; it still counts as one match.
        tx.execute(
            "INSERT OR IGNORE INTO bans (match_id, champion_id) VALUES (?1, ?2)",
            (match_row, champion_id),
        )?;
    }
    tx.commit()?;
    Ok(true)
}

/// Stores the balance changes parsed from a patch's notes, replacing any stored earlier for the
/// same patch so that re-importing after a parser fix does not duplicate rows.
pub fn replace_balance_changes(
    conn: &Connection,
    patch: Patch,
    changes: &[BalanceChange],
) -> Result<()> {
    let tx = write_transaction(conn)?;
    let patch_id = upsert_patch(&tx, patch)?;
    tx.execute(
        "DELETE FROM balance_changes WHERE patch_id = ?1",
        [patch_id],
    )?;
    for change in changes {
        let champion_id = upsert_champion(&tx, &change.champion)?;
        tx.execute(
            "INSERT INTO balance_changes
                 (patch_id, champion_id, ability, attribute, old_value, new_value, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                patch_id,
                champion_id,
                &change.ability,
                &change.attribute,
                &change.old_value,
                &change.new_value,
                change.kind.as_str(),
            ),
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn upsert_patch(tx: &Transaction, patch: Patch) -> Result<i64> {
    // The no-op update makes RETURNING yield the id of an existing row too.
    Ok(tx.query_row(
        "INSERT INTO patches (major, minor) VALUES (?1, ?2)
         ON CONFLICT (major, minor) DO UPDATE SET major = major
         RETURNING id",
        (patch.major, patch.minor),
        |row| row.get(0),
    )?)
}

/// Finds the champion with this name's key, adding it if new. The name stored is the first one
/// seen, so it is whichever source reached the database first.
fn upsert_champion(tx: &Transaction, name: &str) -> Result<i64> {
    Ok(tx.query_row(
        "INSERT INTO champions (key, name) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET key = key
         RETURNING id",
        (champion_key(name), name),
        |row| row.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Every patch that has matches or balance changes, oldest first.
pub fn list_patches(conn: &Connection) -> Result<Vec<PatchSummary>> {
    let mut stmt = conn.prepare(
        "SELECT p.major, p.minor,
                (SELECT COUNT(*) FROM matches WHERE patch_id = p.id),
                (SELECT COUNT(*) FROM balance_changes WHERE patch_id = p.id)
         FROM patches p ORDER BY p.major, p.minor",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PatchSummary {
            patch: Patch {
                major: row.get(0)?,
                minor: row.get(1)?,
            },
            matches: count(row, 2)?,
            balance_changes: count(row, 3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The balance changes recorded for a patch, in the order they appear in its notes.
pub fn balance_changes(conn: &Connection, patch: Patch) -> Result<Vec<BalanceChange>> {
    let mut stmt = conn.prepare(
        "SELECT c.name, b.ability, b.attribute, b.old_value, b.new_value, b.kind
         FROM balance_changes b
         JOIN patches p ON p.id = b.patch_id
         JOIN champions c ON c.id = b.champion_id
         WHERE p.major = ?1 AND p.minor = ?2
         ORDER BY b.id",
    )?;
    let rows = stmt.query_map((patch.major, patch.minor), BalanceChangeRow::from_row)?;
    rows.map(|row| row?.into_change()).collect()
}

/// Win, pick and ban counts for every champion in a patch, alphabetically. Only champions that
/// were played or banned appear. Errors if no matches are stored for the patch.
pub fn champion_stats(
    conn: &Connection,
    patch: Patch,
    filter: &Filter,
) -> Result<Vec<ChampionStats>> {
    stats_query(conn, patch, filter, None)
}

/// A champion's counts in one patch. Shared by `champion_stats` and the per-champion history,
/// which restricts the query to one champion instead of loading all of them.
fn stats_query(
    conn: &Connection,
    patch: Patch,
    filter: &Filter,
    champion_id: Option<i64>,
) -> Result<Vec<ChampionStats>> {
    let patch_id = patch_id(conn, patch)?.ok_or(EngineError::PatchNotFound(patch))?;
    let total_matches = count_matches(conn, patch_id, filter)?;
    if total_matches == 0 {
        return Err(EngineError::PatchNotFound(patch));
    }
    let mut stmt = conn.prepare(
        "SELECT c.name, SUM(u.games), SUM(u.wins), SUM(u.bans)
         FROM (
             SELECT p.champion_id, 1 AS games, p.won AS wins, 0 AS bans
             FROM participants p JOIN matches m ON m.id = p.match_id
             WHERE m.patch_id = :patch
               AND (:region IS NULL OR m.region = lower(:region))
               AND (:tier IS NULL OR m.tier = :tier)
               AND (:role IS NULL OR p.role = :role)
               AND (:champion IS NULL OR p.champion_id = :champion)
             UNION ALL
             SELECT b.champion_id, 0, 0, 1
             FROM bans b JOIN matches m ON m.id = b.match_id
             WHERE m.patch_id = :patch
               AND (:region IS NULL OR m.region = lower(:region))
               AND (:tier IS NULL OR m.tier = :tier)
               AND (:champion IS NULL OR b.champion_id = :champion)
         ) u
         JOIN champions c ON c.id = u.champion_id
         GROUP BY c.id
         ORDER BY c.name",
    )?;
    let rows = stmt.query_map(
        rusqlite::named_params! {
            ":patch": patch_id,
            ":region": filter.region,
            ":tier": filter.tier.map(|tier| tier.as_str()),
            ":role": filter.role.map(|role| role.as_str()),
            ":champion": champion_id,
        },
        ChampionStatsRow::from_row,
    )?;
    rows.map(|row| Ok(row?.into_stats(total_matches))).collect()
}

fn count_matches(conn: &Connection, patch_id: i64, filter: &Filter) -> Result<u64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM matches
         WHERE patch_id = :patch
           AND (:region IS NULL OR region = lower(:region))
           AND (:tier IS NULL OR tier = :tier)",
        rusqlite::named_params! {
            ":patch": patch_id,
            ":region": filter.region,
            ":tier": filter.tier.map(|tier| tier.as_str()),
        },
        |row| count(row, 0),
    )?)
}

fn patch_id(conn: &Connection, patch: Patch) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM patches WHERE major = ?1 AND minor = ?2",
            (patch.major, patch.minor),
            |row| row.get(0),
        )
        .optional()?)
}

/// The id of the champion whose name has this key, or `ChampionNotFound`.
fn find_champion(conn: &Connection, name: &str) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM champions WHERE key = ?1",
        [champion_key(name)],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| EngineError::ChampionNotFound(name.to_string()))
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

/// Reads a count. SQLite integers are signed, and a count is never negative.
fn count(row: &Row, index: usize) -> rusqlite::Result<u64> {
    Ok(row.get::<_, i64>(index)?.max(0) as u64)
}

struct ChampionStatsRow {
    champion: String,
    games: u64,
    wins: u64,
    bans: u64,
}

impl ChampionStatsRow {
    fn from_row(row: &Row) -> rusqlite::Result<ChampionStatsRow> {
        Ok(ChampionStatsRow {
            champion: row.get(0)?,
            games: count(row, 1)?,
            wins: count(row, 2)?,
            bans: count(row, 3)?,
        })
    }

    fn into_stats(self, total_matches: u64) -> ChampionStats {
        ChampionStats {
            champion: self.champion,
            games: self.games,
            wins: self.wins,
            bans: self.bans,
            total_matches,
        }
    }
}

struct BalanceChangeRow {
    champion: String,
    ability: String,
    attribute: String,
    old_value: String,
    new_value: String,
    kind: String,
}

impl BalanceChangeRow {
    fn from_row(row: &Row) -> rusqlite::Result<BalanceChangeRow> {
        Ok(BalanceChangeRow {
            champion: row.get(0)?,
            ability: row.get(1)?,
            attribute: row.get(2)?,
            old_value: row.get(3)?,
            new_value: row.get(4)?,
            kind: row.get(5)?,
        })
    }

    fn into_change(self) -> Result<BalanceChange> {
        let kind: ChangeKind = self
            .kind
            .parse()
            .map_err(|e| EngineError::Corrupt(format!("{e}")))?;
        Ok(BalanceChange {
            champion: self.champion,
            ability: self.ability,
            attribute: self.attribute,
            old_value: self.old_value,
            new_value: self.new_value,
            kind,
        })
    }
}
