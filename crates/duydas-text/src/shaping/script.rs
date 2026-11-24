//! Scripts, languages, and splitting text into runs by script and font.

use std::ops::Range;

use crate::font::Font;
use crate::shaping_tables::{
    DEFAULT_IGNORABLE, SCRIPT_CODES, SCRIPT_COMMON, SCRIPT_INHERITED, SCRIPT_UNKNOWN, char_info,
};

use super::normalize::{decomposition, is_variation_selector, normalize, space_kind};

/// A Unicode script (the Script property, UAX #24).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Script(u8);

impl Default for Script {
    fn default() -> Self {
        Self::COMMON
    }
}

impl Script {
    /// Characters shared by several scripts, such as digits, spaces and punctuation.
    pub const COMMON: Self = Self(SCRIPT_COMMON);
    /// Combining marks that take the script of the character they follow.
    pub const INHERITED: Self = Self(SCRIPT_INHERITED);
    /// Unassigned code points.
    pub const UNKNOWN: Self = Self(SCRIPT_UNKNOWN);

    /// The script of a character.
    pub fn of(c: char) -> Self {
        Self((char_info(c) & 0xFF) as u8)
    }

    /// The script with this ISO 15924 code, such as `Latn` or `Jpan`'s parts `Hani`, `Hira`
    /// and `Kana`; case does not matter.
    pub fn from_iso15924(code: &str) -> Option<Self> {
        SCRIPT_CODES
            .iter()
            .position(|c| c.eq_ignore_ascii_case(code.as_bytes()))
            .and_then(|i| u8::try_from(i).ok())
            .map(Self)
    }

    /// The script's ISO 15924 code.
    pub fn iso15924(self) -> &'static str {
        std::str::from_utf8(&SCRIPT_CODES[usize::from(self.0)]).unwrap_or("Zzzz")
    }

    /// Whether this is a real script rather than Common, Inherited or Unknown.
    pub fn is_specific(self) -> bool {
        !matches!(self.0, SCRIPT_COMMON | SCRIPT_INHERITED | SCRIPT_UNKNOWN)
    }

    /// The OpenType script tag; `None` for Common, Inherited and Unknown, which use the
    /// font's default script.
    pub(crate) fn opentype_tag(self) -> Option<[u8; 4]> {
        if !self.is_specific() {
            return None;
        }
        Some(match &SCRIPT_CODES[usize::from(self.0)] {
            b"Hira" | b"Kana" | b"Hrkt" => *b"kana",
            b"Laoo" => *b"lao ",
            b"Yiii" => *b"yi  ",
            b"Nkoo" => *b"nko ",
            b"Vaii" => *b"vai ",
            code => code.map(|b| b.to_ascii_lowercase()),
        })
    }
}

/// A language, for the font's language-specific forms (OpenType language systems), such as
/// Turkish dotted i in ligatures or Japanese glyph shapes in a Pan-CJK font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Language([Option<[u8; 4]>; 2]);

