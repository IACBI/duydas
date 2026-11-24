//! Mapping characters to glyphs, with canonical decomposition and composition chosen by
//! what the font covers, as HarfBuzz does: a character the font lacks is decomposed when the
//! font has its parts, marks are put in canonical order, and a base and mark the font has a
//! precomposed glyph for are composed again.

use crate::bidi;
use crate::font::Font;
use crate::font::layout::{LJMO, TJMO, VJMO};
use crate::props::props;
use crate::shaping_tables::{COMPOSITIONS, DECOMPOSITIONS, MARK, char_info};

/// A character and the glyph it maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Char {
    pub(super) c: char,
    pub(super) glyph: u16,
    /// Byte offset of the start of the character's grapheme cluster.
    pub(super) cluster: usize,
    /// How to size a space drawn with the font's U+0020 glyph; see [`space_kind`].
    pub(super) space: u8,
    /// A variation selector whose sequence the font lacks: from here on it is a format
    /// character, not a mark, as in HarfBuzz.
    pub(super) unmatched_selector: bool,
    /// The Hangul jamo feature for this character alone (see [`preprocess_hangul`]).
    pub(super) jamo: u8,
    /// A COMBINING GRAPHEME JOINER that keeps no marks apart, so lookups may pass over it.
    /// As in HarfBuzz, this is decided once marks are reordered and before they compose
    /// again.
    pub(super) passable_joiner: bool,
    /// Comes from a character of right-to-left text replaced by its mirror image (rule L4),
    /// so the `rtlm` feature does not apply to it.
    pub(super) mirrored: bool,
}

/// A character of the text on its way to [`Char`]: the grapheme cluster it belongs to and
/// what it carries from preprocessing.
#[derive(Clone, Copy, Debug)]
struct Source {
    c: char,
    cluster: usize,
    /// The Hangul jamo feature (see [`preprocess_hangul`]).
    jamo: u8,
    mirrored: bool,
}

impl Source {
    fn new(c: char, cluster: usize) -> Self {
        Self {
            c,
            cluster,
            jamo: 0,
            mirrored: false,
        }
    }
}

/// The HarfBuzz shaper whose normalization a run gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shaper {
    Default,
    /// Jamo composition and nothing else (see [`preprocess_hangul`]).
    Hangul,
    /// Hebrew presentation forms when nothing else composes and the font has no GPOS mark
    /// positioning (`gpos_mark`), and the order of meteg among the points that fonts expect.
    Hebrew {
        gpos_mark: bool,
    },
}

/// More combining marks in a row than this are left in their order, as in HarfBuzz.
const MAX_COMBINING_MARKS: usize = 32;

pub(super) fn combining_class(c: char) -> u8 {
    ((char_info(c) >> 8) & 0xFF) as u8
}

pub(super) fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{180B}'..='\u{180D}' | '\u{180F}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// A space with no glyph of its own is drawn with U+0020's glyph at the width it should
/// have, as HarfBuzz does: 1 to 6 and 16 are fractions of the em, 18 is 4/18 em, and the
/// others are named below. [`SPACE`] keeps the space glyph's own width.
pub(super) const SPACE: u8 = 20;
pub(super) const SPACE_FIGURE: u8 = 7;
pub(super) const SPACE_PUNCTUATION: u8 = 8;
pub(super) const SPACE_NARROW: u8 = 9;
pub(super) const SPACE_4_EM_18: u8 = 18;

pub(super) fn space_kind(c: char) -> u8 {
    match c {
        '\u{0020}' | '\u{00A0}' => SPACE,
        '\u{2001}' | '\u{2003}' | '\u{3000}' => 1,
        '\u{2000}' | '\u{2002}' => 2,
        '\u{2004}' => 3,
        '\u{2005}' => 4,
        '\u{2009}' => 5,
        '\u{2006}' => 6,
        '\u{200A}' => 16,
        '\u{2007}' => SPACE_FIGURE,
        '\u{2008}' => SPACE_PUNCTUATION,
        '\u{202F}' => SPACE_NARROW,
        '\u{205F}' => SPACE_4_EM_18,
        _ => 0,
    }
}

