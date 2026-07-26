# Design: the strings pipeline and the saves contract

**Status: designed, not built.** Resolved in a design interview 2026-07-26.
This file is the record of what was decided and *why the alternative lost*;
it becomes `strings.md` and `saves.md` (and stops being underscore-prefixed)
when the crates land with their consumers.

Both designs were reached by looking at what four games each built
independently, keeping the decisions that earned their place, and dropping
the ones that were only ever accidents of who wrote them first.

---

## Part 1 — the strings pipeline (`vellum-strings`, grown)

Today the crate is a CSV reader and a `{name}` filler. Four games built the
rest of a localisation pipeline on top of it, separately, and one of them
built it in JavaScript.

### 1. Locale-shaped, English-only content

`assets/strings/en.csv` and a locale resource the lookup reads. Only English
ships. Adding `fr.csv` is a **data** change, never a code change.

Every existing table has a `context` column "for whoever translates it" and
nobody has ever translated one — the column is aspirational. Making the
*format* locale-shaped costs nothing now and is the difference between
translation being a data task later and being a refactor.

### 2. One convention for call shapes, across three languages

| language | lookup | why it matters |
|---|---|---|
| Rust | `tr!("id")`, `trf!("id", k = v)` | literal ids, so the audit can scan |
| JS | `t("id")` | phoenix's client is pure JS; v&t's HUD is a page |
| HTML | `data-i18n="id"` | static markup needs ids too |

