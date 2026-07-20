//! Replaying a command log, and the contract a simulation must keep for that
//! to mean anything.
//!
//! In both games a save file is not a snapshot of the world — it is the seed
//! plus the list of commands that were accepted. Loading a save replays them.
//! That buys a great deal: saves are tiny, shareable, and diffable, and any
//! divergence between two builds shows up as a run that stops reproducing
//! rather than as a corrupt file nobody can read.
//!
//! It also imposes a contract, and the contract is easy to break by accident:
//!
//! - **Applying the same commands to the same seed must produce the same
//!   state.** Every time, on every target.
//! - **A rejected command must change nothing at all** — not the world, not
//!   the clock, and not the random number generator. A rejection that
//!   consumed a draw would make a run depend on how many illegal things a
//!   player tried, which no log records.
//! - **A rejected command must never enter the log**, or replaying it will
//!   reject again in a context where rejection is a hard error.
//!
//! [`Simulation`] states that contract as a trait, [`replay_into`] is the
//! driver both games were already running, and the `contract` module (behind
//! the `testkit` feature) checks it against a real simulation rather than a
//! toy.
//!
//! # What is deliberately not here
//!
//! The scheduler. One game resolves a whole batch of actors simultaneously in
//! phase order; the other applies one player action and then lets everyone
//! react. Those are not two implementations of one idea, they are different
//! games — the first is a stealth game where facing and the tile behind an
//! actor decide everything, the second an investigation game with a travel
//! clock. What they share is the *shape* below, and a shape is worth a trait
//! rather than a rewrite.

use core::fmt;

/// A simulation that can be driven by a log of commands.
pub trait Simulation {
    /// The unit of the replay log. Its serialised encoding is part of the save
    /// format: adding a variant in the middle renumbers the rest for a binary
    /// format, so pin the encoding somewhere.
    type Command: Clone;

    /// Why a command was refused. Surfaced to players in both games rather
    /// than swallowed, so it wants to be worth reading.
    type Rejection: fmt::Display;

    /// Apply one command. Must be the only way the simulation advances, and
    /// must change nothing on rejection.
    fn apply(&mut self, command: &Self::Command) -> Result<(), Self::Rejection>;

    /// Whether the run has ended. A log may contain commands recorded after
    /// the ending in some orderings; replay stops here rather than failing.
    fn is_over(&self) -> bool;

    /// A digest of everything that must match for two runs to be the same run.
    fn digest(&self) -> u64;

    /// Whether a multi-turn action is still resolving, so the next command
    /// cannot be submitted yet.
    ///
    /// Defaults to `false`, which is right for a simulation whose commands
    /// each take exactly one turn. A game with actions that occupy several
    /// turns implements this and [`Self::continue_step`], and the driver pumps
    /// between commands instead of submitting into a busy simulation.
    fn needs_continuation(&self) -> bool {
        false
    }

    /// Advance one turn of an in-progress multi-turn action. Only called while
    /// [`Self::needs_continuation`] is true.
    fn continue_step(&mut self) {}
}

/// A replay that did not reproduce its recording.
///
/// There is one variant because there is one cause worth distinguishing: a
/// command the simulation refused. Anything else — a different digest at the
/// end — is not a fault the driver can detect, and is what the caller compares
/// digests for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diverged<R> {
    /// Index into the log, so the failure names the command.
    pub at_command: usize,
    pub rejection: R,
}

impl<R: fmt::Display> fmt::Display for Diverged<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "replay diverged at command #{}: {}",
            self.at_command, self.rejection
        )
    }
}

impl<R: fmt::Display + fmt::Debug> std::error::Error for Diverged<R> {}

/// Apply a whole log to a freshly built simulation.
///
/// A rejection here is not a user error — the command was accepted once, when
/// it was recorded. It means this build no longer agrees with the one that
/// wrote the log, which is the failure the whole save format rests on not
/// happening.
pub fn replay_into<S: Simulation>(
    sim: &mut S,
    commands: &[S::Command],
) -> Result<(), Diverged<S::Rejection>> {
    for (at_command, command) in commands.iter().enumerate() {
        pump(sim);
        if sim.is_over() {
            return Ok(());
        }
        sim.apply(command).map_err(|rejection| Diverged {
            at_command,
            rejection,
        })?;
    }
    pump(sim);
    Ok(())
}

/// Let any in-progress multi-turn action finish.
fn pump<S: Simulation>(sim: &mut S) {
    while sim.needs_continuation() && !sim.is_over() {
        sim.continue_step();
    }
}

