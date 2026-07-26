//! `<root>/<slot>.ron`.

use std::io;
use std::path::{Path, PathBuf};

use super::{is_slot, Store};

/// One directory of `.ron` files.
///
/// The crate does not guess where that directory is. A save location is a
/// platform question with a different right answer per OS and per storefront,
/// and a shared crate that guessed would be wrong for somebody — so the game
/// passes the path it has already decided on.
#[derive(Clone, Debug)]
pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    pub fn new(root: impl AsRef<Path>) -> FileStore {
        FileStore {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn path(&self, slot: &str) -> io::Result<PathBuf> {
        if !is_slot(slot) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{slot}` is not a slot name"),
            ));
        }
        Ok(self.root.join(format!("{slot}.ron")))
    }
}

impl Store for FileStore {
    type Error = io::Error;

    fn read(&self, slot: &str) -> Result<Option<String>, Self::Error> {
        match std::fs::read_to_string(self.path(slot)?) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Written to a temporary file and renamed over the target.
    ///
    /// A save is most likely to be interrupted exactly when it is being
    /// written — the player quit, the battery went, the tab closed — and a
    /// half-written save is worse than no save, because it looks loadable.
    fn write(&self, slot: &str, contents: &str) -> Result<(), Self::Error> {
        let path = self.path(slot)?;
        std::fs::create_dir_all(&self.root)?;
        let staging = path.with_extension("ron.writing");
        std::fs::write(&staging, contents)?;
        std::fs::rename(&staging, &path)
    }

    fn remove(&self, slot: &str) -> Result<(), Self::Error> {
        match std::fs::remove_file(self.path(slot)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn slots(&self) -> Result<Vec<String>, Self::Error> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            // No directory yet is no saves yet, which is a first run.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut slots = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ron") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if is_slot(stem) {
                        slots.push(stem.to_owned());
                    }
                }
            }
        }
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vellum-save-{}-{name}-{}",
            std::process::id(),
            name.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_slot_round_trips_and_a_missing_one_is_not_an_error() {
        let store = FileStore::new(scratch("round-trip"));
        assert_eq!(store.read("autosave").unwrap(), None);
        store.write("autosave", "(a: 1)").unwrap();
        assert_eq!(store.read("autosave").unwrap().as_deref(), Some("(a: 1)"));
        assert_eq!(store.slots().unwrap(), ["autosave"]);
        store.remove("autosave").unwrap();
        assert_eq!(store.read("autosave").unwrap(), None);
        // Removing what is not there is not a failure worth propagating.
        store.remove("autosave").unwrap();
    }

    #[test]
    fn a_slot_name_that_walks_upward_is_refused_before_it_touches_the_disk() {
        let store = FileStore::new(scratch("traversal"));
        assert!(store.write("../escape", "(a: 1)").is_err());
        assert!(store.read("../escape").is_err());
    }

    /// The staging file must not be mistaken for a save.
    #[test]
    fn an_interrupted_write_leaves_no_loadable_slot() {
        let root = scratch("staging");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("autosave.ron.writing"), "(hal").unwrap();
        let store = FileStore::new(&root);
        assert_eq!(store.slots().unwrap(), Vec::<String>::new());
        assert_eq!(store.read("autosave").unwrap(), None);
    }
}
