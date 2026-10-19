use core_types::ChangeKind;
use patch_notes::parse_patch_notes;

const REAL_EXCERPT: &str = include_str!("fixtures/patch_25_20_excerpt.html");

/// A champion block with the markup stripped to what the parser depends on.
fn champion_block(name: &str, body: &str) -> String {
    format!(
        r#"<div class="patch-change-block"><h3 class="change-title"><a href="/en-us/champions/x/">{name}</a></h3>{body}</div>"#
    )
}

#[test]
fn reads_champion_changes_from_a_real_page() {
    let changes = parse_patch_notes(REAL_EXCERPT);

    let champions: Vec<_> = changes.iter().map(|c| c.champion.as_str()).collect();
    assert_eq!(
        champions,
        ["Anivia", "Anivia", "Azir", "Azir", "Azir", "Azir"]
    );

    let cooldown = &changes[0];
    assert_eq!(cooldown.ability, "Q - Flash Frost");
    assert_eq!(cooldown.attribute, "Cooldown");
    assert_eq!(cooldown.old_value, "12/11/10/9/8");
    assert_eq!(cooldown.new_value, "11/10/9/8/7 seconds");
    assert_eq!(cooldown.kind, ChangeKind::Buff);
}

#[test]
fn ignores_system_and_arena_sections() {
    let changes = parse_patch_notes(REAL_EXCERPT);
    assert!(
        changes
            .iter()
            .all(|c| c.champion == "Anivia" || c.champion == "Azir")
    );
    assert!(!changes.iter().any(|c| c.attribute == "Top Lane Detection"));
}

#[test]
fn keeps_values_with_their_original_text() {
    let changes = parse_patch_notes(REAL_EXCERPT);
    let recharge = changes
        .iter()
        .find(|c| c.attribute == "Recharge Rate")
        .unwrap();
    assert_eq!(recharge.kind, ChangeKind::Nerf);
    let additional = changes
        .iter()
        .find(|c| c.attribute == "Damage to Additional Targets")
        .unwrap();
    assert_eq!(
        additional.new_value,
        "20% until level 8, then +8% per level (100% at 18)"
    );
}

#[test]
fn tolerates_missing_strong_tags_extra_whitespace_and_other_arrows() {
    let html = champion_block(
        "Ahri",
        "<h4>Q - Orb of Deception</h4><ul><li>\n  Damage:&nbsp;80   =>   85 </li></ul>",
    );
    let changes = parse_patch_notes(&html);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].attribute, "Damage");
    assert_eq!(changes[0].old_value, "80");
    assert_eq!(changes[0].new_value, "85");
}

#[test]
fn changes_before_any_ability_heading_are_general() {
    let html = champion_block("Ahri", "<ul><li>Base Armor: 18 ⇒ 20</li></ul>");
    assert_eq!(parse_patch_notes(&html)[0].ability, "General");
}

#[test]
fn skips_lines_that_are_not_attribute_changes() {
    let html = champion_block(
        "Ahri",
        "<ul><li>Tooltip now shows the damage</li>\
         <li>Still crits for 100% crit damage (1.75x ⇒ 2.0x)</li>\
         <li>Mana Cost: 80 ⇒ </li></ul>",
    );
    assert!(parse_patch_notes(&html).is_empty());
}

#[test]
fn blocks_whose_title_does_not_link_to_a_champion_are_skipped() {
    let html = r#"<div class="patch-change-block"><h3 class="change-title"><a href="/en-us/items/">Doran's Blade</a></h3>
        <ul><li>Cost: 450 ⇒ 400</li></ul></div>"#;
    assert!(parse_patch_notes(html).is_empty());
}

#[test]
fn a_page_without_champion_blocks_yields_nothing() {
    assert!(parse_patch_notes("<html><body><p>Not found</p></body></html>").is_empty());
    assert!(parse_patch_notes("").is_empty());
}

#[test]
fn nested_list_items_are_each_read_once() {
    let html = champion_block(
        "Ahri",
        "<ul><li>Damage: 80 ⇒ 85<ul><li>Bonus: 1 ⇒ 2</li></ul></li></ul>",
    );
    let attributes: Vec<_> = parse_patch_notes(&html)
        .into_iter()
        .map(|c| c.attribute)
        .collect();
    assert_eq!(attributes, ["Damage", "Bonus"]);
}
