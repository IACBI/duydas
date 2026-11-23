//! Extended grapheme clusters (UAX #29, section 3): what a user sees as one character, such as
//! `İ` written as I + combining dot, a flag, or a family emoji. Cursor movement, deletion and
//! emergency line breaks work in these units.

use crate::props::{Gcb, InCb, Props, props};
use crate::{char_before, floor_boundary};

/// Iterator over the grapheme clusters of a string; see [`graphemes`].
#[derive(Debug, Clone)]
pub struct Graphemes<'a> {
    text: &'a str,
    position: usize,
}

/// Splits `text` into extended grapheme clusters.
///
/// ```
/// let clusters: Vec<&str> = duydas_text::graphemes("I\u{307}ğ🇹🇷").collect();
/// assert_eq!(clusters, ["I\u{307}", "ğ", "🇹🇷"]);
/// ```
#[must_use]
pub fn graphemes(text: &str) -> Graphemes<'_> {
    Graphemes { text, position: 0 }
}

impl<'a> Iterator for Graphemes<'a> {
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

/// The first grapheme boundary after `index`, or the end of the text. An index inside a
/// character counts from that character's start.
#[must_use]
pub fn next_grapheme_boundary(text: &str, index: usize) -> usize {
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

/// The last grapheme boundary before `index`, or 0. An index inside a character counts from
/// that character's start.
#[must_use]
pub fn prev_grapheme_boundary(text: &str, index: usize) -> usize {
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

/// Whether `index` is a grapheme boundary. The start and end of the text are boundaries;
/// indices inside a character or past the end are not.
#[must_use]
pub fn is_grapheme_boundary(text: &str, index: usize) -> bool {
    if index == 0 || index == text.len() {
        return true;
    }
    if !text.is_char_boundary(index) || index > text.len() {
        return false;
    }
    let before = char_before(text, index).map_or(0, |(i, _)| i);
    next_grapheme_boundary(text, before) == index
}

/// A boundary at or before `index` (a character boundary) that is certain without looking
/// further left: the start of the text, or a character before which every rule breaks. The
/// walk back is as long as the cluster, or as the run in a run of regional indicators.
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
        let previous = props(previous);
        if here.gcb == Gcb::RegionalIndicator && previous.gcb == Gcb::RegionalIndicator {
            // Flags pair up from the start of the run: a boundary comes before every regional
            // indicator preceded by an even number of them.
            let mut run = 0_usize;
            let mut start = index;
            while let Some((i, c)) = char_before(text, start)
                && props(c).gcb == Gcb::RegionalIndicator
            {
                run += 1;
                start = i;
            }
            if run.is_multiple_of(2) {
                return index;
            }
            if run > 1 {
                // The regional indicator before this one starts a pair.
                return previous_index;
            }
            index = start;
            continue;
        }
        // Every other rule but GB9c and GB11 looks only at the two characters around the
        // position. Those two also look further back, so where they could apply the position
        // is not certain.
        let further_back = (here.incb == InCb::Consonant && after_linker(text, index))
            || (here.extended_pictographic() && after_pictographic_zwj(text, index));
        if !further_back && State::new(previous).breaks_before(here) {
            return index;
        }
        index = previous_index;
    }
}

/// Whether `index` follows Extended_Pictographic Extend* ZWJ, the context of GB11.
fn after_pictographic_zwj(text: &str, index: usize) -> bool {
    let mut before = text[..index].chars().rev().map(props);
    if before.next().map(|p| p.gcb) != Some(Gcb::Zwj) {
        return false;
    }
    before
        .find(|p| p.gcb != Gcb::Extend)
        .is_some_and(Props::extended_pictographic)
}

/// Whether `index` follows InCB=Linker InCB=Extend*, the context of GB9c.
fn after_linker(text: &str, index: usize) -> bool {
    for c in text[..index].chars().rev() {
        match props(c).incb {
            InCb::Linker => return true,
            InCb::Extend => {}
            _ => return false,
        }
    }
    false
}

/// The next grapheme boundary after `start`, which must be a boundary.
pub(crate) fn boundary_after(text: &str, start: usize) -> usize {
    let mut chars = text[start..].char_indices();
    let Some((_, first)) = chars.next() else {
        return start;
    };
    let mut state = State::new(props(first));
    for (offset, c) in chars {
        let next = props(c);
        if state.breaks_before(next) {
            return start + offset;
        }
        state.advance(next);
    }
    text.len()
}

/// What the rules need to know about the text before a position.
struct State {
    previous: Props,
    /// Regional indicators immediately before, for GB12 and GB13.
    regional: usize,
    /// After Extended_Pictographic Extend*, for GB11.
    pictographic: bool,
    /// After Extended_Pictographic Extend* ZWJ, for GB11.
    pictographic_zwj: bool,
    /// After InCB=Linker InCB=Extend*, for GB9c.
    linker: bool,
}

impl State {
    fn new(first: Props) -> Self {
        let mut state = Self {
            previous: first,
            regional: 0,
            pictographic: false,
            pictographic_zwj: false,
            linker: false,
        };
        state.track(first);
        state
    }

