//! Where bytes live, and the two places the fleet actually puts them.
//!
//! The trait is unconditional and dependency-free; the backends are features.
//! A game with its own storage — a console SDK, an account server — implements
//! [`Store`] and pays for nothing here. The five games that just want a file
//! natively and `localStorage` on the web get both without writing either.

use core::fmt;

/// Somewhere named slots of text can be kept.
///
/// Text rather than bytes, and RON rather than anything denser: every stored
/// record in this fleet is small, and a save a human can open in an editor is
/// a save a human can diagnose. `localStorage` is a string store anyway.
pub trait Store {
    type Error: fmt::Display;

    /// The slot's contents, or `None` if nothing is stored there. A missing
    /// slot is not an error — a first run has no save.
    fn read(&self, slot: &str) -> Result<Option<String>, Self::Error>;

    /// Replace the slot's contents.
    fn write(&self, slot: &str, contents: &str) -> Result<(), Self::Error>;

    /// Remove the slot. Removing a slot that does not exist succeeds.
    fn remove(&self, slot: &str) -> Result<(), Self::Error>;

    /// Every slot this store holds, for a load menu. Unordered.
    fn slots(&self) -> Result<Vec<String>, Self::Error>;
}

/// A store that keeps nothing, for a test whose subject is not storage.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoStore;

impl Store for NoStore {
    type Error = core::convert::Infallible;

    fn read(&self, _slot: &str) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn write(&self, _slot: &str, _contents: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn remove(&self, _slot: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn slots(&self) -> Result<Vec<String>, Self::Error> {
        Ok(Vec::new())
    }
}

/// Whether a slot name is safe to use as a filename or a storage key.
///
/// Deliberately narrow, and checked by the backends rather than trusted:
/// a slot name reaches a path, and `../../.ssh/id_rsa` is a slot name if
/// nobody looks.
pub fn is_slot(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= 64
        && candidate
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[cfg(feature = "backend-fs")]
mod fs;
#[cfg(feature = "backend-fs")]
pub use fs::FileStore;

#[cfg(all(feature = "backend-web", target_arch = "wasm32"))]
mod web;
#[cfg(all(feature = "backend-web", target_arch = "wasm32"))]
pub use web::LocalStorage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_name_cannot_walk_out_of_its_directory() {
        assert!(is_slot("autosave"));
        assert!(is_slot("slot-1"));
        assert!(!is_slot("../secrets"));
        assert!(!is_slot("a/b"));
        assert!(!is_slot(""));
        assert!(
            !is_slot("Autosave"),
            "case is part of the name on one of the two backends and not the other"
        );
    }
}