/// The canonical decomposition of `c` into one or two characters; Hangul syllables
/// decompose algorithmically.
pub(super) fn decomposition(c: char) -> Option<(char, Option<char>)> {
    let s = u32::from(c);
    if let Some(index) = s.checked_sub(S_BASE).filter(|&i| i < S_COUNT) {
        let t = index % T_COUNT;
        return if t == 0 {
            let l = char::from_u32(L_BASE + index / N_COUNT)?;
            Some((l, char::from_u32(V_BASE + index % N_COUNT / T_COUNT)))
        } else {
            Some((char::from_u32(s - t)?, char::from_u32(T_BASE + t)))
        };
    }
    let i = DECOMPOSITIONS
        .binary_search_by_key(&s, |&(cp, _, _)| cp)
        .ok()?;
    let (_, a, b) = DECOMPOSITIONS[i];
    Some((
        char::from_u32(a)?,
        (b != 0).then(|| char::from_u32(b)).flatten(),
    ))
}

/// The primary composite of `a` and `b`.
pub(super) fn composition(a: char, b: char) -> Option<char> {
    let key = (u32::from(a), u32::from(b));
    let i = COMPOSITIONS
        .binary_search_by(|&(x, y, _)| (x, y).cmp(&key))
        .ok()?;
    char::from_u32(COMPOSITIONS[i].2)
}

/// Maps `text` to characters and glyphs. Each character's cluster is the start of its
/// grapheme cluster.
///
/// Hangul text is handled as HarfBuzz's Hangul shaper does (see [`preprocess_hangul`]):
/// jamo compose into a syllable the font has, even when the font has the jamo too. Other
/// characters the font has are not decomposed or composed.
///
/// In right-to-left text (`mirror`), a character with a mirror image the font has is
/// replaced by it first, as HarfBuzz does. As in HarfBuzz, the characters of one of its
/// grapheme clusters (see [`continuations`]) take the cluster of the first. With `reverse`,
/// those clusters are taken in reverse order (their characters keep theirs), as HarfBuzz
/// does to shape text in its script's direction.
pub(super) fn normalize(
    font: &Font,
    text: &str,
    shaper: Shaper,
    mirror: bool,
    reverse: bool,
) -> Vec<Char> {
    let hangul = shaper == Shaper::Hangul;
    // Bytes bound the characters; the cap keeps a huge run from reserving more than it needs
    // (four-byte characters) or more than a 32-bit target can address up front.
    let mut chars: Vec<Source> = Vec::with_capacity(text.len().min(1 << 16));
    let mut start = 0;
    for grapheme in crate::graphemes(text) {
        chars.extend(grapheme.chars().map(|c| {
            match bidi::mirror(c).filter(|&m| mirror && font.glyph_index(m).is_some()) {
                Some(m) => Source {
                    mirrored: true,
                    ..Source::new(m, start)
                },
                None => Source::new(c, start),
            }
        }));
        start += grapheme.len();
    }
    let continues = continuations(&chars);
    for i in 1..chars.len() {
        if continues[i] {
            chars[i].cluster = chars[i - 1].cluster;
        }
    }
    if reverse {
        chars = reverse_graphemes(&chars, &continues);
    }
    if hangul {
        chars = preprocess_hangul(font, &chars);
    }
    let mut out = Vec::with_capacity(chars.len());
    let mut rest = &chars[..];
    while let Some(cluster) = rest.first().map(|s| s.cluster) {
        // As HarfBuzz, a character and the marks right after it are decomposed together,
        // whatever the grapheme clusters: a base before a joiner is on its own.
        let len = 1 + rest[1..]
            .iter()
            .take_while(|s| char_info(s.c) & MARK != 0)
            .count();
        let (group, tail) = rest.split_at(len);
        rest = tail;
        let mut mapper = Mapper {
            font,
            cluster,
            jamo: 0,
            mirrored: false,
            out: &mut out,
        };
        if !group.iter().any(|s| is_variation_selector(s.c)) {
            mapper.cluster(group, hangul);
            continue;
        }
        // HarfBuzz maps a base and the marks after it as one sequence when a variation
        // selector is among them; the rest of the grapheme (after a joiner, say) as usual.
        let mut part = group;
        while !part.is_empty() {
            let len = 1 + part[1..]
                .iter()
                .take_while(|s| char_info(s.c) & MARK != 0)
                .count();
            let (marks, rest) = part.split_at(len);
            part = rest;
            if marks.iter().any(|s| is_variation_selector(s.c)) {
                mapper.selector_cluster(marks);
            } else {
                mapper.cluster(marks, hangul);
            }
        }
    }
    reorder(&mut out, shaper);
    for i in 1..out.len().saturating_sub(1) {
        let (prev, next) = (combining_class(out[i - 1].c), combining_class(out[i + 1].c));
        if out[i].c == '\u{34F}' && (next == 0 || prev <= next) {
            out[i].passable_joiner = true;
        }
    }
    if !hangul {
        recompose(font, &mut out, shaper);
    }
    out
}

