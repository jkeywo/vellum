# Composition — the entity/world data scheme

project-phoenix-v2 grew a data model worth spreading, independent of its
format. The scheme, format-neutrally:

**Entity templates.** Reusable definitions (`assets/entities/` in phoenix —
ships, stations, planets) that world files reference by path or id. A
template is a complete, valid definition on its own.

**Instances with overrides.** A world file places instances of templates and
overrides individual fields per instance. The merge is shallow and explicit:
an override replaces the authored value, never appends to it, and a field
absent from the override keeps the template's value.

**Layered worlds.** A root world can pull in supporting worlds
(`extra_worlds` in phoenix), stacked in declared order. Later layers add to
or override earlier ones; the root world is the single entry point a player
selects.

**A thin manifest.** The selectable-roots list (`scenarios.toml`) is an
index only; display metadata is single-sourced in each world's own file so
the manifest can never disagree with the world.

**Overlay packs.** A pack overlays base content *by exact authored path*,
is session-scoped, and is rejected whole if its manifest is invalid —
all-or-nothing, never a half-applied overlay.

**Validation is total.** Every reference resolves or loading fails loudly at
startup/validate time, not at first use. Unknown fields are rejected, not
ignored — a typo'd field name is a silent data loss otherwise.

## Adoption

Adopt the *scheme* in your game's own format — RON and Rhai games do not
switch to TOML for this. The natural first adopters are void-and-thunder's
data editor and the-usual's growing content set.

A shared crate (`vellum-compose`: format-neutral template resolution,
override merge, layer stacking, overlay application over serde values) is
built only when void-and-thunder's editor needs it, per
`new-crates-need-a-consumer`. Phoenix's implementation is the design source
and stays home — its `config.rs` and flag machinery are game schema, not
mechanism.
