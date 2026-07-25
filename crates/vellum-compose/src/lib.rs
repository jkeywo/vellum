//! The fleet's content-composition mechanism.
//!
//! project-phoenix-v2 grew the scheme — entity **templates** referenced by
//! worlds, **per-instance overrides**, **layered** world files, and
//! **overlay packs** applied all-or-nothing by authored path — and the fleet
//! decided the scheme is a shared pipeline rather than a convention
//! (`content-pipeline-is-in-scope` in vellum's spec). This crate is the
//! mechanism, extracted as *shape*: phoenix's implementation stays home,
//! entangled with its own game schema, and void-and-thunder's data — ship
//! classes today, its data editor next — is the driving consumer.
//!
//! The rules, exactly as the handbook writes them (`docs/handbook/composition.md`):
//!
//! - A template is a complete, valid definition on its own.
//! - **The merge is shallow and explicit**: an override replaces the
//!   authored value, never appends to it; a field absent from the override
//!   keeps the template's value. (Deep merge is a decision nobody has made,
//!   and this crate refuses to make it by accident.)
//! - Layers stack in declared order; later layers override earlier ones,
//!   one shallow key at a time.
//! - An overlay replaces content *by exact authored path* and is rejected
//!   whole if any path is unknown — all-or-nothing, never half-applied.
//! - Every reference resolves or composition fails loudly, before use.
//!
//! Values are RON values — the format of the first consumer. The mechanism
//! underneath is value-level map merge; a TOML front-end arrives when a
//! TOML game migrates onto the crate.

use std::collections::BTreeMap;

use ron::Value;
use serde::de::DeserializeOwned;

/// What went wrong while composing. Every variant names the thing that was
/// wrong, because a composition error is an authoring error and the author
/// needs the name, not a position.
#[derive(Debug)]
pub enum ComposeError {
    /// A reference named a template the catalog does not hold.
    UnknownTemplate(String),
    /// A merge was asked to treat a non-map as a map.
    NotAMap(&'static str),
    /// An overlay named an authored path that does not exist. The overlay
    /// was not applied — not even the paths that did exist.
    OverlayPathUnknown(String),
    /// A diff was asked to express the *removal* of a field, which a shallow
    /// override cannot say.
    DiffRemoval(String),
    /// The composed value did not deserialize into the requested type.
    Extract(ron::error::Error),
    /// A source string did not parse as RON.
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComposeError::UnknownTemplate(name) => write!(f, "unknown template '{name}'"),
            ComposeError::NotAMap(what) => write!(f, "{what} is not a map"),
            ComposeError::OverlayPathUnknown(path) => {
                write!(
                    f,
                    "overlay names unknown path '{path}'; nothing was applied"
                )
            }
            ComposeError::DiffRemoval(key) => {
                write!(
                    f,
                    "diff cannot express removing {key}: a shallow override                      has no way to say \"delete this field\""
                )
            }
            ComposeError::Extract(error) => write!(f, "composed value did not extract: {error}"),
            ComposeError::Parse(error) => write!(f, "source did not parse: {error}"),
        }
    }
}

impl std::error::Error for ComposeError {}

/// Parse RON text into a composable value.
pub fn parse(text: &str) -> Result<Value, ComposeError> {
    ron::from_str(text).map_err(ComposeError::Parse)
}

/// Deserialize a composed value into the game's own type — the typed exit
/// from value-space, where the game's schema (including its
/// unknown-field policy) gets the final word.
///
/// One normalization happens on the way out: RON's `field: ()` idiom — "this
/// struct, all defaults" — parses into the value layer as a bare unit, which
/// cannot deserialize into a struct. Extraction re-interprets unit values
/// *nested inside* maps and sequences as empty structs, which is what the
/// author wrote. The root value is left alone (a unit root is the "no
/// override" idiom, not an empty struct), and genuinely-unit-typed fields do
/// not occur in authored game data.
pub fn extract<T: DeserializeOwned>(value: Value) -> Result<T, ComposeError> {
    normalize_units(value, false)
        .into_rust()
        .map_err(ComposeError::Extract)
}

/// See [`extract`]: nested units become empty structs (empty maps at the
/// value level); the root keeps its meaning.
fn normalize_units(value: Value, nested: bool) -> Value {
    match value {
        Value::Unit if nested => Value::Map(ron::Map::new()),
        Value::Map(map) => Value::Map(
            map.into_iter()
                .map(|(key, inner)| (key, normalize_units(inner, true)))
                .collect(),
        ),
        Value::Seq(seq) => Value::Seq(
            seq.into_iter()
                .map(|inner| normalize_units(inner, true))
                .collect(),
        ),
        Value::Option(Some(inner)) => Value::Option(Some(Box::new(normalize_units(*inner, true)))),
        other => other,
    }
}

