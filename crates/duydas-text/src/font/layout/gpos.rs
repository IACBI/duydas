//! Glyph positioning (GPOS) lookups 1 to 6; contexts are in the parent module.

use super::super::reader::{i16_at, u16_at};
use super::{
    Apply, IGNORE_FLAGS, IGNORE_MARKS, Lookup, MARK, MAX_NESTING, RIGHT_TO_LEFT, class_def,
    class_in, coverage, offset,
};

/// A glyph's position, in font units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Position {
    pub(crate) x_advance: i32,
    pub(crate) y_advance: i32,
    pub(crate) x_offset: i32,
    pub(crate) y_offset: i32,
    /// The glyph this one attaches to, as an index difference (0 for none), and how.
    attach: i32,
    attach_kind: u8,
}

const ATTACH_MARK: u8 = 1;
const ATTACH_CURSIVE: u8 = 2;

/// A ValueRecord: x placement, y placement, x advance, y advance. Device and variation
/// tables are not applied: positions are in font units at the default instance.
type Value = [i32; 4];

fn value_size(format: u16) -> usize {
    (format & 0xFF).count_ones() as usize * 2
}

fn read_value(t: &[u8], at: usize, format: u16) -> Option<Value> {
    let mut value = [0; 4];
    let mut p = at;
    for (bit, slot) in value.iter_mut().enumerate() {
        if format & (1 << bit) != 0 {
            *slot = i32::from(i16_at(t, p)?);
            p += 2;
        }
    }
    Some(value)
}

impl Position {
    pub(crate) fn with_advance(x_advance: i32) -> Self {
        Self {
            x_advance,
            ..Self::default()
        }
    }

    /// Adds a ValueRecord. In horizontal text HarfBuzz leaves the vertical advance alone.
    fn add(&mut self, [x_place, y_place, x_advance, _]: Value) {
        // Positions wrap at 32 bits, as HarfBuzz's do, rather than overflow on hostile values.
        self.x_offset = self.x_offset.wrapping_add(x_place);
        self.y_offset = self.y_offset.wrapping_add(y_place);
        self.x_advance = self.x_advance.wrapping_add(x_advance);
    }

    /// Hangs the glyph from its neighbour `step` (-1 or 1) away, as cross-stream kerning
    /// does.
    pub(super) fn hang(&mut self, step: i32) {
        self.attach = step;
        self.attach_kind = ATTACH_CURSIVE;
    }
}

/// An anchor point (formats 1 to 3; the contour point and device tables are not used).
fn anchor(t: &[u8], at: usize) -> Option<(i32, i32)> {
    if !(1..=3).contains(&u16_at(t, at)?) {
        return None;
    }
    Some((i32::from(i16_at(t, at + 2)?), i32::from(i16_at(t, at + 4)?)))
}

impl Apply<'_, '_> {
    /// Applies a positioning subtable of type `kind` at glyph `i`.
    pub(super) fn position_at(
        &mut self,
        lookup: &Lookup,
        kind: u16,
        at: usize,
        i: usize,
    ) -> Option<usize> {
        match kind {
            1 => self.single(at, i),
            2 => self.pair(lookup, at, i),
            3 => self.cursive(lookup, at, i),
            4 | 5 => self.mark_to_base(kind == 5, at, i),
            6 => self.mark_to_mark(lookup, at, i),
            _ => None,
        }
    }

    fn single(&mut self, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        let format = u16_at(t, at + 4)?;
        let value = match u16_at(t, at)? {
            1 => read_value(t, at + 6, format)?,
            2 if index < u16_at(t, at + 6)? => {
                read_value(t, at + 8 + usize::from(index) * value_size(format), format)?
            }
            _ => return None,
        };
        self.pos[i].add(value);
        Some(i + 1)
    }

    /// The next glyph the lookup does not skip, passing over default ignorables.
    fn next_glyph(&self, lookup: &Lookup, i: usize) -> Option<usize> {
        self.next_match(lookup, i, false, |info| !info.ignorable)
    }

