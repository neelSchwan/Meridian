//! Reads champion balance changes out of Riot's patch-notes HTML.
//!
//! Riot publishes no structured data for balance changes, so this works on the page itself. It
//! relies on as little markup as possible, so cosmetic redesigns don't break it: a champion's
//! section is a `patch-change-block` whose `change-title` links to a `/champions/` page; ability
//! names are `h4` headings; and each change is a list item of the form `Attribute: old ⇒ new`.
//! Arena, Doom Bots and other mode-specific sections use other markup and are ignored, because
//! they are not Summoner's Rift balance.

use core_types::{BalanceChange, classify_change};
use scraper::{ElementRef, Html, Selector};

/// The arrow Riot puts between the old and new value, and spellings of it that a rewrite of the
/// page might plausibly use instead.
const ARROWS: [&str; 3] = ["⇒", "=>", "→"];

/// The heading assumed for changes listed before any ability heading.
const GENERAL: &str = "General";

/// Extracts every champion change from a patch-notes page. The result is empty when the page has
/// no champion sections, which usually means the layout changed and the parser needs updating.
pub fn parse_patch_notes(html: &str) -> Vec<BalanceChange> {
    let document = Html::parse_document(html);
    let blocks = selector(".patch-change-block");
    document
        .select(&blocks)
        .filter_map(|block| champion_name(block).map(|name| block_changes(block, &name)))
        .flatten()
        .collect()
}

fn selector(css: &str) -> Selector {
    // The selectors are constants in this file, so a failure here is a bug caught by the tests.
    Selector::parse(css).expect("valid selector")
}

/// The champion a block is about, or `None` for blocks about items, systems and the like.
fn champion_name(block: ElementRef) -> Option<String> {
    let title = block.select(&selector("h3.change-title")).next()?;
    let links_to_champion = title.select(&selector("a")).any(|link| {
        link.value()
            .attr("href")
            .is_some_and(|href| href.contains("/champions/"))
    });
    links_to_champion.then(|| text_of(title))
}

fn block_changes(block: ElementRef, champion: &str) -> Vec<BalanceChange> {
    let mut ability = GENERAL.to_string();
    let mut changes = Vec::new();
    for element in block.descendants().filter_map(ElementRef::wrap) {
        match element.value().name() {
            "h4" => ability = text_of(element),
            "li" => {
                if let Some((attribute, old_value, new_value)) =
                    split_change(&text_without_sublists(element))
                {
                    let kind = classify_change(&attribute, &old_value, &new_value);
                    changes.push(BalanceChange {
                        champion: champion.to_string(),
                        ability: ability.clone(),
                        attribute,
                        old_value,
                        new_value,
                        kind,
                    });
                }
            }
            _ => {}
        }
    }
    changes
}

/// The visible text of an element with all runs of whitespace (including non-breaking spaces)
/// collapsed to single spaces.
fn text_of(element: ElementRef) -> String {
    normalize(&element.text().collect::<Vec<_>>().join(" "))
}

/// A list item's own text, leaving out any sub-list, whose items are visited on their own.
fn text_without_sublists(item: ElementRef) -> String {
    let parts: Vec<String> = item
        .children()
        .filter_map(|child| match ElementRef::wrap(child) {
            Some(element) if matches!(element.value().name(), "ul" | "ol") => None,
            Some(element) => Some(text_of(element)),
            None => child.value().as_text().map(|text| text.to_string()),
        })
        .collect();
    normalize(&parts.join(" "))
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Splits `Attribute: old ⇒ new`. Lines without an arrow, or with an arrow before any colon
/// (such as a sentence ending in `(1.75x ⇒ 2.0x)`), are not attribute changes.
fn split_change(line: &str) -> Option<(String, String, String)> {
    let (attribute, values) = line.split_once(':')?;
    let (old_value, new_value) = ARROWS.iter().find_map(|arrow| values.split_once(arrow))?;
    let (attribute, old_value, new_value) = (attribute.trim(), old_value.trim(), new_value.trim());
    if ARROWS.iter().any(|arrow| attribute.contains(arrow))
        || old_value.is_empty()
        || new_value.is_empty()
    {
        return None;
    }
    Some((
        attribute.to_string(),
        old_value.to_string(),
        new_value.to_string(),
    ))
}
