# Determinism

Determinism is the fleet's top value: seeded RNG, fixed or discrete ticks,
byte-identical content across native and wasm, and — in the games that have
earned it — a save that *is* a seed plus a command log.

## The two tiers

**Sacred** — project-murmur and rogue-hunter. A save is a seed and a command
log; loading replays. The RNG byte stream, every digest, and the serialised
shape of everything reachable from sim state are part of the save format. No
change in `crates/` may move any of their fingerprints *by accident*, and the
consumer smoke matrix in this repository's CI enforces it: every push builds
both games against HEAD, runs their determinism suites, and fails if a
golden, fixture, or trace file moved.

Sacred fingerprints may still move by **coordinated fleet decision** — a
recorded decision in vellum's spec, a two-phase migration (new API added
alongside the old; each game migrates and re-blesses in its own PR; the old
API removed once both are off it), and the smoke matrix green at every
intermediate step. The RNG unification (`rng-unification-breaks-saves`) is
the template.

**Re-blessable** — everyone else. These games may adopt a shared crate even
when it changes their RNG stream or hash values, by *deliberately* breaking
and re-blessing their fixtures in the adopting PR. The rules:

- The re-bless is its own commit in the PR, reviewable in isolation.
- After that commit, `git diff --stat -- '*golden*' '*fixture*' '*trace*'`
  stays empty for the rest of the branch.
- A re-bless that was not the point of the PR is a bug, not a convenience.

## Naming

Pinned artifacts contain `golden`, `fixture`, or `trace` in their path — the
consumer smoke matrix greps for exactly those substrings, so an artifact
named outside the convention is an artifact the matrix cannot protect.

## Content

Native, wasm, and CI must ship byte-identical game data wherever replay or
share codes exist — which is why those games embed content at compile time
(see `content.md`). Fingerprints over content skip `\r` so a Windows checkout
and a Linux checkout agree; this bug has been shipped once already and does
not need shipping twice.
