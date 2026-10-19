//! Brings outside data into the Meridian database: ranked matches from the Riot API, and
//! balance changes from Riot's patch notes.
//!
//! Everything here is safe to run repeatedly, for example from cron. Matches already stored are
//! skipped before they are downloaded, and importing a patch's notes again replaces its changes.

mod convert;
mod notes;
mod riot;

pub use convert::to_record;
pub use notes::{fetch_patch_notes, import_notes};
pub use riot::{Sampling, Summary, ingest_matches};
