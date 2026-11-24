//! Bidirectional text: the Unicode Bidirectional Algorithm (UAX #9, revision 52, Unicode 18.0).
//!
//! [`Bidi::new`] splits text into paragraphs, finds each paragraph's direction (or takes
//! the caller's), and resolves an embedding level for every character: even levels run left to
//! right, odd levels right to left. Once text is broken into lines, [`Bidi::visual_runs`] gives
//! a line's runs of one level in the order they are displayed (rules L1 and L2).
//!
//! Like the rest of the crate it works on byte offsets into UTF-8 text: levels are given per
//! byte. Explicit embedding and override characters and boundary neutrals, which the algorithm
//! sets aside (rule X9), take the level of the character before them, so they stay next to it
//! (UAX #9, section 5.2). Mirroring (L4) is left to shaping, which uses [`mirror`]. Rule L3
//! does not apply: shaping places each combining mark on its base, in either direction.
//!
//! The algorithm passes every case of the Unicode conformance tests BidiTest.txt and
//! BidiCharacterTest.txt.

use std::ops::Range;

pub use crate::bidi_tables::BidiClass;
use crate::bidi_tables::{BRACKETS, MIRRORS};

/// The deepest embedding level explicit formatting characters can reach (max_depth).
pub const MAX_DEPTH: u8 = 125;

/// The direction of a paragraph or a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Left to right: even embedding levels.
    #[default]
    LeftToRight,
    /// Right to left: odd embedding levels.
    RightToLeft,
}

impl Direction {
    /// The direction of an embedding level.
    pub fn of_level(level: u8) -> Self {
        if level.is_multiple_of(2) {
            Self::LeftToRight
        } else {
            Self::RightToLeft
        }
    }

    /// The paragraph embedding level for this direction: 0 or 1.
    pub fn level(self) -> u8 {
        match self {
            Self::LeftToRight => 0,
            Self::RightToLeft => 1,
        }
    }
}

/// A character's bidirectional class (Bidi_Class).
pub fn bidi_class(c: char) -> BidiClass {
    crate::bidi_tables::bidi_class(c)
}

/// The character whose glyph mirrors `c`'s (Bidi_Mirroring_Glyph), such as `)` for `(`, if
/// it has one. Right-to-left runs draw mirrored characters with it (rule L4).
pub fn mirror(c: char) -> Option<char> {
    let cp = c as u32;
    let i = MIRRORS.binary_search_by_key(&cp, |&(from, _)| from).ok()?;
    char::from_u32(MIRRORS[i].1)
}

/// One paragraph: its bytes, separator included, and its embedding level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    /// The paragraph's bytes in the text, with the paragraph separator that ends it.
    pub range: Range<usize>,
    /// The paragraph embedding level: 0 for left to right, 1 for right to left.
    pub level: u8,
}

impl Paragraph {
    /// The paragraph's direction.
    pub fn direction(&self) -> Direction {
        Direction::of_level(self.level)
    }
}

/// A run of one embedding level on a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualRun {
    /// The run's bytes in the text, in logical order.
    pub range: Range<usize>,
    /// The run's embedding level; odd levels display right to left.
    pub level: u8,
}

impl VisualRun {
    /// The run's direction.
    pub fn direction(&self) -> Direction {
        Direction::of_level(self.level)
    }
}

/// Text with its embedding levels resolved.
#[derive(Clone, Debug)]
pub struct Bidi<'a> {
    text: &'a str,
    paragraphs: Vec<Paragraph>,
    levels: Vec<u8>,
}

