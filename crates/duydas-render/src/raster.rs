//! Rasterizers. Each draws one command into the pixels inside a clip rectangle the caller has
//! already intersected with the canvas.
//!
//! Shapes are evaluated relative to the pixel holding their top-left corner, so a shape moved by
//! whole pixels produces exactly the same pixels, moved. Scroll blitting relies on this: a
//! blitted region must be indistinguishable from one drawn from scratch.

use crate::color::{lerp, over, over_coverage};
use crate::geom::{FRect, IRect, LRect, UNITS};
use crate::image::{Image, Mask};

/// A pixel buffer and its row length.
#[derive(Debug)]
pub(crate) struct Pixels<'a> {
    pub(crate) data: &'a mut [u32],
    pub(crate) stride: usize,
}

impl Pixels<'_> {
    /// Columns x0..x1 of row y; the caller keeps them inside the buffer.
    fn row(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
        let start = y as usize * self.stride;
        &mut self.data[start + x0 as usize..start + x1 as usize]
    }
}

/// A rounded rectangle prepared for coverage queries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    rect: FRect,
    /// Clamped radii in fixed point: top-left, top-right, bottom-right, bottom-left.
    radii: [i64; 4],
    /// The pixel holding the top-left corner; everything below is relative to it.
    ox: i64,
    oy: i64,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    r: [f64; 4],
}

impl Shape {
    pub(crate) fn new(rect: FRect, radii: [i64; 4]) -> Self {
        let half = (rect.x1 - rect.x0).min(rect.y1 - rect.y0) / 2;
        let radii = radii.map(|r| r.clamp(0, half.max(0)));
        let ox = rect.x0.div_euclid(UNITS);
        let oy = rect.y0.div_euclid(UNITS);
        // f64 keeps 1/64 px exact for every distance a scene can produce.
        let rel = |v: i64, o: i64| (v - o * UNITS) as f64 / UNITS as f64;
        Self {
            rect,
            radii,
            ox,
            oy,
            x0: rel(rect.x0, ox),
            y0: rel(rect.y0, oy),
            x1: rel(rect.x1, ox),
            y1: rel(rect.y1, oy),
            r: radii.map(|r| r as f64 / UNITS as f64),
        }
    }

    /// The shape shrunk by `width` units with its radii reduced to match: a border's inner
    /// edge. None when nothing is left.
    pub(crate) fn inset(&self, width: i64) -> Option<Self> {
        let rect = self.rect.inset(width);
        (!rect.is_empty())
            .then(|| Self::new(rect, self.radii.map(|r| r.saturating_sub(width).max(0))))
    }

    /// Fraction of pixel (px, py), relative to the origin pixel, the shape covers. Straight
    /// edges get exact area coverage; inside a corner's square the distance to the arc is used,
    /// capped by the area coverage of the enclosing rectangle.
    fn coverage(&self, px: i64, py: i64) -> f64 {
        let (left, top) = (px as f64, py as f64);
        let (cx, cy) = (left + 0.5, top + 0.5);
        let area = overlap(left, self.x0, self.x1) * overlap(top, self.y0, self.y1);
        let [tl, tr, br, bl] = self.r;
        let corner = if tl > 0.0 && cx < self.x0 + tl && cy < self.y0 + tl {
            Some((tl, self.x0 + tl, self.y0 + tl))
        } else if tr > 0.0 && cx > self.x1 - tr && cy < self.y0 + tr {
            Some((tr, self.x1 - tr, self.y0 + tr))
        } else if br > 0.0 && cx > self.x1 - br && cy > self.y1 - br {
            Some((br, self.x1 - br, self.y1 - br))
        } else if bl > 0.0 && cx < self.x0 + bl && cy > self.y1 - bl {
            Some((bl, self.x0 + bl, self.y1 - bl))
        } else {
            None
        };
        match corner {
            Some((r, ccx, ccy)) => {
                let (dx, dy) = (cx - ccx, cy - ccy);
                (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, area)
            }
            None => area,
        }
    }

