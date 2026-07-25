@RTK.md

# <GAME NAME> — Agent Guide

<!-- Template. Replace every <ANGLE-BRACKET> placeholder; delete sections the
     game genuinely doesn't have (and say so in the spec instead). The fleet
     conventions this file leans on live in vellum's docs/handbook/. -->

<One paragraph: what the game is, what the player does, what makes it it.>

## Tech stack

| Layer | Technology |
|---|---|
| Simulation core | Rust, `<SIM_CRATE>` — deterministic, headless, no engine/platform deps |
| Client | Bevy, `<CLIENT_CRATE>` — presentation, input, UI, audio; owns no game rules |
| Game data | <FORMAT> under `<DATA_DIR>`, <embedded at compile time / loaded via asset loader> |
| Text | `<STRINGS_CSV_PATH>` — every player-facing string |
| Architecture model | PASM — YAML spec under `pasm/spec/`, tool from vellum |
| Shared crates | vellum (`<LIST OR "none yet">`), pinned by rev |
| CI | fleet-ci caller (`.github/workflows/ci.yml`) → tests, clippy, PASM, Pages deploy |

## Project rules

- The deterministic simulation lives in `<SIM_CRATE>`; presentation, input,
  UI, and audio live in `<CLIENT_CRATE>`. Bevy never reaches the simulation.
- Every player decision enters the sim as a validated, typed command that can
  be rejected with a reason. UI code never mutates sim state.
- Read and update `pasm/spec/` before or alongside every structural or
  gameplay change; record accepted choices in `pasm/spec/core/decisions.yaml`.
- Player-facing prose belongs in content, never in code or sim logic.
- No hardcoded gameplay values: if a designer could tune it, it lives in
  content. Parse-time defaults and awaiting-data placeholders are the only
  exceptions.

## Text — never write a string literal

Every player-facing string lives in `<STRINGS_CSV_PATH>` as `id, context,
english`. Code holds ids, never words.

- `{name}` slots are runtime interpolation.
- Wrap any line you (an agent) write in `[square brackets]`; a human writer
  removes the brackets when the line becomes real prose.
- **Never branch on text** — if presentation needs to know what a string
  means, give it a typed value and let the words be a lookup.

## PASM — keep it up to date

1. Model first, then build — spec entities before Rust for a new system.
2. Record decisions in `pasm/spec/core/decisions.yaml` as you make them.
3. `uv run pasm validate pasm/spec` after any model change; fix before commit.
4. `uv run pasm scan pasm/spec --json` periodically; close gaps in the model
   or the code, whichever is wrong.
5. Never leave dead spec — removing a system updates its declarations.

## Common commands

```bash
# CI gates — run all of these before calling work done
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run pasm validate pasm/spec

# Run the game
cargo run --release -p <CLIENT_CRATE>

# Web build
trunk serve                       # local dev server
trunk build --release             # what CI ships (with --public-url /<repo>/)
```

## Vellum — the shared foundation

This repo pins vellum by rev in up to three places (Cargo.toml,
pyproject.toml, the `uses:` line in `.github/workflows/ci.yml`). A vellum
bump PR aligns all of them and touches nothing else. For local work against
a checkout, see vellum's `docs/handbook/local-dev.md` — overrides are
gitignored or commented, never committed active.
