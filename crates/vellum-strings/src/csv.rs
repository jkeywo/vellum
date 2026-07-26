//! The CSV reader.
//!
//! Unchanged from when this crate was only a reader and a filler: three games
//! parse their tables with it, and the format is the one a translator opens in
//! a spreadsheet.

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
///
/// This is the fleet's *only* implementation of the format, deliberately. A
/// JavaScript page reads [`Table::to_json`](crate::Table::to_json) output
/// produced by this parser rather than parsing the CSV itself, so a page can
/// never disagree with Rust about what a row says.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_and_quoted_fields() {
        let rows = parse_csv("a,b\n\"c,d\",e\n").unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["c,d", "e"]]);
    }

    #[test]
    fn a_doubled_quote_is_one_quote() {
        let rows = parse_csv("\"say \"\"hi\"\"\",x\n").unwrap();
        assert_eq!(rows, vec![vec!["say \"hi\"", "x"]]);
    }

    #[test]
    fn newlines_survive_inside_quotes() {
        let rows = parse_csv("\"one\ntwo\",x\n").unwrap();
        assert_eq!(rows, vec![vec!["one\ntwo", "x"]]);
    }

    #[test]
    fn crlf_reads_the_same_as_lf() {
        assert_eq!(parse_csv("a,b\r\nc,d\r\n"), parse_csv("a,b\nc,d\n"));
    }

    #[test]
    fn malformed_input_is_an_error_not_a_guess() {
        assert!(parse_csv("\"unterminated\n").is_err());
        assert!(parse_csv("\"closed\" then more,x\n").is_err());
        assert!(parse_csv("bare\"quote,x\n").is_err());
    }

    #[test]
    fn a_trailing_row_without_a_newline_is_kept() {
        let rows = parse_csv("a,b\nc,d").unwrap();
        assert_eq!(rows.len(), 2);
    }
}
