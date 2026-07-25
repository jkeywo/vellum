# vellum

The shared foundation for a fleet of Rust/Bevy games. What lives here:

| directory | what it is | rules |
|---|---|---|
| `crates/` | the engine layer — digest, rng, strings, replay, grid | **the no-fingerprint-moves charter below** |
| `pasm/` | the PASM spec tool (Python), consumed by every game as a git subdirectory dependency | ordinary rules |
| `pasm/spec/` | vellum's own PASM spec — the foundation models itself | ordinary rules |
| `docs/handbook/` | fleet conventions: versions, architecture, CI, content, determinism tiers | ordinary rules |
| `templates/` | per-game starting points: AGENTS.md, CI callers, Trunk shell | ordinary rules |
| `.github/workflows/fleet-*.yml` | reusable CI/Pages workflows the games call, pinned by rev | ordinary rules |

The no-fingerprint-moves rule governs `crates/` only; `pasm/`, `docs/`,
`templates/`, and the fleet workflows evolve on ordinary rules. The rest of
this README is the engine layer's charter.

## The engine layer

A Rust roguelike engine layer — specifically, the layer underneath two terminal
roguelikes ([project-murmur](https://github.com/jkeywo/project-murmur) and
[rogue-hunter](https://github.com/jkeywo/rogue-hunter)) that turned out to be
the same layer.

The two games were written independently and converged hard. Both keep the
simulation engine-free and treat Bevy as a frame pump. Both made the save file
*be* the command log. Both hand-rolled PCG32 rather than depend on the `rand`
ecosystem, for the same reason. Both `include_str!` their content so native and
wasm ship byte-identical data. Both keep every player-facing line in one CSV.
Neither knew about the other's version of any of it.

This is the part that was genuinely the same.

| crate | what it is |
|---|---|
| `vellum-digest` | FNV-1a state digests, CRC-32, and the share-code envelope |
| `vellum-rng` | PCG32, with both seeding policies and both bounded draws kept apart |
| `vellum-strings` | the CSV reader and the `{name}` placeholder filler |
| `vellum-replay` | the `Simulation` trait, the replay driver, and contract checks |
| `vellum-grid` | deterministic shortest-path search, priced by the caller |
| `vellum-corpus` | the batch-and-report shape of autonomous testing: case driving under budgets, stall detection, tallies, the report envelope |

## The constraint everything here is shaped by

In both games a save is a seed plus a list of commands. Nothing is snapshotted;
loading replays. That means **the byte sequence of the random number generator
is part of the save format**, as is the serialised shape of every type reachable
from the simulation state. A change that would be a harmless refactor anywhere
else silently invalidates every run a player has recorded.

So the rule this repository was built under: *no change here may move a
fingerprint.* Both games carry pinned fixtures — RNG traces, golden runs with
exact share codes, byte snapshots of every command variant — and every one of
them was byte-identical before and after each extraction. A stage that wanted
to change a fixture had stopped being an extraction.

That rule is also why several things look more awkward than they need to:

- **The RNG has four entry points, not one.** The two games' bounded draws
  compute the same rejection threshold and then diverge completely — one
  multiplies and takes the high word, the other takes a remainder. Both are
  unbiased. Merging them would have rewritten every saved run in both games.
- **The RNG types are not shared, only the arithmetic.** Both games serialise
  their generator *inside* saved state, in different shapes: one field against
  two. `from_parts`/`into_parts` is the seam.
- **The grid search takes indices, not positions.** One game's `Direction` is a
  postcard variant index inside recorded commands; the other's `Pos` sits in a
  world whose RON text is the mission fingerprint. Those types stay home.

## What is deliberately not here

**The scheduler.** One game freezes a batch of actors and resolves them
simultaneously in phase order; the other applies one player action and lets
everyone react. That is not two implementations of one idea — it is the
difference between a stealth game where the tile behind an actor decides
everything and an investigation game with a travel clock. The schedulers encode
the genres. `vellum-replay` shares the *shape* around them and nothing more.

**Content pipelines.** ~100K of schema and cross-reference validation between
the two, sharing a convention rather than a mechanism.

**Generators, planners, renderers.** Both generate-and-prove-then-retry, and
they share no data structure while doing it.

**rogue-hunter's `bfs_step`.** It stops on a tile *adjacent* to its goal and
exits when a node is pushed rather than popped, so a Dijkstra with the same
neighbour order picks a different tile in ties. Converting it would have moved
every digest to deduplicate thirty-five private lines.

## Using it

Both games depend on these crates as git dependencies pinned to a rev:

```toml
[workspace.dependencies]
vellum-rng = { git = "https://github.com/jkeywo/vellum", rev = "..." }
```

For local work against a checkout, add a **gitignored** `.cargo/config.toml` to
the consuming repository:

```toml
[patch."https://github.com/jkeywo/vellum"]
vellum-rng = { path = "../vellum/crates/vellum-rng" }
```

It must stay out of version control. A leaked override would build CI against
whatever happened to be on disk rather than the pinned rev, which is the one
failure mode that makes a pinned rev worthless.

## CI

Alongside the usual fmt / clippy / test, this repository runs a **consumer
smoke job**: it checks out both games at the revs they currently pin, patches
them onto this repository's HEAD, and runs each game's determinism suite. An
engine change that would break a game's golden fixtures therefore fails here,
before the bump PR in that game exists.

That job is the whole coordination mechanism between the three repositories.
Bumping a game is then: merge here, open a PR there that changes only the rev,
let that game's full CI run. A rev-bump PR whose diff touches anything outside
`Cargo.toml` and `Cargo.lock` means the engine changed behaviour.
