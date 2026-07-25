# Canonical .gitignore entries

Every game's `.gitignore` carries these blocks (plus whatever the game
genuinely needs on top). Rationale in `docs/handbook/hygiene.md`.

```gitignore
# Rust
target/
**/*.rs.bk
*.pdb

# Local vellum override — NEVER committed: a leaked override builds CI
# against whatever happened to be on disk instead of the pinned rev.
.cargo/config.toml

# Python tooling
__pycache__/
*.pyc
*.egg-info/
.venv/
.uv-cache*/

# Web build output — CI builds it
dist/
web/pkg/

# Runtime debris — logs, saves, scratch output
*.log
*-autosave.*
target-verify/
```

Also canonical, as its own file: a `.gitattributes` normalizing line
endings (copy vellum's — LF for `rs/py/toml/md/yml/yaml/csv/html/css/js`).
