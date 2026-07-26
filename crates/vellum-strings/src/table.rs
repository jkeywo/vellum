//! The table itself: ids, rows, lookup, and the JSON a page fetches.

use std::collections::BTreeMap;
use std::fmt;

use crate::csv::parse_csv;
use crate::interpolate::{interpolate, placeholders_in};
use crate::plural::{Category, Locale};
use crate::report_missing;

/// Rendered in place of a string whose id is not in the table.
///
/// A fixed `&'static str` rather than the missing id, and the reason is worth
/// keeping: a lookup in a render loop must not allocate. Formatting the id
/// into a marker leaks a string a frame. The id is named by the debug panic
/// and by the miss report instead, where naming it costs nothing.
pub const MISSING: &str = "!!MISSING STRING!!";

/// The header every table carries. `text` rather than `english`, because the
/// column holds whatever language the file is.
pub const HEADER: [&str; 3] = ["id", "context", "text"];

/// One row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What the string is for and where it appears — the note a translator
    /// reads instead of guessing from the text.
    pub context: String,
    /// The text, with any `{named}` slots, and square brackets while the line
    /// is unapproved.
    pub text: String,
}

impl Row {
    /// Whether this line is still unapproved placeholder prose.
    ///
    /// The convention across the fleet: an agent writes `[like this]` and a
    /// human approves the line by deleting the brackets. Nothing enforces it
    /// — a check would fail on exactly the edit it should welcome — but the
    /// share still bracketed is a progress figure worth reporting.
    pub fn is_placeholder(&self) -> bool {
        self.text.starts_with('[') && self.text.ends_with(']')
    }
}

/// Why a table would not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    Csv(crate::csv::CsvError),
    BadHeader { found: String },
    Empty,
    BadId { line: usize, id: String },
    DuplicateId { line: usize, id: String },
    WrongFieldCount { line: usize, found: usize },
    EmptyText { line: usize, id: String },
    UnclosedPlaceholder { line: usize, id: String },
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::Csv(error) => write!(f, "{error}"),
            TableError::BadHeader { found } => {
                write!(f, "header must be `id,context,text`, found `{found}`")
            }
            TableError::Empty => f.write_str("the table is empty"),
            TableError::BadId { line, id } => write!(
                f,
                "line {line}: `{id}` is not an id — ids are dot-separated \
                 lowercase segments of a-z, 0-9, _ and -"
            ),
            TableError::DuplicateId { line, id } => write!(f, "line {line}: duplicate id `{id}`"),
            TableError::WrongFieldCount { line, found } => {
                write!(f, "line {line}: expected 3 fields, found {found}")
            }
            TableError::EmptyText { line, id } => {
                write!(f, "line {line}: `{id}` has no text")
            }
            TableError::UnclosedPlaceholder { line, id } => {
                write!(f, "line {line}: `{id}` has an unclosed {{placeholder")
            }
        }
    }
}

impl std::error::Error for TableError {}

/// Whether a string is shaped like an id.
///
/// Deliberately narrow. This is also the check that catches prose sitting in
/// a data field where an id belongs: `"A rusted garrote"` fails on the
/// capital and the spaces, which is the whole point.
pub fn is_id(candidate: &str) -> bool {
    if candidate.is_empty() || !candidate.contains('.') {
        return false;
    }
    candidate.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    })
}

/// Every string for one locale, keyed by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    locale: Locale,
    rows: BTreeMap<String, Row>,
}