impl<'a> Bidi<'a> {
    /// Resolves `text`'s embedding levels. With `direction`, every paragraph takes it (as a
    /// higher-level protocol may, rule HL1); without, each paragraph takes the direction of its
    /// first strong character, left to right when it has none (rules P2 and P3).
    ///
    /// Paragraphs end after each paragraph separator (Bidi_Class B: line feed, carriage return,
    /// U+0085, U+2029 and others); a carriage return followed by a line feed ends one paragraph.
    pub fn new(text: &'a str, direction: Option<Direction>) -> Self {
        let mut paragraphs = Vec::new();
        let mut levels = vec![0; text.len()];
        let mut start = 0;
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if bidi_class(c) != BidiClass::B {
                continue;
            }
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n') {
                chars.next();
                end += 1;
            }
            paragraphs.push(resolve(text, start..end, direction, &mut levels));
            start = end;
        }
        if start < text.len() || text.is_empty() {
            paragraphs.push(resolve(text, start..text.len(), direction, &mut levels));
        }
        Self {
            text,
            paragraphs,
            levels,
        }
    }

    /// The text.
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// The paragraphs, in order. Empty text has one empty paragraph.
    pub fn paragraphs(&self) -> &[Paragraph] {
        &self.paragraphs
    }

    /// The resolved embedding level of each byte, before line breaking (rules X1 to I2).
    pub fn levels(&self) -> &[u8] {
        &self.levels
    }

    /// The embedding level of each byte of a line, with rule L1 applied: segment and paragraph
    /// separators, and the whitespace and isolate formatting characters before them or at the
    /// end of the line, take the paragraph level. `line` is clamped to the text and to
    /// character boundaries; a line that spans paragraphs is resolved per paragraph.
    pub fn line_levels(&self, line: Range<usize>) -> Vec<u8> {
        let line = self.clamp(line);
        let mut levels = self.levels[line.clone()].to_vec();
        let mut reset = Vec::new();
        for (paragraph, part) in self.parts(&line) {
            let text = &self.text[part.clone()];
            // Walks back from the end, so a trailing run of whitespace is found first.
            reset.clear();
            reset.resize(text.len(), false);
            let mut trailing = true;
            for (i, c) in text.char_indices().rev() {
                let class = bidi_class(c);
                reset[i] = match class {
                    BidiClass::S | BidiClass::B => {
                        trailing = true;
                        true
                    }
                    _ if is_trailing_whitespace(class) => trailing,
                    _ => {
                        trailing = false;
                        false
                    }
                };
                if reset[i] {
                    let at = part.start + i - line.start;
                    levels[at..at + c.len_utf8()].fill(paragraph.level);
                }
            }
            // The characters X9 sets aside stay with the character before them when L1 has
            // moved it (a tab before a soft hyphen, say), unless L1 reset them too.
            let mut before: Option<u8> = None;
            for (i, c) in text.char_indices() {
                let at = part.start + i - line.start;
                if let Some(level) = before
                    && !reset[i]
                    && is_removed(bidi_class(c))
                {
                    levels[at..at + c.len_utf8()].fill(level);
                }
                before = Some(levels[at]);
            }
        }
        levels
    }

    /// A line's runs of one embedding level, in the order they are displayed from left to
    /// right (rules L1 and L2). Each run's bytes are in logical order: a right-to-left run is
    /// displayed from its end to its start.
    pub fn visual_runs(&self, line: Range<usize>) -> Vec<VisualRun> {
        let line = self.clamp(line);
        let levels = self.line_levels(line.clone());
        let mut out = Vec::new();
        // Each paragraph is reordered on its own; a line that spans paragraphs shows them
        // in order.
        for (_, part) in self.parts(&line) {
            let mut runs: Vec<VisualRun> = Vec::new();
            for at in part {
                let level = levels[at - line.start];
                match runs.last_mut() {
                    Some(run) if run.level == level => run.range.end = at + 1,
                    _ => runs.push(VisualRun {
                        range: at..at + 1,
                        level,
                    }),
                }
            }
            let run_levels: Vec<u8> = runs.iter().map(|r| r.level).collect();
            let order = visual_order(&run_levels);
            let mut slots: Vec<Option<VisualRun>> = runs.into_iter().map(Some).collect();
            out.extend(order.into_iter().filter_map(|i| slots[i].take()));
        }
        out
    }

    fn clamp(&self, line: Range<usize>) -> Range<usize> {
        let end = crate::floor_boundary(self.text, line.end);
        crate::floor_boundary(self.text, line.start).min(end)..end
    }

    /// The paragraphs a clamped line overlaps, each with the line's bytes in it, found by
    /// binary search so that laying out many lines stays linear.
    fn parts<'s>(
        &'s self,
        line: &Range<usize>,
    ) -> impl Iterator<Item = (&'s Paragraph, Range<usize>)> + 's {
        let line = line.clone();
        let first = self
            .paragraphs
            .partition_point(|p| p.range.end <= line.start);
        self.paragraphs[first..]
            .iter()
            .take_while(move |p| p.range.start < line.end)
            .map(move |p| {
                let part = p.range.start.max(line.start)..p.range.end.min(line.end);
                (p, part)
            })
            .filter(|(_, part)| !part.is_empty())
    }
}

