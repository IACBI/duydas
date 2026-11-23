//! Outline rasterization: exact-area coverage accumulation over flattened outlines.

use super::outline::OutlineSink;
use super::{Font, FontError, GlyphId};

/// The largest size in pixels per em that [`Rasterizer::rasterize`] accepts.
pub const MAX_GLYPH_SIZE: f32 = 4096.0;
/// Width or height limit of a glyph image.
const MAX_DIMENSION: f32 = 8192.0;
/// How far from the pen, in pixels, a glyph image may reach.
const MAX_OFFSET: f32 = 65_536.0;
/// Pixel limit of a glyph image, bounding the memory one glyph can take.
const MAX_PIXELS: usize = 1 << 22;
/// Line segments one glyph may flatten into.
const MAX_SEGMENTS: usize = 1 << 20;
/// How far, in pixels, a flattened curve may stray from the true one.
pub(super) const TOLERANCE: f32 = 1.0 / 32.0;
/// Pieces one curve may be cut into. A curve that needs more strays far past the image limits.
const MAX_PIECES: f32 = 4096.0;
/// Rows and columns that the segments of one glyph may cross in total, plus one per segment
/// flattened, which bounds the time flattening and filling take: a glyph that reuses a tall component many times could otherwise stay within
/// the segment and pixel limits and still take billions of steps. Real glyphs cross well under
/// a million even at the largest size.
const MAX_WORK: f64 = (1u32 << 24) as f64;

