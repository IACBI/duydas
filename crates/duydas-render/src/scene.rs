//! The display list: what one frame draws, in order.

use std::sync::Arc;

use crate::color::Color;
use crate::geom::{FRect, IRect, LRect, Radii, Rect, fixed};
use crate::hash::Hasher;
use crate::image::{Image, Mask};

/// A region whose content scrolls vertically. See [~~Scene::push_scroll_layer~~].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollLayer {
    /// Identifies the layer from frame to frame; a frame should not reuse an id.
    pub id: u64,
    /// The visible region, in the coordinates of the enclosing content.
    pub viewport: IRect,
    /// Content row shown at the top of the viewport. Content coordinates may be negative: a
    /// chat history anchored at its newest message can count upward from 0 so that prepending
    /// older messages changes neither coordinates nor offset.
    pub offset: i32,
    /// Fills the viewport before the content. Only a layer with an opaque background can be
    /// scrolled by moving pixels, because then nothing behind it shows through.
    pub background: Color,
}

#[derive(Debug, Clone)]
pub(crate) enum Cmd {
    /// Whole pixels, no anti-aliasing: scroll layer backgrounds.
    Clear { rect: LRect, color: u32 },
    Fill {
        rect: FRect,
        radii: [i64; 4],
        color: u32,
    },
    Stroke {
        rect: FRect,
        radii: [i64; 4],
        width: i64,
        color: u32,
    },
    Mask {
        mask: Arc<Mask>,
        x: i64,
        y: i64,
        color: u32,
    },
    Image {
        image: Arc<Image>,
        rect: FRect,
        radii: [i64; 4],
    },
}

/// One recorded draw call.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub(crate) cmd: Cmd,
    /// The scroll layer whose content this item is, as an index into ~~Scene::layers~~.
    pub(crate) layer: Option<usize>,
    /// Whether the item moves when its layer scrolls (false for the layer's background).
    pub(crate) moves: bool,
    /// Pixels the item may touch, ignoring its layer's viewport.
    pub(crate) raw: IRect,
    /// Pixels the item may touch: ~~raw~~ inside the viewport.
    pub(crate) bounds: IRect,
    /// Fingerprint of the command and its clip. For moving items it is taken in the layer's
    /// content coordinates, so scrolling leaves it unchanged.
    pub(crate) key: u64,
}

/// A scroll layer whose pixels the renderer may move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layer {
    pub(crate) id: u64,
    /// The viewport on the canvas, clipped by everything enclosing the layer.
    pub(crate) viewport: IRect,
    /// Content y + translate = canvas y.
    pub(crate) translate: i64,
    pub(crate) background: Color,
    /// Items start..end belong to the layer.
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Scroll translations beyond this many pixels put content out of any canvas's reach; capping
/// them keeps every sum the scene forms far from overflow, however deeply layers nest.
const MAX_TRANSLATE: i64 = 1 << 40;

#[derive(Debug, Clone, Copy)]
struct State {
    /// The clip on the canvas; inside a movable layer, only the clips pushed within it.
    clip: Option<LRect>,
    /// Added to y coordinates to reach the canvas.
    translate: i64,
    /// The movable layer being recorded.
    layer: Option<usize>,
    /// Nothing recorded now can be visible (an empty clip or an unreachable translation).
    hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pushed {
    Clip,
    Layer,
}

/// The draw calls of one frame, in painting order (later calls draw over earlier ones).
///
/// Coordinates are canvas pixels, except inside a scroll layer, where they are content
/// coordinates. Geometry farther than 10^9 pixels from the origin is not drawn. Hand the scene
/// to [~~Renderer::render~~](crate::Renderer::render) once per frame; it takes the commands and
/// leaves the scene empty, keeping its allocations for the next frame.
#[derive(Debug, Default)]
pub struct Scene {
    pub(crate) items: Vec<Item>,
    pub(crate) layers: Vec<Layer>,
    state: Option<State>,
    stack: Vec<(Pushed, Option<State>)>,
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of recorded draw calls (calls that cannot draw anything are not recorded).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Forgets everything recorded.
    pub fn clear(&mut self) {
        self.items.clear();
        self.layers.clear();
        self.state = None;
        self.stack.clear();
    }

    fn current(&self) -> State {
        self.state.unwrap_or(State {
            clip: None,
            translate: 0,
            layer: None,
            hidden: false,
        })
    }