    fn pair(&mut self, lookup: &Lookup, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        let (format1, format2) = (u16_at(t, at + 4)?, u16_at(t, at + 6)?);
        let (size1, size2) = (value_size(format1), value_size(format2));
        let j = self.next_glyph(lookup, i)?;
        let second = self.info[j].glyph;
        let record = match u16_at(t, at)? {
            1 => {
                let set = offset(
                    t,
                    at,
                    at + 10 + 2 * usize::from(index),
                    u16_at(t, at + 8)?,
                    index,
                )?;
                let size = 2 + size1 + size2;
                let (mut lo, mut hi) = (0, usize::from(u16_at(t, set)?));
                let mut found = None;
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    let r = set + 2 + mid * size;
                    let g = u16_at(t, r)?;
                    if g < second {
                        lo = mid + 1;
                    } else if g > second {
                        hi = mid;
                    } else {
                        found = Some(r + 2);
                        break;
                    }
                }
                found?
            }
            2 => {
                let class1 = class_in(t, class_def(t, at, at + 8)?, self.info[i].glyph);
                let class2 = class_in(t, class_def(t, at, at + 10)?, second);
                let (count1, count2) = (u16_at(t, at + 12)?, u16_at(t, at + 14)?);
                if class1 >= count1 || class2 >= count2 {
                    return None;
                }
                // Checked, since the product can pass 32 bits.
                let k = usize::from(class1)
                    .checked_mul(usize::from(count2))?
                    .checked_add(usize::from(class2))?;
                // Inside the table, so the reads after it cannot overflow.
                k.checked_mul(size1 + size2)?
                    .checked_add(at + 16)
                    .filter(|&record| record < t.len())?
            }
            _ => return None,
        };
        let value1 = read_value(t, record, format1)?;
        let value2 = read_value(t, record + size1, format2)?;
        self.pos[i].add(value1);
        self.pos[j].add(value2);
        Some(if format2 != 0 { j + 1 } else { j })
    }

    /// Cursive attachment (type 3): glyph `j`'s entry joins the exit of the glyph `i`
    /// before it. Left to right, `i`'s advance ends at its exit and `j` moves back to its
    /// entry; right to left, the other way round.
    fn cursive(&mut self, lookup: &Lookup, at: usize, j: usize) -> Option<usize> {
        let t = self.t;
        if u16_at(t, at)? != 1 {
            return None;
        }
        let cov = at + usize::from(u16_at(t, at + 2)?);
        let count = u16_at(t, at + 4)?;
        let point = |glyph: u16, which: usize| -> Option<(i32, i32)> {
            let index = coverage(t, cov, glyph)?;
            let record = at + 6 + 4 * usize::from(index) + which;
            anchor(t, offset(t, at, record, count, index)?)
        };
        let (entry_x, entry_y) = point(self.info[j].glyph, 0)?;
        let i = self.prev_match(lookup, j, false, |info| !info.ignorable)?;
        let (exit_x, exit_y) = point(self.info[i].glyph, 2)?;

        let (child, parent, y) = if lookup.flags & RIGHT_TO_LEFT != 0 {
            (i, j, entry_y - exit_y)
        } else {
            (j, i, exit_y - entry_y)
        };
        // The chain the attachment reverses is walked to measure it, then to collect it and
        // to reverse it: the budget must cover all three before anything changes. A chain
        // longer than that spends the budget, walked or not.
        let Some(chain) = cursive_chain_len(self.pos, child, parent, self.limits.left() / 3) else {
            self.limits.spend(usize::MAX);
            return None;
        };
        if !self.limits.spend(3 * chain) {
            return None;
        }
        let pos = &mut *self.pos;
        if self.backward {
            let d = exit_x.wrapping_add(pos[i].x_offset);
            pos[i].x_advance = pos[i].x_advance.wrapping_sub(d);
            pos[i].x_offset = pos[i].x_offset.wrapping_sub(d);
            pos[j].x_advance = entry_x.wrapping_add(pos[j].x_offset);
        } else {
            pos[i].x_advance = exit_x.wrapping_add(pos[i].x_offset);
            let d = entry_x.wrapping_add(pos[j].x_offset);
            pos[j].x_advance = pos[j].x_advance.wrapping_sub(d);
            pos[j].x_offset = pos[j].x_offset.wrapping_sub(d);
        }

        reverse_cursive_chain(pos, child, parent);
        pos[child].attach_kind = ATTACH_CURSIVE;
        pos[child].attach = parent as i32 - child as i32;
        pos[child].y_offset = y;
        if pos[parent].attach == -pos[child].attach {
            pos[parent].attach = 0;
            pos[parent].y_offset = 0;
        }
        self.attached = true;
        Some(j + 1)
    }

    /// The glyph a mark at `i` attaches to with mark-to-base or mark-to-ligature, found as
    /// HarfBuzz finds it: the nearest earlier glyph that is not a mark, whatever the lookup
    /// flags say, and that matching would not pass over. With mark-to-base's coverage
    /// (`bases`), the later glyphs of a multiple substitution's sequence are passed over
    /// unless it lists them. The search remembers where it stopped, so the marks after a base
    /// look only at the glyphs since.
    fn base_of(&mut self, i: usize, bases: Option<usize>) -> Option<usize> {
        if self.last_base_until > i {
            (self.last_base, self.last_base_until) = (None, 0);
        }
        for j in (self.last_base_until..i).rev() {
            if !self.spend() {
                return None;
            }
            let info = self.info[j];
            if self.skips(&info, IGNORE_MARKS, 0)
                || self.passable(&info, false)
                || !self.masked(&info)
            {
                continue;
            }
            if let Some(bases) = bases
                && !self.accept(j)
                && coverage(self.t, bases, info.glyph).is_none()
            {
                continue;
            }
            self.last_base = Some(j);
            break;
        }
        self.last_base_until = i;
        self.last_base
    }

    /// Whether a mark may attach to glyph `j`: of a multiple substitution's sequence only the
    /// first glyph takes marks, unless a mark comes between (HarfBuzz issues 740 and 1020).
    fn accept(&self, j: usize) -> bool {
        let info = self.info[j];
        if !info.multiplied || info.lig_comp() == 0 || j == 0 {
            return true;
        }
        let prev = self.info[j - 1];
        prev.class == MARK
            || !prev.multiplied
            || info.lig_id() != prev.lig_id()
            || prev.lig_comp() + 1 != info.lig_comp()
    }

    /// Mark-to-base (type 4) and mark-to-ligature (type 5) attachment.
    fn mark_to_base(&mut self, ligature: bool, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        if u16_at(t, at)? != 1 {
            return None;
        }
        let mark_index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        let base_coverage = at + usize::from(u16_at(t, at + 4)?);
        let j = self.base_of(i, (!ligature).then_some(base_coverage))?;
        let base_index = coverage(t, base_coverage, self.info[j].glyph)?;
        let classes = usize::from(u16_at(t, at + 6)?);
        let marks = at + usize::from(u16_at(t, at + 8)?);
        let bases = at + usize::from(u16_at(t, at + 10)?);
        let (table, row) = if ligature {
            let attach = offset(
                t,
                bases,
                bases + 2 + 2 * usize::from(base_index),
                u16_at(t, bases)?,
                base_index,
            )?;
            let components = usize::from(u16_at(t, attach)?);
            let (mark, lig) = (self.info[i], self.info[j]);
            let component =
                if lig.lig_id() != 0 && lig.lig_id() == mark.lig_id() && mark.lig_comp() > 0 {
                    usize::from(mark.lig_comp()).min(components)
                } else {
                    components
                };
            (attach, component.checked_sub(1)?)
        } else {
            if base_index >= u16_at(t, bases)? {
                return None;
            }
            (bases, usize::from(base_index))
        };
        self.attach_mark(marks, mark_index, classes, table, row, i, j)
    }

    /// Mark-to-mark attachment (type 6): the mark attaches to the previous mark that the
    /// lookup's mark filtering keeps, if both follow the same base or ligature component.
    fn mark_to_mark(&mut self, lookup: &Lookup, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        if u16_at(t, at)? != 1 {
            return None;
        }
        let mark_index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        let flags = lookup.flags & !IGNORE_FLAGS;
        let mut j = None;
        for k in (0..i).rev() {
            if !self.spend() {
                return None;
            }
            let info = &self.info[k];
            if self.skips(info, flags, lookup.filter) || self.passable(info, false) {
                continue;
            }
            if !self.masked(info) {
                return None;
            }
            j = Some(k);
            break;
        }
        let j = j?;
        let (cur, prev) = (self.info[i], self.info[j]);
        if prev.class != MARK {
            return None;
        }
        let same = if cur.lig_id() == prev.lig_id() {
            cur.lig_id() == 0 || cur.lig_comp() == prev.lig_comp()
        } else {
            (cur.lig_id() > 0 && cur.lig_comp() == 0) || (prev.lig_id() > 0 && prev.lig_comp() == 0)
        };
        if !same {
            return None;
        }
        let base_index = coverage(t, at + usize::from(u16_at(t, at + 4)?), prev.glyph)?;
        let classes = usize::from(u16_at(t, at + 6)?);
        let marks = at + usize::from(u16_at(t, at + 8)?);
        let bases = at + usize::from(u16_at(t, at + 10)?);
        if base_index >= u16_at(t, bases)? {
            return None;
        }
        self.attach_mark(
            marks,
            mark_index,
            classes,
            bases,
            usize::from(base_index),
            i,
            j,
        )
    }

    /// Attaches mark `i` to glyph `j`: the mark's class picks the anchor in row `row` of the
    /// anchor matrix at `table` (its count first, then `classes` offsets a row).
    #[allow(clippy::too_many_arguments)]
    fn attach_mark(
        &mut self,
        marks: usize,
        mark_index: u16,
        classes: usize,
        table: usize,
        row: usize,
        i: usize,
        j: usize,
    ) -> Option<usize> {
        let t = self.t;
        if mark_index >= u16_at(t, marks)? {
            return None;
        }
        let record = marks + 2 + 4 * usize::from(mark_index);
        let class = usize::from(u16_at(t, record)?);
        if class >= classes {
            return None;
        }
        let (mark_x, mark_y) = anchor(t, marks + usize::from(u16_at(t, record + 2)?))?;
        let slot = row
            .checked_mul(classes)?
            .checked_add(class)?
            .checked_mul(2)?
            .checked_add(table + 2)
            .filter(|&slot| slot < t.len())?;
        let (base_x, base_y) = match u16_at(t, slot)? {
            0 => return None,
            o => anchor(t, table + usize::from(o))?,
        };
        let pos = &mut self.pos[i];
        pos.x_offset = base_x - mark_x;
        pos.y_offset = base_y - mark_y;
        pos.attach = j as i32 - i as i32;
        pos.attach_kind = ATTACH_MARK;
        self.attached = true;
        Some(i + 1)
    }
}