/// Whether each character continues HarfBuzz's grapheme cluster before it. They are not
/// Unicode's: a character continues the one before only if it is a mark, ZWJ (or
/// Extended_Pictographic after ZWJ), an emoji modifier, the second regional indicator of a
/// pair, a halfwidth katakana sound mark or a tag. A ZWNJ or a Hangul vowel jamo starts one
/// of its own; a mark after a control character, or an emoji after a ZWJ, does not.
fn continuations(chars: &[Source]) -> Vec<bool> {
    let regional = |c: char| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c);
    let mut continues: Vec<bool> = Vec::with_capacity(chars.len());
    for (i, s) in chars.iter().enumerate() {
        let c = s.c;
        let after = |k: usize| (chars[k].c, continues[k]);
        let continuation = char_info(c) & MARK != 0
            || matches!(
                c,
                '\u{200D}'
                    | '\u{1F3FB}'..='\u{1F3FF}'
                    | '\u{FF9E}'
                    | '\u{FF9F}'
                    | '\u{E0020}'..='\u{E007F}'
            )
            || i.checked_sub(1).map(after).is_some_and(|(p, p_continues)| {
                (regional(c) && regional(p) && !p_continues)
                    || (p == '\u{200D}' && props(c).extended_pictographic())
            });
        continues.push(continuation);
    }
    continues
}

/// Reverses the order of HarfBuzz's grapheme clusters (`continues`, from [`continuations`]),
/// each keeping its characters in order.
fn reverse_graphemes(chars: &[Source], continues: &[bool]) -> Vec<Source> {
    let mut out = Vec::with_capacity(chars.len());
    let mut end = chars.len();
    for start in (1..chars.len()).rev() {
        if !continues[start] {
            out.extend_from_slice(&chars[start..end]);
            end = start;
        }
    }
    out.extend_from_slice(&chars[..end]);
    out
}

const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const S_BASE: u32 = 0xAC00;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = 21 * T_COUNT;
const S_COUNT: u32 = 19 * N_COUNT;

fn is_l(u: u32) -> bool {
    (0x1100..=0x115F).contains(&u) || (0xA960..=0xA97C).contains(&u)
}

fn is_v(u: u32) -> bool {
    (0x1160..=0x11A7).contains(&u) || (0xD7B0..=0xD7C6).contains(&u)
}

fn is_t(u: u32) -> bool {
    (0x11A8..=0x11FF).contains(&u) || (0xD7CB..=0xD7FB).contains(&u)
}

/// A trailing jamo that composes with an LV syllable.
fn is_combining_t(u: u32) -> bool {
    (T_BASE + 1..T_BASE + T_COUNT).contains(&u)
}

