//! Paragraph layout (ADR 0029): text broken into lines of positioned glyphs, with the caret,
//! hit testing and selection over them.
//!
//! Layout takes two steps:
//!
//! - [`ShapedText::new`] does the work that does not depend on the width. It resolves the
//!   text's embedding levels (UAX #9) and splits it into runs of one direction, script and font,
//!   each picked by [`itemize_with`] for its direction. It then
//!   shapes the runs and finds the line break opportunities (UAX #14).
//! - [`ShapedText::layout`] breaks the lines for a width, reorders each line for display
//!   (rules L1 and L2), and places its glyphs. It reuses the glyphs of whole runs, and shapes
//!   again, on every call, only the pieces of the runs that its breaks cut.
//!
//! Lines break at the last opportunity that fits. Spaces at the end of a line hang past the
//! width, as do characters that draw nothing (default ignorables such as U+200B). A word wider than the line breaks between grapheme clusters, as CSS
//! `overflow-wrap: break-word` does. A run cut by a line break is shaped again for each line, so
//! every line draws what shaping its own text gives; a ligature split by the break comes apart.
//! Tabs advance to the next tab stop. Newlines and other mandatory breaks end lines.
//!
//! Positions are in pixels, y down, with the layout's top left at the origin.
//!
//! ```no_run
//! use duydas_text::font::Font;
//! use duydas_text::layout::{Align, Position, ShapedText, TextOptions};
//!
//! # fn fonts() -> (Font, Font) { unimplemented!() }
//! let (latin, japanese) = fonts();
//! let text = ShapedText::new("Merhaba, 世界!", &[&latin, &japanese], &TextOptions::default());
//! let layout = text.layout(Some(240.0), Align::Start);
//! for glyph in layout.glyphs() {
//!     // Draw glyph.glyph of font glyph.font at (glyph.x, glyph.y).
//! #   let _ = glyph;
//! }
//! let caret = layout.caret(Position::new(4));
//! # let _ = caret;
//! ```

mod cursor;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::ops::Range;

use crate::bidi::{Bidi, BidiClass, Direction, Lines, Paragraph, bidi_class, visual_order};
use crate::font::{Font, GlyphId, MAX_GLYPH_SIZE};
use crate::shaping::{Feature, Language, Script, ShapeOptions, ShapedGlyph, itemize_with, shape};
use crate::shaping_tables::{DEFAULT_IGNORABLE, char_info};
use crate::{Strictness, line_breaks};

pub use cursor::{Affinity, Caret, Position, Rect};

/// The tolerance, in pixels, of the test that a line fits, so that rounding in a sum of
/// advances does not push a line that fits exactly onto two.
const EPSILON: f32 = 1.0 / 64.0;
/// How many times a line moves its break back when its glyphs, shaped again at the break,
/// come out wider than measured (kerning with the line before, say). Past that, a shorter line
/// that fits is found by bisection, so a line is shaped again at most this many times plus the
/// logarithm of its length.
const MAX_BACKOFF: usize = 16;
/// The tallest line height that [`LineHeight::Relative`] and [`LineHeight::Absolute`] give,
/// in pixels, so that the lines' positions stay finite.
pub const MAX_LINE_HEIGHT: f32 = 65_536.0;

/// The height of a line, as in CSS: the space beyond a font's ascender and descender is
/// split equally above and below them. A height that is not a positive finite number counts
/// as 0, and one above [`MAX_LINE_HEIGHT`] as that.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    /// The fonts' own: ascender, descender and line gap.
    #[default]
    Normal,
    /// A multiple of the font size, such as 1.5.
    Relative(f32),
    /// A height in pixels.
    Absolute(f32),
}

/// Where lines sit across the layout's width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// At the paragraph's start: left in left-to-right paragraphs, right in right-to-left ones.
    #[default]
    Start,
    /// At the paragraph's end.
    End,
    /// At the left.
    Left,
    /// At the right.
    Right,
    /// Centred.
    Center,
}

/// How to shape and lay out text.
#[derive(Clone, Copy, Debug)]
pub struct TextOptions<'a> {
    /// The font size in pixels per em, at most [`MAX_GLYPH_SIZE`]: a larger size lays out as
    /// that, and anything but a positive number as 0.
    pub size: f32,
    /// Every paragraph's direction. Without one, each paragraph takes the direction of its first
    /// strong character (UAX #9 rules P2 and P3).
    pub direction: Option<Direction>,
    /// The text's language, which picks fonts' language-specific forms.
    pub language: Language,
    /// Feature settings on top of the defaults (see [`ShapeOptions::features`]).
    pub features: &'a [Feature],
    /// The line breaking rules for Japanese small kana.
    pub strictness: Strictness,
    /// The height of a line.
    pub line_height: LineHeight,
    /// The distance between tab stops, in widths of the first font's space; with 0, tabs take
    /// no room.
    pub tab_size: u16,
}

