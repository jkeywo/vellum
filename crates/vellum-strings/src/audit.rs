//! The audit: which ids are used, which exist, and where they disagree.
//!
//! This is the part that makes renaming a string safe, and it only works if
//! *both* halves are honest — an id used with no row, and a row no id reaches.
//! The second half is the one that rots: a game whose ids are partly built at
//! runtime cannot find them by scanning, so an orphan check that only scans
//! either produces false alarms or gets an exemption list that hides real
//! orphans behind a prefix.
//!
//! So the audit takes the derived ids from the caller. It is meant to be run
//! from a game's own test, where the content catalogue is already loaded and
//! the real expansion is one iterator away:
//!
//! ```no_run
//! # use vellum_strings::{audit, AuditInput, Locale, Table};
//! # let table = Table::empty(Locale::ENGLISH);
//! # struct Item { id: String }
//! # let items: Vec<Item> = Vec::new();
//! let report = audit(
//!     &table,
//!     AuditInput::new(&["crates/vt_client/src", "crates/vt_client/assets/ui"])
//!         .derived(items.iter().map(|i| format!("item.{}.name", i.id))),
//! );
//! assert!(report.findings.is_empty(), "{}", report);
//! ```
//!
//! Both checks then become exact: an item with no row fails here rather than
//! at content load, and a row for a deleted item is an orphan rather than
//! something a prefix quietly covers.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::table::Table;

/// How ids are written in one kind of file.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    /// File extensions this marker applies to, without the dot.
    pub extensions: &'static [&'static str],
    /// The text immediately before the quoted id.
    pub call: &'static str,
}

/// The fleet's call shapes, in the three languages that hold player-facing
/// text. Keeping these fixed across the fleet is what lets one audit serve a
/// Rust simulation, a JavaScript console and a static page.
pub const FLEET: &[Marker] = &[
    Marker {
        extensions: &["rs"],
        call: "tr!(",
    },
    Marker {
        extensions: &["rs"],
        call: "trf!(",
    },
    // `.html` as well as `.js`: a page may hold its script inline, and the
    // first game to adopt this pipeline kept its whole HUD — markup and
    // behaviour — in one file. A marker that only looked at `.js` found the
    // attributes and none of the lookups.
    Marker {
        extensions: &["js", "mjs", "html"],
        call: "t(",
    },
    // The interpolating form, symmetric with Rust's trf!. Without it a row
    // reached only with arguments reads as an orphan.
    Marker {
        extensions: &["js", "mjs", "html"],
        call: "tf(",
    },
    Marker {
        extensions: &["html"],
        call: "data-i18n=",
    },
];

/// What to scan, and what the scan cannot see.
pub struct AuditInput<'a> {
    roots: Vec<PathBuf>,
    markers: &'a [Marker],
    derived: BTreeSet<String>,
    skip: Vec<PathBuf>,
}

impl<'a> AuditInput<'a> {
    /// Scan these directories (recursively) with the fleet markers.
    pub fn new<P: AsRef<Path>>(roots: &[P]) -> Self {
        Self {
            roots: roots.iter().map(|r| r.as_ref().to_path_buf()).collect(),
            markers: FLEET,
            derived: BTreeSet::new(),
            skip: Vec::new(),
        }
    }

    /// Ids the scan cannot see because the game builds them at runtime.
    ///
    /// Supply the *expansion*, not a prefix: `item.garrote.name`, one per
    /// real item, not `item.`.
    pub fn derived<I: IntoIterator<Item = String>>(mut self, ids: I) -> Self {
        self.derived.extend(ids);
        self
    }

    /// Use markers other than the fleet's — for a game with an extra call
    /// shape it has not finished migrating away from.
    pub fn markers(mut self, markers: &'a [Marker]) -> Self {
        self.markers = markers;
        self
    }

    /// Skip a file. The one honest use is the module that *defines* the
    /// markers, whose own literals are not lookups.
    pub fn skip<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.skip.push(path.as_ref().to_path_buf());
        self
    }
}

/// Something the audit wants someone to look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// An id is used, and the table has no row for it.
    Undefined { id: String, source: PathBuf },
    /// A row exists that nothing reaches.
    Orphan { id: String },
    /// A stem has some of the locale's plural categories but not all.
    IncompletePlural { stem: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Undefined { id, source } => {
                write!(f, "undefined: `{id}` used in {}", source.display())
            }
            Finding::Orphan { id } => write!(f, "orphan: `{id}` has a row nothing reaches"),
            Finding::IncompletePlural { stem } => {
                write!(f, "incomplete plural: `{stem}` is missing a category")
            }
        }
    }
}

