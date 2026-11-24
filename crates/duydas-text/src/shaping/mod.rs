//! Text shaping: turning text into positioned glyphs (ADR 0027).
//!
//! [`shape`] maps a run of text to glyphs with a font's cmap, then applies the font's
//! OpenType layout tables: GSUB for ligatures, contextual and language-specific forms, and
//! GPOS for kerning and mark positioning. [`itemize`] first splits text into runs of one
//! script and one font, picking for each grapheme cluster the first font of a fallback list
//! that covers it.
//!
//! The engine handles scripts that need no reordering or joining: Latin, Greek, Cyrillic,
//! Armenian, Georgian, Han, kana, Hangul and the like, written left to right. Scripts that
//! need a script-specific shaper (Arabic, the Indic scripts, Thai, Khmer and others),
//! right-to-left and vertical text are not handled yet; ADR 0027 lists what is missing.
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
//!         features: &[],
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

use crate::font::layout::{self, GlyphInfo, Kern, Limits, Plan, Position, Request};
use crate::font::{Font, GlyphId};
use crate::shaping_tables::{DECIMAL_NUMBER, DEFAULT_IGNORABLE, MARK, NONSPACING_MARK, char_info};

pub use script::{Language, Run, Script, itemize};

use normalize::{SPACE, SPACE_4_EM_18, SPACE_FIGURE, SPACE_NARROW, SPACE_PUNCTUATION};

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

/// The features HarfBuzz applies to horizontal left-to-right text, in its order, before and
/// after a script-specific shaper's: `rvrn` (in a GSUB stage of its own), the direction's,
/// automatic fractions (which apply around U+2044 FRACTION SLASH), `rand`, HarfBuzz's own
/// tags, then the common and horizontal ones. A setting in [`ShapeOptions::features`]
/// overrides one of these.
const DEFAULT_FEATURES: [Request; 9] = [
    Request::global(*b"rvrn", 1),
    Request::global(*b"ltra", 1),
    Request::global(*b"ltrm", 1),
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
}

/// A positioned glyph. Distances are in font units: scale by the size in pixels per em over
/// [`FontMetrics::units_per_em`](crate::font::FontMetrics::units_per_em).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// The glyph to draw.
    pub glyph: GlyphId,
    /// Byte offset into the shaped text of the start of the cluster the glyph belongs to.
    /// Clusters never split a grapheme cluster and do not decrease from one glyph to the
    /// next; the text from one cluster's offset to the next belongs to that cluster's glyphs.
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

