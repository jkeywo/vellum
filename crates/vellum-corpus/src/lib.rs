//! The batch-and-report shape of the fleet's autonomous testing.
//!
//! Every game in the fleet runs (or has specified) the same instrument:
//! seeded autonomous cases executed headlessly in batches, producing a
//! deterministic report that CI consumes — rogue-hunter's corpus scan,
//! project-murmur's autoplayer, necessary-work's bots, last-aeon's replay
//! acceptance, void-and-thunder's scenarios. This crate is the part of that
//! instrument which is the same everywhere:
//!
//! - [`drive`] — run cases from an iterator under a [`Budget`] of count
//!   and/or wall-clock time, collecting whatever record the game returns.
//! - [`StallGuard`] — give up on a run that has stopped observably
//!   progressing, without mistaking patience for progress.
//! - [`Tally`] and [`permille`] — the ordered counting and the rate unit
//!   every game's summary already uses.
//! - [`Report`] (feature `json`) — the envelope a batch ships to CI:
//!   provenance, records, summary.
//!
//! What is deliberately *not* here, per the extraction charter: the records
//! themselves, outcome taxonomies, bot policies, and report tables. Those are
//! game vocabulary — a `LossStage`, a `MissionOutcome`, a hand-aligned table
//! whose header sits beside its row format — and they stay home. The two
//! roguelikes' corpus code remains theirs; adopting this crate is a choice
//! each game makes (and for the sacred two, one that must move no bytes).
//!
//! Wall-clock budgets use [`std::time::Instant`], which requires a native
//! target. Corpus runs are a headless CI instrument; nothing here belongs in
//! a shipped wasm build.

use std::collections::BTreeMap;
use std::time::Instant;

/// How much a batch is allowed to cost before it stops taking new cases.
///
/// Both limits are optional; an unlimited budget runs the iterator dry. The
/// time budget is checked *between* cases — a case that overruns is finished,
/// not killed, so a budgeted batch always ends on a whole record.
#[derive(Debug, Clone, Copy, Default)]
pub struct Budget {
    /// Stop after this many cases.
    pub max_cases: Option<u64>,
    /// Stop taking new cases once this much wall-clock time has elapsed.
    pub max_seconds: Option<f64>,
}

impl Budget {
    /// A budget bounded only by case count.
    pub fn cases(n: u64) -> Self {
        Self {
            max_cases: Some(n),
            max_seconds: None,
        }
    }

    /// A budget bounded by count and wall-clock seconds — the CI shape
    /// (`--count 256 --budget-seconds 300`).
    pub fn cases_within(n: u64, seconds: f64) -> Self {
        Self {
            max_cases: Some(n),
            max_seconds: Some(seconds),
        }
    }
}

/// Why a batch stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(rename_all = "kebab-case"))]
pub enum Exhausted {
    /// The case iterator ran dry — every offered case was driven.
    Cases,
    /// The case budget was reached.
    CaseBudget,
    /// The time budget was reached. The records collected so far are still
    /// whole; what a consumer does about the shortfall (fail, warn, report)
    /// is its own call.
    TimeBudget,
}

/// What a driven batch produced.
#[derive(Debug, Clone)]
pub struct Batch<R> {
    /// One record per driven case, in case order. Iteration order is the
    /// determinism seam: same cases, same runner, same records.
    pub records: Vec<R>,
    /// Why the batch stopped.
    pub exhausted: Exhausted,
    /// Wall-clock cost of the whole batch. Provenance, not evidence: never
    /// compare this in a golden check.
    pub elapsed_seconds: f64,
}

/// Drive `run` over `cases` under `budget`, collecting the records.
///
/// The case id is a plain `u64` on purpose: it is a seed for the games whose
/// batches are seed ranges, and an index for those whose batches are lists
/// of authored cases. The driver does not care, and keeping it primitive is
/// what lets both uses share the loop.
pub fn drive<R>(
    cases: impl IntoIterator<Item = u64>,
    budget: Budget,
    mut run: impl FnMut(u64) -> R,
) -> Batch<R> {
    let started = Instant::now();
    let mut records = Vec::new();
    let mut exhausted = Exhausted::Cases;

    for case in cases {
        // Nested rather than let-chained: this crate sits on edition 2021,
        // the floor its consumers set.
        if let Some(max) = budget.max_cases {
            if records.len() as u64 >= max {
                exhausted = Exhausted::CaseBudget;
                break;
            }
        }
        // Checked only once something has been driven: a time budget bounds
        // the batch, it does not veto it. A batch offered cases always
        // produces at least one whole record, however small the budget.
        if let Some(max) = budget.max_seconds {
            if !records.is_empty() && started.elapsed().as_secs_f64() >= max {
                exhausted = Exhausted::TimeBudget;
                break;
            }
        }
        records.push(run(case));
    }

    Batch {
        records,
        exhausted,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    }
}