/// What the audit found, and what it looked at.
pub struct Report {
    pub findings: Vec<Finding>,
    /// Files opened, so an audit that silently scanned nothing is visible.
    pub files_scanned: usize,
    pub ids_used: usize,
    pub rows: usize,
    /// Rows still wrapped in `[brackets]` — a progress figure, not a fault.
    pub bracketed: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} rows, {} used, {} files scanned, {} still bracketed",
            self.rows, self.ids_used, self.files_scanned, self.bracketed
        )?;
        for finding in &self.findings {
            writeln!(f, "  {finding}")?;
        }
        Ok(())
    }
}

/// Check a table against the code and data that use it.
pub fn audit(table: &Table, input: AuditInput<'_>) -> Report {
    let mut files = Vec::new();
    for root in &input.roots {
        collect(root, &mut files);
    }
    files.retain(|path| !input.skip.iter().any(|skip| path.ends_with(skip)));

    let mut used: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut files_scanned = 0usize;
    // Every source, concatenated, for the orphan half only. The two halves
    // deliberately ask different questions. "Is this id defined?" is asked of
    // the call shapes, because a typo inside `tr!` must be a build failure.
    // "Does anything reach this row?" is asked of the whole text, because an
    // id can legitimately sit in a `const` table with no call around it —
    // murmur's keymap holds a hundred that way — and a false orphan would
    // push a game to delete a row it still needs.
    let mut corpus = String::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        files_scanned += 1;
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        for marker in input.markers {
            if !marker.extensions.contains(&extension) {
                continue;
            }
            for id in extract(&text, marker.call) {
                used.entry(id).or_insert_with(|| path.clone());
            }
        }
        corpus.push_str(&text);
    }
    for id in &input.derived {
        used.entry(id.clone()).or_insert_with(|| "<derived>".into());
    }

    let defined: BTreeSet<&str> = table.ids().collect();
    let mut findings = Vec::new();
    for (id, source) in &used {
        if !defined.contains(id.as_str()) {
            findings.push(Finding::Undefined {
                id: id.clone(),
                source: source.clone(),
            });
        }
    }
    for id in &defined {
        if !used.contains_key(*id) && !corpus.contains(&format!("\"{id}\"")) {
            findings.push(Finding::Orphan {
                id: (*id).to_owned(),
            });
        }
    }
    for stem in table.incomplete_plurals() {
        findings.push(Finding::IncompletePlural { stem });
    }

    Report {
        findings,
        files_scanned,
        ids_used: used.len(),
        rows: table.len(),
        bracketed: table.placeholder_count(),
    }
}

