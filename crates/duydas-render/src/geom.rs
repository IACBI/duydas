//! Geometry: layout rectangles, integer pixel rectangles, corner radii, and the fixed-point form
//! the rasterizers work in.

/// Fixed-point units per pixel. Geometry is snapped to 1/64 px when a command is recorded, so
/// moving it by whole pixels is exact and rasterization does not depend on where a shape is.
pub(crate) const UNITS: i64 = 64;

/// Geometry farther than this from the origin, in pixels, is not drawn. Within it every fixed
/// point sum the renderer forms fits in an i64 with room to spare, so nothing saturates.
pub(crate) const MAX_COORD: f64 = 1.0e9;

/// A rectangle in pixels. Fractional edges are anti-aliased.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width; a negative width draws nothing.
    pub width: f32,
    /// Height; a negative height draws nothing.
    pub height: f32,
}

impl Rect {
    /// A rectangle from its top-left corner and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The fixed-point form, or None for non-finite, negative, empty or out-of-range geometry
    /// (beyond ±[`MAX_COORD`] pixels).
    pub(crate) fn to_fixed(self) -> Option<FRect> {
        // f64 holds every f32 exactly, so the far edge loses nothing to the addition.
        let [x, y, width, height] = [self.x, self.y, self.width, self.height].map(f64::from);
        let (x1, y1) = (x + width, y + height);
        let in_range = [x, y, x1, y1]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORD);
        if !in_range || !(width >= 0.0 && height >= 0.0) {
            return None;
        }
        let rect = FRect {
            x0: fixed(x),
            y0: fixed(y),
            x1: fixed_sum(x, width),
            y1: fixed_sum(y, height),
        };
        (!rect.is_empty()).then_some(rect)
    }
}

/// Converts pixels to fixed point, rounding half up so that values a whole pixel apart snap a
/// whole pixel apart. Exact for any f32 input in the drawable range; `as` saturates far
/// outside it.
pub(crate) fn fixed(value: f64) -> i64 {
    (value * UNITS as f64 + 0.5).floor() as i64
}

/// [`fixed`] of the exact sum a + b. The f64 sum of two f32 values can round onto a snapping
/// tie, which would make the far edge depend on where the shape is; the rounding error, which
/// Knuth's two-sum recovers exactly, decides which side of the tie the true sum lies on.
fn fixed_sum(a: f64, b: f64) -> i64 {
    let sum = a + b;
    let b_part = sum - a;
    let error = (a - (sum - b_part)) + (b - b_part);
    // Exact: scaling by 64 and adding a half lose nothing in the drawable range.
    let shifted = sum * UNITS as f64 + 0.5;
    let floor = shifted.floor();
    let below_tie = floor == shifted && error < 0.0;
    floor as i64 - i64::from(below_tie)
}

/// Corner radii in pixels, clockwise from the top-left. Each is clamped to half the shorter side
/// of the rectangle it rounds; negative or non-finite radii count as 0.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radii {
    /// Top-left corner.
    pub top_left: f32,
    /// Top-right corner.
    pub top_right: f32,
    /// Bottom-right corner.
    pub bottom_right: f32,
    /// Bottom-left corner.
    pub bottom_left: f32,
}

impl Radii {
    /// Square corners.
    pub const ZERO: Self = Self::all(0.0);

    /// The same radius on every corner.
    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// [top-left, top-right, bottom-right, bottom-left], with invalid radii as 0.
    pub(crate) fn sanitized(self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .map(|r| if r.is_finite() && r > 0.0 { r } else { 0.0 })
    }
}

/// A rectangle of whole pixels covering columns `x0..x1` and rows `y0..y1`. It is empty when
/// either range is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IRect {
    /// First column.
    pub x0: i32,
    /// First row.
    pub y0: i32,
    /// One past the last column.
    pub x1: i32,
    /// One past the last row.
    pub y1: i32,
}

impl IRect {
    /// No pixels.
    pub const EMPTY: Self = Self::new(0, 0, 0, 0);