/// Prepares Hangul text as HarfBuzz's Hangul shaper does. A syllable, <L, V>, <L, V, T>,
/// <LV>, <LVT> or <LV, T>, is composed when the font has the whole of it, and otherwise
/// decomposed into jamo the font has, which then take the `ljmo`, `vjmo` and `tjmo`
/// features. A tone mark after a syllable moves in front of it, unless its glyph has no
/// width; one with no syllable before it gets a dotted circle, when the font has one.
fn preprocess_hangul(font: &Font, chars: &[Source]) -> Vec<Source> {
    let has = |u: u32| char::from_u32(u).filter(|&c| font.glyph_index(c).is_some());
    let code = |i: usize| chars.get(i).map(|s| u32::from(s.c));
    let jamo = [LJMO, VJMO, TJMO];
    let mut out: Vec<Source> = Vec::with_capacity(chars.len());
    // The last syllable put out, `out[start..end]`, when `start < end`.
    let (mut start, mut end) = (0, 0);
    let mut i = 0;
    while i < chars.len() {
        let source = chars[i];
        let (c, cluster) = (source.c, source.cluster);
        let u = u32::from(c);
        if (0x302E..=0x302F).contains(&u) {
            let zero_width = font
                .glyph_index(c)
                .is_some_and(|g| font.advance(g) == Some(0));
            if start < end && end == out.len() {
                if zero_width {
                    out.push(source);
                } else {
                    out.insert(start, source);
                }
            } else if let Some(circle) = has(0x25CC) {
                let circle = Source::new(circle, cluster);
                out.extend(if zero_width {
                    [circle, source]
                } else {
                    [source, circle]
                });
            } else {
                out.push(source);
            }
            i += 1;
            (start, end) = (out.len(), out.len());
            continue;
        }
        start = out.len();
        if is_l(u) {
            if let Some(v) = code(i + 1).filter(|&v| is_v(v)) {
                let t = code(i + 2).filter(|&t| is_t(t));
                let len = if t.is_some() { 3 } else { 2 };
                if (L_BASE..L_BASE + 19).contains(&u)
                    && (V_BASE..V_BASE + 21).contains(&v)
                    && t.is_none_or(is_combining_t)
                    && let Some(s) = has(S_BASE
                        + (u - L_BASE) * N_COUNT
                        + (v - V_BASE) * T_COUNT
                        + t.map_or(0, |t| t - T_BASE))
                {
                    out.push(Source::new(s, cluster));
                    end = start + 1;
                } else {
                    for k in 0..len {
                        out.push(Source {
                            jamo: jamo[k],
                            ..chars[i + k]
                        });
                    }
                    end = start + len;
                }
                i += len;
                continue;
            }
        } else if (S_BASE..S_BASE + S_COUNT).contains(&u) {
            let has_glyph = font.glyph_index(c).is_some();
            let (lv, t) = ((u - S_BASE) / T_COUNT, (u - S_BASE) % T_COUNT);
            let next = code(i + 1);
            if t == 0
                && let Some(s) = next
                    .filter(|&n| is_combining_t(n))
                    .and_then(|n| has(u + n - T_BASE))
            {
                out.push(Source::new(s, cluster));
                end = start + 1;
                i += 2;
                continue;
            }
            let t_follows = t == 0 && next.is_some_and(is_t);
            if !has_glyph || t_follows {
                let parts = [L_BASE + lv / 21, V_BASE + lv % 21, T_BASE + t];
                let len = if t == 0 { 2 } else { 3 };
                let found: Option<Vec<char>> = parts[..len].iter().map(|&p| has(p)).collect();
                if let Some(found) = found {
                    out.extend(found.iter().zip(jamo).map(|(&c, jamo)| Source {
                        jamo,
                        ..Source::new(c, cluster)
                    }));
                    i += 1;
                    end = start + len;
                    // An LV decomposed for the trailing jamo after it takes that jamo in.
                    if has_glyph {
                        out.push(Source {
                            jamo: TJMO,
                            ..chars[i]
                        });
                        i += 1;
                        end += 1;
                    }
                    continue;
                }
            }
            if has_glyph {
                end = start + 1;
            }
        }
        out.push(source);
        i += 1;
    }
    out
}

