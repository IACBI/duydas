//! Colors and the pixel arithmetic every drawing routine shares.
//!
//! Pixels are `0xAARRGGBB` with premultiplied alpha while drawing, and the canvas stores opaque
//! `0x00RRGGBB`. Blending happens on sRGB values, as browsers and design tools do, so colors
//! match the designs they come from.

/// An sRGB color with straight (not premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha: 0 transparent, 255 opaque.
    pub a: u8,
}

impl Color {
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// A color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque color from `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// The same color with another alpha.
    #[must_use]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self::rgba(self.r, self.g, self.b, a)
    }

    pub(crate) const fn premultiplied(self) -> u32 {
        let a = self.a as u32;
        (a << 24)
            | (div255(self.r as u32 * a) << 16)
            | (div255(self.g as u32 * a) << 8)
            | div255(self.b as u32 * a)
    }

    /// The color as an opaque canvas pixel, ignoring alpha.
    pub(crate) const fn opaque_pixel(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }
}

/// x / 255, rounded to nearest, for x ≤ 255 × 255.
pub(crate) const fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}

/// Multiplies each 8-bit channel of p by a / 255, rounded; two channels per multiply.
#[inline]
pub(crate) const fn scale(p: u32, a: u32) -> u32 {
    let rb = (p & 0x00FF_00FF) * a + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
    let ag = ((p >> 8) & 0x00FF_00FF) * a + 0x0080_0080;
    let ag = (ag + ((ag >> 8) & 0x00FF_00FF)) & 0xFF00_FF00;
    rb | ag
}

/// Draws premultiplied src over an opaque destination pixel. No channel can carry into the
/// next, because premultiplied channels never exceed alpha.
#[inline]
pub(crate) const fn over(dst: u32, src: u32) -> u32 {
    let inv = 255 - (src >> 24);
    (src + scale(dst, inv)) & 0x00FF_FFFF
}

/// Draws premultiplied src, weighted by an 8-bit coverage, over an opaque pixel.
#[inline]
pub(crate) const fn over_coverage(dst: u32, src: u32, coverage: u32) -> u32 {
    match coverage {
        0 => dst,
        255 if src >> 24 == 255 => src & 0x00FF_FFFF,
        255 => over(dst, src),
        _ => over(dst, scale(src, coverage)),
    }
}

/// Interpolates two premultiplied pixels: f = 0 gives a, f = 256 gives b. Keeps channels at or
/// below alpha, since the same rounding applies to both.
#[inline]
pub(crate) const fn lerp(a: u32, b: u32, f: u32) -> u32 {
    let g = 256 - f;
    let rb = ((a & 0x00FF_00FF) * g + (b & 0x00FF_00FF) * f + 0x0080_0080) >> 8;
    let ag = ((a >> 8) & 0x00FF_00FF) * g + ((b >> 8) & 0x00FF_00FF) * f + 0x0080_0080;
    (rb & 0x00FF_00FF) | (ag & 0xFF00_FF00)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_by_255_rounds_exactly() {
        for x in 0..=255_u32 {
            for a in 0..=255_u32 {
                let exact = (f64::from(x * a) / 255.0).round() as u32;
                assert_eq!(div255(x * a), exact, "{x} * {a}");
                let packed = x | (x << 8) | (x << 16) | (x << 24);
                let scaled = scale(packed, a);
                assert_eq!(scaled, exact * 0x0101_0101, "{x} * {a} packed");
            }
        }
    }

    #[test]
    fn source_over_matches_the_formula_and_never_carries() {
        for alpha in [0_u8, 1, 64, 128, 200, 254, 255] {
            for value in [0_u8, 1, 100, 255] {
                let src = Color::rgba(value, 255 - value, value / 2, alpha).premultiplied();
                for dst in [0x0000_0000, 0x00FF_FFFF, 0x0012_3456, 0x00FF_0000] {
                    let out = over(dst, src);
                    assert_eq!(out >> 24, 0, "the X byte stays clear");
                    for shift in [0, 8, 16] {
                        let s = (src >> shift) & 0xFF;
                        let d = (dst >> shift) & 0xFF;
                        let expected = s + div255(d * (255 - u32::from(alpha)));
                        assert_eq!((out >> shift) & 0xFF, expected);
                    }
                }
            }
        }
        assert_eq!(
            over(0x0012_3456, Color::rgb(1, 2, 3).premultiplied()),
            0x0001_0203
        );
        assert_eq!(over(0x0012_3456, 0), 0x0012_3456);
    }

    #[test]
    fn interpolation_stays_premultiplied() {
        let a = Color::rgba(255, 0, 10, 40).premultiplied();
        let b = Color::rgba(0, 255, 255, 255).premultiplied();
        for f in 0..=256 {
            let p = lerp(a, b, f);
            let alpha = p >> 24;
            for shift in [0, 8, 16] {
                assert!((p >> shift) & 0xFF <= alpha, "f = {f}: {p:#010x}");
            }
        }
        assert_eq!(lerp(a, b, 0), a);
        assert_eq!(lerp(a, b, 256), b);
    }
}