impl Table {
    /// Parse a table for `locale`, reporting *every* problem rather than the
    /// first — an author fixes a batch per run.
    pub fn parse(locale: Locale, source: &str) -> Result<Table, Vec<TableError>> {
        let records = match parse_csv(source) {
            Ok(records) => records,
            Err(error) => return Err(vec![TableError::Csv(error)]),
        };
        let mut records = records.into_iter();
        match records.next() {
            Some(header) if header == HEADER => {}
            Some(header) => {
                return Err(vec![TableError::BadHeader {
                    found: header.join(","),
                }]);
            }
            None => return Err(vec![TableError::Empty]),
        }

        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        let mut errors = Vec::new();
        for (index, record) in records.enumerate() {
            // Header consumed above, and humans count from one.
            let line = index + 2;
            // Blank spacers and `#` section markers carry no row. A table
            // large enough to need them — murmur's runs to six hundred rows —
            // is unreadable in a text editor without them, and they cost
            // nothing to skip.
            let first = record.first().map(|f| f.trim()).unwrap_or("");
            if first.is_empty() || first.starts_with('#') {
                continue;
            }
            if record.len() != 3 {
                errors.push(TableError::WrongFieldCount {
                    line,
                    found: record.len(),
                });
                continue;
            }
            let [id, context, text]: [String; 3] = record.try_into().expect("length checked");
            // A hand-edited table picks up stray spaces around an id and a
            // context; neither is content. The text is left exactly as
            // written — its leading space may be deliberate.
            let id = id.trim().to_owned();
            let context = context.trim().to_owned();
            if !is_id(&id) {
                errors.push(TableError::BadId { line, id });
                continue;
            }
            if rows.contains_key(&id) {
                errors.push(TableError::DuplicateId { line, id });
                continue;
            }
            // Two authoring mistakes murmur learned to refuse: a truncated
            // edit that leaves the text empty, and a `{` with no `}`, which
            // reaches the screen as literal noise rather than a value.
            if text.trim().is_empty() {
                errors.push(TableError::EmptyText { line, id });
                continue;
            }
            if text.matches('{').count() != text.matches('}').count() {
                errors.push(TableError::UnclosedPlaceholder { line, id });
                continue;
            }
            rows.insert(id, Row { context, text });
        }

        if errors.is_empty() {
            Ok(Table { locale, rows })
        } else {
            Err(errors)
        }
    }

    /// An empty table, for a test whose subject is not the prose.
    pub fn empty(locale: Locale) -> Table {
        Table {
            locale,
            rows: BTreeMap::new(),
        }
    }

    /// Build from `(id, text)` pairs, for a fixture that needs real prose
    /// behind its ids. Panics on a malformed id, which in a test is the right
    /// moment to find out.
    pub fn from_rows(locale: Locale, rows: &[(&str, &str)]) -> Table {
        let mut table = Table::empty(locale);
        for (id, text) in rows {
            assert!(is_id(id), "`{id}` is not an id");
            table.rows.insert(
                (*id).to_owned(),
                Row {
                    context: String::new(),
                    text: (*text).to_owned(),
                },
            );
        }
        table
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// The text for an id, or `None`.
    pub fn get(&self, id: &str) -> Option<&str> {
        self.rows.get(id).map(|row| row.text.as_str())
    }

    /// The row for an id, context included.
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.get(id)
    }

    /// The text for an id.
    ///
    /// A missing id **panics in a debug build** — it is a bug, and the call
    /// site is where you want to be standing. In a release build it returns
    /// [`MISSING`] and reports the miss (see
    /// [`on_missing`](crate::on_missing)), because a copy slip should not
    /// take down a running game, and a marker nobody is looking at needs a
    /// log line to be worth anything.
    pub fn text(&self, id: &str) -> &str {
        match self.rows.get(id) {
            Some(row) => row.text.as_str(),
            None => {
                debug_assert!(false, "string table has no row for `{id}`");
                report_missing(id);
                MISSING
            }
        }
    }

    /// The text for an id with its `{named}` slots filled.
    pub fn format(&self, id: &str, args: &[(&str, &str)]) -> String {
        interpolate(self.text(id), args)
    }

