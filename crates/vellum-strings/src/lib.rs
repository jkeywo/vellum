//! The authored-text substrate: a CSV reader and a placeholder filler.
//!
//! Both games keep every player-facing line in one CSV and refer to it by id,
//! so that structure files carry numbers and shape while prose stays in one
//! translatable place. Both therefore needed the same two unglamorous things,
//! and both wrote them.
//!
//! # Why this is the safe crate to share
//!
//! Neither game's save format can be affected by anything here. rogue-hunter
//! excludes the string table from its content fingerprint and marks rendered
//! log text `#[serde(skip)]` precisely so that copy edits and translations
//! cannot move a digest; murmur's fingerprint is over `World`, which holds
//! ids rather than prose. Changing how text is parsed or filled therefore
//! cannot invalidate a saved run — which makes this the one part of the
//! authored-content pipeline that can move without a determinism argument.
//!
//! What is *not* here: the tables themselves. Each game's lookup type has its
//! own missing-id sentinel, its own lifetimes, and its own opinion about
//! whether a missing id should be loud in a release build. Those are visible
//! to players and to authors, so they stay where they are authored.

use std::fmt;

/// Where a CSV file stopped making sense.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvError {
    /// 1-based, so it matches what a text editor shows.
    pub line: usize,
    pub message: &'static str,
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for CsvError {}

/// Split CSV text into records, resolving quoting.
///
/// A minimal RFC 4180 reader, hand-rolled rather than pulled from a crate: the
/// format is entirely under the authors' control, both games ship to wasm and
/// care about payload size, and a state machine this small can be tested
/// exhaustively against the cases that actually bite — quoted commas, doubled
/// quotes, apostrophes, and the CRLF that text editors leave behind.
///
/// Malformed input is an error rather than a best guess. A silently mangled
/// row becomes garbled text in front of a player, at which point the CSV is
/// the last place anyone looks.
pub fn parse_csv(source: &str) -> Result<Vec<Vec<String>>, CsvError> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    // Whether the current field began with a quote, so stray text after the
    // closing quote is rejected rather than silently joined on.
    let mut closed = false;
    let mut line = 1usize;
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                        closed = true;
                    }
                }
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                _ => field.push(ch),
            }
            continue;
        }

        match ch {
            '"' if field.is_empty() && !closed => quoted = true,
            '"' => {
                return Err(CsvError {
                    line,
                    message: "unexpected quote inside a bare field",
                });
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                line += 1;
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                closed = false;
            }
            _ if closed => {
                return Err(CsvError {
                    line,
                    message: "text after a closing quote",
                });
            }
            _ => field.push(ch),
        }
    }

    if quoted {
        return Err(CsvError {
            line,
            message: "unterminated quoted field",
        });
    }
    // A trailing newline leaves nothing pending; anything else is a last row.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

/// Replace each `{name}` slot in `template` with its argument.
///
/// Two decisions worth keeping:
///
/// **Unmatched slots stay visible.** A `{room}` left on screen points at the
/// bug; an empty gap hides it. In debug builds it also trips an assertion, so
/// it is caught before anyone sees it.
///
/// **Substituted values are never rescanned.** This is a single left-to-right
/// pass, so a value that happens to contain `{name}` is inserted literally
/// rather than being filled in by a later argument. Implementing this as
/// repeated `str::replace` — one pass per argument, which is the obvious way
/// and was how one of the two games did it — lets authored data inject into
/// its own template: a villain called `{hunter}` would come back out as the
/// hunter's name.
pub fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if !template.contains('{') {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match args.iter().find(|(key, _)| *key == name) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        debug_assert!(false, "no argument for placeholder {{{name}}}");
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            // An unbalanced brace keeps its trailing text verbatim. Both games
            // reject these when the table is parsed, so reaching here means the
            // template came from somewhere else.
            None => {
                out.push('{');
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(source: &str) -> Vec<Vec<String>> {
        parse_csv(source).expect("parses")
    }

    #[test]
    fn plain_fields_split_on_commas() {
        assert_eq!(rows("a,b,c"), vec![vec!["a", "b", "c"]]);
    }

    #[test]
    fn a_quoted_field_may_contain_a_comma() {
        assert_eq!(
            rows(r#"id,"one, two",z"#),
            vec![vec!["id", "one, two", "z"]]
        );
    }

    #[test]
    fn a_doubled_quote_is_one_literal_quote() {
        assert_eq!(
            rows(r#"id,"she said ""no""","#),
            vec![vec!["id", r#"she said "no""#, ""]]
        );
    }

    #[test]
    fn apostrophes_need_no_escaping() {
        assert_eq!(
            rows("id,the wolf's den"),
            vec![vec!["id", "the wolf's den"]]
        );
    }

    #[test]
    fn crlf_and_lf_both_end_a_record() {
        assert_eq!(
            rows("a,b\r\nc,d\ne,f"),
            vec![vec!["a", "b"], vec!["c", "d"], vec!["e", "f"]]
        );
    }

    #[test]
    fn a_trailing_newline_does_not_add_an_empty_record() {
        assert_eq!(rows("a,b\r\n"), vec![vec!["a", "b"]]);
    }

    #[test]
    fn a_quoted_field_may_span_lines() {
        assert_eq!(
            rows("id,\"one\ntwo\"\nnext,x"),
            vec![vec!["id", "one\ntwo"], vec!["next", "x"]]
        );
    }

    #[test]
    fn an_empty_source_yields_no_records() {
        assert_eq!(parse_csv("").expect("parses"), Vec::<Vec<String>>::new());
    }

    #[test]
    fn an_unterminated_quote_is_an_error() {
        assert!(parse_csv("id,\"never closed").is_err());
    }

    #[test]
    fn text_after_a_closing_quote_is_an_error() {
        assert!(parse_csv(r#"id,"closed"trailing"#).is_err());
    }

    #[test]
    fn errors_point_at_the_line_to_fix() {
        let error = parse_csv("a,b\nc,\"unterminated").expect_err("fails");
        assert_eq!(error.line, 2);
    }

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(
            interpolate(
                "the {who} in the {where}",
                &[("who", "wolf"), ("where", "den")]
            ),
            "the wolf in the den"
        );
    }

    #[test]
    fn a_template_without_braces_is_returned_as_is() {
        assert_eq!(interpolate("plain text", &[]), "plain text");
    }

    /// The reason this is a single pass rather than one `str::replace` per
    /// argument: authored data must not be able to inject into its template.
    #[test]
    fn a_substituted_value_is_not_rescanned() {
        let filled = interpolate(
            "{villain} stalks {hunter}",
            &[("villain", "{hunter}"), ("hunter", "Reyes")],
        );
        assert_eq!(
            filled, "{hunter} stalks Reyes",
            "a value containing a placeholder was filled in by a later argument"
        );
    }

    #[test]
    fn an_unmatched_slot_stays_visible() {
        // Loud in release; the debug assertion covers development.
        #[cfg(not(debug_assertions))]
        assert_eq!(interpolate("a {missing} slot", &[]), "a {missing} slot");
    }

    #[test]
    fn an_unbalanced_brace_keeps_its_tail() {
        assert_eq!(interpolate("open {brace", &[]), "open {brace");
    }
}