/// Shapes `text` with `font`: a glyph run in visual order (left to right).
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
    // Hangul goes through HarfBuzz's Hangul shaper: jamo composition, no other normalization,
    // and marks keep their advances.
    let hangul = options.script.opentype_tag() == Some(*b"hang");
    let chars = normalize::normalize(font, text, hangul);
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
                hidden: matches!(
                    ch.c,
                    '\u{34F}' | '\u{180B}'..='\u{180D}' | '\u{180F}' | '\u{E0020}'..='\u{E007F}'
                ),
                unicode_mark,
                combining_class: if unicode_mark {
                    normalize::combining_class(ch.c)
                } else {
                    0
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
    // COMBINING GRAPHEME JOINER that keeps no marks apart can be passed over, as in HarfBuzz.
    for i in 1..info.len().saturating_sub(1) {
        let (prev, next) = (info[i - 1].combining_class, info[i + 1].combining_class);
        if chars[i].c == '\u{34F}' && (next == 0 || prev <= next) {
            info[i].hidden = false;
        }
    }

    let scripts: Vec<[u8; 4]> = options.script.opentype_tag().into_iter().collect();
    let language = options.language.opentype_tags();
    // In HarfBuzz's order: the defaults with the shaper's features, then the caller's; a
    // tag's last setting wins. The Hangul shaper then turns calt off whatever the caller
    // said, as Uniscribe does (some CJK fonts put jamo lookups there).
    let mut requests: Vec<Request> = DEFAULT_FEATURES.to_vec();
    if hangul {
        requests.extend(HANGUL_FEATURES);
    }
    requests.extend(COMMON_FEATURES);
    requests.extend(
        options
            .features
            .iter()
            .map(|f| Request::global(f.tag, f.value)),
    );
    if hangul {
        requests.push(Request::global(*b"calt", 0));
    }
    let simple = !hangul && options.features.is_empty();
    let plan = layout::plan(&tables, &scripts, language, &requests, simple);
    let jamo = [*b"ljmo", *b"vjmo", *b"tjmo"].map(|tag| plan.one_mask(tag));
    for (g, ch) in info.iter_mut().zip(&chars) {
        g.mask = plan.global_mask;
        if let Some(&mask) = usize::from(ch.jamo)
            .checked_sub(1)
            .and_then(|k| jamo.get(k))
        {
            g.mask |= mask;
        }
    }
    fraction_masks(&plan, &chars, &mut info);
    let mut limits = Limits::new(info.len());

    layout::substitute(&tables, &plan.gsub, &mut info, &mut limits);

    let mut pos: Vec<Position> = info.iter().map(|g| default_position(font, g)).collect();
    let mut attached = layout::position(&tables, &plan.gpos, &mut info, &mut pos, &mut limits);
    // The legacy kern table stands in when GPOS does not kern this script and language; it
    // kerns if the plan keeps `kern`.
    let kern = tables.kern.filter(|_| !plan.gpos_kern).and_then(Kern::new);
    // HarfBuzz resolves attachments only if something attached.
    if let Some(kern) = &kern
        && plan.global_mask & plan.mask(*b"kern") != 0
    {
        attached |= kern.apply(&info, &mut pos, &limits);
    }
    // Without GPOS (or with cross-stream kerning) nothing positions marks: they move back
    // over their advance when it is zeroed, and the fallback places them by combining class.
    let adjust = tables.gpos.is_none() && !kern.as_ref().is_some_and(|k| k.cross_stream);
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
        layout::propagate_attachments(&mut pos);
    }
    if adjust && !hangul {
        fallback::position_marks(font, &info, &mut pos, adjust, &limits);
    }
    (output(font, &info, &pos), limits)
}

/// HarfBuzz's automatic fractions: around each U+2044 FRACTION SLASH with digits on both
/// sides, the digits before get `numr`, those after `dnom`, and they and the slash `frac`, if
/// the font has `frac` or both of the others. `info` has one glyph for each of `chars`.
fn fraction_masks(plan: &Plan, chars: &[normalize::Char], info: &mut [GlyphInfo]) {
    let [frac, numr, dnom] = [*b"frac", *b"numr", *b"dnom"].map(|tag| plan.one_mask(tag));
    if frac == 0 && (numr == 0 || dnom == 0) {
        return;
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
/// advance) or, without a space glyph, removed (their cluster passing to the next glyph when
/// nothing precedes it).
fn output(font: &Font, info: &[GlyphInfo], pos: &[Position]) -> Vec<ShapedGlyph> {
    let space = font.glyph_index(' ');
    let mut glyphs: Vec<ShapedGlyph> = Vec::with_capacity(info.len());
    let mut carried: Option<(usize, usize)> = None;
    for (i, (g, p)) in info.iter().zip(pos).enumerate() {
        let mut cluster = g.cluster;
        if let Some((from, to)) = carried {
            if from == cluster {
                cluster = to;
            } else {
                carried = None;
            }
        }
        if g.ignorable {
            match space {
                Some(space) => glyphs.push(ShapedGlyph {
                    glyph: space,
                    cluster,
                    x_advance: p.x_advance,
                    y_advance: p.y_advance,
                    x_offset: p.x_offset,
                    y_offset: p.y_offset,
                }),
                // As HarfBuzz's delete_glyphs_inplace: with nothing before it, the glyph's
                // cluster merges into the next glyph's, unless that glyph shares it.
                None if glyphs.is_empty() => {
                    if let Some(next) = info
                        .get(i + 1)
                        .map(|n| n.cluster)
                        .filter(|&c| c != g.cluster)
                    {
                        carried = Some((next, cluster.min(next)));
                    }
                }
                None => {}
            }
            continue;
        }
        glyphs.push(ShapedGlyph {
            glyph: GlyphId(g.glyph),
            cluster,
            x_advance: p.x_advance,
            y_advance: p.y_advance,
            x_offset: p.x_offset,
            y_offset: p.y_offset,
        });
    }
    glyphs
}
