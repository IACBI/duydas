//! CFF outlines (the `CFF ` table): Type 2 charstrings, in plain and CID-keyed fonts.

use super::FontError;
use super::outline::{OutlineSink, Transform, Transformed};
use super::reader::{Reader, Span, u8_at, u16_at};

const MAX_STACK: usize = 48;
const MAX_SUBR_DEPTH: u8 = 10;
/// Operators one glyph may execute, so nested subroutines cannot make a lookup arbitrarily
/// expensive.
const MAX_OPERATORS: u32 = 1 << 20;
/// Font DICTs a CID-keyed font may have.
const MAX_FONT_DICTS: u32 = 256;
/// Bytes of Private DICTs one font may have parsed, so many font DICTs pointing at one huge
/// Private DICT cannot make parsing expensive. Real ones take a few hundred bytes each.
const MAX_PRIVATE_BYTES: usize = 1 << 20;

const MALFORMED: FontError = FontError::Malformed("CFF");

/// A CFF INDEX: a count, an offset array and the data the offsets point into. Positions are
/// relative to the start of the CFF table.
#[derive(Clone, Copy, Debug, Default)]
struct Index {
    count: u32,
    off_size: u8,
    offsets: usize,
    /// The position offset 1 points to, minus one (offsets start at 1).
    base: usize,
    /// The first byte after the INDEX.
    end: usize,
}

impl Index {
    fn parse(cff: &[u8], pos: usize) -> Result<Self, FontError> {
        let mut r = Reader::at(cff, pos).ok_or(MALFORMED)?;
        let count = u32::from(r.u16().ok_or(MALFORMED)?);
        if count == 0 {
            return Ok(Self {
                end: pos + 2,
                ..Self::default()
            });
        }
        let off_size = r.u8().ok_or(MALFORMED)?;
        if !(1..=4).contains(&off_size) {
            return Err(MALFORMED);
        }
        let offsets = pos.checked_add(3).ok_or(MALFORMED)?;
        let array = (count as usize + 1) * usize::from(off_size);
        let base = offsets.checked_add(array - 1).ok_or(MALFORMED)?;
        let mut index = Self {
            count,
            off_size,
            offsets,
            base,
            end: 0,
        };
        let last = index.offset(cff, count).ok_or(MALFORMED)?;
        index.end = base
            .checked_add(last)
            .filter(|&end| last > 0 && end <= cff.len())
            .ok_or(MALFORMED)?;
        Ok(index)
    }

    fn offset(&self, cff: &[u8], i: u32) -> Option<usize> {
        let size = usize::from(self.off_size);
        let at = self.offsets.checked_add((i as usize).checked_mul(size)?)?;
        let bytes = cff.get(at..at.checked_add(size)?)?;
        // Up to four bytes: at most u32::MAX, which fits usize on the targets we build for.
        usize::try_from(bytes.iter().fold(0u32, |v, &b| v << 8 | u32::from(b))).ok()
    }

    fn get<'a>(&self, cff: &'a [u8], i: u32) -> Option<&'a [u8]> {
        if i >= self.count {
            return None;
        }
        let start = self.offset(cff, i)?;
        let end = self.offset(cff, i + 1)?;
        if start == 0 || start > end {
            return None;
        }
        // Each entry must stay inside the INDEX's own data.
        let (start, end) = (self.base.checked_add(start)?, self.base.checked_add(end)?);
        if end > self.end {
            return None;
        }
        cff.get(start..end)
    }
}

