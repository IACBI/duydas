//! Damage tracking and scroll blitting.
//!
//! Each frame the renderer fingerprints every 64 × 64 tile of the canvas from the commands that
//! touch it, compares the fingerprints with last frame's and redraws only the tiles that
//! differ. When a scroll layer moved by less than its height, it first moves the layer's pixels
//! and compares against last frame's commands moved the same way, so a scroll redraws the newly
//! exposed rows and nothing else that did not change.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::geom::IRect;
use crate::hash::mix;
use crate::raster::{self, Shape};
use crate::scene::{Cmd, Item, Layer, Scene};

const TILE_SHIFT: u32 = 6;
const TILE: i32 = 1 << TILE_SHIFT;
/// Beyond this many rectangles, damage is merged into one bounding rectangle.
const MAX_RECTS: usize = 32;
const EMPTY_TILE: u64 = 0x243F_6A88_85A3_08D3;

/// What a [`Renderer::render`] call did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Whether the whole canvas was redrawn (first frame, new size, background or canvas, or a
    /// canvas changed outside the renderer).
    pub full: bool,
    /// Pixels cleared and drawn again.
    pub repainted: u64,
    /// Pixels moved by scroll blits instead of being drawn.
    pub scrolled: u64,
    /// Commands drawn, counting a command once per damaged rectangle it touches.
    pub draws: u32,
}

/// Draws [`Scene`]s into a [`Canvas`], redrawing only what changed since the previous frame.
///
/// Use one renderer per canvas. A canvas the renderer did not draw last, or one changed
/// outside it, is redrawn in full, so mixing them up costs time but never shows stale pixels.
/// Damage is found by comparing 64-bit fingerprints; two different frames with the same
/// fingerprint for a tile (about one chance in 2^64 per tile) would leave that tile stale until
/// it changes again.
#[derive(Debug, Default)]
pub struct Renderer {
    prev_items: Vec<Item>,
    prev_layers: Vec<Layer>,
    target: Option<Target>,
    old_tiles: Vec<u64>,
    new_tiles: Vec<u64>,
    tile_rects: Vec<IRect>,
    repaint: Vec<IRect>,
    damage: Vec<IRect>,
    blits: Vec<Blit>,
    blit_of_prev: Vec<Option<usize>>,
    blit_of_next: Vec<Option<usize>>,
    /// Rectangles (indices into tile_rects) that reach the current tile row, and the next one.
    open: Vec<usize>,
    next_open: Vec<usize>,
}

/// What the previous frame was drawn into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    canvas: (u64, u64),
    width: u32,
    height: u32,
    background: Color,
}

#[derive(Debug, Clone, Copy)]
struct Blit {
    /// The layer's index last frame.
    prev: usize,
    /// The part of the viewport on the canvas: the pixels that move.
    area: IRect,
    /// The part of ~~area~~ holding moved pixels (all of it but the exposed strip).
    kept: IRect,
    dy: i32,
    /// The layer's translation this frame.
    translate: i64,
}

impl Renderer {
    /// A renderer whose first frame is drawn in full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next frame a full redraw.
    pub fn invalidate(&mut self) {
        self.target = None;
    }

    /// The rectangles of the canvas changed by the last [`render`](Self::render): what to
    /// present. Moved pixels count as changed.
    pub fn damage(&self) -> &[IRect] {
        &self.damage
    }

