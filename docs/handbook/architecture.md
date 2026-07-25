# Architecture

Every game in the fleet has the same skeleton, however different the genres:

**A headless, deterministic simulation crate and a thin client.** The sim
crate depends on at most `bevy_ecs`/`bevy_app` (many are pure Rust); it never
touches a renderer, a window, an asset server, or a file. The client owns all
presentation, input, UI, and audio, and owns *no game rules*. Bevy is a frame
pump. This is what makes every game testable in CI, replayable, and portable
between native and web without a behavioural diff.

**A semantic command boundary.** Every meaningful player decision arrives at
the sim as a validated, typed command — never as direct mutation from UI
code. Commands can be rejected, and a rejection carries a player-readable
reason. Dev cheats go through the same path as everything else, so they
replay. AI and humans issue the same commands wherever both exist; nothing
downstream of admission may branch on which one it was.

**Modules, not monoliths.** Code is organised as a module tree with one
concern per file; a crate whose `lib.rs` is four thousand lines is a bug in
slow motion (an agent asked to change one part of a file it cannot hold in
its head will invent a second copy of something that already exists —
which is the exact failure PASM exists to catch). `nw-simulation`'s layout is
the in-fleet reference for a small sim: `state`, `command`, `sim`, `rng`,
`trace`, `units`.

**The sim's outputs are projections, not access.** Clients read snapshots or
viewmodels the sim publishes; they do not reach into sim state. Where the
client needs derived numbers, the sim derives them (traces, previews) so the
UI never re-implements a rule.

What is deliberately *not* standardized: the scheduler (fixed tick,
turn-based, or batch resolution — the scheduler encodes the genre), the
generator, the renderer, and the state/screen mechanism (Bevy `States`,
hand-rolled screen enums, and controller structs are all in use and all
fine). Pick what the game needs; keep it in the layer it belongs to.
