//! Character to glyph mapping (the `cmap` table): formats 0, 4, 6, 12 and 13, and format 14
//! for variation sequences such as Japanese ideographic variants.

use super::FontError;
use super::reader::{Reader, Span, u8_at, u16_at, u24_at, u32_at};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cmap {
    main: Option<Subtable>,
    variations: Option<Span>,
}

#[derive(Clone, Copy, Debug)]
struct Subtable {
    span: Span,
    format: u16,
    encoding: Encoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Unicode,
    /// Windows symbol fonts map their characters at U+F000 to U+F0FF.
    Symbol,
    /// Mac Roman, used only for ASCII.
    MacRoman,
}

/// The result of looking up a variation sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Variation {
    /// The font does not list the sequence.
    Missing,
    /// The sequence uses the base character's usual glyph.
    Default,
    /// The sequence has its own glyph.
    Glyph(u16),
}

impl Cmap {
    pub(crate) fn parse(data: &[u8], table: Span) -> Result<Self, FontError> {
        const MALFORMED: FontError = FontError::Malformed("cmap");
        let bytes = table.of(data);
        let mut r = Reader::new(bytes);
        r.skip(2).ok_or(MALFORMED)?;
        let count = r.u16().ok_or(MALFORMED)?;
        let mut best: Option<(u8, Subtable)> = None;
        let mut variations = None;
        let mut records = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let platform = r.u16().ok_or(MALFORMED)?;
            let encoding = r.u16().ok_or(MALFORMED)?;
            let offset = r.u32().ok_or(MALFORMED)? as usize;
            records.push((platform, encoding, offset));
        }
        let mut offsets: Vec<usize> = records.iter().map(|&(_, _, o)| o).collect();
        offsets.sort_unstable();
        offsets.dedup();
        // Records often share a subtable: each is checked once, and the checks of all of them
        // together are bounded.
        let mut checked: Vec<Option<bool>> = vec![None; offsets.len()];
        let mut budget = MAX_CHECKS;
        for &(platform, encoding, offset) in &records {
            let at = offsets.partition_point(|&o| o < offset);
            let next = offsets.get(at + 1).copied().unwrap_or(bytes.len());
            let Some((format, span)) = subtable(bytes, offset, next) else {
                continue;
            };
            // Rank first, so only a subtable that would be used is checked.
            let candidate = if format == 14 {
                if variations.is_some() || (platform, encoding) != (0, 5) {
                    continue;
                }
                None
            } else {
                let unicode = platform == 0 || (platform == 3 && matches!(encoding, 1 | 10));
                let (score, encoding) = match (unicode, platform, encoding, format) {
                    (true, _, _, 12) => (6, Encoding::Unicode),
                    (true, _, _, 13) => (5, Encoding::Unicode),
                    (true, _, _, 4) => (4, Encoding::Unicode),
                    (true, _, _, 0 | 6) => (3, Encoding::Unicode),
                    (false, 3, 0, _) => (2, Encoding::Symbol),
                    (false, 1, 0, 0 | 6) => (1, Encoding::MacRoman),
                    _ => continue,
                };
                if best.is_some_and(|(s, _)| score <= s) {
                    continue;
                }
                Some((score, encoding))
            };
            // A broken subtable must not displace a usable one of lower rank.
            let well_formed = *checked[at]
                .get_or_insert_with(|| well_formed(span.of(bytes), format, &mut budget));
            if !well_formed {
                continue;
            }
            let span = Span {
                start: table.start + span.start,
                end: table.start + span.end,
            };
            let Some((score, encoding)) = candidate else {
                variations = Some(span);
                continue;
            };
            best = Some((
                score,
                Subtable {
                    span,
                    format,
                    encoding,
                },
            ));
        }
        Ok(Self {
            main: best.map(|(_, s)| s),
            variations,
        })
    }

    /// The glyph for a code point, or `None` (including glyph 0, the missing glyph).
    pub(crate) fn lookup(&self, data: &[u8], c: u32) -> Option<u16> {
        let sub = self.main?;
        let t = sub.span.of(data);
        let found = |c| lookup(t, sub.format, c).filter(|&g| g != 0);
        match sub.encoding {
            Encoding::Unicode => found(c),
            Encoding::Symbol => {
                found(c).or_else(|| (c <= 0xFF).then(|| found(0xF000 + c)).flatten())
            }
            Encoding::MacRoman => (c < 0x80).then(|| found(c)).flatten(),
        }
    }

    /// How the font maps the sequence of `c` followed by the variation selector `selector`.
    pub(crate) fn variation(&self, data: &[u8], c: u32, selector: u32) -> Variation {
        self.variations
            .and_then(|span| variation(span.of(data), c, selector))
            .unwrap_or(Variation::Missing)
    }
}

