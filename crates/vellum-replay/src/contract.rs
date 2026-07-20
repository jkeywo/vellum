//! Checks a real simulation keeps the replay contract.
//!
//! These are written as functions that panic rather than as `#[test]`s,
//! because the thing under test lives in the consuming crate: a game calls
//! them from its own test with its own constructor and its own script. The
//! panic messages are the actual output, so they say what broke and why it
//! matters rather than printing two digests.
//!
//! Both games already assert most of this about themselves, in their own
//! words. Sharing the checks means a game gets the ones it had not thought to
//! write — in particular [`rejection_is_pure`], which neither was testing
//! against the *digest*, only against the visible outcome.

use crate::{replay_into, Log, Simulation};

/// Applying the same script to a fresh simulation twice must land in the same
/// state.
///
/// The floor of the whole format. If this fails, nothing else is worth
/// checking.
pub fn replay_is_deterministic<S: Simulation>(new: impl Fn() -> S, script: &[S::Command]) {
    let mut first = new();
    replay_into(&mut first, script)
        .unwrap_or_else(|fault| panic!("the script did not apply to a fresh simulation: {fault}"));

    let mut second = new();
    replay_into(&mut second, script)
        .unwrap_or_else(|fault| panic!("the script applied once but not twice: {fault}"));

    assert_eq!(
        first.digest(),
        second.digest(),
        "the same script produced two different states. Something in the \
         simulation depends on more than the seed and the commands."
    );
}

/// A refused command must leave the simulation byte-for-byte as it was.
///
/// Stronger than checking the visible outcome, which is how both games were
/// testing it: a rejection that quietly consumed a random draw would leave the
/// world looking untouched while every subsequent draw shifted, so a run would
/// depend on how many illegal things a player happened to try. Nothing in a
/// command log records that.
///
/// `rejected` must be a command the simulation refuses in the state the script
/// leaves it in; the check fails loudly if it is accepted, because a test that
/// silently proves nothing is worse than no test.
pub fn rejection_is_pure<S: Simulation>(
    new: impl Fn() -> S,
    script: &[S::Command],
    rejected: &S::Command,
) {
    let mut sim = new();
    replay_into(&mut sim, script)
        .unwrap_or_else(|fault| panic!("the setup script failed: {fault}"));

    let before = sim.digest();
    let outcome = sim.apply(rejected);
    assert!(
        outcome.is_err(),
        "the command given as `rejected` was accepted, so this check proves \
         nothing. Pick one the simulation actually refuses here."
    );
    assert_eq!(
        sim.digest(),
        before,
        "a refused command changed the state. Time, a random draw, or some \
         piece of world moved on a command that was not allowed to happen."
    );
}

/// Refused commands must not reach the log, and the log must still replay.
pub fn refusals_stay_out_of_the_log<S: Simulation>(
    new: impl Fn() -> S,
    script: &[S::Command],
    rejected: &S::Command,
) {
    let mut log = Log::new(new());
    for command in script {
        // The script is expected to apply; a refusal here is a broken script.
        log.apply(command.clone())
            .unwrap_or_else(|error| panic!("the setup script was refused: {error}"));
    }
    let accepted_before = log.commands().len();
    let _ = log.apply(rejected.clone());
    assert_eq!(
        log.commands().len(),
        accepted_before,
        "a refused command was recorded. Replaying this log will refuse it \
         again, in a context where refusal is a hard error."
    );

    let mut fresh = new();
    replay_into(&mut fresh, log.commands())
        .unwrap_or_else(|fault| panic!("the recorded log did not replay: {fault}"));
    assert_eq!(
        fresh.digest(),
        log.sim().digest(),
        "the recorded log replayed to a different state than the run that \
         produced it"
    );
}

/// Run every contract check.
pub fn check_all<S: Simulation>(
    new: impl Fn() -> S + Copy,
    script: &[S::Command],
    rejected: &S::Command,
) {
    replay_is_deterministic(new, script);
    rejection_is_pure(new, script, rejected);
    refusals_stay_out_of_the_log(new, script, rejected);
}