/// Render a composed value as RON text in the authored style.
///
/// `ron`'s own serializer writes a [`Value::Map`] as a brace-map with quoted
/// keys (`{ "hull": 1.0 }`), because the value layer cannot know a map used
/// to be a struct. Authored game data is structs, written `(hull: 1.0)`, and
/// an editor that saved files in a different dialect than authors write them
/// would make every save a rewrite. This renderer keeps the authored style:
/// maps as parenthesised bodies with bare identifier keys, sequences in
/// brackets, and every leaf delegated to `ron`'s serializer so numbers and
/// strings render exactly as it would.
///
/// Map keys must be identifier-like strings (authored field names); anything
/// else is reported, because it did not come from a struct.
pub fn write_ron(value: &Value) -> Result<String, ComposeError> {
    let mut out = String::new();
    render(value, 0, &mut out)?;
    out.push('\n');
    Ok(out)
}

fn render(value: &Value, indent: usize, out: &mut String) -> Result<(), ComposeError> {
    let pad = "    ".repeat(indent + 1);
    let close = "    ".repeat(indent);
    match value {
        Value::Map(map) => {
            if map.is_empty() {
                out.push_str("()");
                return Ok(());
            }
            out.push_str(
                "(
",
            );
            for (key, inner) in map.iter() {
                let Value::String(name) = key else {
                    return Err(ComposeError::NotAMap("a non-string map key"));
                };
                let identifier =
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !identifier {
                    return Err(ComposeError::NotAMap("a non-identifier field name"));
                }
                out.push_str(&pad);
                out.push_str(name);
                out.push_str(": ");
                render(inner, indent + 1, out)?;
                out.push_str(
                    ",
",
                );
            }
            out.push_str(&close);
            out.push(')');
        }
        Value::Seq(seq) => {
            if seq.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str(
                "[
",
            );
            for inner in seq {
                out.push_str(&pad);
                render(inner, indent + 1, out)?;
                out.push_str(
                    ",
",
                );
            }
            out.push_str(&close);
            out.push(']');
        }
        Value::Option(Some(inner)) => {
            out.push_str("Some(");
            render(inner, indent, out)?;
            out.push(')');
        }
        leaf => out.push_str(
            &ron::ser::to_string(leaf)
                .map_err(|_| ComposeError::NotAMap("an unserializable leaf"))?,
        ),
    }
    Ok(())
}

/// Named templates. A catalog is the composition scheme's noun for "the
/// reusable definitions this content set may reference".
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    templates: BTreeMap<String, Value>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a template.
    pub fn insert(&mut self, name: &str, template: Value) {
        self.templates.insert(name.to_owned(), template);
    }

    /// Resolve a reference, loudly.
    pub fn resolve(&self, name: &str) -> Result<&Value, ComposeError> {
        self.templates
            .get(name)
            .ok_or_else(|| ComposeError::UnknownTemplate(name.to_owned()))
    }

    /// Resolve a reference and apply a per-instance override in one step —
    /// the scheme's most common composition.
    pub fn instantiate(&self, name: &str, overrides: &Value) -> Result<Value, ComposeError> {
        apply(self.resolve(name)?, overrides)
    }

    /// Template names, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(String::as_str)
    }
}

/// Apply a shallow override: every key present in `overrides` replaces the
/// base's value wholesale; every absent key keeps the base's. A unit value
/// as override means "no overrides" and returns the base unchanged, so an
/// optional override field composes without ceremony.
pub fn apply(base: &Value, overrides: &Value) -> Result<Value, ComposeError> {
    if matches!(overrides, Value::Unit) {
        return Ok(base.clone());
    }
    let Value::Map(base_map) = base else {
        return Err(ComposeError::NotAMap("override base"));
    };
    let Value::Map(override_map) = overrides else {
        return Err(ComposeError::NotAMap("override"));
    };
    let mut merged = base_map.clone();
    for (key, value) in override_map.iter() {
        merged.insert(key.clone(), value.clone());
    }
    Ok(Value::Map(merged))
}

