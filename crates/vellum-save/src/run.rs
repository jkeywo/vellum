//! A replayable run: what was recorded, and whether a build still reproduces
//! it.
//!
//! The half of "save" that **never migrates**. There is no hook for it here
//! and there is not meant to be: a run's entire value is that it reproduces
//! what it recorded, so a run replayed under changed rules is a different run.
//! Offering a migration would only give that lie a friendly name.

use core::fmt;

use serde::{Deserialize, Serialize};
use vellum_replay::{replay_into, Simulation};

use crate::version::{Moved, Versions};

/// One periodic state digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub tick: u64,
    pub digest: u64,
}

/// The divergence record: what the simulation hashed, and when.
///
/// The final digest is always kept. Periodic samples are optional because
/// their whole purpose is *locating* a divergence, and that only matters when
/// there is somewhere to look: necessary-work's runs are twenty thousand ticks
/// long and "it diverged at 1200" is the difference between a bug report and a
/// shrug, while rogue-hunter's are short enough that the final digest already
/// says everything.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// Sampling cadence in ticks. Zero means the sim samples nothing, which
    /// is the right answer for a short run.
    pub every: u64,
    pub samples: Vec<Sample>,
    pub final_digest: u64,
    pub final_tick: u64,
}

impl Ledger {
    /// Where two ledgers first disagree, preferring the earliest tick — the
    /// point of sampling is that the *first* divergence is the informative
    /// one; everything after it is downstream noise.
    pub fn first_disagreement(&self, other: &Ledger) -> Option<Sample> {
        for mine in &self.samples {
            match other.samples.iter().find(|s| s.tick == mine.tick) {
                Some(theirs) if theirs.digest == mine.digest => continue,
                // A tick the replay never sampled is itself a disagreement:
                // the run did not go the same way.
                _ => return Some(*mine),
            }
        }
        None
    }
}

/// A simulation that keeps its own divergence ledger.
///
/// The sampling lives in the simulation rather than in a driver callback, and
/// that is deliberate: the simulation is the only thing that knows what a tick
/// is. A driver that sampled would have to be told the cadence, be handed a
/// clock, and be trusted to call at the right moment — three chances to
/// disagree with the recording about when a hash was taken.
pub trait Sampling: Simulation {
    fn ledger(&self) -> &Ledger;

    /// Advance to `tick`, sampling on cadence, without executing anything.
    ///
    /// Two jobs, one method. A tick-stamped command is applied by advancing
    /// here and *then* executing, which is what makes a real-time log a
    /// command log. And once the log is exhausted [`verify`] calls it once
    /// more to run the tail out to the recorded end, because a run usually
    /// keeps going after its last input.
    ///
    /// It is not [`Simulation::needs_continuation`], which was the first
    /// thing tried and is wrong: `replay_into` pumps continuations *before
    /// every command*, so a sim that reported "not yet at the end tick" would
    /// run the entire game out before its first command arrived. That hook
    /// means "a multi-turn action is mid-resolution", and it still does.
    fn advance_to(&mut self, tick: u64);
}

/// Everything needed to replay a run and check that it replayed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run<C> {
    pub versions: Versions,
    /// Which scenario, level or mission this was. Free-form: the games
    /// disagree about what a run is *of*, and none of them are wrong.
    pub scenario: String,
    pub seed: u64,
    /// The log. Rejected commands belong here too if the game logs them — a
    /// command that was refused once must be refused again, and a replay that
    /// silently accepted it has diverged.
    pub commands: Vec<C>,
    pub ledger: Ledger,
}

impl<C: Serialize + serde::de::DeserializeOwned> Run<C> {
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    pub fn from_ron(text: &str) -> Result<Run<C>, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

/// What happened when a build was asked to reproduce a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict<R> {
    /// Every command replayed and every digest matched.
    Reproduced,
    /// This build will not attempt it: something the run depends on moved.
    /// Refused before replaying, because a replay under different rules
    /// proves nothing either way.
    Refused(Moved),
    /// A command the simulation would not accept. It was accepted once, when
    /// it was recorded, so this build no longer agrees with that one.
    Rejected { at_command: usize, rejection: R },
    /// The commands all replayed and the state came out different. The tick
    /// is `None` when only the final digest disagreed — a run with no
    /// periodic samples can say *that* it diverged but not *where*.
    Diverged {
        at_tick: Option<u64>,
        recorded: u64,
        replayed: u64,
    },
}