    /// Draws `scene` over `background` (alpha ignored), redrawing only what differs from the
    /// previous frame, and takes the scene's commands, leaving it empty for the next frame.
    /// [`damage`](Self::damage) then lists the changed rectangles.
    pub fn render(
        &mut self,
        scene: &mut Scene,
        canvas: &mut Canvas,
        background: Color,
    ) -> RenderStats {
        scene.finish();
        let background = background.with_alpha(255);
        let target = Target {
            canvas: canvas.state(),
            width: canvas.width(),
            height: canvas.height(),
            background,
        };
        let bounds = canvas.bounds();
        let mut stats = RenderStats::default();
        self.repaint.clear();
        self.damage.clear();
        if self.target == Some(target) {
            self.find_blits(scene, bounds);
            self.hash_tiles(scene, bounds);
            for blit in &self.blits {
                canvas.scroll(blit.area, blit.dy);
                self.damage.push(blit.area);
                let strip = if blit.dy > 0 {
                    IRect::new(
                        blit.area.x0,
                        blit.area.y0,
                        blit.area.x1,
                        blit.area.y0 + blit.dy,
                    )
                } else {
                    IRect::new(
                        blit.area.x0,
                        blit.area.y1 + blit.dy,
                        blit.area.x1,
                        blit.area.y1,
                    )
                };
                stats.scrolled += blit.area.area() - strip.area();
                self.repaint.push(strip);
                // Whatever was drawn over the layer moved with it: redraw where it was and
                // where its pixels went.
                let source = blit.area.translate(0, blit.dy.saturating_neg());
                let (mut was, mut went) = (IRect::EMPTY, IRect::EMPTY);
                for item in &self.prev_items[self.prev_layers[blit.prev].end..] {
                    let b = item.bounds.intersect(&blit.area);
                    was = was.union(&b);
                    went = went.union(&b.intersect(&source).translate(0, blit.dy));
                }
                self.repaint
                    .extend([was, went].into_iter().filter(|r| !r.is_empty()));
            }
            self.diff_tiles(bounds);
            if self.repaint.len() > MAX_RECTS {
                let all = self.repaint.iter().fold(IRect::EMPTY, |a, r| a.union(r));
                self.repaint.clear();
                self.repaint.push(all);
            }
        } else {
            stats.full = true;
            self.repaint.push(bounds);
        }

        for &rect in &self.repaint {
            canvas.fill(rect, background);
            stats.repainted += rect.area();
            for item in &scene.items {
                let clip = item.bounds.intersect(&rect);
                if !clip.is_empty() {
                    draw(canvas, item, clip);
                    stats.draws = stats.draws.saturating_add(1);
                }
            }
        }
        self.damage.extend_from_slice(&self.repaint);
        if self.damage.len() > MAX_RECTS {
            let all = self.damage.iter().fold(IRect::EMPTY, |a, r| a.union(r));
            self.damage.clear();
            self.damage.push(all);
        }

        // Another renderer drawing into this canvas changes its state, so this one notices.
        canvas.touch();
        self.target = Some(Target {
            canvas: canvas.state(),
            ..target
        });
        std::mem::swap(&mut self.prev_items, &mut scene.items);
        std::mem::swap(&mut self.prev_layers, &mut scene.layers);
        scene.clear();
        stats
    }

    /// Layers present in both frames with the same viewport and background that moved by less
    /// than their visible height. Overlapping layers are not blitted together.
    fn find_blits(&mut self, scene: &Scene, bounds: IRect) {
        self.blits.clear();
        self.blit_of_prev.clear();
        self.blit_of_prev.resize(self.prev_layers.len(), None);
        self.blit_of_next.clear();
        self.blit_of_next.resize(scene.layers.len(), None);
        for (next, layer) in scene.layers.iter().enumerate() {
            let Some(prev) = self.prev_layers.iter().position(|p| p.id == layer.id) else {
                continue;
            };
            let old = &self.prev_layers[prev];
            if old.viewport != layer.viewport || old.background != layer.background {
                continue;
            }
            let area = layer.viewport.intersect(&bounds);
            let Ok(dy) = i32::try_from(layer.translate - old.translate) else {
                continue;
            };
            if dy == 0
                || dy.unsigned_abs() >= area.height()
                || self.blits.iter().any(|b| b.area.intersects(&area))
            {
                continue;
            }
            self.blit_of_prev[prev] = Some(self.blits.len());
            self.blit_of_next[next] = Some(self.blits.len());
            self.blits.push(Blit {
                prev,
                area,
                kept: area.intersect(&area.translate(0, dy)),
                dy,
                translate: layer.translate,
            });
        }
    }

    /// Fingerprints the tiles twice: as the canvas holds them after the blits (last frame's
    /// commands, moved with their layers) and as this frame draws them. Inside a blitted layer
    /// only the moved pixels are compared; the exposed strip is redrawn regardless, and content
    /// scrolling into it would otherwise mark whole tiles below it.
    fn hash_tiles(&mut self, scene: &Scene, bounds: IRect) {
        let (cols, rows) = tile_grid(bounds);
        let prev = self.prev_items.iter().map(|item| match item.layer {
            Some(l) if item.moves => match self.blit_of_prev[l].map(|b| &self.blits[b]) {
                Some(blit) => (
                    item.raw.translate(0, blit.dy).intersect(&blit.kept),
                    moved_key(item.key, blit.translate),
                ),
                None => (
                    item.bounds,
                    moved_key(item.key, self.prev_layers[l].translate),
                ),
            },
            _ => (item.bounds, item.key),
        });
        fingerprint(&mut self.old_tiles, cols, rows, bounds, prev);
        let next = scene.items.iter().map(|item| match item.layer {
            Some(l) if item.moves => {
                let bounds = match self.blit_of_next[l].map(|b| &self.blits[b]) {
                    Some(blit) => item.bounds.intersect(&blit.kept),
                    None => item.bounds,
                };
                (bounds, moved_key(item.key, scene.layers[l].translate))
            }
            _ => (item.bounds, item.key),
        });
        fingerprint(&mut self.new_tiles, cols, rows, bounds, next);
    }

