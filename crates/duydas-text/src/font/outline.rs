//! Glyph outlines: the sink that receives them, bounds, and affine transforms.

/// Receives a glyph outline in font units, y up.
///
/// Every contour starts with `move_to` and ends with `close`, which implies a straight line
/// back to the contour's start. TrueType outlines arrive as lines and quadratic curves, CFF
/// outlines as lines and cubic curves.
pub trait OutlineSink {
    /// Starts a contour at a point.
    fn move_to(&mut self, x: f32, y: f32);
    /// A straight line to a point.
    fn line_to(&mut self, x: f32, y: f32);
    /// A quadratic Bézier curve with one control point.
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32);
    /// A cubic Bézier curve with two control points.
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    /// Closes the current contour.
    fn close(&mut self);
}

/// The control box of an outline in font units: the smallest box holding every point and
/// control point. It contains the outline, and equals its ink box when no control point sticks
/// out of a curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Left edge.
    pub x_min: f32,
    /// Bottom edge.
    pub y_min: f32,
    /// Right edge.
    pub x_max: f32,
    /// Top edge.
    pub y_max: f32,
}

/// Collects the control box of an outline.
#[derive(Debug, Default)]
pub(crate) struct BoundsSink {
    pub(crate) bounds: Option<Bounds>,
}

impl BoundsSink {
    fn add(&mut self, x: f32, y: f32) {
        let b = self.bounds.get_or_insert(Bounds {
            x_min: x,
            y_min: y,
            x_max: x,
            y_max: y,
        });
        b.x_min = b.x_min.min(x);
        b.y_min = b.y_min.min(y);
        b.x_max = b.x_max.max(x);
        b.y_max = b.y_max.max(y);
    }
}

impl OutlineSink for BoundsSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.add(x1, y1);
        self.add(x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.add(x1, y1);
        self.add(x2, y2);
        self.add(x, y);
    }

    fn close(&mut self) {}
}

/// An affine transform: `x' = a·x + c·y + e`, `y' = b·x + d·y + f` (the OpenType component
/// convention).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transform {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) c: f32,
    pub(crate) d: f32,
    pub(crate) e: f32,
    pub(crate) f: f32,
}

impl Transform {
    pub(crate) const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub(crate) fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

/// Feeds transformed points to a sink.
pub(crate) struct Pen<'a> {
    pub(crate) transform: Transform,
    pub(crate) sink: &'a mut dyn OutlineSink,
}

impl Pen<'_> {
    pub(crate) fn move_to(&mut self, (x, y): (f32, f32)) {
        let (x, y) = self.transform.apply(x, y);
        self.sink.move_to(x, y);
    }

    pub(crate) fn line_to(&mut self, (x, y): (f32, f32)) {
        let (x, y) = self.transform.apply(x, y);
        self.sink.line_to(x, y);
    }

    pub(crate) fn quad_to(&mut self, (x1, y1): (f32, f32), (x, y): (f32, f32)) {
        let (x1, y1) = self.transform.apply(x1, y1);
        let (x, y) = self.transform.apply(x, y);
        self.sink.quad_to(x1, y1, x, y);
    }

    pub(crate) fn close(&mut self) {
        self.sink.close();
    }
}

/// An [`OutlineSink`] that transforms points before passing them on.
pub(crate) struct Transformed<'a> {
    pub(crate) transform: Transform,
    pub(crate) sink: &'a mut dyn OutlineSink,
}

impl OutlineSink for Transformed<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.transform.apply(x, y);
        self.sink.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.transform.apply(x, y);
        self.sink.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.transform.apply(x1, y1);
        let (x, y) = self.transform.apply(x, y);
        self.sink.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.transform.apply(x1, y1);
        let (x2, y2) = self.transform.apply(x2, y2);
        let (x, y) = self.transform.apply(x, y);
        self.sink.curve_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.sink.close();
    }
}
