//! Locale data and formatting for the shipped languages: plural categories, integer grouping,
//! long dates and case mapping. Hand-written from CLDR 46 for en, tr and ja; adding a language
//! means adding its rules here (see docs/i18n-guide.md).

/// The language whose rules apply. Region does not change any rule the app uses yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    En,
    Tr,
    Ja,
}

impl Lang {
    /// CLDR plural category for an integer.
    pub(crate) fn plural_category(self, n: i64) -> &'static str {
        match self {
            // en: one if i = 1 and v = 0. tr: one if n = 1. Operands use the absolute value, so
            // for integers both rules mean |n| == 1.
            Lang::En | Lang::Tr if n.unsigned_abs() == 1 => "one",
            Lang::En | Lang::Tr | Lang::Ja => "other",
        }
    }

    /// CLDR plural category for a number literal as written, where visible fraction digits
    /// matter: English "1.0" is other, Turkish "1.0" is one.
    pub(crate) fn plural_category_literal(self, literal: &str) -> &'static str {
        let (_, int, fraction) = decimal_parts(literal);
        match self {
            Lang::En if int == "1" && fraction.is_empty() => "one",
            Lang::Tr if int == "1" && fraction.bytes().all(|b| b == b'0') => "one",
            Lang::En | Lang::Tr | Lang::Ja => "other",
        }
    }

    fn group_separator(self) -> char {
        match self {
            Lang::En | Lang::Ja => ',',
            Lang::Tr => '.',
        }
    }

    /// Integer with grouping (CLDR minimum grouping digits is 1 for all three languages).
    pub(crate) fn format_integer(self, value: i64) -> String {
        let digits = value.unsigned_abs().to_string();
        let separator = self.group_separator();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
        if value < 0 {
            out.push('-');
        }
        for (i, digit) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(separator);
            }
            out.push(digit);
        }
        out
    }

    /// Long date (CLDR yMMMMd), or None if the date does not exist or the year is before 1 CE
    /// (eras are not supported).
    pub(crate) fn format_date(self, year: i32, month: u8, day: u8) -> Option<String> {
        if year < 1 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        let index = usize::from(month - 1);
        Some(match self {
            Lang::En => format!("{} {day}, {year}", EN_MONTHS[index]),
            Lang::Tr => format!("{day} {} {year}", TR_MONTHS[index]),
            Lang::Ja => format!("{year}年{month}月{day}日"),
        })
    }

    /// Uppercases with the language's rules. Turkish maps dotted i to İ; std's mapping (full
    /// Unicode, e.g. ß to SS) covers the rest and is language-neutral.
    pub(crate) fn uppercase(self, text: &str) -> String {
        match self {
            Lang::Tr => {
                let mut out = String::with_capacity(text.len());
                for c in text.chars() {
                    if c == 'i' {
                        out.push('İ');
                    } else {
                        out.extend(c.to_uppercase());
                    }
                }
                out
            }
            Lang::En | Lang::Ja => text.to_uppercase(),
        }
    }
}

const EN_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const TR_MONTHS: [&str; 12] = [
    "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
    "Kasım", "Aralık",
];

/// Splits a number literal into sign, integer digits without leading zeros, and fraction digits
/// as written.
fn decimal_parts(literal: &str) -> (bool, &str, &str) {
    let (negative, rest) = match literal.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, literal),
    };
    let (int, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    let int = int.trim_start_matches('0');
    (negative, if int.is_empty() { "0" } else { int }, fraction)
}

/// A number literal for display: leading integer zeros dropped, fraction digits kept as written
/// ("0007.50" becomes "7.50"), like Fluent's own number output.
pub(crate) fn normalize_literal(literal: &str) -> String {
    let (negative, int, fraction) = decimal_parts(literal);
    let sign = if negative { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{fraction}")
    }
}

/// Whether two number literals have the same value ("1.0" = "1", "-0" = "0"), compared as
/// decimal text so the whole i64 range stays exact.
pub(crate) fn decimal_eq(a: &str, b: &str) -> bool {
    fn normalized(literal: &str) -> (bool, &str, &str) {
        let (negative, int, fraction) = decimal_parts(literal);
        let fraction = fraction.trim_end_matches('0');
        let zero = int == "0" && fraction.is_empty();
        (negative && !zero, int, fraction)
    }
    normalized(a) == normalized(b)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::{Lang, decimal_eq};

    #[test]
    fn plural_categories() {
        for n in [0, 2, 5, 11, 21, 101, -2] {
            assert_eq!(Lang::En.plural_category(n), "other", "{n}");
            assert_eq!(Lang::Tr.plural_category(n), "other", "{n}");
        }
        assert_eq!(Lang::En.plural_category(1), "one");
        assert_eq!(Lang::Tr.plural_category(1), "one");
        assert_eq!(Lang::Ja.plural_category(1), "other");
        assert_eq!(Lang::En.plural_category(-1), "one");
        assert_eq!(Lang::Tr.plural_category(i64::MIN), "other");
    }

    #[test]
    fn plural_categories_of_literals() {
        assert_eq!(Lang::En.plural_category_literal("1"), "one");
        assert_eq!(Lang::En.plural_category_literal("1.0"), "other");
        assert_eq!(Lang::Tr.plural_category_literal("1.0"), "one");
        assert_eq!(Lang::Tr.plural_category_literal("1.5"), "other");
        assert_eq!(Lang::Tr.plural_category_literal("-01"), "one");
        assert_eq!(Lang::Ja.plural_category_literal("1"), "other");
    }

    #[test]
    fn decimal_equality() {
        assert!(decimal_eq("1", "1.0"));
        assert!(decimal_eq("-0", "0.00"));
        assert!(decimal_eq("007", "7"));
        assert!(!decimal_eq("-1", "1"));
        assert!(!decimal_eq("9007199254740993", "9007199254740992"));
    }

    #[test]
    fn integer_grouping_edges() {
        assert_eq!(Lang::En.format_integer(0), "0");
        assert_eq!(Lang::En.format_integer(999), "999");
        assert_eq!(Lang::En.format_integer(1000), "1,000");
        assert_eq!(Lang::Tr.format_integer(-1_234_567), "-1.234.567");
        assert_eq!(
            Lang::En.format_integer(i64::MIN),
            "-9,223,372,036,854,775,808"
        );
    }

    #[test]
    fn leap_years() {
        assert!(Lang::En.format_date(2024, 2, 29).is_some());
        assert!(Lang::En.format_date(2000, 2, 29).is_some());
        assert!(Lang::En.format_date(1900, 2, 29).is_none());
        assert!(Lang::En.format_date(2026, 4, 31).is_none());
        assert!(Lang::En.format_date(2026, 13, 1).is_none());
        assert!(Lang::En.format_date(2026, 1, 0).is_none());
        assert!(Lang::En.format_date(0, 1, 1).is_none());
    }

    #[test]
    fn turkish_case_mapping_keeps_other_letters() {
        assert_eq!(Lang::Tr.uppercase("iıöüşçğ straße"), "İIÖÜŞÇĞ STRASSE");
    }
}
