//! The glyph buffer lookups edit: a gap buffer, so inserting or removing glyphs where a
//! lookup is working moves only the glyphs between that place and the previous edit. A
//! lookup works from the start of the run to the end, so its edits cost time in proportion to
//! the run, where shifting the rest of the run at each edit would cost its square. HarfBuzz
//! gets the same from its separate output buffer.

use std::ops::{Index, IndexMut};

use super::GlyphInfo;

pub(crate) struct Buffer {
    /// The glyphs before the gap, the gap, then the glyphs after it.
    slots: Vec<GlyphInfo>,
    gap: usize,
    gap_len: usize,
}

impl Buffer {
    pub(crate) fn new(glyphs: Vec<GlyphInfo>) -> Self {
        Self {
            gap: glyphs.len(),
            slots: glyphs,
            gap_len: 0,
        }
    }

    pub(crate) fn into_vec(mut self) -> Vec<GlyphInfo> {
        let len = self.len();
        self.move_gap(len);
        self.slots.truncate(len);
        self.slots
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len() - self.gap_len
    }

    pub(crate) fn get(&self, i: usize) -> Option<&GlyphInfo> {
        (i < self.len()).then(|| &self.slots[self.slot(i)])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &GlyphInfo> {
        self.slots[..self.gap]
            .iter()
            .chain(&self.slots[self.gap + self.gap_len..])
    }

    fn slot(&self, i: usize) -> usize {
        if i < self.gap { i } else { i + self.gap_len }
    }

    /// Moves the gap to just before glyph `to` (`to` at most the length).
    fn move_gap(&mut self, to: usize) {
        if to < self.gap {
            self.slots.copy_within(to..self.gap, to + self.gap_len);
        } else if to > self.gap {
            let from = self.gap + self.gap_len;
            self.slots.copy_within(from..to + self.gap_len, self.gap);
        }
        self.gap = to;
    }

    /// Removes glyph `i`.
    pub(crate) fn remove(&mut self, i: usize) {
        assert!(i < self.len(), "glyph {i} is past the end");
        self.move_gap(i);
        self.gap_len += 1;
    }

    /// Replaces glyph `i` with `glyphs`, which must not be empty.
    pub(crate) fn replace_with(&mut self, i: usize, glyphs: &[GlyphInfo]) {
        let Some((first, rest)) = glyphs.split_first() else {
            return self.remove(i);
        };
        self[i] = *first;
        if rest.is_empty() {
            return;
        }
        self.move_gap(i + 1);
        if self.gap_len < rest.len() {
            // Grow the gap by at least half the buffer, so growing costs constant time per
            // glyph over a run.
            let grow = (rest.len() - self.gap_len).max(self.len() / 2 + 16);
            let at = self.gap + self.gap_len;
            self.slots
                .splice(at..at, std::iter::repeat_n(GlyphInfo::default(), grow));
            self.gap_len += grow;
        }
        self.slots[self.gap..self.gap + rest.len()].copy_from_slice(rest);
        self.gap += rest.len();
        self.gap_len -= rest.len();
    }
}

impl Index<usize> for Buffer {
    type Output = GlyphInfo;

    fn index(&self, i: usize) -> &GlyphInfo {
        assert!(i < self.len(), "glyph {i} is past the end");
        &self.slots[self.slot(i)]
    }
}

impl IndexMut<usize> for Buffer {
    fn index_mut(&mut self, i: usize) -> &mut GlyphInfo {
        assert!(i < self.len(), "glyph {i} is past the end");
        let slot = self.slot(i);
        &mut self.slots[slot]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(buffer: &Buffer) -> Vec<u16> {
        buffer.iter().map(|g| g.glyph).collect()
    }

    fn info(glyph: u16) -> GlyphInfo {
        GlyphInfo {
            glyph,
            ..GlyphInfo::default()
        }
    }

    #[test]
    fn edits_anywhere_keep_the_order() {
        let mut buffer = Buffer::new((0..6).map(info).collect());
        buffer.replace_with(1, &[info(10), info(11), info(12)]);
        assert_eq!(glyphs(&buffer), [0, 10, 11, 12, 2, 3, 4, 5]);
        buffer.remove(6);
        buffer.remove(0);
        assert_eq!(glyphs(&buffer), [10, 11, 12, 2, 3, 5]);
        buffer.replace_with(5, &[info(20), info(21)]);
        buffer.replace_with(0, &[info(30)]);
        assert_eq!(buffer.len(), 7);
        assert_eq!(buffer[6].glyph, 21);
        buffer[3].glyph = 40;
        assert_eq!(glyphs(&buffer), [30, 11, 12, 40, 3, 20, 21]);
        assert!(buffer.get(7).is_none());
        let v: Vec<u16> = buffer.into_vec().iter().map(|g| g.glyph).collect();
        assert_eq!(v, [30, 11, 12, 40, 3, 20, 21]);
    }

    #[test]
    fn edits_moving_forward_take_linear_time() {
        // Doubling every glyph of a long run, front to back.
        let n = 200_000;
        let mut buffer = Buffer::new((0..n).map(|_| info(1)).collect());
        let started = std::time::Instant::now();
        let mut i = 0;
        while i < buffer.len() {
            buffer.replace_with(i, &[info(2), info(3)]);
            i += 2;
        }
        assert_eq!(buffer.len(), 2 * n as usize);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }
}
