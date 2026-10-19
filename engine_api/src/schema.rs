use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{EngineError, Result};

/// Bump this and add a step to `migrate` whenever the schema changes.
pub const SCHEMA_VERSION: i64 = 1;

/// Matches are the source of truth: every statistic is a query over `participants` and `bans`
/// joined to `matches`, so nothing is stored that could drift out of date.
const SCHEMA_V1: &str = "
    CREATE TABLE patches (
        id    INTEGER PRIMARY KEY,
        major INTEGER NOT NULL,
        minor INTEGER NOT NULL,
        UNIQUE (major, minor)
    );

    -- `key` is core_types::champion_key, which joins Riot's match data to its patch notes.
    CREATE TABLE champions (
        id   INTEGER PRIMARY KEY,
        key  TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL
    );

    -- `tier` is the tier of the player the match was found through, not of every player in it.
    CREATE TABLE matches (
        id            INTEGER PRIMARY KEY,
        riot_match_id TEXT NOT NULL UNIQUE,
        patch_id      INTEGER NOT NULL REFERENCES patches(id),
        region        TEXT NOT NULL,
        tier          TEXT NOT NULL CHECK (tier IN (
                          'IRON', 'BRONZE', 'SILVER', 'GOLD', 'PLATINUM', 'EMERALD',
                          'DIAMOND', 'MASTER', 'GRANDMASTER', 'CHALLENGER')),
        started_at    INTEGER NOT NULL,
        duration_secs INTEGER NOT NULL
    );

    -- A ranked draft never repeats a champion, hence the primary key.
    CREATE TABLE participants (
        match_id    INTEGER NOT NULL REFERENCES matches(id),
        champion_id INTEGER NOT NULL REFERENCES champions(id),
        role        TEXT CHECK (role IS NULL OR role IN ('top', 'jungle', 'middle', 'bottom', 'support')),
        won         INTEGER NOT NULL CHECK (won IN (0, 1)),
        PRIMARY KEY (match_id, champion_id)
    );

    CREATE TABLE bans (
        match_id    INTEGER NOT NULL REFERENCES matches(id),
        champion_id INTEGER NOT NULL REFERENCES champions(id),
        PRIMARY KEY (match_id, champion_id)
    );

    CREATE TABLE balance_changes (
        id          INTEGER PRIMARY KEY,
        patch_id    INTEGER NOT NULL REFERENCES patches(id),
        champion_id INTEGER NOT NULL REFERENCES champions(id),
        ability     TEXT NOT NULL,
        attribute   TEXT NOT NULL,
        old_value   TEXT NOT NULL,
        new_value   TEXT NOT NULL,
        kind        TEXT NOT NULL CHECK (kind IN ('buff', 'nerf', 'adjustment'))
    );

    CREATE INDEX idx_matches_patch ON matches(patch_id);
    CREATE INDEX idx_participants_champion ON participants(champion_id);
    CREATE INDEX idx_bans_champion ON bans(champion_id);
    CREATE INDEX idx_balance_changes_patch ON balance_changes(patch_id);
";

fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Brings the database up to `SCHEMA_VERSION`, creating it if empty.
/// Refuses databases written by a newer version of Meridian rather than guessing at their schema.
pub fn migrate(conn: &Connection) -> Result<()> {
    // Common case: already current, so no write lock is needed.
    if schema_version(conn)? == SCHEMA_VERSION {
        return Ok(());
    }

    // Take the write lock, then read the version again: another process starting at the same
    // time may have migrated the database while this one was waiting.
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let version = schema_version(&tx)?;
    if version > SCHEMA_VERSION {
        return Err(EngineError::UnsupportedSchemaVersion(version));
    }
    if version < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}