    /// The text for a pluralised id, selected on `count`.
    ///
    /// `stem` is the id without a category suffix: `hud.enemies` finds
    /// `hud.enemies.one` or `hud.enemies.other` under English. The count is
    /// *not* supplied as an argument automatically — a row names its own
    /// slots, and a language may want the number in a different place or not
    /// at all.
    pub fn plural(&self, stem: &str, count: i64, args: &[(&str, &str)]) -> String {
        let category = self.locale.category(count);
        self.format(&format!("{stem}.{category}"), args)
    }

    /// Every id, in order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.rows.keys().map(String::as_str)
    }

    /// Every id and row, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Row)> {
        self.rows.iter().map(|(id, row)| (id.as_str(), row))
    }

    /// Every id and row whose id starts with `prefix`, in id order.
    ///
    /// This is how an authored *pool* reaches a generator — murmur's person
    /// names, districts and briefing reasons are rows, not a RON list, so the
    /// table is the list. Two consequences follow, and a game reading a pool
    /// owns both:
    ///
    /// - **The order is the fingerprint.** Ids are ordered as bytes, so a
    ///   pool meant to be indexed must zero-pad (`names.first.01`). Unpadded,
    ///   adding a tenth entry reorders the first nine and every existing seed
    ///   generates something different.
    /// - **These ids are reached without a literal**, so the audit cannot see
    ///   them by scanning. Feed the ids from here into
    ///   [`AuditInput::derived`](crate::AuditInput::derived) and the orphan
    ///   half stays exact: the same call that reads a pool declares it.
    pub fn with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a Row)> {
        self.rows
            .range(prefix.to_string()..)
            .take_while(move |(id, _)| id.starts_with(prefix))
            .map(|(id, row)| (id.as_str(), row))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many rows are still unapproved placeholder prose.
    pub fn placeholder_count(&self) -> usize {
        self.rows
            .values()
            .filter(|row| row.is_placeholder())
            .count()
    }

    /// The table as a JSON object of id to text — what a page fetches.
    ///
    /// Written by hand so this crate keeps no dependencies: it is the one
    /// piece of the pipeline a JavaScript consumer reads, and the fleet's
    /// rule is that Rust is the only thing that ever parses the CSV. A page
    /// therefore reads what this produced and cannot disagree with Rust
    /// about a row.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        for (index, (id, row)) in self.rows.iter().enumerate() {
            if index > 0 {
                out.push_str(",\n");
            }
            out.push_str("  ");
            write_json_string(&mut out, id);
            out.push_str(": ");
            write_json_string(&mut out, &row.text);
        }
        out.push_str("\n}\n");
        out
    }

    /// Ids whose rows name different `{slots}` than the reference locale's.
    ///
    /// A translation that drops `{n}` or invents `{count}` renders a
    /// half-filled sentence; this names it instead. A no-op while a game
    /// ships one locale, and exactly the check it will want on the day it
    /// ships two.
    pub fn placeholder_drift(&self, reference: &Table) -> Vec<PlaceholderDrift> {
        let mut drift = Vec::new();
        for (id, row) in &reference.rows {
            let Some(mine) = self.rows.get(id) else {
                continue; // a missing row is the audit's business, not this
            };
            let want = placeholders_in(&row.text);
            let got = placeholders_in(&mine.text);
            let missing: Vec<String> = want.iter().filter(|n| !got.contains(n)).cloned().collect();
            let extra: Vec<String> = got.iter().filter(|n| !want.contains(n)).cloned().collect();
            if !missing.is_empty() || !extra.is_empty() {
                drift.push(PlaceholderDrift {
                    id: id.clone(),
                    locale: self.locale.tag,
                    missing,
                    extra,
                });
            }
        }
        drift
    }

    /// Stems that have some category rows but not all the locale's
    /// categories — the shape that renders `!!MISSING STRING!!` for exactly
    /// one count.
    pub fn incomplete_plurals(&self) -> Vec<String> {
        let mut stems: BTreeMap<&str, Vec<Category>> = BTreeMap::new();
        for id in self.rows.keys() {
            let Some((stem, suffix)) = id.rsplit_once('.') else {
                continue;
            };
            if let Some(category) = Category::from_suffix(suffix) {
                stems.entry(stem).or_default().push(category);
            }
        }
        stems
            .into_iter()
            .filter(|(_, found)| {
                self.locale
                    .categories
                    .iter()
                    .any(|wanted| !found.contains(wanted))
            })
            .map(|(stem, _)| stem.to_owned())
            .collect()
    }
}

