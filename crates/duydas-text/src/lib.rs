//! Duydaş text stack (ADR 0021, step 4).
//!
//! Unicode text segmentation from Unicode 18.0 data (ADR 0025):
//!
//! - [`graphemes`]: extended grapheme clusters (UAX #29), the unit of cursor movement and
//!   deletion;
//! - [`words`]: word boundaries (UAX #29), for selection and moving by word;
//! - [`line_breaks`]: line break opportunities (UAX #14), with the Japanese rules that keep
//!   closing brackets, small kana and punctuation off the start of a line.
//!
//! Every function takes byte offsets into UTF-8 text. All three algorithms pass the Unicode
//! conformance tests.
//!
//! Fonts (ADR 0026): [`font`] parses OpenType fonts and rasterizes their glyphs.

pub mod font;
mod grapheme;
mod line;
mod props;
#[rustfmt::skip]
mod tables;
#[cfg(test)]
mod tests;
mod word;

pub use grapheme::{
    Graphemes, graphemes, is_grapheme_boundary, next_grapheme_boundary, prev_grapheme_boundary,
};
pub use line::{LineBreak, LineBreaks, Strictness, line_breaks};
pub use word::{Words, is_word, next_word_boundary, prev_word_boundary, words};

/// The version of the Unicode Character Database the tables come from, as (major, minor,
/// update).
pub const UNICODE_VERSION: (u8, u8, u8) = tables::UNICODE_VERSION;

/// `index` clamped to the text and moved back to a character boundary.
fn floor_boundary(text: &str, index: usize) -> usize {
    text.floor_char_boundary(index.min(text.len()))
}

/// The character ending at `index` (a character boundary) and where it starts.
fn char_before(text: &str, index: usize) -> Option<(usize, char)> {
    let c = text[..index].chars().next_back()?;
    Some((index - c.len_utf8(), c))
}