impl Default for TextOptions<'_> {
    fn default() -> Self {
        Self {
            size: 16.0,
            direction: None,
            language: Language::default(),
            features: &[],
            strictness: Strictness::default(),
            line_height: LineHeight::default(),
            tab_size: 8,
        }
    }
}

/// A glyph placed in a layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    /// The index of the glyph's font in the list given to [`ShapedText::new`].
    pub font: usize,
    /// The glyph to draw.
    pub glyph: GlyphId,
    /// Where to draw the glyph's origin: x from the layout's left edge.
    pub x: f32,
    /// Where to draw the glyph's origin: y from the layout's top edge, down, at the line's
    /// baseline unless the glyph is raised or lowered.
    pub y: f32,
    /// The byte offset in the text of the start of the glyph's cluster.
    pub cluster: usize,
}

/// A line of a layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The line's bytes in the text, with the spaces and the line break that end it.
    pub range: Range<usize>,
    /// The line's glyphs in [`Layout::glyphs`], in display order from left to right (where a
    /// font moves the pen back, a glyph's origin can be left of the one before).
    pub glyphs: Range<usize>,
    /// The direction of the line's paragraph; for a line holding the end of one paragraph and
    /// the start of the next (after U+001C to U+001E), of the last.
    pub direction: Direction,
    /// The left edge of the line's content: its glyphs but the spaces at its end, which hang
    /// past it on the paragraph's end side.
    pub x: f32,
    /// The width of the line's content.
    pub width: f32,
    /// The top of the line.
    pub top: f32,
    /// The line's height.
    pub height: f32,
    /// The baseline's distance from the layout's top.
    pub baseline: f32,
}

/// Text shaped for layout: its levels, runs and glyphs, ready to break into lines at any
/// width with [`ShapedText::layout`]. It keeps a copy of the text and of the fonts (which
/// share their data), so it can be kept with the text it shows.
#[derive(Clone, Debug)]
pub struct ShapedText {
    text: Box<str>,
    fonts: Vec<Font>,
    size: f32,
    language: Language,
    features: Box<[Feature]>,
    line_height: LineHeight,
    /// The tab stop interval and the space's advance, in pixels.
    tab: f32,
    space: f32,
    paragraphs: Vec<Paragraph>,
    levels: Vec<u8>,
    items: Vec<Item>,
    glyphs: Vec<ShapedGlyph>,
    graphemes: Vec<Grapheme>,
    /// The graphemes between mandatory breaks, each after a line break or at the end; the last
    /// is empty when the text ends with a line break, or is empty.
    hard_lines: Vec<Range<usize>>,
}

/// What a grapheme cluster is to line breaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    /// Whitespace (Bidi_Class WS), which hangs at the end of a line.
    Space,
    Tab,
    /// A mandatory line break: a newline, CR LF, or another character of Line_Break BK, CR,
    /// LF or NL.
    Break,
}

#[derive(Clone, Copy, Debug)]
struct Grapheme {
    start: usize,
    /// The width the grapheme takes in its run's glyphs, in pixels; a ligature's width is
    /// split equally between the graphemes it draws.
    advance: f32,
    kind: Kind,
    /// A line may break before this grapheme.
    wrap: bool,
    /// Not made only of default-ignorable characters, which draw nothing.
    visible: bool,
}

impl Grapheme {
    /// Whitespace, a tab, a line break or a grapheme that draws nothing: at the end of a line
    /// it hangs past the width, and it never makes a line overflow.
    fn hangs(self) -> bool {
        self.kind != Kind::Text || !self.visible
    }
}

/// A shaped run: bytes of one embedding level, script and font, with no tab or line break.
#[derive(Clone, Debug)]
struct Item {
    range: Range<usize>,
    font: usize,
    script: Script,
    rtl: bool,
    glyphs: Range<usize>,
}

/// A grapheme's place in a layout: its horizontal extent and line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cell {
    start: usize,
    left: f32,
    right: f32,
    rtl: bool,
    kind: Kind,
    line: usize,
}

/// Text broken into lines and placed. It has at least one line: empty text has one empty line.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    lines: Vec<Line>,
    glyphs: Vec<PlacedGlyph>,
    cells: Vec<Cell>,
    /// Each line's cells.
    line_cells: Vec<Range<usize>>,
    /// The cells that take room, but line breaks, from left to right: in `line_order` for
    /// each line. They follow each other without overlapping.
    order: Vec<usize>,
    line_order: Vec<Range<usize>>,
    len: usize,
    width: f32,
    height: f32,
}