/// Whether a class belongs to the whitespace that rule L1 resets: whitespace, isolate
/// formatting characters, and the characters rule X9 sets aside, which go with them.
fn is_trailing_whitespace(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, Ws | Fsi | Lri | Rli | Pdi) || is_removed(class)
}

/// Whether rule X9 sets characters of this class aside: explicit embeddings and overrides,
/// their terminator, and boundary neutrals.
fn is_removed(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, Bn | Lre | Lro | Rle | Rlo | Pdf)
}

/// The display order of items with these embedding levels (rule L2): the index of the item
/// shown first, then the next, from left to right.
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    let Some(lowest_odd) = levels.iter().map(|&l| l | 1).min() else {
        return order;
    };
    let mut level = highest;
    while level >= lowest_odd {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] < level {
                i += 1;
                continue;
            }
            let start = i;
            while i < order.len() && levels[order[i]] >= level {
                i += 1;
            }
            order[start..i].reverse();
        }
        if level == 0 {
            break;
        }
        level -= 1;
    }
    order
}

/// Marks characters rule X9 removes until their levels are filled in.
const REMOVED: u8 = u8::MAX;

/// A directional status stack entry (rule X1).
#[derive(Clone, Copy)]
struct Status {
    level: u8,
    /// The class characters in this scope are overridden to, if any.
    overriding: Option<BidiClass>,
    isolate: bool,
}

/// Resolves one paragraph's levels into `levels` and returns the paragraph.
fn resolve(
    text: &str,
    range: Range<usize>,
    direction: Option<Direction>,
    levels: &mut [u8],
) -> Paragraph {
    let paragraph_text = &text[range.clone()];
    let chars: Vec<(usize, char)> = paragraph_text.char_indices().collect();
    let original: Vec<BidiClass> = chars.iter().map(|&(_, c)| bidi_class(c)).collect();
    let matching = matching_pdis(&original);
    let level = direction.map_or_else(
        || first_strong(&original, &matching, 0..original.len()).unwrap_or(0),
        Direction::level,
    );
    let (mut char_levels, mut types) = explicit_levels(&original, &matching, level);
    for sequence in isolating_run_sequences(&original, &matching, &char_levels, level) {
        resolve_sequence(&sequence, &chars, &mut types, &mut char_levels);
    }
    // Characters X9 removed take the level of the character before them.
    let mut previous = level;
    for (k, &(offset, c)) in chars.iter().enumerate() {
        let char_level = match char_levels[k] {
            REMOVED => previous,
            l => l,
        };
        previous = char_level;
        let at = range.start + offset;
        levels[at..at + c.len_utf8()].fill(char_level);
    }
    Paragraph { range, level }
}

/// Each isolate initiator's matching PDI (BD9), or None when the paragraph ends first.
fn matching_pdis(classes: &[BidiClass]) -> Vec<Option<usize>> {
    let mut matching = vec![None; classes.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, &class) in classes.iter().enumerate() {
        match class {
            BidiClass::Lri | BidiClass::Rli | BidiClass::Fsi => open.push(i),
            BidiClass::Pdi => {
                if let Some(initiator) = open.pop() {
                    matching[initiator] = Some(i);
                }
            }
            _ => {}
        }
    }
    matching
}

/// The level the first strong character in `range` gives (rules P2 and P3), skipping
/// isolates; None without one.
fn first_strong(
    classes: &[BidiClass],
    matching: &[Option<usize>],
    range: Range<usize>,
) -> Option<u8> {
    let mut i = range.start;
    while i < range.end {
        match classes[i] {
            BidiClass::L => return Some(0),
            BidiClass::R | BidiClass::Al => return Some(1),
            BidiClass::Lri | BidiClass::Rli | BidiClass::Fsi => i = matching[i]?,
            _ => {}
        }
        i += 1;
    }
    None
}

