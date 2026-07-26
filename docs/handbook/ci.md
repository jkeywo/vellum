# CI

A game's CI is the ~15-line caller in `templates/ci.yml`, invoking
`jkeywo/vellum/.github/workflows/fleet-ci.yml@<rev>`. What the reusable
workflow runs:

| job | steps |
|---|---|
| pasm | `uv sync` → `pasm validate` (gate) → `pasm scan` (gate by default) → optional `pasm scenario` glob |
| rust | fmt → clippy `-D warnings` → tests → per-game `extra-check-commands` |
| wasm | Trunk (or wasm-pack) build → Pages artifact |
| deploy | GitHub Pages, main-branch pushes only |

Conventions the caller carries: `permissions` granting `pages: write` +
`id-token: write` (the deploy job can only narrow what the caller grants),
workflow-level `concurrency` (a called workflow cannot set it), and push
runs restricted to `main` so PR runs are the only other trigger.

A workspace with a **wasm-only crate** lints in two passes: the host
workspace excluding that crate, then the crate alone on
`wasm32-unknown-unknown`. Both go in `clippy-command` (it takes one command
per line) with `rust-targets: wasm32-unknown-unknown` so the target is
installed — the lint belongs to the rust job, not the wasm one, because it
is a lint and not a build.

Fleet defaults, overridable per game through inputs: ubuntu runner (Bevy
native tests need `apt-packages: libasound2-dev libudev-dev`), toolchain
1.95.0 when the repo has no `rust-toolchain.toml`, `pasm-scan: gate`
(`advisory` exists for repos still stabilizing their spec, and is a state to
leave, not to live in).

**When bespoke CI is allowed:** a repo whose pipeline genuinely exceeds the
workflow's shape (phoenix: node build, vitest, string checks, Playwright
smoke; rogue-hunter: hand-rolled web shell and corpus budget runs — though
its checks fit `extra-check-commands` and `pasm-scenario-glob`) keeps its
own workflow files but builds the shared steps from the composite actions
under `actions/`, so setup, PASM gating, and Pages assembly cannot drift.

**Migrating a hand-rolled rust-cache step:** `setup-fleet-rust` exposes no
`shared-key`, and `cache-workspaces` is not a substitute — that is rust-cache's
`workspaces`, which takes paths, so a label like `native` points the cache at a
directory that does not exist and quietly caches nothing rather than failing.
If each key had exactly one job, drop it: rust-cache's default key is already
per-job, so the caches stay separate (phoenix's native/wasm split turned out to
be this). If two or more jobs deliberately share one cache, the action cannot
express that yet — keep a hand-rolled step for those jobs, or add the input
here and bump the consumers. Nobody has needed the latter yet.

Keep any in-repo command documentation (AGENTS.md "Common Commands") in sync
with what CI actually runs — trusting a stale list is how a batch lands
green locally and red in CI.
