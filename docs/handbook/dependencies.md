# Dependencies

## Near-zero ecosystem crates

The fleet hand-rolls what most Bevy projects import: RNGs, CSV readers,
asset loaders, input handling, UI. This is deliberate, with two recurring
reasons — wasm payload size, and byte-layout stability (a dependency bump
that reshuffles an RNG or a serializer invalidates saves in the sacred
games). The current sanctioned exceptions, each with a documented reason in
its game: `bevy_egui` (last-aeon's UI), `bevy_rapier3d` (phoenix's physics),
`bevy_ratatui` + `ratatui` (the terminal games), `ratzilla` (murmur's web
terminal), `ul-next` (void-and-thunder's optional native HUD).

Adding an ecosystem crate to a game is a decision — record it in the game's
`decisions.yaml` with the reason, or don't add it.

## Pinning

Everything shared is pinned to a rev, never a branch:

- Cargo: `{ git = "https://github.com/jkeywo/vellum", rev = "<rev>" }`
- Python: `pasm @ git+https://github.com/jkeywo/vellum@<rev>#subdirectory=pasm`
- CI: `uses: jkeywo/vellum/.github/workflows/fleet-ci.yml@<rev>`

One repo may carry up to three pins on vellum (Cargo, pyproject, workflow).
That is legal — the fetchers are independent — but a bump PR aligns every
pin the repo has in one diff. See `bump-workflow.md`.

## wasm-bindgen

The `wasm-bindgen` CLI version must match the crate version in `Cargo.lock`.
Trunk resolves this automatically (it installs a matching CLI), which is one
of the reasons Trunk is the canonical web build; repos with bespoke wasm
builds pin the CLI explicitly and keep the pin in lockstep with the lock.

## Python

uv everywhere: `[tool.uv] package = false` tooling projects, a committed
`uv.lock`, `uv sync` in CI, `uv run pasm ...` for every invocation. pip
setups migrate at adoption time.