/// The least odd (`odd`) or even level greater than `level`.
fn next_level(level: u8, odd: bool) -> u8 {
    if odd {
        (level + 1) | 1
    } else {
        (level + 2) & !1
    }
}

/// Rules X1 to X9: each character's explicit level (REMOVED for those X9 removes) and its
/// class after overrides.
fn explicit_levels(
    classes: &[BidiClass],
    matching: &[Option<usize>],
    paragraph: u8,
) -> (Vec<u8>, Vec<BidiClass>) {
    use BidiClass::*;
    let mut levels = vec![paragraph; classes.len()];
    let mut types = classes.to_vec();
    let mut stack = vec![Status {
        level: paragraph,
        overriding: None,
        isolate: false,
    }];
    let (mut overflow_isolates, mut overflow_embeddings, mut valid_isolates) =
        (0usize, 0usize, 0usize);
    for (i, &class) in classes.iter().enumerate() {
        let top = *stack.last().unwrap_or(&Status {
            level: paragraph,
            overriding: None,
            isolate: false,
        });
        match class {
            Rle | Lre | Rlo | Lro => {
                let level = next_level(top.level, matches!(class, Rle | Rlo));
                if level <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    stack.push(Status {
                        level,
                        overriding: match class {
                            Rlo => Some(R),
                            Lro => Some(L),
                            _ => None,
                        },
                        isolate: false,
                    });
                } else if overflow_isolates == 0 {
                    overflow_embeddings += 1;
                }
                levels[i] = REMOVED;
            }
            Rli | Lri | Fsi => {
                levels[i] = top.level;
                if let Some(o) = top.overriding {
                    types[i] = o;
                }
                let rtl = match class {
                    Rli => true,
                    Lri => false,
                    _ => {
                        let end = matching[i].unwrap_or(classes.len());
                        first_strong(classes, matching, i + 1..end) == Some(1)
                    }
                };
                let level = next_level(top.level, rtl);
                if level <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    valid_isolates += 1;
                    stack.push(Status {
                        level,
                        overriding: None,
                        isolate: true,
                    });
                } else {
                    overflow_isolates += 1;
                }
            }
            Pdi => {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates > 0 {
                    overflow_embeddings = 0;
                    while stack.last().is_some_and(|s| !s.isolate) {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolates -= 1;
                }
                let top = *stack.last().unwrap_or(&top);
                levels[i] = top.level;
                if let Some(o) = top.overriding {
                    types[i] = o;
                }
            }
            Pdf => {
                if overflow_isolates > 0 {
                } else if overflow_embeddings > 0 {
                    overflow_embeddings -= 1;
                } else if !top.isolate && stack.len() >= 2 {
                    stack.pop();
                }
                levels[i] = REMOVED;
            }
            B => levels[i] = paragraph,
            Bn => levels[i] = REMOVED,
            _ => {
                levels[i] = top.level;
                if let Some(o) = top.overriding {
                    types[i] = o;
                }
            }
        }
    }
    (levels, types)
}

/// One isolating run sequence (BD13): its characters in order, and its sos and eos.
struct Sequence {
    chars: Vec<usize>,
    level: u8,
    sos: BidiClass,
    eos: BidiClass,
}

fn is_isolate_initiator(class: BidiClass) -> bool {
    matches!(class, BidiClass::Lri | BidiClass::Rli | BidiClass::Fsi)
}