struct Mapper<'a> {
    font: &'a Font,
    cluster: usize,
    /// The jamo feature of the character being mapped.
    jamo: u8,
    /// Whether the character being mapped is a mirror image.
    mirrored: bool,
    out: &'a mut Vec<Char>,
}

impl Mapper<'_> {
    fn glyph(&self, c: char) -> Option<u16> {
        self.font.glyph_index(c).map(|g| g.0)
    }

    /// Maps a character and the marks after it. With marks they are decomposed as far as the
    /// font allows, so the marks can be reordered and composed again; a character alone (a
    /// mark too, at the start of the text) keeps what the font has, as in HarfBuzz.
    fn cluster(&mut self, group: &[Source], hangul: bool) {
        let shortest = hangul || group.len() == 1;
        for s in group {
            self.cluster = s.cluster;
            self.jamo = s.jamo;
            self.mirrored = s.mirrored;
            self.current(s.c, shortest);
        }
    }

    /// A cluster with a variation selector is not normalized: a base whose variant the font
    /// lists takes that glyph and drops the selector; otherwise both keep their own glyphs.
    fn selector_cluster(&mut self, group: &[Source]) {
        let nominal = |m: &Self, c: char| m.glyph(c).unwrap_or(0);
        let mut i = 0;
        while i < group.len() {
            let Source {
                c,
                jamo,
                mirrored,
                cluster,
            } = group[i];
            self.cluster = cluster;
            self.jamo = jamo;
            self.mirrored = mirrored;
            i += 1;
            let Some(selector) = group
                .get(i)
                .map(|s| s.c)
                .filter(|&s| is_variation_selector(s))
            else {
                let glyph = nominal(self, c);
                self.push(c, glyph, 0);
                continue;
            };
            let selector_cluster = group[i].cluster;
            i += 1;
            match self.font.glyph_variant(c, selector) {
                Some(glyph) => {
                    // HarfBuzz replaces the pair, merging their clusters.
                    self.cluster = cluster.min(selector_cluster);
                    self.push(c, glyph.0, 0);
                }
                None => {
                    let glyph = nominal(self, c);
                    self.push(c, glyph, 0);
                    self.jamo = 0;
                    self.cluster = selector_cluster;
                    let glyph = nominal(self, selector);
                    self.push(selector, glyph, 0);
                    if let Some(last) = self.out.last_mut() {
                        last.unmatched_selector = true;
                    }
                }
            }
            self.jamo = 0;
            self.mirrored = false;
            while let Some(&Source {
                c: more, cluster, ..
            }) = group.get(i).filter(|s| is_variation_selector(s.c))
            {
                self.cluster = cluster;
                let glyph = nominal(self, more);
                self.push(more, glyph, 0);
                i += 1;
            }
        }
    }

    fn push(&mut self, c: char, glyph: u16, space: u8) {
        self.out.push(Char {
            c,
            glyph,
            cluster: self.cluster,
            space,
            unmatched_selector: false,
            jamo: self.jamo,
            passable_joiner: false,
            mirrored: self.mirrored,
        });
    }

    fn current(&mut self, c: char, shortest: bool) {
        if shortest && let Some(glyph) = self.glyph(c) {
            return self.push(c, glyph, 0);
        }
        if self.decompose(c, shortest) {
            return;
        }
        if let Some(glyph) = self.glyph(c) {
            return self.push(c, glyph, 0);
        }
        let kind = space_kind(c);
        if kind != 0
            && let Some(glyph) = self.glyph(' ')
        {
            return self.push(c, glyph, kind);
        }
        // The one no-break character that is not a space and has a breaking twin.
        if c == '\u{2011}'
            && let Some(glyph) = self.glyph('\u{2010}')
        {
            return self.push(c, glyph, 0);
        }
        self.push(c, 0, 0);
    }

    /// Pushes `c`'s decomposition if the font covers it; pushes nothing otherwise.
    fn decompose(&mut self, c: char, shortest: bool) -> bool {
        let Some((a, b)) = decomposition(c) else {
            return false;
        };
        let b = match b {
            Some(b) => match self.glyph(b) {
                Some(glyph) => Some((b, glyph)),
                None => return false,
            },
            None => None,
        };
        let a_glyph = self.glyph(a);
        let done = if shortest && let Some(glyph) = a_glyph {
            self.push(a, glyph, 0);
            true
        } else if self.decompose(a, shortest) {
            true
        } else if let Some(glyph) = a_glyph {
            self.push(a, glyph, 0);
            true
        } else {
            false
        };
        if done && let Some((b, glyph)) = b {
            self.push(b, glyph, 0);
        }
        done
    }
}

