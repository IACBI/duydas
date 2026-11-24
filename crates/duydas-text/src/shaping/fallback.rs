//! Mark positioning for fonts without GPOS, after HarfBuzz's fallback: each mark with a
//! combining class goes above, below or beside its base according to that class, using the
//! glyphs' boxes as HarfBuzz reads them (a TrueType glyph's header box), and marks of the same
//! class stack. The arithmetic wraps at 32 bits, as
//! HarfBuzz's does, so hostile metrics cannot overflow it.
//!
//! Combining classes here are HarfBuzz's modified ones (see `char_info`), which
//! [`recategorize`] turns into placements first.

use std::collections::HashMap;

use crate::font::layout::{GlyphInfo, Limits, Position};
use crate::font::{Font, GlyphId};

/// Combining classes that place marks.
const ATTACHED_BELOW_LEFT: u8 = 200;
const ATTACHED_BELOW: u8 = 202;
const ATTACHED_ABOVE: u8 = 214;
const ATTACHED_ABOVE_RIGHT: u8 = 216;
const BELOW_LEFT: u8 = 218;
const BELOW: u8 = 220;
const BELOW_RIGHT: u8 = 222;
const ABOVE_LEFT: u8 = 228;
const ABOVE: u8 = 230;
const ABOVE_RIGHT: u8 = 232;
const DOUBLE_BELOW: u8 = 233;
const DOUBLE_ABOVE: u8 = 234;

/// The placement of a nonspacing mark `c` with combining class `class` (HarfBuzz's
/// modified class), as HarfBuzz's fallback reads it: Hebrew, Arabic, Syriac, Thai, Lao and
/// Tibetan points, whose classes say only in what order they combine, go above, below or
/// beside their base; Thai and Lao marks without a class get one.
pub(super) fn recategorize(c: char, class: u8) -> u8 {
    if class >= 200 {
        return class;
    }
    let mut class = class;
    if u32::from(c) & !0xFF == 0x0E00 {
        class = match (class, c) {
            (0, '\u{E31}' | '\u{E34}'..='\u{E37}' | '\u{E47}' | '\u{E4C}'..='\u{E4E}') => {
                ABOVE_RIGHT
            }
            (0, '\u{EB1}' | '\u{EB4}'..='\u{EB7}' | '\u{EBB}' | '\u{ECC}' | '\u{ECD}') => ABOVE,
            (0, '\u{EBC}') => BELOW,
            // Thai virama.
            (1.., '\u{E3A}') => BELOW_RIGHT,
            _ => class,
        };
    }
    match class {
        // Hebrew: hataf segol, hataf patah, hataf qamats, tsere, segol, patah, qamats,
        // sheva, hiriq, qubuts and meteg (classes 15 to 25 in HarfBuzz's numbering).
        15..=25 => BELOW,
        // Rafe.
        13 => ATTACHED_ABOVE,
        // Shin dot.
        10 => ABOVE_RIGHT,
        // Sin dot, holam.
        11 | 14 => ABOVE_LEFT,
        // Point varika.
        26 => ABOVE,
        // Arabic and Syriac: fathatan, dammatan, fatha, damma, shadda, sukun, superscript
        // alef and alaph above; kasratan and kasra below.
        28 | 29 | 31 | 32 | 27 | 34 | 35 | 36 => ABOVE,
        30 | 33 => BELOW,
        // Thai sara u and uu, mai.
        3 => BELOW_RIGHT,
        107 => ABOVE_RIGHT,
        // Lao sign u and uu, mai.
        118 => BELOW,
        122 => ABOVE,
        // Tibetan sign aa, sign i, sign u.
        129 => BELOW,
        132 => ABOVE,
        131 => BELOW,
        _ => class,
    }
}

/// A glyph's box as HarfBuzz reports it: left and top edges, width, and height down
/// (negative).
#[derive(Clone, Copy, Debug, Default)]
struct Extents {
    x_bearing: i32,
    y_bearing: i32,
    width: i32,
    height: i32,
}

/// HarfBuzz's extents: a TrueType glyph's header box, or a CFF glyph's outline box.
fn extents(font: &Font, glyph: u16) -> Option<Extents> {
    match font.header_extents(GlyphId(glyph)) {
        Ok(Some([x_bearing, y_bearing, width, height])) => {
            return Some(Extents {
                x_bearing,
                y_bearing,
                width,
                height,
            });
        }
        Ok(None) => {}
        Err(_) => return None,
    }
    match font.bounds(GlyphId(glyph)) {
        Ok(None) => Some(Extents::default()),
        Ok(Some(b)) => {
            let (left, right) = (b.x_min.floor() as i32, b.x_max.ceil() as i32);
            let (bottom, top) = (b.y_min.floor() as i32, b.y_max.ceil() as i32);
            Some(Extents {
                x_bearing: left,
                y_bearing: top,
                width: right.wrapping_sub(left),
                height: bottom.wrapping_sub(top),
            })
        }
        Err(_) => None,
    }
}

