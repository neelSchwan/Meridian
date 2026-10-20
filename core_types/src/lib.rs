//! Domain types and statistics for Meridian: patches, matches, balance changes, and the pure
//! calculations that compare one patch with another. No I/O, no database.

mod analysis;
mod change;
mod patch;
mod record;
mod stats;

pub use analysis::{
    ChampionStats, Effectiveness, HistoryPoint, Impact, MetaShift, PatchReport, effectiveness,
    impacts, meta_shift,
};
pub use change::{BalanceChange, ChangeKind, classify_change, overall_kind};
pub use patch::Patch;
pub use record::{Filter, MatchRecord, Participant, PatchSummary, Role, Tier, champion_key};
pub use stats::{
    ALPHA, MIN_GAMES, ProportionTest, trailing_mean, two_proportion_test, wilson_interval,
};

use std::fmt;

/// Domain-level errors: only things we can validate locally (no database lookups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidPatch(String),
    EmptyMatchId,
    EmptyRegion,
    NoParticipants(String),
    DuplicateChampion { match_id: String, champion: String },
    UnknownTier(String),
    UnknownRole(String),
    UnknownChangeKind(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomainError::InvalidPatch(s) => write!(
                f,
                "invalid patch '{s}': expected a version like 16.1 or 16.1.724.5821"
            ),
            DomainError::EmptyMatchId => write!(f, "match id must not be empty"),
            DomainError::EmptyRegion => write!(f, "region must not be empty"),
            DomainError::NoParticipants(id) => write!(f, "match {id} has no participants"),
            DomainError::DuplicateChampion { match_id, champion } => {
                write!(
                    f,
                    "match {match_id} lists champion {champion} more than once"
                )
            }
            DomainError::UnknownTier(s) => write!(f, "unknown tier '{s}'"),
            DomainError::UnknownRole(s) => write!(
                f,
                "unknown role '{s}': expected top, jungle, middle, bottom or support"
            ),
            DomainError::UnknownChangeKind(s) => write!(f, "unknown change kind '{s}'"),
        }
    }
}

impl std::error::Error for DomainError {}