/// Sorts each run of combining marks by combining class, keeping equal classes in order.
fn reorder(chars: &mut [Char], shaper: Shaper) {
    let mut i = 0;
    while i < chars.len() {
        if combining_class(chars[i].c) == 0 {
            i += 1;
            continue;
        }
        let end = chars[i..]
            .iter()
            .position(|ch| combining_class(ch.c) == 0)
            .map_or(chars.len(), |n| i + n);
        if end - i <= MAX_COMBINING_MARKS {
            // HarfBuzz's insertion sort, which merges the clusters of the marks it moves past.
            for k in i + 1..end {
                let class = combining_class(chars[k].c);
                let mut j = k;
                while j > i && combining_class(chars[j - 1].c) > class {
                    j -= 1;
                }
                if j < k {
                    merge_clusters(chars, j, k + 1);
                    chars[j..=k].rotate_right(1);
                }
            }
            if let Shaper::Hebrew { .. } = shaper {
                reorder_hebrew(&mut chars[i..end]);
            }
        }
        i = end;
    }
}

/// HarfBuzz's `merge_clusters`: the characters from `start` to `end` take the smallest of
/// their clusters, and so do the characters next to them that share the cluster of the
/// first or last. Moving marks across a cluster boundary (a mark that starts a grapheme
/// cluster, as at the start of the text or after a control character) merges the clusters.
fn merge_clusters(chars: &mut [Char], mut start: usize, mut end: usize) {
    let Some(cluster) = chars[start..end].iter().map(|ch| ch.cluster).min() else {
        return;
    };
    if cluster != chars[end - 1].cluster {
        while end < chars.len() && chars[end - 1].cluster == chars[end].cluster {
            end += 1;
        }
    }
    if cluster != chars[start].cluster {
        while start > 0 && chars[start - 1].cluster == chars[start].cluster {
            start -= 1;
        }
    }
    for ch in &mut chars[start..end] {
        ch.cluster = cluster;
    }
}

/// HarfBuzz's Hebrew mark order: in patah or qamats, then sheva or hiriq, then meteg or a
/// mark below, the last two swap, the order fonts expect. Only the first such triple in a
/// run of marks moves. The classes are HarfBuzz's (see `char_info`).
fn reorder_hebrew(marks: &mut [Char]) {
    const PATAH: u8 = 20;
    const QAMATS: u8 = 21;
    const SHEVA: u8 = 22;
    const HIRIQ: u8 = 23;
    const METEG: u8 = 25;
    const BELOW: u8 = 220;
    for i in 2..marks.len() {
        let class = |k: usize| combining_class(marks[k].c);
        if matches!(class(i - 2), PATAH | QAMATS)
            && matches!(class(i - 1), SHEVA | HIRIQ)
            && matches!(class(i), METEG | BELOW)
        {
            // Both are in the cluster of the base before them, so their clusters agree.
            marks.swap(i - 1, i);
            break;
        }
    }
}

