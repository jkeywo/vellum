//! Three version dimensions, and a gate that names which one moved.

use core::fmt;

use serde::{Deserialize, Serialize};

/// What a stored record was written against.
///
/// Three numbers rather than one, because they invalidate different things and
/// a player asking "why can't I load this?" deserves a different answer in
/// each case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    /// The bytes. Bumped by hand when a field is restructured or removed, and
    /// the only dimension [`Progress`](crate::Progress) cares about — it is
    /// the one a migration can do anything about.
    pub format: u32,
    /// The simulation. Bumped by hand when the rules change, however
    /// slightly. A string rather than a number because "0.4-pre" says more in
    /// a bug report than "7", and nothing compares them for order.
    pub rules: String,
    /// The authored data, **computed** rather than remembered.
    ///
    /// A digest, so nobody has to bump it. Two games learned this the same
    /// way: a content version a human maintains is a content version that is
    /// wrong the first time someone edits a data file in a hurry.
    pub content: u64,
}

impl Versions {
    pub fn new(format: u32, rules: impl Into<String>, content: u64) -> Versions {
        Versions {
            format,
            rules: rules.into(),
            content,
        }
    }

    /// Whether a record written against `self` can be trusted by a build
    /// running `current`, and if not, which dimension moved.
    ///
    /// Checked in the order a reader cares about: unreadable bytes first,
    /// then a different simulation, then different data.
    pub fn check(&self, current: &Versions) -> Result<(), Moved> {
        if self.format != current.format {
            return Err(Moved::Format {
                stored: self.format,
                current: current.format,
            });
        }
        if self.rules != current.rules {
            return Err(Moved::Rules {
                stored: self.rules.clone(),
                current: current.rules.clone(),
            });
        }
        if self.content != current.content {
            return Err(Moved::Content {
                stored: self.content,
                current: current.content,
            });
        }
        Ok(())
    }

    /// One byte folding all three, for a medium that cannot afford three
    /// fields — a share code someone pastes into a chat window.
    ///
    /// A paste cannot carry a diagnosis anyway: the reader either reproduces
    /// the run or does not. Anything that needs to *explain* the refusal
    /// should carry the whole [`Versions`] and call [`check`](Self::check).
    pub fn compatibility_byte(&self) -> u8 {
        let mut folded = vellum_digest::fnv1a(&self.format.to_le_bytes());
        folded = vellum_digest::fold_digest(folded, vellum_digest::fnv1a(self.rules.as_bytes()));
        folded = vellum_digest::fold_digest(folded, self.content);
        (folded & 0xff) as u8
    }
}

/// Which version dimension moved, and to what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moved {
    Format { stored: u32, current: u32 },
    Rules { stored: String, current: String },
    Content { stored: u64, current: u64 },
}

impl fmt::Display for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Moved::Format { stored, current } => write!(
                f,
                "save format {stored} was written by a different build; this one reads {current}"
            ),
            Moved::Rules { stored, current } => write!(
                f,
                "recorded under rules `{stored}`; this build simulates `{current}`"
            ),
            Moved::Content { stored, current } => write!(
                f,
                "the authored data has changed since this was recorded \
                 ({stored:016x} then, {current:016x} now)"
            ),
        }
    }
}

impl std::error::Error for Moved {}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions() -> Versions {
        Versions::new(3, "0.4", 0xabcd)
    }

    #[test]
    fn matching_versions_pass() {
        assert_eq!(versions().check(&versions()), Ok(()));
    }

    /// The point of three dimensions: the refusal says which conversation to
    /// have. "The rules changed" and "the data changed" are not the same
    /// message to a player, or to whoever is debugging it.
    #[test]
    fn each_dimension_is_named_separately() {
        let mut current = versions();
        current.format = 4;
        assert!(matches!(
            versions().check(&current),
            Err(Moved::Format { .. })
        ));

        let mut current = versions();
        current.rules = "0.5".into();
        assert!(matches!(
            versions().check(&current),
            Err(Moved::Rules { .. })
        ));

        let mut current = versions();
        current.content = 0x1234;
        assert!(matches!(
            versions().check(&current),
            Err(Moved::Content { .. })
        ));
    }

    /// Unreadable bytes are reported before a rules difference, because a
    /// reader that cannot parse the record never learns the rules anyway.
    #[test]
    fn format_is_reported_before_the_others() {
        let current = Versions::new(4, "0.5", 0x1234);
        assert!(matches!(
            versions().check(&current),
            Err(Moved::Format { .. })
        ));
    }

    #[test]
    fn the_compatibility_byte_moves_with_every_dimension() {
        let base = versions().compatibility_byte();
        let mut moved = versions();
        moved.rules = "0.5".into();
        assert_ne!(base, moved.compatibility_byte());
        let mut moved = versions();
        moved.content = 0x1234;
        assert_ne!(base, moved.compatibility_byte());
    }
}