The two largest string consumers in the fleet are **not Rust** (phoenix's
whole client; v&t's `hud.html`). A Rust-only pipeline would leave most of the
fleet's actual player-facing text outside it. The Rust audit scans `.rs`,
`.js` and `.html` — replacing phoenix's `check-strings.mjs`, which is
currently the only reason that convention is enforced anywhere.

### 3. One parser, two emissions

CSV stays the **authored** format: `id,context,english` opens in a
spreadsheet, which is what a translator has.

```
en.csv --[the Rust parser]--> in-memory table   (Rust consumers)
                     \
                      -----> en.json            (JS consumers fetch)
```

The emit step runs at build time (trunk pre_build / build.rs / a cargo call
from a node build script) and is gitignored. A JS page can never disagree
with Rust about the table, because Rust produced what it reads.

*Rejected:* a JS CSV parser. The format has quoted fields, embedded commas
and newlines, `""` escapes and CRLF; two implementations of one format is
the exact shape of the bug that already bit this fleet once, when a CRLF
checkout produced a different content fingerprint and share codes recorded
on Windows were refused on Linux.

### 4. A missing id is loud in dev and visible in release

- **debug**: panic at the call site, naming the id.
- **release**: render a fixed marker *and log it*. A marker only helps if
  someone is looking at the screen; the log line is greppable afterwards.

The marker is a `&'static str`, not `format!("⟨{id}⟩")` — murmur's lesson,
worth inheriting verbatim: *"a lookup in a render loop cannot leak memory one
frame at a time."*

This matches the semantics `vellum-strings` already chose for unfilled
`{slots}`, which last-aeon accepted when it migrated. One rule for both.

### 5. Derived ids: the game supplies the expanded set

Content-driven games build ids like `item.garrote.name` from a structural id,
which a source scan cannot see. murmur's answer is a **prefix allowlist** —
nine prefixes exempted from the orphan check — and it has two blind spots:
delete every venue and `venue.nightclub.name` still looks used; add an item
with no name row and only a content *load* catches it.

Instead, the audit is a library function the game calls **from its own test**,
where the catalogue is already loaded, so it passes the real expansion:

```rust
vellum_strings::audit(&table, AuditInput {
    scan: &["crates/*/src", "gui", "*.html"],
    derived: data.items().map(|i| format!("item.{}.name", i.id)),
})
```

Both checks become exact. An item with no row fails *statically*; a row for a
deleted item shows as an orphan. This is what makes "defined but unused"
trustworthy — and that check is the thing that makes renaming a string safe.

### 6. Content carries no prose, and nowhere to put it

Default: authored data has **no text field at all**; the id is derived as
`<kind>.<structural id>.<field>`. A `display_name:` in a RON file is a hard
load error, not a warning — murmur's property, generalised. Hardcoding prose
is impossible because there is nowhere to write it.

An explicit `name_id:` is allowed only where the convention genuinely cannot
reach, and *that* is the audited case — so the escape hatch stays small and
visible instead of becoming the norm (which is what phoenix's
prose-in-a-TOML-key checker exists to police).

### 7. Whole-sentence rows, CLDR categories, per-locale selector

```
hud.enemies.one     "1 enemy remaining"
hud.enemies.other   "{n} enemies remaining"

en -> |n| if n == 1 { One } else { Other }
ru -> |n| One | Few | Many          (rows added; no code change)
```

Generalised from last-aeon: *"a pluralised string is two whole rows rather
than a sentence glued together from fragments — word order stays the
translator's to choose."* Fragment concatenation is banned and visible in
review; branching on prose stays banned (give presentation a typed value and
let the words be a lookup).

*Rejected:* ICU MessageFormat. It puts a parser and a mini-language in the
crate and stops a row being readable in a spreadsheet cell.

---

## Part 2 — the saves contract (`vellum-save`, new)

### 1. "Save" is two unrelated things; name them

| | what it is | who wants it |
|---|---|---|
| `Progress` | durable state: unlocks, totals, settings | v&t, the-usual, murmur |
| `Run` | a replayable run: seed + log + digest | rogue-hunter, murmur, last-aeon, necessary-work |

A game opts into either, or both.

**On snapshots — corrected 2026-07-26.** This design said a snapshot is an
optional **field of Run**, not a third concept. The implementation did not build
it: `Run<C>` is `{ versions, scenario, seed, commands, ledger }`, with nowhere to
put captured world state. That was not an oversight so much as an absence of
demand — neither Wave 2 consumer below exercises a snapshot, so the field had
nothing to answer to.

project-phoenix-v2 is that missing consumer. Its authoritative world snapshot
(its issues #848 and #862–#867) must serve persistence, campaign continuity,
native hosting, and P2P transfer from one artifact, and it is neither a
`Progress` (not player totals, does not migrate) nor a `Run` as built (no seed
plus log). Two constraints it brings:

- the snapshot must be storable **with no command log** — phoenix's snapshot
  lands before its deterministic-lockstep work, so its first artifact has
  captured state and an empty log and ledger;
- whether that is a field on `Run` or the third concept this section rejected is
  now an open question rather than a settled one, and it is answered by phoenix's
  adoption rather than in advance.

v&t forces the distinction: its input is continuous analog at 64 Hz, so a
"command log" for v&t would be an input recording, not a log. It can never be
a replay game — and it still wants totals to survive a refresh.

*Rejected:* one unified envelope with optional log/snapshot/hashes. v&t would
carry three permanently empty fields, and "optional" multiplies into states
nobody tests.

### 2. Three version dimensions, because they invalidate different things

| dimension | set by | meaning |
|---|---|---|
| format | a developer, manually | the bytes won't deserialise |
| rules | a developer, manually | the sim behaves differently |
| content | **computed**, automatically | authored data moved |

`Run` checks all three and the refusal **names which one moved** — "rules
changed" and "content changed" are different conversations. `Progress` checks
format only, unless it stores content ids and declares that dependency.

Content is a digest rather than a number precisely so nobody has to remember
to bump it (last-aeon and necessary-work both learned this).

Encoding follows the medium: a record carries three named fields; a
size-constrained share code folds them into one compatibility byte, because a
paste cannot carry diagnosis anyway.

### 3. Runs refuse forever; Progress migrates

The asymmetry is **inherent, not a policy choice**, so the crate encodes it:

- `Run` has **no migration hook at all**. A run's whole value is that it
  reproduces what it recorded; a run replayed under new rules is a different
  run. A migrated run is a lie, and the crate should not offer one a friendly
  name.
- `Progress` takes an ordered `vN → vN+1` chain. Additive fields need no
  entry (serde defaults cover them), so the chain only grows when a field is
  restructured or removed.

This replaces "no release has shipped, so bumps carry no migrations" — honest
today, brutal the day something ships.

### 4. `Store` trait unconditional; backends behind features

```
vellum-save            trait Store             (no dependencies)
  feature backend-fs   <data_dir>/<game>/<slot>.ron
  feature backend-web  localStorage["<game>:<slot>"]
  feature run          pulls vellum-replay
```

Follows vellum-perf's `json` precedent rather than vellum-editor's always-on
coupling: a game with its own storage pays nothing, and the five that don't
get a working native+web pair. A `Progress`-only consumer (v&t) never pulls
`vellum-replay`.

### 5. Three digest roles, named separately

| role | question | where |
|---|---|---|
| integrity | did the bytes survive the trip? | CRC-32 in `ShareCodec` (already shipped) |
| corruption | does stored state match itself? | self-hash on `Progress` and any snapshot |
| divergence | did a replay reproduce the recording? | ledger on `Run` |

The divergence ledger keeps the final hash always and periodic hashes every
N ticks optionally (0 = off). necessary-work's 20,000-tick runs get
`Diverged { at_tick: 1200 }`; rogue-hunter's short runs pay nothing. A
contract that conflates these ends up with one field doing a job it cannot do.

### 6. necessary-work aligns onto `vellum-replay` — correcting an earlier call

Its spec currently records that `vellum-replay`'s `Simulation` trait was
"deliberately not adopted" because the game's log is tick-stamped real time
rather than command-driven. **That judgement was wrong and is to be revised.**

The trait fits, with no change to the sacred crate, once the tick is
understood as part of the *command* rather than something a driver schedules:

- `type Command = LoggedCommand { tick, command }` — which is what a stamped
  command already is.
- `apply` = advance to that tick, then execute.
- The sim **samples its own hash ledger** while advancing, so periodic hashes
  need no driver callback.
- The tail to `final_tick` is driven by a `Sampling::advance_to` on the saves
  crate, called once after the log is exhausted.

**Correction, found while building wave 2.** This section first said
`needs_continuation` / `continue_step` would drive the tail. They will not:
`replay_into` pumps continuations *before every command*, not only after the
last one, so a sim reporting "not yet at the end tick" plays the entire run
out before its first command arrives — every command then lands at the final
tick, and the digests disagree for a reason that looks nothing like the cause.
That hook means "a multi-turn action is mid-resolution" and still does. The
tail needs its own method; it doubles as the advance-to-tick half of `apply`,
so a game implements one thing rather than two.

This *deletes* necessary-work's bespoke replay loop rather than complicating
anything, and it makes `Run` the same shape for every game that has one.

---

## Sequencing

Each wave lands a crate with real driving consumers, per
`new-crates-need-a-consumer`.

**Wave 1 — strings** (the finished design; touches every game eventually)
1. `vellum-strings` grows: locale shape, table + plural selector, the
   three-language audit, the JSON emit.
2. **v&t adopts greenfield** — proves the hard path: Rust, JS and HTML
   consumers in one game, with no prior table to lean on.
3. **murmur migrates** — proves it against the richest existing table:
   derived ids, `tr!`/`trf!`, and the orphan check that must stay trustworthy.

**Wave 2 — saves**
1. `vellum-save`: `Progress`, `Run`, the gate, the ledger, the backends.
2. **v&t adopts `Progress`** — totals, unlocks, settings; native + web.
3. **necessary-work adopts `Run`** and aligns onto `vellum-replay`, revising
   its `vellum-adoption` decision.
4. **phoenix adopts the snapshot** — the third consumer, added 2026-07-26 and
   the reason §1's correction exists. It settles where captured world state
   lives, and it is the only consumer that exercises a stored snapshot at all.
   Its tree is `#862` (the bounded tracer) → `#863`/`#864` (dynamic and
   scenario state) → `#865`/`#866` (slots on `Store`, file export through the
   same record), with `#867`'s campaign projection staying in phoenix.

## Known gaps, stated rather than hidden

- **v&t exercises `Progress` only.** It has no meta-progression at all
  (`Plunder { ships_boarded }` and `Encounter { wave, .. }`, both reset per
  run), so it can never test `Run`. necessary-work is the `Run` driver for
  exactly this reason.
- **Nothing in the original Wave 2 exercised a snapshot**, which is why the
  field went unbuilt — see §1. Phoenix closes that gap, and until its adoption
  lands, "a snapshot is a field of `Run`" is a design intention with no code and
  no test behind it. Stated here rather than left to be discovered.
- **`Store` is text, so a snapshot is text.** RON keeps a save diagnosable by
  hand, and that is the right trade for five games storing small records. A game
  whose authoritative state is large enough to want a dense binary encoding
  would be the case that reopens the choice; phoenix's world snapshot is the
  first candidate and has not yet been measured.
- **The JSON emit adds a build-order constraint** to any game whose page is
  assembled outside cargo (phoenix's `build-client.mjs`). Real cost, accepted
  in exchange for one parser.
- **Locale selectors beyond English are untested** until a second locale
  exists. The design admits them; nothing proves them yet.
