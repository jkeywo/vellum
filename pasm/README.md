# PASM

A game modelling system, to help avoid an LLM going off on one.

You write a spec describing what the codebase is *supposed* to be — its
entities, their relationships, the contracts between them — and PASM checks the
implementation against it. The point is not documentation. It is that an agent
asked to change one part of a system it cannot hold in its head will happily
invent a plausible second way of doing something that already exists, and a
scanner that knows the intended shape catches that before it lands.

First shared between [project-murmur](https://github.com/jkeywo/project-murmur)
and [rogue-hunter](https://github.com/jkeywo/rogue-hunter). Both games grew the
same tooling independently and ended up with byte-identical copies of it — every
`.py` under `architecture/`, `cli/`, `core/`, `domains/`, `implementation/`,
`integration/`, `migration/` and `scanners/` matched exactly. It was extracted
once so it stopped being maintained twice, and now lives here in the fleet's
shared repository, absorbed from the standalone
[jkeywo/pasm](https://github.com/jkeywo/pasm) at its extraction rev.

## What stays with each game

The **spec** does. `pasm/spec/` is authored per project — it is the model of
*that* codebase — and it stays in the repository it describes. This package is
the machinery that reads and checks a spec, not the spec itself. (That includes
this repository: vellum's own `pasm/spec/` models the fleet foundation and is
checked by the same tool.)

Game-specific tools also stay where they are —
`rogue-hunter/pasm/tools/extract_strings.py` is a statement about
rogue-hunter's content pipeline, not about architecture models in general.

## Use

```sh
uv sync --group dev               # dev environment for working on PASM itself
uv run pasm validate path/to/pasm/spec   # check a model
uv run pasm scan --json           # scan the implementation against it
uv run pasm scenario tests/replays/x.yaml
```

The CLI takes paths, so a consuming repository needs no layout in particular —
only a spec directory to point at.

Consuming projects depend on it straight from git, as a subdirectory of this
repository, pinned to a rev:

```toml
dependencies = [
  "pasm @ git+https://github.com/jkeywo/vellum@<rev>#subdirectory=pasm",
]
```

For local work against a checkout, a consuming project can override the source
(kept commented or gitignored, never active in CI):

```toml
[tool.uv.sources]
pasm = { path = "../vellum/pasm", editable = true }
```
