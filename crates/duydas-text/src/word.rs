//! Word boundaries (UAX #29, section 4), for double-click selection and moving the cursor by
//! word. The default rules keep letters, numbers and katakana runs together and split
//! ideographs and hiragana into single characters; Japanese words need a dictionary, which
//! UAX #29 leaves to tailoring.

use crate::props::{Wb, props};
use crate::{char_before, floor_boundary};

/// Iterator over the segments between word boundaries; see [`words`].
#[derive(Debug, Clone)]
pub struct Words<'a> {
    text: &'a str,
    position: usize,
}

/// Splits `text` at every word boundary. Spaces and punctuation come out as segments of their
/// own; [`is_word`] tells words from them.
///
/// These are UAX #29's word boundaries, which can fall inside a grapheme cluster in rare cases
/// (a virama joining two consonants across them). A caret should stop only where
/// [`is_grapheme_boundary`](crate::is_grapheme_boundary) agrees; to filter many boundaries,
/// walk [`graphemes`](crate::graphemes) alongside instead.
///
/// ```
/// let words: Vec<&str> = duydas_text::words("Kaç kişi? 3,5 m.").collect();
/// assert_eq!(words, ["Kaç", " ", "kişi", "?", " ", "3,5", " ", "m", "."]);
/// ```
#[must_use]
pub fn words(text: &str) -> Words<'_> {
    Words { text, position: 0 }
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.position >= self.text.len() {
            return None;
        }
        let start = self.position;
        self.position = boundary_after(self.text, start);
        Some(&self.text[start..self.position])
    }
}

/// Whether a segment from [`words`] is a word: it contains an alphabetic character (letters,
/// ideographs, kana) or a number, by this crate's Unicode version.
#[must_use]
pub fn is_word(segment: &str) -> bool {
    segment.chars().any(|c| props(c).word_character())
}

/// The first word boundary after `index`, or the end of the text. An index inside a character
/// counts from that character's start, and one past the end counts as the end.
#[must_use]
pub fn next_word_boundary(text: &str, index: usize) -> usize {
    let index = floor_boundary(text, index);
    if index >= text.len() {
        return text.len();
    }
    let mut boundary = safe_start(text, index);
    loop {
        boundary = boundary_after(text, boundary);
        if boundary > index {
            return boundary;
        }
    }
}

/// The last word boundary before `index`, or 0. An index inside a character counts from that
/// character's start, so `prev_word_boundary(" é", 2)` is 0, the boundary before the space;
/// one past the end counts as the end.
#[must_use]
pub fn prev_word_boundary(text: &str, index: usize) -> usize {
    let index = floor_boundary(text, index);
    let Some((before, _)) = char_before(text, index) else {
        return 0;
    };
    let mut boundary = safe_start(text, before);
    loop {
        let next = boundary_after(text, boundary);
        if next >= index {
            return boundary;
        }
        boundary = next;
    }
}

/// A boundary at or before `index` that is certain without looking further left. Starting
/// there loses no context: the rules that look back past a position (WB7, WB7c, WB11) only
/// join where a rule at the position itself (WB6, WB7b, WB12) joins too. The walk back is as
/// long as the word, or as the run in a run of regional indicators.
fn safe_start(text: &str, index: usize) -> usize {
    let mut index = index;
    loop {
        let Some(c) = text[index..].chars().next() else {
            return index;
        };
        let Some((previous_index, previous)) = char_before(text, index) else {
            return index;
        };
        let here = props(c);
        let raw = props(previous).wb;
        let certain = match here.wb {
            Wb::Newline | Wb::Cr => true,
            Wb::Lf => raw != Wb::Cr,
            // WB4 attaches these to the unit before them, except after a newline.
            Wb::Extend | Wb::Format | Wb::Zwj => is_newline(raw),
            class => {
                let (start, unit) = unit_before(text, index);
                let context = Context {
                    before: (start > 0).then(|| unit_before(text, start).1),
                    a: unit,
                    raw,
                    b: class,
                    pictographic: here.extended_pictographic(),
                    after: unit_after(text, index),
                };
                !context.may_join()
            }
        };
        if certain {
            return index;
        }
        index = previous_index;
    }
}

fn is_newline(class: Wb) -> bool {
    matches!(class, Wb::Newline | Wb::Cr | Wb::Lf)
}

