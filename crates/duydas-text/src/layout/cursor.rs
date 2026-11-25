//! The caret, hit testing, selection and caret movement over a [`Layout`].
//!
//! A position is a byte offset at a grapheme cluster boundary with an affinity. Where one
//! offset has two places on screen, the affinity picks one:
//!
//! - where a line wraps, the end of the line before (upstream) or the start of the next
//!   (downstream);
//! - where the direction changes, the trailing edge of the grapheme before (upstream) or the
//!   leading edge of the one after (downstream).

use std::ops::Range;

use super::{Kind, Layout};
use crate::bidi::Direction;

/// Which of two places on screen a position means where one offset has two: where a line
/// wraps, the end of the line before (upstream) or the start of the next (downstream); where
/// the direction changes, the trailing edge of the grapheme before (upstream) or the leading
/// edge of the one after (downstream).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Affinity {
    /// With the grapheme after the position.
    #[default]
    Downstream,
    /// With the grapheme before it.
    Upstream,
}

/// A caret position in a layout's text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Position {
    /// A byte offset in the text; inside a grapheme cluster, it means the cluster's start.
    pub index: usize,
    /// Which place on screen the offset means where it has two.
    pub affinity: Affinity,
}

impl Position {
    /// The position at `index`, with the grapheme after it.
    pub const fn new(index: usize) -> Self {
        Self {
            index,
            affinity: Affinity::Downstream,
        }
    }

    /// The position at `index`, with the grapheme before it.
    pub const fn upstream(index: usize) -> Self {
        Self {
            index,
            affinity: Affinity::Upstream,
        }
    }
}

/// Where to draw a caret: a vertical bar the height of its line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// The bar's x.
    pub x: f32,
    /// The top of the bar: the top of its line.
    pub top: f32,
    /// The bar's height: its line's.
    pub height: f32,
    /// The index of the line in [`Layout::lines`].
    pub line: usize,
}

/// A rectangle, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
}

/// Where a position is on screen: its line and x.
#[derive(Clone, Copy, Debug)]
struct Spot {
    line: usize,
    x: f32,
}

impl Layout {
    /// Where to draw the caret at `position`.
    pub fn caret(&self, position: Position) -> Caret {
        let spot = self.spot(position);
        let line = &self.lines[spot.line];
        Caret {
            x: spot.x,
            top: line.top,
            height: line.height,
            line: spot.line,
        }
    }

    /// The position nearest to a point: on the first line that ends below `y` (the last line
    /// if none does), at the nearest grapheme edge to `x`.
    pub fn hit(&self, x: f32, y: f32) -> Position {
        let line = self
            .lines
            .partition_point(|l| l.top + l.height <= y)
            .min(self.lines.len() - 1);
        self.hit_line(line, x)
    }

    /// The position on line `line` (the last line, if there are fewer) at the nearest
    /// grapheme edge to `x`; moving the caret up or down a line keeps its x this way. An `x`
    /// beyond the line, infinite or not a number, gives the line's end on that side (the left
    /// for NaN). A position never falls after a line's line break.
    pub fn hit_line(&self, line: usize, x: f32) -> Position {
        let line = line.min(self.lines.len() - 1);
        let order = self.visual(line);
        let (Some(&first), Some(&last)) = (order.first(), order.last()) else {
            return self.line_start(line);
        };
        if x.is_nan() || x < self.cells[first].left {
            return self.edge_position(first, false);
        }
        if x >= self.cells[last].right {
            return self.edge_position(last, true);
        }
        // The cells follow each other, so the first that ends after `x` holds it.
        let k = order[order.partition_point(|&k| self.cells[k].right <= x)];
        let c = &self.cells[k];
        // In f64 the sum is exact, so the half nearer to `x` is too.
        let right = 2.0 * f64::from(x) >= f64::from(c.left) + f64::from(c.right);
        self.edge_position(k, right)
    }

    /// The rectangles that highlight the graphemes in `range` (whose ends move back to
    /// grapheme cluster boundaries): on each line, one per stretch of adjacent graphemes, from
    /// left to right, as the text's directions place them. Line breaks are not highlighted.
    pub fn selection(&self, range: Range<usize>) -> Vec<Rect> {
        let (start, end) = (self.snap(range.start), self.snap(range.end));
        let mut rects = Vec::new();
        if start >= end {
            return rects;
        }
        let mut k = self.cells.partition_point(|c| c.start < start);
        let last = self.cells.partition_point(|c| c.start < end);
        let mut spans: Vec<(f32, f32)> = Vec::new();
        while k < last {
            let line = self.cells[k].line;
            spans.clear();
            while k < last && self.cells[k].line == line {
                let c = &self.cells[k];
                if c.kind != Kind::Break && c.right > c.left {
                    spans.push((c.left, c.right));
                }
                k += 1;
            }
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let l = &self.lines[line];
            let mut current: Option<(f32, f32)> = None;
            for &(left, right) in &spans {
                match &mut current {
                    Some(span) if left <= span.1 => span.1 = span.1.max(right),
                    _ => {
                        if let Some((a, b)) = current.replace((left, right)) {
                            rects.push(Rect {
                                x: a,
                                y: l.top,
                                width: b - a,
                                height: l.height,
                            });
                        }
                    }
                }
            }
            if let Some((a, b)) = current {
                rects.push(Rect {
                    x: a,
                    y: l.top,
                    width: b - a,
                    height: l.height,
                });
            }
        }
        rects
    }

    /// The line `position` is on.
    pub fn line_of(&self, position: Position) -> usize {
        self.spot(position).line
    }