/// The format and extent of the subtable at `offset`, clamped to the table. A format 4
/// subtable runs to `next`, where the next subtable starts or the table ends: its 16-bit length
/// cannot hold the size of a large one, and fonts write it wrapped or capped.
fn subtable(table: &[u8], offset: usize, next: usize) -> Option<(u16, Span)> {
    let format = u16_at(table, offset)?;
    let length = match format {
        4 => next.saturating_sub(offset),
        0 | 6 => usize::from(u16_at(table, offset + 2)?),
        12 | 13 => u32_at(table, offset + 4)? as usize,
        14 => u32_at(table, offset + 2)? as usize,
        _ => return None,
    };
    let end = offset.saturating_add(length).min(table.len());
    Some((format, Span { start: offset, end }))
}

/// How many format 4 segments a font's subtables may make `Cmap::parse` check in all: 32
/// subtables of the largest size, where fonts have one or two.
const MAX_CHECKS: usize = 1 << 20;

/// Whether a subtable's header and arrays fit its extent, so lookups in it can succeed.
/// `budget` is the number of format 4 segments left to check.
fn well_formed(t: &[u8], format: u16, budget: &mut usize) -> bool {
    match format {
        0 => holds(t, 6, 256, 1),
        4 => u16_at(t, 6).is_some_and(|seg_x2| {
            let segments = usize::from(seg_x2 / 2);
            let Some(left) = budget.checked_sub(segments) else {
                return false;
            };
            *budget = left;
            seg_x2 > 0
                && seg_x2 % 2 == 0
                && holds(t, 16, usize::from(seg_x2), 4)
                && format4_arrays_fit(t, usize::from(seg_x2))
        }),
        6 => u16_at(t, 8).is_some_and(|count| holds(t, 10, usize::from(count), 2)),
        12 | 13 => {
            u32_at(t, 12).is_some_and(|groups| groups > 0 && holds(t, 16, groups as usize, 12))
        }
        14 => u32_at(t, 6).is_some_and(|records| holds(t, 10, records as usize, 11)),
        _ => false,
    }
}

/// Whether every glyph array a format 4 segment points to lies within the subtable.
fn format4_arrays_fit(t: &[u8], seg_x2: usize) -> bool {
    let ranges = 16 + 3 * seg_x2;
    (0..seg_x2 / 2).all(|i| {
        let (Some(end), Some(start), Some(offset)) = (
            u16_at(t, 14 + 2 * i),
            u16_at(t, 16 + seg_x2 + 2 * i),
            u16_at(t, ranges + 2 * i),
        ) else {
            return false;
        };
        offset == 0
            || end < start
            || holds(
                t,
                ranges + 2 * i + usize::from(offset),
                usize::from(end - start) + 1,
                2,
            )
    })
}

/// Whether `t` holds `count` records of `size` bytes from `start`, without overflowing on
/// 32-bit targets.
fn holds(t: &[u8], start: usize, count: usize, size: usize) -> bool {
    count
        .checked_mul(size)
        .and_then(|n| n.checked_add(start))
        .is_some_and(|end| end <= t.len())
}

/// The record count of the array whose 32-bit count is at `at`, if the whole array fits.
fn array(t: &[u8], at: usize, size: usize) -> Option<usize> {
    let count = u32_at(t, at)? as usize;
    holds(t, at.checked_add(4)?, count, size).then_some(count)
}

fn lookup(t: &[u8], format: u16, c: u32) -> Option<u16> {
    match format {
        0 => usize::try_from(c)
            .ok()
            .filter(|&c| c < 256)
            .and_then(|c| u8_at(t, 6 + c))
            .map(u16::from),
        4 => format4(t, c),
        6 => {
            let first = u32::from(u16_at(t, 6)?);
            let count = u32::from(u16_at(t, 8)?);
            let i = c.checked_sub(first).filter(|&i| i < count)?;
            u16_at(t, 10 + 2 * i as usize)
        }
        12 | 13 => groups(t, c, format == 13),
        _ => None,
    }
}

