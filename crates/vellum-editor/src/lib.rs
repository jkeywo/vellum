//! The fleet's runtime data-editor mechanism.
//!
//! Extracted from void-and-thunder's design panel, which is the shape a
//! data-driven game's in-game editor keeps arriving at: **one reflection
//! walker** that renders any tuning struct as paired slider-and-textbox
//! widgets, a **descriptor table** supplying the two things a type cannot
//! say about its own fields, and a **sparse save path** that writes back
//! only what differs from the defaults.
//!
//! What is here is the mechanism. What stays in the game is everything that
//! is *about that game*: the panel's chrome and layout, which resources it
//! offers, and — importantly — the descriptor table's **contents**. This
//! crate defines what a descriptor is and how one is looked up; only the
//! game knows that a thrust runs to 3000 and that `Broadside.port` is a
//! reload timer nobody should be typing into.
//!
//! # Two things reflection cannot tell you
//!
//! - **A sensible range.** The type says `f32`, not "0 to 3000". A slider
//!   needs the second.
//! - **Authored config versus live state.** A ship's class carries both its
//!   authored damage *and* its half-spent reload timer. Writing the latter
//!   from an editor hands every future ship of that class a broken starting
//!   state, and saving it puts mid-combat values in an authored file.
//!
//! Anything the table is silent about still works: a heuristic range, and
//! treated as config. That degradation is deliberate — a new field being
//! *badly ranged* is a small problem; a new field being *invisible* is the
//! problem an editor exists to avoid.
//!
//! # Version coupling
//!
//! This crate is version-coupled to its consumer in a way no other vellum
//! crate is: a reflected value and an [`egui::Ui`] cross the boundary, so
//! the consumer's `bevy_reflect` and the `egui` its `bevy_egui` re-exports
//! must be the versions pinned here (the fleet standard, Bevy 0.19 /
//! bevy_egui 0.41). A mismatch is a type error at the call site, which is
//! the failure mode worth having: loud, immediate, and impossible to ship.

use bevy_reflect::{PartialReflect, ReflectMut, ReflectRef};
use serde::Serialize;

/// The composition mechanism this crate's save path is built on, re-exported
/// so an editor needs one dependency rather than two: a consumer that
/// renders a sparse override also needs to *write* it, and that renderer is
/// `vellum-compose`'s.
pub use vellum_compose::{self, write_ron, ComposeError};

/// Whether a field is authored, or belongs to a running instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Authored: editable, saved to disk, pushed onto live instances.
    Config,
    /// Live state: shown greyed out, never written by an edit, never saved.
    Live,
}

/// Editor metadata for one field.
#[derive(Debug, Clone, Copy)]
pub struct FieldSpec {
    pub min: f32,
    pub max: f32,
    pub kind: FieldKind,
}

impl FieldSpec {
    /// An authored field with a slider range.
    pub const fn config(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            kind: FieldKind::Config,
        }
    }

    /// A live-state field: shown, never written.
    pub const fn live() -> Self {
        Self {
            min: 0.0,
            max: 0.0,
            kind: FieldKind::Live,
        }
    }

    /// The fallback for a field the table is silent about: zero to twice the
    /// current value (or 1, at zero). Wrong often enough to want tuning,
    /// never so wrong the field is unusable — and the text box beside every
    /// slider means the exact value is always reachable regardless.
    pub fn heuristic(current: f32) -> Self {
        Self::config(0.0, (current.abs() * 2.0).max(1.0))
    }
}

/// A game's field descriptors: entries keyed by **owning type and field**.
///
/// Keyed by owner, not by bare field name, because bare names collide and
/// the collisions matter: one game had `Broadside.damage` (authored) and
/// `EmpDefense.damage` (how much EMP a ship has soaked), and conflating them
/// wiped a ship's EMP load every time anything else was retuned.
pub struct SpecTable {
    entries: &'static [(&'static str, &'static str, FieldSpec)],
}

impl SpecTable {
    pub const fn new(entries: &'static [(&'static str, &'static str, FieldSpec)]) -> Self {
        Self { entries }
    }

    /// The spec for a field, falling back to [`FieldSpec::heuristic`].
    pub fn spec_for(&self, owner: &str, field: &str, current: f32) -> FieldSpec {
        for (o, f, spec) in self.entries {
            if *o == owner && *f == field {
                return *spec;
            }
        }
        FieldSpec::heuristic(current)
    }
}

/// The short type name of a reflected value — `"Broadside"`, not
/// `game::combat::Broadside` — which is how [`SpecTable`] keys its entries.
///
/// Returns `""` for a value whose type is not registered, which simply means
/// every field of it falls back to the heuristic range.
pub fn owner_of(value: &dyn PartialReflect) -> &'static str {
    value
        .get_represented_type_info()
        .map(|info| info.type_path_table().short_path())
        .unwrap_or("")
}

