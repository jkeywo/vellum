# Game-design dynamics

How a game's *design* — causality, pacing, scarcity, and intent — is modelled in
PASM so that an agent can reason about it without reading a five-thousand-line
world file, and so that drift between intent and content is caught by CI. The
decisions behind this page are recorded in `pasm/spec/core/decisions.yaml`
(`authored-content-is-design-truth`) and the PASM changelog; this page explains.

## The shape

The authored content file (a TOML world, a RON tuning file) is **truth**. The
spec never copies its numbers. Instead:

- **Anchors** say where a value lives and what the design intends of it.
  `expect` is a structural claim (drift = error); `min`/`max` are intent
  (a human tuning outside them gets a warning — a conversation, not a gate).
- **Gates** are the causal spine: `requires`/`enables` order progression,
  `requires_player_action` names the verbs that drive a gate, and a gate with
  a `deadline_id` must say what failing it causes (`on_failure`) or declare
  `benign: true`. A gate with neither player action nor a `self_resolving`
  declaration is flagged: the act may be a cutscene.
- **Consequences** are outcomes with design weight — campaign flags,
  casualties. `magnitude_source` is prose; `depends_on_state` points at the
  state the number is read from; `handler` names the script function that
  implements it, and the content check verifies the function exists.
- **A `scenario_model`** opts a world file into closed-world completeness:
  every authored deadline must be claimed by exactly one gate. Add a twelfth
  deadline to the world and validation fails until the design says what it
  means.
- **`design_invariant`** binds intent ↔ authored numbers ↔ the test that
  asserts the relation. PASM never evaluates the relation; it verifies the
  triangle, so renaming the asserting test breaks the build instead of
  silently killing the claim.
- **`pacing`** is a budget of phase windows over the mission clock; every
  authored deadline must land in exactly one window, and a cast role no phase
  engages is the missing workload map, surfaced as a warning.
- **`design_principle`** is a falsifiable hypothesis: context, construction,
  expected dynamic, experience hypothesis, and `measured_by`. Accepted but
  unmeasured is UNVERIFIED; declared counter-evidence demands a human
  decision. Taste is not evaluated — it is forced into a testable shape.

Game-local vocabulary maps onto the kernel with `specialises: gate` (etc.), so
a `storm_band` or a `wave` gets the kernel's validators without new tool code.

## The workflow

```bash
pasm design bootstrap assets/worlds/<world>.toml --out pasm/spec/design/<world>.yaml
```
drafts origin-ai skeletons (one gate per authored deadline, each carrying its
open questions). An agent fills in causal intent from the world file's
commentary and the GDD; a human ratifies through `pasm review`.

```bash
pasm design digest
```
is the agent's entry point for design work: declared intent merged with live
authored values, causal chains ("what happens if the crew miss `lyra_clear`?"),
the scarcity ledger, and the content hashes write-back requires.

```bash
pasm design writeback changes.json [--dry-run]
```
applies value retunes, templated row insertions/removals, and Rhai handler
stubs to the authoritative file — hash-guarded, comment-preserving, reparsed
before writing, and **refused** outside declared bounds. The agent is bound by
declared intent; the human never is.

## Fleet examples

- `project-phoenix-v2/pasm/spec/design/falling-skyway.yaml` — the full
  treatment: 11 anchored deadlines, the 52-vs-66 scarcity invariant asserted
  by a headless test, a three-act pacing budget, and the AI-backfill survey's
  zero-input finding standing in CI as a warning instead of living in a loose
  document.
- `void-and-thunder/pasm/spec/design/skirmish.yaml` — the genericity proof:
  no clock, no deadlines, RON content, a wave director modelled as three
  gates, and the shipped-tuning-equals-defaults test bound as an invariant.