    /// Relative columns lo..hi of row py that the shape covers completely ((0, 0) if none).
    fn full_span(&self, py: i64) -> (i64, i64) {
        let top = py as f64;
        if top < self.y0 || top + 1.0 > self.y1 {
            return (0, 0);
        }
        let cy = top + 0.5;
        let [tl, tr, br, bl] = self.r;
        let upper = |r: f64| if cy < self.y0 + r { r } else { 0.0 };
        let lower = |r: f64| if cy > self.y1 - r { r } else { 0.0 };
        let left = upper(tl).max(lower(bl));
        let right = upper(tr).max(lower(br));
        let lo = (self.x0 + left).ceil() as i64;
        let hi = (self.x1 - right).floor() as i64;
        if lo < hi { (lo, hi) } else { (0, 0) }
    }

    /// The fully covered columns of absolute row y, clamped to clip; lo == hi when there are
    /// none.
    fn full_columns(&self, y: i32, clip: IRect) -> (i32, i32) {
        let (lo, hi) = self.full_span(i64::from(y) - self.oy);
        let (x0, x1) = (i64::from(clip.x0), i64::from(clip.x1));
        // Clamped into the clip, so the narrowing is lossless.
        let lo = (lo + self.ox).clamp(x0, x1);
        let hi = (hi + self.ox).clamp(lo, x1);
        (lo as i32, hi as i32)
    }

    /// Coverage of absolute pixel (x, y).
    fn coverage_at(&self, x: i32, y: i32) -> f64 {
        self.coverage(i64::from(x) - self.ox, i64::from(y) - self.oy)
    }
}

/// Length of [p, p + 1] ∩ [a, b], clamped to 0..1.
fn overlap(p: f64, a: f64, b: f64) -> f64 {
    ((p + 1.0).min(b) - p.max(a)).clamp(0.0, 1.0)
}

fn coverage8(c: f64) -> u32 {
    (c * 255.0 + 0.5) as u32
}

/// Fills a shape with a premultiplied color.
pub(crate) fn fill(px: &mut Pixels<'_>, clip: IRect, shape: &Shape, color: u32) {
    let opaque = color >> 24 == 255;
    for y in clip.y0..clip.y1 {
        let (lo, hi) = shape.full_columns(y, clip);
        let row = px.row(y, clip.x0, clip.x1);
        for x in (clip.x0..lo).chain(hi..clip.x1) {
            let c = coverage8(shape.coverage_at(x, y));
            let p = &mut row[(x - clip.x0) as usize];
            *p = over_coverage(*p, color, c);
        }
        let span = &mut row[(lo - clip.x0) as usize..(hi - clip.x0) as usize];
        if opaque {
            span.fill(color & 0x00FF_FFFF);
        } else {
            for p in span {
                *p = over(*p, color);
            }
        }
    }
}

/// Fills the ring between two shapes (a border).
pub(crate) fn stroke(px: &mut Pixels<'_>, clip: IRect, outer: &Shape, inner: &Shape, color: u32) {
    for y in clip.y0..clip.y1 {
        // Where the inner shape covers fully, the ring covers nothing.
        let (lo, hi) = inner.full_columns(y, clip);
        let row = px.row(y, clip.x0, clip.x1);
        for x in (clip.x0..lo).chain(hi..clip.x1) {
            let c = outer.coverage_at(x, y) - inner.coverage_at(x, y);
            let p = &mut row[(x - clip.x0) as usize];
            *p = over_coverage(*p, color, coverage8(c.max(0.0)));
        }
    }
}

/// Fills a whole-pixel area (the clip) with a premultiplied color.
pub(crate) fn clear(px: &mut Pixels<'_>, clip: IRect, color: u32) {
    for y in clip.y0..clip.y1 {
        let row = px.row(y, clip.x0, clip.x1);
        if color >> 24 == 255 {
            row.fill(color & 0x00FF_FFFF);
        } else {
            for p in row {
                *p = over(*p, color);
            }
        }
    }
}

