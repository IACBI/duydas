//! Character properties for segmentation, looked up in the generated tables.
//!
//! Variant names follow the Unicode property value names in CamelCase: `Lb::Al` is
//! Line_Break=AL, `Gcb::RegionalIndicator` is Grapheme_Cluster_Break=Regional_Indicator.

pub(crate) use crate::tables::props;

/// Line_Break (UAX #14) after the default LB1 resolution: AI, SG and XX are AL, and SA is CM
/// for combining marks and AL otherwise. CJ is kept; the line breaker resolves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lb {
    Ak,
    Al,
    Ap,
    As,
    B2,
    Ba,
    Bb,
    Bk,
    Cb,
    Cj,
    Cl,
    Cm,
    Cp,
    Cr,
    Eb,
    Em,
    Ex,
    Gl,
    H2,
    H3,
    Hh,
    Hl,
    Hy,
    Id,
    In,
    Is,
    Jl,
    Jt,
    Jv,
    Lf,
    Nl,
    Ns,
    Nu,
    Op,
    Po,
    Pr,
    Qu,
    Ri,
    Sp,
    Sy,
    Vf,
    Vi,
    Wj,
    Zw,
    Zwj,
}

/// Grapheme_Cluster_Break (UAX #29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gcb {
    Other,
    Cr,
    Lf,
    Control,
    Extend,
    Zwj,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
}

/// Word_Break (UAX #29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wb {
    Other,
    Cr,
    Lf,
    Newline,
    Extend,
    Zwj,
    RegionalIndicator,
    Format,
    Katakana,
    HebrewLetter,
    ALetter,
    SingleQuote,
    DoubleQuote,
    MidNumLet,
    MidLetter,
    MidNum,
    Numeric,
    ExtendNumLet,
    WSegSpace,
}

/// Indic_Conjunct_Break, for grapheme rule GB9c.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InCb {
    None,
    Linker,
    Consonant,
    Extend,
}

/// Everything segmentation needs to know about one code point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Props {
    pub(crate) lb: Lb,
    pub(crate) gcb: Gcb,
    pub(crate) wb: Wb,
    pub(crate) incb: InCb,
    flags: u8,
}

impl Props {
    /// East_Asian_Width is Fullwidth, Wide or Halfwidth.
    const EAST_ASIAN: u8 = 1;
    /// General_Category=Pi.
    const INITIAL_PUNCTUATION: u8 = 2;
    /// General_Category=Pf.
    const FINAL_PUNCTUATION: u8 = 4;
    const EXTENDED_PICTOGRAPHIC: u8 = 8;
    /// General_Category=Cn.
    const UNASSIGNED: u8 = 16;
    /// Alphabetic, or a number (General_Category=Nd, Nl or No).
    const WORD_CHARACTER: u8 = 32;

    pub(crate) const fn new(lb: Lb, gcb: Gcb, wb: Wb, incb: InCb, flags: u8) -> Self {
        Self {
            lb,
            gcb,
            wb,
            incb,
            flags,
        }
    }

    pub(crate) const fn east_asian(self) -> bool {
        self.flags & Self::EAST_ASIAN != 0
    }

    pub(crate) const fn initial_punctuation(self) -> bool {
        self.flags & Self::INITIAL_PUNCTUATION != 0
    }

    pub(crate) const fn final_punctuation(self) -> bool {
        self.flags & Self::FINAL_PUNCTUATION != 0
    }

    pub(crate) const fn extended_pictographic(self) -> bool {
        self.flags & Self::EXTENDED_PICTOGRAPHIC != 0
    }

    pub(crate) const fn unassigned(self) -> bool {
        self.flags & Self::UNASSIGNED != 0
    }

    pub(crate) const fn word_character(self) -> bool {
        self.flags & Self::WORD_CHARACTER != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_match_the_ucd() {
        let a = props('a');
        assert_eq!(
            (a.lb, a.gcb, a.wb, a.incb),
            (Lb::Al, Gcb::Other, Wb::ALetter, InCb::None)
        );
        assert_eq!(props('\u{3041}').lb, Lb::Cj, "small hiragana a");
        assert_eq!(props('\u{3002}').lb, Lb::Cl, "ideographic full stop");
        assert!(props('\u{3042}').east_asian());
        assert!(!props('ı').east_asian());
        assert_eq!(props('\u{0E01}').lb, Lb::Al, "Thai SA resolves to AL");
        assert_eq!(
            props('\u{0E31}').lb,
            Lb::Cm,
            "Thai vowel sign SA resolves to CM"
        );
        assert_eq!(props('\u{00A7}').lb, Lb::Al, "AI resolves to AL");
        assert!(props('\u{201C}').initial_punctuation());
        assert!(props('\u{201D}').final_punctuation());
        assert!(props('\u{1F600}').extended_pictographic());
        assert!(props('\u{1FFFD}').unassigned());
        assert_eq!(props('\u{0915}').incb, InCb::Consonant);
        assert_eq!(props('\u{094D}').incb, InCb::Linker);
        assert_eq!(props('\u{30A2}').wb, Wb::Katakana);
        assert_eq!(props('\u{1F1F9}').gcb, Gcb::RegionalIndicator);
        assert_eq!(props('\u{10FFFF}').gcb, Gcb::Other);
        assert!(
            props('\u{3D000}').word_character(),
            "a Unicode 18 ideograph"
        );
        assert!(props('ı').word_character() && props('²').word_character());
        assert!(!props('、').word_character() && !props('_').word_character());
    }
}
