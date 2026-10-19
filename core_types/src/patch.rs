use serde::{Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use crate::DomainError;

/// A game patch as Riot's match API numbers it, such as `16.1`. Patches sort oldest to newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Patch {
    pub major: u32,
    pub minor: u32,
}

impl Patch {
    /// The label Riot uses for this patch's notes, such as `26.1` for game version `16.1`.
    ///
    /// Since 2025 the notes are numbered by year while the game still reports its own major
    /// number, which is ten lower. Earlier patches used one number for both.
    pub fn notes_label(&self) -> String {
        let major = if self.major >= 15 {
            self.major + 10
        } else {
            self.major
        };
        format!("{major}.{}", self.minor)
    }
}

impl fmt::Display for Patch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Parses `16.1` or a full game version such as `16.1.724.5821`; components after the second
/// are ignored.
impl FromStr for Patch {
    type Err = DomainError;

    fn from_str(text: &str) -> Result<Patch, DomainError> {
        let invalid = || DomainError::InvalidPatch(text.to_string());
        let mut parts = text.trim().split('.');
        let major = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or_else(invalid)?;
        let minor = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or_else(invalid)?;
        Ok(Patch { major, minor })
    }
}

impl Serialize for Patch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_full_versions() {
        let short: Patch = "16.1".parse().unwrap();
        let full: Patch = "16.1.724.5821".parse().unwrap();
        assert_eq!(short, full);
        assert_eq!(short.to_string(), "16.1");
    }

    #[test]
    fn rejects_text_that_is_not_a_patch() {
        for text in ["", "16", "x.y", "16.", "-1.2"] {
            assert!(text.parse::<Patch>().is_err(), "{text}");
        }
    }

    #[test]
    fn orders_numerically_not_alphabetically() {
        let nine: Patch = "16.9".parse().unwrap();
        let ten: Patch = "16.10".parse().unwrap();
        assert!(nine < ten);
    }

    #[test]
    fn notes_label_shifts_the_major_number_from_2025_on() {
        assert_eq!("16.1".parse::<Patch>().unwrap().notes_label(), "26.1");
        assert_eq!("15.20".parse::<Patch>().unwrap().notes_label(), "25.20");
        assert_eq!("14.24".parse::<Patch>().unwrap().notes_label(), "14.24");
    }
}