/// Draw `value`'s fields as editable widgets. Returns true if anything
/// changed.
///
/// `editable` is false for a live-state subtree, which greys the whole thing
/// out rather than letting an edit stamp a reload timer onto a running
/// instance.
pub fn edit_value(
    ui: &mut egui::Ui,
    table: &SpecTable,
    name: &str,
    value: &mut dyn PartialReflect,
    editable: bool,
) -> bool {
    // The top of a walk has no parent struct to key the descriptor table
    // against, so a leaf here falls back to the heuristic range. Everything
    // an editor shows at the top level is a struct, so this never bites.
    edit_field(ui, table, name, value, editable, None)
}

/// One field. `spec` comes from the *parent* struct's entry in the table,
/// which is what lets two same-named fields on different types be told
/// apart.
fn edit_field(
    ui: &mut egui::Ui,
    table: &SpecTable,
    name: &str,
    value: &mut dyn PartialReflect,
    editable: bool,
    spec: Option<FieldSpec>,
) -> bool {
    // Leaves first: most fields are numbers, and checking them here keeps
    // the struct recursion below from knowing about primitives at all.
    if let Some(v) = value.try_downcast_mut::<f32>() {
        return edit_f32(ui, table, name, v, editable, spec);
    }
    if let Some(v) = value.try_downcast_mut::<u32>() {
        let mut f = *v as f32;
        if edit_f32(ui, table, name, &mut f, editable, spec) {
            *v = f.round().max(0.0) as u32;
            return true;
        }
        return false;
    }
    if let Some(v) = value.try_downcast_mut::<usize>() {
        let mut f = *v as f32;
        if edit_f32(ui, table, name, &mut f, editable, spec) {
            *v = f.round().max(0.0) as usize;
            return true;
        }
        return false;
    }
    if let Some(v) = value.try_downcast_mut::<bool>() {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(editable, |ui| {
                changed = ui.checkbox(v, name).changed();
            });
        });
        return changed;
    }

    // Not a leaf: recurse into a struct's fields under a collapsing header.
    // Anything else is shown read-only rather than skipped, so an
    // unsupported field is visible-but-inert instead of silently absent.
    let is_struct = matches!(value.reflect_ref(), ReflectRef::Struct(_));
    if !is_struct {
        ui.horizontal(|ui| {
            ui.label(name);
            ui.weak("(not editable)");
        });
        return false;
    }

    let mut changed = false;
    egui::CollapsingHeader::new(name)
        .default_open(true)
        .show(ui, |ui| {
            let owner = owner_of(&*value).to_string();
            let ReflectMut::Struct(s) = value.reflect_mut() else {
                return;
            };
            for i in 0..s.field_len() {
                // `name_at` and `field_at_mut` cannot be held at once (one
                // borrows immutably, the other mutably), so take the name
                // first.
                let field_name = s.name_at(i).unwrap_or("?").to_string();
                let field_spec = table.spec_for(&owner, &field_name, 0.0);
                // A whole subtree marked Live is greyed out, not hidden:
                // seeing a reload timer tick is useful, writing it is not.
                let live = field_spec.kind == FieldKind::Live;
                let Some(field) = s.field_at_mut(i) else {
                    continue;
                };
                changed |= edit_field(
                    ui,
                    table,
                    &field_name,
                    field,
                    editable && !live,
                    Some(field_spec),
                );
            }
        });
    changed
}

/// One number: a slider for feel, a drag box for precision. Both edit the
/// same value, so you can sweep to find the shape and then type the exact
/// figure.
fn edit_f32(
    ui: &mut egui::Ui,
    table: &SpecTable,
    name: &str,
    value: &mut f32,
    editable: bool,
    spec: Option<FieldSpec>,
) -> bool {
    let spec = spec.unwrap_or_else(|| table.spec_for("", name, *value));
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(editable, |ui| {
            ui.label(name);
            // The slider is clamped to the spec's range; the drag box is
            // not, so a value outside the table's guess is still reachable
            // by typing.
            changed |= ui
                .add(
                    egui::Slider::new(value, spec.min..=spec.max)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Never),
                )
                .changed();
            changed |= ui
                .add(egui::DragValue::new(value).speed(drag_speed(spec.min, spec.max)))
                .changed();
        });
    });
    changed
}

/// Drag granularity scaled to the field's range, so a 0..1 fraction moves in
/// hundredths while a 0..3000 thrust moves in whole units.
fn drag_speed(min: f32, max: f32) -> f64 {
    ((max - min) as f64 / 300.0).max(0.001)
}

/// A serializable value's RON-value form, for composing and diffing.
pub fn value_of<T: Serialize>(value: &T) -> Result<ron::Value, vellum_compose::ComposeError> {
    let config = ron::ser::PrettyConfig::new().struct_names(false);
    let text = ron::ser::to_string_pretty(value, config)
        .map_err(|_| vellum_compose::ComposeError::NotAMap("an unserializable value"))?;
    vellum_compose::parse(&text)
}

