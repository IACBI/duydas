//! Text shaping: turning text into positioned glyphs (ADR 0027).
//!
//! [`shape`] maps a run of text to glyphs with a font's cmap, then applies the font's
//! OpenType layout tables: GSUB for ligatures, contextual and language-specific forms, and
//! GPOS for kerning and mark positioning. [`itemize`] first splits text into runs of one
//! script and one font, picking for each grapheme cluster the first font of a fallback list
//! that covers it.
//!
//! The engine handles scripts that need no reordering or joining: Latin, Greek, Cyrillic,
//! Armenian, Georgian, Han, kana, Hangul, Hebrew and the like, in either direction (ADR
//! 0028): a right-to-left run, as [`crate::bidi`] resolves it, sets
//! [`ShapeOptions::direction`]. Scripts that need a script-specific shaper (Arabic and the
//! other joining scripts, the Indic scripts, Thai, Khmer and others) and vertical text are
//! not handled yet; ADR 0027 lists what is missing.
//!
//! ```no_run
//! use duydas_text::font::Font;
//! use duydas_text::shaping::{Language, ShapeOptions, itemize, shape};
//!
//! # fn fonts() -> (Font, Font) { unimplemented!() }
//! let (latin, japanese) = fonts();
//! let text = "Merhaba, 世界!";
//! let fonts = [&latin, &japanese];
//! for run in itemize(text, &fonts) {
//!     let options = ShapeOptions {
//!         script: run.script,
//!         language: Language::from_bcp47("tr-TR"),
//!         // No extra features, left to right.
//!         ..ShapeOptions::default()
//!     };
//!     let glyphs = shape(fonts[run.font], &text[run.range.clone()], &options);
//!     // Glyph clusters are byte offsets into the run's text.
//! #   let _ = glyphs;
//! }
//! ```

mod fallback;
mod normalize;
mod script;
#[cfg(test)]
mod tests;

use crate::bidi::Direction;
use crate::font::layout::{self, GlyphInfo, Kern, Limits, Plan, Position, Request};
use crate::font::{Font, GlyphId};
use crate::shaping_tables::{
    DECIMAL_NUMBER, DEFAULT_IGNORABLE, LETTER, MARK, NONSPACING_MARK, char_info,
};

pub use script::{Language, Run, Script, itemize};

use normalize::{SPACE, SPACE_4_EM_18, SPACE_FIGURE, SPACE_NARROW, SPACE_PUNCTUATION, Shaper};

/// An OpenType feature setting, such as `liga` off or `ss01` on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Feature {
    /// The feature tag.
    pub tag: [u8; 4],
    /// 0 turns the feature off, 1 on; for an alternate substitution, which alternate. As in
    /// HarfBuzz, a value keeps its low eight bits, and features that share an alternate
    /// lookup combine their values as bit fields: `ss01=2` and `ss02=2` on one lookup pick
    /// alternate 10 (`2 | 2 << 2`), or none if the set has fewer.
    pub value: u32,
}

impl Feature {
    /// A feature setting.
    pub const fn new(tag: [u8; 4], value: u32) -> Self {
        Self { tag, value }
    }
}

/// The features HarfBuzz applies to horizontal text, in its order, before and after a
/// script-specific shaper's: `rvrn` (in a GSUB stage of its own), the direction's
/// ([`LTR_FEATURES`] or [`RTL_FEATURES`]), automatic fractions (which apply around U+2044
/// FRACTION SLASH), `rand`, HarfBuzz's own tags, then the common and horizontal ones. A
/// setting in [`ShapeOptions::features`] overrides one of these.
const RVRN: Request = Request::global(*b"rvrn", 1);
const LTR_FEATURES: [Request; 2] = [Request::global(*b"ltra", 1), Request::global(*b"ltrm", 1)];
/// `rtlm` applies only to the characters that were not mirrored.
const RTL_FEATURES: [Request; 2] = [Request::global(*b"rtla", 1), Request::masked(*b"rtlm")];
const DEFAULT_FEATURES: [Request; 6] = [
    Request::masked(*b"frac"),
    Request::masked(*b"numr"),
    Request::masked(*b"dnom"),
    Request {
        random: true,
        ..Request::global(*b"rand", 255)
    },
    Request::global(*b"Harf", 1),
    Request::global(*b"HARF", 1),
];
const COMMON_FEATURES: [Request; 16] = [
    Request::global(*b"Buzz", 1),
    Request::global(*b"BUZZ", 1),
    Request::global(*b"abvm", 1),
    Request::global(*b"blwm", 1),
    Request::global(*b"ccmp", 1),
    Request::global(*b"locl", 1),
    Request {
        manual_joiners: true,
        ..Request::global(*b"mark", 1)
    },
    Request {
        manual_joiners: true,
        ..Request::global(*b"mkmk", 1)
    },
    Request::global(*b"rlig", 1),
    Request::global(*b"calt", 1),
    Request::global(*b"clig", 1),
    Request::global(*b"curs", 1),
    Request::global(*b"dist", 1),
    Request {
        fallback: true,
        ..Request::global(*b"kern", 1)
    },
    Request::global(*b"liga", 1),
    Request::global(*b"rclt", 1),
];
/// HarfBuzz's Hangul shaper adds the jamo features, for the jamo it leaves uncomposed.
const HANGUL_FEATURES: [Request; 3] = [
    Request::masked(*b"ljmo"),
    Request::masked(*b"vjmo"),
    Request::masked(*b"tjmo"),
];

