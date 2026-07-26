//! Durable state that outlives a run, and the only kind of save that migrates.

use core::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// One step of a migration chain: read a record written at `from`, hand back
/// one readable at `from + 1`.
///
/// It works on `ron::Value` rather than on either struct, because the old
/// struct no longer exists — that is what "the format changed" means. Only a
/// *restructured* or *removed* field needs a step; adding a field with a serde
/// default needs nothing, which is why most chains stay short.
pub struct Migration {
    pub from: u32,
    pub apply: fn(ron::Value) -> Result<ron::Value, String>,
}

impl fmt::Debug for Migration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Migration(v{} -> v{})", self.from, self.from + 1)
    }
}

/// Durable state: unlocks, totals, settings — whatever should survive a run
/// ending, a quit, or a browser refresh.
///
/// This is the half of "save" that migrates. A run refuses forever because a
/// migrated run would be a different run; progress is just the player's
/// accumulated history, and refusing to load it deletes something they earned.
pub trait Progress: Serialize + DeserializeOwned {
    /// Bumped by hand when a field is restructured or removed.
    const FORMAT: u32;

    /// Ordered `vN -> vN+1` steps, oldest first. A gap in the chain is a
    /// load failure that names the gap rather than a silent default.
    const MIGRATIONS: &'static [Migration] = &[];
}

/// What is actually stored: the payload, what it was written against, and a
/// hash of itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Record {
    format: u32,
    /// FNV-1a over the payload text. The corruption question — "does this
    /// record still match itself?" — kept separate from integrity (did the
    /// bytes survive a paste) and divergence (did a replay reproduce).
    hash: u64,
    payload: String,
}

/// Why stored progress would not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The envelope itself is not RON. Something other than this crate wrote
    /// the slot.
    Unreadable(String),
    /// The payload does not match its own hash: truncated, hand-edited, or
    /// half-written.
    Corrupt { stored: u64, computed: u64 },
    /// Written by a *newer* build. There is no forward migration and there
    /// never can be, so this is refused rather than guessed at.
    FromTheFuture { stored: u32, current: u32 },
    /// The chain cannot get from the stored version to the current one.
    NoMigration { from: u32 },
    /// A migration step ran and failed.
    MigrationFailed { from: u32, message: String },
    /// The payload parsed as RON but not as the game's type.
    NotTheType(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unreadable(message) => write!(f, "not a save record: {message}"),
            LoadError::Corrupt { stored, computed } => write!(
                f,
                "the save does not match its own checksum \
                 (recorded {stored:016x}, computed {computed:016x})"
            ),
            LoadError::FromTheFuture { stored, current } => write!(
                f,
                "this save is from a newer version (format {stored}; this build reads {current})"
            ),
            LoadError::NoMigration { from } => {
                write!(f, "nothing knows how to bring format {from} forward")
            }
            LoadError::MigrationFailed { from, message } => {
                write!(f, "migrating format {from} forward failed: {message}")
            }
            LoadError::NotTheType(message) => write!(f, "the save did not fit: {message}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Why progress would not save.
#[derive(Clone, Debug)]
pub enum SaveError<E> {
    /// The value would not serialize. A bug, not a condition.
    Unserializable(String),
    /// The store refused.
    Store(E),
}

impl<E: fmt::Display> fmt::Display for SaveError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Unserializable(message) => write!(f, "could not write the save: {message}"),
            SaveError::Store(error) => write!(f, "{error}"),
        }
    }
}

/// Serialize, hash, and hand the whole record to the store.
pub fn save<P: Progress, S: Store>(
    store: &S,
    slot: &str,
    value: &P,
) -> Result<(), SaveError<S::Error>> {
    let contents = encode(value)?;
    store.write(slot, &contents).map_err(SaveError::Store)
}