/// Calls `op` with each DICT operator and its operands. Two-byte operators are `1200 + b1`.
fn parse_dict(
    bytes: &[u8],
    mut op: impl FnMut(u16, &[f64]) -> Result<(), FontError>,
) -> Result<(), FontError> {
    let mut stack = [0f64; MAX_STACK];
    let mut len = 0;
    let mut r = Reader::new(bytes);
    while r.remaining() > 0 {
        let b0 = r.u8().ok_or(MALFORMED)?;
        let value = match b0 {
            12 => {
                let b1 = r.u8().ok_or(MALFORMED)?;
                op(1200 + u16::from(b1), &stack[..len])?;
                len = 0;
                continue;
            }
            0..=21 => {
                op(u16::from(b0), &stack[..len])?;
                len = 0;
                continue;
            }
            28 => f64::from(r.i16().ok_or(MALFORMED)?),
            29 => f64::from(r.i32().ok_or(MALFORMED)?),
            30 => real(&mut r)?,
            32..=246 => f64::from(i32::from(b0) - 139),
            247..=250 => {
                let b1 = r.u8().ok_or(MALFORMED)?;
                f64::from((i32::from(b0) - 247) * 256 + i32::from(b1) + 108)
            }
            251..=254 => {
                let b1 = r.u8().ok_or(MALFORMED)?;
                f64::from(-(i32::from(b0) - 251) * 256 - i32::from(b1) - 108)
            }
            _ => return Err(MALFORMED),
        };
        *stack.get_mut(len).ok_or(MALFORMED)? = value;
        len += 1;
    }
    Ok(())
}

/// A DICT real number: packed decimal nibbles ending with 0xf.
fn real(r: &mut Reader<'_>) -> Result<f64, FontError> {
    let mut text = String::new();
    loop {
        let byte = r.u8().ok_or(MALFORMED)?;
        for nibble in [byte >> 4, byte & 0xf] {
            match nibble {
                0..=9 => text.push(char::from(b'0' + nibble)),
                0xa => text.push('.'),
                0xb => text.push('E'),
                0xc => text.push_str("E-"),
                0xe => text.push('-'),
                0xf => return Ok(text.parse().unwrap_or(0.0)),
                _ => return Err(MALFORMED),
            }
        }
        if text.len() > 64 {
            return Err(MALFORMED);
        }
    }
}

/// A DICT operand used as an offset or size.
fn to_usize(value: Option<&f64>) -> Result<usize, FontError> {
    match value {
        Some(&v) if (0.0..=f64::from(u32::MAX)).contains(&v) && v.fract() == 0.0 => Ok(v as usize),
        _ => Err(MALFORMED),
    }
}

