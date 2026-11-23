//! Line break opportunities (UAX #14), including the Japanese rules that keep closing
//! brackets, small kana and the prolonged sound mark off the start of a line.

use std::str::CharIndices;

use crate::props::{Lb, props};

/// Whether a line may start with Japanese small kana (ぁ, っ, ャ, ...) or the prolonged sound
/// mark ー, the characters of Line_Break class CJ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Strictness {
    /// CJ characters are non-starters (CJ → NS), so they stay with the character before them.
    /// A space before one can still allow a break (LB18), unless an earlier rule such as LB16
    /// forbids it. This is the UAX #14 default, and what CSS `line-break: normal` and
    /// `strict` do with small kana.
    #[default]
    Strict,
    /// CJ characters are ideographs (CJ → ID), so a line may start with them; narrow columns
    /// fill better. This is UAX #14's tailoring for relaxed kana rules and the small-kana part
    /// of CSS `line-break: loose`; CSS `loose`'s other relaxations are not implemented.
    Loose,
}

/// A position where a line may or must end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineBreak {
    /// Byte offset of the break: the next line starts here.
    pub index: usize,
    /// A hard break (after a newline character, and at the end of the text) that layout must
    /// take, as opposed to an opportunity it may take.
    pub mandatory: bool,
}

/// Iterator over the line break opportunities in a text; see [`line_breaks`].
#[derive(Debug, Clone)]
pub struct LineBreaks<'a> {
    units: Units<'a>,
    /// Unit before `a`.
    before: Option<Unit>,
    a: Option<Unit>,
    b: Option<Unit>,
    c: Option<Unit>,
    d: Option<Unit>,
    /// The last unit at or before `a` that is not a space (for the rules with `SP*`), and
    /// the class of the unit before it (`None` at the start of the text).
    base: Option<Unit>,
    base_before: Option<Lb>,
    /// `a` ends a run `NU (SY | IS)*`; and so does the unit before `a`.
    number: bool,
    number_before: bool,
    /// Regional indicators ending at `a`.
    regional: usize,
    done: bool,
}

/// Finds the line break opportunities in `text`, in order. The end of a non-empty text is
/// always a mandatory break.
///
/// These are UAX #14's opportunities, which can fall inside a grapheme cluster in rare cases (a
/// combining mark after a space, LB10). Layout should wrap only where
/// [`is_grapheme_boundary`](crate::is_grapheme_boundary) agrees; to filter many boundaries,
/// walk [`graphemes`](crate::graphemes) alongside instead.
///
/// ```
/// use duydas_text::{Strictness, line_breaks};
///
/// let ends: Vec<usize> = line_breaks("「はい」と言った。", Strictness::Strict)
///     .map(|b| b.index)
///     .collect();
/// // Never after 「, and never before 」, 。 or the small っ.
/// assert_eq!(ends, [6, 12, 15, 21, 27]);
/// ```
#[must_use]
pub fn line_breaks(text: &str, strictness: Strictness) -> LineBreaks<'_> {
    let mut units = Units {
        chars: text.char_indices(),
        pending: None,
        strictness,
        end: text.len(),
    };
    let a = units.next();
    let b = units.next();
    let c = units.next();
    let d = units.next();
    let mut breaks = LineBreaks {
        units,
        before: None,
        a,
        b,
        c,
        d,
        base: None,
        base_before: None,
        number: false,
        number_before: false,
        regional: 0,
        done: a.is_none(),
    };
    if let Some(first) = a {
        breaks.enter(None, first);
    }
    breaks
}

impl Iterator for LineBreaks<'_> {
    type Item = LineBreak;

    fn next(&mut self) -> Option<LineBreak> {
        loop {
            if self.done {
                return None;
            }
            let (Some(a), Some(b)) = (self.a, self.b) else {
                self.done = true;
                return Some(LineBreak {
                    index: self.units.end,
                    mandatory: true,
                });
            };
            let decision = self.decide(a, b);
            let previous = self.a;
            self.before = previous;
            self.a = self.b;
            self.b = self.c;
            self.c = self.d;
            self.d = self.units.next();
            self.enter(previous, b);
            if let Some(mandatory) = decision {
                return Some(LineBreak {
                    index: b.start,
                    mandatory,
                });
            }
        }
    }
}