/// A simulation together with the log of what has been accepted.
///
/// The single rule this exists to enforce: a rejected command does not enter
/// the log. Both games got this right independently and both wrote a test for
/// it, which is a fair sign it is the mistake waiting to be made.
#[derive(Clone, Debug)]
pub struct Log<S: Simulation> {
    sim: S,
    commands: Vec<S::Command>,
}

impl<S: Simulation> Log<S> {
    pub fn new(sim: S) -> Self {
        Self {
            sim,
            commands: Vec::new(),
        }
    }

    /// Apply a command, recording it only if it was accepted.
    pub fn apply(&mut self, command: S::Command) -> Result<(), S::Rejection> {
        self.sim.apply(&command)?;
        self.commands.push(command);
        Ok(())
    }

    pub fn commands(&self) -> &[S::Command] {
        &self.commands
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    pub fn sim_mut(&mut self) -> &mut S {
        &mut self.sim
    }

    pub fn into_parts(self) -> (S, Vec<S::Command>) {
        (self.sim, self.commands)
    }
}

#[cfg(feature = "testkit")]
pub mod contract;

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter that refuses to go negative, with a two-turn "big" step, so
    /// the driver's continuation pumping is exercised by something.
    #[derive(Clone, Debug)]
    struct Counter {
        value: i64,
        busy: u8,
        draws: u64,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Step {
        Add(i64),
        Sub(i64),
        Big,
    }

    #[derive(Debug)]
    struct WouldGoNegative;

    impl fmt::Display for WouldGoNegative {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "that would take the counter below zero")
        }
    }

    impl Simulation for Counter {
        type Command = Step;
        type Rejection = WouldGoNegative;

        fn apply(&mut self, command: &Step) -> Result<(), WouldGoNegative> {
            match command {
                Step::Add(n) => self.value += n,
                Step::Sub(n) => {
                    // Checked *before* anything changes: rejection is pure.
                    if self.value - n < 0 {
                        return Err(WouldGoNegative);
                    }
                    self.value -= n;
                }
                Step::Big => self.busy = 2,
            }
            self.draws += 1;
            Ok(())
        }

        fn is_over(&self) -> bool {
            self.value >= 100
        }

        fn digest(&self) -> u64 {
            (self.value as u64)
                .wrapping_mul(31)
                .wrapping_add(self.draws)
                .wrapping_add(u64::from(self.busy))
        }

        fn needs_continuation(&self) -> bool {
            self.busy > 0
        }

        fn continue_step(&mut self) {
            self.busy -= 1;
            self.value += 1;
        }
    }

    fn counter() -> Counter {
        Counter {
            value: 0,
            busy: 0,
            draws: 0,
        }
    }

    #[test]
    fn a_log_replays_to_the_same_state() {
        let script = vec![Step::Add(5), Step::Big, Step::Sub(2), Step::Add(1)];
        let mut live = counter();
        replay_into(&mut live, &script).expect("replays");

        let mut again = counter();
        replay_into(&mut again, &script).expect("replays");
        assert_eq!(live.digest(), again.digest());
    }

    #[test]
    fn continuations_are_pumped_between_commands() {
        // Big sets two busy turns, each adding one. Without pumping the value
        // would be 0, and the busy counter would never drain.
        let mut sim = counter();
        replay_into(&mut sim, &[Step::Big]).expect("replays");
        assert_eq!(sim.value, 2);
        assert_eq!(sim.busy, 0);
    }

    #[test]
    fn a_refused_command_names_its_index() {
        let mut sim = counter();
        let fault = replay_into(&mut sim, &[Step::Add(1), Step::Sub(50)])
            .expect_err("the second command cannot apply");
        assert_eq!(fault.at_command, 1);
    }

    #[test]
    fn a_rejected_command_never_enters_the_log() {
        let mut log = Log::new(counter());
        log.apply(Step::Add(3)).expect("accepted");
        log.apply(Step::Sub(99)).expect_err("refused");
        assert_eq!(log.commands(), &[Step::Add(3)]);

        // And the log replays, which it would not if the refusal were recorded.
        let mut fresh = counter();
        replay_into(&mut fresh, log.commands()).expect("the log replays");
        assert_eq!(fresh.digest(), log.sim().digest());
    }

    #[test]
    fn replay_stops_at_an_ending_rather_than_failing() {
        let script = vec![Step::Add(100), Step::Add(1), Step::Add(1)];
        let mut sim = counter();
        replay_into(&mut sim, &script).expect("stops cleanly at the ending");
        assert_eq!(sim.value, 100, "commands after the ending were applied");
    }
}
