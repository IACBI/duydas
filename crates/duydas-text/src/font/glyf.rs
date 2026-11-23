//! TrueType outlines (the `glyf` and `loca` tables), including composite glyphs.

use super::FontError;
use super::outline::{OutlineSink, Pen, Transform};
use super::reader::{Reader, Span, i16_at, u16_at, u32_at};

/// How deeply composite glyphs may nest.
const MAX_DEPTH: u8 = 8;
/// Points and components one glyph may expand to, so a composite that reuses another many
/// times over cannot make a lookup arbitrarily expensive.
const MAX_POINTS: u32 = 1 << 20;

const MALFORMED: FontError = FontError::Malformed("glyf");

#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyf {
    pub(crate) loca: Span,
    pub(crate) glyf: Span,
    pub(crate) long: bool,
    pub(crate) num_glyphs: u16,
}

impl Glyf {
    /// The glyph's bytes; empty for a glyph without an outline, such as a space.
    pub(super) fn bytes<'a>(&self, data: &'a [u8], glyph: u16) -> Result<&'a [u8], FontError> {
        if glyph >= self.num_glyphs {
            return Err(FontError::NoSuchGlyph);
        }
        let loca = self.loca.of(data);
        let i = usize::from(glyph);
        let range = if self.long {
            u32_at(loca, 4 * i)
                .zip(u32_at(loca, 4 * i + 4))
                .map(|(a, b)| (a as usize, b as usize))
        } else {
            u16_at(loca, 2 * i)
                .zip(u16_at(loca, 2 * i + 2))
                .map(|(a, b)| (2 * usize::from(a), 2 * usize::from(b)))
        };
        let (start, end) = range.ok_or(FontError::Malformed("loca"))?;
        if start > end {
            return Err(FontError::Malformed("loca"));
        }
        self.glyf.of(data).get(start..end).ok_or(MALFORMED)
    }

    /// A simple glyph's boxes around every point (including single-point contours that draw
    /// nothing), as its header should record it, and around its explicit on-curve points, if
    /// it has any.
    #[cfg(test)]
    pub(super) fn boxes(&self, data: &[u8], glyph: u16) -> Option<([f32; 4], Option<[f32; 4]>)> {
        fn grow(b: Option<[f32; 4]>, p: &Point) -> Option<[f32; 4]> {
            Some(b.map_or([p.x, p.y, p.x, p.y], |[a, c, d, e]| {
                [a.min(p.x), c.min(p.y), d.max(p.x), e.max(p.y)]
            }))
        }
        let mut r = Reader::new(self.bytes(data, glyph).ok()?);
        let contours = usize::try_from(r.i16()?).ok().filter(|&n| n > 0)?;
        r.skip(8)?;
        let (_, points) = decode(&mut r, contours, &mut MAX_POINTS.clone()).ok()?;
        let all = points.iter().fold(None, grow)?;
        let on = points.iter().filter(|p| p.on).fold(None, grow);
        Some((all, on))
    }

    /// The left edge of the glyph's box as its header records it, which with the left side
    /// bearing in `hmtx` places the outline; `None` for a glyph without an outline.
    pub(crate) fn x_min(&self, data: &[u8], glyph: u16) -> Option<i16> {
        i16_at(self.bytes(data, glyph).ok()?, 2)
    }

    /// Sends the glyph's outline to `sink`. `lsb` gives a glyph's left side bearing from
    /// `hmtx`.
    ///
    /// TrueType places a glyph's origin the left side bearing to the left of the box its header
    /// records; a composite whose component has USE_MY_METRICS takes that component's origin.
    /// The outline is moved so that the origin is at 0, as FreeType does. In nearly every font
    /// the bearing equals the box's left edge, and nothing moves.
    pub(crate) fn outline(
        &self,
        data: &[u8],
        glyph: u16,
        lsb: &dyn Fn(u16) -> Option<i16>,
        sink: &mut dyn OutlineSink,
    ) -> Result<(), FontError> {
        let mut budget = MAX_POINTS;
        let mut loaded = Loaded::default();
        let mut context = Context {
            data,
            lsb,
            budget: &mut budget,
        };
        let origin = match self.load(&mut context, glyph, 0, &mut loaded)? {
            Some(origin) => Some(origin),
            None => self.origin(data, lsb, glyph),
        };
        let mut pen = Pen {
            transform: Transform {
                e: origin.map_or(0.0, |x| -x),
                ..Transform::IDENTITY
            },
            sink,
        };
        let mut start = 0;
        for &end in &loaded.ends {
            emit(loaded.points.get(start..=end).unwrap_or(&[]), &mut pen);
            start = end + 1;
        }
        Ok(())
    }

    /// A glyph's own origin: its header's left edge minus its left side bearing. An empty
    /// glyph's left edge is 0, as in FreeType.
    fn origin(&self, data: &[u8], lsb: &dyn Fn(u16) -> Option<i16>, glyph: u16) -> Option<f32> {
        let x_min = if self.bytes(data, glyph).ok()?.is_empty() {
            0
        } else {
            self.x_min(data, glyph)?
        };
        Some(f32::from(x_min) - f32::from(lsb(glyph)?))
    }

    /// Appends the glyph's points and contours to `out`. Composites are assembled as
    /// points rather than drawn component by component, because a component may be placed by
    /// matching one of its points to a point of the components before it.
    ///
    /// Returns the origin a USE_MY_METRICS component gives a composite, if any.
    fn load(
        &self,
        cx: &mut Context<'_>,
        glyph: u16,
        depth: u8,
        out: &mut Loaded,
    ) -> Result<Option<f32>, FontError> {
        let bytes = self.bytes(cx.data, glyph)?;
        if bytes.is_empty() {
            return Ok(None);
        }
        let mut r = Reader::new(bytes);
        let contours = r.i16().ok_or(MALFORMED)?;
        r.skip(8).ok_or(MALFORMED)?;
        match usize::try_from(contours) {
            // A glyph may be just a header, without even an instruction length.
            Ok(0) => Ok(None),
            Ok(contours) => {
                let (ends, points) = decode(&mut r, contours, cx.budget)?;
                let base = out.points.len();
                out.ends.extend(ends.iter().map(|&end| base + end));
                out.points.extend(points);
                Ok(None)
            }
            Err(_) => self.composite(cx, &mut r, depth, out),
        }
    }

    fn composite(
        &self,
        cx: &mut Context<'_>,
        r: &mut Reader<'_>,
        depth: u8,
        out: &mut Loaded,
    ) -> Result<Option<f32>, FontError> {
        const WORDS: u16 = 0x0001;
        const XY_VALUES: u16 = 0x0002;
        const SCALE: u16 = 0x0008;
        const MORE: u16 = 0x0020;
        const XY_SCALE: u16 = 0x0040;
        const TWO_BY_TWO: u16 = 0x0080;
        const USE_MY_METRICS: u16 = 0x0200;
        const SCALED_OFFSET: u16 = 0x0800;
        const UNSCALED_OFFSET: u16 = 0x1000;

        if depth >= MAX_DEPTH {
            return Err(FontError::Malformed("glyf: composite nesting"));
        }
        // Point numbers count from this composite's first point.
        let start = out.points.len();
        let mut origin = None;
        loop {
            *cx.budget = cx.budget.checked_sub(1).ok_or(FontError::TooLarge)?;
            let flags = r.u16().ok_or(MALFORMED)?;
            let child = r.u16().ok_or(MALFORMED)?;
            // Offsets are signed; point numbers, used without XY_VALUES, are not.
            let (arg1, arg2): (i32, i32) = match (flags & WORDS != 0, flags & XY_VALUES != 0) {
                (true, true) => (
                    r.i16().ok_or(MALFORMED)?.into(),
                    r.i16().ok_or(MALFORMED)?.into(),
                ),
                (true, false) => (
                    r.u16().ok_or(MALFORMED)?.into(),
                    r.u16().ok_or(MALFORMED)?.into(),
                ),
                (false, true) => (
                    r.i8().ok_or(MALFORMED)?.into(),
                    r.i8().ok_or(MALFORMED)?.into(),
                ),
                (false, false) => (
                    r.u8().ok_or(MALFORMED)?.into(),
                    r.u8().ok_or(MALFORMED)?.into(),
                ),
            };
            let mut f2dot14 = || r.i16().map(|v| f32::from(v) / 16384.0).ok_or(MALFORMED);
            let (a, b, c, d) = if flags & SCALE != 0 {
                let s = f2dot14()?;
                (s, 0.0, 0.0, s)
            } else if flags & XY_SCALE != 0 {
                let (x, y) = (f2dot14()?, f2dot14()?);
                (x, 0.0, 0.0, y)
            } else if flags & TWO_BY_TWO != 0 {
                (f2dot14()?, f2dot14()?, f2dot14()?, f2dot14()?)
            } else {
                (1.0, 0.0, 0.0, 1.0)
            };
            let linear = Transform {
                a,
                b,
                c,
                d,
                e: 0.0,
                f: 0.0,
            };

            let base = out.points.len();
            let child_origin = self.load(cx, child, depth + 1, out)?;
            let (parent, component) = out.points.split_at_mut(base);
            let parent = &parent[start..];
            for p in component.iter_mut() {
                (p.x, p.y) = linear.apply(p.x, p.y);
            }
            let (e, f) = if flags & XY_VALUES != 0 {
                let (dx, dy) = (arg1 as f32, arg2 as f32);
                if flags & SCALED_OFFSET != 0 && flags & UNSCALED_OFFSET == 0 {
                    linear.apply(dx, dy)
                } else {
                    (dx, dy)
                }
            } else {
                // Move the component so its point arg2 lands on the composite's point arg1.
                let point = |points: &[Point], i: i32| {
                    let i = i as usize;
                    match points.get(i) {
                        Some(&p) => Ok(p),
                        // The four phantom points after a glyph's own carry its metrics.
                        None if i < points.len() + 4 => {
                            Err(FontError::Unsupported("glyf: alignment to a phantom point"))
                        }
                        None => Err(MALFORMED),
                    }
                };
                let (to, from) = (point(parent, arg1)?, point(component, arg2)?);
                (to.x - from.x, to.y - from.y)
            };
            if e != 0.0 || f != 0.0 {
                for p in component.iter_mut() {
                    p.x += e;
                    p.y += f;
                }
            }
            if flags & USE_MY_METRICS != 0 {
                // FreeType takes the component's metrics as they are, untransformed.
                origin = child_origin.or_else(|| self.origin(cx.data, cx.lsb, child));
            }
            if flags & MORE == 0 {
                return Ok(origin);
            }
        }
    }
}