/// An anti-aliased glyph: one coverage byte per pixel, row by row, 0 empty and 255 covered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphImage {
    /// Pixels from the pen position to the image's left edge.
    pub left: i32,
    /// Pixels from the baseline to the image's top edge, downwards: negative for anything
    /// above the baseline.
    pub top: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width × height` coverage values.
    pub data: Vec<u8>,
}

/// Turns glyph outlines into [`GlyphImage`]s.
///
/// Curves are flattened to within 1/32 px. Each pixel gets the signed area the outline covers
/// in it, made positive and capped at full coverage, as font-rs and stb_truetype do. That is
/// exact for contours that do not overlap, which is how fonts draw glyphs, and fills the
/// inside of overlapping ones; only an edge pixel that two overlapping contours share can come
/// out darker or lighter than the non-zero winding rule gives. There is no hinting: glyphs keep
/// their designed shapes and are positioned at fractional pixels, as on high-density displays.
///
/// The rasterizer keeps its buffers between glyphs, so reuse one instead of creating one per
/// glyph.
#[derive(Debug, Default)]
pub struct Rasterizer {
    segments: Vec<Segment>,
    cells: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rasterizer {
    /// A rasterizer with empty buffers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rasterizes a glyph at `size` pixels per em, with the pen at `(offset_x, offset_y)`
    /// within its pixel (y down). Pass the fractional part of the pen position there, and add
    /// the integer part to the image's `left` and `top`.
    ///
    /// Returns `None` for a glyph without ink, such as a space. Fails with
    /// [`FontError::InvalidSize`] for a size that is not positive and at most
    /// [`MAX_GLYPH_SIZE`] or a non-finite offset, with [`FontError::TooLarge`] if the image
    /// would exceed 8,192 pixels a side or 4,194,304 pixels or take too many steps to fill, and with the font's error if the
    /// outline cannot be read.
    pub fn rasterize(
        &mut self,
        font: &Font,
        glyph: GlyphId,
        size: f32,
        offset_x: f32,
        offset_y: f32,
    ) -> Result<Option<GlyphImage>, FontError> {
        if !(size > 0.0 && size <= MAX_GLYPH_SIZE && offset_x.is_finite() && offset_y.is_finite()) {
            return Err(FontError::InvalidSize);
        }
        self.segments.clear();
        let mut builder = Builder {
            segments: &mut self.segments,
            scale: size / f32::from(font.metrics().units_per_em),
            dx: offset_x,
            dy: offset_y,
            start: (0.0, 0.0),
            current: (0.0, 0.0),
            open: false,
            overflow: false,
            work: 0.0,
        };
        font.outline(glyph, &mut builder)?;
        builder.close();
        if builder.overflow {
            return Err(FontError::TooLarge);
        }
        self.fill()
    }

    /// The segments a glyph flattens into, in pixels, as `[x0, y0, x1, y1]`.
    #[cfg(test)]
    pub(super) fn flatten(
        &mut self,
        font: &Font,
        glyph: GlyphId,
        size: f32,
    ) -> Result<Vec<[f32; 4]>, FontError> {
        let _ = self.rasterize(font, glyph, size, 0.0, 0.0)?;
        Ok(self
            .segments
            .iter()
            .map(|s| [s.x0, s.y0, s.x1, s.y1])
            .collect())
    }

    fn fill(&mut self) -> Result<Option<GlyphImage>, FontError> {
        let Some(first) = self.segments.first() else {
            return Ok(None);
        };
        let (mut x_min, mut y_min) = (first.x0.min(first.x1), first.y0.min(first.y1));
        let (mut x_max, mut y_max) = (first.x0.max(first.x1), first.y0.max(first.y1));
        for s in &self.segments {
            x_min = x_min.min(s.x0.min(s.x1));
            x_max = x_max.max(s.x0.max(s.x1));
            y_min = y_min.min(s.y0.min(s.y1));
            y_max = y_max.max(s.y0.max(s.y1));
        }
        let (left, right) = (x_min.floor(), x_max.ceil());
        let (top, bottom) = (y_min.floor(), y_max.ceil());
        // Also rejects NaN and infinities. Within 2¹⁶ px of the origin, f32 pixel coordinates
        // round by at most 1/256 px, small next to the flattening tolerance.
        let fits = |a: f32, b: f32| {
            (b - a) >= 0.0
                && (b - a) <= MAX_DIMENSION
                && a.abs() <= MAX_OFFSET
                && b.abs() <= MAX_OFFSET
        };
        if !fits(left, right) || !fits(top, bottom) {
            return Err(FontError::TooLarge);
        }
        let width = (right - left) as usize;
        let height = (bottom - top) as usize;
        if width == 0 || height == 0 {
            return Ok(None);
        }
        if width * height > MAX_PIXELS {
            return Err(FontError::TooLarge);
        }
        // Two spare cells per row take the carry past the right edge.
        let stride = width + 2;
        self.cells.clear();
        self.cells.resize(stride * height, 0.0);
        for s in &self.segments {
            accumulate(
                &mut self.cells,
                stride,
                width as f32,
                height,
                (s.x0 - left, s.y0 - top),
                (s.x1 - left, s.y1 - top),
            );
        }
        let mut data = Vec::with_capacity(width * height);
        for row in self.cells.chunks_exact(stride) {
            let mut sum = 0.0f32;
            data.extend(row[..width].iter().map(|&cell| {
                sum += cell;
                (sum.abs().min(1.0) * 255.0 + 0.5) as u8
            }));
        }
        Ok(Some(GlyphImage {
            left: left as i32,
            top: top as i32,
            width: width as u32,
            height: height as u32,
            data,
        }))
    }
}

/// Adds one line's signed area to the cells it crosses. A cell holds the change in coverage
/// from the cell before it, so a running sum along the row gives each pixel's coverage.
fn accumulate(
    cells: &mut [f32],
    stride: usize,
    width: f32,
    height: usize,
    (x0, y0): (f32, f32),
    (x1, y1): (f32, f32),
) {
    if y0 == y1 {
        return;
    }
    let (direction, (x0, y0), (x1, y1)) = if y0 < y1 {
        (1.0, (x0, y0), (x1, y1))
    } else {
        (-1.0, (x1, y1), (x0, y0))
    };
    // In f64: for a nearly horizontal line the slope overflows f32, and infinity times a zero
    // height would put NaN in the cells.
    let dxdy = f64::from(x1 - x0) / f64::from(y1 - y0);
    let y_start = y0.max(0.0);
    let y_end = y1.min(height as f32);
    let mut x = f64::from(x0) + f64::from(y_start - y0) * dxdy;
    let first_row = y_start as usize;
    let last_row = (y_end.ceil() as usize).min(height);
    for row in first_row..last_row {
        let top = (row as f32).max(y_start);
        let bottom = ((row + 1) as f32).min(y_end);
        if bottom <= top {
            continue;
        }
        let dy = bottom - top;
        let x_next = x + dxdy * f64::from(dy);
        let d = dy * direction;
        let clamp = |v: f64| (v as f32).clamp(0.0, width);
        let (xa, xb) = if x < x_next {
            (clamp(x), clamp(x_next))
        } else {
            (clamp(x_next), clamp(x))
        };
        let line = &mut cells[row * stride..(row + 1) * stride];
        let xa_floor = xa.floor();
        let ia = xa_floor as usize;
        let xb_ceil = xb.ceil();
        let ib = xb_ceil as usize;
        if ib <= ia + 1 {
            // The line stays within one pixel column of this row.
            let mid = 0.5 * (xa + xb) - xa_floor;
            line[ia] += d * (1.0 - mid);
            line[ia + 1] += d * mid;
        } else {
            let s = 1.0 / (xb - xa);
            let fa = xa - xa_floor;
            let a0 = 0.5 * s * (1.0 - fa) * (1.0 - fa);
            let fb = xb - xb_ceil + 1.0;
            let am = 0.5 * s * fb * fb;
            line[ia] += d * a0;
            if ib == ia + 2 {
                line[ia + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - fa);
                line[ia + 1] += d * (a1 - a0);
                for cell in &mut line[ia + 2..ib - 1] {
                    *cell += d * s;
                }
                let a2 = a1 + (ib - ia - 3) as f32 * s;
                line[ib - 1] += d * (1.0 - a2 - am);
            }
            line[ib] += d * am;
        }
        x = x_next;
    }
}

/// Maps an outline to pixels (y down) and flattens its curves into segments.
struct Builder<'a> {
    segments: &'a mut Vec<Segment>,
    scale: f32,
    dx: f32,
    dy: f32,
    start: (f32, f32),
    current: (f32, f32),
    open: bool,
    overflow: bool,
    /// Rows and columns crossed so far.
    work: f64,
}

impl Builder<'_> {
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.scale + self.dx, -y * self.scale + self.dy)
    }

    fn segment(&mut self, to: (f32, f32)) {
        let from = self.current;
        self.current = to;
        if !(to.0.is_finite() && to.1.is_finite()) {
            self.overflow = true;
            return;
        }
        // Every piece counts, including horizontal ones that are then dropped, so flattening
        // many flat curves is bounded too.
        self.work += 1.0;
        // Horizontal segments add no area.
        if from.1 != to.1 {
            self.work += f64::from((to.0 - from.0).abs()) + f64::from((to.1 - from.1).abs()) + 1.0;
        }
        if self.work > MAX_WORK || self.segments.len() >= MAX_SEGMENTS {
            self.overflow = true;
            return;
        }
        if from.1 == to.1 {
            return;
        }
        self.segments.push(Segment {
            x0: from.0,
            y0: from.1,
            x1: to.0,
            y1: to.1,
        });
    }

    /// How many pieces keep a curve with this second difference within the tolerance. The
    /// chord of a piece of length `h` strays at most `h² · |B''| / 8` from the curve.
    ///
    /// `None` when the curve would need more than [`MAX_PIECES`]: its control points are so
    /// far out that the glyph exceeds the image limits anyway.
    fn pieces(second_difference: f32, factor: f32) -> Option<u32> {
        let n = (factor * second_difference / TOLERANCE).sqrt().ceil();
        (n <= MAX_PIECES).then(|| (n as u32).max(1))
    }
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

impl OutlineSink for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = self.map(x, y);
        self.start = p;
        self.current = p;
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.segment(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.current, self.map(x1, y1), self.map(x, y));
        // |B''| = 2·|p0 − 2p1 + p2|
        let dd = length(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1);
        let Some(n) = Self::pieces(dd, 0.25) else {
            self.overflow = true;
            return;
        };
        for i in 1..=n {
            if self.overflow {
                return;
            }
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c) = (u * u, 2.0 * u * t, t * t);
            self.segment((
                a * p0.0 + b * p1.0 + c * p2.0,
                a * p0.1 + b * p1.1 + c * p2.1,
            ));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (
            self.current,
            self.map(x1, y1),
            self.map(x2, y2),
            self.map(x, y),
        );
        // |B''| ≤ 6·max of the two second differences.
        let dd = length(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1)
            .max(length(p1.0 - 2.0 * p2.0 + p3.0, p1.1 - 2.0 * p2.1 + p3.1));
        let Some(n) = Self::pieces(dd, 0.75) else {
            self.overflow = true;
            return;
        };
        for i in 1..=n {
            if self.overflow {
                return;
            }
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.segment((
                a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
                a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
            ));
        }
    }

    fn close(&mut self) {
        if self.open {
            self.segment(self.start);
            self.open = false;
        }
    }
}
