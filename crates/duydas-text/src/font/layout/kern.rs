//! The legacy `kern` table, which HarfBuzz applies when GPOS has no `kern` feature: the
//! Microsoft (version 0) and Apple (version 1) headers, with horizontal pair (format 0) and
//! class (format 2) subtables. Vertical and variation subtables are skipped; cross-stream
//! ones move glyphs up and down, as HarfBuzz applies them.

use super::super::reader::{i16_at, u16_at, u32_at};
use super::{GlyphInfo, Limits, MARK, Position};

/// A `kern` table and its usable subtables.
pub(crate) struct Kern<'a> {
    table: &'a [u8],
    subtables: Vec<Subtable>,
    /// Whether a subtable kerns across the line; HarfBuzz then leaves marks alone.
    pub(crate) cross_stream: bool,
}

struct Subtable {
    header: usize,
    body: usize,
    end: usize,
    format: u8,
    cross_stream: bool,
}

impl<'a> Kern<'a> {
    /// Reads the table's subtable list; `None` if the table is malformed or empty.
    pub(crate) fn new(table: &'a [u8]) -> Option<Self> {
        let mut subtables = Vec::new();
        let mut cross_stream = false;
        let apple = u16_at(table, 0)? == 1;
        let (count, mut at) = if apple {
            (u32_at(table, 4)? as usize, 8)
        } else {
            (usize::from(u16_at(table, 2)?), 4)
        };
        for k in 0..count.min(256) {
            let (length, coverage, format, header) = if apple {
                let length = u32_at(table, at)? as usize;
                let coverage = table.get(at + 4).copied()?;
                (length, coverage, table.get(at + 5).copied()?, 8)
            } else {
                let length = usize::from(u16_at(table, at + 2)?);
                let coverage = u16_at(table, at + 4)?;
                (length, (coverage & 0xFF) as u8, (coverage >> 8) as u8, 6)
            };
            // The last Microsoft subtable may be longer than its 16-bit length can say.
            let end = if k + 1 == count || length < header {
                table.len()
            } else {
                at.checked_add(length)?.min(table.len())
            };
            // Horizontal, and for Apple's header not a variation subtable.
            let (usable, across) = if apple {
                (coverage & 0xA0 == 0, coverage & 0x40 != 0)
            } else {
                (coverage & 0x01 != 0, coverage & 0x04 != 0)
            };
            cross_stream |= across;
            if usable && matches!(format, 0 | 2) {
                subtables.push(Subtable {
                    header: at,
                    body: at + header,
                    end,
                    format,
                    cross_stream: across,
                });
            }
            if end >= table.len() {
                break;
            }
            at = end;
        }
        (count > 0).then_some(Self {
            table,
            subtables,
            cross_stream,
        })
    }

    /// Adds the kerning between each glyph and the next one that is not a mark, as HarfBuzz
    /// does: half to the first glyph's advance and half to the second, which moves back by
    /// its half. A cross-stream subtable instead raises the second glyph by the kerning,
    /// and every glyph then hangs from the one before it, so later glyphs rise with it.
    /// Returns whether a cross-stream subtable moved a glyph. Each subtable spends a step of
    /// `limits` per glyph, and kerning stops when they run out.
    ///
    /// Right-to-left text in logical order (`backward`) is kerned in visual order, as
    /// HarfBuzz does, and its glyphs hang from the next one instead.
    pub(crate) fn apply(
        &self,
        info: &[GlyphInfo],
        pos: &mut [Position],
        limits: &Limits,
        backward: bool,
    ) -> bool {
        let n = info.len();
        let at = |k: usize| if backward { n - 1 - k } else { k };
        let mut chained = false;
        let mut raised = false;
        for subtable in &self.subtables {
            // A pass over the run, whatever it finds: marks passed over and pairs tried.
            if !limits.spend(info.len()) {
                return raised;
            }
            if subtable.cross_stream && !chained {
                chained = true;
                for p in pos.iter_mut() {
                    p.hang(if backward { 1 } else { -1 });
                }
            }
            let data = &self.table[..subtable.end];
            let mut k = 0;
            while k < n {
                let Some(next) =
                    (k + 1..n).find(|&j| info[at(j)].class != MARK && !info[at(j)].ignorable)
                else {
                    break;
                };
                let (i, j) = (at(k), at(next));
                let (left, right) = (info[i].glyph, info[j].glyph);
                let kern = match subtable.format {
                    0 => pair(data, subtable.body, left, right),
                    _ => class_pair(data, subtable.header, subtable.body, left, right),
                };
                if kern != 0 {
                    if subtable.cross_stream {
                        pos[j].y_offset = kern;
                        raised = true;
                    } else {
                        let first = kern >> 1;
                        let second = kern - first;
                        pos[i].x_advance = pos[i].x_advance.wrapping_add(first);
                        pos[j].x_advance = pos[j].x_advance.wrapping_add(second);
                        pos[j].x_offset = pos[j].x_offset.wrapping_add(second);
                    }
                }
                k = next;
            }
        }
        raised
    }
}

/// Format 0: sorted (left, right, value) pairs.
fn pair(t: &[u8], at: usize, left: u16, right: u16) -> i32 {
    let find = || -> Option<i32> {
        let count = usize::from(u16_at(t, at)?);
        let key = (u32::from(left) << 16) | u32::from(right);
        // HarfBuzz's search over [lo, hi], so a font whose pairs are out of order kerns the
        // pairs it kerns there.
        let (mut lo, mut hi) = (0isize, count as isize - 1);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let record = at + 8 + 6 * mid as usize;
            let k = u32_at(t, record)?;
            if key < k {
                hi = mid - 1;
            } else if key > k {
                lo = mid + 1;
            } else {
                return Some(i32::from(i16_at(t, record + 4)?));
            }
        }
        None
    };
    find().unwrap_or(0)
}

/// Format 2: class tables whose values are byte offsets that add up to the kerning value's
/// position, counted like every offset of the subtable from the start of its header.
fn class_pair(t: &[u8], subtable: usize, body: usize, left: u16, right: u16) -> i32 {
    let find = || -> Option<i32> {
        let class = |offset: usize, glyph: u16| -> Option<usize> {
            let table = subtable + offset;
            let first = u16_at(t, table)?;
            let i = glyph
                .checked_sub(first)
                .filter(|&i| i < u16_at(t, table + 2).unwrap_or(0))?;
            u16_at(t, table + 4 + 2 * usize::from(i)).map(usize::from)
        };
        let array = usize::from(u16_at(t, body + 6)?);
        let l = class(usize::from(u16_at(t, body + 2)?), left).unwrap_or(0);
        let r = class(usize::from(u16_at(t, body + 4)?), right).unwrap_or(0);
        let offset = l + r;
        if offset < array {
            return None;
        }
        Some(i32::from(i16_at(t, subtable + offset)?))
    };
    find().unwrap_or(0)
}