    /// Adds the tiles whose fingerprints differ to the repaint list, merged into rectangles:
    /// runs along each tile row, each joined with an identical run in the row above. Both rows'
    /// runs are sorted by x, so one pass matches them.
    fn diff_tiles(&mut self, bounds: IRect) {
        let (cols, rows) = tile_grid(bounds);
        self.tile_rects.clear();
        self.open.clear();
        for ty in 0..rows {
            let y0 = (ty as i32) << TILE_SHIFT;
            let y1 = (y0 + TILE).min(bounds.y1);
            let row = ty * cols;
            let mut above = 0;
            self.next_open.clear();
            let mut tx = 0;
            while tx < cols {
                if self.old_tiles[row + tx] == self.new_tiles[row + tx] {
                    tx += 1;
                    continue;
                }
                let start = tx;
                while tx < cols && self.old_tiles[row + tx] != self.new_tiles[row + tx] {
                    tx += 1;
                }
                let x0 = (start as i32) << TILE_SHIFT;
                let x1 = ((tx as i32) << TILE_SHIFT).min(bounds.x1);
                while above < self.open.len() && self.tile_rects[self.open[above]].x0 < x0 {
                    above += 1;
                }
                let joined = self.open.get(above).copied().filter(|&i| {
                    let r = self.tile_rects[i];
                    r.x0 == x0 && r.x1 == x1
                });
                match joined {
                    Some(i) => {
                        self.tile_rects[i].y1 = y1;
                        self.next_open.push(i);
                        above += 1;
                    }
                    None => {
                        self.next_open.push(self.tile_rects.len());
                        self.tile_rects.push(IRect::new(x0, y0, x1, y1));
                    }
                }
            }
            std::mem::swap(&mut self.open, &mut self.next_open);
        }
        self.repaint.extend_from_slice(&self.tile_rects);
    }
}

fn tile_grid(bounds: IRect) -> (usize, usize) {
    let tiles = |n: u32| n.div_ceil(TILE as u32) as usize;
    (tiles(bounds.width()), tiles(bounds.height()))
}

/// A moving item's key with its layer's translation folded in, so the same content at another
/// scroll position fingerprints differently.
fn moved_key(key: u64, translate: i64) -> u64 {
    mix(key, translate as u64)
}

fn fingerprint(
    tiles: &mut Vec<u64>,
    cols: usize,
    rows: usize,
    bounds: IRect,
    items: impl Iterator<Item = (IRect, u64)>,
) {
    tiles.clear();
    tiles.resize(cols * rows, EMPTY_TILE);
    for (b, key) in items {
        let b = b.intersect(&bounds);
        if b.is_empty() {
            continue;
        }
        // b lies inside the canvas, so its coordinates are non-negative.
        let (tx0, tx1) = (
            (b.x0 >> TILE_SHIFT) as usize,
            ((b.x1 - 1) >> TILE_SHIFT) as usize,
        );
        let (ty0, ty1) = (
            (b.y0 >> TILE_SHIFT) as usize,
            ((b.y1 - 1) >> TILE_SHIFT) as usize,
        );
        for ty in ty0..=ty1 {
            for tile in &mut tiles[ty * cols + tx0..=ty * cols + tx1] {
                *tile = mix(*tile, key);
            }
        }
    }
}

fn draw(canvas: &mut Canvas, item: &Item, clip: IRect) {
    let mut px = canvas.raw();
    match &item.cmd {
        // The item's bounds are exactly the cleared pixels.
        Cmd::Clear { color, .. } => raster::clear(&mut px, clip, *color),
        Cmd::Fill { rect, radii, color } => {
            raster::fill(&mut px, clip, &Shape::new(*rect, *radii), *color);
        }
        Cmd::Stroke {
            rect,
            radii,
            width,
            color,
        } => {
            let outer = Shape::new(*rect, *radii);
            match outer.inset(*width) {
                Some(inner) => raster::stroke(&mut px, clip, &outer, &inner, *color),
                None => raster::fill(&mut px, clip, &outer, *color),
            }
        }
        Cmd::Mask { mask, x, y, color } => raster::mask(&mut px, clip, mask, *x, *y, *color),
        Cmd::Image { image, rect, radii } => {
            raster::image(&mut px, clip, image, &Shape::new(*rect, *radii));
        }
    }
}