    /// Restricts the following calls to ~~rect~~ (intersected with the current clip) until the
    /// matching [~~pop_clip~~](Self::pop_clip).
    pub fn push_clip(&mut self, rect: IRect) {
        let state = self.current();
        let rect = LRect::from(rect).translate(state.translate);
        let clip = state.clip.map_or(rect, |c| c.intersect(&rect));
        self.stack.push((Pushed::Clip, self.state));
        self.state = Some(State {
            clip: Some(clip),
            hidden: state.hidden || clip.is_empty(),
            ..state
        });
    }

    /// Ends the innermost [~~push_clip~~](Self::push_clip). Ignored when the innermost push was
    /// a scroll layer or there is none.
    pub fn pop_clip(&mut self) {
        if let Some(&(Pushed::Clip, saved)) = self.stack.last() {
            self.stack.pop();
            self.state = saved;
        }
    }

    /// Starts a scroll layer: fills its viewport with its background, then records the
    /// following calls in content coordinates, clipped to the viewport, until the matching
    /// [~~pop_scroll_layer~~](Self::pop_scroll_layer).
    ///
    /// When the same layer scrolls by less than its height between frames, the renderer moves
    /// its pixels and only draws the newly exposed rows (plus anything else that changed). That
    /// needs an opaque background, a layer not nested in another one, and an id not used
    /// earlier in the frame; other layers are drawn like a clip and an offset.
    pub fn push_scroll_layer(&mut self, layer: ScrollLayer) {
        let state = self.current();
        self.stack.push((Pushed::Layer, self.state));
        let viewport = LRect::from(layer.viewport).translate(state.translate);
        let translate = viewport.y0 - i64::from(layer.offset);
        let visible = state.clip.map_or(viewport, |c| c.intersect(&viewport));
        if state.hidden || visible.is_empty() || translate.abs() > MAX_TRANSLATE {
            self.state = Some(State {
                hidden: true,
                ..state
            });
            return;
        }
        let movable = state.layer.is_none()
            && layer.background.a == 255
            && !self.layers.iter().any(|l| l.id == layer.id);
        if movable {
            let index = self.layers.len();
            let start = self.items.len();
            let bounds = visible.clamped();
            self.layers.push(Layer {
                id: layer.id,
                viewport: bounds,
                translate,
                background: layer.background,
                start,
                end: start,
            });
            let cmd = Cmd::Clear {
                rect: visible,
                color: layer.background.premultiplied(),
            };
            if !bounds.is_empty() {
                self.items.push(Item {
                    key: key(&cmd, None, 0),
                    cmd,
                    layer: Some(index),
                    moves: false,
                    raw: bounds,
                    bounds,
                });
            }
            self.state = Some(State {
                clip: None,
                translate,
                layer: Some(index),
                hidden: false,
            });
        } else {
            if layer.background.a > 0 {
                let cmd = Cmd::Clear {
                    rect: visible,
                    color: layer.background.premultiplied(),
                };
                self.record(cmd, visible);
            }
            self.state = Some(State {
                clip: Some(visible),
                translate,
                layer: state.layer,
                hidden: false,
            });
        }
    }

    /// Ends the innermost [~~push_scroll_layer~~](Self::push_scroll_layer). Ignored when the
    /// innermost push was a clip or there is none.
    pub fn pop_scroll_layer(&mut self) {
        if let Some(&(Pushed::Layer, saved)) = self.stack.last() {
            self.stack.pop();
            self.close_layer(saved);
            self.state = saved;
        }
    }

    fn close_layer(&mut self, outer: Option<State>) {
        let inner = self.current().layer;
        let outer = outer.and_then(|s| s.layer);
        if let Some(index) = inner
            && inner != outer
        {
            self.layers[index].end = self.items.len();
        }
    }

    /// Closes layers left open, so the scene can be rendered.
    pub(crate) fn finish(&mut self) {
        while let Some((_, saved)) = self.stack.pop() {
            self.close_layer(saved);
            self.state = saved;
        }
    }

    /// Fills a rectangle; fractional edges are anti-aliased.
    pub fn fill(&mut self, rect: Rect, color: Color) {
        self.fill_rounded(rect, Radii::ZERO, color);
    }

    /// Fills a rounded rectangle.
    pub fn fill_rounded(&mut self, rect: Rect, radii: Radii, color: Color) {
        if color.a == 0 {
            return;
        }
        let Some(rect) = self.place(rect) else { return };
        let cmd = Cmd::Fill {
            rect,
            radii: fixed_radii(radii),
            color: color.premultiplied(),
        };
        self.record(cmd, rect.pixel_bounds());
    }