    /// A rectangle from its edges.
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// A rectangle from its top-left corner and size.
    pub const fn from_size(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self::new(
            x,
            y,
            x.saturating_add_unsigned(width),
            y.saturating_add_unsigned(height),
        )
    }

    /// Width in pixels (0 when empty).
    pub const fn width(&self) -> u32 {
        if self.x1 > self.x0 {
            self.x1.abs_diff(self.x0)
        } else {
            0
        }
    }

    /// Height in pixels (0 when empty).
    pub const fn height(&self) -> u32 {
        if self.y1 > self.y0 {
            self.y1.abs_diff(self.y0)
        } else {
            0
        }
    }

    /// Whether the rectangle covers no pixels.
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Number of pixels.
    pub const fn area(&self) -> u64 {
        self.width() as u64 * self.height() as u64
    }

    /// The pixels in both rectangles (possibly empty).
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        Self::new(
            self.x0.max(other.x0),
            self.y0.max(other.y0),
            self.x1.min(other.x1),
            self.y1.min(other.y1),
        )
    }

    /// Whether the rectangles share a pixel.
    pub fn intersects(&self, other: &Self) -> bool {
        !self.intersect(other).is_empty()
    }

    /// The smallest rectangle containing both (an empty side is ignored).
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            *other
        } else if other.is_empty() {
            *self
        } else {
            Self::new(
                self.x0.min(other.x0),
                self.y0.min(other.y0),
                self.x1.max(other.x1),
                self.y1.max(other.y1),
            )
        }
    }

    /// Whether every pixel of `other` is in this rectangle.
    pub fn contains(&self, other: &Self) -> bool {
        other.is_empty()
            || (self.x0 <= other.x0
                && self.y0 <= other.y0
                && self.x1 >= other.x1
                && self.y1 >= other.y1)
    }

    /// The rectangle moved by (dx, dy), saturating at the i32 range.
    #[must_use]
    pub const fn translate(&self, dx: i32, dy: i32) -> Self {
        Self::new(
            self.x0.saturating_add(dx),
            self.y0.saturating_add(dy),
            self.x1.saturating_add(dx),
            self.y1.saturating_add(dy),
        )
    }
}

/// A rectangle of whole pixels with room for any scroll translation, so moving it is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LRect {
    pub(crate) x0: i64,
    pub(crate) y0: i64,
    pub(crate) x1: i64,
    pub(crate) y1: i64,
}

impl LRect {
    pub(crate) fn from_size(x: i64, y: i64, width: u32, height: u32) -> Self {
        Self {
            x0: x,
            y0: y,
            x1: x + i64::from(width),
            y1: y + i64::from(height),
        }
    }

    pub(crate) const fn translate(&self, dy: i64) -> Self {
        Self {
            y0: self.y0 + dy,
            y1: self.y1 + dy,
            ..*self
        }
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub(crate) fn intersect(&self, other: &Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// The same pixels where they fit in an IRect; clamping only drops pixels far outside any
    /// canvas.
    pub(crate) fn clamped(&self) -> IRect {
        let c = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        IRect::new(c(self.x0), c(self.y0), c(self.x1), c(self.y1))
    }
}

impl From<IRect> for LRect {
    fn from(r: IRect) -> Self {
        Self {
            x0: r.x0.into(),
            y0: r.y0.into(),
            x1: r.x1.into(),
            y1: r.y1.into(),
        }
    }
}

/// A rectangle in 1/64 px.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FRect {
    pub(crate) x0: i64,
    pub(crate) y0: i64,
    pub(crate) x1: i64,
    pub(crate) y1: i64,
}

impl FRect {
    pub(crate) const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// The pixels the rectangle touches. Pixels outside get no coverage from any shape inside it.
    pub(crate) const fn pixel_bounds(&self) -> LRect {
        LRect {
            x0: self.x0.div_euclid(UNITS),
            y0: self.y0.div_euclid(UNITS),
            x1: ceil_px(self.x1),
            y1: ceil_px(self.y1),
        }
    }