/// A run that has stopped observably progressing.
///
/// The caller decides what "observable progress" is by choosing the snapshot
/// type: coarse on purpose, excluding anything that always advances (a turn
/// counter makes every stall look like progress). The guard trips after
/// `limit` consecutive identical observations — generous limits are correct,
/// because waiting is sometimes real play.
#[derive(Debug, Clone)]
pub struct StallGuard<P: PartialEq> {
    limit: u32,
    last: Option<P>,
    run: u32,
}

impl<P: PartialEq> StallGuard<P> {
    /// Trip after `limit` consecutive observations with no change.
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            last: None,
            run: 0,
        }
    }

    /// Record one observation. Returns `true` when the run has now stalled.
    pub fn observe(&mut self, now: P) -> bool {
        if self.last.as_ref() == Some(&now) {
            self.run += 1;
        } else {
            self.last = Some(now);
            self.run = 0;
        }
        self.run >= self.limit
    }
}

/// An ordered counter — the `BTreeMap<K, u64>` every game's summary already
/// carries for stages and rejection tags, kept ordered so iteration (and any
/// report built from it) is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(transparent))]
pub struct Tally<K: Ord>(BTreeMap<K, u64>);

impl<K: Ord> Default for Tally<K> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<K: Ord> Tally<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Count one occurrence of `key`.
    pub fn add(&mut self, key: K) {
        *self.0.entry(key).or_insert(0) += 1;
    }

    /// Count `n` occurrences of `key` — merging another counter's entry.
    pub fn add_n(&mut self, key: K, n: u64) {
        *self.0.entry(key).or_insert(0) += n;
    }

    /// How many times `key` was counted.
    pub fn count(&self, key: &K) -> u64 {
        self.0.get(key).copied().unwrap_or(0)
    }

    /// Total across every key.
    pub fn total(&self) -> u64 {
        self.0.values().sum()
    }

    /// Entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, u64)> {
        self.0.iter().map(|(key, count)| (key, *count))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Ord> Extend<K> for Tally<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        for key in iter {
            self.add(key);
        }
    }
}

/// `part` per thousand of `whole`, in integer arithmetic — the rate unit the
/// fleet counts in. Zero when `whole` is zero, so an empty batch reads as
/// zero-rate rather than panicking mid-report.
pub fn permille(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    ((part * 1000) / whole) as u32
}

#[cfg(feature = "json")]
mod report {
    use super::Exhausted;

    /// Where a report came from — enough to reproduce the batch. Everything
    /// here is context, not evidence: a golden comparison of two reports
    /// compares `records` and `summary`, never provenance (whose elapsed
    /// time differs on every run by nature).
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct Provenance {
        /// What produced the batch — a binary name, a test name.
        pub runner: String,
        /// The cases asked for, in the runner's own words ("seeds 0..256",
        /// "authored scenarios").
        pub cases: String,
        /// Content fingerprint, git rev, or whatever ties the batch to the
        /// exact data it ran against. Absent when the runner has none.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub fingerprint: Option<String>,
        /// Why the batch stopped.
        pub exhausted: Exhausted,
        /// Wall-clock cost. Context only.
        pub elapsed_seconds: f64,
    }

    /// The envelope a batch ships to CI: provenance, the per-case records,
    /// and the game's own summary. Record and summary types are the game's —
    /// this envelope only fixes where they sit, so every game's report can
    /// be picked up by the same tooling.
    #[derive(Debug, Clone, serde::Serialize)]
    pub struct Report<R: serde::Serialize, S: serde::Serialize> {
        pub title: String,
        pub provenance: Provenance,
        pub records: Vec<R>,
        pub summary: S,
    }

    impl<R: serde::Serialize, S: serde::Serialize> Report<R, S> {
        /// The report as pretty JSON — the artifact CI stores and tooling
        /// reads. Serialization of the fleet's own types cannot fail; a
        /// game's record type that can is a bug worth the panic.
        pub fn to_json(&self) -> String {
            serde_json::to_string_pretty(self).expect("corpus reports serialize")
        }
    }
}

