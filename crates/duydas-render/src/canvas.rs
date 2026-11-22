//! The pixel buffer the renderer draws into.

use crate::color::Color;
use crate::geom::IRect;
use crate::next_id;
use crate::raster::Pixels;

/// Largest width or height of a canvas, image or mask, in pixels. Keeps every pixel index and
/// byte count far from overflow on 32-bit targets.
pub const MAX_DIMENSION: u32 = 16_384;

/// Opaque `0x00RRGGBB` pixels, rows top to bottom: the layout `duydas_sys::window::Frame`
/// presents without conversion.
///
/// A [`Renderer`](crate::Renderer) only redraws what changed, so it must know the canvas still
/// holds its last frame. Every canvas has an identity and counts changes made outside the
/// renderer; when either differs from what the renderer saw, it redraws everything.
#[derive(Debug)]
pub struct Canvas {
    id: u64,
    generation: u64,
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Canvas {
    /// A canvas filled with `background` (alpha ignored). None if a side is 0 or larger than
    /// [`MAX_DIMENSION`].
    pub fn new(width: u32, height: u32, background: Color) -> Option<Self> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return None;
        }
        let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        Some(Self {
            id: next_id(),
            generation: 0,
            width,
            height,
            pixels: vec![background.opaque_pixel(); count],
        })
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Every pixel of the canvas.
    pub const fn bounds(&self) -> IRect {
        IRect::from_size(0, 0, self.width, self.height)
    }

    /// The pixels.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// One pixel, or None outside the canvas.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        (x < self.width && y < self.height)
            .then(|| self.pixels[y as usize * self.width as usize + x as usize])
    }

    /// Fills the part of `rect` inside the canvas with an opaque color. The next render
    /// redraws the whole canvas.
    pub fn clear(&mut self, rect: IRect, color: Color) {
        self.generation += 1;
        self.fill(rect, color);
    }

    /// (identity, revision): what a renderer compares to trust the pixels.
    pub(crate) const fn state(&self) -> (u64, u64) {
        (self.id, self.generation)
    }

    /// Marks the pixels as changed by a renderer.
    pub(crate) const fn touch(&mut self) {
        self.generation += 1;
    }

    pub(crate) fn fill(&mut self, rect: IRect, color: Color) {
        let r = rect.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        let value = color.opaque_pixel();
        let width = self.width as usize;
        for y in r.y0 as usize..r.y1 as usize {
            self.pixels[y * width + r.x0 as usize..y * width + r.x1 as usize].fill(value);
        }
    }

    pub(crate) fn raw(&mut self) -> Pixels<'_> {
        Pixels {
            data: &mut self.pixels,
            stride: self.width as usize,
        }
    }

    /// Moves the pixels of `area` (inside the canvas) down by dy rows, or up for negative dy.
    /// Rows moved in from outside the area keep their old pixels.
    pub(crate) fn scroll(&mut self, area: IRect, dy: i32) {
        let area = area.intersect(&self.bounds());
        if area.is_empty() || dy == 0 || dy.unsigned_abs() >= area.height() {
            return;
        }
        let width = self.width as usize;
        let (x0, x1) = (area.x0 as usize, area.x1 as usize);
        let copy = |pixels: &mut Vec<u32>, to: i32| {
            let from = (to - dy) as usize * width;
            let to = to as usize * width;
            pixels.copy_within(from + x0..from + x1, to + x0);
        };
        if dy > 0 {
            for y in (area.y0 + dy..area.y1).rev() {
                copy(&mut self.pixels, y);
            }
        } else {
            for y in area.y0..area.y1 + dy {
                copy(&mut self.pixels, y);
            }
        }
    }
}

/// Canvases are equal when their pixels are.
impl PartialEq for Canvas {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.pixels == other.pixels
    }
}

impl Eq for Canvas {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_clear() {
        assert!(Canvas::new(0, 1, Color::BLACK).is_none());
        assert!(Canvas::new(MAX_DIMENSION + 1, 1, Color::BLACK).is_none());
        let mut c = Canvas::new(4, 3, Color::hex(0x10_2030)).expect("canvas");
        assert_eq!(c.pixel(3, 2), Some(0x0010_2030));
        let before = c.state();
        c.clear(IRect::new(-5, 1, 2, 9), Color::WHITE);
        assert_ne!(c.state(), before, "outside changes are counted");
        assert_eq!(c.pixel(1, 1), Some(0x00FF_FFFF));
        assert_eq!(c.pixel(2, 1), Some(0x0010_2030));
        assert_eq!(c.pixel(0, 0), Some(0x0010_2030));
        assert_eq!(c.pixel(4, 0), None);
    }

    #[test]
    fn scrolling_moves_only_the_area() {
        let mut c = Canvas::new(3, 4, Color::BLACK).expect("canvas");
        for (i, p) in c.pixels.iter_mut().enumerate() {
            *p = i as u32;
        }
        c.scroll(IRect::new(1, 0, 3, 4), 1);
        assert_eq!(c.pixels, [0, 1, 2, 3, 1, 2, 6, 4, 5, 9, 7, 8]);
        c.scroll(IRect::new(1, 0, 3, 4), -2);
        assert_eq!(c.pixels, [0, 4, 5, 3, 7, 8, 6, 4, 5, 9, 7, 8]);
        // A move as tall as the area does nothing: nothing of the old content stays visible.
        let before = c.pixels.clone();
        c.scroll(IRect::new(0, 0, 3, 4), 4);
        assert_eq!(c.pixels, before);
    }
}
