# vellum's own PASM spec

The foundation models itself with the same tool every game uses. Two files:

- `core/foundation.yaml` — what lives in this repository and where, with
  implementation mappings that `pasm scan` checks against the actual tree.
- `core/decisions.yaml` — the fleet-level decision record: choices that bind
  all seven games (version policy, CI shape, determinism tiers, what may
  become a shared crate and what may not). Game-level decisions stay in each
  game's own `pasm/spec/core/decisions.yaml`.

Checked in CI (`.github/workflows/pasm.yml`):

```sh
cd pasm
uv run pasm validate spec --workspace-root ..
uv run pasm scan spec --json --workspace-root ..
```