/// How to shape a run.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeOptions<'a> {
    /// The run's script, which picks the font's script-specific lookups.
    pub script: Script,
    /// The text's language, which picks the font's language-specific forms.
    pub language: Language,
    /// Feature settings on top of the defaults, the last setting of a tag winning.
    pub features: &'a [Feature],
    /// The run's direction, as the bidirectional algorithm resolved it (see
    /// [`crate::bidi`]). Right to left mirrors brackets and the like and turns on the font's
    /// right-to-left forms; either way the glyphs come out left to right.
    pub direction: Direction,
}

/// A positioned glyph. Distances are in font units: scale by the size in pixels per em over
/// [`FontMetrics::units_per_em`](crate::font::FontMetrics::units_per_em).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// The glyph to draw.
    pub glyph: GlyphId,
    /// Byte offset into the shaped text of the start of the cluster the glyph belongs to.
    /// Clusters never split a grapheme cluster. They do not decrease from one glyph to the
    /// next in left-to-right text and do not increase in right-to-left text; the text from
    /// one cluster's offset to the next larger one belongs to that cluster's glyphs.
    pub cluster: usize,
    /// How far the pen moves after the glyph.
    pub x_advance: i32,
    /// Vertical pen movement; zero for horizontal text unless the font says otherwise.
    pub y_advance: i32,
    /// Where to draw the glyph relative to the pen.
    pub x_offset: i32,
    /// Vertical drawing offset, up positive.
    pub y_offset: i32,
}

/// Shapes `text` with `font`: a glyph run in visual order (left to right), whatever the
/// run's direction.
///
/// Characters the font lacks become glyph 0, the missing-glyph box; [`itemize`] picks a
/// font for each part of the text first. Default-ignorable characters such as joiners and
/// variation selectors take part in substitution and are then drawn as zero-width spaces,
/// or dropped when the font has no space glyph.
pub fn shape(font: &Font, text: &str, options: &ShapeOptions<'_>) -> Vec<ShapedGlyph> {
    shape_with_limits(font, text, options).0
}

