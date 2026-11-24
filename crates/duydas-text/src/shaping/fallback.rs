//! Mark positioning for fonts without GPOS, after HarfBuzz's fallback: each mark with a
//! combining class goes above, below or beside its base according to that class, using the
//! glyphs' ink boxes, and marks of the same class stack. The arithmetic wraps at 32 bits, as
//! HarfBuzz's does, so hostile metrics cannot overflow it.

use std::collections::HashMap;

use crate::font::layout::{GlyphInfo, Limits, Position};
use crate::font::{Font, GlyphId};

/// Canonical combining classes that place marks.
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

/// A glyph's ink box as HarfBuzz reports it: left and top edges, width, and height down
/// (negative).
#[derive(Clone, Copy, Debug, Default)]
struct Extents {
    x_bearing: i32,
    y_bearing: i32,
    width: i32,
    height: i32,
}

fn extents(font: &Font, glyph: u16) -> Option<Extents> {
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

/// What positioning needs besides the glyphs: the font, each glyph's ink box once, and the
/// shaping budget, a step of which each mark spends.
struct Context<'a> {
    font: &'a Font,
    extents: HashMap<u16, Option<Extents>>,
    limits: &'a Limits,
}

impl Context<'_> {
    /// A glyph's ink box, read from its outline the first time; a long run of marks reads
    /// each distinct glyph once.
    fn extents(&mut self, glyph: u16) -> Option<Extents> {
        let font = self.font;
        *self
            .extents
            .entry(glyph)
            .or_insert_with(|| extents(font, glyph))
    }
}

/// Positions every mark cluster: a base and the marks (by General_Category) after it. When
/// `limits` runs out, the marks left keep their positions.
pub(super) fn position_marks(
    font: &Font,
    info: &[GlyphInfo],
    pos: &mut [Position],
    adjust: bool,
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
    if end - start < 2 {
        return;
    }
    let mut i = start;
    while i < end {
        if !info[i].unicode_mark {
            let j = (i + 1..end).find(|&j| !info[j].unicode_mark).unwrap_or(end);
            position_around_base(cx, info, pos, i, j, adjust);
            i = j;
        } else {
            i += 1;
        }
    }
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
    let (mut x_offset, mut y_offset) = (
        pos[base].x_advance.wrapping_neg(),
        pos[base].y_advance.wrapping_neg(),
    );
    let mut component_extents = base_extents;
    let mut last_component = -1;
    let mut last_class: Option<u8> = None;
    let mut cluster_extents = base_extents;
    for i in base + 1..end {
        let class = info[i].combining_class;
        if class == 0 {
            x_offset = x_offset.wrapping_sub(pos[i].x_advance);
            y_offset = y_offset.wrapping_sub(pos[i].y_advance);
            continue;
        }
        if components > 1 {
            // HarfBuzz's unsigned arithmetic: a mark that follows no component takes the last.
            let mut component = i32::from(info[i].lig_comp()) - 1;
            if lig_id == 0 || lig_id != info[i].lig_id() || !(0..components).contains(&component) {
                component = components - 1;
            }
            if last_component != component {
                last_component = component;
                last_class = None;
                component_extents = base_extents;
                let share = component
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
        position_mark(font, mark, &mut pos[i], &mut cluster_extents, class);
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
) {
    let Some(mark) = mark else {
        return;
    };
    let y_gap = i32::from(font.metrics().units_per_em) / 16;
    pos.y_offset = 0;
    // Left and right marks are not moved across.
    let right = base.x_bearing.wrapping_add(base.width);
    pos.x_offset = match class {
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