/// What loading a glyph needs besides the glyph.
struct Context<'a> {
    data: &'a [u8],
    lsb: &'a dyn Fn(u16) -> Option<i16>,
    /// Points and components the glyph may still expand to.
    budget: &'a mut u32,
}

/// A glyph's points and the index of each contour's last point.
#[derive(Debug, Default)]
struct Loaded {
    points: Vec<Point>,
    ends: Vec<usize>,
}

/// A simple glyph's contour end indexes and points.
fn decode(
    r: &mut Reader<'_>,
    contours: usize,
    budget: &mut u32,
) -> Result<(Vec<usize>, Vec<Point>), FontError> {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const REPEAT: u8 = 0x08;
    const X_SAME_OR_POSITIVE: u8 = 0x10;
    const Y_SAME_OR_POSITIVE: u8 = 0x20;

    let mut ends = Vec::with_capacity(contours);
    for _ in 0..contours {
        let end = usize::from(r.u16().ok_or(MALFORMED)?);
        if ends.last().is_some_and(|&previous| end <= previous) {
            return Err(MALFORMED);
        }
        ends.push(end);
    }
    let points = ends.last().map_or(0, |&end| end + 1);
    *budget = budget
        .checked_sub(u32::try_from(points).map_err(|_| MALFORMED)?)
        .ok_or(FontError::TooLarge)?;
    let instructions = usize::from(r.u16().ok_or(MALFORMED)?);
    r.skip(instructions).ok_or(MALFORMED)?;

    let mut flags = Vec::with_capacity(points);
    while flags.len() < points {
        let flag = r.u8().ok_or(MALFORMED)?;
        flags.push(flag);
        if flag & REPEAT != 0 {
            let repeat = usize::from(r.u8().ok_or(MALFORMED)?);
            if repeat > points - flags.len() {
                return Err(MALFORMED);
            }
            flags.extend(std::iter::repeat_n(flag, repeat));
        }
    }
    let mut coordinate = |short: u8, same: u8| -> Result<Vec<i32>, FontError> {
        let mut value = 0i32;
        let mut out = Vec::with_capacity(points);
        for &flag in &flags {
            let delta = if flag & short != 0 {
                let d = i32::from(r.u8().ok_or(MALFORMED)?);
                if flag & same != 0 { d } else { -d }
            } else if flag & same != 0 {
                0
            } else {
                i32::from(r.i16().ok_or(MALFORMED)?)
            };
            value = value.wrapping_add(delta);
            out.push(value);
        }
        Ok(out)
    };
    let xs = coordinate(X_SHORT, X_SAME_OR_POSITIVE)?;
    let ys = coordinate(Y_SHORT, Y_SAME_OR_POSITIVE)?;

    let points = (0..points)
        .map(|i| Point {
            // In range: each list has one entry per point.
            x: xs.get(i).copied().unwrap_or(0) as f32,
            y: ys.get(i).copied().unwrap_or(0) as f32,
            on: flags.get(i).is_some_and(|f| f & ON_CURVE != 0),
        })
        .collect();
    Ok((ends, points))
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
    on: bool,
}

impl Point {
    fn xy(self) -> (f32, f32) {
        (self.x, self.y)
    }
}

fn mid(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

/// Emits one contour. Two off-curve points in a row imply an on-curve point halfway between
/// them, and a contour may start off the curve.
fn emit(points: &[Point], pen: &mut Pen<'_>) {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return;
    };
    if points.len() < 2 {
        return;
    }
    let (start, rest) = if first.on {
        (first.xy(), &points[1..])
    } else if last.on {
        (last.xy(), &points[..points.len() - 1])
    } else {
        (mid(first.xy(), last.xy()), points)
    };
    pen.move_to(start);
    let mut control: Option<(f32, f32)> = None;
    for p in rest {
        if p.on {
            match control.take() {
                Some(c) => pen.quad_to(c, p.xy()),
                None => pen.line_to(p.xy()),
            }
        } else {
            if let Some(c) = control {
                pen.quad_to(c, mid(c, p.xy()));
            }
            control = Some(p.xy());
        }
    }
    if let Some(c) = control {
        pen.quad_to(c, start);
    }
    pen.close();
}