fn shape_with_limits(
    font: &Font,
    text: &str,
    options: &ShapeOptions<'_>,
) -> (Vec<ShapedGlyph>, Limits) {
    if text.is_empty() {
        return (Vec::new(), Limits::new(0));
    }
    let tables = font.layout_tables();
    let script = options.script.opentype_tag();
    // Hangul goes through HarfBuzz's Hangul shaper: jamo composition, no other normalization,
    // and marks keep their advances. Hebrew goes through its Hebrew shaper.
    let hangul = script == Some(*b"hang");
    let hebrew = script == Some(*b"hebr");
    let rtl = options.direction == Direction::RightToLeft;
    // As HarfBuzz, a run is shaped in its script's own direction: text written the other
    // way round has its grapheme clusters reversed first. Digits or regional indicators
    // without letters count as left to right in a right-to-left script.
    let native = match options.script.horizontal_direction() {
        Some(Direction::RightToLeft) if !rtl && numeric(text) => Some(Direction::LeftToRight),
        native => native,
    };
    let reverse = native.is_some_and(|d| d != options.direction);
    // The glyphs are right-to-left text in logical order, to be reversed at the end.
    let backward = rtl != reverse;

    let scripts: Vec<[u8; 4]> = script.into_iter().collect();
    let language = options.language.opentype_tags();
    let requests = requests(hangul, rtl, options.features);
    let simple = !hangul && options.features.is_empty();
    let plan = layout::plan(&tables, &scripts, language, &requests, simple);

    let shaper = if hangul {
        Shaper::Hangul
    } else if hebrew {
        Shaper::Hebrew {
            gpos_mark: plan.one_mask(*b"mark") != 0,
        }
    } else {
        Shaper::Default
    };
    let chars = normalize::normalize(font, text, shaper, rtl, reverse);
    let mut info: Vec<GlyphInfo> = chars
        .iter()
        .map(|ch| {
            let flags = if ch.unmatched_selector {
                char_info(ch.c) & !(MARK | NONSPACING_MARK)
            } else {
                char_info(ch.c)
            };
            let unicode_mark = flags & MARK != 0;
            let mut info = GlyphInfo {
                glyph: ch.glyph,
                cluster: ch.cluster,
                ignorable: flags & DEFAULT_IGNORABLE != 0,
                zwnj: ch.c == '\u{200C}',
                zwj: ch.c == '\u{200D}',
                // A COMBINING GRAPHEME JOINER that keeps no marks apart can be passed over,
                // as in HarfBuzz.
                hidden: matches!(
                    ch.c,
                    '\u{34F}' | '\u{180B}'..='\u{180D}' | '\u{180F}' | '\u{E0020}'..='\u{E007F}'
                ) && !ch.passable_joiner,
                unicode_mark,
                // Only the fallback positioning reads it, so it can take HarfBuzz's
                // recategorized classes.
                combining_class: match (unicode_mark, flags & NONSPACING_MARK != 0) {
                    (false, _) => 0,
                    (true, false) => normalize::combining_class(ch.c),
                    (true, true) => fallback::recategorize(ch.c, normalize::combining_class(ch.c)),
                },
                nonspacing_mark: flags & NONSPACING_MARK != 0,
                space: ch.space,
                ..GlyphInfo::default()
            };
            // As HarfBuzz, a default ignorable (COMBINING GRAPHEME JOINER, the Mongolian
            // variation selectors) is never a mark when the font has no glyph classes, so
            // IgnoreMarks does not pass over it.
            let class = if flags & NONSPACING_MARK != 0 && flags & DEFAULT_IGNORABLE == 0 {
                layout::MARK
            } else {
                layout::BASE
            };
            tables.gdef.classify(&mut info, class);
            info
        })
        .collect();

    let jamo = [*b"ljmo", *b"vjmo", *b"tjmo"].map(|tag| plan.one_mask(tag));
    // HarfBuzz sets only the bit for value 1, whatever value the caller gave rtlm.
    let rtlm = if rtl { plan.one_mask(*b"rtlm") } else { 0 };
    for (g, ch) in info.iter_mut().zip(&chars) {
        g.mask = plan.global_mask;
        if let Some(&mask) = usize::from(ch.jamo)
            .checked_sub(1)
            .and_then(|k| jamo.get(k))
        {
            g.mask |= mask;
        }
        if !ch.mirrored {
            g.mask |= rtlm;
        }
    }
    fraction_masks(&plan, &chars, &mut info, backward);
    let mut limits = Limits::new(info.len());

    layout::substitute(&tables, &plan.gsub, &mut info, &mut limits);

    let mut pos: Vec<Position> = info.iter().map(|g| default_position(font, g)).collect();
    // The Hebrew shaper leaves GPOS out unless the font has it for Hebrew itself: old fonts
    // position Hebrew marks for other scripts' sake.
    let gpos = tables.gpos.is_some() && (!hebrew || plan.gpos_script == Some(*b"hebr"));
    let mut attached = gpos
        && layout::position(
            &tables,
            &plan.gpos,
            &mut info,
            &mut pos,
            &mut limits,
            backward,
        );
    // The legacy kern table stands in when GPOS does not kern this script and language; it
    // kerns if the plan keeps `kern`.
    let kern = tables
        .kern
        .filter(|_| !(gpos && plan.gpos_kern))
        .and_then(Kern::new);
    // HarfBuzz resolves attachments only if something attached.
    if let Some(kern) = &kern
        && plan.global_mask & plan.mask(*b"kern") != 0
    {
        attached |= kern.apply(&info, &mut pos, &limits, backward);
    }
    // Without GPOS (or with cross-stream kerning) nothing positions marks: the fallback
    // places them by combining class. Left to right, a mark moves back over its advance when
    // that is zeroed; right to left it then hangs over the glyph after it, which is the one
    // before it once the glyphs are reversed.
    let fallback = !gpos && !kern.as_ref().is_some_and(|k| k.cross_stream);
    let adjust = fallback && !backward;
    // Marks take no room once GPOS has positioned them (HarfBuzz's "zero width marks by
    // GDEF, late").
    if !hangul {
        for (g, p) in info.iter().zip(&mut pos) {
            if g.class == layout::MARK {
                if adjust {
                    p.x_offset = p.x_offset.wrapping_sub(p.x_advance);
                    p.y_offset = p.y_offset.wrapping_sub(p.y_advance);
                }
                p.x_advance = 0;
                p.y_advance = 0;
            }
        }
    }
    for (g, p) in info.iter().zip(&mut pos) {
        if g.ignorable {
            (p.x_advance, p.y_advance, p.x_offset, p.y_offset) = (0, 0, 0, 0);
        }
    }
    if attached {
        layout::propagate_attachments(&mut pos, backward);
    }
    if fallback && !hangul {
        fallback::position_marks(font, &info, &mut pos, adjust, (rtl, backward), &limits);
    }
    if backward {
        info.reverse();
        pos.reverse();
    }
    (output(font, &info, &pos), limits)
}