/// What positioning needs besides the glyphs: the font, each glyph's ink box once, the
/// shaping budget, a step of which each mark spends, and the directions.
struct Context<'a> {
    font: &'a Font,
    extents: HashMap<u16, Option<Extents>>,
    limits: &'a Limits,
    /// The run is right to left, which puts a ligature's first component on the right.
    rtl: bool,
    /// The glyphs are right-to-left text in logical order.
    backward: bool,
}

impl Context<'_> {
    /// A glyph's box, read the first time it is needed; a long run of marks reads each
    /// distinct glyph once.
    fn extents(&mut self, glyph: u16) -> Option<Extents> {
        let font = self.font;
        *self
            .extents
            .entry(glyph)
            .or_insert_with(|| extents(font, glyph))
    }
}

/// Positions every mark cluster: a base and the marks (by General_Category) after it. When
/// `limits` runs out, the marks left keep their positions. `adjust` moves a mark whose
/// advance is zeroed back over it; `rtl` and `backward` are as in [`Context`].
pub(super) fn position_marks(
    font: &Font,
    info: &[GlyphInfo],
    pos: &mut [Position],
    adjust: bool,
    (rtl, backward): (bool, bool),
    limits: &Limits,
) {
    // Finding the clusters and their bases visits each glyph once more.
    if !limits.spend(info.len()) {
        return;
    }
    let mut cx = Context {
        font,
        extents: HashMap::new(),
        limits,
        rtl,
        backward,
    };
    let mut start = 0;
    for i in 1..info.len() {
        if !info[i].unicode_mark {
            position_cluster(&mut cx, info, pos, start, i, adjust);
            start = i;
        }
    }
    position_cluster(&mut cx, info, pos, start, info.len(), adjust);
}

fn position_cluster(
    cx: &mut Context<'_>,
    info: &[GlyphInfo],
    pos: &mut [Position],
    start: usize,
    end: usize,
    adjust: bool,
) {
    // Only the first glyph can be a base: `position_marks` starts a cluster at each one. A
    // cluster of marks alone, at the start of the text, has none.
    if end - start < 2 || info[start].unicode_mark {
        return;
    }
    position_around_base(cx, info, pos, start, end, adjust);
}

fn zero_mark_advances(
    info: &[GlyphInfo],
    pos: &mut [Position],
    start: usize,
    end: usize,
    adjust: bool,
) {
    for i in start..end {
        if info[i].nonspacing_mark {
            if adjust {
                pos[i].x_offset = pos[i].x_offset.wrapping_sub(pos[i].x_advance);
                pos[i].y_offset = pos[i].y_offset.wrapping_sub(pos[i].y_advance);
            }
            pos[i].x_advance = 0;
            pos[i].y_advance = 0;
        }
    }
}

fn position_around_base(
    cx: &mut Context<'_>,
    info: &[GlyphInfo],
    pos: &mut [Position],
    base: usize,
    end: usize,
    adjust: bool,
) {
    if !cx.limits.spend(end - base) {
        return;
    }
    let font = cx.font;
    let Some(mut base_extents) = cx.extents(info[base].glyph) else {
        zero_mark_advances(info, pos, base + 1, end, adjust);
        return;
    };
    base_extents.y_bearing = base_extents.y_bearing.wrapping_add(pos[base].y_offset);
    // The advance, not the ink, places marks across: it also works for glyphs without ink.
    base_extents.x_bearing = 0;
    base_extents.width = font.advance(GlyphId(info[base].glyph)).map_or(0, i32::from);

    let lig_id = info[base].lig_id();
    let components = i32::from(info[base].components());
    // Marks hang back over the advances before them, left to right; right to left the
    // glyphs come out reversed, so they move forward over the advances after the base.
    let (mut x_offset, mut y_offset) = if cx.backward {
        (0, 0)
    } else {
        (
            pos[base].x_advance.wrapping_neg(),
            pos[base].y_advance.wrapping_neg(),
        )
    };
    let mut component_extents = base_extents;
    let mut last_component = -1;
    let mut last_class: Option<u8> = None;
    let mut cluster_extents = base_extents;
    for i in base + 1..end {
        let class = info[i].combining_class;
        if class == 0 {
            if cx.backward {
                x_offset = x_offset.wrapping_add(pos[i].x_advance);
                y_offset = y_offset.wrapping_add(pos[i].y_advance);
            } else {
                x_offset = x_offset.wrapping_sub(pos[i].x_advance);
                y_offset = y_offset.wrapping_sub(pos[i].y_advance);
            }
            continue;
        }
        if components > 1 {
            // HarfBuzz's signed arithmetic: a mark of another ligature, or past the last
            // component, takes the last; one whose component wrapped to 0 (four bits are
            // kept) stays at -1, a component's width before the ligature (after, right to
            // left).
            let mut component = i32::from(info[i].lig_comp()) - 1;
            if lig_id == 0 || lig_id != info[i].lig_id() || component >= components {
                component = components - 1;
            }
            if last_component != component {
                last_component = component;
                last_class = None;
                component_extents = base_extents;
                let order = if cx.rtl {
                    components - 1 - component
                } else {
                    component
                };
                let share = order
                    .wrapping_mul(component_extents.width)
                    .wrapping_div(components);
                component_extents.x_bearing = component_extents.x_bearing.wrapping_add(share);
                component_extents.width = component_extents.width.wrapping_div(components);
            }
        }
        if last_class != Some(class) {
            last_class = Some(class);
            cluster_extents = component_extents;
        }
        let mark = cx.extents(info[i].glyph);
        position_mark(
            font,
            mark,
            &mut pos[i],
            &mut cluster_extents,
            class,
            cx.backward,
        );
        let p = &mut pos[i];
        p.x_advance = 0;
        p.y_advance = 0;
        p.x_offset = p.x_offset.wrapping_add(x_offset);
        p.y_offset = p.y_offset.wrapping_add(y_offset);
    }
}

