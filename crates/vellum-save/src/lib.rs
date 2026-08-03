//! The fleet's persistence contract.
//!
//! # "Save" is two unrelated things
//!
//! | | what it is | who wants it |
//! |---|---|---|
//! | [`Progress`] | durable state: unlocks, totals, settings | v&t, the-usual, murmur |
//! | [`Run`] | a replayable run: seed + log + digests | rogue-hunter, murmur, last-aeon, necessary-work |
//!
//! A [`Run`] may also carry a [`Snapshot`] — captured world state to start
//! from instead of only a seed. That is not a third concept: a snapshot
//! refuses forever exactly as a log does (captured state under changed rules
//! is just as unreplayable), so it lives behind [`Run`]'s version gate. A
//! snapshot with an empty log is a saved game; with a continuation log it is
//! a resumable one. phoenix is the consumer that shaped it, and no other
//! game's stored runs change by a byte — the field is skipped when absent.
//!
//! A game opts into either, or both. They are not two configurations of one
//! envelope, and the difference is not cosmetic — **runs refuse forever and
//! progress migrates**, which is inherent rather than a policy this crate
//! chose. A run's whole value is that it reproduces what it recorded, so a
//! migrated run is a different run wearing the same name. Progress is just
//! what a player accumulated, and refusing to load it throws that away.
//!
//! So [`Run`] has no migration hook at all, and [`Progress`] takes an ordered
//! chain of them.
//!
//! void-and-thunder is what forced the split into the open: its input is
//! continuous analog at 64 Hz, so a "command log" for it would be an input
//! recording rather than a log. It can never be a replay game — and it still
//! wants its totals to survive a browser refresh.
//!
//! # Three version dimensions
//!
//! [`Versions`] carries format, rules and content, because they invalidate
//! different things and a refusal should say which. Content is a *digest*
//! rather than a number a human maintains, which two games in this fleet
//! learned the same way.
//!
//! # Three digest roles, kept apart
//!
//! | role | question | where |
//! |---|---|---|
//! | integrity | did the bytes survive the trip? | `ShareCodec`'s CRC, in vellum-digest |
//! | corruption | does stored state still match itself? | the self-hash on a [`Progress`] record |
//! | divergence | did a replay reproduce the recording? | the [`Ledger`] on a [`Run`] |
//!
//! One field cannot do all three jobs, and a contract that conflates them ends
//! up with a checksum being asked whether a simulation changed.
//!
//! # What is behind a feature
//!
//! ```text
//! vellum-save            Store, Progress, Versions   (serde + ron only)
//!   feature run          Run, Snapshot, verify       (pulls vellum-replay)
//!   feature backend-fs   <root>/<slot>.ron
//!   feature backend-web  localStorage["<game>:<slot>"]
//! ```
//!
//! A `Progress`-only consumer never compiles a replay engine it has no use
//! for, and a game with its own storage implements [`Store`] and pays for no
//! backend at all.

mod progress;
mod store;
mod version;

pub use progress::{decode, encode, load, save, LoadError, Migration, Progress, SaveError};
pub use store::{is_slot, NoStore, Store};
pub use version::{Moved, Versions};

#[cfg(feature = "backend-fs")]
pub use store::FileStore;
#[cfg(all(feature = "backend-web", target_arch = "wasm32"))]
pub use store::LocalStorage;

#[cfg(feature = "run")]
mod run;
#[cfg(feature = "run")]
pub use run::{verify, Ledger, Run, Sample, Sampling, Snapshot, Verdict};
