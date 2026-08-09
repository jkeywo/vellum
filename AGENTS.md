@RTK.md

# vellum — Agent Guide

This is the fleet's foundation: engine crates, the PASM tool, the handbook, and
the CI the games call. Seven games depend on it. Nothing here is a private
detail — every change has a blast radius, and the whole repository is shaped
around making that radius visible before it lands.

`README.md` is the charter — *why* the engine layer is what it is, and what is
deliberately not in it. Read it before adding to `crates/`. This file is how to
work here.

## Know the blast radius before you edit

| Area | Rule |
|---|---|
| `crates/` | **No-fingerprint-moves.** A change that alters an RNG byte stream, a digest, or the serialised shape of anything reachable from sim state invalidates recorded player runs in project-murmur and rogue-hunter. See `README.md` and `docs/handbook/determinism.md`. |
| `pasm/` | Ordinary rules — but a tool change is checked against seven authored specs, not just this one. |
| `pasm/spec/` | Vellum models itself with its own tool. Fleet decisions are recorded here, in `core/decisions.yaml`. |
| `docs/handbook/` | Normative for the fleet. A convention that applies to more than one game belongs here and nowhere else. |
| `templates/` | Starting points that are **copied**, not pinned. Editing one changes what the *next* game gets; it reaches existing games only if someone applies it by hand. |
| `.github/workflows/`, `actions/` | Games pin these by rev. Changing `fleet-ci.yml` or a composite action is a breaking-change surface like any other. |

## A new crate needs a driving consumer

Nothing lands in `crates/` speculatively. A crate exists because a named game
needs it now, and it is designed against that game's real use — the fleet's
crates each arrived that way, and the ones that generalise did so afterwards,
on evidence. If you cannot name the consumer and the code it replaces, the
crate is not ready.

The same test applies to widening an existing crate's API.

## Gates

All of these must be green before each commit — but run the full list **once**,
as a final gate pass when the change is otherwise sound, not after every edit,
implementation pass, or review pass (clippy is a near-full rebuild). While
iterating, use `cargo check` and the targeted tests for the area you touched;
review passes are read-only and run no gates. This repository has **no root
`pyproject.toml`** — the Python gates run from `pasm/`.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

```bash
cd pasm && uv sync --group dev && uv run pytest -q
cd pasm && uv run pasm validate spec --workspace-root ..
cd pasm && uv run pasm scan spec --json --workspace-root .. > /dev/null
```

CI splits along the same seam, so a docs edit never burns a matrix run:
`ci.yml` covers the engine plus the **consumer smoke matrix** (project-murmur,
rogue-hunter, necessary-work checked out at their pinned revs, patched onto
HEAD, determinism suites run); `pasm.yml` covers the Python package, this
repo's own spec, and anything under `docs/`, `templates/`, `actions/`, or
`.github/workflows/`.

The smoke matrix is the coordination mechanism between repositories. If it
fails, an engine change moved a fingerprint — treat that as the finding, not as
a flaky job to re-run.

## Changing the PASM tool

The seven games' specs are the real test suite. From the workbench directory
above this one, validate every game before merging a tool change:

```bash
uv run pasm validate pasm/spec        # from each game's root
```

A new rule that fires on a real spec is a conversation with that game's owner,
not a reason to quietly weaken the rule. A rule that fires nowhere may simply
mean the specs do not yet declare what it checks — say so rather than claiming
a clean bill of health.

Gotcha: a PASM YAML string containing `": "` must be quoted, or validation
fails with `invalid-list-item ... must be a string`.

Decisions carry an origin: `core.origin: ai` on entities an agent originated,
a literal `[ai] ` prefix on rationale bullets it wrote, absence meaning human.
AI-origin items are revisable by agents without ceremony and are listed by
`uv run pasm review spec` (from `pasm/`, like the other vellum invocations)
until a human ratifies them by deleting the marker. Mark what you originate —
including in vellum's own spec — and never remove a marker yourself.

## Nothing here reaches a game by merging

Delivery is a rev bump, per `docs/handbook/bump-workflow.md`. Merging to `main`
changes what games *can* adopt, never what they do. So:

- Leave deprecated entry points in place until every consumer has migrated —
  the fleet's fingerprint-moving migrations are two-phase by design.
- A bump PR in a game touches only manifests, lockfiles, and workflow refs.
  If your change forces more than that, it is an adoption, not a bump, and it
  belongs in its own PR with the re-bless as its own commit.
- Sacred-tier fingerprints (project-murmur, rogue-hunter) move only by a
  recorded decision in `pasm/spec/core/decisions.yaml` with the smoke matrix
  green at every intermediate step.

## Conventions worth not rediscovering

- **Record decisions as PASM, not prose.** `pasm/spec/core/decisions.yaml` is
  where a fleet choice becomes citable. Handbook pages explain conventions;
  they do not stand in for the decision record.
- **Line endings are pinned to LF** by `.gitattributes`. CRLF drift is what
  once made identical PASM copies look forked, and what made content
  fingerprints disagree between Windows and Linux checkouts.
- **Versions are policy, not a campaign** (`docs/handbook/versions.md`) — the
  standard is Rust 1.95.0 and Bevy 0.19, adopted by a game when it is next
  actively developed. `crates/` stays edition 2021: the floor its consumers set.
- **Repo roots stay clean** (`docs/handbook/hygiene.md`). Generated output,
  caches, and packaging residue are ignored before they first appear.
- **The `.cargo/config.toml` override is for local work only** and is never
  committed active in a consuming game (`docs/handbook/local-dev.md`). A leaked
  override builds CI against whatever is on disk, which makes the pinned rev
  worthless.

## Working across the fleet

The games are checked out beside this repository. `../AGENTS.md` is the
workbench guide — the fleet map, the pin-sweep command, and the cross-repo
protocol. Each game's own `AGENTS.md` is authoritative inside its directory,
and `templates/AGENTS.md` is what a new game starts from.