/// Before `child` attaches cursively to `new_parent`, turns its old chain around so the
/// glyphs it was attached to hang from it instead (HarfBuzz `reverse_cursive_minor_offset`).
/// The length of the cursive chain from `child` that [`reverse_cursive_chain`] reverses, or
/// None if it is longer than `cap`.
pub(super) fn cursive_chain_len(
    pos: &[Position],
    child: usize,
    new_parent: usize,
    cap: usize,
) -> Option<usize> {
    let mut len = 0;
    let mut i = child;
    while pos[i].attach != 0 && pos[i].attach_kind == ATTACH_CURSIVE && len < pos.len() {
        let Some(j) = i
            .checked_add_signed(pos[i].attach as isize)
            .filter(|&j| j < pos.len())
        else {
            break;
        };
        len += 1;
        if len > cap {
            return None;
        }
        if j == new_parent {
            break;
        }
        i = j;
    }
    Some(len)
}

fn reverse_cursive_chain(pos: &mut [Position], child: usize, new_parent: usize) {
    let mut chain = Vec::new();
    let mut i = child;
    while pos[i].attach != 0 && pos[i].attach_kind == ATTACH_CURSIVE && chain.len() < pos.len() {
        let Some(j) = i
            .checked_add_signed(pos[i].attach as isize)
            .filter(|&j| j < pos.len())
        else {
            break;
        };
        chain.push((i, j));
        pos[i].attach = 0;
        if j == new_parent {
            break;
        }
        i = j;
    }
    for &(i, j) in chain.iter().rev() {
        if j == new_parent {
            continue;
        }
        pos[j].y_offset = pos[i].y_offset.wrapping_neg();
        pos[j].attach = i as i32 - j as i32;
        pos[j].attach_kind = ATTACH_CURSIVE;
    }
}

