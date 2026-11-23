//! Cross-checks between the segmenters on random text: random access (the boundary functions
//! that start in the middle) must agree with iterating from the start, and every iterator must
//! produce valid, increasing boundaries.

use crate::{
    Strictness, graphemes, is_grapheme_boundary, line_breaks, next_grapheme_boundary,
    next_word_boundary, prev_grapheme_boundary, prev_word_boundary, words,
};

/// Characters that exercise every rule with context: flags, emoji sequences, Hangul, Indic
/// conjuncts, prepended marks, Hebrew, numbers, quotes, newlines, Japanese.
#[rustfmt::skip]
const ALPHABET: &[char] = &[
    'a', 'İ', 'ı', '\u{307}', '1', '.', ',', ':', ';', '\'', '"', ' ', '\u{3000}',
    '\r', '\n', '\u{85}', '\u{200D}', '\u{200B}', '\u{2060}', '\u{AD}', '\u{1F1F9}', '\u{1F1F7}',
    '\u{1F468}', '\u{1F3FD}', '\u{1F44D}', '\u{2764}', '\u{FE0F}', '\u{1100}', '\u{1161}',
    '\u{11A8}', '\u{AC00}', '\u{0915}', '\u{094D}', '\u{093F}', '\u{0600}', '\u{05D0}', '-',
    '\u{2010}', '(', ')', '「', '」', '。', '、', 'ー', 'ぁ', 'っ', 'カ', 'ｶ', '\u{FF9E}', '漢', '$', '%',
    '\u{201C}', '\u{201D}', '\u{AB}', '\u{BB}', '_', '\u{25CC}', '\u{1B05}', '\u{1B44}',
    '\u{0E01}', '\u{0E31}', '\u{1F000}', '\u{E0061}',
];

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn text(&mut self, any: bool) -> String {
        let len = self.below(24);
        (0..len)
            .map(|_| {
                if any && self.below(4) == 0 {
                    // Any scalar value, unassigned ones included.
                    loop {
                        let cp = (self.next() % 0x11_0000) as u32;
                        if let Some(c) = char::from_u32(cp) {
                            break c;
                        }
                    }
                } else {
                    ALPHABET[self.below(ALPHABET.len())]
                }
            })
            .collect()
    }
}

fn ends<'a>(segments: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut end = 0;
    segments
        .map(|s| {
            end += s.len();
            end
        })
        .collect()
}

/// For every byte index, the expected next and previous boundary given the full list.
fn check_random_access(
    text: &str,
    boundaries: &[usize],
    next: fn(&str, usize) -> usize,
    prev: fn(&str, usize) -> usize,
) {
    for index in 0..=text.len() + 2 {
        let start = text.floor_char_boundary(index.min(text.len()));
        let expected_next = boundaries
            .iter()
            .copied()
            .find(|&b| b > start)
            .unwrap_or(text.len());
        let expected_prev = std::iter::once(0)
            .chain(boundaries.iter().copied())
            .rfind(|&b| b < start)
            .unwrap_or(0);
        assert_eq!(
            next(text, index),
            expected_next,
            "next from {index} in {text:?}"
        );
        assert_eq!(
            prev(text, index),
            expected_prev,
            "prev from {index} in {text:?}"
        );
    }
}

#[test]
fn random_access_agrees_with_iteration() {
    let mut rng = Rng(0x5EED_D0D0_CAFE_F00D);
    for round in 0..4000 {
        let text = rng.text(round % 2 == 1);
        let clusters = ends(graphemes(&text));
        check_random_access(
            &text,
            &clusters,
            next_grapheme_boundary,
            prev_grapheme_boundary,
        );
        for index in 0..=text.len() {
            let expected = index == 0 || clusters.contains(&index);
            assert_eq!(
                is_grapheme_boundary(&text, index),
                expected,
                "{index} in {text:?}"
            );
        }
        let word_ends = ends(words(&text));
        check_random_access(&text, &word_ends, next_word_boundary, prev_word_boundary);
        // Words and line breaks never split a grapheme cluster here, except where UAX #14
        // and UAX #29 deliberately differ (a space before a combining mark, LB10).
        for b in &word_ends {
            assert!(text.is_char_boundary(*b));
        }
    }
}

#[test]
fn line_breaks_are_valid_on_random_text() {
    let mut rng = Rng(0xFACE_B00C_0BAD_BEEF);
    for round in 0..4000 {
        let text = rng.text(round % 2 == 1);
        for strictness in [Strictness::Strict, Strictness::Loose] {
            let all: Vec<_> = line_breaks(&text, strictness).collect();
            if text.is_empty() {
                assert!(all.is_empty());
                continue;
            }
            let last = all.last().expect("a non-empty text ends with a break");
            assert_eq!((last.index, last.mandatory), (text.len(), true), "{text:?}");
            for pair in all.windows(2) {
                assert!(pair[0].index < pair[1].index, "{text:?}");
            }
            for b in &all {
                assert!(b.index > 0 && text.is_char_boundary(b.index), "{text:?}");
                let before = text[..b.index].chars().next_back();
                let hard = matches!(
                    before,
                    Some('\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{B}' | '\u{C}')
                );
                if b.index < text.len() {
                    assert_eq!(b.mandatory, hard, "{text:?} at {}", b.index);
                }
            }
        }
    }
}

#[test]
fn strictness_only_changes_breaks_next_to_small_kana() {
    use crate::props::{Lb, props};
    let mut rng = Rng(0x0DDB_A11C_AB1E_5EED);
    for _ in 0..4000 {
        let text = rng.text(false);
        let strict: Vec<usize> = line_breaks(&text, Strictness::Strict)
            .map(|b| b.index)
            .collect();
        let normal: Vec<usize> = line_breaks(&text, Strictness::Loose)
            .map(|b| b.index)
            .collect();
        let differ = strict
            .iter()
            .filter(|b| !normal.contains(b))
            .chain(normal.iter().filter(|b| !strict.contains(b)));
        for &index in differ {
            // The base character before the position (past combining marks), or the one after.
            let before = text[..index]
                .chars()
                .rev()
                .find(|&c| !matches!(props(c).lb, Lb::Cm | Lb::Zwj));
            let after = text[index..].chars().next();
            let near_cj = [before, after]
                .into_iter()
                .flatten()
                .any(|c| props(c).lb == Lb::Cj);
            assert!(near_cj, "{text:?} at {index}");
        }
    }
}
