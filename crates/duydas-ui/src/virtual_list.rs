//! Windowing for long lists whose rows have different heights, such as chat history.
//!
//! Only rows near the viewport are built. Each row starts at an estimated height that is replaced
//! by its measured height once it is built. The list is bottom-anchored, so scroll
//! offsets count from the end: prepending history or re-measuring rows *above* the viewport never
//! moves what the user is looking at. Changes *below* the viewport (new messages while scrolled
//! up, late measurements) do move it; callers compensate with [`keep_position`].

use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Id, Scrollable, column, keyed_column, operation, scrollable, sensor, space};
use iced::{Element, Task};

/// Row heights with prefix sums (a Fenwick tree), so lookups stay O(log n) at any length.
#[derive(Debug, Clone)]
pub struct HeightIndex {
    heights: Vec<f32>,
    /// 1-based Fenwick tree; `f64` so repeated updates don't drift.
    tree: Vec<f64>,
    estimate: f32,
}

/// The rows to build for one frame and the space that stands in for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Window {
    /// First row to build.
    pub start: usize,
    /// One past the last row to build.
    pub end: usize,
    /// Height of the rows before `start`.
    pub before: f32,
    /// Height of the rows from `end` on.
    pub after: f32,
}

impl HeightIndex {
    /// Creates an index of `len` rows, each assumed to be `estimate` tall until measured.
    ///
    /// `estimate` must be finite and positive (a zero estimate would let one unmeasured row stand
    /// for the whole viewport); other values are replaced by 1.
    pub fn new(len: usize, estimate: f32) -> Self {
        let estimate = if estimate.is_finite() && estimate > 0.0 {
            estimate
        } else {
            1.0
        };
        let mut index = Self {
            heights: vec![estimate; len],
            tree: Vec::new(),
            estimate,
        };
        index.rebuild();
        index
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    /// Whether there are no rows.
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// Current height of row `i` (measured or estimated).
    pub fn height(&self, i: usize) -> Option<f32> {
        self.heights.get(i).copied()
    }

    /// Records the measured height of row `i`. Non-finite heights are ignored and negative ones
    /// count as zero, so a bad measurement can't break the prefix sums.
    ///
    /// The exact height is always stored. The return value says whether it moved by half a pixel
    /// or more, which callers can use to skip redraws caused by layout rounding.
    pub fn set(&mut self, i: usize, height: f32) -> bool {
        let Some(old) = self.heights.get(i).copied() else {
            return false;
        };
        if !height.is_finite() {
            return false;
        }
        let height = height.max(0.0);
        if height == old {
            return false;
        }
        self.heights[i] = height;
        let mut node = i + 1;
        let delta = f64::from(height) - f64::from(old);
        while node < self.tree.len() {
            self.tree[node] += delta;
            node += lowest_bit(node);
        }
        (height - old).abs() >= 0.5
    }

    /// Inserts `count` unmeasured rows before the first row (older history).
    ///
    /// O(n): the tree is rebuilt. At 10k rows that is microseconds per page, but cumulative work
    /// grows with loaded history (see ADR 0018).
    pub fn prepend(&mut self, count: usize) {
        self.heights
            .splice(0..0, std::iter::repeat_n(self.estimate, count));
        self.rebuild();
    }

    /// Adds `count` unmeasured rows after the last row (new messages).
    pub fn append(&mut self, count: usize) {
        for _ in 0..count {
            self.heights.push(self.estimate);
            let node = self.heights.len();
            // A Fenwick node covers (node - lowbit, node]; sum the part that already exists.
            let covered = self.prefix(node - 1) - self.prefix(node - lowest_bit(node));
            self.tree.push(f64::from(self.estimate) + covered);
        }
    }

    /// Height of all rows.
    pub fn total(&self) -> f32 {
        self.prefix(self.len()) as f32
    }

    /// Distance from the top of the list to the top of row `i`.
    pub fn offset_of(&self, i: usize) -> f32 {
        self.prefix(i.min(self.len())) as f32
    }

    /// Row containing the top-based position `y`, clamped to the valid range.
    pub fn index_at(&self, y: f32) -> usize {
        if self.is_empty() {
            return 0;
        }
        let mut remaining = f64::from(y.max(0.0));
        let mut node = 0;
        let mut step = (self.tree.len() - 1).next_power_of_two();
        while step > 0 {
            let next = node + step;
            if next < self.tree.len() && self.tree[next] <= remaining {
                node = next;
                remaining -= self.tree[next];
            }
            step /= 2;
        }
        node.min(self.len() - 1)
    }

    /// Rows to build for a viewport `viewport` tall whose bottom edge is `from_bottom` above
    /// the end of the list, plus `overscan` of extra content on each side.
    pub fn window(&self, from_bottom: f32, viewport: f32, overscan: f32) -> Window {
        if self.is_empty() {
            return Window::default();
        }
        let total = self.total();
        let top = (total - viewport - from_bottom.max(0.0)).max(0.0);
        let start = self.index_at(top - overscan);
        let end = (self.index_at(top + viewport + overscan) + 1).min(self.len());
        let before = self.offset_of(start);
        Window {
            start,
            end,
            before,
            after: (total - self.offset_of(end)).max(0.0),
        }
    }

    fn prefix(&self, count: usize) -> f64 {
        let mut node = count;
        let mut sum = 0.0;
        while node > 0 {
            sum += self.tree[node];
            node -= lowest_bit(node);
        }
        sum
    }

    fn rebuild(&mut self) {
        self.tree = vec![0.0; self.heights.len() + 1];
        for (i, height) in self.heights.iter().enumerate() {
            let node = i + 1;
            self.tree[node] += f64::from(*height);
            let parent = node + lowest_bit(node);
            if parent < self.tree.len() {
                self.tree[parent] += self.tree[node];
            }
        }
    }
}

fn lowest_bit(n: usize) -> usize {
    n.isolate_lowest_one()
}

/// Sensor anticipation distance that counts every laid-out row as shown. Iced measures it from
/// row corners, so a finite "overscan-sized" value misses rows taller than the viewport.
const MEASURE_EVERYWHERE: f32 = 1.0e7;

/// Builds the bottom-anchored scrollable for `window`.
///
/// `items` is the whole list, oldest first, in the same order as the [`HeightIndex`] that
/// produced `window`. `on_measure` receives a row's key and height as soon as the row is laid
/// out and whenever its height changes. Every built row is measured, not just visible ones:
/// with wrong estimates (or stale heights after a resize) all built rows can sit outside the
/// viewport, and only measuring them corrects the window.
///
/// Rows are keyed by `key`, but Iced's keyed column only matches one contiguous inserted or
/// removed range by key; when the window slides, row state is still diffed by position. Rows must therefore not
/// hold widget state that matters (focus, an inline editor) until the Phase 1 widget reconciles
/// by key.
pub fn list<'a, T, Message: 'a>(
    items: &'a [T],
    window: Window,
    key: impl Fn(&T) -> u64,
    row: impl Fn(&'a T) -> Element<'a, Message>,
    on_measure: impl Fn(u64, f32) -> Message + Clone + 'a,
) -> Scrollable<'a, Message> {
    let end = window.end.min(items.len());
    let start = window.start.min(end);
    let rows = items[start..end].iter().map(|item| {
        let id = key(item);
        let on_show = on_measure.clone();
        let on_resize = on_measure.clone();
        let measured = sensor(row(item))
            .key(id)
            .anticipate(MEASURE_EVERYWHERE)
            .on_show(move |size| on_show(id, size.height))
            .on_resize(move |size| on_resize(id, size.height));
        (id, measured.into())
    });
    // Keyed children must all be the same widget type (keyed diffing skips the type check), so
    // the spacers sit outside the keyed column.
    let content = column![
        space().height(window.before),
        keyed_column(rows),
        space().height(window.after),
    ];
    scrollable(content).anchor_bottom()
}

/// Pins the list at `from_bottom`, an absolute distance from the end.
///
/// Issue it with every prepend. Iced keeps the offset set by `operation::snap_to` *relative*, and
/// relative offsets count from the anchor, so on a bottom-anchored list "relative 1.0" is the very
/// top: after "jump to oldest" and a prepend, the view would stay glued to the new top. (Wheel
/// scrolling and scrollbar drags already store absolute offsets.)
///
/// Also issue it, with `from_bottom` increased by the added height, when appending rows while the
/// user is scrolled away from the live edge; otherwise the visible rows move up.
pub fn keep_position<T>(id: impl Into<Id>, from_bottom: f32) -> Task<T> {
    operation::scroll_to(
        id,
        AbsoluteOffset {
            x: 0.0,
            y: from_bottom,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_offset(heights: &[f32], i: usize) -> f32 {
        heights[..i].iter().sum()
    }

    #[test]
    fn prefix_sums_track_updates() {
        let mut index = HeightIndex::new(1000, 50.0);
        let mut expected = vec![50.0_f32; 1000];
        for i in (0..1000).step_by(7) {
            let h = 20.0 + (i % 13) as f32 * 9.0;
            index.set(i, h);
            expected[i] = h;
        }
        for i in [0, 1, 2, 63, 64, 65, 500, 999, 1000] {
            assert!(
                (index.offset_of(i) - brute_offset(&expected, i)).abs() < 0.01,
                "{i}"
            );
        }
        assert!((index.total() - expected.iter().sum::<f32>()).abs() < 0.01);
    }

    #[test]
    fn index_at_finds_containing_row() {
        let mut index = HeightIndex::new(5, 10.0);
        index.set(2, 30.0); // rows span 0-10, 10-20, 20-50, 50-60, 60-70
        assert_eq!(index.index_at(-5.0), 0);
        assert_eq!(index.index_at(0.0), 0);
        assert_eq!(index.index_at(9.9), 0);
        assert_eq!(index.index_at(10.0), 1);
        assert_eq!(index.index_at(49.0), 2);
        assert_eq!(index.index_at(50.0), 3);
        assert_eq!(index.index_at(1e9), 4);
    }

    #[test]
    fn prepend_and_append_keep_measurements() {
        let mut index = HeightIndex::new(3, 10.0);
        index.set(1, 40.0);
        index.prepend(2);
        assert_eq!(index.len(), 5);
        assert_eq!(index.height(3), Some(40.0));
        assert_eq!(index.height(0), Some(10.0));
        index.append(70);
        assert_eq!(index.len(), 75);
        assert!((index.total() - (74.0 * 10.0 + 40.0)).abs() < 0.01);
        for i in 0..=75 {
            let expected = i as f32 * 10.0 + if i > 3 { 30.0 } else { 0.0 };
            assert!((index.offset_of(i) - expected).abs() < 0.01, "{i}");
        }
    }

    #[test]
    fn window_covers_viewport_from_bottom() {
        let index = HeightIndex::new(10_000, 50.0);
        let at_end = index.window(0.0, 500.0, 0.0);
        assert_eq!((at_end.start, at_end.end), (9_990, 10_000));
        assert_eq!(at_end.after, 0.0);
        assert!((at_end.before - 9_990.0 * 50.0).abs() < 0.01);

        let scrolled = index.window(1_000.0, 500.0, 100.0);
        assert_eq!((scrolled.start, scrolled.end), (9_968, 9_983));
        let built: f32 = (scrolled.end - scrolled.start) as f32 * 50.0;
        assert!((scrolled.before + built + scrolled.after - index.total()).abs() < 0.01);
    }

    #[test]
    fn prepend_does_not_move_bottom_anchored_window() {
        let mut index = HeightIndex::new(1_000, 50.0);
        let before = index.window(2_000.0, 600.0, 0.0);
        index.prepend(200);
        let after = index.window(2_000.0, 600.0, 0.0);
        assert_eq!(after.start, before.start + 200);
        assert_eq!(after.end, before.end + 200);
        assert_eq!(after.after, before.after);
    }

    #[test]
    fn short_lists_and_empty_lists() {
        assert_eq!(
            HeightIndex::new(0, 50.0).window(0.0, 500.0, 50.0),
            Window::default()
        );
        let short = HeightIndex::new(3, 50.0).window(0.0, 500.0, 50.0);
        assert_eq!(
            (short.start, short.end, short.before, short.after),
            (0, 3, 0.0, 0.0)
        );
    }

    #[test]
    fn exact_heights_are_kept_but_rounding_is_reported_as_unchanged() {
        let mut index = HeightIndex::new(2, 50.0);
        assert!(!index.set(0, 50.3));
        assert!((index.total() - 100.3).abs() < 1e-4);
        assert!(index.set(0, 51.0));
        assert!(!index.set(5, 10.0));
    }

    #[test]
    fn invalid_heights_cannot_corrupt_the_index() {
        let mut index = HeightIndex::new(3, 50.0);
        assert!(!index.set(1, f32::NAN));
        assert!(!index.set(1, f32::INFINITY));
        assert!(index.set(1, -20.0));
        assert_eq!(index.height(1), Some(0.0));
        assert!((index.total() - 100.0).abs() < 1e-4);
        assert_eq!(index.index_at(75.0), 2);

        for bad in [0.0, -1.0, f32::NAN] {
            assert_eq!(HeightIndex::new(2, bad).height(0), Some(1.0));
        }
    }
}