impl ShapedText {
    /// Shapes `text` with `fonts`, the first font of the list that covers each grapheme
    /// cluster drawing it (see [`itemize_with`]). The first font
    /// also sets the smallest line height and the tab stops.
    pub fn new(text: &str, fonts: &[&Font], options: &TextOptions<'_>) -> Self {
        let size = positive(options.size).min(MAX_GLYPH_SIZE);
        let mut graphemes: Vec<Grapheme> = Vec::new();
        let mut start = 0;
        for grapheme in crate::graphemes(text) {
            graphemes.push(Grapheme {
                start,
                advance: 0.0,
                kind: kind(grapheme),
                wrap: false,
                visible: !grapheme
                    .chars()
                    .all(|c| char_info(c) & DEFAULT_IGNORABLE != 0),
            });
            start += grapheme.len();
        }
        // Opportunities inside a grapheme cluster (a mark after a space, LB10) are not taken.
        let mut next = 0;
        for opportunity in line_breaks(text, options.strictness) {
            if opportunity.mandatory {
                continue;
            }
            next += graphemes[next..].partition_point(|g| g.start < opportunity.index);
            if let Some(g) = graphemes.get_mut(next)
                && g.start == opportunity.index
            {
                g.wrap = true;
            }
        }
        let mut hard_lines = Vec::new();
        let mut first = 0;
        for (i, g) in graphemes.iter().enumerate() {
            if g.kind == Kind::Break {
                hard_lines.push(first..i + 1);
                first = i + 1;
            }
        }
        hard_lines.push(first..graphemes.len());

        let bidi = Bidi::new(text, options.direction);
        let mut items = Vec::new();
        let mut glyphs = Vec::new();
        let byte = |i: usize| graphemes.get(i).map_or(text.len(), |g| g.start);
        for hard in &hard_lines {
            let bytes = byte(hard.start)..byte(hard.end);
            if bytes.is_empty() {
                continue;
            }
            // Rule L1 as at the end of the line: only a soft break can change it.
            let levels = bidi.line_levels(bytes.clone());
            let mut segment: Option<(Range<usize>, u8)> = None;
            let mut flush = |segment: &mut Option<(Range<usize>, u8)>| {
                if let Some((range, level)) = segment.take() {
                    let run = Run {
                        text,
                        fonts,
                        language: options.language,
                        features: options.features,
                    };
                    run.shape_segment(range, level, &mut items, &mut glyphs);
                }
            };
            for i in hard.clone() {
                let g = graphemes[i];
                if matches!(g.kind, Kind::Tab | Kind::Break) {
                    flush(&mut segment);
                    continue;
                }
                // A grapheme takes the level of its first character, so no run splits it.
                let level = levels[g.start - bytes.start];
                let end = byte(i + 1);
                match &mut segment {
                    Some((range, l)) if *l == level => range.end = end,
                    _ => {
                        flush(&mut segment);
                        segment = Some((g.start..end, level));
                    }
                }
            }
            flush(&mut segment);
        }

        let scale = |f: &Font| size / f32::from(f.metrics().units_per_em.max(1));
        for item in &items {
            let a = graphemes.partition_point(|g| g.start < item.range.start);
            let b = graphemes.partition_point(|g| g.start < item.range.end);
            let mut extents = vec![(0.0, 0.0); b - a];
            measure(
                &glyphs[item.glyphs.clone()],
                scale(fonts[item.font]),
                &graphemes[a..b],
                item.range.end,
                item.rtl,
                (0.0, 0.0),
                &mut extents,
            );
            for (g, (left, right)) in graphemes[a..b].iter_mut().zip(extents) {
                g.advance = right - left;
            }
        }
        let space = fonts.first().map_or(size / 4.0, |f| {
            f.glyph_index(' ')
                .and_then(|g| f.advance(g))
                .map_or(size / 4.0, |a| f32::from(a) * scale(f))
        });
        let (paragraphs, levels) = bidi.into_parts();
        Self {
            text: text.into(),
            fonts: fonts.iter().map(|&f| f.clone()).collect(),
            size,
            language: options.language,
            features: options.features.into(),
            line_height: options.line_height,
            tab: space * f32::from(options.tab_size),
            space,
            paragraphs,
            levels,
            items,
            glyphs,
            graphemes,
            hard_lines,
        }
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Breaks the text into lines no wider than `max_width` pixels, or only at line breaks
    /// without one, and places them. `align` places each line across `max_width`, or across
    /// the widest line without one. A width that is not a positive number counts as 0: every
    /// grapheme cluster that takes room gets a line of its own, though whitespace still hangs.
    pub fn layout(&self, max_width: Option<f32>, align: Align) -> Layout {
        let max = max_width
            .map(|w| if w > 0.0 { w } else { 0.0 })
            .filter(|w| w.is_finite());
        let mut layout = Layout {
            lines: Vec::new(),
            glyphs: Vec::new(),
            cells: Vec::with_capacity(self.graphemes.len()),
            line_cells: Vec::new(),
            order: Vec::new(),
            line_order: Vec::new(),
            len: self.text.len(),
            width: 0.0,
            height: 0.0,
        };
        let fits = |line: &Built| max.is_none_or(|max| line.width <= max + EPSILON);
        let mut top = 0.0;
        for hard in &self.hard_lines {
            let mut start = hard.start;
            loop {
                let (mut end, mut earlier) = self.find_break(start, hard.end, max);
                let mut line = self.build_line(start..end);
                let mut tries = 0;
                while !fits(&line)
                    && tries < MAX_BACKOFF
                    && let Some(back) = self.back_off(start, end, &mut earlier)
                {
                    end = back;
                    line = self.build_line(start..end);
                    tries += 1;
                }
                let first = (start..end).find(|&k| !self.graphemes[k].hangs());
                if !fits(&line)
                    && let Some(first) = first
                    && self.graphemes[first + 1..end].iter().any(|g| !g.hangs())
                {
                    // Every shorter line tried came out too wide as well (a ligature that only
                    // forms over the whole line, say): bisect for one that fits, keeping the
                    // first grapheme that takes room at the least. Shaping makes the width grow
                    // unevenly with the length, so it is not always the longest that would.
                    let (mut fit, mut wide) = (first + 1, end);
                    while wide - fit > 1 {
                        let mid = fit + (wide - fit) / 2;
                        if fits(&self.build_line(start..mid)) {
                            fit = mid;
                        } else {
                            wide = mid;
                        }
                    }
                    // What hangs after the break stays on the line.
                    end = fit;
                    while end < hard.end && self.graphemes[end].hangs() {
                        end += 1;
                    }
                    line = self.build_line(start..end);
                }
                self.place(&mut layout, line, &mut top);
                start = end;
                if start >= hard.end {
                    break;
                }
            }
        }
        layout.height = top;
        layout.width = layout.lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let width = max.unwrap_or(layout.width);
        for (line, cells) in layout.lines.iter_mut().zip(&layout.line_cells) {
            let slack = width - line.width;
            let target = match (align, line.direction) {
                (Align::Left, _)
                | (Align::Start, Direction::LeftToRight)
                | (Align::End, Direction::RightToLeft) => 0.0,
                (Align::Right, _)
                | (Align::Start, Direction::RightToLeft)
                | (Align::End, Direction::LeftToRight) => slack,
                (Align::Center, _) => slack / 2.0,
            };
            let shift = target - line.x;
            line.x = target;
            for g in &mut layout.glyphs[line.glyphs.clone()] {
                g.x += shift;
            }
            for c in &mut layout.cells[cells.clone()] {
                c.left += shift;
                c.right += shift;
            }
        }
        // The cells that take room, in order on screen, from their final places: a shift can
        // round a narrow cell away.
        for cells in &layout.line_cells {
            let first = layout.order.len();
            let all = &layout.cells;
            layout.order.extend(
                cells
                    .clone()
                    .filter(|&k| all[k].kind != Kind::Break && all[k].right > all[k].left),
            );
            layout.order[first..].sort_by(|&a, &b| all[a].left.total_cmp(&all[b].left));
            layout.line_order.push(first..layout.order.len());
        }
        layout
    }

    /// Where the line starting at grapheme `start` ends, from the graphemes' advances, and the
    /// earlier opportunities, nearest last, for when it comes out too wide once shaped.
    fn find_break(&self, start: usize, end: usize, max: Option<f32>) -> (usize, Vec<usize>) {
        let Some(max) = max else {
            return (end, Vec::new());
        };
        let mut pen = 0.0;
        // Whether the line has a grapheme that does not hang yet: only after one can another
        // overflow.
        let mut content = false;
        let mut opportunities = Vec::new();
        for i in start..end {
            let g = self.graphemes[i];
            if i > start && g.wrap {
                opportunities.push(i);
            }
            let advance = self.advance(g, pen);
            if !g.hangs() && content && pen + advance > max + EPSILON {
                return match opportunities.pop() {
                    Some(at) => {
                        let keep = opportunities.len().saturating_sub(MAX_BACKOFF);
                        (at, opportunities.split_off(keep))
                    }
                    // No opportunity fits: break the word before the grapheme that overflows.
                    None => (
                        i,
                        ((start + 1).max(i.saturating_sub(MAX_BACKOFF))..i).collect(),
                    ),
                };
            }
            content |= !g.hangs();
            pen += advance;
        }
        let keep = opportunities.len().saturating_sub(MAX_BACKOFF);
        (end, opportunities.split_off(keep))
    }

    /// The last place before `end` to break a word on a line from `start`, with no
    /// opportunity left: after a grapheme that does not hang, as one that hangs takes no room
    /// at the end of the line anyway.
    fn split_point(&self, start: usize, end: usize) -> Option<usize> {
        // Only a break before something that takes room narrows the line.
        let last = (start..end).rev().find(|&k| !self.graphemes[k].hangs())?;
        (start + 1..=last)
            .rev()
            .find(|&k| !self.graphemes[k - 1].hangs())
    }

    /// The next place to move a line's break back to, from `end`: the nearest earlier
    /// opportunity left that moves something taking room off the line, or a place to break the
    /// word.
    fn back_off(&self, start: usize, end: usize, earlier: &mut Vec<usize>) -> Option<usize> {
        while let Some(k) = earlier.pop() {
            if self.graphemes[k..end].iter().any(|g| !g.hangs()) {
                return Some(k);
            }
        }
        self.split_point(start, end)
    }

    fn advance(&self, g: Grapheme, pen: f32) -> f32 {
        match g.kind {
            Kind::Tab => self.tab_advance(pen),
            Kind::Break => 0.0,
            Kind::Text | Kind::Space => g.advance,
        }
    }

    /// A tab's advance from `pen`, the distance from the line's start: to the next tab stop
    /// at least half a space away, as in CSS.
    fn tab_advance(&self, pen: f32) -> f32 {
        if self.tab <= 0.0 {
            return 0.0;
        }
        let mut stop = ((pen / self.tab).floor() + 1.0) * self.tab;
        if stop - pen < self.space / 2.0 {
            stop += self.tab;
        }
        stop - pen
    }

    fn byte(&self, grapheme: usize) -> usize {
        self.graphemes
            .get(grapheme)
            .map_or(self.text.len(), |g| g.start)
    }

    fn scale(&self, font: usize) -> f32 {
        self.size / f32::from(self.fonts[font].metrics().units_per_em.max(1))
    }

    /// The direction of the paragraph that `byte` belongs to; the empty line after a final
    /// line break continues the last paragraph.
    fn direction_at(&self, byte: usize) -> Direction {
        let i = self.paragraphs.partition_point(|p| p.range.end <= byte);
        self.paragraphs
            .get(i)
            .or(self.paragraphs.last())
            .map_or(Direction::LeftToRight, Paragraph::direction)
    }

    /// Lays out the graphemes of one line from x = 0, before alignment.
    fn build_line(&self, graphemes: Range<usize>) -> Built {
        let bytes = self.byte(graphemes.start)..self.byte(graphemes.end);
        // A line holding two paragraphs takes the last one's direction, so what hangs at the
        // end of that paragraph is at the end of the line.
        let last = graphemes
            .end
            .checked_sub(1)
            .filter(|&k| k >= graphemes.start);
        let direction = self.direction_at(last.map_or(bytes.start, |k| self.graphemes[k].start));
        let mut built = Built {
            bytes: bytes.clone(),
            graphemes: graphemes.clone(),
            direction,
            glyphs: Vec::new(),
            cells: Vec::new(),
            x: 0.0,
            width: 0.0,
            fonts: Vec::new(),
        };
        if graphemes.is_empty() {
            return built;
        }
        let lines = Lines {
            text: &self.text,
            paragraphs: &self.paragraphs,
            levels: &self.levels,
        };
        let levels = lines.line_levels(bytes.clone());
        let level = |i: usize| levels[self.graphemes[i].start - bytes.start];

        // Runs of one level in display order, each paragraph reordered on its own (rule L2).
        // Paragraphs share a line only after U+001C to U+001E, which end a bidi paragraph
        // without breaking the line; they follow each other in the line's direction, the last
        // paragraph's.
        let mut runs: Vec<(Range<usize>, u8)> = Vec::new();
        let mut groups: Vec<Range<usize>> = Vec::new();
        let mut i = graphemes.start;
        while i < graphemes.end {
            let at = self.graphemes[i].start;
            let paragraph = self.paragraphs.partition_point(|p| p.range.end <= at);
            let paragraph_end = self
                .paragraphs
                .get(paragraph)
                .map_or(bytes.end, |p| p.range.end);
            let end =
                self.graphemes[i..graphemes.end].partition_point(|g| g.start < paragraph_end) + i;
            let mut part: Vec<(Range<usize>, u8)> = Vec::new();
            for k in i..end {
                match part.last_mut() {
                    Some((range, l)) if *l == level(k) => range.end = k + 1,
                    _ => part.push((k..k + 1, level(k))),
                }
            }
            let part_levels: Vec<u8> = part.iter().map(|(_, l)| *l).collect();
            let first = runs.len();
            runs.extend(
                visual_order(&part_levels)
                    .into_iter()
                    .map(|j| part[j].clone()),
            );
            groups.push(first..runs.len());
            i = end;
        }
        if direction == Direction::RightToLeft && groups.len() > 1 {
            runs = groups
                .iter()
                .rev()
                .flat_map(|g| runs[g.clone()].iter().cloned())
                .collect();
        }

        // Pieces in display order: shaped text, or a lone tab, line break or grapheme no font
        // draws.
        let mut pieces: Vec<Piece<'_>> = Vec::new();
        for (run, level) in &runs {
            let rtl = level % 2 == 1;
            let first = pieces.len();
            let mut k = run.start;
            while k < run.end {
                let g = self.graphemes[k];
                let item = self
                    .items
                    .get(self.items.partition_point(|it| it.range.end <= g.start))
                    .filter(|it| it.range.start <= g.start);
                let Some(item) = item.filter(|_| matches!(g.kind, Kind::Text | Kind::Space)) else {
                    pieces.push(Piece::Lone { grapheme: k, rtl });
                    k += 1;
                    continue;
                };
                let end = (k + self.graphemes[k..run.end]
                    .partition_point(|x| x.start < item.range.end))
                .max(k + 1);
                let range = g.start..self.byte(end);
                let glyphs = if range == item.range && item.rtl == rtl {
                    Cow::Borrowed(&self.glyphs[item.glyphs.clone()])
                } else {
                    Cow::Owned(self.reshape(item, range.clone(), rtl))
                };
                pieces.push(Piece::Shaped {
                    font: item.font,
                    glyphs,
                    graphemes: k..end,
                    end: range.end,
                    rtl,
                });
                k = end;
            }
            if rtl {
                pieces[first..].reverse();
            }
        }

        // Widths from each piece's glyphs, then the tabs' advances in logical order:
        // a tab is at the paragraph's level (rule L1), so what comes before it on the line is
        // shown before it in the paragraph's direction.
        let n = graphemes.len();
        let offset = graphemes.start;
        let mut extents = vec![(0.0f32, 0.0f32); n];
        let mut rtl_of = vec![false; n];
        for piece in &pieces {
            match piece {
                Piece::Shaped {
                    font,
                    glyphs,
                    graphemes: range,
                    end,
                    rtl,
                } => {
                    let slots = range.start - offset..range.end - offset;
                    measure(
                        glyphs,
                        self.scale(*font),
                        &self.graphemes[range.clone()],
                        *end,
                        *rtl,
                        (0.0, 0.0),
                        &mut extents[slots.clone()],
                    );
                    rtl_of[slots].fill(*rtl);
                    if !built.fonts.contains(font) {
                        built.fonts.push(*font);
                    }
                }
                Piece::Lone { grapheme, rtl } => rtl_of[grapheme - offset] = *rtl,
            }
        }
        let mut widths: Vec<f32> = extents.iter().map(|(l, r)| r - l).collect();
        let mut pen = 0.0;
        for (k, w) in widths.iter_mut().enumerate() {
            if self.graphemes[offset + k].kind == Kind::Tab {
                *w = self.tab_advance(pen);
            }
            pen += *w;
        }
        // The graphemes before those that hang at the end are the line's content.
        let content = n
            - (0..n)
                .rev()
                .take_while(|&k| self.graphemes[offset + k].hangs())
                .count();

        // The pen as the glyphs are placed, and the right edge of the cells so far: a cluster
        // that moves the pen back takes no room, so cells never overlap. Each piece starts after
        // the cells before it, as the widths above, and so the tab stops, assume; a piece is
        // shaped on its own anyway.
        let mut x: f32 = 0.0;
        let mut edge = 0.0;
        for piece in pieces {
            x = x.max(edge);
            match piece {
                Piece::Shaped {
                    font,
                    glyphs,
                    graphemes: range,
                    end,
                    rtl,
                } => {
                    let scale = self.scale(font);
                    let slots = range.start - offset..range.end - offset;
                    let (after, right) = measure(
                        &glyphs,
                        scale,
                        &self.graphemes[range],
                        end,
                        rtl,
                        (x, edge),
                        &mut extents[slots],
                    );
                    edge = right;
                    for g in glyphs.iter() {
                        built.glyphs.push(PlacedGlyph {
                            font,
                            glyph: g.glyph,
                            x: x + g.x_offset as f32 * scale,
                            // Raised glyphs go up; the baseline is added once it is known.
                            y: -(g.y_offset as f32) * scale,
                            cluster: g.cluster,
                        });
                        x += g.x_advance as f32 * scale;
                    }
                    debug_assert_eq!(x.to_bits(), after.to_bits());
                }
                Piece::Lone { grapheme, .. } => {
                    let left = x;
                    x += widths[grapheme - offset];
                    edge = x;
                    extents[grapheme - offset] = (left, x);
                }
            }
        }
        let (left, right) = extents[..content]
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), e| {
                (l.min(e.0), r.max(e.1))
            });
        (built.x, built.width) = if content > 0 {
            (left, right - left)
        } else {
            match direction {
                Direction::LeftToRight => (0.0, 0.0),
                Direction::RightToLeft => (edge, 0.0),
            }
        };
        built.cells = extents
            .into_iter()
            .zip(rtl_of)
            .enumerate()
            .map(|(k, ((left, right), rtl))| (left, right, rtl, self.graphemes[offset + k].kind))
            .collect();
        built
    }

    /// The glyphs for part of an item, shaped on their own in the direction of the run they
    /// are on.
    fn reshape(&self, item: &Item, range: Range<usize>, rtl: bool) -> Vec<ShapedGlyph> {
        let options = ShapeOptions {
            script: item.script,
            language: self.language,
            features: &self.features,
            direction: if rtl {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            },
        };
        let mut glyphs = shape(&self.fonts[item.font], &self.text[range.clone()], &options);
        for g in &mut glyphs {
            g.cluster += range.start;
        }
        glyphs
    }

    /// Adds a built line below the lines before it.
    fn place(&self, layout: &mut Layout, built: Built, top: &mut f32) {
        let (above, below) = self.line_box(&built.fonts);
        // Rounding the halves of a zero leading can leave a sum just below zero.
        let height = (above + below).max(0.0);
        let baseline = *top + above;
        let first_glyph = layout.glyphs.len();
        layout
            .glyphs
            .extend(built.glyphs.into_iter().map(|g| PlacedGlyph {
                y: g.y + baseline,
                ..g
            }));
        let line = layout.lines.len();
        let first_cell = layout.cells.len();
        for (k, (left, right, rtl, kind)) in built.cells.into_iter().enumerate() {
            layout.cells.push(Cell {
                start: self.graphemes[built.graphemes.start + k].start,
                left,
                right,
                rtl,
                kind,
                line,
            });
        }
        layout.line_cells.push(first_cell..layout.cells.len());
        layout.lines.push(Line {
            range: built.bytes,
            glyphs: first_glyph..layout.glyphs.len(),
            direction: built.direction,
            x: built.x,
            width: built.width,
            top: *top,
            height,
            baseline,
        });
        *top += height;
    }

    /// The extent of a line above and below its baseline: the largest of the first font's
    /// (CSS's strut) and of the fonts on the line.
    fn line_box(&self, fonts: &[usize]) -> (f32, f32) {
        let height = |content: f32| {
            match self.line_height {
                LineHeight::Normal => None,
                LineHeight::Relative(factor) => Some(positive(factor * self.size)),
                LineHeight::Absolute(pixels) => Some(positive(pixels)),
            }
            .map_or(content, |h| h.min(MAX_LINE_HEIGHT))
        };
        if self.fonts.is_empty() {
            let h = height(0.0);
            return (h / 2.0, h / 2.0);
        }
        let mut above = f32::NEG_INFINITY;
        let mut below = f32::NEG_INFINITY;
        for &font in std::iter::once(&0).chain(fonts) {
            let m = self.fonts[font].metrics();
            let scale = self.scale(font);
            // A broken font's ascender below the baseline, or descender above it, gives no
            // room rather than a line of negative height.
            let ascent = (f32::from(m.ascender) * scale).max(0.0);
            let descent = (-f32::from(m.descender) * scale).max(0.0);
            let gap = f32::from(m.line_gap.max(0)) * scale;
            let half_leading = (height(ascent + descent + gap) - ascent - descent) / 2.0;
            above = above.max(ascent + half_leading);
            below = below.max(descent + half_leading);
        }
        (above, below)
    }
}

