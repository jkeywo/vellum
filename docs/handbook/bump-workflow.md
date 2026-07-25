# The bump workflow

Vellum changes reach a game only through a rev bump, and a bump PR has a
strict shape: **its diff touches only manifests, lockfiles, and workflow
refs.** Anything else in the diff means the shared code changed behaviour,
and the PR stops being a bump.

## Bumping a game

1. Merge the change here. CI's consumer smoke matrix has already built the
   sacred games against it and proven no fingerprint moved.
2. In the game, update every vellum pin the repo carries — any of:
   - `Cargo.toml` rev(s) (+ `Cargo.lock` via `cargo update -p <crates>`)
   - `pyproject.toml` pasm rev (+ `uv.lock` via `uv sync`)
   - `.github/workflows/ci.yml` `uses:` rev
   Align all pins the repo has to the same rev in the same PR.
3. Let the game's full CI run. Green means the bump is what it claims.

## Adding a consumer to the smoke matrix

A game joins `.github/workflows/ci.yml`'s matrix here when it consumes
engine crates and has a real determinism suite — pinned fixtures the matrix
can watch. Games that consume nothing from `crates/` never join; the matrix
is per-push cost paid on every engine change.

## Re-blessable adoptions

A game in the re-blessable tier adopting a crate that moves its fixtures
does so in its *adoption* PR (fixture re-bless as its own commit — see
`determinism.md`), not in a bump PR. Bumps after adoption follow the strict
shape like everyone else.