    fn breaks_before(&self, next: Props) -> bool {
        use Gcb::*;
        let previous = self.previous.gcb;
        match (previous, next.gcb) {
            // GB3, then GB4 and GB5: break around controls, but not inside CR LF.
            (Cr, Lf) => false,
            (Control | Cr | Lf, _) | (_, Control | Cr | Lf) => true,
            // GB6-GB8: Hangul syllables.
            (L, L | V | Lv | Lvt) | (Lv | V, V | T) | (Lvt | T, T) => false,
            // GB9, GB9a, GB9b: extending characters, spacing marks, prepended characters.
            (_, Extend | Zwj | SpacingMark) | (Prepend, _) => false,
            // GB9c: Indic conjuncts.
            _ if self.linker && next.incb == InCb::Consonant => false,
            // GB11: emoji ZWJ sequences.
            _ if self.pictographic_zwj && next.extended_pictographic() => false,
            // GB12, GB13: flags are pairs of regional indicators.
            (RegionalIndicator, RegionalIndicator) => self.regional.is_multiple_of(2),
            // GB999.
            _ => true,
        }
    }

    fn advance(&mut self, next: Props) {
        self.previous = next;
        self.track(next);
    }

    fn track(&mut self, c: Props) {
        self.regional = if c.gcb == Gcb::RegionalIndicator {
            self.regional + 1
        } else {
            0
        };
        self.pictographic_zwj = self.pictographic && c.gcb == Gcb::Zwj;
        self.pictographic =
            c.extended_pictographic() || (self.pictographic && c.gcb == Gcb::Extend);
        self.linker = match c.incb {
            InCb::Linker => true,
            InCb::Extend => self.linker,
            _ => false,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish_and_japanese_clusters() {
        let clusters: Vec<&str> = graphemes("İstanbul'da ı\u{307} ｶﾞ が か\u{3099}").collect();
        assert_eq!(clusters[0], "İ");
        assert!(clusters.contains(&"ı\u{307}"));
        assert!(
            clusters.contains(&"か\u{3099}"),
            "combining voiced mark joins its kana"
        );
        assert!(
            clusters.contains(&"ｶﾞ"),
            "halfwidth voiced mark is a spacing extender"
        );
    }

    #[test]
    fn emoji_sequences_are_single_clusters() {
        let family = "👨\u{200D}👩\u{200D}👧";
        let thumbs = "👍🏽";
        let flags = "🇹🇷🇯🇵";
        let text = format!("{family}{thumbs}{flags}\r\n");
        let clusters: Vec<&str> = graphemes(&text).collect();
        assert_eq!(clusters, [family, thumbs, "🇹🇷", "🇯🇵", "\r\n"]);
    }

    #[test]
    fn cursor_moves_by_cluster() {
        let text = "a👨\u{200D}👩b";
        assert_eq!(next_grapheme_boundary(text, 0), 1);
        assert_eq!(next_grapheme_boundary(text, 1), text.len() - 1);
        assert_eq!(
            next_grapheme_boundary(text, 3),
            text.len() - 1,
            "inside a character"
        );
        assert_eq!(prev_grapheme_boundary(text, text.len() - 1), 1);
        assert_eq!(prev_grapheme_boundary(text, 1), 0);
        assert_eq!(prev_grapheme_boundary(text, 0), 0);
        assert_eq!(next_grapheme_boundary(text, 99), text.len());
        assert!(is_grapheme_boundary(text, 1));
        assert!(!is_grapheme_boundary(text, 5));
        assert!(!is_grapheme_boundary(text, 99));
    }

    #[test]
    fn regional_indicator_parity_from_the_middle() {
        let flags = "🇹🇷🇯🇵🇩🇪";
        // Each flag is two four-byte regional indicators.
        assert_eq!(prev_grapheme_boundary(flags, 24), 16);
        assert_eq!(next_grapheme_boundary(flags, 12), 16);
        assert_eq!(prev_grapheme_boundary(flags, 12), 8);
        // A prepended mark before the run joins the first indicator.
        let prepended = "\u{600}🇹🇷";
        assert_eq!(prev_grapheme_boundary(prepended, prepended.len()), 0);
        assert_eq!(next_grapheme_boundary(prepended, 0), prepended.len());
        assert_eq!(prev_grapheme_boundary(prepended, 6), 0);
    }
}