/// The inverse of [`apply`]: the shallow override that takes `base` to
/// `merged`.
///
/// This is the editor's save path: a panel edits a fully-materialised value,
/// and what belongs in the authored file is only what *differs* from the
/// template — "a class only needs to say how it differs" survives being
/// edited. Identical values diff to a unit ("no override"), so
/// `apply(base, diff(base, merged)?) == merged` holds for every pair of maps
/// with the same key set.
///
/// A key present in `base` but absent from `merged` is unrepresentable as a
/// shallow override (there is no "remove this field" in the scheme), and is
/// reported rather than silently dropped.
pub fn diff(base: &Value, merged: &Value) -> Result<Value, ComposeError> {
    let Value::Map(base_map) = base else {
        return Err(ComposeError::NotAMap("diff base"));
    };
    let Value::Map(merged_map) = merged else {
        return Err(ComposeError::NotAMap("diff target"));
    };
    for key in base_map.keys() {
        if merged_map.get(key).is_none() {
            return Err(ComposeError::DiffRemoval(format!("{key:?}")));
        }
    }
    let mut overrides = ron::Map::new();
    for (key, value) in merged_map.iter() {
        if base_map.get(key) != Some(value) {
            overrides.insert(key.clone(), value.clone());
        }
    }
    if overrides.is_empty() {
        Ok(Value::Unit)
    } else {
        Ok(Value::Map(overrides))
    }
}

/// Stack layers in declared order: the first layer is the base, each later
/// layer overrides it one shallow key at a time.
pub fn stack<'a>(layers: impl IntoIterator<Item = &'a Value>) -> Result<Value, ComposeError> {
    let mut layers = layers.into_iter();
    let Some(first) = layers.next() else {
        return Err(ComposeError::NotAMap("empty layer stack"));
    };
    let mut composed = first.clone();
    for layer in layers {
        composed = apply(&composed, layer)?;
    }
    Ok(composed)
}

