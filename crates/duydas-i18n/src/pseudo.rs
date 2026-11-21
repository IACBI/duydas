//! Pseudo-localization: accented, bracketed and ~40% longer text, to expose hardcoded strings,
//! truncation and layouts that cannot absorb translation expansion.

pub(crate) fn transform(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    out.push('[');
    let mut letters = 0usize;
    for c in input.chars() {
        if c.is_alphabetic() {
            letters += 1;
        }
        out.push(accent(c));
    }
    for _ in 0..letters.div_ceil(5) * 2 {
        out.push('·');
    }
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
        let out = super::transform("Settings");
        assert!(out.starts_with("[Šéţţîñĝš"));
        assert!(out.ends_with(']'));
        assert!(out.chars().count() >= "Settings".len() * 14 / 10);
    }
}