#[cfg(feature = "json")]
pub use report::{Provenance, Report};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_collects_every_case_in_order() {
        let batch = drive(0..5, Budget::default(), |case| case * 2);
        assert_eq!(batch.records, vec![0, 2, 4, 6, 8]);
        assert_eq!(batch.exhausted, Exhausted::Cases);
    }

    #[test]
    fn a_case_budget_stops_the_batch_short() {
        let batch = drive(0..100, Budget::cases(3), |case| case);
        assert_eq!(batch.records, vec![0, 1, 2]);
        assert_eq!(batch.exhausted, Exhausted::CaseBudget);
    }

    #[test]
    fn a_time_budget_bounds_the_batch_but_never_vetoes_it() {
        // A zero-second budget still drives the first case — a batch offered
        // cases always produces at least one whole record — and trips before
        // the second.
        let batch = drive(0..100, Budget::cases_within(100, 0.0), |case| case);
        assert_eq!(batch.records, vec![0]);
        assert_eq!(batch.exhausted, Exhausted::TimeBudget);
    }

    #[test]
    fn an_empty_iterator_is_an_exhausted_batch_not_an_error() {
        let batch = drive(std::iter::empty(), Budget::default(), |case| case);
        assert!(batch.records.is_empty());
        assert_eq!(batch.exhausted, Exhausted::Cases);
    }

    #[test]
    fn the_stall_guard_trips_on_consecutive_identical_observations() {
        let mut guard = StallGuard::new(3);
        assert!(!guard.observe((0, false)));
        assert!(!guard.observe((0, false)));
        assert!(!guard.observe((0, false)));
        // The fourth identical observation is the third consecutive repeat.
        assert!(guard.observe((0, false)));
    }

    #[test]
    fn any_progress_resets_the_stall_run() {
        let mut guard = StallGuard::new(2);
        assert!(!guard.observe(1));
        assert!(!guard.observe(1));
        assert!(!guard.observe(2)); // progress
        assert!(!guard.observe(2));
        assert!(guard.observe(2));
    }

    #[test]
    fn tallies_count_in_key_order() {
        let mut tally = Tally::new();
        tally.extend(["stalled", "won", "won", "arrested"]);
        tally.add_n("won", 2);
        let entries: Vec<_> = tally.iter().map(|(k, n)| (*k, n)).collect();
        assert_eq!(
            entries,
            vec![("arrested", 1), ("stalled", 1), ("won", 4)],
            "iteration is key-ordered, so reports are deterministic"
        );
        assert_eq!(tally.total(), 6);
        assert_eq!(tally.count(&"missing"), 0);
    }

    #[test]
    fn permille_is_integer_and_total_on_empty() {
        assert_eq!(permille(1, 3), 333);
        assert_eq!(permille(3, 3), 1000);
        assert_eq!(
            permille(0, 0),
            0,
            "an empty batch is zero-rate, not a panic"
        );
    }

    #[cfg(feature = "json")]
    #[test]
    fn the_report_envelope_serializes_with_game_types_inside() {
        #[derive(serde::Serialize)]
        struct Record {
            case: u64,
            outcome: &'static str,
        }
        #[derive(serde::Serialize)]
        struct Summary {
            outcomes: Tally<&'static str>,
            won_permille: u32,
        }
        let mut outcomes = Tally::new();
        outcomes.extend(["cleared", "cleared", "lost"]);
        let report = Report {
            title: "example".into(),
            provenance: Provenance {
                runner: "test".into(),
                cases: "seeds 0..3".into(),
                fingerprint: None,
                exhausted: Exhausted::Cases,
                elapsed_seconds: 0.0,
            },
            records: vec![
                Record {
                    case: 0,
                    outcome: "cleared",
                },
                Record {
                    case: 1,
                    outcome: "cleared",
                },
                Record {
                    case: 2,
                    outcome: "lost",
                },
            ],
            summary: Summary {
                won_permille: permille(outcomes.count(&"cleared"), outcomes.total()),
                outcomes,
            },
        };
        let json = report.to_json();
        assert!(json.contains("\"cleared\": 2"), "tallies serialize as maps");
        assert!(json.contains("\"won_permille\": 666"));
        assert!(json.contains("\"exhausted\": \"cases\""));
    }
}
