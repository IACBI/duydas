//! OpenType fonts: parsing, outlines and rasterization (ADR 0026).
//!
//! [`Font`] reads TrueType (`glyf`) and CFF outlines from `.ttf`, `.otf` and `.ttc` data, and
//! answers what layout asks of a font: which glyph a character maps to, how far a glyph
//! advances, and the font's line metrics. [`Rasterizer`] turns a glyph into an anti-aliased
//! coverage mask.
//!
//! Every read is bounds-checked, so a malformed font yields a [`FontError`], never a panic, and
//! the work and memory one glyph can take are bounded.

mod cff;
mod cmap;
mod glyf;
pub(crate) mod layout;
mod outline;
mod raster;
mod reader;
#[cfg(test)]
pub(crate) mod tests;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use outline::{Bounds, OutlineSink};
pub use raster::{GlyphImage, MAX_GLYPH_SIZE, Rasterizer};

use cmap::{Cmap, Variation};
use outline::BoundsSink;
use reader::{Reader, Span, i16_at, u16_at, u32_at};

const SFNT: u32 = 0x0001_0000;
const OTTO: u32 = u32::from_be_bytes(*b"OTTO");
const TRUE: u32 = u32::from_be_bytes(*b"true");
const TTCF: u32 = u32::from_be_bytes(*b"ttcf");

/// A glyph's index in its font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(pub u16);

/// Why a font or glyph could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontError {
    /// The data is not an OpenType font or font collection.
    UnknownFormat,
    /// The collection has no face with that index.
    NoSuchFace,
    /// A table the font needs is missing.
    MissingTable(&'static str),
    /// A table is truncated or inconsistent.
    Malformed(&'static str),
    /// The font uses something this implementation does not support.
    Unsupported(&'static str),
    /// The glyph id is past the font's last glyph.
    NoSuchGlyph,
    /// The glyph needs more work or memory than the limits allow.
    TooLarge,
    /// The requested size or position is out of range.
    InvalidSize,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("not an OpenType font or collection"),
            Self::NoSuchFace => f.write_str("no face with that index"),
            Self::MissingTable(table) => write!(f, "missing '{table}' table"),
            Self::Malformed(table) => write!(f, "malformed {table}"),
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::NoSuchGlyph => f.write_str("glyph id out of range"),
            Self::TooLarge => f.write_str("glyph exceeds the size limits"),
            Self::InvalidSize => f.write_str("size or position out of range"),
        }
    }
}

impl std::error::Error for FontError {}

/// Vertical metrics in font units, y up: the ascender is positive and the descender negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontMetrics {
    /// Font units per em; a size in pixels per em divided by this scales font units to pixels.
    pub units_per_em: u16,
    /// Distance from the baseline to the top of a line.
    pub ascender: i16,
    /// Distance from the baseline to the bottom of a line (negative).
    pub descender: i16,
    /// Extra space between lines.
    pub line_gap: i16,
    /// Height of lowercase letters, if the font says.
    pub x_height: Option<i16>,
    /// Height of capital letters, if the font says.
    pub cap_height: Option<i16>,
    /// Position of the top of the underline (negative below the baseline).
    pub underline_position: i16,
    /// Underline thickness.
    pub underline_thickness: i16,
    /// Position of the top of the strikeout line.
    pub strikeout_position: i16,
    /// Strikeout thickness.
    pub strikeout_thickness: i16,
}

/// What a face looks like, for picking faces from a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontStyle {
    /// From 1 to 1000: 400 is regular, 700 bold.
    pub weight: u16,
    /// From 1 (ultra-condensed) to 9 (ultra-expanded); 5 is normal.
    pub width: u16,
    /// Italic or oblique.
    pub italic: bool,
    /// Every glyph has the same advance.
    pub monospace: bool,
}

/// An OpenType face, parsed once and cheap to clone.
///
/// Font data is shared; table contents are read on demand.
#[derive(Clone)]
pub struct Font {
    data: Arc<[u8]>,
    id: u64,
    num_glyphs: u16,
    metrics: FontMetrics,
    style: FontStyle,
    cmap: Cmap,
    hmtx: Span,
    long_metrics: u16,
    outlines: Outlines,
    name: Option<Span>,
    variable: bool,
    gdef: Option<Span>,
    gsub: Option<Span>,
    gpos: Option<Span>,
    kern: Option<Span>,
    /// Shared by clones, so what one shaping works out serves the next.
    layout_cache: Arc<layout::Cache>,
}