/// Every quoted id immediately following `call`.
///
/// The boundary guard matters more than it looks: `tr!(` is a suffix of
/// `include_str!(`, and JavaScript's `t(` is a suffix of nearly every
/// function name there is. A marker that continues an identifier — or that
/// follows a `.`, so a method call — is not a lookup.
fn extract(text: &str, call: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut scanned = 0usize;
    while let Some(at) = text[scanned..].find(call) {
        let start = scanned + at;
        scanned = start + call.len();
        let preceded_by_name = text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        if preceded_by_name {
            continue;
        }
        let after = text[scanned..].trim_start();
        // Nested rather than let-chained: this crate sits on edition 2021,
        // the floor its consumers set.
        for quote in ['"', '\''] {
            if let Some(body) = after.strip_prefix(quote) {
                if let Some(end) = body.find(quote) {
                    ids.push(body[..end].to_owned());
                    break;
                }
            }
        }
    }
    ids
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Never walk into build output: a copied asset would be counted
            // twice and a vendored dependency is not this game's text.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "target" | "dist" | "node_modules" | ".git") {
                continue;
            }
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plural::Locale;

    #[test]
    fn extracts_ids_and_ignores_lookalikes() {
        let rust = r#"
            let a = tr!("hud.wave");
            let b = trf!("hud.hull", n = 3);
            let c = include_str!("not.an.id");
            let d = format!("also.not");
        "#;
        assert_eq!(extract(rust, "tr!("), vec!["hud.wave".to_owned()]);
        assert_eq!(extract(rust, "trf!("), vec!["hud.hull".to_owned()]);
    }

    #[test]
    fn javascripts_t_does_not_match_every_function() {
        let js = r#"
            const a = t("hud.wave");
            const b = t('hud.plunder');
            const c = format("nope.here");
            const d = obj.t("method.call");
            const e = `${t("hud.warp")}`;
        "#;
        let ids = extract(js, "t(");
        assert_eq!(
            ids,
            vec![
                "hud.wave".to_owned(),
                "hud.plunder".to_owned(),
                "hud.warp".to_owned()
            ],
            "format( and .t( must not match"
        );
    }

    #[test]
    fn html_attributes_are_read() {
        let html = r#"<span data-i18n="hud.enemies">ENEMIES</span>"#;
        assert_eq!(extract(html, "data-i18n="), vec!["hud.enemies".to_owned()]);
    }

    /// A page with inline script holds both kinds of lookup, and the markers
    /// must find both — the gap the first adopting game walked straight into.
    #[test]
    fn a_page_with_inline_script_yields_both_kinds() {
        let html = r#"
            <span data-i18n="hud.wave">WAVE</span>
            <script>
              el.prompt.textContent = t("title.prompt.keyboard");
            </script>
        "#;
        let markers: Vec<&Marker> = FLEET
            .iter()
            .filter(|m| m.extensions.contains(&"html"))
            .collect();
        let found: Vec<String> = markers.iter().flat_map(|m| extract(html, m.call)).collect();
        assert!(found.contains(&"hud.wave".to_owned()));
        assert!(found.contains(&"title.prompt.keyboard".to_owned()));
    }

    fn fixture_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vellum-strings-audit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn undefined_and_orphan_are_both_found() {
        let dir = fixture_dir();
        let source = dir.join("audit_case.rs");
        std::fs::write(
            &source,
            r#"tr!("used.and.defined"); tr!("used.not.defined");"#,
        )
        .unwrap();

        let table = Table::from_rows(
            Locale::ENGLISH,
            &[("used.and.defined", "x"), ("defined.not.used", "y")],
        );
        let report = audit(&table, AuditInput::new(&[&source]));

        assert!(report.findings.contains(&Finding::Undefined {
            id: "used.not.defined".into(),
            source: source.clone(),
        }));
        assert!(report.findings.contains(&Finding::Orphan {
            id: "defined.not.used".into(),
        }));
        let _ = std::fs::remove_file(&source);
    }

    /// An id in a `const` table is reached by whatever reads that table, not
    /// by a call the scanner can see. It is not an orphan — but a bare
    /// literal is also not a lookup, so it must not make the undefined half
    /// fire either. Only a call shape claims "this id must exist".
    #[test]
    fn a_bare_literal_answers_the_orphan_half_but_not_the_other() {
        let dir = fixture_dir();
        let source = dir.join("keymap_case.rs");
        std::fs::write(
            &source,
            r#"const KEYS: &[(&str, &str)] = &[("c", "keymap.carry.label")];
               const GONE: &str = "keymap.ghost.label";"#,
        )
        .unwrap();

        let table = Table::from_rows(Locale::ENGLISH, &[("keymap.carry.label", "Carry")]);
        let report = audit(&table, AuditInput::new(&[&source]));

        assert!(
            report.ok(),
            "
{report}"
        );
        let _ = std::fs::remove_file(&source);
    }

    /// The reason the derived set exists: an id the scan cannot see must
    /// count as used, and must still have to exist.
    #[test]
    fn derived_ids_are_used_and_still_checked() {
        let dir = fixture_dir();
        let source = dir.join("derived_case.rs");
        std::fs::write(&source, "// no literal lookups here\n").unwrap();

        let table = Table::from_rows(Locale::ENGLISH, &[("item.garrote.name", "Garrote")]);

        // Supplied and present: no findings.
        let report = audit(
            &table,
            AuditInput::new(&[&source]).derived(["item.garrote.name".to_owned()]),
        );
        assert!(report.ok(), "{report}");

        // A new item with no row is caught here, not at content load.
        let report = audit(
            &table,
            AuditInput::new(&[&source]).derived([
                "item.garrote.name".to_owned(),
                "item.pistol.name".to_owned(),
            ]),
        );
        assert_eq!(
            report.findings,
            vec![Finding::Undefined {
                id: "item.pistol.name".into(),
                source: "<derived>".into(),
            }]
        );

        // A row for a deleted item is an orphan — the blind spot a prefix
        // allowlist has.
        let stale = Table::from_rows(
            Locale::ENGLISH,
            &[("item.garrote.name", "G"), ("item.deleted.name", "D")],
        );
        let report = audit(
            &stale,
            AuditInput::new(&[&source]).derived(["item.garrote.name".to_owned()]),
        );
        assert_eq!(
            report.findings,
            vec![Finding::Orphan {
                id: "item.deleted.name".into()
            }]
        );
        let _ = std::fs::remove_file(&source);
    }
}