impl LineBreaks<'_> {
    /// Updates the left context once `unit` becomes `a`; `previous` is the unit before it.
    fn enter(&mut self, previous: Option<Unit>, unit: Unit) {
        if unit.class != Lb::Sp {
            self.base_before = previous.map(|p| p.class);
            self.base = Some(unit);
        }
        self.number_before = self.number;
        self.number =
            unit.class == Lb::Nu || (matches!(unit.class, Lb::Sy | Lb::Is) && self.number);
        self.regional = if unit.class == Lb::Ri {
            self.regional + 1
        } else {
            0
        };
    }

    /// The break between units `a` and `b`: `None` for none, `Some(true)` for a mandatory
    /// break, `Some(false)` for an opportunity.
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per UAX #14 rule, in the order the standard applies them"
    )]
    fn decide(&self, a: Unit, b: Unit) -> Option<bool> {
        use Lb::*;
        const NO: Option<bool> = None;
        const BREAK: Option<bool> = Some(false);
        const MANDATORY: Option<bool> = Some(true);
        let c = self.c;
        let d = self.d;
        let class = |u: Option<Unit>| u.map(|u| u.class);
        let base = class(self.base);

        // LB4, LB5: hard line breaks.
        match a.class {
            Bk | Lf | Nl => return MANDATORY,
            Cr => return if b.class == Lf { NO } else { MANDATORY },
            _ => {}
        }
        // LB6, LB7.
        if matches!(b.class, Bk | Cr | Lf | Nl | Sp | Zw) {
            return NO;
        }
        // LB8: ZW SP* ÷
        if base == Some(Zw) {
            return BREAK;
        }
        // LB8a: ZWJ ×
        if a.ends_with_zwj {
            return NO;
        }
        // LB9 and LB10 shaped the units. LB11, LB12, LB12a.
        if a.class == Wj || b.class == Wj || a.class == Gl {
            return NO;
        }
        if b.class == Gl && !matches!(a.class, Sp | Hy | Hh) {
            return NO;
        }
        // LB13.
        if matches!(b.class, Cl | Cp | Ex | Sy) {
            return NO;
        }
        // LB14: OP SP* ×
        if base == Some(Op) {
            return NO;
        }
        // LB15a: (sot | BK | CR | LF | NL | OP | QU | GL | SP | ZW) [Pi & QU] SP* ×
        if let Some(quote) = self.base
            && quote.class == Qu
            && quote.initial
            && matches!(
                self.base_before,
                None | Some(Bk | Cr | Lf | Nl | Op | Qu | Gl | Sp | Zw)
            )
        {
            return NO;
        }
        // LB15b: × [Pf & QU] (SP | GL | WJ | CL | QU | CP | EX | IS | SY | BK | CR | LF | NL | ZW | eot)
        if b.class == Qu
            && b.final_
            && class(c).is_none_or(|c| {
                matches!(
                    c,
                    Sp | Gl | Wj | Cl | Qu | Cp | Ex | Is | Sy | Bk | Cr | Lf | Nl | Zw
                )
            })
        {
            return NO;
        }
        // LB15c: SP ÷ IS NU
        if a.class == Sp && b.class == Is && class(c) == Some(Nu) {
            return BREAK;
        }
        // LB15d: × IS
        if b.class == Is {
            return NO;
        }
        // LB16: (CL | CP) SP* × NS
        if b.class == Ns && matches!(base, Some(Cl | Cp)) {
            return NO;
        }
        // LB17: B2 SP* × B2
        if b.class == B2 && base == Some(B2) {
            return NO;
        }
        // LB18: SP ÷
        if a.class == Sp {
            return BREAK;
        }
        // LB19: × [QU - Pi], [QU - Pf] ×
        if (b.class == Qu && !b.initial) || (a.class == Qu && !a.final_) {
            return NO;
        }
        // LB19a: quotation marks not surrounded by East Asian characters.
        if b.class == Qu && (!a.east_asian || c.is_none_or(|c| !c.east_asian)) {
            return NO;
        }
        if a.class == Qu && (!b.east_asian || self.before.is_none_or(|p| !p.east_asian)) {
            return NO;
        }
        // LB20: ÷ CB, CB ÷
        if a.class == Cb || b.class == Cb {
            return BREAK;
        }
        // LB20a: (sot | BK | CR | LF | NL | SP | ZW | CB | GL) (HY | HH) × (AL | HL)
        if matches!(a.class, Hy | Hh)
            && matches!(b.class, Al | Hl)
            && matches!(
                class(self.before),
                None | Some(Bk | Cr | Lf | Nl | Sp | Zw | Cb | Gl)
            )
        {
            return NO;
        }
        // LB21.
        if matches!(b.class, Ba | Hh | Hy | Ns) || a.class == Bb {
            return NO;
        }
        // LB21a: HL (HY | HH) × [^HL]
        if matches!(a.class, Hy | Hh) && class(self.before) == Some(Hl) && b.class != Hl {
            return NO;
        }
        // LB21b, LB22.
        if (a.class == Sy && b.class == Hl) || b.class == In {
            return NO;
        }
        let ideographic = |class: Lb| matches!(class, Id | Eb | Em);
        let korean = |class: Lb| matches!(class, Jl | Jv | Jt | H2 | H3);
        let letter = |class: Lb| matches!(class, Al | Hl);
        let affix = |class: Lb| matches!(class, Pr | Po);
        match (a.class, b.class) {
            // LB23.
            (x, Nu) if letter(x) => return NO,
            (Nu, y) if letter(y) => return NO,
            // LB23a.
            (Pr, y) if ideographic(y) => return NO,
            (x, Po) if ideographic(x) => return NO,
            // LB24.
            (x, y) if affix(x) && letter(y) => return NO,
            (x, y) if letter(x) && affix(y) => return NO,
            _ => {}
        }
        // LB25: numbers.
        if affix(b.class) {
            let number = if matches!(a.class, Cl | Cp) {
                self.number_before
            } else {
                self.number
            };
            if number {
                return NO;
            }
        }
        if affix(a.class)
            && (b.class == Nu
                || (b.class == Op
                    && (class(c) == Some(Nu) || (class(c) == Some(Is) && class(d) == Some(Nu)))))
        {
            return NO;
        }
        if (matches!(a.class, Hy | Is) || self.number) && b.class == Nu {
            return NO;
        }
        // LB26, LB27: Korean syllable blocks.
        match (a.class, b.class) {
            (Jl, Jl | Jv | H2 | H3) | (Jv | H2, Jv | Jt) | (Jt | H3, Jt) => return NO,
            (x, Po) if korean(x) => return NO,
            (Pr, y) if korean(y) => return NO,
            // LB28.
            (x, y) if letter(x) && letter(y) => return NO,
            _ => {}
        }
        // LB28a: Brahmic orthographic syllables.
        let aksara = |u: Unit| matches!(u.class, Ak | As) || u.dotted_circle;
        let aksara_start = |u: Unit| u.class == Ak || u.dotted_circle;
        if (a.class == Ap && aksara(b))
            || (aksara(a) && matches!(b.class, Vf | Vi))
            || (a.class == Vi && self.before.is_some_and(aksara) && aksara_start(b))
            || (aksara(a) && aksara(b) && class(c) == Some(Vf))
        {
            return NO;
        }
        // LB29: IS × (AL | HL)
        if a.class == Is && letter(b.class) {
            return NO;
        }
        // LB30: (AL | HL | NU) × [OP - EastAsian], [CP - EastAsian] × (AL | HL | NU)
        let word = |class: Lb| matches!(class, Al | Hl | Nu);
        if (word(a.class) && b.class == Op && !b.east_asian)
            || (a.class == Cp && !a.east_asian && word(b.class))
        {
            return NO;
        }
        // LB30a: regional indicator pairs.
        if a.class == Ri && b.class == Ri && !self.regional.is_multiple_of(2) {
            return NO;
        }
        // LB30b: EB × EM, [Extended_Pictographic & Cn] × EM
        if b.class == Em && (a.class == Eb || a.unassigned_pictographic) {
            return NO;
        }
        // LB31.
        BREAK
    }
}