fn is_ignored(class: Wb) -> bool {
    matches!(class, Wb::Extend | Wb::Format | Wb::Zwj)
}

/// The start and class of the WB4 unit ending at `index` (greater than 0).
fn unit_before(text: &str, index: usize) -> (usize, Wb) {
    // The last run of Extend, Format and ZWJ characters, and the character before it.
    let mut first = None;
    for (i, c) in text[..index].char_indices().rev() {
        let wb = props(c).wb;
        if !is_ignored(wb) {
            // A newline absorbs nothing; the unit then starts after it.
            return match first {
                Some(unit) if is_newline(wb) => unit,
                _ => (i, wb),
            };
        }
        first = Some((i, wb));
    }
    first.unwrap_or((0, Wb::Other))
}

/// The class of the WB4 unit after the one that starts at `index`.
fn unit_after(text: &str, index: usize) -> Option<Wb> {
    let mut chars = text[index..].chars();
    let first = props(chars.next()?).wb;
    let absorbs = !is_newline(first);
    chars
        .map(|c| props(c).wb)
        .find(|&wb| !(absorbs && is_ignored(wb)))
}

/// A position between units `a` and `b`, with the units around them.
struct Context {
    before: Option<Wb>,
    a: Wb,
    /// The class of the last character before the position, for WB3c and WB3d.
    raw: Wb,
    b: Wb,
    pictographic: bool,
    after: Option<Wb>,
}

impl Context {
    /// Whether any rule keeps `a` and `b` together, given only the units around them. Regional
    /// indicator pairs depend on the whole run, so they always may.
    fn may_join(&self) -> bool {
        use Wb::*;
        let Self {
            before,
            a,
            raw,
            b,
            pictographic,
            after,
        } = *self;
        let is = |class: Option<Wb>, test: fn(Wb) -> bool| class.is_some_and(test);
        let letter_or_number = |c: Wb| ahletter(c) || c == Numeric;
        (raw == Zwj && pictographic)
            || (raw == WSegSpace && b == WSegSpace)
            || (letter_or_number(a) && letter_or_number(b))
            || (ahletter(a) && mid_letter(b) && is(after, ahletter))
            || (mid_letter(a) && ahletter(b) && is(before, ahletter))
            || (a == HebrewLetter && b == SingleQuote)
            || (a == HebrewLetter && b == DoubleQuote && after == Some(HebrewLetter))
            || (a == DoubleQuote && b == HebrewLetter && before == Some(HebrewLetter))
            || (mid_num(a) && b == Numeric && before == Some(Numeric))
            || (a == Numeric && mid_num(b) && after == Some(Numeric))
            || (a == Katakana && b == Katakana)
            || ((letter_or_number(a) || matches!(a, Katakana | ExtendNumLet)) && b == ExtendNumLet)
            || (a == ExtendNumLet && (letter_or_number(b) || b == Katakana))
            || (a == RegionalIndicator && b == RegionalIndicator)
    }
}

/// A character with the Extend, Format and ZWJ characters after it (rule WB4).
#[derive(Clone, Copy)]
struct Unit {
    start: usize,
    class: Wb,
    pictographic: bool,
    /// The class of the unit's last character, for the rules that see raw characters.
    last: Wb,
}

struct Units<'a> {
    text: &'a str,
    position: usize,
}

impl Iterator for Units<'_> {
    type Item = Unit;

    fn next(&mut self) -> Option<Unit> {
        let mut chars = self.text[self.position..].char_indices();
        let (_, first) = chars.next()?;
        let base = props(first);
        let start = self.position;
        let mut end = start + first.len_utf8();
        let mut last = base.wb;
        if !matches!(base.wb, Wb::Cr | Wb::Lf | Wb::Newline) {
            for (offset, c) in chars {
                let wb = props(c).wb;
                if !matches!(wb, Wb::Extend | Wb::Format | Wb::Zwj) {
                    break;
                }
                last = wb;
                end = start + offset + c.len_utf8();
            }
        }
        self.position = end;
        Some(Unit {
            start,
            class: base.wb,
            pictographic: base.extended_pictographic(),
            last,
        })
    }
}

