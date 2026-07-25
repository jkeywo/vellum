# Local development against vellum

Games consume vellum pinned to a rev. Working on a game and the shared code
together needs a local override — and every override mechanism here has the
same iron rule: **it must never reach version control in an active state.**
A leaked override builds CI against whatever happened to be on disk, which
is the one failure mode that makes a pinned rev worthless.

## Rust crates

A gitignored `.cargo/config.toml` in the *game* repository:

```toml
[patch."https://github.com/jkeywo/vellum"]
vellum-rng = { path = "../vellum/crates/vellum-rng" }
# ...one line per crate the game consumes
```

`.cargo/config.toml` is in each game's `.gitignore` (see
`templates/gitignore-snippets.md`). The consumer smoke matrix writes exactly
this file in CI — that is the sanctioned, ephemeral use.

## pasm

A commented block in the game's `pyproject.toml`, uncommented only locally:

```toml
# [tool.uv.sources]
# pasm = { path = "../vellum/pasm", editable = true }
```

Re-comment before committing; the committed state always resolves the
pinned rev.

## The fleet CI caller

There is no local override for a `uses:` pin. Iterating on `fleet-ci.yml`
means pushing a branch here and temporarily pointing one game's caller at
the branch rev in a PR — never merging a caller that references a branch.

## Layout assumption

All of this assumes the sibling-checkout layout the fleet already uses:
`C:\Coding\vellum` next to `C:\Coding\<game>`. Overrides are written
relative (`../vellum/...`) so the same block works on any machine with that
layout.