/// The fonts and settings items are shaped with.
struct Run<'t> {
    text: &'t str,
    fonts: &'t [&'t Font],
    language: Language,
    features: &'t [Feature],
}

impl Run<'_> {
    /// Splits a segment of one level into items by script and font, and shapes them.
    fn shape_segment(
        &self,
        range: Range<usize>,
        level: u8,
        items: &mut Vec<Item>,
        glyphs: &mut Vec<ShapedGlyph>,
    ) {
        let direction = Direction::of_level(level);
        let segment = &self.text[range.clone()];
        for run in itemize_with(segment, self.fonts, direction) {
            let bytes = range.start + run.range.start..range.start + run.range.end;
            let options = ShapeOptions {
                script: run.script,
                language: self.language,
                features: self.features,
                direction,
            };
            let first = glyphs.len();
            glyphs.extend(
                shape(self.fonts[run.font], &self.text[bytes.clone()], &options)
                    .into_iter()
                    .map(|g| ShapedGlyph {
                        cluster: g.cluster + bytes.start,
                        ..g
                    }),
            );
            items.push(Item {
                range: bytes,
                font: run.font,
                script: run.script,
                rtl: direction == Direction::RightToLeft,
                glyphs: first..glyphs.len(),
            });
        }
    }
}

/// A line laid out from x = 0.
struct Built {
    bytes: Range<usize>,
    graphemes: Range<usize>,
    direction: Direction,
    glyphs: Vec<PlacedGlyph>,
    /// Each grapheme's left and right, direction and kind.
    cells: Vec<(f32, f32, bool, Kind)>,
    /// The left edge and the width of the content: the glyphs but what hangs at the end.
    x: f32,
    width: f32,
    fonts: Vec<usize>,
}

