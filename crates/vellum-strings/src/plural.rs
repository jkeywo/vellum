//! Plural categories, and the per-locale rule that picks one.
//!
//! A pluralised string is *whole rows*, one per category, never a sentence
//! glued together from fragments — word order stays the translator's to
//! choose. Generalised from the game that had it first:
//!
//! ```text
//! hud.enemies.one     "1 enemy remaining"
//! hud.enemies.other   "{n} enemies remaining"
//! ```
//!
//! The categories are CLDR's. English only ever produces `One` and `Other`,
//! so the extra names cost an English-only game nothing — and the day a
//! language needs `Few`, that language adds a selector and some rows, not a
//! change to this format or to any call site.

use core::fmt;

/// A CLDR plural category.
///
/// All six exist in the type even though English uses two, because the
/// alternative is a format change the first time a locale needs a third.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl Category {
    /// The suffix this category takes on an id stem.
    pub const fn suffix(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }

    /// Every category, for the audit's "which rows should exist" question.
    pub const ALL: [Category; 6] = [
        Category::Zero,
        Category::One,
        Category::Two,
        Category::Few,
        Category::Many,
        Category::Other,
    ];

    /// Read a suffix back, for recognising a categorised id.
    pub fn from_suffix(suffix: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.suffix() == suffix)
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// A locale: its tag, and the rule that maps a count to a category.
///
/// The rule is a plain function pointer rather than a table of CLDR data.
/// Pulling in the whole of CLDR to serve one shipped language would be
/// machinery in place of a decision; a locale that needs real rules writes
/// them, in the language of whoever speaks it.
#[derive(Clone, Copy)]
pub struct Locale {
    /// BCP-47 tag: `en`, `fr`, `pt-BR`.
    pub tag: &'static str,
    /// Which categories this locale can produce, in the order the audit
    /// expects rows for them.
    pub categories: &'static [Category],
    /// The count-to-category rule.
    pub plural: fn(i64) -> Category,
}

impl Locale {
    /// English: one, other.
    pub const ENGLISH: Locale = Locale {
        tag: "en",
        categories: &[Category::One, Category::Other],
        plural: english_plural,
    };

    /// The category a count falls into under this locale.
    pub fn category(&self, count: i64) -> Category {
        (self.plural)(count)
    }
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locale")
            .field("tag", &self.tag)
            .field("categories", &self.categories)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Locale {
    /// Two locales are the same locale if they are the same tag. The rule is
    /// a function pointer and comparing those is not meaningful.
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

impl Eq for Locale {}

fn english_plural(count: i64) -> Category {
    if count == 1 {
        Category::One
    } else {
        Category::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_splits_one_from_everything_else() {
        let en = Locale::ENGLISH;
        assert_eq!(en.category(1), Category::One);
        for count in [0, 2, 7, -1, 1000] {
            assert_eq!(en.category(count), Category::Other, "count {count}");
        }
    }

    #[test]
    fn suffixes_round_trip() {
        for category in Category::ALL {
            assert_eq!(Category::from_suffix(category.suffix()), Some(category));
        }
        assert_eq!(Category::from_suffix("name"), None);
    }

    /// A locale with a third category needs no change here — it supplies its
    /// own rule and lists the categories it can produce.
    #[test]
    fn a_locale_may_declare_more_categories() {
        fn russian(count: i64) -> Category {
            match (count % 10, count % 100) {
                (1, n) if n != 11 => Category::One,
                (2..=4, n) if !(12..=14).contains(&n) => Category::Few,
                _ => Category::Many,
            }
        }
        let ru = Locale {
            tag: "ru",
            categories: &[Category::One, Category::Few, Category::Many],
            plural: russian,
        };
        assert_eq!(ru.category(1), Category::One);
        assert_eq!(ru.category(3), Category::Few);
        assert_eq!(ru.category(11), Category::Many);
        assert_eq!(ru.category(22), Category::Few);
    }
}
