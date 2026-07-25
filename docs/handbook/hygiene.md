# Hygiene

Repo roots stay clean. The canonical `.gitignore` entries live in
`templates/gitignore-snippets.md`; the rules behind them:

- **Runtime debris never lands.** Logs (`*.log`), autosaves, screenshot
  output, scratch dirs (`target-verify/`), and tool caches (`.uv-cache*/`,
  `__pycache__/`, `*.egg-info/`) are ignored *before* they first appear —
  every one of these has already shown up untracked in some game's root.
- **`.cargo/config.toml` is always ignored** in game repos (see
  `local-dev.md` — a committed override is the failure mode).
- **Line endings are normalized.** Every repo carries a `.gitattributes`
  pinning LF for text formats; this repository's is the reference. CRLF
  drift is what made the pasm copies look forked when they never were, and
  what once made content fingerprints differ between Windows and Linux
  checkouts.
- **Editor/IDE state stays out** unless the whole fleet uses it.
- **Generated output is not committed** (Trunk's `dist/`, wasm-pack's
  `pkg/`) — CI builds it. The one historical exception (committed `web/pkg`
  in necessary-work) disappears with its Trunk migration.
- **Stale packaging residue is removed on sight** (egg-info directories from
  pip days, cache directories from abandoned experiments). If it isn't the
  source of anything, it goes.