    /// Draws a border of ~~width~~ pixels just inside a rounded rectangle; the inner corners
    /// are rounded by the radii minus the width.
    pub fn stroke_rounded(&mut self, rect: Rect, radii: Radii, width: f32, color: Color) {
        let width = if width.is_finite() {
            fixed(width.into())
        } else {
            0
        };
        if color.a == 0 || width <= 0 {
            return;
        }
        let Some(rect) = self.place(rect) else { return };
        let cmd = Cmd::Stroke {
            rect,
            radii: fixed_radii(radii),
            width,
            color: color.premultiplied(),
        };
        self.record(cmd, rect.pixel_bounds());
    }

    /// Paints ~~color~~ through a coverage mask (a glyph or icon) with its top-left pixel at
    /// (x, y). Masks are placed on whole pixels; the text stack bakes subpixel positions into
    /// the mask.
    pub fn draw_mask(&mut self, mask: &Arc<Mask>, x: i32, y: i32, color: Color) {
        let state = self.current();
        if color.a == 0 || state.hidden {
            return;
        }
        let (x, y) = (i64::from(x), i64::from(y) + state.translate);
        let bounds = LRect::from_size(x, y, mask.width(), mask.height());
        let cmd = Cmd::Mask {
            mask: Arc::clone(mask),
            x,
            y,
            color: color.premultiplied(),
        };
        self.record(cmd, bounds);
    }

    /// Draws an image stretched over ~~rect~~ with rounded corners (radii of half the size
    /// make a circle). Drawn at its own size on whole pixels, an image is copied texel for
    /// texel; otherwise it is filtered bilinearly, so shrink large images with
    /// [~~Image::resized~~] first.
    pub fn draw_image(&mut self, image: &Arc<Image>, rect: Rect, radii: Radii) {
        let Some(rect) = self.place(rect) else { return };
        let cmd = Cmd::Image {
            image: Arc::clone(image),
            rect,
            radii: fixed_radii(radii),
        };
        self.record(cmd, rect.pixel_bounds());
    }

    fn place(&self, rect: Rect) -> Option<FRect> {
        let state = self.current();
        if state.hidden {
            return None;
        }
        Some(rect.to_fixed()?.translate(state.translate))
    }

