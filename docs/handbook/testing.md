# Testing

The fleet's testing spine, in order of authority:

1. **Unit and integration tests** on the pure sim modules — the bulk of every
   game's suite, runnable headless on any machine.
2. **Determinism fixtures** — golden runs, RNG traces, command-byte
   snapshots. These are load-bearing artifacts, not test conveniences; see
   `determinism.md` for the tiers and the re-bless rules.
3. **Autonomous corpus runs** — the fleet standard for "does the game
   actually work": seeded bots or autoplayers executed headlessly in
   batches, producing a deterministic report that CI consumes. Every game
   either has this shape (rogue-hunter's corpus scan, murmur's autoplayer,
   necessary-work's bots, last-aeon's replay acceptance, phoenix's balance
   runner) or has it specified. The shared batch/report machinery is the
   planned `vellum-corpus` crate, extracted from the two sacred games'
   converged code, with void-and-thunder's scenario harness as the first
   consumer.
4. **PASM** — `pasm validate` and `pasm scan` gate CI in every repo,
   including this one. Spec assertions can fail without touching a line of
   Rust; run them whenever the spec changes.

Rules of thumb, fleet-wide:

- A test asserts observable behaviour through the public interface — never
  private fields, call counts, or implementation details.
- A game that merely compiles against a changed shared crate proves nothing;
  only the determinism suite and the corpus see the failure that matters.
- Scenario checks (`pasm scenario`) run authored step lists against the
  declared model — use them where a game has meaningful multi-step flows
  whose ordering the spec constrains.
- Performance numbers are benchmark evidence, not unit-test assertions. The
  planned profiling crate (`vellum-perf`, from phoenix's cross-runtime
  measurement contract) keeps baselines versioned and comparisons warned
  before they are ever gates.