/// An overlay pack: replacements by exact authored path. Session-scoped by
/// convention — the caller owns when it applies and when it is cleared.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    replacements: BTreeMap<String, Value>,
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, replacement: Value) {
        self.replacements.insert(path.to_owned(), replacement);
    }

    /// Apply to a content set keyed by authored path — all-or-nothing.
    /// Every overlay path is checked against the content set before any
    /// replacement happens, so a rejected overlay leaves the base exactly
    /// as it was.
    pub fn apply_to(&self, content: &mut BTreeMap<String, Value>) -> Result<(), ComposeError> {
        for path in self.replacements.keys() {
            if !content.contains_key(path) {
                return Err(ComposeError::OverlayPathUnknown(path.clone()));
            }
        }
        for (path, replacement) in &self.replacements {
            content.insert(path.clone(), replacement.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn template() -> Value {
        parse(r#"( hull: 100.0, speed: 12.0, name: "patrol" )"#).unwrap()
    }

    #[test]
    fn an_override_replaces_named_fields_and_keeps_the_rest() {
        let composed = apply(&template(), &parse("( hull: 40.0 )").unwrap()).unwrap();
        let Value::Map(map) = composed else { panic!() };
        let get = |k: &str| map.get(&Value::String(k.into())).cloned().unwrap();
        assert_eq!(get("hull"), parse("40.0").unwrap(), "named field replaced");
        assert_eq!(get("speed"), parse("12.0").unwrap(), "absent field kept");
    }

    #[test]
    fn the_merge_is_shallow_a_nested_map_is_replaced_wholesale() {
        let base = parse("( drive: ( thrust: 10.0, turn: 2.0 ) )").unwrap();
        let composed = apply(&base, &parse("( drive: ( thrust: 5.0 ) )").unwrap()).unwrap();
        let Value::Map(map) = composed else { panic!() };
        let drive = map.get(&Value::String("drive".into())).unwrap();
        assert_eq!(
            drive,
            &parse("( thrust: 5.0 )").unwrap(),
            "the nested turn is GONE: an override replaces the authored \
             value, never merges into it — deep merge is a decision nobody \
             has made"
        );
    }

    #[test]
    fn a_unit_override_is_no_override() {
        assert_eq!(apply(&template(), &Value::Unit).unwrap(), template());
    }

    #[test]
    fn catalogs_resolve_loudly() {
        let mut catalog = Catalog::new();
        catalog.insert("house_patrol", template());
        assert!(catalog.resolve("house_patrol").is_ok());
        let missing = catalog.instantiate("corsair_sloop", &Value::Unit);
        assert!(
            matches!(missing, Err(ComposeError::UnknownTemplate(name)) if name == "corsair_sloop")
        );
    }

    #[test]
    fn layers_stack_in_declared_order() {
        let base = parse("( a: 1, b: 1, c: 1 )").unwrap();
        let second = parse("( b: 2 )").unwrap();
        let third = parse("( b: 3, c: 3 )").unwrap();
        let composed = stack([&base, &second, &third]).unwrap();
        assert_eq!(composed, parse("( a: 1, b: 3, c: 3 )").unwrap());
    }

    #[test]
    fn overlays_are_all_or_nothing() {
        let mut content: BTreeMap<String, Value> = BTreeMap::new();
        content.insert("data/ships.ron".into(), template());
        let before = content.clone();

        let mut overlay = Overlay::new();
        overlay.insert("data/ships.ron", parse("( hull: 1.0 )").unwrap());
        overlay.insert("data/missing.ron", Value::Unit);
        let refused = overlay.apply_to(&mut content);
        assert!(matches!(
            refused,
            Err(ComposeError::OverlayPathUnknown(path)) if path == "data/missing.ron"
        ));
        assert_eq!(content, before, "a rejected overlay applies nothing");

        let mut good = Overlay::new();
        good.insert("data/ships.ron", parse("( hull: 1.0 )").unwrap());
        good.apply_to(&mut content).unwrap();
        assert_eq!(content["data/ships.ron"], parse("( hull: 1.0 )").unwrap());
    }

    #[test]
    fn diff_is_the_inverse_of_apply() {
        let base = template();
        let edited = apply(&base, &parse("( hull: 40.0 )").unwrap()).unwrap();
        let overrides = diff(&base, &edited).unwrap();
        assert_eq!(overrides, parse("( hull: 40.0 )").unwrap());
        assert_eq!(
            apply(&base, &overrides).unwrap(),
            edited,
            "apply(base, diff(base, merged)) must reproduce merged"
        );
    }

    #[test]
    fn an_unedited_value_diffs_to_no_override() {
        let overrides = diff(&template(), &template()).unwrap();
        assert_eq!(overrides, Value::Unit, "identical values need no override");
        assert_eq!(apply(&template(), &overrides).unwrap(), template());
    }

    #[test]
    fn a_removed_field_is_reported_not_dropped() {
        let smaller = parse("( hull: 100.0 )").unwrap();
        assert!(matches!(
            diff(&template(), &smaller),
            Err(ComposeError::DiffRemoval(_))
        ));
    }

    #[test]
    fn written_ron_keeps_the_authored_style_and_round_trips() {
        let value = parse(
            r#"( hull: 40.0, name: "patrol", drive: ( thrust: 10.0 ), tags: [1, 2], extra: (), opt: Some(3) )"#,
        )
        .unwrap();
        let text = write_ron(&value).unwrap();
        assert!(
            text.contains("hull: 40.0") && text.contains('(') && !text.contains('{'),
            "authored struct style, never brace-maps:
{text}"
        );
        assert_eq!(parse(&text).unwrap(), value, "written RON must round-trip");
    }

    #[test]
    fn extraction_is_the_typed_exit() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Ship {
            hull: f64,
            speed: f64,
            name: String,
        }
        let composed = apply(&template(), &parse("( hull: 40.0 )").unwrap()).unwrap();
        let ship: Ship = extract(composed).unwrap();
        assert_eq!(
            ship,
            Ship {
                hull: 40.0,
                speed: 12.0,
                name: "patrol".into()
            }
        );
    }

    #[test]
    fn a_unit_field_extracts_as_a_defaulted_struct() {
        // RON's `field: ()` idiom: "this struct, all defaults". The value
        // layer flattens it to a unit; extraction restores the meaning.
        #[derive(Debug, Default, PartialEq, Deserialize)]
        #[serde(default)]
        struct Drive {
            thrust: f64,
        }
        #[derive(Debug, PartialEq, Deserialize)]
        struct Ship {
            hull: f64,
            drive: Drive,
        }
        let value = parse("( hull: 50.0, drive: () )").unwrap();
        let ship: Ship = extract(value).unwrap();
        assert_eq!(ship.drive, Drive::default());
        assert_eq!(ship.hull, 50.0);
    }

    #[test]
    fn extraction_failures_carry_the_reason() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Strict {
            hull: f64,
        }
        let wrong = parse(r#"( hull: "not a number" )"#).unwrap();
        assert!(matches!(
            extract::<Strict>(wrong),
            Err(ComposeError::Extract(_))
        ));
    }
}
