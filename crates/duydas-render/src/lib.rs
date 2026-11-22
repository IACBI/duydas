//! Duydaş CPU renderer (ADR 0024).
//!
//! Draws rectangles, rounded rectangles, borders, coverage masks (glyphs, icons) and images
//! into a [`Canvas`] of opaque pixels that the platform layer presents as is. Frames are
//! recorded into a [`Scene`]; the [`Renderer`] redraws only the parts of the canvas whose
//! commands changed since the previous frame and scrolls [`ScrollLayer`]s by moving pixels, so
//! a typical frame touches a small fraction of the window.
//!
//! ```
//! use duydas_render::{Canvas, Color, IRect, Radii, Rect, Renderer, Scene, ScrollLayer};
//!
//! let mut canvas = Canvas::new(320, 240, Color::BLACK).expect("valid size");
//! let mut renderer = Renderer::new();
//! let mut scene = Scene::new();
//! for offset in [0, 24] {
//!     scene.push_scroll_layer(ScrollLayer {
//!         id: 1,
//!         viewport: IRect::new(0, 0, 320, 200),
//!         offset,
//!         background: Color::hex(0x1E1F22),
//!     });
//!     for i in 0..20 {
//!         let y = i as f32 * 48.0;
//!         let card = Rect::new(8.0, y + 4.0, 304.0, 40.0);
//!         scene.fill_rounded(card, Radii::all(6.0), Color::hex(0x2B2D31));
//!     }
//!     scene.pop_scroll_layer();
//!     let stats = renderer.render(&mut scene, &mut canvas, Color::BLACK);
//!     // The second frame moves pixels and draws only the 24 rows scrolled into view.
//!     let drawn = if offset == 0 { 320 * 240 } else { 320 * 24 };
//!     assert_eq!(stats.repainted, drawn);
//! }
//! ```

mod canvas;
mod color;
mod geom;
mod hash;
mod image;
mod raster;
mod renderer;
mod scene;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicU64, Ordering};

pub use canvas::{Canvas, MAX_DIMENSION};
pub use color::Color;
pub use geom::{IRect, Radii, Rect};
pub use image::{Image, Mask};
pub use renderer::{RenderStats, Renderer};
pub use scene::{Scene, ScrollLayer};

/// A process-unique identity for canvases, images and masks.
pub(crate) fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
