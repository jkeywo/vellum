# Versions

The fleet standard, for any crate or game being actively developed:

| what | standard |
|---|---|
| Rust | 1.95.0, pinned by a committed `rust-toolchain.toml` (see `templates/`) |
| Bevy | 0.19 |
| Edition | 2024 for new crates (`crates/` here stays 2021 — the floor its consumers set) |
| wasm target | `wasm32-unknown-unknown`, listed in `rust-toolchain.toml` |

**Adoption is policy, not a campaign.** A game upgrades to the standard when
it is next actively developed — the upgrade rides along with real work that
would have exercised the game anyway. Nobody opens a Bevy-upgrade PR for its
own sake: the games span three Bevy majors today and all of them work, and an
upgrade earns nothing until someone is in that codebase regardless.

The corollary: when you *are* actively developing a game that lags the
standard, the upgrade is part of your job, not a separate someday-task. Do it
first, on its own commit, before the feature work.

The recorded decision is `versions-are-policy-not-campaign` in
`pasm/spec/core/decisions.yaml`.
