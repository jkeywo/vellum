//! Filling `{name}` slots, and reading which slots a template asks for.

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

/// The `{named}` slots a template asks for, in order of first appearance.
///
/// What the cross-locale check compares: a translation that drops `{n}` or
/// invents `{count}` is a bug the audit can name before a player sees a
/// half-filled sentence.
pub fn placeholders_in(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                if !name.is_empty() && !names.iter().any(|seen| seen == name) {
                    names.push(name.to_owned());
                }
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_named_slots() {
        let out = interpolate(
            "{who} boarded {what}",
            &[("who", "Bob"), ("what", "a hulk")],
        );
        assert_eq!(out, "Bob boarded a hulk");
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "no argument for placeholder")
    )]
    fn an_unfilled_slot_stays_visible() {
        let out = interpolate("held by {holder}", &[]);
        #[cfg(not(debug_assertions))]
        assert_eq!(out, "held by {holder}");
        #[cfg(debug_assertions)]
        let _ = out;
    }

    #[test]
    fn a_substituted_value_is_never_rescanned() {
        // The villain is literally called "{hunter}". Naive repeated replace
        // would hand back the hunter's name.
        let out = interpolate(
            "{villain} stalks {hunter}",
            &[("villain", "{hunter}"), ("hunter", "Anna")],
        );
        assert_eq!(out, "{hunter} stalks Anna");
    }

    #[test]
    fn placeholders_are_listed_once_in_order() {
        assert_eq!(
            placeholders_in("{a} then {b} then {a}"),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert!(placeholders_in("no slots here").is_empty());
    }
}