/// Rule X10: the isolating run sequences of a paragraph, with their sos and eos.
fn isolating_run_sequences(
    original: &[BidiClass],
    matching: &[Option<usize>],
    levels: &[u8],
    paragraph: u8,
) -> Vec<Sequence> {
    // Level runs over the characters X9 keeps.
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut run_of = vec![usize::MAX; levels.len()];
    let mut previous: Option<u8> = None;
    for (i, &level) in levels.iter().enumerate() {
        if level == REMOVED {
            continue;
        }
        if previous != Some(level) {
            runs.push(Vec::new());
        }
        if let Some(run) = runs.last_mut() {
            run.push(i);
        }
        run_of[i] = runs.len() - 1;
        previous = Some(level);
    }
    let mut initiator_of = vec![false; levels.len()];
    for pdi in matching.iter().flatten() {
        initiator_of[*pdi] = true;
    }
    let mut sequences = Vec::new();
    for run in &runs {
        let first = run[0];
        if original[first] == BidiClass::Pdi && initiator_of[first] {
            continue;
        }
        let mut chars = run.clone();
        loop {
            let last = chars[chars.len() - 1];
            let Some(pdi) = matching[last].filter(|_| is_isolate_initiator(original[last])) else {
                break;
            };
            let next = run_of[pdi];
            if next == usize::MAX || runs[next][0] != pdi {
                break;
            }
            chars.extend_from_slice(&runs[next]);
        }
        let first = chars[0];
        let last = chars[chars.len() - 1];
        let level = levels[first];
        let before = levels[..first]
            .iter()
            .rev()
            .find(|&&l| l != REMOVED)
            .copied()
            .unwrap_or(paragraph);
        let after = if is_isolate_initiator(original[last]) {
            paragraph
        } else {
            levels[last + 1..]
                .iter()
                .find(|&&l| l != REMOVED)
                .copied()
                .unwrap_or(paragraph)
        };
        let side = |other: u8| {
            if level.max(other) % 2 == 1 {
                BidiClass::R
            } else {
                BidiClass::L
            }
        };
        sequences.push(Sequence {
            chars,
            level,
            sos: side(before),
            eos: side(after),
        });
    }
    sequences
}

/// The strong direction a resolved class gives neutrals: numbers count as R (rules N0, N1).
fn strong(class: BidiClass) -> Option<BidiClass> {
    match class {
        BidiClass::L => Some(BidiClass::L),
        BidiClass::R | BidiClass::En | BidiClass::An => Some(BidiClass::R),
        _ => None,
    }
}

/// Neutral and isolate formatting characters (NI).
fn is_neutral(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, B | S | Ws | On | Fsi | Lri | Rli | Pdi)
}