/// A character with the combining marks and ZWJs after it (LB9), with the properties the rules
/// read.
#[derive(Debug, Clone, Copy)]
struct Unit {
    start: usize,
    class: Lb,
    east_asian: bool,
    initial: bool,
    final_: bool,
    unassigned_pictographic: bool,
    dotted_circle: bool,
    ends_with_zwj: bool,
}

#[derive(Debug, Clone)]
struct Units<'a> {
    chars: CharIndices<'a>,
    pending: Option<(usize, char)>,
    strictness: Strictness,
    end: usize,
}

/// A character's class after resolving CJ by strictness (LB1).
fn resolve(c: char, strictness: Strictness) -> Lb {
    match props(c).lb {
        Lb::Cj if strictness == Strictness::Loose => Lb::Id,
        Lb::Cj => Lb::Ns,
        other => other,
    }
}

impl Iterator for Units<'_> {
    type Item = Unit;

    fn next(&mut self) -> Option<Unit> {
        let (start, first) = self.pending.take().or_else(|| self.chars.next())?;
        let p = props(first);
        let class = resolve(first, self.strictness);
        let mut unit = Unit {
            start,
            class,
            east_asian: p.east_asian(),
            initial: p.initial_punctuation(),
            final_: p.final_punctuation(),
            unassigned_pictographic: p.extended_pictographic() && p.unassigned(),
            dotted_circle: first == '\u{25CC}',
            ends_with_zwj: class == Lb::Zwj,
        };
        if matches!(class, Lb::Bk | Lb::Cr | Lb::Lf | Lb::Nl | Lb::Sp | Lb::Zw) {
            return Some(unit);
        }
        for (index, c) in self.chars.by_ref() {
            let next = resolve(c, self.strictness);
            if !matches!(next, Lb::Cm | Lb::Zwj) {
                self.pending = Some((index, c));
                break;
            }
            unit.ends_with_zwj = next == Lb::Zwj;
        }
        // LB10: a combining sequence with no base acts as U+0041 A.
        if matches!(class, Lb::Cm | Lb::Zwj) {
            unit = Unit {
                class: Lb::Al,
                east_asian: false,
                initial: false,
                final_: false,
                unassigned_pictographic: false,
                ..unit
            };
        }
        Some(unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str, strictness: Strictness) -> Vec<&str> {
        let mut start = 0;
        line_breaks(text, strictness)
            .map(|b| {
                let line = &text[start..b.index];
                start = b.index;
                line
            })
            .collect()
    }

    #[test]
    fn japanese_kinsoku() {
        // Never after 「, never before 」, 。 or the small っ.
        assert_eq!(
            breaks("「東京」へ行った。", Strictness::Strict),
            ["「東", "京」", "へ", "行っ", "た。"]
        );
        // Strict keeps small kana and ー with the character before them.
        assert_eq!(
            breaks("ちょっとコーヒー", Strictness::Strict),
            ["ちょっ", "と", "コー", "ヒー"]
        );
        assert_eq!(
            breaks("ちょっとコーヒー", Strictness::Loose),
            ["ち", "ょ", "っ", "と", "コ", "ー", "ヒ", "ー"]
        );
        assert_eq!(
            breaks("「東京」へ。", Strictness::Strict),
            ["「東", "京」", "へ。"]
        );
    }

    #[test]
    fn mixed_scripts_and_punctuation() {
        assert_eq!(
            breaks("Merhaba dünya, nasılsın?", Strictness::Strict),
            ["Merhaba ", "dünya, ", "nasılsın?"]
        );
        assert_eq!(
            breaks("Unicode「ユニ」", Strictness::Strict),
            ["Unicode", "「ユ", "ニ」"]
        );
        assert_eq!(breaks("(₺1.250,00)", Strictness::Strict), ["(₺1.250,00)"]);
        assert_eq!(breaks("e-posta", Strictness::Strict), ["e-", "posta"]);
    }

    #[test]
    fn mandatory_breaks() {
        let all: Vec<LineBreak> = line_breaks("a\r\nb\nc", Strictness::Strict).collect();
        let expected = [(3, true), (5, true), (6, true)]
            .map(|(index, mandatory)| LineBreak { index, mandatory });
        assert_eq!(all, expected);
        assert_eq!(line_breaks("", Strictness::Strict).count(), 0);
        assert_eq!(
            line_breaks("a b", Strictness::Strict).collect::<Vec<_>>(),
            [
                LineBreak {
                    index: 2,
                    mandatory: false
                },
                LineBreak {
                    index: 3,
                    mandatory: true
                }
            ]
        );
    }

    #[test]
    fn emoji_sequences_stay_whole() {
        let family = "👨\u{200D}👩\u{200D}👧";
        let text = format!("{family}{family}👍🏽");
        assert_eq!(breaks(&text, Strictness::Strict), [family, family, "👍🏽"]);
    }
}