fn format4(t: &[u8], c: u32) -> Option<u16> {
    let c = u16::try_from(c).ok()?;
    let seg_x2 = usize::from(u16_at(t, 6)?);
    let segments = seg_x2 / 2;
    let ends = 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    // The first segment whose end is at or after `c`.
    let (mut lo, mut hi) = (0, segments);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if u16_at(t, ends + 2 * mid)? < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == segments {
        return None;
    }
    let start = u16_at(t, starts + 2 * lo)?;
    if c < start {
        return None;
    }
    let delta = u16_at(t, deltas + 2 * lo)?;
    let range_offset = usize::from(u16_at(t, ranges + 2 * lo)?);
    if range_offset == 0 {
        return Some(c.wrapping_add(delta));
    }
    let at = ranges + 2 * lo + range_offset + 2 * usize::from(c - start);
    let glyph = u16_at(t, at)?;
    (glyph != 0).then(|| glyph.wrapping_add(delta))
}

fn groups(t: &[u8], c: u32, constant: bool) -> Option<u16> {
    // `well_formed` checked that the groups fit.
    let count = u32_at(t, 12)? as usize;
    let (mut lo, mut hi) = (0, count);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let at = 16 + 12 * mid;
        let start = u32_at(t, at)?;
        let end = u32_at(t, at + 4)?;
        if c < start {
            hi = mid;
        } else if c > end {
            lo = mid + 1;
        } else {
            let first = u32_at(t, at + 8)?;
            let glyph = if constant {
                first
            } else {
                first.checked_add(c - start)?
            };
            return u16::try_from(glyph).ok();
        }
    }
    None
}

fn variation(t: &[u8], c: u32, selector: u32) -> Option<Variation> {
    let count = u32_at(t, 6)? as usize;
    let (mut lo, mut hi) = (0, count);
    let record = loop {
        if lo >= hi {
            return None;
        }
        let mid = (lo + hi) / 2;
        let at = 10 + 11 * mid;
        let vs = u24_at(t, at)?;
        if selector < vs {
            hi = mid;
        } else if selector > vs {
            lo = mid + 1;
        } else {
            break at;
        }
    };
    let default = u32_at(t, record + 3)? as usize;
    if default != 0 {
        // A table whose array does not fit is ignored, as in `well_formed`.
        let ranges = array(t, default, 4).unwrap_or(0);
        let (mut lo, mut hi) = (0, ranges);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = default + 4 + 4 * mid;
            let start = u24_at(t, at)?;
            let extra = u32::from(u8_at(t, at + 3)?);
            if c < start {
                hi = mid;
            } else if c > start + extra {
                lo = mid + 1;
            } else {
                return Some(Variation::Default);
            }
        }
    }
    let special = u32_at(t, record + 7)? as usize;
    if special != 0 {
        let mappings = array(t, special, 5).unwrap_or(0);
        let (mut lo, mut hi) = (0, mappings);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = special + 4 + 5 * mid;
            let code = u24_at(t, at)?;
            if c < code {
                hi = mid;
            } else if c > code {
                lo = mid + 1;
            } else {
                return Some(Variation::Glyph(u16_at(t, at + 3)?));
            }
        }
    }
    Some(Variation::Missing)
}

#[cfg(test)]
mod tests {
    use super::well_formed;

    #[test]
    fn format_4_checks_draw_on_a_budget() {
        // Two segments, the second the closing 0xFFFF one, with no glyph arrays: the header,
        // then the ends, padding, starts, deltas and range offsets.
        let words: [u16; 16] = [
            4, 32, 0, 4, 4, 1, 0, 0x41, 0xFFFF, 0, 0x41, 0xFFFF, 0, 1, 0, 0,
        ];
        let t: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
        let mut budget = 1;
        assert!(!well_formed(&t, 4, &mut budget));
        assert_eq!(budget, 1);
        let mut budget = 2;
        assert!(well_formed(&t, 4, &mut budget));
        assert_eq!(budget, 0);
    }
}
