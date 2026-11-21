//! Design tokens. Values sit on a 4 px grid; colors were chosen for WCAG AA contrast on their
//! intended backgrounds (checked in tests below).

use iced::{Color, Theme, color, theme::Palette};

/// Spacing scale in logical pixels.
pub mod space {
    /// 4 px
    pub const XS: f32 = 4.0;
    /// 8 px
    pub const SM: f32 = 8.0;
    /// 12 px
    pub const MD: f32 = 12.0;
    /// 16 px
    pub const LG: f32 = 16.0;
    /// 24 px
    pub const XL: f32 = 24.0;
    /// 32 px
    pub const XXL: f32 = 32.0;
}

/// Type scale in logical pixels.
pub mod text {
    /// Captions and hints.
    pub const CAPTION: f32 = 12.0;
    /// Body text.
    pub const BODY: f32 = 15.0;
    /// Section titles.
    pub const TITLE: f32 = 20.0;
    /// Screen headings.
    pub const DISPLAY: f32 = 30.0;
    /// Line-height multiplier that leaves room for CJK glyphs.
    pub const LINE_HEIGHT: f32 = 1.5;
}

/// Corner radii.
pub mod radius {
    /// Inputs and small controls.
    pub const SM: f32 = 6.0;
    /// Cards and panels.
    pub const MD: f32 = 10.0;
}

/// Dark palette: deep ink background with a warm amber accent.
pub const DARK: Palette = Palette {
    background: color!(0x12141a),
    text: color!(0xe8e6e3),
    primary: color!(0xf2a93b),
    success: color!(0x5fbf8f),
    warning: color!(0xe0b341),
    danger: color!(0xe5636b),
};

/// Light palette.
pub const LIGHT: Palette = Palette {
    background: color!(0xf7f5f2),
    text: color!(0x1b1d22),
    primary: color!(0x9a5b00),
    success: color!(0x1f7a4c),
    warning: color!(0x8a6100),
    danger: color!(0xb3262f),
};

/// The app theme for the given appearance.
pub fn theme(dark: bool) -> Theme {
    if dark {
        Theme::custom("Duydaş Dark", DARK)
    } else {
        Theme::custom("Duydaş Light", LIGHT)
    }
}

/// WCAG 2.x contrast ratio between two opaque colors.
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    fn luminance(c: Color) -> f32 {
        let channel = |v: f32| {
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_meets_wcag_aa() {
        for palette in [DARK, LIGHT] {
            assert!(contrast_ratio(palette.text, palette.background) >= 4.5);
            assert!(contrast_ratio(palette.primary, palette.background) >= 4.5);
            assert!(contrast_ratio(palette.danger, palette.background) >= 4.5);
        }
    }
}