/// A row whose slots disagree with the reference locale's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderDrift {
    pub id: String,
    pub locale: &'static str,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl fmt::Display for PlaceholderDrift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]:", self.id, self.locale)?;
        if !self.missing.is_empty() {
            write!(f, " missing {{{}}}", self.missing.join("}, {"))?;
        }
        if !self.extra.is_empty() {
            write!(f, " unexpected {{{}}}", self.extra.join("}, {"))?;
        }
        Ok(())
    }
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "id,context,text\n";

    fn table(body: &str) -> Table {
        Table::parse(Locale::ENGLISH, &format!("{HEAD}{body}")).expect("table parses")
    }

    /// A table large enough to need sections gets them; neither a spacer nor
    /// a `#` marker is a row.
    #[test]
    fn blank_and_commented_rows_carry_nothing() {
        let csv = concat!(
            "id,context,text
",
            "
",
            "# --- the hub ---,,
",
            "hub.enter,Entering the hub,[You step inside.]
",
        );
        let table = Table::parse(Locale::ENGLISH, csv).expect("parses");
        assert_eq!(table.len(), 1);
        assert_eq!(table.text("hub.enter"), "[You step inside.]");
    }

    #[test]
    fn a_truncated_edit_and_an_unclosed_slot_are_both_refused() {
        let csv = concat!(
            "id,context,text
",
            "a.empty,Note,
",
            "a.unclosed,Note,[found in {room]
",
        );
        let errors = Table::parse(Locale::ENGLISH, csv).expect_err("refuses both");
        assert!(matches!(errors[0], TableError::EmptyText { .. }));
        assert!(matches!(errors[1], TableError::UnclosedPlaceholder { .. }));
    }

    /// The pool contract: id order, and nothing from a neighbouring prefix.
    /// `names.last` must not leak into `names.first`, and the zero-padding is
    /// what keeps the order stable as the pool grows.
    #[test]
    fn a_pool_reads_in_id_order_and_stops_at_its_prefix() {
        let csv = concat!(
            "id,context,text
",
            "names.first.01,Given name,Ada
",
            "names.first.02,Given name,Bram
",
            "names.first.10,Given name,Chen
",
            "names.last.01,Family name,Okonkwo
",
        );
        let table = Table::parse(Locale::ENGLISH, csv).expect("parses");
        let pool: Vec<&str> = table
            .with_prefix("names.first.")
            .map(|(_, row)| row.text.as_str())
            .collect();
        assert_eq!(pool, ["Ada", "Bram", "Chen"]);
        let ids: Vec<&str> = table.with_prefix("names.first.").map(|(id, _)| id).collect();
        assert_eq!(ids, ["names.first.01", "names.first.02", "names.first.10"]);
    }

    /// A hand-edited row with stray spaces is the same row.
    #[test]
    fn an_id_is_trimmed_before_it_is_judged() {
        let csv = concat!("id,context,text
", "  hub.enter , Note ,[In.]
");
        let table = Table::parse(Locale::ENGLISH, csv).expect("parses");
        assert_eq!(table.text("hub.enter"), "[In.]");
        assert_eq!(table.row("hub.enter").expect("present").context, "Note");
    }

    #[test]
    fn ids_are_narrow_enough_to_catch_prose() {
        assert!(is_id("hud.wave"));
        assert!(is_id("ship.corsair_sloop.name"));
        assert!(is_id("a.b-c.d0"));
        // The cases that matter: prose in a field where an id belongs.
        assert!(!is_id("A rusted garrote"));
        assert!(!is_id("Salvage HUD"));
        assert!(!is_id("nodots"));
        assert!(!is_id("Trailing.Capitals"));
        assert!(!is_id("empty..segment"));
        assert!(!is_id(""));
    }

    #[test]
    fn a_table_parses_and_looks_up() {
        let table = table("hud.wave,The wave counter,WAVE\n");
        assert_eq!(table.get("hud.wave"), Some("WAVE"));
        assert_eq!(table.text("hud.wave"), "WAVE");
        assert_eq!(table.row("hud.wave").unwrap().context, "The wave counter");
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn every_problem_is_reported_not_just_the_first() {
        let errors = Table::parse(
            Locale::ENGLISH,
            "id,context,text\nNot An Id,c,t\ngood.id,c,t\ngood.id,c,t\nshort,c\n",
        )
        .expect_err("a broken table");
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(matches!(errors[0], TableError::BadId { .. }));
        assert!(matches!(errors[1], TableError::DuplicateId { .. }));
        assert!(matches!(errors[2], TableError::WrongFieldCount { .. }));
    }

    #[test]
    fn a_wrong_header_is_named() {
        let errors =
            Table::parse(Locale::ENGLISH, "id,context,english\na.b,c,t\n").expect_err("old header");
        assert!(matches!(errors[0], TableError::BadHeader { .. }));
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "no row for"))]
    fn a_missing_id_panics_in_debug_and_marks_in_release() {
        let table = table("hud.wave,c,WAVE\n");
        let got = table.text("hud.absent");
        #[cfg(not(debug_assertions))]
        assert_eq!(got, MISSING);
        #[cfg(debug_assertions)]
        let _ = got;
    }

    #[test]
    fn plurals_select_on_count() {
        let table = table("hud.enemies.one,c,1 enemy left\nhud.enemies.other,c,{n} enemies left\n");
        assert_eq!(table.plural("hud.enemies", 1, &[]), "1 enemy left");
        assert_eq!(
            table.plural("hud.enemies", 4, &[("n", "4")]),
            "4 enemies left"
        );
    }

    #[test]
    fn a_half_pluralised_stem_is_named() {
        let half = table("hud.enemies.one,c,1 enemy left\n");
        assert_eq!(half.incomplete_plurals(), vec!["hud.enemies".to_owned()]);

        let complete = table("hud.enemies.one,c,x\nhud.enemies.other,c,y\n");
        assert!(complete.incomplete_plurals().is_empty());
    }

    #[test]
    fn a_plain_row_is_not_mistaken_for_a_plural_stem() {
        // `.name` is not a category, so this stem is complete by construction.
        let table = table("ship.sloop.name,c,Sloop\n");
        assert!(table.incomplete_plurals().is_empty());
    }

    #[test]
    fn placeholder_drift_names_dropped_and_invented_slots() {
        let reference = table("a.b,c,{n} ships and {port}\n");
        let translated = Table::from_rows(
            Locale {
                tag: "fr",
                ..Locale::ENGLISH
            },
            &[("a.b", "{count} navires")],
        );
        let drift = translated.placeholder_drift(&reference);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].missing, vec!["n".to_owned(), "port".to_owned()]);
        assert_eq!(drift[0].extra, vec!["count".to_owned()]);
    }

    #[test]
    fn the_bracket_census_counts_unapproved_prose() {
        let table = table("a.b,c,[draft line]\nc.d,c,approved line\n");
        assert_eq!(table.placeholder_count(), 1);
    }

    #[test]
    fn json_is_parseable_and_escapes_what_it_must() {
        let table = table("a.b,c,\"say \"\"hi\"\"\nand \\ this\"\n");
        let json = table.to_json();
        assert!(
            json.contains(r#""a.b": "say \"hi\"\nand \\ this""#),
            "{json}"
        );
        assert!(json.starts_with('{') && json.trim_end().ends_with('}'));
    }
}
