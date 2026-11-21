//! Pseudo-localization: accented, bracketed and ~40% longer text, to expose hardcoded strings,
//! truncation and layouts that cannot absorb translation expansion.

use std::borrow::Cow;

/// Fluent transform for pattern text. Interpolated values (user data such as channel names) are
/// not passed through it, so they stay readable.
pub(crate) fn accent_text(input: &str) -> Cow<'_, str> {
    if input.chars().any(|c| accent(c) != c) {
        Cow::Owned(input.chars().map(accent).collect())
    } else {
        Cow::Borrowed(input)
    }
}

/// Brackets the finished message and pads it by roughly 40% of its letters.
pub(crate) fn wrap(text: &str) -> String {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let padding = letters.div_ceil(5) * 2;
    let mut out = String::with_capacity(text.len() + padding * 2 + 2);
    out.push('[');
    out.push_str(text);
    out.extend(std::iter::repeat_n('·', padding));
    out.push(']');
    out
}

fn accent(c: char) -> char {
    match c {
        'a' => 'å',
        'b' => 'ƀ',
        'c' => 'ç',
        'd' => 'ð',
        'e' => 'é',
        'f' => 'ƒ',
        'g' => 'ĝ',
        'h' => 'ĥ',
        'i' => 'î',
        'j' => 'ĵ',
        'k' => 'ķ',
        'l' => 'ļ',
        'm' => 'ɱ',
        'n' => 'ñ',
        'o' => 'ö',
        'p' => 'þ',
        'r' => 'ŕ',
        's' => 'š',
        't' => 'ţ',
        'u' => 'û',
        'w' => 'ŵ',
        'y' => 'ý',
        'z' => 'ž',
        'A' => 'Å',
        'C' => 'Ç',
        'D' => 'Ð',
        'E' => 'É',
        'I' => 'Î',
        'L' => 'Ļ',
        'N' => 'Ñ',
        'O' => 'Ö',
        'S' => 'Š',
        'T' => 'Ţ',
        'U' => 'Û',
        'Y' => 'Ý',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn expands_and_brackets() {
        let out = super::wrap(&super::accent_text("Settings"));
        assert!(out.starts_with("[Šéţţîñĝš"));
        assert!(out.ends_with(']'));
        assert!(out.chars().count() >= "Settings".len() * 14 / 10);
    }
}
