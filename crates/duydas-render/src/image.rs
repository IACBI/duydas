//! Images (premultiplied RGBA) and coverage masks (glyphs, icons).

use std::fmt;

use crate::canvas::MAX_DIMENSION;
use crate::color::Color;
use crate::next_id;

fn pixel_count(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return None;
    }
    usize::try_from(u64::from(width) * u64::from(height)).ok()
}

/// A premultiplied-alpha image: one `u32` per pixel as `0xAARRGGBB`, rows top to bottom.
///
/// Images are immutable. Each one carries an identity the renderer uses to tell whether a draw
/// call shows the same pixels as last frame; clones share it, which is sound because neither
/// can change.
#[derive(Clone)]
pub struct Image {
    id: u64,
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Image {
    /// From straight-alpha RGBA bytes, four per pixel. None if a side is 0 or larger than
    /// [`MAX_DIMENSION`](crate::MAX_DIMENSION), or the length does not match.
    pub fn from_rgba8(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        let count = pixel_count(width, height)?;
        if rgba.len() != count.checked_mul(4)? {
            return None;
        }
        let pixels = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Color::rgba(r, g, b, a).premultiplied())
            .collect();
        Some(Self {
            id: next_id(),
            width,
            height,
            pixels,
        })
    }

    /// From premultiplied `0xAARRGGBB` pixels. Channels above alpha are clamped to it, so a
    /// malformed image cannot corrupt neighboring channels when blended.
    pub fn from_premultiplied(width: u32, height: u32, mut pixels: Vec<u32>) -> Option<Self> {
        if pixels.len() != pixel_count(width, height)? {
            return None;
        }
        for p in &mut pixels {
            *p = clamp_premultiplied(*p);
        }
        Some(Self {
            id: next_id(),
            width,
            height,
            pixels,
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

    /// The premultiplied pixels.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    /// A copy resampled by area averaging: each output pixel is the mean of the source area it
    /// covers, rounded once. Use it to shrink images ahead of time (avatars decoded larger than
    /// shown): drawing scales with bilinear filtering, which aliases below half size.
    ///
    /// The arithmetic is exact integer math, so the result does not depend on the order the two
    /// axes are resampled in; the cheaper order is used. Scratch memory is a few rows (or
    /// columns) of the output.
    pub fn resized(&self, width: u32, height: u32) -> Option<Self> {
        let count = pixel_count(width, height)?;
        let (sw, sh, tw, th) = (
            u64::from(self.width),
            u64::from(self.height),
            u64::from(width),
            u64::from(height),
        );
        let mut pixels = vec![0; count];
        // Rows first costs about sh × (sw + tw) operations, columns first sw × (sh + th).
        let columns_first = sw * (sh + th) < sh * (sw + tw);
        let (src_w, src_h) = (self.width as usize, self.height as usize);
        let (dst_w, dst_h) = (width as usize, height as usize);
        if columns_first {
            // Resample the transposed image without materializing it: its rows are our columns.
            let lines = Lines {
                count: src_w,
                len: src_h,
                at: &|line, i| self.pixels[i * src_w + line],
            };
            resample(&lines, dst_w, dst_h, &mut |line, i, p| {
                pixels[i * dst_w + line] = p
            });
        } else {
            let lines = Lines {
                count: src_h,
                len: src_w,
                at: &|line, i| self.pixels[line * src_w + i],
            };
            resample(&lines, dst_h, dst_w, &mut |line, i, p| {
                pixels[line * dst_w + i] = p
            });
        }
        Some(Self {
            id: next_id(),
            width,
            height,
            pixels,
        })
    }
}

/// An image seen as ~~count~~ lines of ~~len~~ pixels (rows, or columns of the transposed view).
struct Lines<'a> {
    count: usize,
    len: usize,
    at: &'a dyn Fn(usize, usize) -> u32,
}

/// Area-averages ~~src~~ to ~~lines~~ lines of ~~len~~ pixels: first along each source line,
/// then across lines. Sums stay integers until the single final division.
fn resample(src: &Lines<'_>, lines: usize, len: usize, out: &mut dyn FnMut(usize, usize, u32)) {
    let along = weights(src.len, len);
    let across = weights(src.count, lines);
    // Lines resampled along their length. Consecutive output lines share at most two source
    // lines when enlarging and one when shrinking, so two slots avoid recomputing them.
    let mut cache = [
        (usize::MAX, vec![[0_u64; 4]; len]),
        (usize::MAX, vec![[0_u64; 4]; len]),
    ];
    let mut victim = 0;
    let mut acc = vec![[0_u64; 4]; len];
    // Every output pixel's weights sum to src.len × src.count.
    let total = src.len as u64 * src.count as u64;
    for (line, (first, ws)) in across.iter().enumerate() {
        acc.fill([0; 4]);
        for (k, &w) in ws.iter().enumerate() {
            let source = first + k;
            let slot = match cache.iter().position(|(index, _)| *index == source) {
                Some(slot) => slot,
                None => {
                    let slot = victim;
                    victim ^= 1;
                    let (index, row) = &mut cache[slot];
                    *index = source;
                    for (o, (start, cws)) in row.iter_mut().zip(&along) {
                        *o = [0; 4];
                        for (j, &cw) in cws.iter().enumerate() {
                            add(o, (src.at)(source, start + j), cw);
                        }
                    }
                    slot
                }
            };
            for (a, h) in acc.iter_mut().zip(&cache[slot].1) {
                for (a, h) in a.iter_mut().zip(h) {
                    *a += h * w;
                }
            }
        }
        for (i, a) in acc.iter().enumerate() {
            out(line, i, pack(*a, total));
        }
    }
}

/// For each of ~~target~~ output pixels: the first source pixel it covers and the integer
/// overlap of each source pixel from there on, in units where a source pixel is ~~target~~ long
/// and an output pixel ~~source~~ long (so each output pixel's weights sum to ~~source~~).
fn weights(source: usize, target: usize) -> Vec<(usize, Vec<u64>)> {
    let (s, t) = (source as u64, target as u64);
    (0..t)
        .map(|j| {
            let (lo, hi) = (j * s, (j + 1) * s);
            let first = lo / t;
            let last = hi.div_ceil(t).min(s);
            let ws = (first..last)
                .map(|i| hi.min((i + 1) * t) - lo.max(i * t))
                .collect();
            (first as usize, ws)
        })
        .collect()
}

fn add(acc: &mut [u64; 4], p: u32, weight: u64) {
    for (a, shift) in acc.iter_mut().zip([24, 16, 8, 0]) {
        *a += u64::from((p >> shift) & 0xFF) * weight;
    }
}

fn pack(acc: [u64; 4], total: u64) -> u32 {
    let [a, r, g, b] = acc.map(|v| ((v + total / 2) / total).min(255) as u32);
    clamp_premultiplied((a << 24) | (r << 16) | (g << 8) | b)
}

fn clamp_premultiplied(p: u32) -> u32 {
    let a = p >> 24;
    let channel = |shift: u32| ((p >> shift) & 0xFF).min(a) << shift;
    (a << 24) | channel(16) | channel(8) | channel(0)
}

/// An 8-bit coverage mask: 0 transparent, 255 fully covered. Glyphs and single-color icons.
///
/// Immutable, with an identity like [`Image`]'s.
#[derive(Clone)]
pub struct Mask {
    id: u64,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Mask {
    /// A mask from rows of coverage values, top to bottom. None if a side is 0 or larger than
    /// [`MAX_DIMENSION`](crate::MAX_DIMENSION), or the length does not match.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        (data.len() == pixel_count(width, height)?).then(|| Self {
            id: next_id(),
            width,
            height,
            data,
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

    /// The coverage values.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) const fn id(&self) -> u64 {
        self.id
    }
}

// Pixel data is left out of Debug: a glyph cache would print megabytes.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Mask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mask")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Images are equal when their pixels are, whatever their identity.
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.pixels == other.pixels
    }
}

