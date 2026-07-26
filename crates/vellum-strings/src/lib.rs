//! The fleet's localisation pipeline: authored text, looked up by id, and
//! audited so a rename is safe.
//!
//! Four games grew most of this separately — a CSV of `id, context, text`,
//! `{name}` slots, `[brackets]` marking prose no human has approved, and a
//! check that every id used exists and every row is reached. One of them
//! wrote the check in JavaScript. This is that pipeline, once.
//!
//! # Why this crate can move freely
//!
//! Nothing here can touch a save format. rogue-hunter excludes its string
//! table from the content fingerprint and marks rendered log text
//! `#[serde(skip)]` precisely so copy edits and translations cannot move a
//! digest; murmur's fingerprint is over `World`, which holds ids rather than
//! prose. That is what makes text the one part of the content pipeline that
//! can change without a determinism argument — and it is a property each
//! game must keep, not one this crate can enforce.
//!
//! # The shape
//!
//! - [`Table`] — one locale's rows, parsed from CSV, looked up by id.
//! - [`Locale`] and [`Category`] — plural categories and the rule that picks
//!   one. English ships; a locale that needs `Few` adds a selector and rows.
//! - [`audit`] — the check, across `.rs`, `.js` and `.html`, with the
//!   game supplying ids it builds at runtime.
//! - [`Table::to_json`] — what a page fetches, so **Rust is the only thing
//!   in the fleet that parses the CSV**.
//!
//! # Call shapes
//!
//! Fixed across the fleet, because the audit reads them:
//!
//! | language | lookup | with arguments |
//! |---|---|---|
//! | Rust | `tr!("id")` | `trf!("id", k = v)` |
//! | JavaScript | `t("id")` | `tf("id", { k: v })` |
//! | HTML | `data-i18n="id"` | — |
//!
//! JavaScript may live inline in a `.html` page as easily as in a `.js`
//! file, so both lookups are scanned in both.
//!
//! The macros live in the game (they close over its table); what belongs
//! here is the shape they take, so one audit can serve all three languages.

mod audit;
mod csv;
mod interpolate;
mod plural;
mod table;

pub use audit::{audit, AuditInput, Finding, Marker, Report, FLEET};
pub use csv::{parse_csv, CsvError};
pub use interpolate::{interpolate, placeholders_in};
pub use plural::{Category, Locale};
pub use table::{is_id, PlaceholderDrift, Row, Table, TableError, HEADER, MISSING};

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// Route missed lookups into the game's own logger.
///
/// A missing id panics in a debug build, so this is about release: the
/// marker on screen only helps someone who is looking at the screen, and the
/// log line is what is left afterwards. Games have their own logging
/// discipline — Bevy's `warn!`, phoenix's categorised `plog!` — so this crate
/// takes a hook rather than a logging dependency and an opinion.
///
/// Without a hook, misses go to stderr, **once per id**: a lookup in a render
/// loop would otherwise write sixty lines a second about one typo.
pub fn on_missing(hook: fn(&str)) {
    let _ = MISSING_HOOK.set(hook);
}

static MISSING_HOOK: OnceLock<fn(&str)> = OnceLock::new();
static REPORTED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

pub(crate) fn report_missing(id: &str) {
    if let Some(hook) = MISSING_HOOK.get() {
        hook(id);
        return;
    }
    let seen = REPORTED.get_or_init(|| Mutex::new(BTreeSet::new()));
    let Ok(mut seen) = seen.lock() else {
        return; // a poisoned lock is not worth a second panic
    };
    if seen.insert(id.to_owned()) {
        eprintln!("missing string: `{id}`");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole pipeline on one small table: parse, look up, fill, pluralise,
    /// and emit what a page reads.
    #[test]
    fn a_table_serves_rust_and_a_page_from_one_parse() {
        let csv = "id,context,text\n\
                   hud.wave,Wave counter label,WAVE\n\
                   hud.enemies.one,Enemy count,1 enemy\n\
                   hud.enemies.other,Enemy count,{n} enemies\n";
        let table = Table::parse(Locale::ENGLISH, csv).expect("parses");

        assert_eq!(table.text("hud.wave"), "WAVE");
        assert_eq!(table.plural("hud.enemies", 1, &[]), "1 enemy");
        assert_eq!(table.plural("hud.enemies", 3, &[("n", "3")]), "3 enemies");

        let json = table.to_json();
        assert!(json.contains(r#""hud.wave": "WAVE""#));
        assert!(json.contains(r#""hud.enemies.other": "{n} enemies""#));
    }
}