/// The next word boundary after `start`, which must be a boundary.
fn boundary_after(text: &str, start: usize) -> usize {
    let mut units = Units {
        text,
        position: start,
    };
    let Some(mut a) = units.next() else {
        return start;
    };
    let mut before: Option<Unit> = None;
    let mut b = units.next();
    let mut regional = usize::from(a.class == Wb::RegionalIndicator);
    while let Some(current) = b {
        let after = units.next();
        if breaks(before, a, current, after, regional) {
            return current.start;
        }
        regional = if current.class == Wb::RegionalIndicator {
            regional + 1
        } else {
            0
        };
        before = Some(a);
        a = current;
        b = after;
    }
    text.len()
}

fn ahletter(class: Wb) -> bool {
    matches!(class, Wb::ALetter | Wb::HebrewLetter)
}

fn mid_letter(class: Wb) -> bool {
    matches!(class, Wb::MidLetter | Wb::MidNumLet | Wb::SingleQuote)
}

fn mid_num(class: Wb) -> bool {
    matches!(class, Wb::MidNum | Wb::MidNumLet | Wb::SingleQuote)
}

/// Whether there is a boundary between units `a` and `b`. `regional` counts the regional
/// indicators ending at `a`.
fn breaks(before: Option<Unit>, a: Unit, b: Unit, after: Option<Unit>, regional: usize) -> bool {
    use Wb::*;
    let before = before.map(|u| u.class);
    let after = after.map(|u| u.class);
    let is = |class: Option<Wb>, test: fn(Wb) -> bool| class.is_some_and(test);
    match (a.class, b.class) {
        // WB3, WB3a, WB3b: newlines.
        (Cr, Lf) => false,
        (Newline | Cr | Lf, _) | (_, Newline | Cr | Lf) => true,
        // WB3c, WB3d: emoji ZWJ sequences and runs of spaces, on raw characters.
        _ if a.last == Zwj && b.pictographic => false,
        _ if a.last == WSegSpace && b.class == WSegSpace => false,
        // WB4 built the units. WB5-WB7: letters, also across "e.g." and "can't".
        (x, y) if ahletter(x) && ahletter(y) => false,
        (x, y) if ahletter(x) && mid_letter(y) && is(after, ahletter) => false,
        (x, y) if mid_letter(x) && ahletter(y) && is(before, ahletter) => false,
        // WB7a-WB7c: Hebrew quotation marks.
        (HebrewLetter, SingleQuote) => false,
        (HebrewLetter, DoubleQuote) if after == Some(HebrewLetter) => false,
        (DoubleQuote, HebrewLetter) if before == Some(HebrewLetter) => false,
        // WB8-WB12: numbers, also next to letters and across "3,456.789".
        (Numeric, Numeric) => false,
        (x, Numeric) if ahletter(x) => false,
        (Numeric, y) if ahletter(y) => false,
        (x, Numeric) if mid_num(x) && before == Some(Numeric) => false,
        (Numeric, y) if mid_num(y) && after == Some(Numeric) => false,
        // WB13-WB13b: katakana and connectors such as "_".
        (Katakana, Katakana) => false,
        (x, ExtendNumLet) if ahletter(x) || matches!(x, Numeric | Katakana | ExtendNumLet) => false,
        (ExtendNumLet, y) if ahletter(y) || matches!(y, Numeric | Katakana) => false,
        // WB15, WB16: flags.
        (RegionalIndicator, RegionalIndicator) => regional.is_multiple_of(2),
        // WB999.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_segments() {
        let segments: Vec<&str> = words("カタカナと漢字、ｶﾀｶﾅ").collect();
        assert_eq!(segments, ["カタカナ", "と", "漢", "字", "、", "ｶﾀｶﾅ"]);
        assert!(is_word("漢"));
        assert!(!is_word("、"));
    }

    #[test]
    fn apostrophes_and_numbers() {
        let segments: Vec<&str> = words("İstanbul'da 1.000,5 TL").collect();
        assert_eq!(segments, ["İstanbul'da", " ", "1.000,5", " ", "TL"]);
    }

    #[test]
    fn cursor_moves_by_word() {
        let text = "merhaba  dünya";
        assert_eq!(next_word_boundary(text, 0), 7);
        assert_eq!(next_word_boundary(text, 7), 9);
        assert_eq!(next_word_boundary(text, 9), text.len());
        assert_eq!(prev_word_boundary(text, text.len()), 9);
        assert_eq!(prev_word_boundary(text, 8), 7);
        assert_eq!(prev_word_boundary(text, 3), 0);
    }
}