impl<R: fmt::Display> fmt::Display for Verdict<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Reproduced => f.write_str("reproduced"),
            Verdict::Refused(moved) => write!(f, "not replayable: {moved}"),
            Verdict::Rejected {
                at_command,
                rejection,
            } => write!(
                f,
                "command #{at_command} was refused on replay: {rejection}"
            ),
            Verdict::Diverged {
                at_tick: Some(tick),
                recorded,
                replayed,
            } => write!(
                f,
                "diverged at tick {tick} (recorded {recorded:016x}, replayed {replayed:016x})"
            ),
            Verdict::Diverged {
                at_tick: None,
                recorded,
                replayed,
            } => write!(
                f,
                "final state differs (recorded {recorded:016x}, replayed {replayed:016x})"
            ),
        }
    }
}

/// Replay `run` into `sim` and report whether it reproduced.
///
/// `sim` must be freshly built from `run.seed` and the current content — this
/// takes it rather than building it because only the game knows how.
///
/// The version gate runs first and refuses without replaying. That ordering is
/// the whole reason the gate exists: replaying a run under changed rules
/// produces a divergence report about a run that was never going to reproduce,
/// which reads like a bug in the simulation instead of a stale save.
pub fn verify<S: Sampling>(
    run: &Run<S::Command>,
    current: &Versions,
    sim: &mut S,
) -> Verdict<S::Rejection> {
    if let Err(moved) = run.versions.check(current) {
        return Verdict::Refused(moved);
    }

    if let Err(diverged) = replay_into(sim, &run.commands) {
        return Verdict::Rejected {
            at_command: diverged.at_command,
            rejection: diverged.rejection,
        };
    }
    // The tail. A run rarely ends on its last command — necessary-work's runs
    // play out for thousands of ticks after the last order is given, and the
    // digest that matters is the one at the end of that.
    sim.advance_to(run.ledger.final_tick);

    let replayed = sim.ledger();
    if let Some(sample) = run.ledger.first_disagreement(replayed) {
        let found = replayed
            .samples
            .iter()
            .find(|s| s.tick == sample.tick)
            .map(|s| s.digest)
            .unwrap_or_default();
        return Verdict::Diverged {
            at_tick: Some(sample.tick),
            recorded: sample.digest,
            replayed: found,
        };
    }

    let final_digest = sim.digest();
    if final_digest != run.ledger.final_digest {
        return Verdict::Diverged {
            at_tick: None,
            recorded: run.ledger.final_digest,
            replayed: final_digest,
        };
    }

    Verdict::Reproduced
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter whose commands are stamped with the tick they happened on —
    /// the shape necessary-work's log already has, and the reason the trait
    /// fits a real-time simulation without changing it.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Stamped {
        tick: u64,
        add: i64,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct WouldGoNegative;
    impl fmt::Display for WouldGoNegative {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("that would take the total below zero")
        }
    }

    struct Counter {
        tick: u64,
        total: i64,
        ledger: Ledger,
    }

    impl Counter {
        fn new(every: u64) -> Counter {
            Counter {
                tick: 0,
                total: 0,
                ledger: Ledger {
                    every,
                    ..Ledger::default()
                },
            }
        }

        /// Advance one tick, sampling on cadence. This is the "sim samples its
        /// own ledger" half — no driver is told when a tick happens.
        fn step(&mut self) {
            self.tick += 1;
            if self.ledger.every > 0 && self.tick.is_multiple_of(self.ledger.every) {
                self.ledger.samples.push(Sample {
                    tick: self.tick,
                    digest: self.digest(),
                });
            }
            self.ledger.final_tick = self.tick;
            self.ledger.final_digest = self.digest();
        }
    }

    impl Simulation for Counter {
        type Command = Stamped;
        type Rejection = WouldGoNegative;

        /// Advance to the command's tick, then execute it. This is what makes
        /// a tick-stamped real-time log a command log.
        fn apply(&mut self, command: &Stamped) -> Result<(), WouldGoNegative> {
            self.advance_to(command.tick);
            if self.total + command.add < 0 {
                return Err(WouldGoNegative);
            }
            self.total += command.add;
            Ok(())
        }

        fn is_over(&self) -> bool {
            false
        }

        fn digest(&self) -> u64 {
            vellum_digest::fnv1a(&self.total.to_le_bytes())
        }
    }

    impl Sampling for Counter {
        fn ledger(&self) -> &Ledger {
            &self.ledger
        }

        fn advance_to(&mut self, tick: u64) {
            while self.tick < tick {
                self.step();
            }
        }
    }

    fn record(commands: Vec<Stamped>, run_to: u64, every: u64) -> Run<Stamped> {
        let mut sim = Counter::new(every);
        replay_into(&mut sim, &commands).expect("the recording itself replays");
        sim.advance_to(run_to);
        Run {
            versions: Versions::new(1, "0.1", 0x99),
            scenario: "counting".into(),
            seed: 7,
            commands,
            ledger: sim.ledger.clone(),
        }
    }

    fn commands() -> Vec<Stamped> {
        vec![
            Stamped { tick: 10, add: 5 },
            Stamped { tick: 20, add: 3 },
            Stamped { tick: 30, add: -2 },
        ]
    }

    #[test]
    fn a_recording_reproduces_itself() {
        let run = record(commands(), 100, 10);
        let mut sim = Counter::new(10);
        assert_eq!(
            verify(&run, &run.versions.clone(), &mut sim),
            Verdict::Reproduced
        );
    }

    /// The trap that `advance_to` exists to avoid, kept as a test because it
    /// is the obvious thing to try and it fails quietly.
    ///
    /// `replay_into` pumps continuations *before every command*, not only
    /// after the last one. A simulation that reported `needs_continuation`
    /// until its end tick would therefore play the entire run out before its
    /// first command arrived — every command would then land at the final
    /// tick, and the digests would disagree for a reason that looks nothing
    /// like the cause.
    #[test]
    fn the_tail_cannot_be_driven_by_needs_continuation() {
        struct Eager(Counter);

        impl Simulation for Eager {
            type Command = Stamped;
            type Rejection = WouldGoNegative;
            fn apply(&mut self, command: &Stamped) -> Result<(), WouldGoNegative> {
                self.0.apply(command)
            }
            fn is_over(&self) -> bool {
                false
            }
            fn digest(&self) -> u64 {
                self.0.digest()
            }
            // The wrong reading of the hook.
            fn needs_continuation(&self) -> bool {
                self.0.tick < 100
            }
            fn continue_step(&mut self) {
                self.0.step();
            }
        }

        let mut eager = Eager(Counter::new(10));
        replay_into(&mut eager, &commands()).expect("nothing is rejected");
        // Every command landed at tick 100 rather than at 10, 20 and 30, so
        // the whole run was sampled with an empty total. Tick 10 agrees by
        // coincidence — the first command had not been applied there either —
        // and everything after it does not.
        assert_eq!(eager.0.tick, 100);
        let honest = record(commands(), 100, 10);
        assert_eq!(
            honest.ledger.first_disagreement(&eager.0.ledger),
            Some(Sample {
                tick: 20,
                digest: honest.ledger.samples[1].digest
            }),
            "the eager sim should first disagree at tick 20"
        );
    }

    #[test]
    fn a_run_round_trips_through_ron() {
        let run = record(commands(), 100, 10);
        let text = run.to_ron().expect("serializes");
        assert_eq!(Run::<Stamped>::from_ron(&text).expect("parses"), run);
    }

    /// The gate runs before the replay, so a stale save reads as a stale save
    /// rather than as a simulation that has started producing wrong answers.
    #[test]
    fn changed_rules_refuse_without_replaying() {
        let run = record(commands(), 100, 10);
        let current = Versions::new(1, "0.2", 0x99);
        let mut sim = Counter::new(10);
        assert!(matches!(
            verify(&run, &current, &mut sim),
            Verdict::Refused(Moved::Rules { .. })
        ));
    }

    /// There is no migration hook, and this is the test that says so: the
    /// only way to read a run recorded under other rules is not to.
    #[test]
    fn a_run_has_no_way_to_be_brought_forward() {
        let mut run = record(commands(), 100, 10);
        run.versions.content = 0x1234;
        let current = Versions::new(1, "0.1", 0x99);
        let mut sim = Counter::new(10);
        assert!(matches!(
            verify(&run, &current, &mut sim),
            Verdict::Refused(Moved::Content { .. })
        ));
    }

    /// A build that refuses a command it once accepted names the command.
    #[test]
    fn a_command_refused_on_replay_is_located() {
        let mut run = record(commands(), 100, 10);
        // Rewrite the log so the third command overdraws: the recording is
        // now something this simulation will not do.
        run.commands[2].add = -99;
        let mut sim = Counter::new(10);
        assert!(matches!(
            verify(&run, &run.versions.clone(), &mut sim),
            Verdict::Rejected { at_command: 2, .. }
        ));
    }

    /// The reason periodic samples exist: the earliest disagreement, not the
    /// final one, is the one that says where to look.
    #[test]
    fn periodic_samples_locate_a_divergence() {
        let mut run = record(commands(), 100, 10);
        run.ledger.samples[2].digest ^= 0xff;
        let mut sim = Counter::new(10);
        assert!(matches!(
            verify(&run, &run.versions.clone(), &mut sim),
            Verdict::Diverged {
                at_tick: Some(30),
                ..
            }
        ));
    }

    /// Without samples a run can still say *that* it diverged. A short run
    /// pays nothing for a locator it does not need.
    #[test]
    fn an_unsampled_run_reports_divergence_without_a_tick() {
        let mut run = record(commands(), 100, 0);
        run.ledger.final_digest ^= 0xff;
        let mut sim = Counter::new(0);
        assert!(matches!(
            verify(&run, &run.versions.clone(), &mut sim),
            Verdict::Diverged { at_tick: None, .. }
        ));
    }
}
