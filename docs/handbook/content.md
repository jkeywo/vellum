# Content

## Formats

Rhai, RON, and TOML are all sanctioned; the format is a per-game choice
(scripting logic wants Rhai, Rust-shaped data wants RON, editor- and
JS-adjacent pipelines want TOML). What is *not* per-game:

- **No hardcoded gameplay values.** If a designer could plausibly tune it,
  it lives in content. Sanctioned exceptions: parse-time `unwrap_or`
  defaults that reproduce the compiled-in value, and client-side
  placeholders awaiting authoritative data.
- **Strict parsing.** Unknown fields are rejected; cross-references are
  validated; a content error fails loudly at load or validate time.

## The two loading patterns

**Compile-time embedding** (`include_str!`, a build-script walk, or a
generated constant): the default wherever replay determinism or share codes
exist, because native, wasm, and CI must ship byte-identical data. No hot
reload, by design; the iteration loop is headless tools and tests.

**Asset-loader loading** (a generic RON/TOML loader over the engine's asset
system): for games that want hot reload or an editor workflow.
void-and-thunder is the reference: the sim crate owns the deserialized
*types* and never touches a file; the client owns the loader; a `Default`
impl reproduces the compiled-in values so missing data degrades instead of
crashing; an equality guard breaks the save→watch→reparse loop; and the
determinism trade-off is recorded in the game's spec (hot reload and replay
do not mix — pick per run, not per game).

Pick one pattern per game and record the choice in its spec. The pattern is
the standard; the format is not.

## Strings

Player-facing text follows the strings convention wherever the game has
adopted it (four games so far): every string lives in one CSV (`id, context,
english`), code holds ids and never words, and two orthogonal conventions
apply —

- `{name}` slots are runtime interpolation, filled by `interpolate` /
  `trf!`-style lookups.
- `[square brackets]` wrapping a whole line marks agent-written placeholder
  copy awaiting a human writer. Keep the brackets on anything you write.

**Never branch on text.** If presentation needs to know what a string means,
give it a typed value and let the words be a lookup. A used-but-undefined or
defined-but-unused id fails the build where the convention is fully adopted.
Strings are excluded from content fingerprints so a copy edit never
invalidates a share code.