/// Resolves attachments into offsets from the pen position: a mark moves with its base and
/// over the advances between them (back over them left to right; `backward`, for
/// right-to-left text in logical order, forward over those after the base up to and
/// including its own); a cursive glyph moves vertically with the glyph it hangs from.
pub(crate) fn propagate_attachments(pos: &mut [Position], backward: bool) {
    // Running sums of the advances, so a mark far from its base costs no more than a near
    // one. Advances do not change from here on.
    let mut sums = Vec::with_capacity(pos.len() + 1);
    let (mut x, mut y) = (0i64, 0i64);
    sums.push((x, y));
    for p in pos.iter() {
        x += i64::from(p.x_advance);
        y += i64::from(p.y_advance);
        sums.push((x, y));
    }
    for i in 0..pos.len() {
        propagate(pos, &sums, i, backward, MAX_NESTING);
    }
}

fn propagate(pos: &mut [Position], sums: &[(i64, i64)], i: usize, backward: bool, depth: u8) {
    let (chain, kind) = (pos[i].attach, pos[i].attach_kind);
    if chain == 0 {
        return;
    }
    pos[i].attach = 0;
    let Some(j) = i
        .checked_add_signed(chain as isize)
        .filter(|&j| j < pos.len())
    else {
        return;
    };
    if depth == 0 {
        return;
    }
    propagate(pos, sums, j, backward, depth - 1);
    let (x_offset, y_offset) = (pos[j].x_offset, pos[j].y_offset);
    let p = &mut pos[i];
    p.y_offset = p.y_offset.wrapping_add(y_offset);
    if kind == ATTACH_CURSIVE {
        return;
    }
    p.x_offset = p.x_offset.wrapping_add(x_offset);
    // Truncating a 64-bit sum of advances gives what adding them one by one with 32-bit
    // wrapping would.
    if j < i && backward {
        p.x_offset = p
            .x_offset
            .wrapping_add((sums[i + 1].0 - sums[j + 1].0) as i32);
        p.y_offset = p
            .y_offset
            .wrapping_add((sums[i + 1].1 - sums[j + 1].1) as i32);
    } else if j < i {
        p.x_offset = p.x_offset.wrapping_sub((sums[i].0 - sums[j].0) as i32);
        p.y_offset = p.y_offset.wrapping_sub((sums[i].1 - sums[j].1) as i32);
    }
}