/// Rules W1 to I2 on one isolating run sequence.
fn resolve_sequence(
    sequence: &Sequence,
    chars: &[(usize, char)],
    types: &mut [BidiClass],
    levels: &mut [u8],
) {
    use BidiClass::*;
    let seq = &sequence.chars;
    let n = seq.len();
    // Characters that were NSM before W1, for N0.
    let was_nsm: Vec<bool> = seq.iter().map(|&i| types[i] == Nsm).collect();

    // W1.
    let mut previous = sequence.sos;
    for &i in seq {
        if types[i] == Nsm {
            types[i] = previous;
        } else {
            previous = if matches!(types[i], Lri | Rli | Fsi | Pdi) {
                On
            } else {
                types[i]
            };
        }
    }
    // W2 and W3.
    let mut last_strong = sequence.sos;
    for &i in seq {
        match types[i] {
            L | R | Al => last_strong = types[i],
            En if last_strong == Al => types[i] = An,
            _ => {}
        }
    }
    for &i in seq {
        if types[i] == Al {
            types[i] = R;
        }
    }
    // W4.
    for k in 1..n.saturating_sub(1) {
        let (before, here, after) = (types[seq[k - 1]], types[seq[k]], types[seq[k + 1]]);
        types[seq[k]] = match (before, here, after) {
            (En, Es | Cs, En) => En,
            (An, Cs, An) => An,
            _ => here,
        };
    }
    // W5.
    let mut k = 0;
    while k < n {
        if types[seq[k]] != Et {
            k += 1;
            continue;
        }
        let start = k;
        while k < n && types[seq[k]] == Et {
            k += 1;
        }
        let next_to_number =
            (start > 0 && types[seq[start - 1]] == En) || (k < n && types[seq[k]] == En);
        if next_to_number {
            for &i in &seq[start..k] {
                types[i] = En;
            }
        }
    }
    // W6.
    for &i in seq {
        if matches!(types[i], Es | Et | Cs) {
            types[i] = On;
        }
    }
    // W7.
    let mut last_strong = sequence.sos;
    for &i in seq {
        match types[i] {
            L | R => last_strong = types[i],
            En if last_strong == L => types[i] = L,
            _ => {}
        }
    }

    let embedding = if sequence.level % 2 == 1 { R } else { L };
    // N0.
    for (open, close) in bracket_pairs(seq, chars, types) {
        let mut found_embedding = false;
        let mut found_opposite = false;
        for &i in &seq[open + 1..close] {
            match strong(types[i]) {
                Some(d) if d == embedding => found_embedding = true,
                Some(_) => found_opposite = true,
                None => {}
            }
        }
        let direction = if found_embedding {
            embedding
        } else if found_opposite {
            let context = seq[..open]
                .iter()
                .rev()
                .find_map(|&i| strong(types[i]))
                .unwrap_or(sequence.sos);
            if context != embedding {
                context
            } else {
                embedding
            }
        } else {
            continue;
        };
        for bracket in [open, close] {
            types[seq[bracket]] = direction;
            for k in bracket + 1..n {
                if !was_nsm[k] {
                    break;
                }
                types[seq[k]] = direction;
            }
        }
    }
    // N1 and N2.
    let mut k = 0;
    while k < n {
        if !is_neutral(types[seq[k]]) {
            k += 1;
            continue;
        }
        let start = k;
        while k < n && is_neutral(types[seq[k]]) {
            k += 1;
        }
        let before = if start == 0 {
            sequence.sos
        } else {
            strong(types[seq[start - 1]]).unwrap_or(embedding)
        };
        let after = if k == n {
            sequence.eos
        } else {
            strong(types[seq[k]]).unwrap_or(embedding)
        };
        let direction = if before == after { before } else { embedding };
        for &i in &seq[start..k] {
            types[i] = direction;
        }
    }
    // I1 and I2.
    for &i in seq {
        let level = levels[i];
        levels[i] = match (level.is_multiple_of(2), types[i]) {
            (true, R) => level + 1,
            (true, An | En) => level + 2,
            (false, L | En | An) => level + 1,
            _ => level,
        };
    }
}

/// A bracket's pair under canonical equivalence: U+2329 and U+232A are U+3008 and U+3009.
fn canonical_bracket(cp: u32) -> u32 {
    match cp {
        0x2329 => 0x3008,
        0x232A => 0x3009,
        other => other,
    }
}

/// The bracket pairs of a sequence (BD16), as positions in it, ordered by opening bracket.
fn bracket_pairs(
    seq: &[usize],
    chars: &[(usize, char)],
    types: &[BidiClass],
) -> Vec<(usize, usize)> {
    const STACK: usize = 63;
    let mut stack: Vec<(u32, usize)> = Vec::with_capacity(STACK);
    let mut pairs = Vec::new();
    for (k, &i) in seq.iter().enumerate() {
        if types[i] != BidiClass::On {
            continue;
        }
        let cp = chars[i].1 as u32;
        let Ok(found) = BRACKETS.binary_search_by_key(&cp, |&(c, _)| c) else {
            continue;
        };
        let pair = BRACKETS[found].1;
        if pair & 0x8000_0000 != 0 {
            if stack.len() == STACK {
                return Vec::new();
            }
            stack.push((canonical_bracket(pair & 0x7FFF_FFFF), k));
        } else if let Some(depth) = stack
            .iter()
            .rposition(|&(closing, _)| closing == canonical_bracket(cp))
        {
            pairs.push((stack[depth].1, k));
            stack.truncate(depth);
        }
    }
    // Found in order of their closing brackets; put in order of the opening ones without
    // a sort, which would make many pairs cost more than linear time.
    if pairs.len() > 1 {
        let mut closing = vec![usize::MAX; seq.len()];
        for &(open, close) in &pairs {
            closing[open] = close;
        }
        pairs.clear();
        pairs.extend(
            (0..seq.len())
                .filter(|&k| closing[k] != usize::MAX)
                .map(|k| (k, closing[k])),
        );
    }
    pairs
}