    /// The position at the start of line `line`.
    pub fn line_start(&self, line: usize) -> Position {
        Position::new(self.lines[line.min(self.lines.len() - 1)].range.start)
    }

    /// The position at the end of line `line`: before its line break, or upstream where it
    /// wraps.
    pub fn line_end(&self, line: usize) -> Position {
        let line = line.min(self.lines.len() - 1);
        match self.line_cells[line].clone().last() {
            Some(k) if self.cells[k].kind == Kind::Break => Position::new(self.cells[k].start),
            _ if line + 1 == self.lines.len() => Position::new(self.len),
            _ => Position::upstream(self.lines[line].range.end),
        }
    }

    /// The position one grapheme to the left on screen, passing over graphemes that take no
    /// room. From the left end of a line it goes on in the line's paragraph's direction, to the
    /// right end of the line before in left-to-right text or of the line after in right-to-left
    /// text. Where a line wraps, it moves on past the offset the two lines share, so every key
    /// press moves the offset.
    pub fn move_left(&self, position: Position) -> Position {
        self.step(position, false)
    }

    /// The position one grapheme to the right on screen; see [`Layout::move_left`].
    pub fn move_right(&self, position: Position) -> Position {
        self.step(position, true)
    }

    fn step(&self, position: Position, right: bool) -> Position {
        let position = self.tidy(position);
        if let Some(next) = self.step_on_line(position, right) {
            return next;
        }
        let line = self.spot(position).line;
        let forward = right == (self.lines[line].direction == Direction::LeftToRight);
        let target = if forward {
            (line + 1 < self.lines.len()).then_some(line + 1)
        } else {
            line.checked_sub(1)
        };
        let Some(target) = target else {
            return position;
        };
        // Moving right, the caret comes into the line at its left end, and the other way round.
        let t = self.entry(target, right);
        if t.index != position.index {
            return t;
        }
        // Where a line wraps, both ends are one offset: move on past it, or, on a line with
        // nothing that takes room, to its other end.
        self.step_on_line(t, right).unwrap_or_else(|| {
            if forward {
                self.line_end(target)
            } else {
                self.line_start(target)
            }
        })
    }

    /// The position at the left end of line `line` on screen when `right`, else at its right
    /// end.
    fn entry(&self, line: usize, right: bool) -> Position {
        let order = self.visual(line);
        let end = if right { order.first() } else { order.last() };
        end.map_or_else(|| self.line_start(line), |&k| self.edge_position(k, !right))
    }

    /// The cells of line `line` that take room, from left to right.
    fn visual(&self, line: usize) -> &[usize] {
        &self.order[self.line_order[line].clone()]
    }

    /// The position one grapheme to the side on `position`'s line, if it is not at that end.
    /// Graphemes that take no room are passed over.
    fn step_on_line(&self, position: Position, right: bool) -> Option<Position> {
        let spot = self.spot(position);
        let order = self.visual(spot.line);
        // The cells follow each other, so those wholly before the caret come first.
        let slot = order.partition_point(|&k| self.cells[k].right <= spot.x);
        if right {
            order.get(slot).map(|&k| self.edge_position(k, true))
        } else {
            slot.checked_sub(1)
                .map(|s| self.edge_position(order[s], false))
        }
    }

    /// The position at the left or right edge of cell `k`.
    fn edge_position(&self, k: usize, right: bool) -> Position {
        let c = &self.cells[k];
        // The left edge of a left-to-right grapheme and the right edge of a right-to-left one
        // are its start.
        if c.rtl == right {
            Position::new(c.start)
        } else {
            self.tidy(Position::upstream(self.cell_end(k)))
        }
    }

    /// `position` at a grapheme boundary, downstream unless that is somewhere else.
    fn tidy(&self, position: Position) -> Position {
        let index = self.snap(position.index);
        if position.affinity == Affinity::Upstream {
            let up = self.spot(Position::upstream(index));
            let down = self.spot(Position::new(index));
            if up.line != down.line || up.x != down.x {
                return Position::upstream(index);
            }
        }
        Position::new(index)
    }

    /// `index` moved back to the start of its grapheme cluster, and clamped to the text.
    fn snap(&self, index: usize) -> usize {
        if index >= self.len {
            return self.len;
        }
        let k = self.cells.partition_point(|c| c.start <= index);
        k.checked_sub(1).map_or(0, |k| self.cells[k].start)
    }

    /// The byte after cell `k`'s grapheme.
    pub(super) fn cell_end(&self, k: usize) -> usize {
        self.cells.get(k + 1).map_or(self.len, |c| c.start)
    }

    fn spot(&self, position: Position) -> Spot {
        let index = self.snap(position.index);
        let after = self.cells.partition_point(|c| c.start < index);
        let after = (after < self.cells.len()).then_some(after);
        let before = match after {
            Some(a) => a.checked_sub(1),
            None => self.cells.len().checked_sub(1),
        }
        .filter(|&b| self.cells[b].kind != Kind::Break);
        let trailing = |k: usize| {
            let c = &self.cells[k];
            Spot {
                line: c.line,
                x: if c.rtl { c.left } else { c.right },
            }
        };
        match (position.affinity, before, after) {
            (Affinity::Upstream, Some(b), _) | (_, Some(b), None) => trailing(b),
            (_, _, Some(a)) => {
                let c = &self.cells[a];
                Spot {
                    line: c.line,
                    x: if c.rtl { c.right } else { c.left },
                }
            }
            // The empty line at the end of the text, or empty text.
            (_, None, None) => {
                let line = self.lines.len() - 1;
                Spot {
                    line,
                    x: self.lines[line].x,
                }
            }
        }
    }
}