enum Piece<'s> {
    Shaped {
        font: usize,
        glyphs: Cow<'s, [ShapedGlyph]>,
        graphemes: Range<usize>,
        /// The byte after the piece.
        end: usize,
        rtl: bool,
    },
    Lone {
        grapheme: usize,
        rtl: bool,
    },
}

/// Each grapheme's extent in a piece's glyphs, and the pen and the right edge after them. The
/// glyphs are placed from `start.0`, and the edges go no further left than `start.1`, the
/// right edge of the cells before: a cluster that moves the pen back takes no room, so cells
/// never overlap. The piece ends at byte `end`. A cluster's width is split equally between
/// its graphemes in the piece's direction, but graphemes made only of default ignorables, which
/// draw nothing, take no share unless the cluster has nothing else; a grapheme with no glyph of
/// its own takes no room. The pen moves as when the glyphs are placed, and neighbouring
/// graphemes share their edge exactly, so positions found from either side agree.
fn measure(
    glyphs: &[ShapedGlyph],
    scale: f32,
    graphemes: &[Grapheme],
    end: usize,
    rtl: bool,
    start: (f32, f32),
    out: &mut [(f32, f32)],
) -> (f32, f32) {
    let mut clusters: Vec<usize> = glyphs.iter().map(|g| g.cluster).collect();
    clusters.sort_unstable();
    clusters.dedup();
    let mut done = vec![false; out.len()];
    let mut x = start.0;
    let first = start.0.max(start.1);
    let mut edge = first;
    let mut i = 0;
    while i < glyphs.len() {
        let cluster = glyphs[i].cluster;
        while i < glyphs.len() && glyphs[i].cluster == cluster {
            x += glyphs[i].x_advance as f32 * scale;
            i += 1;
        }
        let (x0, x1) = (edge, x.max(edge));
        edge = x1;
        let next = clusters
            .get(clusters.partition_point(|&c| c <= cluster))
            .copied()
            .unwrap_or(end);
        let a = graphemes.partition_point(|g| g.start < cluster);
        let b = graphemes.partition_point(|g| g.start < next);
        if a >= b {
            continue;
        }
        let visible = graphemes[a..b].iter().filter(|g| g.visible).count();
        let count = if visible == 0 { b - a } else { visible };
        let share = (x1 - x0) / count as f32;
        // The edge after the cluster's first `n` graphemes that take a share, in logical order.
        let edge_after = |n: usize| match (n, rtl) {
            (0, false) => x0,
            (0, true) => x1,
            (n, false) if n == count => x1,
            (n, true) if n == count => x0,
            // Rounding, at subnormal sizes say, must not put an edge outside the cluster.
            (n, false) => (x0 + n as f32 * share).min(x1),
            (n, true) => (x1 - n as f32 * share).max(x0),
        };
        let mut n = 0;
        for slot in a..b {
            let before = edge_after(n);
            if visible == 0 || graphemes[slot].visible {
                n += 1;
            }
            let after = edge_after(n);
            out[slot] = if rtl {
                (after, before)
            } else {
                (before, after)
            };
            done[slot] = true;
        }
    }
    for k in 0..out.len() {
        if !done[k] {
            let at = match (k.checked_sub(1), rtl) {
                (Some(before), false) => out[before].1,
                (Some(before), true) => out[before].0,
                (None, false) => first,
                (None, true) => edge,
            };
            out[k] = (at, at);
        }
    }
    (x, edge)
}

fn kind(grapheme: &str) -> Kind {
    match grapheme.chars().next() {
        Some('\n' | '\r' | '\u{B}' | '\u{C}' | '\u{85}' | '\u{2028}' | '\u{2029}') => Kind::Break,
        Some('\t') => Kind::Tab,
        _ if grapheme.chars().all(|c| bidi_class(c) == BidiClass::Ws) => Kind::Space,
        _ => Kind::Text,
    }
}

/// `value` if it is a positive finite number, else 0.
fn positive(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

impl Layout {
    /// The lines, top to bottom; there is always at least one.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The glyphs, line by line, each line's from left to right.
    pub fn glyphs(&self) -> &[PlacedGlyph] {
        &self.glyphs
    }

    /// The width of the widest line's content.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The height of the lines together.
    pub fn height(&self) -> f32 {
        self.height
    }
}