/// BCP 47 primary language subtags (and a few ISO 639-2 codes) with their OpenType language
/// system tags, sorted by subtag.
const LANGUAGES: &[(&str, [u8; 4])] = &[
    ("af", *b"AFK "),
    ("ar", *b"ARA "),
    ("az", *b"AZE "),
    ("aze", *b"AZE "),
    ("ba", *b"BSH "),
    ("be", *b"BEL "),
    ("bg", *b"BGR "),
    ("bn", *b"BEN "),
    ("bs", *b"BOS "),
    ("ca", *b"CAT "),
    ("crh", *b"CRT "),
    ("cs", *b"CSY "),
    ("cv", *b"CHU "),
    ("cy", *b"WEL "),
    ("da", *b"DAN "),
    ("de", *b"DEU "),
    ("el", *b"ELL "),
    ("en", *b"ENG "),
    ("eng", *b"ENG "),
    ("eo", *b"NTO "),
    ("es", *b"ESP "),
    ("et", *b"ETI "),
    ("eu", *b"EUQ "),
    ("fa", *b"FAR "),
    ("fi", *b"FIN "),
    ("fr", *b"FRA "),
    ("ga", *b"IRI "),
    ("gag", *b"GAG "),
    ("gd", *b"GAE "),
    ("gl", *b"GAL "),
    ("gu", *b"GUJ "),
    ("he", *b"IWR "),
    ("hi", *b"HIN "),
    ("hr", *b"HRV "),
    ("hu", *b"HUN "),
    ("hy", *b"HYE "),
    ("id", *b"IND "),
    ("is", *b"ISL "),
    ("it", *b"ITA "),
    ("ja", *b"JAN "),
    ("jpn", *b"JAN "),
    ("ka", *b"KAT "),
    ("kaa", *b"KRK "),
    ("kk", *b"KAZ "),
    ("km", *b"KHM "),
    ("kn", *b"KAN "),
    ("ko", *b"KOR "),
    ("ku", *b"KUR "),
    ("ky", *b"KIR "),
    ("lt", *b"LTH "),
    ("lv", *b"LVI "),
    ("mk", *b"MKD "),
    ("ml", *b"MAL "),
    ("mn", *b"MNG "),
    ("mo", *b"MOL "),
    ("mr", *b"MAR "),
    ("ms", *b"MLY "),
    ("mt", *b"MTS "),
    ("nb", *b"NOR "),
    ("nl", *b"NLD "),
    ("nn", *b"NYN "),
    ("no", *b"NOR "),
    ("pl", *b"PLK "),
    ("pt", *b"PTG "),
    ("ro", *b"ROM "),
    ("ru", *b"RUS "),
    ("sk", *b"SKY "),
    ("sl", *b"SLV "),
    ("sq", *b"SQI "),
    ("sr", *b"SRB "),
    ("sv", *b"SVE "),
    ("ta", *b"TAM "),
    ("te", *b"TEL "),
    ("th", *b"THA "),
    ("tk", *b"TKM "),
    ("tr", *b"TRK "),
    ("tt", *b"TAT "),
    ("tur", *b"TRK "),
    ("uk", *b"UKR "),
    ("ur", *b"URD "),
    ("uz", *b"UZB "),
    ("vi", *b"VIT "),
];

// The binary search needs the language list sorted; checked when the crate compiles.
const _: () = {
    let mut i = 1;
    let mut sorted = true;
    while i < LANGUAGES.len() {
        let (a, b) = (LANGUAGES[i - 1].0.as_bytes(), LANGUAGES[i].0.as_bytes());
        let mut k = 0;
        let mut less = a.len() < b.len();
        while k < a.len() && k < b.len() {
            if a[k] != b[k] {
                less = a[k] < b[k];
                break;
            }
            k += 1;
        }
        sorted &= less;
        i += 1;
    }
    assert!(sorted, "LANGUAGES is sorted by subtag");
};

impl Language {
    /// The language of a BCP 47 tag such as `tr-TR`, `ja` or `zh-Hant-TW`. A language
    /// without an OpenType language system gets the font's default forms.
    pub fn from_bcp47(tag: &str) -> Self {
        let mut subtags = tag.split(['-', '_']);
        let primary = subtags.next().unwrap_or("").to_ascii_lowercase();
        if primary == "zh" {
            return Self::chinese(subtags);
        }
        Self([
            LANGUAGES
                .binary_search_by_key(&primary.as_str(), |&(s, _)| s)
                .ok()
                .map(|i| LANGUAGES[i].1),
            None,
        ])
    }

    /// Chinese, as HarfBuzz tags it: an explicit script subtag first (Simplified or
    /// Traditional), then the region (Taiwan, Hong Kong, Macao), Simplified otherwise. Macao
    /// falls back to Hong Kong forms when the font has none of its own.
    fn chinese<'a>(subtags: impl Iterator<Item = &'a str>) -> Self {
        let (mut script, mut region) = (None, None);
        for subtag in subtags.take_while(|s| !s.eq_ignore_ascii_case("x")) {
            let subtag = subtag.to_ascii_lowercase();
            match subtag.as_str() {
                "hans" | "hant" if script.is_none() => script = Some(subtag),
                "tw" | "hk" | "mo" if region.is_none() => region = Some(subtag),
                _ => {}
            }
        }
        let macao = [Some(*b"ZHTM"), Some(*b"ZHH ")];
        Self(match (script.as_deref(), region.as_deref()) {
            (Some("hans"), _) => [Some(*b"ZHS "), None],
            (_, Some("hk")) => [Some(*b"ZHH "), None],
            (_, Some("mo")) => macao,
            (Some("hant"), _) | (_, Some("tw")) => [Some(*b"ZHT "), None],
            _ => [Some(*b"ZHS "), None],
        })
    }

    /// The OpenType language system tag, if the language has one.
    pub fn opentype_tag(self) -> Option<[u8; 4]> {
        self.0[0]
    }

    /// The language system tags to try in order: the tag, then a fallback for the few
    /// languages HarfBuzz gives one.
    pub(crate) fn opentype_tags(self) -> [Option<[u8; 4]>; 2] {
        self.0
    }
}