fn position_mark(
    font: &Font,
    mark: Option<Extents>,
    pos: &mut Position,
    base: &mut Extents,
    class: u8,
    backward: bool,
) {
    let Some(mark) = mark else {
        return;
    };
    let y_gap = i32::from(font.metrics().units_per_em) / 16;
    pos.y_offset = 0;
    // Left and right marks are not moved across.
    let right = base.x_bearing.wrapping_add(base.width);
    pos.x_offset = match class {
        // A double mark straddles the base's end, its right edge left to right, its left
        // edge right to left.
        DOUBLE_BELOW | DOUBLE_ABOVE if backward => base
            .x_bearing
            .wrapping_sub(mark.width / 2)
            .wrapping_sub(mark.x_bearing),
        DOUBLE_BELOW | DOUBLE_ABOVE => right
            .wrapping_sub(mark.width / 2)
            .wrapping_sub(mark.x_bearing),
        ATTACHED_BELOW_LEFT | BELOW_LEFT | ABOVE_LEFT => {
            base.x_bearing.wrapping_sub(mark.x_bearing)
        }
        ATTACHED_ABOVE_RIGHT | BELOW_RIGHT | ABOVE_RIGHT => {
            right.wrapping_sub(mark.width).wrapping_sub(mark.x_bearing)
        }
        _ => base
            .x_bearing
            .wrapping_add(base.width.wrapping_sub(mark.width) / 2)
            .wrapping_sub(mark.x_bearing),
    };
    let below = |pos: &mut Position, base: &mut Extents| {
        pos.y_offset = base
            .y_bearing
            .wrapping_add(base.height)
            .wrapping_sub(mark.y_bearing);
        // Marks below never move up.
        if (y_gap > 0) == (pos.y_offset > 0) {
            base.height = base.height.wrapping_sub(pos.y_offset);
            pos.y_offset = 0;
        }
        base.height = base.height.wrapping_add(mark.height);
    };
    let above = |pos: &mut Position, base: &mut Extents| {
        pos.y_offset = base
            .y_bearing
            .wrapping_sub(mark.y_bearing.wrapping_add(mark.height));
        // Marks above do not move down much.
        if (y_gap > 0) != (pos.y_offset > 0) {
            let correction = pos.y_offset.wrapping_neg() / 2;
            base.y_bearing = base.y_bearing.wrapping_add(correction);
            base.height = base.height.wrapping_sub(correction);
            pos.y_offset = pos.y_offset.wrapping_add(correction);
        }
        base.y_bearing = base.y_bearing.wrapping_sub(mark.height);
        base.height = base.height.wrapping_add(mark.height);
    };
    match class {
        DOUBLE_BELOW | BELOW_LEFT | BELOW | BELOW_RIGHT => {
            base.height = base.height.wrapping_sub(y_gap);
            below(pos, base);
        }
        ATTACHED_BELOW_LEFT | ATTACHED_BELOW => below(pos, base),
        DOUBLE_ABOVE | ABOVE_LEFT | ABOVE | ABOVE_RIGHT => {
            base.y_bearing = base.y_bearing.wrapping_add(y_gap);
            base.height = base.height.wrapping_sub(y_gap);
            above(pos, base);
        }
        ATTACHED_ABOVE | ATTACHED_ABOVE_RIGHT => above(pos, base),
        _ => {}
    }
}