    fn record(&mut self, cmd: Cmd, bounds: LRect) {
        let state = self.current();
        if state.hidden {
            return;
        }
        let raw = state
            .clip
            .map_or(bounds, |c| c.intersect(&bounds))
            .clamped();
        let (bounds, local) = match state.layer {
            Some(index) => {
                let layer = &self.layers[index];
                (raw.intersect(&layer.viewport), layer.translate)
            }
            None => (raw, 0),
        };
        if bounds.is_empty() {
            return;
        }
        self.items.push(Item {
            key: key(&cmd, state.clip, local),
            cmd,
            layer: state.layer,
            moves: true,
            raw,
            bounds,
        });
    }
}

fn fixed_radii(radii: Radii) -> [i64; 4] {
    radii.sanitized().map(|r| fixed(r.into()))
}

/// Fingerprint of a command and its clip, with y coordinates moved up by ~~dy~~ pixels. The
/// arithmetic is exact, so equal fingerprints mean equal geometry relative to the layer.
fn key(cmd: &Cmd, clip: Option<LRect>, dy: i64) -> u64 {
    let mut h = Hasher::new(0x9E37_79B9_7F4A_7C15);
    let rect = |h: &mut Hasher, r: FRect| {
        let r = r.translate(-dy);
        for v in [r.x0, r.y0, r.x1, r.y1] {
            h.write(v as u64);
        }
    };
    let pixels = |h: &mut Hasher, r: LRect| {
        let r = r.translate(-dy);
        for v in [r.x0, r.y0, r.x1, r.y1] {
            h.write(v as u64);
        }
    };
    let radii = |h: &mut Hasher, radii: &[i64; 4]| radii.iter().for_each(|&v| h.write(v as u64));
    match cmd {
        Cmd::Clear { rect: r, color } => {
            h.write(0);
            pixels(&mut h, *r);
            h.write(u64::from(*color));
        }
        Cmd::Fill {
            rect: r,
            radii: rr,
            color,
        } => {
            h.write(1);
            rect(&mut h, *r);
            radii(&mut h, rr);
            h.write(u64::from(*color));
        }
        Cmd::Stroke {
            rect: r,
            radii: rr,
            width,
            color,
        } => {
            h.write(2);
            rect(&mut h, *r);
            radii(&mut h, rr);
            h.write(*width as u64);
            h.write(u64::from(*color));
        }
        Cmd::Mask { mask, x, y, color } => {
            h.write(3);
            h.write(mask.id());
            h.write(*x as u64);
            h.write((y - dy) as u64);
            h.write(u64::from(*color));
        }
        Cmd::Image {
            image,
            rect: r,
            radii: rr,
        } => {
            h.write(4);
            h.write(image.id());
            rect(&mut h, *r);
            radii(&mut h, rr);
        }
    }
    match clip {
        Some(c) => {
            h.write(1);
            pixels(&mut h, c);
        }
        None => h.write(0),
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: u64, offset: i32) -> ScrollLayer {
        ScrollLayer {
            id,
            viewport: IRect::new(10, 20, 110, 220),
            offset,
            background: Color::hex(0x202020),
        }
    }

    #[test]
    fn invisible_and_invalid_calls_are_not_recorded() {
        let mut s = Scene::new();
        s.fill(Rect::new(0.0, 0.0, 10.0, 10.0), Color::TRANSPARENT);
        s.fill(Rect::new(0.0, 0.0, f32::NAN, 10.0), Color::WHITE);
        s.stroke_rounded(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Radii::ZERO,
            0.0,
            Color::WHITE,
        );
        s.push_clip(IRect::new(0, 0, 5, 5));
        s.fill(Rect::new(6.0, 6.0, 10.0, 10.0), Color::WHITE);
        s.pop_clip();
        assert!(s.is_empty());
    }

    #[test]
    fn layer_content_is_translated_and_keyed_in_content_coordinates() {
        let mut keys = Vec::new();
        for offset in [0, 30] {
            let mut s = Scene::new();
            s.push_scroll_layer(layer(7, offset));
            s.fill(Rect::new(10.0, 40.0, 50.0, 20.0), Color::WHITE);
            s.pop_scroll_layer();
            s.finish();
            assert_eq!(s.layers.len(), 1);
            assert_eq!(s.layers[0].end, 2, "background and one fill");
            let item = &s.items[1];
            assert_eq!(item.bounds, IRect::new(10, 60 - offset, 60, 80 - offset));
            assert!(item.moves);
            keys.push(item.key);
        }
        assert_eq!(keys[0], keys[1], "scrolling keeps content keys");
    }

    #[test]
    fn only_eligible_layers_can_move() {
        let mut s = Scene::new();
        s.push_scroll_layer(layer(1, 0));
        s.push_scroll_layer(layer(2, 5)); // nested: drawn as a clip
        s.pop_scroll_layer();
        s.pop_scroll_layer();
        s.push_scroll_layer(ScrollLayer {
            background: Color::rgba(0, 0, 0, 128),
            ..layer(3, 0)
        });
        s.pop_scroll_layer();
        s.push_scroll_layer(layer(1, 0)); // reused id
        s.finish();
        let ids: Vec<u64> = s.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [1]);
        // The nested layer's background moves with the outer layer's content.
        assert!(s.items[1].moves && s.items[1].layer == Some(0));
    }

    #[test]
    fn deep_nesting_with_huge_offsets_cannot_overflow() {
        let mut s = Scene::new();
        for _ in 0..2_000 {
            s.push_scroll_layer(ScrollLayer {
                id: 5,
                viewport: IRect::new(0, i32::MAX - 1, 1, i32::MAX),
                offset: i32::MIN,
                background: Color::TRANSPARENT,
            });
        }
        s.fill(Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE);
        s.draw_mask(
            &Arc::new(Mask::new(1, 1, vec![255]).expect("mask")),
            0,
            0,
            Color::WHITE,
        );
        assert!(s.is_empty());
        for _ in 0..2_000 {
            s.pop_scroll_layer();
        }
        s.fill(Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE);
        assert_eq!(s.len(), 1, "popping restores the visible state");
    }

    #[test]
    fn unbalanced_pops_are_ignored() {
        let mut s = Scene::new();
        s.pop_clip();
        s.pop_scroll_layer();
        s.push_clip(IRect::new(0, 0, 10, 10));
        s.pop_scroll_layer(); // innermost is a clip: ignored
        s.fill(Rect::new(20.0, 0.0, 5.0, 5.0), Color::WHITE);
        assert!(s.is_empty(), "the clip is still active");
        s.pop_clip();
        s.fill(Rect::new(20.0, 0.0, 5.0, 5.0), Color::WHITE);
        assert_eq!(s.len(), 1);
    }
}