/// The local subroutines named by a Private DICT, or an empty INDEX. `budget` is the number of
/// Private DICT bytes the font may still parse.
fn private_subrs(
    cff: &[u8],
    size: usize,
    offset: usize,
    budget: &mut usize,
) -> Result<Index, FontError> {
    *budget = budget.checked_sub(size).ok_or(FontError::TooLarge)?;
    let end = offset.checked_add(size).ok_or(MALFORMED)?;
    let dict = cff.get(offset..end).ok_or(MALFORMED)?;
    let mut subrs = None;
    parse_dict(dict, |op, args| {
        if op == 19 {
            subrs = Some(to_usize(args.first())?);
        }
        Ok(())
    })?;
    match subrs {
        Some(relative) => Index::parse(cff, offset.checked_add(relative).ok_or(MALFORMED)?),
        None => Ok(Index::default()),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Cff {
    table: Span,
    charstrings: Index,
    global_subrs: Index,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Plain {
        subrs: Index,
        matrix: Option<Transform>,
    },
    Cid {
        select: FdSelect,
        /// Each font DICT's local subroutines and transform to font units.
        fonts: Vec<(Index, Option<Transform>)>,
    },
}

/// A DICT's FontMatrix, which maps charstring units to ems.
fn font_matrix(args: &[f64]) -> Result<[f64; 6], FontError> {
    match args {
        &[a, b, c, d, e, f] if args.iter().all(|v| v.is_finite()) => Ok([a, b, c, d, e, f]),
        _ => Err(MALFORMED),
    }
}

/// `m` applied after `inner`, both FontMatrix arrays. As in FreeType, `m`'s own
/// translation is dropped: a font DICT with a matrix of its own keeps only its translation,
/// mapped through `m`.
fn concat(m: [f64; 6], inner: [f64; 6]) -> [f64; 6] {
    let [a, b, c, d, _, _] = m;
    let [ia, ib, ic, id, ie, if_] = inner;
    [
        a * ia + c * ib,
        b * ia + d * ib,
        a * ic + c * id,
        b * ic + d * id,
        a * ie + c * if_,
        b * ie + d * if_,
    ]
}

/// The transform from charstring units to font units, or `None` for the identity.
///
/// `matrix` maps charstring units to ems; `top_units` is the top DICT's units per em: the
/// reciprocal of its matrix's vertical scale, or the font's units per em without one. This is
/// FreeType's arithmetic: it normalizes each matrix by its vertical scale and rescales glyphs by
/// the ratio of the top DICT's units per em to the font DICT's, which comes to
/// `matrix × top_units`. So a plain font's matrix keeps only its shape, and a CID font DICT's
/// matrix keeps its scale relative to the top DICT.
fn to_font_units(matrix: [f64; 6], top_units: f64) -> Option<Transform> {
    let t = matrix.map(|v| (v * top_units) as f32);
    let t = Transform {
        a: t[0],
        b: t[1],
        c: t[2],
        d: t[3],
        e: t[4],
        f: t[5],
    };
    let near = |x: f32, y: f32| (x - y).abs() < 1e-6;
    let is_identity = near(t.a, 1.0)
        && near(t.b, 0.0)
        && near(t.c, 0.0)
        && near(t.d, 1.0)
        && near(t.e, 0.0)
        && near(t.f, 0.0);
    let finite = [t.a, t.b, t.c, t.d, t.e, t.f].iter().all(|v| v.is_finite());
    (!is_identity && finite).then_some(t)
}

/// A matrix's vertical scale, by which FreeType normalizes it; `None` if it is degenerate.
fn vertical_scale(m: &[f64; 6]) -> Option<f64> {
    let s = if m[3] != 0.0 { m[3].abs() } else { m[1].abs() };
    (s > 0.0 && s.is_finite()).then_some(s)
}

#[derive(Clone, Copy, Debug)]
enum FdSelect {
    /// One byte per glyph.
    Array(usize),
    /// Ranges of glyphs sharing a font DICT.
    Ranges { start: usize, count: u16 },
}

impl FdSelect {
    fn parse(cff: &[u8], pos: usize, glyphs: u32) -> Result<Self, FontError> {
        match u8_at(cff, pos).ok_or(MALFORMED)? {
            0 if pos + 1 + glyphs as usize <= cff.len() => Ok(Self::Array(pos + 1)),
            3 => {
                let count = u16_at(cff, pos + 1).ok_or(MALFORMED)?;
                let end = pos + 3 + 3 * usize::from(count) + 2;
                if count == 0 || end > cff.len() {
                    return Err(MALFORMED);
                }
                Ok(Self::Ranges {
                    start: pos + 3,
                    count,
                })
            }
            _ => Err(MALFORMED),
        }
    }

    fn get(&self, cff: &[u8], glyph: u16) -> Option<usize> {
        match *self {
            Self::Array(start) => u8_at(cff, start + usize::from(glyph)).map(usize::from),
            Self::Ranges { start, count } => {
                // The last range whose first glyph is at or before `glyph`; the sentinel after
                // the ranges ends the last one.
                let first = |i: usize| u16_at(cff, start + 3 * i);
                let (mut lo, mut hi) = (0, usize::from(count));
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if first(mid)? <= glyph {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                let range = lo.checked_sub(1)?;
                (glyph < first(range + 1)?)
                    .then(|| u8_at(cff, start + 3 * range + 2).map(usize::from))
                    .flatten()
            }
        }
    }
}

impl Cff {
    pub(crate) fn parse(data: &[u8], table: Span, units_per_em: u16) -> Result<Self, FontError> {
        let cff = table.of(data);
        let major = u8_at(cff, 0).ok_or(MALFORMED)?;
        if major != 1 {
            return Err(FontError::Unsupported("CFF version"));
        }
        let header = usize::from(u8_at(cff, 2).ok_or(MALFORMED)?);
        let names = Index::parse(cff, header)?;
        let top = Index::parse(cff, names.end)?;
        let strings = Index::parse(cff, top.end)?;
        let global_subrs = Index::parse(cff, strings.end)?;

        let mut charstrings = None;
        let mut private = None;
        let mut cid = false;
        let mut fd_array = None;
        let mut fd_select = None;
        let mut top_matrix = None;
        parse_dict(top.get(cff, 0).ok_or(MALFORMED)?, |op, args| {
            match op {
                1207 => top_matrix = Some(font_matrix(args)?),
                17 => charstrings = Some(to_usize(args.first())?),
                18 => private = Some((to_usize(args.first())?, to_usize(args.get(1))?)),
                1206 if args.first() != Some(&2.0) => {
                    return Err(FontError::Unsupported("CFF charstring type"));
                }
                1230 => cid = true,
                1236 => fd_array = Some(to_usize(args.first())?),
                1237 => fd_select = Some(to_usize(args.first())?),
                _ => {}
            }
            Ok(())
        })?;
        let top_matrix = match top_matrix {
            Some(m) => Some((m, vertical_scale(&m).ok_or(MALFORMED)?)),
            None => None,
        };
        let top_units = top_matrix.map_or(f64::from(units_per_em), |(_, s)| 1.0 / s);
        let top_matrix = top_matrix.map(|(m, _)| m);
        let charstrings = Index::parse(cff, charstrings.ok_or(MALFORMED)?)?;
        if charstrings.count == 0 {
            return Err(MALFORMED);
        }
        let kind = if cid {
            let fonts = Index::parse(cff, fd_array.ok_or(MALFORMED)?)?;
            if fonts.count == 0 || fonts.count > MAX_FONT_DICTS {
                return Err(MALFORMED);
            }
            let mut budget = MAX_PRIVATE_BYTES;
            // Font DICTs often share a Private DICT; parse each one once.
            let mut parsed: Vec<((usize, usize), Index)> = Vec::new();
            let mut list = Vec::new();
            for i in 0..fonts.count {
                let mut private = None;
                let mut matrix = None;
                parse_dict(fonts.get(cff, i).ok_or(MALFORMED)?, |op, args| {
                    match op {
                        18 => private = Some((to_usize(args.first())?, to_usize(args.get(1))?)),
                        1207 => matrix = Some(font_matrix(args)?),
                        _ => {}
                    }
                    Ok(())
                })?;
                let subrs = match private {
                    Some(key) => match parsed.iter().find(|(k, _)| *k == key) {
                        Some(&(_, subrs)) => subrs,
                        None => {
                            let subrs = private_subrs(cff, key.0, key.1, &mut budget)?;
                            parsed.push((key, subrs));
                            subrs
                        }
                    },
                    None => Index::default(),
                };
                // A font DICT's matrix applies within the top DICT's, which defaults to the
                // identity here; without its own, the font DICT uses the top DICT's.
                let transform = match (matrix, top_matrix) {
                    (Some(m), Some(top)) => to_font_units(concat(top, m), top_units),
                    (Some(m), None) => to_font_units(m, top_units),
                    (None, Some(top)) => to_font_units(top, top_units),
                    (None, None) => None,
                };
                list.push((subrs, transform));
            }
            let select = FdSelect::parse(cff, fd_select.ok_or(MALFORMED)?, charstrings.count)?;
            Kind::Cid {
                select,
                fonts: list,
            }
        } else {
            let subrs = match private {
                Some((size, offset)) => {
                    private_subrs(cff, size, offset, &mut MAX_PRIVATE_BYTES.clone())?
                }
                None => Index::default(),
            };
            Kind::Plain {
                subrs,
                matrix: top_matrix.and_then(|m| to_font_units(m, top_units)),
            }
        };
        Ok(Self {
            table,
            charstrings,
            global_subrs,
            kind,
        })
    }

    /// The number of glyphs the CharStrings INDEX holds.
    pub(crate) fn glyph_count(&self) -> u32 {
        self.charstrings.count
    }

    pub(crate) fn outline(
        &self,
        data: &[u8],
        glyph: u16,
        sink: &mut dyn OutlineSink,
    ) -> Result<(), FontError> {
        let cff = self.table.of(data);
        if u32::from(glyph) >= self.charstrings.count {
            return Err(FontError::NoSuchGlyph);
        }
        let code = self
            .charstrings
            .get(cff, u32::from(glyph))
            .ok_or(MALFORMED)?;
        let (local, matrix) = match &self.kind {
            Kind::Plain { subrs, matrix } => (*subrs, *matrix),
            Kind::Cid { select, fonts } => {
                let fd = select.get(cff, glyph).ok_or(MALFORMED)?;
                *fonts.get(fd).ok_or(MALFORMED)?
            }
        };
        let mut transformed;
        let sink: &mut dyn OutlineSink = match matrix {
            Some(transform) => {
                transformed = Transformed { transform, sink };
                &mut transformed
            }
            None => sink,
        };
        let mut machine = Machine {
            cff,
            global: self.global_subrs,
            local,
            stack: [0.0; MAX_STACK],
            len: 0,
            x: 0.0,
            y: 0.0,
            open: false,
            stems: 0,
            width_parsed: false,
            operators: 0,
            sink,
        };
        // A charstring that ends without endchar ends there, as in FreeType.
        if !machine.run(code, 0)? {
            machine.end_char()?;
        }
        Ok(())
    }
}

/// Subroutine index bias, which lets small indexes reach more subroutines.
fn bias(count: u32) -> i64 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// A Type 2 charstring interpreter.
struct Machine<'a, 's> {
    cff: &'a [u8],
    global: Index,
    local: Index,
    /// Operands and the current point are `f64`: charstring numbers are 16.16 fixed point,
    /// which `f32` cannot hold exactly, and sums of small steps must not stall.
    stack: [f64; MAX_STACK],
    len: usize,
    x: f64,
    y: f64,
    open: bool,
    stems: usize,
    width_parsed: bool,
    operators: u32,
    sink: &'s mut dyn OutlineSink,
}

impl Machine<'_, '_> {
    /// Runs a charstring; `Ok(true)` means `endchar` ended the glyph.
    fn run(&mut self, code: &[u8], depth: u8) -> Result<bool, FontError> {
        let mut r = Reader::new(code);
        while r.remaining() > 0 {
            self.operators += 1;
            if self.operators > MAX_OPERATORS {
                return Err(FontError::TooLarge);
            }
            let b0 = r.u8().ok_or(MALFORMED)?;
            match b0 {
                // hstem, vstem, hstemhm, vstemhm
                1 | 3 | 18 | 23 => {
                    self.width(self.len % 2 == 1);
                    self.stems += self.len / 2;
                    self.len = 0;
                }
                // hintmask, cntrmask: operands before them are implied vstems.
                19 | 20 => {
                    self.width(self.len % 2 == 1);
                    self.stems += self.len / 2;
                    self.len = 0;
                    r.skip(self.stems.div_ceil(8)).ok_or(MALFORMED)?;
                }
                21 => {
                    self.width(self.len > 2);
                    let [dx, dy] = self.top_args::<2>()?;
                    self.move_by(dx, dy);
                }
                22 => {
                    self.width(self.len > 1);
                    let [dx] = self.top_args::<1>()?;
                    self.move_by(dx, 0.0);
                }
                4 => {
                    self.width(self.len > 1);
                    let [dy] = self.top_args::<1>()?;
                    self.move_by(0.0, dy);
                }
                5 => {
                    for i in (0..self.len.saturating_sub(1)).step_by(2) {
                        self.line_by(self.stack[i], self.stack[i + 1]);
                    }
                    self.len = 0;
                }
                6 | 7 => {
                    let mut horizontal = b0 == 6;
                    for i in 0..self.len {
                        let d = self.stack[i];
                        if horizontal {
                            self.line_by(d, 0.0);
                        } else {
                            self.line_by(0.0, d);
                        }
                        horizontal = !horizontal;
                    }
                    self.len = 0;
                }
                8 => {
                    let mut i = 0;
                    while i + 6 <= self.len {
                        self.curve_at(i);
                        i += 6;
                    }
                    self.len = 0;
                }
                // hhcurveto
                27 => {
                    let mut i = 0;
                    let mut dy1 = 0.0;
                    if self.len % 4 == 1 {
                        dy1 = self.stack[0];
                        i = 1;
                    }
                    while i + 4 <= self.len {
                        let s = &self.stack;
                        let (dx1, dx2, dy2, dx3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                        self.curve_by(dx1, dy1, dx2, dy2, dx3, 0.0);
                        dy1 = 0.0;
                        i += 4;
                    }
                    self.len = 0;
                }
                // vvcurveto
                26 => {
                    let mut i = 0;
                    let mut dx1 = 0.0;
                    if self.len % 4 == 1 {
                        dx1 = self.stack[0];
                        i = 1;
                    }
                    while i + 4 <= self.len {
                        let s = &self.stack;
                        let (dy1, dx2, dy2, dy3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                        self.curve_by(dx1, dy1, dx2, dy2, 0.0, dy3);
                        dx1 = 0.0;
                        i += 4;
                    }
                    self.len = 0;
                }
                // vhcurveto, hvcurveto
                30 | 31 => {
                    let mut vertical = b0 == 30;
                    let mut i = 0;
                    while i + 4 <= self.len {
                        let s = &self.stack;
                        let last = if self.len - i == 5 { s[i + 4] } else { 0.0 };
                        let (a, b, c, d) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                        if vertical {
                            self.curve_by(0.0, a, b, c, d, last);
                        } else {
                            self.curve_by(a, 0.0, b, c, last, d);
                        }
                        vertical = !vertical;
                        i += 4;
                    }
                    self.len = 0;
                }
                // rcurveline
                24 => {
                    let mut i = 0;
                    while i + 8 <= self.len {
                        self.curve_at(i);
                        i += 6;
                    }
                    if i + 2 <= self.len {
                        self.line_by(self.stack[i], self.stack[i + 1]);
                    }
                    self.len = 0;
                }
                // rlinecurve
                25 => {
                    let mut i = 0;
                    while i + 8 <= self.len {
                        self.line_by(self.stack[i], self.stack[i + 1]);
                        i += 2;
                    }
                    if i + 6 <= self.len {
                        self.curve_at(i);
                    }
                    self.len = 0;
                }
                // callsubr, callgsubr
                10 | 29 => {
                    let subrs = if b0 == 10 { self.local } else { self.global };
                    let index = self.pop()? as i64 + bias(subrs.count);
                    let code = u32::try_from(index)
                        .ok()
                        .and_then(|i| subrs.get(self.cff, i))
                        .ok_or(MALFORMED)?;
                    if depth >= MAX_SUBR_DEPTH {
                        return Err(FontError::Malformed("CFF: subroutine nesting"));
                    }
                    if self.run(code, depth + 1)? {
                        return Ok(true);
                    }
                }
                // FreeType rejects a return outside a subroutine.
                11 if depth == 0 => return Err(MALFORMED),
                11 => return Ok(false),
                14 => {
                    self.end_char()?;
                    return Ok(true);
                }
                12 => {
                    let b1 = r.u8().ok_or(MALFORMED)?;
                    self.flex(b1)?;
                    self.len = 0;
                }
                28 => self.push(f64::from(r.i16().ok_or(MALFORMED)?))?,
                32..=246 => self.push(f64::from(i16::from(b0) - 139))?,
                247..=250 => {
                    let b1 = i16::from(r.u8().ok_or(MALFORMED)?);
                    self.push(f64::from((i16::from(b0) - 247) * 256 + b1 + 108))?;
                }
                251..=254 => {
                    let b1 = i16::from(r.u8().ok_or(MALFORMED)?);
                    self.push(f64::from(-(i16::from(b0) - 251) * 256 - b1 - 108))?;
                }
                255 => self.push(r.i32().ok_or(MALFORMED)? as f64 / 65536.0)?,
                _ => return Err(MALFORMED),
            }
        }
        Ok(false)
    }

    /// The four flex operators. Hints and the flex depth are ignored: they only matter to a
    /// hinting rasterizer.
    fn flex(&mut self, op: u8) -> Result<(), FontError> {
        match op {
            35 => {
                let a = self.args::<13>()?;
                self.curve_by(a[0], a[1], a[2], a[3], a[4], a[5]);
                self.curve_by(a[6], a[7], a[8], a[9], a[10], a[11]);
            }
            34 => {
                let [dx1, dx2, dy2, dx3, dx4, dx5, dx6] = self.args::<7>()?;
                self.curve_by(dx1, 0.0, dx2, dy2, dx3, 0.0);
                self.curve_by(dx4, 0.0, dx5, -dy2, dx6, 0.0);
            }
            36 => {
                let [dx1, dy1, dx2, dy2, dx3, dx4, dx5, dy5, dx6] = self.args::<9>()?;
                self.curve_by(dx1, dy1, dx2, dy2, dx3, 0.0);
                self.curve_by(dx4, 0.0, dx5, dy5, dx6, -(dy1 + dy2 + dy5));
            }
            37 => {
                let a = self.args::<11>()?;
                let dx: f64 = a[0] + a[2] + a[4] + a[6] + a[8];
                let dy: f64 = a[1] + a[3] + a[5] + a[7] + a[9];
                let (dx6, dy6) = if dx.abs() > dy.abs() {
                    (a[10], -dy)
                } else {
                    (-dx, a[10])
                };
                self.curve_by(a[0], a[1], a[2], a[3], a[4], a[5]);
                self.curve_by(a[6], a[7], a[8], a[9], dx6, dy6);
            }
            // The arithmetic and storage operators of CFF1, which fonts practically never use.
            _ => return Err(FontError::Unsupported("CFF charstring operator")),
        }
        Ok(())
    }

    /// Ends the glyph, for `endchar` or the end of the top-level charstring.
    fn end_char(&mut self) -> Result<(), FontError> {
        self.width(self.len == 1 || self.len == 5);
        if self.len >= 4 {
            // endchar with an accent (seac), deprecated since CFF2.
            return Err(FontError::Unsupported("CFF accented endchar"));
        }
        self.close();
        self.len = 0;
        Ok(())
    }

    /// The first stack-clearing operator may carry the glyph's width as an extra operand. The
    /// width comes from `hmtx` instead, so it is dropped.
    fn width(&mut self, present: bool) {
        if !self.width_parsed {
            self.width_parsed = true;
            if present && self.len > 0 {
                self.stack.copy_within(1..self.len, 0);
                self.len -= 1;
            }
        }
    }

    /// The top `N` operands, as a moveto takes them when there are extra ones below.
    fn top_args<const N: usize>(&mut self) -> Result<[f64; N], FontError> {
        let start = self.len.checked_sub(N).ok_or(MALFORMED)?;
        let mut out = [0.0; N];
        out.copy_from_slice(&self.stack[start..self.len]);
        self.len = 0;
        Ok(out)
    }

    fn args<const N: usize>(&mut self) -> Result<[f64; N], FontError> {
        if self.len < N {
            return Err(MALFORMED);
        }
        let mut out = [0.0; N];
        out.copy_from_slice(&self.stack[..N]);
        self.len = 0;
        Ok(out)
    }

    fn push(&mut self, value: f64) -> Result<(), FontError> {
        *self.stack.get_mut(self.len).ok_or(MALFORMED)? = value;
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<f64, FontError> {
        self.len = self.len.checked_sub(1).ok_or(MALFORMED)?;
        Ok(self.stack[self.len])
    }

    fn close(&mut self) {
        if self.open {
            self.sink.close();
            self.open = false;
        }
    }

    fn move_by(&mut self, dx: f64, dy: f64) {
        self.close();
        self.x += dx;
        self.y += dy;
        self.sink.move_to(self.x as f32, self.y as f32);
        self.open = true;
    }

    /// Drawing before any moveto starts a contour at the current point.
    fn ensure_open(&mut self) {
        if !self.open {
            self.sink.move_to(self.x as f32, self.y as f32);
            self.open = true;
        }
    }

    fn line_by(&mut self, dx: f64, dy: f64) {
        self.ensure_open();
        self.x += dx;
        self.y += dy;
        self.sink.line_to(self.x as f32, self.y as f32);
    }

    fn curve_at(&mut self, i: usize) {
        let s = self.stack;
        self.curve_by(s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]);
    }

    fn curve_by(&mut self, dx1: f64, dy1: f64, dx2: f64, dy2: f64, dx3: f64, dy3: f64) {
        self.ensure_open();
        let (x1, y1) = (self.x + dx1, self.y + dy1);
        let (x2, y2) = (x1 + dx2, y1 + dy2);
        self.x = x2 + dx3;
        self.y = y2 + dy3;
        self.sink.curve_to(
            x1 as f32,
            y1 as f32,
            x2 as f32,
            y2 as f32,
            self.x as f32,
            self.y as f32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::concat;

    #[test]
    fn a_font_dict_matrix_drops_the_top_translation() {
        let top = [0.002, 0.0, 0.0, 0.002, 0.02, 0.0];
        let font_dict = [1.0, 0.0, 0.0, 1.0, 5.0, 0.0];
        let m = concat(top, font_dict);
        let expected = [0.002, 0.0, 0.0, 0.002, 0.01, 0.0];
        assert!(
            m.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-12),
            "{m:?}"
        );
    }
}