/// Blends a premultiplied color through a coverage mask whose top-left pixel is at (x, y).
pub(crate) fn mask(px: &mut Pixels<'_>, clip: IRect, mask: &Mask, x: i64, y: i64, color: u32) {
    let area = LRect::from_size(x, y, mask.width(), mask.height())
        .intersect(&LRect::from(clip))
        .clamped();
    if area.is_empty() {
        return;
    }
    let stride = mask.width() as usize;
    let width = area.width() as usize;
    // area lies inside the mask, so these offsets are in 0..width and 0..height.
    let dx = (i64::from(area.x0) - x) as usize;
    for row in area.y0..area.y1 {
        let start = (i64::from(row) - y) as usize * stride + dx;
        let src = &mask.data()[start..start + width];
        let dst = px.row(row, area.x0, area.x1);
        for (d, &c) in dst.iter_mut().zip(src) {
            *d = over_coverage(*d, color, u32::from(c));
        }
    }
}

/// Draws an image stretched over a shape (bilinear filtering, clamped at the edges) and
/// clipped to its rounded corners.
pub(crate) fn image(px: &mut Pixels<'_>, clip: IRect, image: &Image, shape: &Shape) {
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    let src = image.pixels();
    let r = shape.rect;
    let native = r.x0.rem_euclid(UNITS) == 0
        && r.y0.rem_euclid(UNITS) == 0
        && r.x1 - r.x0 == iw as i64 * UNITS
        && r.y1 - r.y0 == ih as i64 * UNITS;
    if native {
        // One image pixel per canvas pixel: blend texels directly, through the corner
        // coverage when there is any. clip lies inside the image.
        let dx = (i64::from(clip.x0) - shape.ox) as usize;
        let square = shape.radii == [0; 4];
        for y in clip.y0..clip.y1 {
            let start = (i64::from(y) - shape.oy) as usize * iw + dx;
            let texels = &src[start..start + clip.width() as usize];
            let (lo, hi) = shape.full_columns(y, clip);
            let row = px.row(y, clip.x0, clip.x1);
            for (x, (d, &p)) in (clip.x0..).zip(row.iter_mut().zip(texels)) {
                let c = if square || (lo..hi).contains(&x) {
                    255
                } else {
                    coverage8(shape.coverage_at(x, y))
                };
                *d = over_coverage(*d, p, c);
            }
        }
        return;
    }
    let sx = iw as f64 / (shape.x1 - shape.x0);
    let sy = ih as f64 / (shape.y1 - shape.y0);
    for y in clip.y0..clip.y1 {
        let ry = i64::from(y) - shape.oy;
        let (r0, r1, fy) = taps((ry as f64 + 0.5 - shape.y0) * sy - 0.5, ih);
        let (top_row, bottom_row) = (&src[r0 * iw..(r0 + 1) * iw], &src[r1 * iw..(r1 + 1) * iw]);
        let (lo, hi) = shape.full_columns(y, clip);
        let row = px.row(y, clip.x0, clip.x1);
        for x in clip.x0..clip.x1 {
            let rx = i64::from(x) - shape.ox;
            let c = if (lo..hi).contains(&x) {
                255
            } else {
                coverage8(shape.coverage(rx, ry))
            };
            if c == 0 {
                continue;
            }
            let (c0, c1, fx) = taps((rx as f64 + 0.5 - shape.x0) * sx - 0.5, iw);
            let top = lerp(top_row[c0], top_row[c1], fx);
            let bottom = lerp(bottom_row[c0], bottom_row[c1], fx);
            let p = &mut row[(x - clip.x0) as usize];
            *p = over_coverage(*p, lerp(top, bottom, fy), c);
        }
    }
}

/// The two source indices around sample position u and the weight of the second (0..=256).
fn taps(u: f64, n: usize) -> (usize, usize, u32) {
    let floor = u.floor();
    let i = floor as i64;
    let last = n as i64 - 1;
    let weight = (((u - floor) * 256.0 + 0.5) as u32).min(256);
    (
        i.clamp(0, last) as usize,
        (i + 1).clamp(0, last) as usize,
        weight,
    )
}
