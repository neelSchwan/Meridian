use core_types::Patch;
use engine_api::{Connection, replace_balance_changes};
use patch_notes::parse_patch_notes;

use crate::BoxError;

const NOTES_URL: &str = "https://www.leagueoflegends.com/en-us/news/game-updates";

/// Downloads a patch's notes page. Riot has used two address styles over the years, so both are
/// tried.
pub async fn fetch_patch_notes(patch: Patch) -> Result<String, BoxError> {
    let label = patch.notes_label().replace('.', "-");
    let addresses = [
        format!("{NOTES_URL}/patch-{label}-notes/"),
        format!("{NOTES_URL}/league-of-legends-patch-{label}-notes/"),
    ];
    for address in &addresses {
        let response = riven::reqwest::get(address).await?;
        if response.status().is_success() {
            return Ok(response.text().await?);
        }
    }
    Err(format!(
        "no patch notes found for patch {patch} at {}",
        addresses.join(" or ")
    )
    .into())
}

/// Parses a notes page and stores its balance changes for `patch`, returning how many it found.
/// A page with no champion changes is an error rather than an empty import, because it almost
/// always means Riot changed the page layout.
pub fn import_notes(conn: &Connection, patch: Patch, html: &str) -> Result<usize, BoxError> {
    let changes = parse_patch_notes(html);
    if changes.is_empty() {
        return Err(format!(
            "found no champion changes in the notes for patch {patch}; \
             the page layout may have changed and the parser may need updating"
        )
        .into());
    }
    replace_balance_changes(conn, patch, &changes)?;
    Ok(changes.len())
}