/// A run of text with one script and one font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// Index of the font in the fallback list.
    pub font: usize,
    /// The run's script; Common or Inherited characters take the script around them.
    pub script: Script,
}

/// Splits `text` into runs to shape, each with one script and the font that draws it.
///
/// Each grapheme cluster gets the first font in `fonts` that covers all its characters
/// (default-ignorable characters and variation selectors aside; a character also counts as
/// covered when the font has its canonical decomposition, and a letter with marks when
/// shaping composes them into a glyph the font has, such as e and U+0301 into é). A cluster of Common or Inherited
/// characters, such as a space or a digit, stays with the font before it when that font
/// covers it. A cluster no font covers goes to the first font that has its first character,
/// so a letter keeps its font when only a mark after it is missing; failing that, it stays
/// with the font before it (the first font at the start), and shows the missing-glyph box
/// for what the font lacks. The fallback order is the caller's policy: put the
/// font whose shapes the text should take first, such as a Japanese font before a Chinese
/// one for Japanese text.
///
/// Returns no runs when `fonts` is empty.
pub fn itemize(text: &str, fonts: &[&Font]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    if fonts.is_empty() {
        return runs;
    }
    let scripts = resolve_scripts(text);
    // Both walk the text in order, so the script run is found by moving forward.
    let mut script_run = 0;
    let mut start = 0;
    let mut previous: Option<usize> = None;
    for grapheme in crate::graphemes(text) {
        let range = start..start + grapheme.len();
        start = range.end;
        let neutral = grapheme.chars().all(|c| !Script::of(c).is_specific());
        let font = previous
            .filter(|&f| neutral && covers(fonts[f], grapheme))
            .or_else(|| fonts.iter().position(|f| covers(f, grapheme)))
            .or_else(|| {
                let base = grapheme.chars().next()?;
                fonts.iter().position(|f| f.glyph_index(base).is_some())
            })
            .or(previous)
            .unwrap_or(0);
        previous = Some(font);
        while scripts
            .get(script_run)
            .is_some_and(|(r, _)| r.end <= range.start)
        {
            script_run += 1;
        }
        let script = scripts
            .get(script_run)
            .filter(|(r, _)| r.contains(&range.start))
            .map_or(Script::COMMON, |&(_, s)| s);
        match runs.last_mut() {
            Some(run) if run.font == font && run.script == script => run.range.end = range.end,
            _ => runs.push(Run {
                range,
                font,
                script,
            }),
        }
    }
    runs
}

/// Splits `text` into runs of one script. Common and Inherited characters take the script
/// before them, or after them at the start of the text; text with no specific script is
/// Common.
fn resolve_scripts(text: &str) -> Vec<(Range<usize>, Script)> {
    let mut runs: Vec<(Range<usize>, Script)> = Vec::new();
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        let script = Script::of(c);
        match runs.last_mut() {
            Some((range, current)) if !script.is_specific() || *current == script => {
                range.end = end;
            }
            Some((range, current)) if !current.is_specific() => {
                range.end = end;
                *current = script;
            }
            _ => runs.push((i..end, script)),
        }
    }
    runs
}

/// Whether `font` can draw every character of `grapheme` that needs a glyph, one by one
/// or as shaping maps them, composing a letter and its marks into a glyph the font has.
fn covers(font: &Font, grapheme: &str) -> bool {
    covers_each(font, grapheme)
        || normalize(font, grapheme, false)
            .iter()
            .all(|ch| ch.glyph != 0 || char_info(ch.c) & DEFAULT_IGNORABLE != 0)
}

fn covers_each(font: &Font, grapheme: &str) -> bool {
    let mut chars = grapheme.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(&selector) = chars.peek()
            && is_variation_selector(selector)
            && font.glyph_variant(c, selector).is_some()
        {
            chars.next();
            continue;
        }
        if char_info(c) & DEFAULT_IGNORABLE != 0 {
            continue;
        }
        if !covers_char(font, c, 0) {
            return false;
        }
    }
    true
}

fn covers_char(font: &Font, c: char, depth: u8) -> bool {
    if font.glyph_index(c).is_some() {
        return true;
    }
    if space_kind(c) != 0 && font.glyph_index(' ').is_some() {
        return true;
    }
    depth < 4
        && decomposition(c).is_some_and(|(a, b)| {
            covers_char(font, a, depth + 1) && b.is_none_or(|b| font.glyph_index(b).is_some())
        })
}