/// The sparse override an edited value represents against its defaults.
///
/// The authored-file contract in every data-driven game the fleet has is
/// that a definition says only how it *differs* from the compiled-in
/// defaults. An editor that serialized whole edited values would destroy
/// that on the first save, entombing every default beside the one field
/// somebody moved. This is the counter: diff in value-space, write what
/// differs, and let a value equal to its defaults render as the
/// take-the-defaults idiom.
pub fn sparse_override<T: Serialize>(
    defaults: &T,
    edited: &T,
) -> Result<ron::Value, vellum_compose::ComposeError> {
    vellum_compose::diff(&value_of(defaults)?, &value_of(edited)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_reflect::Reflect;

    #[derive(Reflect, Debug, Clone, PartialEq, Serialize)]
    struct Weapon {
        damage: f32,
        guns: u32,
        timer: f32,
    }

    #[derive(Reflect, Debug, Clone, PartialEq, Serialize)]
    struct Armour {
        /// Same *name* as the weapon's, opposite meaning: soaked, not dealt.
        damage: f32,
        resist: f32,
    }

    static TABLE: SpecTable = SpecTable::new(&[
        ("Weapon", "damage", FieldSpec::config(0.0, 100.0)),
        ("Weapon", "guns", FieldSpec::config(1.0, 12.0)),
        ("Weapon", "timer", FieldSpec::live()),
        ("Armour", "damage", FieldSpec::live()),
    ]);

    #[test]
    fn the_same_field_name_can_mean_different_things() {
        assert_eq!(
            TABLE.spec_for("Weapon", "damage", 0.0).kind,
            FieldKind::Config
        );
        assert_eq!(
            TABLE.spec_for("Armour", "damage", 0.0).kind,
            FieldKind::Live,
            "keying by owner is what stops an edit wiping live state"
        );
    }

    #[test]
    fn an_unknown_field_still_gets_a_usable_range() {
        let spec = TABLE.spec_for("Weapon", "added_next_week", 40.0);
        assert_eq!(spec.kind, FieldKind::Config);
        assert!(
            spec.min <= 40.0 && spec.max >= 40.0,
            "range must contain it"
        );
        assert!(spec.max > spec.min, "a zero-width slider is unusable");
        assert!(
            TABLE.spec_for("Nothing", "at_all", 0.0).max > 0.0,
            "even a zero-valued unknown gets a non-empty range"
        );
    }

    #[test]
    fn owner_names_are_short_paths() {
        let weapon = Weapon {
            damage: 1.0,
            guns: 2,
            timer: 0.0,
        };
        assert_eq!(owner_of(&weapon), "Weapon", "short path, not the full path");
    }

    /// The walker's whole justification: it must find every field of a
    /// struct it has never heard of.
    #[test]
    fn reflection_exposes_every_field() {
        let mut weapon = Weapon {
            damage: 1.0,
            guns: 2,
            timer: 0.5,
        };
        let ReflectMut::Struct(s) = weapon.reflect_mut() else {
            panic!("a struct should reflect as one");
        };
        let names: Vec<&str> = (0..s.field_len()).filter_map(|i| s.name_at(i)).collect();
        assert_eq!(names, ["damage", "guns", "timer"]);
    }

    /// Integers must round-trip through the walker's downcast, or edits
    /// would be silently dropped for every count-like field.
    #[test]
    fn integer_fields_downcast() {
        let mut weapon = Weapon {
            damage: 1.0,
            guns: 2,
            timer: 0.0,
        };
        let ReflectMut::Struct(s) = weapon.reflect_mut() else {
            panic!()
        };
        let idx = (0..s.field_len())
            .find(|i| s.name_at(*i) == Some("guns"))
            .expect("guns field");
        assert!(
            s.field_at_mut(idx)
                .unwrap()
                .try_downcast_mut::<u32>()
                .is_some(),
            "a count must be reachable as a u32 or the editor cannot edit it"
        );
    }

    #[test]
    fn a_sparse_override_says_only_what_changed() {
        let defaults = Weapon {
            damage: 10.0,
            guns: 3,
            timer: 0.0,
        };
        let edited = Weapon {
            damage: 25.0,
            ..defaults.clone()
        };
        let overrides = sparse_override(&defaults, &edited).expect("diffs");
        assert_eq!(
            overrides,
            vellum_compose::parse("( damage: 25.0 )").unwrap(),
            "only the moved field belongs in the authored file"
        );
        // And it composes back to exactly what was edited.
        let composed = vellum_compose::apply(&value_of(&defaults).unwrap(), &overrides).unwrap();
        assert_eq!(composed, value_of(&edited).unwrap());
    }

    #[test]
    fn an_unedited_value_overrides_nothing() {
        let defaults = Weapon {
            damage: 10.0,
            guns: 3,
            timer: 0.0,
        };
        let overrides = sparse_override(&defaults, &defaults.clone()).expect("diffs");
        assert_eq!(overrides, ron::Value::Unit, "the take-the-defaults idiom");
    }
}