/// The features HarfBuzz requests for a run, in its order: the defaults with the shaper's
/// features, then the caller's, a tag's last setting winning. The Hangul shaper then turns
/// calt off whatever the caller said, as Uniscribe does (some CJK fonts put jamo lookups
/// there).
fn requests(hangul: bool, rtl: bool, features: &[Feature]) -> Vec<Request> {
    let mut requests: Vec<Request> = vec![RVRN];
    requests.extend(if rtl { RTL_FEATURES } else { LTR_FEATURES });
    requests.extend(DEFAULT_FEATURES);
    if hangul {
        requests.extend(HANGUL_FEATURES);
    }
    requests.extend(COMMON_FEATURES);
    requests.extend(features.iter().map(|f| Request::global(f.tag, f.value)));
    if hangul {
        requests.push(Request::global(*b"calt", 0));
    }
    requests
}

/// The Hebrew shaper `shape` uses for `font` with no features, in the default language:
/// points compose into presentation forms unless the font has a `mark` feature for the
/// script it picks.
fn hebrew_shaper(font: &Font) -> Shaper {
    let tables = font.layout_tables();
    let language = Language::default().opentype_tags();
    let plan = layout::plan(
        &tables,
        &[*b"hebr"],
        language,
        &requests(false, false, &[]),
        true,
    );
    Shaper::Hebrew {
        gpos_mark: plan.one_mask(*b"mark") != 0,
    }
}

/// Whether `text` has digits or regional indicators and no letters, which HarfBuzz shapes
/// left to right even in a right-to-left script.
fn numeric(text: &str) -> bool {
    let mut numeric = false;
    for c in text.chars() {
        let flags = char_info(c);
        if flags & LETTER != 0 {
            return false;
        }
        numeric |= flags & DECIMAL_NUMBER != 0 || ('\u{1F1E6}'..='\u{1F1FF}').contains(&c);
    }
    numeric
}

/// HarfBuzz's automatic fractions: around each U+2044 FRACTION SLASH with digits on both
/// sides, the digits before get `numr`, those after `dnom`, and they and the slash `frac`, if
/// the font has `frac` or both of the others. In right-to-left text in logical order
/// (`backward`), HarfBuzz swaps `numr` and `dnom`. `info` has one glyph for each of
/// `chars`.
fn fraction_masks(plan: &Plan, chars: &[normalize::Char], info: &mut [GlyphInfo], backward: bool) {
    let [frac, mut numr, mut dnom] = [*b"frac", *b"numr", *b"dnom"].map(|tag| plan.one_mask(tag));
    if frac == 0 && (numr == 0 || dnom == 0) {
        return;
    }
    if backward {
        (numr, dnom) = (dnom, numr);
    }
    let digit = |i: usize| char_info(chars[i].c) & DECIMAL_NUMBER != 0;
    // Where the digits just before `i` start, so long runs are not searched again.
    let mut run = 0;
    let mut i = 0;
    while i < info.len() {
        if chars[i].c == '\u{2044}' {
            let end = (i + 1..info.len())
                .find(|&j| !digit(j))
                .unwrap_or(info.len());
            if run < i && end > i + 1 {
                for g in &mut info[run..i] {
                    g.mask |= numr | frac;
                }
                info[i].mask |= frac;
                for g in &mut info[i + 1..end] {
                    g.mask |= frac | dnom;
                }
                run = i + 1;
                i = end;
                continue;
            }
        }
        if !digit(i) {
            run = i + 1;
        }
        i += 1;
    }
}