/// The stored text for `value`, for a game that owns its own writing.
pub fn encode<P: Progress, E>(value: &P) -> Result<String, SaveError<E>> {
    let payload =
        ron::to_string(value).map_err(|error| SaveError::Unserializable(error.to_string()))?;
    let record = Record {
        format: P::FORMAT,
        hash: vellum_digest::fnv1a(payload.as_bytes()),
        payload,
    };
    ron::ser::to_string_pretty(&record, ron::ser::PrettyConfig::default())
        .map_err(|error| SaveError::Unserializable(error.to_string()))
}

/// Read a slot, migrating it forward if it was written by an older build.
///
/// `Ok(None)` means the slot is empty, which is a first run rather than a
/// problem. A store failure is surfaced as `Err(Err(..))` so a caller can tell
/// "the disk is unreadable" from "the save is unreadable" — they call for
/// different responses.
#[allow(clippy::type_complexity)]
pub fn load<P: Progress, S: Store>(
    store: &S,
    slot: &str,
) -> Result<Option<P>, Result<LoadError, S::Error>> {
    let Some(contents) = store.read(slot).map_err(Err)? else {
        return Ok(None);
    };
    decode::<P>(&contents).map(Some).map_err(Ok)
}

/// Parse, verify, and migrate stored text. The half of [`load`] that has
/// nothing to do with where the text came from.
pub fn decode<P: Progress>(contents: &str) -> Result<P, LoadError> {
    let record: Record =
        ron::from_str(contents).map_err(|error| LoadError::Unreadable(error.to_string()))?;

    let computed = vellum_digest::fnv1a(record.payload.as_bytes());
    if computed != record.hash {
        return Err(LoadError::Corrupt {
            stored: record.hash,
            computed,
        });
    }
    if record.format > P::FORMAT {
        return Err(LoadError::FromTheFuture {
            stored: record.format,
            current: P::FORMAT,
        });
    }

    if record.format == P::FORMAT {
        return ron::from_str(&record.payload).map_err(|e| LoadError::NotTheType(e.to_string()));
    }

    let mut value: ron::Value =
        ron::from_str(&record.payload).map_err(|e| LoadError::Unreadable(e.to_string()))?;
    let mut at = record.format;
    while at < P::FORMAT {
        let step = P::MIGRATIONS
            .iter()
            .find(|m| m.from == at)
            .ok_or(LoadError::NoMigration { from: at })?;
        value = (step.apply)(value)
            .map_err(|message| LoadError::MigrationFailed { from: at, message })?;
        at += 1;
    }
    value
        .into_rust()
        .map_err(|error| LoadError::NotTheType(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// An in-memory store, which is all the Progress tests need.
    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<String, String>>);

    impl Store for Memory {
        type Error = core::convert::Infallible;
        fn read(&self, slot: &str) -> Result<Option<String>, Self::Error> {
            Ok(self.0.borrow().get(slot).cloned())
        }
        fn write(&self, slot: &str, contents: &str) -> Result<(), Self::Error> {
            self.0.borrow_mut().insert(slot.into(), contents.into());
            Ok(())
        }
        fn remove(&self, slot: &str) -> Result<(), Self::Error> {
            self.0.borrow_mut().remove(slot);
            Ok(())
        }
        fn slots(&self) -> Result<Vec<String>, Self::Error> {
            Ok(self.0.borrow().keys().cloned().collect())
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Totals {
        runs: u32,
        best_wave: u32,
        #[serde(default)]
        ships_boarded: u32,
    }

    impl Progress for Totals {
        const FORMAT: u32 = 1;
    }

    #[test]
    fn progress_round_trips_through_a_store() {
        let store = Memory::default();
        let totals = Totals {
            runs: 12,
            best_wave: 7,
            ships_boarded: 30,
        };
        save(&store, "profile", &totals).unwrap();
        let loaded: Totals = load(&store, "profile").unwrap().expect("stored");
        assert_eq!(loaded, totals);
    }

    /// A first run has no save, and that is not a failure to report.
    #[test]
    fn an_empty_slot_is_not_an_error() {
        let store = Memory::default();
        assert_eq!(load::<Totals, _>(&store, "profile").unwrap(), None);
    }

    /// Adding a field is not a format change: serde's default covers it, and
    /// a chain that grew an entry for every added field would be noise.
    #[test]
    fn an_added_field_needs_no_migration() {
        let store = Memory::default();
        #[derive(Serialize, Deserialize)]
        struct Older {
            runs: u32,
            best_wave: u32,
        }
        impl Progress for Older {
            const FORMAT: u32 = 1;
        }
        save(
            &store,
            "profile",
            &Older {
                runs: 3,
                best_wave: 2,
            },
        )
        .unwrap();

        let loaded: Totals = load(&store, "profile").unwrap().expect("stored");
        assert_eq!(loaded.runs, 3);
        assert_eq!(loaded.ships_boarded, 0);
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Renamed {
        runs: u32,
        /// Was `best_wave` at format 1.
        deepest_wave: u32,
    }

    impl Progress for Renamed {
        const FORMAT: u32 = 2;
        const MIGRATIONS: &'static [Migration] = &[Migration {
            from: 1,
            apply: |value| {
                let ron::Value::Map(mut map) = value else {
                    return Err("expected a map".into());
                };
                let old = map
                    .remove(&ron::Value::String("best_wave".into()))
                    .ok_or("v1 had no best_wave")?;
                map.insert(ron::Value::String("deepest_wave".into()), old);
                Ok(ron::Value::Map(map))
            },
        }];
    }

    /// A restructured field does need a step, and the step runs on load.
    #[test]
    fn a_renamed_field_migrates_forward() {
        let store = Memory::default();
        #[derive(Serialize, Deserialize)]
        struct V1 {
            runs: u32,
            best_wave: u32,
        }
        impl Progress for V1 {
            const FORMAT: u32 = 1;
        }
        save(
            &store,
            "profile",
            &V1 {
                runs: 9,
                best_wave: 4,
            },
        )
        .unwrap();

        let loaded: Renamed = load(&store, "profile").unwrap().expect("stored");
        assert_eq!(
            loaded,
            Renamed {
                runs: 9,
                deepest_wave: 4
            }
        );
    }

    /// A gap in the chain says so, rather than quietly handing back defaults —
    /// which would read to a player as their progress having been wiped.
    #[test]
    fn a_missing_step_is_named_rather_than_defaulted() {
        #[derive(Debug, Serialize, Deserialize)]
        struct Shape {
            runs: u32,
        }
        impl Progress for Shape {
            const FORMAT: u32 = 3;
        }

        let store = Memory::default();
        #[derive(Serialize, Deserialize)]
        struct V1 {
            runs: u32,
        }
        impl Progress for V1 {
            const FORMAT: u32 = 1;
        }
        save(&store, "profile", &V1 { runs: 1 }).unwrap();

        assert_eq!(
            load::<Shape, _>(&store, "profile").unwrap_err(),
            Ok(LoadError::NoMigration { from: 1 })
        );
    }

    /// A hand-edited or truncated save is caught before it becomes state.
    #[test]
    fn a_tampered_payload_is_refused() {
        let store = Memory::default();
        save(
            &store,
            "profile",
            &Totals {
                runs: 1,
                best_wave: 1,
                ships_boarded: 1,
            },
        )
        .unwrap();
        let stored = store
            .read("profile")
            .unwrap()
            .unwrap()
            .replace("runs:1", "runs:999");
        store.write("profile", &stored).unwrap();

        assert!(matches!(
            load::<Totals, _>(&store, "profile").unwrap_err(),
            Ok(LoadError::Corrupt { .. })
        ));
    }

    /// Downgrading is a real thing players do, and there is no honest way to
    /// read a format that did not exist when this build was written.
    #[test]
    fn a_save_from_a_newer_build_is_refused_not_guessed_at() {
        let store = Memory::default();
        save(
            &store,
            "profile",
            &Renamed {
                runs: 1,
                deepest_wave: 1,
            },
        )
        .unwrap();

        assert!(matches!(
            load::<Totals, _>(&store, "profile").unwrap_err(),
            Ok(LoadError::FromTheFuture {
                stored: 2,
                current: 1
            })
        ));
    }
}