#[derive(Clone, Debug)]
enum Outlines {
    Glyf(glyf::Glyf),
    Cff(Arc<cff::Cff>),
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("id", &self.id)
            .field("family", &self.family_name())
            .field("glyphs", &self.num_glyphs)
            .finish_non_exhaustive()
    }
}

/// Table locations of one face.
#[derive(Debug, Default)]
struct Tables {
    head: Option<Span>,
    hhea: Option<Span>,
    maxp: Option<Span>,
    hmtx: Option<Span>,
    cmap: Option<Span>,
    loca: Option<Span>,
    glyf: Option<Span>,
    cff: Option<Span>,
    cff2: Option<Span>,
    os2: Option<Span>,
    post: Option<Span>,
    name: Option<Span>,
    fvar: Option<Span>,
    gdef: Option<Span>,
    gsub: Option<Span>,
    gpos: Option<Span>,
    kern: Option<Span>,
    bitmaps: bool,
}

impl Tables {
    fn read(data: &[u8], offset: usize) -> Result<Self, FontError> {
        const DIRECTORY: FontError = FontError::Malformed("table directory");
        let mut r = Reader::at(data, offset).ok_or(FontError::UnknownFormat)?;
        if !matches!(r.u32(), Some(SFNT | OTTO | TRUE)) {
            return Err(FontError::UnknownFormat);
        }
        let count = r.u16().ok_or(DIRECTORY)?;
        r.skip(6).ok_or(DIRECTORY)?;
        let mut t = Self::default();
        for _ in 0..count {
            let tag = r.bytes(4).ok_or(DIRECTORY)?;
            r.skip(4).ok_or(DIRECTORY)?;
            let start = r.u32().ok_or(DIRECTORY)? as usize;
            let length = r.u32().ok_or(DIRECTORY)? as usize;
            let slot = match tag {
                b"head" => &mut t.head,
                b"hhea" => &mut t.hhea,
                b"maxp" => &mut t.maxp,
                b"hmtx" => &mut t.hmtx,
                b"cmap" => &mut t.cmap,
                b"loca" => &mut t.loca,
                b"glyf" => &mut t.glyf,
                b"CFF " => &mut t.cff,
                b"CFF2" => &mut t.cff2,
                b"OS/2" => &mut t.os2,
                b"post" => &mut t.post,
                b"name" => &mut t.name,
                b"fvar" => &mut t.fvar,
                b"GDEF" => &mut t.gdef,
                b"GSUB" => &mut t.gsub,
                b"GPOS" => &mut t.gpos,
                b"kern" => &mut t.kern,
                // Apple's bitmap-only fonts have `bhed` in place of `head`.
                b"CBDT" | b"EBDT" | b"sbix" | b"bdat" | b"bhed" => {
                    t.bitmaps = true;
                    continue;
                }
                _ => continue,
            };
            let end = start
                .checked_add(length)
                .filter(|&end| end <= data.len())
                .ok_or(DIRECTORY)?;
            *slot = Some(Span { start, end });
        }
        Ok(t)
    }
}

/// Reads a table field that a length check has already guaranteed.
fn field<T>(value: Option<T>, table: &'static str) -> Result<T, FontError> {
    value.ok_or(FontError::Malformed(table))
}

impl Font {
    /// How many faces `data` holds: the count of a collection, 1 for a single font, or `None`
    /// if it is not OpenType data.
    pub fn face_count(data: &[u8]) -> Option<u32> {
        match u32_at(data, 0)? {
            TTCF => {
                u32_at(data, 8).filter(|&n| n > 0 && 12 + 4 * u64::from(n) <= data.len() as u64)
            }
            SFNT | OTTO | TRUE => Some(1),
            _ => None,
        }
    }

