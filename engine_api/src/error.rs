use core_types::{DomainError, Patch};
use std::fmt;

/// Errors returned by the storage engine.
#[derive(Debug)]
pub enum EngineError {
    Domain(DomainError),
    /// No matches are stored for this patch (within the filter, if one was given).
    PatchNotFound(Patch),
    /// A comparison needs an earlier patch with matches, and there is none.
    NoPreviousPatch(Patch),
    /// Nothing is stored for a champion with this name.
    ChampionNotFound(String),
    /// The database was created by a newer version of Meridian.
    UnsupportedSchemaVersion(i64),
    /// Stored data that cannot be interpreted, e.g. a balance change with an unknown kind.
    Corrupt(String),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Domain(e) => write!(f, "{e}"),
            EngineError::PatchNotFound(patch) => write!(f, "no matches found for patch {patch}"),
            EngineError::NoPreviousPatch(patch) => write!(
                f,
                "no patch with matches comes before {patch}, so there is nothing to compare it with"
            ),
            EngineError::ChampionNotFound(name) => write!(f, "champion '{name}' not found"),
            EngineError::UnsupportedSchemaVersion(version) => write!(
                f,
                "database schema version {version} is newer than this build supports; upgrade meridian"
            ),
            EngineError::Corrupt(msg) => write!(f, "corrupt data: {msg}"),
            EngineError::Sqlite(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::Domain(e) => Some(e),
            EngineError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DomainError> for EngineError {
    fn from(e: DomainError) -> Self {
        EngineError::Domain(e)
    }
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        EngineError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;
