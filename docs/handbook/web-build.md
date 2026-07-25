# Web build

**Trunk is the canonical web build.** A game has a `Trunk.toml`, an
`index.html` shell (start from `templates/index.html`), and nothing else —
no hand-run `wasm-bindgen`, no copy scripts in CI. Trunk installs a
wasm-bindgen CLI matching `Cargo.lock`, injects the init script, and copies
whatever `data-trunk` links declare.

Rules that come from shipped bugs:

- **`--public-url /<repo-name>/`** on every Pages build. The fleet-ci
  workflow derives it from the repository name; a wrong public URL is a
  white page whose assets 404 under the project subpath.
- **Trunk's dev server answers a missing file with 200 and the index page**,
  not a 404 — so a mistyped asset path surfaces as a parse error about
  `<!DOCTYPE` in whatever tried to load it. Keep asset paths in one place,
  and set `AssetMetaCheck::Never` in Bevy apps so meta probing doesn't hit
  the same trap.
- **Local verification before the CI switch**: `trunk serve`, watch the
  loading overlay appear and clear, then check the deployed Pages URL with
  the network tab open — zero 404s.

The shell (`templates/index.html`) carries: a loading overlay removed when
the canvas attaches, a fullscreen toggle the wasm app never sees, and
nothing game-specific. Games add their own shims (audio, HUD bridges) as
separate `<script>` blocks.

Documented exceptions: rogue-hunter's web client is deliberately not a Bevy
app (hand-written JS over a wasm library — wasm-pack stays);
project-phoenix-v2 wraps Trunk in a node pipeline (post-build wasm-opt, a
pure-JS client page, a separate viewer). Both reuse the composite actions
for the shared steps rather than the whole fleet-ci workflow.
