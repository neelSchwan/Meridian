use serde::Serialize;
use std::fmt;
use std::str::FromStr;

use crate::DomainError;

/// Whether a change makes a champion stronger or weaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Buff,
    Nerf,
    /// Neither direction could be determined, or a champion's changes pull both ways.
    Adjustment,
}

impl ChangeKind {
    /// The lower-case name used in the database and in output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeKind::Buff => "buff",
            ChangeKind::Nerf => "nerf",
            ChangeKind::Adjustment => "adjustment",
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChangeKind {
    type Err = DomainError;

    fn from_str(text: &str) -> Result<ChangeKind, DomainError> {
        match text {
            "buff" => Ok(ChangeKind::Buff),
            "nerf" => Ok(ChangeKind::Nerf),
            "adjustment" => Ok(ChangeKind::Adjustment),
            _ => Err(DomainError::UnknownChangeKind(text.to_string())),
        }
    }
}

/// One line of a champion's patch notes: an attribute going from an old value to a new one.
/// Values keep the notes' own text, because they range from `80` to `25/50/75/100% (levels 1/6/11/16)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BalanceChange {
    pub champion: String,
    /// The ability heading the line appeared under, such as `Q - Orb of Deception` or `Base Stats`.
    pub ability: String,
    pub attribute: String,
    pub old_value: String,
    pub new_value: String,
    pub kind: ChangeKind,
}

/// Attributes where a bigger number is a weaker champion.
const LOWER_IS_BETTER: [&str; 6] = [
    "cooldown",
    "recharge",
    "cost",
    "cast time",
    "delay",
    "windup",
];

/// Decides whether an attribute change is a buff or a nerf.
///
/// This compares the first number in each value, which for a per-level list like `80/90/100` is
/// the rank-one value, and reads the direction from the attribute name. Values that begin with
/// words rather than a number, such as a reworked passive, are not compared and count as an
/// adjustment.
pub fn classify_change(attribute: &str, old_value: &str, new_value: &str) -> ChangeKind {
    let (Some(old), Some(new)) = (first_number(old_value), first_number(new_value)) else {
        return ChangeKind::Adjustment;
    };
    if old == new {
        return ChangeKind::Adjustment;
    }
    let name = attribute.to_lowercase();
    let lower_is_better = LOWER_IS_BETTER.iter().any(|word| name.contains(word));
    if (new > old) != lower_is_better {
        ChangeKind::Buff
    } else {
        ChangeKind::Nerf
    }
}

/// The number a value starts with, ignoring leading symbols such as `+` and `(`.
fn first_number(text: &str) -> Option<f64> {
    let start = text.find(char::is_alphanumeric)?;
    let number: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok()
}

/// The overall direction of a champion's changes in one patch: a buff or nerf only when every
/// directional change agrees. `None` when there are no changes at all.
pub fn overall_kind(kinds: impl IntoIterator<Item = ChangeKind>) -> Option<ChangeKind> {
    let (mut any, mut buffs, mut nerfs) = (false, false, false);
    for kind in kinds {
        any = true;
        buffs |= kind == ChangeKind::Buff;
        nerfs |= kind == ChangeKind::Nerf;
    }
    match (any, buffs, nerfs) {
        (false, ..) => None,
        (true, true, false) => Some(ChangeKind::Buff),
        (true, false, true) => Some(ChangeKind::Nerf),
        _ => Some(ChangeKind::Adjustment),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ChangeKind::*;

    #[test]
    fn a_higher_damage_number_is_a_buff() {
        assert_eq!(classify_change("Damage", "80", "85"), Buff);
        assert_eq!(
            classify_change("Damage", "60 / 100 / 140 (+ 40% AP)", "70 / 110 / 150"),
            Buff
        );
    }

    #[test]
    fn a_higher_cooldown_or_cost_is_a_nerf() {
        assert_eq!(classify_change("Cooldown", "12/11/10", "13/12/11"), Nerf);
        assert_eq!(classify_change("Mana Cost", "80", "100"), Nerf);
        assert_eq!(
            classify_change("Recharge Rate", "10 / 9 / 8", "12 / 10.5 / 9"),
            Nerf
        );
    }

    #[test]
    fn a_lower_cooldown_is_a_buff() {
        assert_eq!(
            classify_change("Cooldown", "12/11/10/9/8", "11/10/9/8/7 seconds"),
            Buff
        );
    }

    #[test]
    fn leading_symbols_are_ignored_when_reading_a_value() {
        assert_eq!(
            classify_change("AP Ratio", "+ 40 / 45 / 50% AP", "+ 30 / 40 / 60% AP"),
            Nerf
        );
        assert_eq!(
            classify_change("Damage", "(+ 19/22.5% AD)", "(+ 25/30% AD)"),
            Buff
        );
    }

    #[test]
    fn decimals_are_compared_as_numbers() {
        assert_eq!(classify_change("Attack Speed Growth", "2.25%", "2%"), Nerf);
        assert_eq!(classify_change("Attack Speed Growth", "2%", "2.25%"), Buff);
    }

    #[test]
    fn changes_without_comparable_numbers_are_adjustments() {
        assert_eq!(
            classify_change("Passive Rework", "Gain Movement Speed", "Grants a shield"),
            Adjustment
        );
        assert_eq!(classify_change("Damage", "80", "80 (+ 10% AP)"), Adjustment);
        assert_eq!(
            classify_change("Passive", "Gain 4-72 Movement Speed", "Grants 6-40% more"),
            Adjustment
        );
    }

    #[test]
    fn overall_kind_needs_the_directional_changes_to_agree() {
        assert_eq!(overall_kind([]), None);
        assert_eq!(overall_kind([Buff, Buff, Adjustment]), Some(Buff));
        assert_eq!(overall_kind([Nerf]), Some(Nerf));
        assert_eq!(overall_kind([Buff, Nerf]), Some(Adjustment));
        assert_eq!(overall_kind([Adjustment]), Some(Adjustment));
    }
}