/// The Hebrew presentation form of a letter and a point, which HarfBuzz composes for old
/// fonts that have the forms but do not position marks: these compositions are excluded
/// from Unicode normalization.
fn hebrew_presentation_form(a: char, b: char) -> Option<char> {
    // The letters alef to tav with dagesh; 0 where none is encoded.
    const DAGESH: [u32; 27] = [
        0xFB30, 0xFB31, 0xFB32, 0xFB33, 0xFB34, 0xFB35, 0xFB36, 0, 0xFB38, 0xFB39, 0xFB3A, 0xFB3B,
        0xFB3C, 0, 0xFB3E, 0, 0xFB40, 0xFB41, 0, 0xFB43, 0xFB44, 0, 0xFB46, 0xFB47, 0xFB48, 0xFB49,
        0xFB4A,
    ];
    let form = match (u32::from(b), u32::from(a)) {
        (0x05B4, 0x05D9) => 0xFB1D,
        (0x05B7, 0x05F2) => 0xFB1F,
        (0x05B7, 0x05D0) => 0xFB2E,
        (0x05B8, 0x05D0) => 0xFB2F,
        (0x05B9, 0x05D5) => 0xFB4B,
        (0x05BC, a @ 0x05D0..=0x05EA) => DAGESH[(a - 0x05D0) as usize],
        (0x05BC, 0xFB2A) => 0xFB2C,
        (0x05BC, 0xFB2B) => 0xFB2D,
        (0x05BF, 0x05D1) => 0xFB4C,
        (0x05BF, 0x05DB) => 0xFB4D,
        (0x05BF, 0x05E4) => 0xFB4E,
        (0x05C1, 0x05E9) => 0xFB2A,
        (0x05C1, 0xFB49) => 0xFB2C,
        (0x05C2, 0x05E9) => 0xFB2B,
        (0x05C2, 0xFB49) => 0xFB2D,
        _ => 0,
    };
    char::from_u32(form).filter(|&c| c != '\0')
}

/// Composes marks with their starter where the font has the composite, unless a mark of
/// the same or a higher class comes between them.
fn recompose(font: &Font, chars: &mut Vec<Char>, shaper: Shaper) {
    let compose = |a: char, b: char| {
        composition(a, b).or_else(|| match shaper {
            Shaper::Hebrew { gpos_mark: false } => hebrew_presentation_form(a, b),
            _ => None,
        })
    };
    if chars.len() < 2 {
        return;
    }
    let mut out: Vec<Char> = Vec::with_capacity(chars.len());
    let mut starter = 0;
    for k in 0..chars.len() {
        let ch = chars[k];
        if let Some(last) = out.last()
            && char_info(ch.c) & MARK != 0
            && (starter == out.len() - 1 || combining_class(last.c) < combining_class(ch.c))
            && let Some(composed) = compose(out[starter].c, ch.c)
            && let Some(glyph) = font.glyph_index(composed)
        {
            merge_composed(&mut out, starter, ch.cluster, &mut chars[k + 1..]);
            let base = &mut out[starter];
            base.c = composed;
            base.glyph = glyph.0;
            base.space = 0;
            continue;
        }
        out.push(ch);
        if out.len() == 1 || combining_class(ch.c) == 0 {
            starter = out.len() - 1;
        }
    }
    *chars = out;
}

/// HarfBuzz's `merge_out_clusters` when a mark of cluster `mark` composes with the starter
/// at `starter`: the characters from the starter on take the smallest of their clusters and
/// the mark's, as do those before them in the starter's cluster and those after the mark
/// (`rest`) in its cluster. Nothing changes when they are all in one cluster, as they are
/// unless the mark starts a grapheme cluster.
fn merge_composed(out: &mut [Char], starter: usize, mark: usize, rest: &mut [Char]) {
    let cluster = out[starter..]
        .iter()
        .map(|ch| ch.cluster)
        .fold(mark, usize::min);
    if cluster == mark && out[starter..].iter().all(|ch| ch.cluster == mark) {
        return;
    }
    let mut start = starter;
    while start > 0 && out[start - 1].cluster == out[start].cluster {
        start -= 1;
    }
    for ch in rest.iter_mut().take_while(|ch| ch.cluster == mark) {
        ch.cluster = cluster;
    }
    for ch in &mut out[start..] {
        ch.cluster = cluster;
    }
}