    /// Moved down by dy whole pixels; exact for any translation a scene can produce.
    pub(crate) const fn translate(&self, dy: i64) -> Self {
        let dy = dy * UNITS;
        Self {
            y0: self.y0 + dy,
            y1: self.y1 + dy,
            ..*self
        }
    }

    /// Shrunk by d units on every side.
    pub(crate) const fn inset(&self, d: i64) -> Self {
        Self {
            x0: self.x0.saturating_add(d),
            y0: self.y0.saturating_add(d),
            x1: self.x1.saturating_sub(d),
            y1: self.y1.saturating_sub(d),
        }
    }
}

const fn ceil_px(value: i64) -> i64 {
    value.div_euclid(UNITS) + (value.rem_euclid(UNITS) != 0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_bounds_cover_exactly_the_touched_pixels() {
        let r = Rect::new(-0.5, 1.0, 2.0, 0.25).to_fixed().expect("valid");
        assert_eq!(r.pixel_bounds().clamped(), IRect::new(-1, 1, 2, 2));
        let whole = Rect::new(3.0, 4.0, 5.0, 6.0).to_fixed().expect("valid");
        assert_eq!(whole.pixel_bounds().clamped(), IRect::new(3, 4, 8, 10));
    }

    #[test]
    fn invalid_geometry_is_rejected() {
        for r in [
            Rect::new(f32::NAN, 0.0, 1.0, 1.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 1.0),
            Rect::new(0.0, 0.0, -1.0, 1.0),
            Rect::new(0.0, 0.0, 1.0, f32::NAN),
            Rect::new(0.0, 0.0, 0.0, 5.0),
            Rect::new(0.0, 0.0, 2.0e9, 1.0),
            Rect::new(0.0, -1.5e9, 1.0, 1.0),
        ] {
            assert_eq!(r.to_fixed(), None, "{r:?}");
        }
        assert!(Rect::new(-1.0e9, 0.0, 1.0e9, 1.0).to_fixed().is_some());
    }

    #[test]
    fn snapping_is_translation_consistent_at_ties() {
        // Half-unit ties round up, so values a pixel apart stay a pixel apart.
        for v in [
            -1.0_f64 / 128.0,
            127.0 / 128.0,
            -0.5 / 64.0,
            2.5 / 64.0,
            131_071.0 + 1.0 / 64.0,
        ] {
            assert_eq!(fixed(v + 1.0) - fixed(v), UNITS, "{v}");
        }
        // A sum just below a tie stays below it wherever the rectangle is.
        let height = f32::from_bits(0x3F01_FFFF);
        let near = Rect::new(0.0, 0.0, 1.0, height).to_fixed().expect("valid");
        let far = Rect::new(0.0, 536_870_912.0, 1.0, height)
            .to_fixed()
            .expect("valid");
        assert_eq!(far.y1 - far.y0, near.y1 - near.y0);
        // The far edge of a thin rectangle far from the origin survives.
        let thin = Rect::new(0.0, -33_554_432.0, 1.0, 1.0)
            .to_fixed()
            .expect("valid");
        assert_eq!(thin.y1 - thin.y0, UNITS);
    }

    #[test]
    fn rect_set_operations() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 15, 15);
        assert_eq!(a.intersect(&b), IRect::new(5, 5, 10, 10));
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert!(a.intersect(&IRect::new(10, 0, 20, 10)).is_empty());
        assert_eq!(a.union(&IRect::EMPTY), a);
        assert!(a.contains(&IRect::new(2, 2, 3, 3)));
        assert_eq!(IRect::new(5, 0, 1, 1).area(), 0);
        assert_eq!(IRect::new(0, 0, i32::MAX, 1).translate(10, 0).x1, i32::MAX);
        let wide = LRect::from(IRect::new(i32::MIN, 0, i32::MAX, 1)).translate(i64::from(i32::MAX));
        assert_eq!(
            wide.clamped(),
            IRect::new(i32::MIN, i32::MAX, i32::MAX, i32::MAX)
        );
    }
}