    /// Parses face `index` of a font or collection (`0` for a single font).
    ///
    /// Supports TrueType and CFF outlines. CFF2 and bitmap-only fonts fail with
    /// [`FontError::Unsupported`]. A variable font gives its default instance.
    pub fn from_data(data: impl Into<Arc<[u8]>>, index: u32) -> Result<Self, FontError> {
        let data: Arc<[u8]> = data.into();
        let offset = match u32_at(&data, 0).ok_or(FontError::UnknownFormat)? {
            TTCF => {
                let count = u32_at(&data, 8).ok_or(FontError::Malformed("collection header"))?;
                if index >= count {
                    return Err(FontError::NoSuchFace);
                }
                // In u64 so a huge index cannot overflow on 32-bit targets.
                let entry = usize::try_from(12 + 4 * u64::from(index))
                    .map_err(|_| FontError::Malformed("collection header"))?;
                u32_at(&data, entry).ok_or(FontError::Malformed("collection header"))? as usize
            }
            SFNT | OTTO | TRUE if index == 0 => 0,
            SFNT | OTTO | TRUE => return Err(FontError::NoSuchFace),
            _ => return Err(FontError::UnknownFormat),
        };
        let t = Tables::read(&data, offset)?;
        let required = |span: Option<Span>, name| span.ok_or(FontError::MissingTable(name));

        if t.head.is_none() && t.bitmaps {
            return Err(FontError::Unsupported("bitmap-only font"));
        }
        let head = required(t.head, "head")?.of(&data);
        if head.len() < 54 {
            return Err(FontError::Malformed("head"));
        }
        let units_per_em = field(u16_at(head, 18), "head")?;
        if !(16..=16384).contains(&units_per_em) {
            return Err(FontError::Malformed("head"));
        }
        let long_loca = match field(i16_at(head, 50), "head")? {
            0 => false,
            1 => true,
            _ => return Err(FontError::Malformed("head")),
        };
        let mac_style = field(u16_at(head, 44), "head")?;

        let maxp = required(t.maxp, "maxp")?.of(&data);
        let num_glyphs = u16_at(maxp, 4).ok_or(FontError::Malformed("maxp"))?;

        let hhea = required(t.hhea, "hhea")?.of(&data);
        if hhea.len() < 36 {
            return Err(FontError::Malformed("hhea"));
        }
        let hhea_metrics = (
            field(i16_at(hhea, 4), "hhea")?,
            field(i16_at(hhea, 6), "hhea")?,
            field(i16_at(hhea, 8), "hhea")?,
        );
        let cmap = Cmap::parse(&data, required(t.cmap, "cmap")?)?;

        // The glyph count is what both maxp and the outline table can supply, so every glyph
        // id the font hands out has an outline to read.
        let (outlines, num_glyphs) = match (t.glyf, t.loca, t.cff) {
            (Some(glyf), Some(loca), _) => {
                let entry = if long_loca { 4 } else { 2 };
                let located = ((loca.end - loca.start) / entry).saturating_sub(1);
                let num_glyphs = num_glyphs.min(u16::try_from(located).unwrap_or(u16::MAX));
                let glyf = glyf::Glyf {
                    loca,
                    glyf,
                    long: long_loca,
                    num_glyphs,
                };
                (Outlines::Glyf(glyf), num_glyphs)
            }
            (_, _, Some(cff)) => {
                let cff = cff::Cff::parse(&data, cff, units_per_em)?;
                let num_glyphs =
                    num_glyphs.min(u16::try_from(cff.glyph_count()).unwrap_or(u16::MAX));
                (Outlines::Cff(Arc::new(cff)), num_glyphs)
            }
            _ if t.cff2.is_some() => return Err(FontError::Unsupported("CFF2 outlines")),
            _ if t.bitmaps => return Err(FontError::Unsupported("bitmap-only font")),
            _ => return Err(FontError::MissingTable("glyf")),
        };

        let long_metrics = field(u16_at(hhea, 34), "hhea")?;
        if long_metrics == 0 {
            return Err(FontError::Malformed("hmtx"));
        }
        let long_metrics = long_metrics.min(num_glyphs.max(1));
        // Every glyph has an advance and a left side bearing.
        let hmtx = required(t.hmtx, "hmtx")?;
        let needed = 4 * usize::from(long_metrics)
            + 2 * usize::from(num_glyphs.saturating_sub(long_metrics));
        if hmtx.end - hmtx.start < needed {
            return Err(FontError::Malformed("hmtx"));
        }

        let os2 = t
            .os2
            .map(|span| span.of(&data))
            .filter(|os2| os2.len() >= 78);
        let post = t
            .post
            .map(|span| span.of(&data))
            .filter(|post| post.len() >= 16);
        let (metrics, style) = Self::read_metrics(units_per_em, hhea_metrics, mac_style, os2, post);

        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            num_glyphs,
            metrics,
            style,
            cmap,
            hmtx,
            long_metrics,
            outlines,
            name: t.name,
            variable: t.fvar.is_some(),
            gdef: t.gdef,
            gsub: t.gsub,
            gpos: t.gpos,
            kern: t.kern,
            layout_cache: Arc::default(),
            data,
        })
    }

    /// Line metrics follow the typographic values in `OS/2` when the font asks for them
    /// (USE_TYPO_METRICS), and `hhea` otherwise, as browsers and FreeType do. A font whose
    /// `hhea` has neither ascender nor descender falls back to the typographic values, then to
    /// the Windows ones, as FreeType does.
    fn read_metrics(
        units_per_em: u16,
        (ascender, descender, line_gap): (i16, i16, i16),
        mac_style: u16,
        os2: Option<&[u8]>,
        post: Option<&[u8]>,
    ) -> (FontMetrics, FontStyle) {
        let get = |table: Option<&[u8]>, at| table.and_then(|t| i16_at(t, at));
        let getu = |table: Option<&[u8]>, at| table.and_then(|t| u16_at(t, at));
        let selection = getu(os2, 62).unwrap_or(0);
        let (ascender, descender, line_gap) = match os2 {
            Some(os2) if selection & (1 << 7) != 0 => (
                i16_at(os2, 68).unwrap_or(ascender),
                i16_at(os2, 70).unwrap_or(descender),
                i16_at(os2, 72).unwrap_or(line_gap),
            ),
            Some(os2) if ascender == 0 && descender == 0 => {
                let typo = (i16_at(os2, 68), i16_at(os2, 70), i16_at(os2, 72));
                match typo {
                    (Some(a), Some(d), Some(g)) if a != 0 || d != 0 => (a, d, g),
                    _ => {
                        let ascent = u16_at(os2, 74).and_then(|v| i16::try_from(v).ok());
                        // A descent of 32,768 still fits once negated.
                        let descent =
                            u16_at(os2, 76).and_then(|v| i16::try_from(-i32::from(v)).ok());
                        match (ascent, descent) {
                            (Some(a), Some(d)) => (a, d, 0),
                            _ => (ascender, descender, line_gap),
                        }
                    }
                }
            }
            _ => (ascender, descender, line_gap),
        };
        let version = getu(os2, 0).unwrap_or(0);
        let v2 = os2.filter(|t| version >= 2 && t.len() >= 90);
        let thickness = |value: Option<i16>| value.filter(|&v| v > 0);
        let default_thickness = (units_per_em / 14) as i16;
        let metrics = FontMetrics {
            units_per_em,
            ascender,
            descender,
            line_gap,
            x_height: get(v2, 86).filter(|&v| v > 0),
            cap_height: get(v2, 88).filter(|&v| v > 0),
            underline_position: get(post, 8).unwrap_or(-(default_thickness)),
            underline_thickness: thickness(get(post, 10)).unwrap_or(default_thickness),
            strikeout_position: get(os2, 28).unwrap_or(ascender / 3),
            strikeout_thickness: thickness(get(os2, 26)).unwrap_or(default_thickness),
        };
        let style = FontStyle {
            weight: getu(os2, 4)
                .filter(|w| (1..=1000).contains(w))
                // Without one, FreeType goes by head's bold flag.
                .unwrap_or(if mac_style & 1 != 0 { 700 } else { 400 }),
            width: getu(os2, 6).filter(|w| (1..=9).contains(w)).unwrap_or(5),
            italic: selection & (1 | 1 << 9) != 0 || mac_style & 2 != 0,
            monospace: post.and_then(|t| u32_at(t, 12)).is_some_and(|v| v != 0),
        };
        (metrics, style)
    }

    /// A process-unique identity, for caches keyed by font.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The number of glyphs.
    pub fn glyph_count(&self) -> u16 {
        self.num_glyphs
    }

    /// Line and decoration metrics.
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// Weight, width and slant.
    pub fn style(&self) -> FontStyle {
        self.style
    }

    /// Whether the font has variation axes. Only its default instance is available.
    pub fn is_variable(&self) -> bool {
        self.variable
    }

    /// The glyph for a character, or `None` if the font does not cover it.
    pub fn glyph_index(&self, c: char) -> Option<GlyphId> {
        self.cmap
            .lookup(&self.data, u32::from(c))
            .filter(|&g| g < self.num_glyphs)
            .map(GlyphId)
    }

    /// The glyph for `c` followed by the variation selector `selector`, such as an
    /// ideographic variant of a kanji, or `None` if the font does not list the sequence; the
    /// caller then shows `c`'s usual glyph.
    pub fn glyph_variant(&self, c: char, selector: char) -> Option<GlyphId> {
        match self
            .cmap
            .variation(&self.data, u32::from(c), u32::from(selector))
        {
            Variation::Default => self.glyph_index(c),
            Variation::Glyph(g) => (g != 0 && g < self.num_glyphs).then_some(GlyphId(g)),
            Variation::Missing => None,
        }
    }

    /// The font's layout tables; a GSUB or GPOS table of an unknown major version counts as
    /// absent.
    pub(crate) fn layout_tables(&self) -> layout::Tables<'_> {
        let table = |span: Option<Span>| span.map(|s| s.of(&self.data));
        let known = |t: &&[u8]| u16_at(t, 0) == Some(1);
        layout::Tables {
            gdef: table(self.gdef).map(layout::Gdef::new).unwrap_or_default(),
            gsub: table(self.gsub).filter(known),
            gpos: table(self.gpos).filter(known),
            kern: table(self.kern),
            cache: Some(&self.layout_cache),
        }
    }

    /// How far the pen moves after the glyph, in font units.
    pub fn advance(&self, glyph: GlyphId) -> Option<u16> {
        if glyph.0 >= self.num_glyphs {
            return None;
        }
        if self.long_metrics == 0 {
            return Some(0);
        }
        // Glyphs past the long metrics share the last advance.
        let i = usize::from(glyph.0.min(self.long_metrics - 1));
        u16_at(self.hmtx.of(&self.data), 4 * i)
    }

    /// The left side bearing from `hmtx`, if the table has it.
    fn left_side_bearing(&self, glyph: GlyphId) -> Option<i16> {
        let hmtx = self.hmtx.of(&self.data);
        let (g, n) = (usize::from(glyph.0), usize::from(self.long_metrics));
        if g < n {
            i16_at(hmtx, 4 * g + 2)
        } else {
            i16_at(hmtx, 4 * n + 2 * (g - n))
        }
    }

    /// A TrueType glyph's extents as HarfBuzz reads them at the default instance: the box its
    /// header records, placed by its left side bearing, as the left edge, the top, the width
    /// and the height (down, so negative). Single-point contours count, though they draw
    /// nothing. A glyph without an outline has zero extents. `None` for a CFF glyph, whose
    /// box comes from its outline.
    pub(crate) fn header_extents(&self, glyph: GlyphId) -> Result<Option<[i32; 4]>, FontError> {
        let Outlines::Glyf(glyf) = &self.outlines else {
            return Ok(None);
        };
        let bytes = glyf.bytes(&self.data, glyph.0)?;
        // HarfBuzz reads a glyph too short for its header as empty.
        let Some([x0, y0, x1, y1]) = [2, 4, 6, 8]
            .into_iter()
            .map(|k| i16_at(bytes, k).map(i32::from))
            .collect::<Option<Vec<i32>>>()
            .and_then(|v| <[i32; 4]>::try_from(v).ok())
        else {
            return Ok(Some([0; 4]));
        };
        let left = self.left_side_bearing(glyph).map_or(x0.min(x1), i32::from);
        let top = y0.max(y1);
        Ok(Some([left, top, (x1 - x0).abs(), y0.min(y1) - top]))
    }

    /// Sends the glyph's outline to `sink`, in font units with y up. On error the sink may
    /// have received part of the outline.
    pub fn outline<S: OutlineSink>(&self, glyph: GlyphId, sink: &mut S) -> Result<(), FontError> {
        if glyph.0 >= self.num_glyphs {
            return Err(FontError::NoSuchGlyph);
        }
        match &self.outlines {
            Outlines::Glyf(glyf) => {
                let lsb = |g| self.left_side_bearing(GlyphId(g));
                glyf.outline(&self.data, glyph.0, &lsb, sink)
            }
            Outlines::Cff(cff) => cff.outline(&self.data, glyph.0, sink),
        }
    }

    /// The control box of the glyph's outline, or `None` for a glyph without one.
    pub fn bounds(&self, glyph: GlyphId) -> Result<Option<Bounds>, FontError> {
        let mut sink = BoundsSink::default();
        self.outline(glyph, &mut sink)?;
        Ok(sink.bounds)
    }

    /// The family name, preferring the typographic family (which groups more than four
    /// styles) and English.
    pub fn family_name(&self) -> Option<String> {
        self.name(&[16, 1])
    }

    /// The style name within the family, such as "Bold Italic".
    pub fn subfamily_name(&self) -> Option<String> {
        self.name(&[17, 2])
    }

    /// The full name, such as "Segoe UI Bold".
    pub fn full_name(&self) -> Option<String> {
        self.name(&[4])
    }

    /// The PostScript name, a unique ASCII identifier.
    pub fn postscript_name(&self) -> Option<String> {
        self.name(&[6])
    }

    /// The first of the name ids the font has, preferring Windows US English, then other
    /// Windows English, then Unicode, then any Windows language, then Mac English.
    fn name(&self, ids: &[u16]) -> Option<String> {
        let table = self.name?.of(&self.data);
        let count = usize::from(u16_at(table, 2)?);
        let storage = usize::from(u16_at(table, 4)?);
        for &id in ids {
            let mut best: Option<(u8, u16, usize, usize)> = None;
            for i in 0..count {
                let mut r = Reader::at(table, 6 + 12 * i)?;
                let (platform, encoding, language) = (r.u16()?, r.u16()?, r.u16()?);
                if r.u16()? != id {
                    continue;
                }
                let (length, offset) = (usize::from(r.u16()?), usize::from(r.u16()?));
                let score = match (platform, encoding, language) {
                    // Windows names are UTF-16 in the symbol encoding too.
                    (3, 0 | 1 | 10, 0x409) => 5,
                    // Every English locale has primary language 0x09.
                    (3, 0 | 1 | 10, l) if l & 0x3FF == 0x09 => 4,
                    (0, _, _) => 3,
                    (3, 0 | 1 | 10, _) => 2,
                    (1, 0, 0) => 1,
                    _ => continue,
                };
                if best.is_none_or(|(s, ..)| score > s) {
                    best = Some((score, platform, length, offset));
                }
            }
            if let Some((_, platform, length, offset)) = best {
                let start = storage + offset;
                let bytes = table.get(start..start + length)?;
                return Some(if platform == 1 {
                    bytes.iter().map(|&b| mac_roman(b)).collect()
                } else {
                    char::decode_utf16(
                        bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&p| u16::from_be_bytes(p)),
                    )
                    .map(|c| c.unwrap_or('\u{FFFD}'))
                    .collect()
                });
            }
        }
        None
    }
}

/// Decodes a Mac Roman byte.
fn mac_roman(b: u8) -> char {
    /// U+0080 to U+00FF of Mac Roman, from Apple's mapping (ROMAN.TXT).
    const HIGH: [u16; 128] = [
        0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4,
        0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8, 0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF,
        0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020,
        0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9, 0x2122, 0x00B4,
        0x00A8, 0x2260, 0x00C6, 0x00D8, 0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202,
        0x2211, 0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8, 0x00BF, 0x00A1,
        0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB, 0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3,
        0x00D5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA,
        0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02, 0x2021, 0x00B7, 0x201A,
        0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF, 0x00CC,
        0x00D3, 0x00D4, 0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF,
        0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
    ];
    match b.checked_sub(0x80) {
        None => char::from(b),
        Some(i) => char::from_u32(u32::from(HIGH[usize::from(i)])).unwrap_or('\u{FFFD}'),
    }
}
