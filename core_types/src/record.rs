use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use crate::{DomainError, Patch};

/// The lane a participant played, as reported by Riot's `teamPosition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Top,
    Jungle,
    Middle,
    Bottom,
    Support,
}

impl Role {
    /// The lower-case name used in the database and in output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Top => "top",
            Role::Jungle => "jungle",
            Role::Middle => "middle",
            Role::Bottom => "bottom",
            Role::Support => "support",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = DomainError;

    fn from_str(text: &str) -> Result<Role, DomainError> {
        match text.to_ascii_lowercase().as_str() {
            "top" => Ok(Role::Top),
            "jungle" => Ok(Role::Jungle),
            "middle" | "mid" => Ok(Role::Middle),
            "bottom" | "bot" | "adc" => Ok(Role::Bottom),
            "support" | "utility" => Ok(Role::Support),
            _ => Err(DomainError::UnknownRole(text.to_string())),
        }
    }
}

/// A ranked tier. Matches are labelled with the tier of the player they were discovered through,
/// which approximates the skill level of everyone in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tier {
    Iron,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Emerald,
    Diamond,
    Master,
    Grandmaster,
    Challenger,
}

impl Tier {
    /// The upper-case name Riot's API uses, such as `DIAMOND`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Iron => "IRON",
            Tier::Bronze => "BRONZE",
            Tier::Silver => "SILVER",
            Tier::Gold => "GOLD",
            Tier::Platinum => "PLATINUM",
            Tier::Emerald => "EMERALD",
            Tier::Diamond => "DIAMOND",
            Tier::Master => "MASTER",
            Tier::Grandmaster => "GRANDMASTER",
            Tier::Challenger => "CHALLENGER",
        }
    }

    /// Master and above have no divisions and are listed through separate endpoints.
    pub fn is_apex(&self) -> bool {
        matches!(self, Tier::Master | Tier::Grandmaster | Tier::Challenger)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Tier {
    type Err = DomainError;

    fn from_str(text: &str) -> Result<Tier, DomainError> {
        match text.to_ascii_uppercase().as_str() {
            "IRON" => Ok(Tier::Iron),
            "BRONZE" => Ok(Tier::Bronze),
            "SILVER" => Ok(Tier::Silver),
            "GOLD" => Ok(Tier::Gold),
            "PLATINUM" => Ok(Tier::Platinum),
            "EMERALD" => Ok(Tier::Emerald),
            "DIAMOND" => Ok(Tier::Diamond),
            "MASTER" => Ok(Tier::Master),
            "GRANDMASTER" => Ok(Tier::Grandmaster),
            "CHALLENGER" => Ok(Tier::Challenger),
            _ => Err(DomainError::UnknownTier(text.to_string())),
        }
    }
}

/// Restricts a query to one slice of the match data. `None` means "all".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Filter {
    /// A platform such as `na1` or `euw1`.
    pub region: Option<String>,
    pub tier: Option<Tier>,
    /// Bans are not tied to a role, so they are counted whatever role is chosen.
    pub role: Option<Role>,
}

/// One champion's appearance in a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub champion: String,
    pub role: Option<Role>,
    pub won: bool,
}

/// Everything Meridian keeps about one ranked game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchRecord {
    /// Riot's id for the match, such as `NA1_5123456789`. Used to skip matches seen before.
    pub match_id: String,
    pub patch: Patch,
    pub region: String,
    pub tier: Tier,
    /// Unix time in seconds.
    pub started_at: i64,
    pub duration_secs: i64,
    pub participants: Vec<Participant>,
    /// Champions banned in champion select, from both teams.
    pub bans: Vec<String>,
}

impl MatchRecord {
    /// Checks the facts the statistics rely on: the match is identifiable and no champion is
    /// counted twice (a ranked draft never allows it, so a repeat means bad data).
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.match_id.trim().is_empty() {
            return Err(DomainError::EmptyMatchId);
        }
        if self.region.trim().is_empty() {
            return Err(DomainError::EmptyRegion);
        }
        if self.participants.is_empty() {
            return Err(DomainError::NoParticipants(self.match_id.clone()));
        }
        let mut seen = HashSet::new();
        for participant in &self.participants {
            if !seen.insert(champion_key(&participant.champion)) {
                return Err(DomainError::DuplicateChampion {
                    match_id: self.match_id.clone(),
                    champion: participant.champion.clone(),
                });
            }
        }
        Ok(())
    }
}

/// What is stored for one patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchSummary {
    pub patch: Patch,
    pub matches: u64,
    pub balance_changes: u64,
}

/// The identity of a champion across data sources: lower-case letters and digits only, so
/// "Kai'Sa", "KaiSa" and "kaisa" agree.
///
/// Three champions are named differently in Riot's match data than in patch notes, so they are
/// mapped to the match-data spelling.
pub fn champion_key(name: &str) -> String {
    let key: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    match key.as_str() {
        "wukong" => "monkeyking".to_string(),
        "nunuwillump" => "nunu".to_string(),
        "renataglasc" => "renata".to_string(),
        _ => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(champion: &str) -> Participant {
        Participant {
            champion: champion.to_string(),
            role: None,
            won: true,
        }
    }

    fn record(champions: &[&str]) -> MatchRecord {
        MatchRecord {
            match_id: "NA1_1".to_string(),
            patch: "16.1".parse().unwrap(),
            region: "na1".to_string(),
            tier: Tier::Gold,
            started_at: 0,
            duration_secs: 1800,
            participants: champions.iter().map(|c| participant(c)).collect(),
            bans: vec![],
        }
    }

    #[test]
    fn champion_key_ignores_punctuation_and_case() {
        assert_eq!(champion_key("Kai'Sa"), champion_key("KAISA"));
        assert_eq!(champion_key("Dr. Mundo"), "drmundo");
    }

    #[test]
    fn champion_key_joins_patch_note_names_to_match_data_names() {
        assert_eq!(champion_key("Wukong"), champion_key("MonkeyKing"));
        assert_eq!(champion_key("Nunu & Willump"), champion_key("Nunu"));
        assert_eq!(champion_key("Renata Glasc"), champion_key("Renata"));
    }

    #[test]
    fn a_repeated_champion_is_rejected() {
        let error = record(&["Ahri", "Jinx", "ahri"]).validate().unwrap_err();
        assert!(matches!(error, DomainError::DuplicateChampion { .. }));
    }

    #[test]
    fn a_match_without_participants_is_rejected() {
        let error = record(&[]).validate().unwrap_err();
        assert_eq!(error, DomainError::NoParticipants("NA1_1".to_string()));
    }

    #[test]
    fn tiers_and_roles_round_trip_through_text() {
        assert_eq!("diamond".parse::<Tier>().unwrap(), Tier::Diamond);
        assert_eq!(Tier::Diamond.to_string(), "DIAMOND");
        assert_eq!("UTILITY".parse::<Role>().unwrap(), Role::Support);
        assert!("plat".parse::<Tier>().is_err());
    }
}