impl Eq for Image {}

/// Masks are equal when their coverage is, whatever their identity.
impl PartialEq for Mask {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.data == other.data
    }
}

impl Eq for Mask {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_validates_sizes() {
        assert!(Image::from_rgba8(2, 2, &[0; 16]).is_some());
        assert!(Image::from_rgba8(2, 2, &[0; 15]).is_none());
        assert!(Image::from_rgba8(0, 2, &[]).is_none());
        assert!(Image::from_rgba8(MAX_DIMENSION + 1, 1, &[]).is_none());
        assert!(Mask::new(3, 1, vec![0; 3]).is_some());
        assert!(Mask::new(3, 1, vec![0; 4]).is_none());
    }

    #[test]
    fn premultiplied_input_is_clamped() {
        let image = Image::from_premultiplied(1, 1, vec![0x40FF_8020]).expect("image");
        assert_eq!(image.pixels(), &[0x4040_4020]);
    }

    #[test]
    fn resizing_thin_images_needs_little_memory() {
        // 1 × 16384 to 16384 × 1: a full-size intermediate would be 4 GiB.
        let tall =
            Image::from_premultiplied(1, MAX_DIMENSION, vec![0xFF10_2030; MAX_DIMENSION as usize])
                .expect("image");
        let wide = tall.resized(MAX_DIMENSION, 1).expect("resized");
        assert!(wide.pixels().iter().all(|&p| p == 0xFF10_2030));
        let back = wide.resized(1, MAX_DIMENSION).expect("resized");
        assert_eq!(back, tall);
    }

    #[test]
    fn resizing_rounds_exactly_whatever_the_axis_order() {
        // Red 44 at (1, 0) of a black 4 × 6 image: output (1, 0) of 5 × 5 averages to exactly
        // 27.5, which rounds to 28.
        let mut pixels = vec![0xFF00_0000; 24];
        pixels[1] = 0xFF2C_0000;
        let image = Image::from_premultiplied(4, 6, pixels).expect("image");
        let out = image.resized(5, 5).expect("resized");
        assert_eq!(out.pixels()[1], 0xFF1C_0000);
    }

    #[test]
    fn area_averaging_preserves_mean_and_flat_color() {
        let flat = Image::from_premultiplied(9, 7, vec![0xFF20_4060; 63]).expect("image");
        let small = flat.resized(4, 3).expect("resized");
        assert!(small.pixels().iter().all(|&p| p == 0xFF20_4060));

        // Alternating black and white columns average to mid gray at half width.
        let stripes: Vec<u32> = (0..16)
            .map(|i| if i % 2 == 0 { 0xFF00_0000 } else { 0xFFFF_FFFF })
            .collect();
        let image = Image::from_premultiplied(8, 2, stripes).expect("image");
        let half = image.resized(4, 1).expect("resized");
        for &p in half.pixels() {
            assert_eq!(p, 0xFF80_8080, "{p:#010x}");
        }
    }
}