/// A glyph's advance before GPOS. A space the font has no glyph for takes its own width with
/// the U+0020 glyph.
fn default_position(font: &Font, g: &GlyphInfo) -> Position {
    let advance = |glyph: u16| font.advance(GlyphId(glyph)).map_or(0, i32::from);
    let em = i32::from(font.metrics().units_per_em);
    let x_advance = match g.space {
        0 | SPACE => advance(g.glyph),
        n @ (1..=6 | 16) => (em + i32::from(n) / 2) / i32::from(n),
        SPACE_4_EM_18 => em * 4 / 18,
        SPACE_FIGURE => ('0'..='9')
            .find_map(|c| font.glyph_index(c))
            .map_or(advance(g.glyph), |d| advance(d.0)),
        SPACE_PUNCTUATION => ['.', ',']
            .into_iter()
            .find_map(|c| font.glyph_index(c))
            .map_or(advance(g.glyph), |d| advance(d.0)),
        SPACE_NARROW => advance(g.glyph) / 2,
        _ => advance(g.glyph),
    };
    Position::with_advance(x_advance)
}

/// The shaped glyphs, with default ignorables drawn with the space glyph (they have no
/// advance) or, without a space glyph, removed as HarfBuzz's `delete_glyphs_inplace` does:
/// a removed glyph's cluster passes to the next glyph when nothing precedes it, and to the
/// glyphs before it when it is smaller than theirs (as in right-to-left text), unless the next
/// glyph shares it.
fn output(font: &Font, info: &[GlyphInfo], pos: &[Position]) -> Vec<ShapedGlyph> {
    let space = font.glyph_index(' ');
    let mut glyphs: Vec<ShapedGlyph> = Vec::with_capacity(info.len());
    let mut carried: Option<(usize, usize)> = None;
    // The glyphs from `run` on share a cluster; `lowered` is the smaller one a removed
    // glyph passed to them, set on them only when another glyph follows, so that a long run
    // followed by many removed glyphs is not rewritten for each.
    let mut run = 0;
    let mut lowered: Option<usize> = None;
    for (i, (g, p)) in info.iter().zip(pos).enumerate() {
        let mut cluster = g.cluster;
        if let Some((from, to)) = carried {
            if from == cluster {
                cluster = to;
            } else {
                carried = None;
            }
        }
        let glyph = match (g.ignorable, space) {
            (false, _) => GlyphId(g.glyph),
            (true, Some(space)) => space,
            (true, None) => {
                let next = info.get(i + 1).map(|n| n.cluster);
                if next == Some(g.cluster) {
                    continue;
                }
                match lowered.or_else(|| glyphs.last().map(|g| g.cluster)) {
                    Some(last) if cluster < last => {
                        lowered = Some(cluster);
                        while run > 0 && glyphs[run - 1].cluster == cluster {
                            run -= 1;
                        }
                    }
                    Some(_) => {}
                    None => {
                        if let Some(next) = next {
                            carried = Some((next, cluster.min(next)));
                        }
                    }
                }
                continue;
            }
        };
        if let Some(lowered) = lowered.take() {
            for g in &mut glyphs[run..] {
                g.cluster = lowered;
            }
        }
        if glyphs.last().is_none_or(|last| last.cluster != cluster) {
            run = glyphs.len();
        }
        glyphs.push(ShapedGlyph {
            glyph,
            cluster,
            x_advance: p.x_advance,
            y_advance: p.y_advance,
            x_offset: p.x_offset,
            y_offset: p.y_offset,
        });
    }
    if let Some(lowered) = lowered {
        for g in &mut glyphs[run..] {
            g.cluster = lowered;
        }
    }
    glyphs
}
