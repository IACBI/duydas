//! Shaping tests: a hand-built font with known substitutions and positions, the Unicode
//! data behind normalization, and every installed font.

use std::sync::Arc;

use super::normalize::{composition, decomposition};
use super::*;
use crate::font::layout::tests::{
    alternates, gdef, layout_table, ligatures, lookup, mark_attach, mark_to_ligature, multiple,
    pair, simple_table, single,
};
use crate::font::tests::{
    cmap_table, collect, font_dirs, format12, format14, simple_glyph, tt_font,
};

// Glyphs of the test font.
const SPACE_GLYPH: u16 = 1;
const F: u16 = 2;
const I: u16 = 3;
const A_CAP: u16 = 4;
const V_CAP: u16 = 5;
const E: u16 = 6;
const ACUTE: u16 = 7;
const E_ACUTE: u16 = 8;
const O: u16 = 9;
const DIAERESIS: u16 = 10;
const FI: u16 = 11;
const A: u16 = 12;
const DOT_BELOW: u16 = 13;
const KUZU: u16 = 14;
const KUZU_VARIANT: u16 = 15;

/// Advances; the acute has one so the test can see it zeroed.
const ADVANCES: [u16; 17] = [
    500, 250, 300, 250, 600, 600, 500, 100, 500, 500, 0, 550, 500, 0, 1000, 1000, 520,
];

fn test_font() -> Font {
    Font::from_data(test_font_data(), 0).expect("the test font parses")
}

fn test_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[
        (
            3,
            10,
            format12(&[
                (0x20, 0x20, 1),
                (0x30, 0x30, 16),
                (0x41, 0x41, 4),
                (0x56, 0x56, 5),
                (0x61, 0x61, 12),
                (0x65, 0x65, 6),
                (0x66, 0x66, 2),
                (0x69, 0x69, 3),
                (0x6F, 0x6F, 9),
                (0xE9, 0xE9, 8),
                (0x301, 0x301, 7),
                (0x308, 0x308, 10),
                (0x323, 0x323, 13),
                (0x845B, 0x845B, 14),
            ]),
        ),
        (0, 5, format14(0xE0100, &[], &[(0x845B, 15)])),
    ]);
    let classes: Vec<(u16, u16)> = (1..17)
        .map(|g| match g {
            FI => (g, 2),
            ACUTE | DIAERESIS | DOT_BELOW => (g, 3),
            _ => (g, 1),
        })
        .collect();
    // Turkish has no ligatures: fi would lose the dot of i.
    let gsub = layout_table(
        *b"latn",
        &[(*b"TRK ", 0xFFFF, &[])],
        &[(*b"liga", &[0])],
        vec![lookup(4, 0, vec![ligatures(&[(&[F, I], FI)])])],
    );
    let above = Some((250, 600));
    let marks = [(ACUTE, 0, (50, 0)), (DIAERESIS, 0, (50, 0))];
    let gpos = simple_table(
        &[*b"kern", *b"mark"],
        vec![
            lookup(2, 0, vec![pair(&[(A_CAP, V_CAP, -80)])]),
            lookup(
                4,
                0,
                vec![mark_attach(
                    &marks,
                    &[(E, vec![above]), (O, vec![above]), (A, vec![above])],
                    1,
                )],
            ),
        ],
    );
    let glyphs = vec![Vec::new(); ADVANCES.len()];
    let metrics: Vec<(u16, i16)> = ADVANCES.iter().map(|&a| (a, 0)).collect();
    tt_font(
        &glyphs,
        &metrics,
        &[
            (b"cmap", cmap),
            (b"GDEF", gdef(&classes, &[], &[])),
            (b"GSUB", gsub),
            (b"GPOS", gpos),
        ],
    )
}

/// A second font: a space, あ and a missing é, for fallback.
fn kana_font() -> Font {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[(0x20, 0x20, 1), (0x65, 0x65, 3), (0x3042, 0x3042, 2)]),
    )]);
    let data = tt_font(
        &vec![Vec::new(); 4],
        &[(500, 0), (250, 0), (1000, 0), (500, 0)],
        &[(b"cmap", cmap)],
    );
    Font::from_data(data, 0).expect("the kana font parses")
}

// Glyphs of the legacy font: no layout tables, a kern table and outlines.
const L_A: u16 = 2;
const L_V: u16 = 3;
const L_E: u16 = 4;
const L_ACUTE: u16 = 5;
const L_DOT: u16 = 6;
const L_GA: u16 = 7;
const L_GAK: u16 = 8;
const L_KIYEOK: u16 = 9;
const L_A_JAMO: u16 = 10;
const L_KIYEOK_FINAL: u16 = 11;

/// A font in the old style: a kern table instead of GPOS, so marks are placed from the
/// glyphs' outlines, and Hangul syllables with their jamo.
fn legacy_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x41, 0x41, 2),
            (0x56, 0x56, 3),
            (0x65, 0x65, 4),
            (0x301, 0x301, 5),
            (0x323, 0x323, 6),
            (0x1100, 0x1100, 9),
            (0x1161, 0x1161, 10),
            (0x11A8, 0x11A8, 11),
            (0xAC00, 0xAC01, 7),
        ]),
    )]);
    let rect = |x0: i16, y0: i16, x1: i16, y1: i16| {
        simple_glyph(&[&[
            (x0, y0, true),
            (x0, y1, true),
            (x1, y1, true),
            (x1, y0, true),
        ]])
    };
    let glyphs = vec![
        Vec::new(),
        Vec::new(),
        rect(0, 0, 600, 700),
        rect(0, 0, 600, 700),
        rect(50, 0, 450, 500),
        rect(-150, 550, -50, 700),
        rect(-150, -150, -50, -50),
        rect(100, -100, 900, 800),
        rect(100, -100, 900, 800),
        rect(100, -100, 900, 800),
        rect(100, -100, 900, 800),
        rect(100, -100, 900, 800),
    ];
    let advances = [500, 250, 600, 600, 500, 0, 0, 1000, 1000, 1000, 1000, 1000];
    let metrics: Vec<(u16, i16)> = advances
        .iter()
        .zip([0, 0, 0, 0, 50, -150, -150, 100, 100, 100, 100, 100])
        .map(|(&a, l)| (a, l))
        .collect();
    // Version 0, one horizontal format 0 subtable: A V -80 and V A -60.
    let mut kern = vec![0, 0, 0, 1, 0, 0, 0, 26, 0, 1, 0, 2, 0, 12, 0, 1, 0, 0];
    for (left, right, value) in [(L_A, L_V, -80i16), (L_V, L_A, -60)] {
        kern.extend(left.to_be_bytes());
        kern.extend(right.to_be_bytes());
        kern.extend(value.to_be_bytes());
    }
    tt_font(&glyphs, &metrics, &[(b"cmap", cmap), (b"kern", kern)])
}

// Glyphs of the parity font.
const P_A: u16 = 2;
const P_B: u16 = 3;
const P_F: u16 = 4;
const P_I: u16 = 5;
const P_ACUTE: u16 = 6;
const P_CIRCUMFLEX: u16 = 7;
const P_B1: u16 = 8;
const P_B2: u16 = 9;
const P_FI: u16 = 10;
const P_X: u16 = 11;
const P_C: u16 = 12;

/// A font for the corners of HarfBuzz's behaviour no installed font reaches: `b` becomes
/// two glyphs and `c` becomes `b` in `rvrn` (a later lookup), `f i` ligates over marks,
/// two marks ligate in `clig` (passing over ligatures) and in `dlig` (not), marks attach
/// to bases and to the ligature, and a legacy kern table kerns and raises (cross-stream).
fn parity_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x61, 0x62, u32::from(P_A)),
            (0x63, 0x63, u32::from(P_C)),
            (0x66, 0x66, u32::from(P_F)),
            (0x69, 0x69, u32::from(P_I)),
            (0x301, 0x302, u32::from(P_ACUTE)),
        ]),
    )]);
    let classes: Vec<(u16, u16)> = (1..13)
        .map(|g| match g {
            P_FI => (g, 2),
            P_ACUTE | P_CIRCUMFLEX | P_X => (g, 3),
            _ => (g, 1),
        })
        .collect();
    let gsub = simple_table(
        &[*b"ccmp", *b"liga", *b"clig", *b"dlig", *b"rvrn"],
        vec![
            lookup(2, 0, vec![multiple(&[(P_B, &[P_B1, P_B2])])]),
            lookup(4, 0x8, vec![ligatures(&[(&[P_F, P_I], P_FI)])]),
            lookup(4, 0x4, vec![ligatures(&[(&[P_ACUTE, P_CIRCUMFLEX], P_X)])]),
            lookup(4, 0, vec![ligatures(&[(&[P_ACUTE, P_CIRCUMFLEX], P_X)])]),
            lookup(1, 0, vec![single(&[(P_C, P_B)])]),
        ],
    );
    let marks = [
        (P_ACUTE, 0, (50, 0)),
        (P_CIRCUMFLEX, 0, (50, -20)),
        (P_X, 0, (40, 10)),
    ];
    let gpos = simple_table(
        &[*b"mark", *b"mark"],
        vec![
            lookup(
                4,
                0,
                vec![mark_attach(
                    &marks,
                    &[
                        (P_A, vec![Some((300, 700))]),
                        (P_B1, vec![Some((250, 650))]),
                    ],
                    1,
                )],
            ),
            lookup(
                5,
                0,
                vec![mark_to_ligature(
                    &marks,
                    &[(P_FI, vec![vec![Some((150, 700))], vec![Some((450, 720))]])],
                    1,
                )],
            ),
        ],
    );
    // Version 0: a horizontal subtable (a a -50, a b1 -30) and a cross-stream one
    // (a b1 120, b2 a -40).
    let subtable = |coverage: u16, pairs: &[(u16, u16, i16)]| {
        let n = pairs.len() as u16;
        let mut s = Vec::new();
        for v in [0, 14 + 6 * n, coverage, n, 12, 1, 6 * n - 12] {
            s.extend(v.to_be_bytes());
        }
        for &(left, right, value) in pairs {
            s.extend(left.to_be_bytes());
            s.extend(right.to_be_bytes());
            s.extend(value.to_be_bytes());
        }
        s
    };
    let mut kern = vec![0, 0, 0, 2];
    kern.extend(subtable(0x0001, &[(P_A, P_A, -50), (P_A, P_B1, -30)]));
    kern.extend(subtable(0x0005, &[(P_A, P_B1, 120), (P_B2, P_A, -40)]));
    let advances = [500, 250, 600, 600, 300, 300, 0, 0, 300, 300, 600, 0, 600];
    let metrics: Vec<(u16, i16)> = advances.iter().map(|&a| (a, 0)).collect();
    tt_font(
        &vec![Vec::new(); advances.len()],
        &metrics,
        &[
            (b"cmap", cmap),
            (b"GDEF", gdef(&classes, &[], &[])),
            (b"GSUB", gsub),
            (b"GPOS", gpos),
            (b"kern", kern),
        ],
    )
}

/// Without GDEF, glyph classes come from Unicode, and COMBINING GRAPHEME JOINER is a base
/// even though it is a nonspacing mark. Between marks of classes 232 and 230, which
/// canonical ordering would swap, it keeps them apart, so an IgnoreMarks ligature cannot
/// reach over it; between marks of classes 230 and 232, already in order, it does nothing
/// and is passed over. HarfBuzz 10.4.0 shapes these texts the same way.
#[test]
fn grapheme_joiners_are_not_marks_without_gdef() {
    const LIG: u16 = 7;
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x61, 0x62, 2),
            (0x300, 0x300, 4),
            (0x315, 0x315, 5),
            (0x34F, 0x34F, 6),
        ]),
    )]);
    let gsub = simple_table(
        &[*b"liga"],
        vec![lookup(4, 0x8, vec![ligatures(&[(&[2, 3], LIG)])])],
    );
    let metrics = vec![(500u16, 0i16); 8];
    let data = tt_font(
        &vec![Vec::new(); 8],
        &metrics,
        &[(b"cmap", cmap), (b"GSUB", gsub)],
    );
    let font = Font::from_data(data, 0).expect("the font parses");
    let kept = ids(&run(&font, "a\u{315}\u{34F}\u{300}b", &latin()));
    assert_eq!(kept, [2, 5, SPACE_GLYPH, 4, 3]);
    let joined = ids(&run(&font, "a\u{300}\u{34F}\u{315}b", &latin()));
    assert_eq!(joined[0], LIG);
}

/// The parity font shaped as HarfBuzz 10.4.0 shapes it (glyph, cluster, advance, x and y
/// offsets).
#[test]
fn parity_font_shapes_like_harfbuzz() {
    let font = Font::from_data(parity_font_data(), 0).expect("the parity font parses");
    let shaped = |text: &str, features: &[Feature]| -> Vec<(u16, usize, i32, i32, i32)> {
        let options = ShapeOptions {
            features,
            ..latin()
        };
        run(&font, text, &options)
            .iter()
            .map(|g| (g.glyph.0, g.cluster, g.x_advance, g.x_offset, g.y_offset))
            .collect()
    };
    // The cross-stream kern subtable hangs every glyph from the one before, which replaces
    // the marks' attachments: they keep their anchor offsets from their own position.
    assert_eq!(
        shaped("a\u{301}", &[]),
        [(P_A, 0, 600, 0, 0), (P_ACUTE, 0, 0, 250, 700)]
    );
    // Of b's two glyphs the mark attaches to the first, though only the second precedes it.
    assert_eq!(
        shaped("b\u{301}", &[]),
        [
            (P_B1, 0, 300, 0, 0),
            (P_B2, 0, 300, 0, 0),
            (P_ACUTE, 0, 0, 200, 650)
        ]
    );
    // `mark` does not pass over a joiner.
    assert_eq!(
        shaped("a\u{200D}\u{301}", &[]),
        [
            (P_A, 0, 600, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (P_ACUTE, 0, 0, 0, 0)
        ]
    );
    // The marks between and after f i follow components of the ligature; `clig`, which
    // passes over ligatures, ligates them, and `dlig`, which does not, cannot.
    assert_eq!(
        shaped("f\u{301}i\u{302}", &[]),
        [(P_FI, 0, 600, 0, 0), (P_X, 0, 0, 110, 690)]
    );
    let dlig = [Feature::new(*b"clig", 0), Feature::new(*b"dlig", 1)];
    assert_eq!(
        shaped("f\u{301}i\u{302}", &dlig),
        [
            (P_FI, 0, 600, 0, 0),
            (P_ACUTE, 0, 0, 100, 700),
            (P_CIRCUMFLEX, 0, 0, 400, 1440)
        ]
    );
    // `rvrn` comes first, so its b is then split.
    assert_eq!(
        shaped("c", &[]),
        [(P_B1, 0, 300, 0, 0), (P_B2, 0, 300, 0, 0)]
    );
    // Both subtables kern: half the horizontal value to each glyph, and the cross-stream
    // value raises the second glyph and everything after it.
    assert_eq!(
        shaped("aab", &[]),
        [
            (P_A, 0, 575, 0, 0),
            (P_A, 1, 560, -25, 0),
            (P_B1, 2, 285, -15, 120),
            (P_B2, 2, 300, 0, 120)
        ]
    );
    assert_eq!(
        shaped("a\u{301}ba", &[]),
        [
            (P_A, 0, 585, 0, 0),
            (P_ACUTE, 0, 0, 250, 700),
            (P_B1, 3, 285, -15, 820),
            (P_B2, 3, 300, 0, 820),
            (P_A, 4, 600, 0, 780)
        ]
    );
    assert_eq!(
        shaped("bb\u{301}\u{302}", &[]),
        [
            (P_B1, 0, 300, 0, 0),
            (P_B2, 0, 300, 0, 0),
            (P_B1, 1, 300, 0, 0),
            (P_B2, 1, 300, 0, 0),
            (P_X, 1, 0, 210, 640)
        ]
    );
    assert_eq!(
        shaped("ab", &[Feature::new(*b"kern", 0)]),
        [
            (P_A, 0, 600, 0, 0),
            (P_B1, 1, 300, 0, 0),
            (P_B2, 1, 300, 0, 0)
        ]
    );
}

/// Edits all over a long run, and marks far from their base, take linear time: quadratic
/// work would take minutes here even optimized, and runs out of budget before finishing.
#[test]
fn long_runs_shape_in_linear_time() {
    let font = Font::from_data(parity_font_data(), 0).expect("the parity font parses");
    let started = std::time::Instant::now();
    let split = run(&font, &"b".repeat(100_000), &latin());
    assert_eq!(split.len(), 200_000);
    assert!(split.iter().step_by(2).all(|g| g.glyph.0 == P_B1));
    let ligated = run(&font, &"fi".repeat(100_000), &latin());
    assert_eq!(ligated.len(), 100_000);
    assert!(ligated.iter().all(|g| g.glyph.0 == P_FI));
    // Every mark finds the same base; the kern table's chain then hangs each glyph from the
    // one before, so each mark rises by the one before it, as HarfBuzz has it.
    let text = String::from("a") + &"\u{301}".repeat(100_000);
    let marked = run(&font, &text, &latin());
    assert_eq!(marked.len(), 100_001);
    assert!(
        (1..)
            .zip(&marked[1..])
            .all(|(k, g)| (g.x_offset, g.y_offset) == (250, 700 * k))
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

fn legacy_font() -> Font {
    Font::from_data(legacy_font_data(), 0).expect("the legacy font parses")
}

fn positions(glyphs: &[ShapedGlyph]) -> Vec<(u16, i32, i32, i32)> {
    glyphs
        .iter()
        .map(|g| (g.glyph.0, g.x_advance, g.x_offset, g.y_offset))
        .collect()
}

/// Shaping without GPOS, checked against HarfBuzz 10.4.0 on the same font: the kern table
/// splits each pair's value, marks sit on the outline of their base, and Hangul jamo
/// compose to the syllables the font has.
#[test]
fn legacy_fonts_kern_and_place_marks_like_harfbuzz() {
    let font = legacy_font();
    let no_kern = [Feature::new(*b"kern", 0)];
    let hang = ShapeOptions {
        script: Script::of('가'),
        ..ShapeOptions::default()
    };
    let shaped = |text: &str, options: &ShapeOptions<'_>| positions(&run(&font, text, options));
    assert_eq!(
        shaped("AVA", &latin()),
        [(L_A, 560, 0, 0), (L_V, 530, -40, 0), (L_A, 570, -30, 0)]
    );
    let unkerned = ShapeOptions {
        features: &no_kern,
        ..latin()
    };
    assert_eq!(
        shaped("AVA", &unkerned),
        [(L_A, 600, 0, 0), (L_V, 600, 0, 0), (L_A, 600, 0, 0)]
    );
    // Above and below the base; the kern table reaches past the mark.
    assert_eq!(
        shaped("e\u{323}\u{301}", &latin()),
        [
            (L_E, 500, 0, 0),
            (L_DOT, 0, -150, -12),
            (L_ACUTE, 0, -150, 12)
        ]
    );
    assert_eq!(
        shaped("A\u{301}V", &latin()),
        [
            (L_A, 560, 0, 0),
            (L_ACUTE, 0, -160, 212),
            (L_V, 560, -40, 0)
        ]
    );
    // COMBINING GRAPHEME JOINER shows as an empty space and the marks around it still
    // find the base.
    assert_eq!(
        shaped("e\u{301}\u{34F}\u{323}", &latin()),
        [
            (L_E, 500, 0, 0),
            (L_ACUTE, 0, -150, 12),
            (1, 0, 0, 0),
            (L_DOT, 0, -150, -12)
        ]
    );
    // A selector the font has no sequence for is not a mark: the acute after it has no base
    // and is placed against the selector's empty glyph.
    assert_eq!(
        shaped("A\u{FE00}\u{301}", &latin()),
        [(L_A, 600, 0, 0), (1, 0, 0, 0), (L_ACUTE, 0, 350, -244)]
    );
    assert_eq!(
        shaped("\u{1100}\u{1161}\u{11A8}", &hang),
        [(L_GAK, 1000, 0, 0)]
    );
    assert_eq!(shaped("\u{1100}\u{1161}", &hang), [(L_GA, 1000, 0, 0)]);
    assert_eq!(shaped("\u{AC00}\u{11A8}", &hang), [(L_GAK, 1000, 0, 0)]);
    // Hangul marks are not placed by the fallback.
    assert_eq!(
        shaped("\u{1100}\u{1161}\u{301}", &hang),
        [(L_GA, 1000, 0, 0), (L_ACUTE, 0, 0, 0)]
    );
    // Jamo that make no syllable stay apart.
    assert_eq!(
        shaped("\u{1100}\u{11A8}\u{1161}", &hang),
        [
            (L_KIYEOK, 1000, 0, 0),
            (L_KIYEOK_FINAL, 1000, 0, 0),
            (L_A_JAMO, 1000, 0, 0)
        ]
    );
}

/// Texts that reach every stage: ligatures, kerning, marks to attach, place by outline,
/// reorder and compose, joiners, variation selectors, spaces and Hangul.
const ROBUSTNESS_TEXTS: [&str; 6] = [
    "fi AV e\u{301}o\u{308} a\u{323}\u{301}",
    "f\u{200D}i f\u{200C}i e\u{34F}\u{301} \u{2003}\u{2011}",
    "葛\u{E0100}葛\u{E0101} A\u{FE00}\u{301}",
    "\u{1100}\u{1161}\u{11A8} \u{AC00}\u{11A8}",
    "AVA e\u{323}\u{301}\u{34F}\u{323}",
    "\u{301}\u{301}\u{301} \u{200C}\u{200C} \u{E0020}",
];

/// Shapes every robustness text in every way the tests do; what matters is that it returns.
fn shape_everything(data: &[u8]) {
    let Ok(font) = Font::from_data(data.to_vec(), 0) else {
        return;
    };
    let turkish = ShapeOptions {
        language: Language::from_bcp47("tr"),
        ..latin()
    };
    let hang = ShapeOptions {
        script: Script::of('가'),
        ..ShapeOptions::default()
    };
    let features = [Feature::new(*b"kern", 0), Feature::new(*b"salt", 3)];
    let custom = ShapeOptions {
        features: &features,
        ..latin()
    };
    for text in ROBUSTNESS_TEXTS {
        for options in [&latin(), &turkish, &hang, &custom] {
            let glyphs = run(&font, text, options);
            assert!(glyphs.iter().all(|g| g.cluster <= text.len()));
        }
        let _ = itemize(text, &[&font, &font]);
    }
}

#[test]
fn truncated_fonts_shape_cleanly() {
    for data in [test_font_data(), legacy_font_data()] {
        for len in 0..data.len() {
            shape_everything(&data[..len]);
        }
    }
}

#[test]
fn corrupted_fonts_shape_cleanly() {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |below: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % below as u64) as usize
    };
    for data in [test_font_data(), legacy_font_data()] {
        // The layout tables: the font tests corrupt the rest.
        let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
        let ranges: Vec<(usize, usize)> = (0..tables)
            .filter_map(|i| {
                let record = &data[12 + 16 * i..28 + 16 * i];
                let field = |at: usize| {
                    u32::from_be_bytes(record[at..at + 4].try_into().expect("four bytes")) as usize
                };
                matches!(&record[..4], b"GDEF" | b"GSUB" | b"GPOS" | b"kern")
                    .then(|| (field(8), field(12)))
            })
            .collect();
        assert!(!ranges.is_empty());
        for _ in 0..1500 {
            let mut copy = data.clone();
            for _ in 0..1 + next(8) {
                let (start, len) = ranges[next(ranges.len())];
                let at = start + next(len);
                copy[at] = match next(4) {
                    0 => 0,
                    1 => 0xFF,
                    _ => next(256) as u8,
                };
            }
            shape_everything(&copy);
        }
    }
}

fn run(font: &Font, text: &str, options: &ShapeOptions<'_>) -> Vec<ShapedGlyph> {
    shape(font, text, options)
}

fn latin() -> ShapeOptions<'static> {
    ShapeOptions {
        script: Script::of('a'),
        ..ShapeOptions::default()
    }
}

fn ids(glyphs: &[ShapedGlyph]) -> Vec<u16> {
    glyphs.iter().map(|g| g.glyph.0).collect()
}

fn clusters(glyphs: &[ShapedGlyph]) -> Vec<usize> {
    glyphs.iter().map(|g| g.cluster).collect()
}

fn advances(glyphs: &[ShapedGlyph]) -> Vec<i32> {
    glyphs.iter().map(|g| g.x_advance).collect()
}

#[test]
fn ligatures_follow_language_and_features() {
    let font = test_font();
    let fi = run(&font, "fi", &latin());
    assert_eq!(ids(&fi), [FI]);
    assert_eq!((clusters(&fi), advances(&fi)), (vec![0], vec![550]));
    let turkish = ShapeOptions {
        language: Language::from_bcp47("tr-TR"),
        ..latin()
    };
    assert_eq!(ids(&run(&font, "fi", &turkish)), [F, I]);
    let off = [Feature::new(*b"liga", 0)];
    assert_eq!(
        ids(&run(
            &font,
            "fi",
            &ShapeOptions {
                features: &off,
                ..latin()
            }
        )),
        [F, I]
    );
    // Common text uses the font's default script, here latn by fallback.
    assert_eq!(ids(&run(&font, "fi", &ShapeOptions::default())), [FI]);
}

#[test]
fn kerning_and_mark_positioning() {
    let font = test_font();
    assert_eq!(advances(&run(&font, "AV", &latin())), [520, 600]);
    // ö is not in the font: o and the combining diaeresis are, and the mark goes on the
    // base's anchor, 250 - 50 into the 500-unit o.
    let o = run(&font, "ö", &latin());
    assert_eq!(ids(&o), [O, DIAERESIS]);
    assert_eq!(clusters(&o), [0, 0]);
    assert_eq!(
        (o[1].x_advance, o[1].x_offset, o[1].y_offset),
        (0, -300, 600)
    );
}

#[test]
fn normalization_follows_the_font() {
    let font = test_font();
    // The font has é, so both spellings give it.
    assert_eq!(ids(&run(&font, "e\u{301}", &latin())), [E_ACUTE]);
    assert_eq!(ids(&run(&font, "\u{E9}", &latin())), [E_ACUTE]);
    // It has no á: the acute stays a mark, positioned, its advance zeroed.
    let a = run(&font, "\u{E1}", &latin());
    assert_eq!(ids(&a), [A, ACUTE]);
    assert_eq!((a[1].x_advance, a[1].x_offset), (0, -300));
    // Marks are put in canonical order: dot below (220) before acute (230).
    assert_eq!(
        ids(&run(&font, "a\u{301}\u{323}", &latin())),
        [A, DOT_BELOW, ACUTE]
    );
    // Clusters are byte offsets of grapheme clusters.
    assert_eq!(clusters(&run(&font, "e\u{301}f", &latin())), [0, 3]);
    assert_eq!(
        clusters(&run(&font, "ae\u{301}\u{323}f", &latin())),
        [0, 1, 1, 6]
    );
}

#[test]
fn ignorables_spaces_and_missing_characters() {
    let font = test_font();
    // ZWNJ breaks the ligature; ZWJ does not. Both are drawn as zero-width spaces.
    let zwnj = run(&font, "f\u{200C}i", &latin());
    assert_eq!(
        (ids(&zwnj), advances(&zwnj)),
        (vec![F, SPACE_GLYPH, I], vec![300, 0, 250])
    );
    let zwj = run(&font, "f\u{200D}i", &latin());
    assert_eq!(
        (ids(&zwj), advances(&zwj)),
        (vec![FI, SPACE_GLYPH], vec![550, 0])
    );
    // Spaces the font lacks use its space glyph at their own width.
    let spaces = run(&font, "\u{2003}\u{2009}\u{2007}\u{202F}\u{A0}", &latin());
    assert_eq!(ids(&spaces), [SPACE_GLYPH; 5]);
    assert_eq!(advances(&spaces), [1000, 200, 520, 125, 250]);
    // A character no font covers is glyph 0.
    let euro = run(&font, "€", &latin());
    assert_eq!((ids(&euro), advances(&euro)), (vec![0], vec![500]));
    assert!(run(&font, "", &latin()).is_empty());
}

#[test]
fn variation_selectors_pick_glyphs() {
    let font = test_font();
    let han = ShapeOptions {
        script: Script::of('葛'),
        ..ShapeOptions::default()
    };
    // A sequence the font has takes its glyph and drops the selector, as HarfBuzz does; one it
    // lacks keeps the nominal glyph, and the selector shows as the font's space.
    assert_eq!(ids(&run(&font, "葛\u{E0100}", &han)), [KUZU_VARIANT]);
    assert_eq!(ids(&run(&font, "葛\u{E0101}", &han)), [KUZU, SPACE_GLYPH]);
    assert_eq!(ids(&run(&font, "葛", &han)), [KUZU]);
    assert_eq!(clusters(&run(&font, "葛\u{E0100}葛", &han)), [0, 7]);
    assert_eq!(clusters(&run(&font, "葛\u{E0101}葛", &han)), [0, 0, 7]);
}

#[test]
fn itemizing_picks_a_font_for_each_cluster() {
    let (latin_font, kana) = (test_font(), kana_font());
    let fonts = [&latin_font, &kana];
    let latn = Script::from_iso15924("latn").expect("a script");
    let hira = Script::from_iso15924("Hira").expect("a script");
    // The space after あ stays with the kana font and takes its script; é needs the first
    // font, which covers it.
    assert_eq!(
        itemize("Aa あ é", &fonts),
        [
            Run {
                range: 0..3,
                font: 0,
                script: latn
            },
            Run {
                range: 3..7,
                font: 1,
                script: hira
            },
            Run {
                range: 7..9,
                font: 0,
                script: latn
            },
        ]
    );
    // The second font has e but not the acute, so e + acute goes to the first font; a
    // character no font has stays with the font before it.
    assert_eq!(
        itemize("あe\u{301}€", &fonts),
        [
            Run {
                range: 0..3,
                font: 1,
                script: hira
            },
            Run {
                range: 3..9,
                font: 0,
                script: latn
            },
        ]
    );
    // Text that starts with Common characters takes the first real script.
    assert_eq!(
        itemize(" あ", &fonts),
        [
            Run {
                range: 0..1,
                font: 0,
                script: hira
            },
            Run {
                range: 1..4,
                font: 1,
                script: hira
            },
        ]
    );
    assert_eq!(
        itemize("123", &fonts),
        [Run {
            range: 0..3,
            font: 0,
            script: Script::COMMON
        }]
    );
    assert!(itemize("abc", &[]).is_empty());
    assert!(itemize("", &fonts).is_empty());
}

/// A font with only a and no space glyph, so default ignorables are dropped.
fn spaceless_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(3, 10, format12(&[(0x61, 0x61, 1)]))]);
    tt_font(
        &vec![Vec::new(); 2],
        &[(500, 0), (600, 0)],
        &[(b"cmap", cmap)],
    )
}

// Glyphs of the mask font.
const M_A: u16 = 2;
const M_B: u16 = 3;
const M_C: u16 = 4;
const M_ACUTE: u16 = 5;
const M_R1: u16 = 6;
const M_LIG: u16 = 11;
const M_FINAL: u16 = 12;

/// A font for HarfBuzz's feature masks and ligature properties: an alternate lookup that
/// `salt` and `ss01` share, a `rand` lookup, and a ligature of sixteen components that a
/// second ligature takes in, with a mark between the components.
fn mask_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x61, 0x63, u32::from(M_A)),
            (0x301, 0x301, u32::from(M_ACUTE)),
        ]),
    )]);
    let classes: Vec<(u16, u16)> = (1..=M_FINAL)
        .map(|g| match g {
            M_LIG | M_FINAL => (g, 2),
            M_ACUTE => (g, 3),
            _ => (g, 1),
        })
        .collect();
    let r: Vec<u16> = (M_R1..M_R1 + 5).collect();
    let gsub = layout_table(
        *b"DFLT",
        &[],
        &[
            (*b"salt", &[0]),
            (*b"ss01", &[0]),
            (*b"rand", &[1]),
            (*b"liga", &[2, 3]),
        ],
        vec![
            lookup(3, 0, vec![alternates(&[(M_A, &r[..3])])]),
            lookup(3, 0, vec![alternates(&[(M_B, &r)])]),
            lookup(4, 0x8, vec![ligatures(&[(&[M_C; 16], M_LIG)])]),
            lookup(4, 0x8, vec![ligatures(&[(&[M_LIG, M_C], M_FINAL)])]),
        ],
    );
    let gpos = simple_table(
        &[*b"mark"],
        vec![lookup(
            5,
            0,
            vec![mark_to_ligature(
                &[(M_ACUTE, 0, (0, 0))],
                &[(
                    M_FINAL,
                    vec![vec![Some((100, 100))], vec![Some((400, 200))]],
                )],
                1,
            )],
        )],
    );
    let metrics: Vec<(u16, i16)> = (0..=M_FINAL)
        .map(|g| (if g == M_ACUTE { 0 } else { 600 }, 0))
        .collect();
    tt_font(
        &vec![Vec::new(); metrics.len()],
        &metrics,
        &[
            (b"cmap", cmap),
            (b"GDEF", gdef(&classes, &[], &[])),
            (b"GSUB", gsub),
            (b"GPOS", gpos),
        ],
    )
}

// Glyphs of the fallback font.
const N_A: u16 = 2;
const N_ACUTE: u16 = 3;
const N_LIG: u16 = 5;
const N_ONE: u16 = 6;
const N_TWO: u16 = 7;
const N_SLASH: u16 = 8;
const N_L: u16 = 14;
const N_V: u16 = 15;
const N_T: u16 = 16;
const N_OLD_L: u16 = 17;
const N_TONE: u16 = 18;
const N_CIRCLE: u16 = 19;

/// A font without GDEF and GPOS, for what HarfBuzz decides from Unicode properties: a
/// ligature that starts with a mark, automatic fractions, and the Hangul jamo features.
fn fallback_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x31, 0x32, u32::from(N_ONE)),
            (0x61, 0x61, u32::from(N_A)),
            (0x301, 0x302, u32::from(N_ACUTE)),
            (0x1100, 0x1100, u32::from(N_L)),
            (0x1161, 0x1161, u32::from(N_V)),
            (0x11A8, 0x11A8, u32::from(N_T)),
            (0x2044, 0x2044, u32::from(N_SLASH)),
            (0x25CC, 0x25CC, u32::from(N_CIRCLE)),
            (0x302E, 0x302E, u32::from(N_TONE)),
            (0xA97D, 0xA97D, u32::from(N_OLD_L)),
        ]),
    )]);
    // After the cmap's glyphs: the numerators, denominators and fraction slash, then the
    // jamo forms.
    let (numr, dnom, frac, jamo) = (9, 11, 13, 20);
    let gsub = layout_table(
        *b"DFLT",
        &[],
        &[
            (*b"liga", &[0]),
            (*b"numr", &[1]),
            (*b"dnom", &[2]),
            (*b"frac", &[3]),
            (*b"ljmo", &[4]),
            (*b"vjmo", &[5]),
            (*b"tjmo", &[6]),
        ],
        vec![
            lookup(4, 0, vec![ligatures(&[(&[N_ACUTE, N_A], N_LIG)])]),
            lookup(1, 0, vec![single(&[(N_ONE, numr), (N_TWO, numr + 1)])]),
            lookup(1, 0, vec![single(&[(N_ONE, dnom), (N_TWO, dnom + 1)])]),
            lookup(1, 0, vec![single(&[(N_SLASH, frac)])]),
            lookup(1, 0, vec![single(&[(N_L, jamo), (N_OLD_L, jamo + 3)])]),
            lookup(1, 0, vec![single(&[(N_V, jamo + 1)])]),
            lookup(1, 0, vec![single(&[(N_T, jamo + 2)])]),
        ],
    );
    let rect = |x0: i16, y0: i16, x1: i16, y1: i16| {
        simple_glyph(&[&[
            (x0, y0, true),
            (x0, y1, true),
            (x1, y1, true),
            (x1, y0, true),
        ]])
    };
    let count = usize::from(jamo) + 4;
    let marks = [N_ACUTE, N_ACUTE + 1];
    let glyphs: Vec<Vec<u8>> = (0..count)
        .map(|g| match u16::try_from(g).expect("a glyph id") {
            0 | 1 => Vec::new(),
            m if marks.contains(&m) => rect(-250, 600, -50, 750),
            _ => rect(50, 0, 550, 500),
        })
        .collect();
    let metrics: Vec<(u16, i16)> = (0..count)
        .map(|g| {
            let mark = u16::try_from(g).is_ok_and(|g| marks.contains(&g));
            if mark { (300, -250) } else { (600, 50) }
        })
        .collect();
    tt_font(&glyphs, &metrics, &[(b"cmap", cmap), (b"GSUB", gsub)])
}

/// Shapes `text` with `font` in `script` (glyph, cluster, advance, x and y offsets).
fn shaped_with(
    font: &Font,
    script: char,
    text: &str,
    features: &[Feature],
) -> Vec<(u16, usize, i32, i32, i32)> {
    let options = ShapeOptions {
        script: Script::of(script),
        language: Language::default(),
        features,
    };
    run(font, text, &options)
        .iter()
        .map(|g| (g.glyph.0, g.cluster, g.x_advance, g.x_offset, g.y_offset))
        .collect()
}

/// The mask font shaped as HarfBuzz 10.4.0 shapes it.
#[test]
fn feature_masks_and_long_ligatures_follow_harfbuzz() {
    let font = Font::from_data(mask_font_data(), 0).expect("the mask font parses");
    let shaped = |text: &str, features: &[Feature]| shaped_with(&font, 'a', text, features);
    let aab = |features: &[Feature]| -> Vec<u16> {
        shaped("aab", features).iter().map(|g| g.0).collect()
    };
    let (salt, ss01, rand) = (*b"salt", *b"ss01", *b"rand");
    let r = |k: u16| M_R1 + k;
    // `rand` is on by default and picks HarfBuzz's pseudo-random sequence.
    assert_eq!(
        shaped("abbbbbbbb", &[])
            .iter()
            .map(|g| g.0)
            .collect::<Vec<_>>(),
        [M_A, r(1), r(4), r(2), r(2), r(3), r(1), r(0), r(4)]
    );
    assert_eq!(aab(&[Feature::new(rand, 2)]), [M_A, M_A, r(1)]);
    assert_eq!(aab(&[Feature::new(rand, 0)]), [M_A, M_A, M_B]);
    // Each feature's value picks an alternate of the lookup they share, but both together
    // mix their mask bits into a value that picks none.
    assert_eq!(aab(&[Feature::new(salt, 1)]), [r(0), r(0), r(1)]);
    assert_eq!(aab(&[Feature::new(ss01, 2)]), [r(1), r(1), r(1)]);
    assert_eq!(
        aab(&[Feature::new(salt, 1), Feature::new(ss01, 2)]),
        [M_A, M_A, r(1)]
    );
    // A value keeps its low eight bits.
    assert_eq!(aab(&[Feature::new(salt, 257)]), [r(0), r(0), r(1)]);
    assert_eq!(aab(&[Feature::new(salt, 256)]), [M_A, M_A, r(1)]);
    // Sixteen components count as 0 in HarfBuzz's four bits, so the mark between them
    // attaches to the last component of the ligature that takes this one in.
    let last_component = [(M_FINAL, 0, 600, 0, 0), (M_ACUTE, 0, 0, -200, 200)];
    let text = "c".repeat(15) + "\u{301}cc";
    assert_eq!(shaped(&text, &[]), last_component);
    let text = "c".repeat(16) + "\u{301}c";
    assert_eq!(shaped(&text, &[]), last_component);
}

/// The fallback font shaped as HarfBuzz 10.4.0 shapes it.
#[test]
fn unicode_driven_features_follow_harfbuzz() {
    let font = Font::from_data(fallback_font_data(), 0).expect("the fallback font parses");
    let latin = |text: &str, features: &[Feature]| shaped_with(&font, 'a', text, features);
    let hangul = |text: &str, features: &[Feature]| shaped_with(&font, '가', text, features);
    let (numr, dnom, frac) = (9, 11, 13);
    // A ligature that starts with a mark becomes a letter, and the next mark goes on it.
    assert_eq!(
        latin("\u{301}a\u{302}", &[]),
        [(N_LIG, 0, 600, 0, 0), (N_ACUTE + 1, 0, 0, 0, -19)]
    );
    // Digits around U+2044 FRACTION SLASH become a fraction without asking.
    assert_eq!(
        latin("1\u{2044}2", &[]),
        [
            (numr, 0, 600, 0, 0),
            (frac, 1, 600, 0, 0),
            (dnom + 1, 4, 600, 0, 0)
        ]
    );
    assert_eq!(
        latin("1\u{2044}2", &[Feature::new(*b"frac", 0)]),
        [
            (numr, 0, 600, 0, 0),
            (N_SLASH, 1, 600, 0, 0),
            (dnom + 1, 4, 600, 0, 0)
        ]
    );
    assert_eq!(
        latin("1\u{2044}2", &[Feature::new(*b"numr", 1)]),
        [
            (numr, 0, 600, 0, 0),
            (frac, 1, 600, 0, 0),
            (numr + 1, 4, 600, 0, 0)
        ]
    );
    // A digit between two slashes is a denominator and a numerator; numr applies first.
    assert_eq!(
        latin("12\u{2044}2\u{2044}1", &[]),
        [
            (numr, 0, 600, 0, 0),
            (numr + 1, 1, 600, 0, 0),
            (frac, 2, 600, 0, 0),
            (numr + 1, 5, 600, 0, 0),
            (frac, 6, 600, 0, 0),
            (dnom, 9, 600, 0, 0)
        ]
    );
    // A jamo feature the caller turns on applies to every glyph, not only to jamo that
    // form no syllable.
    assert_eq!(hangul("\u{1100}", &[]), [(N_L, 0, 600, 0, 0)]);
    assert_eq!(
        hangul("\u{1100}", &[Feature::new(*b"ljmo", 1)]),
        [(20, 0, 600, 0, 0)]
    );
    assert_eq!(
        hangul("\u{1161}", &[Feature::new(*b"vjmo", 2)]),
        [(21, 0, 600, 0, 0)]
    );
    // U+A97D is not a leading jamo, so the vowel has no syllable for the tone mark, which
    // then takes a dotted circle after it.
    assert_eq!(
        hangul("\u{A97D}\u{1161}\u{302E}", &[]),
        [
            (N_OLD_L, 0, 600, 0, 0),
            (N_V, 3, 600, 0, 0),
            (N_TONE, 3, 600, 0, 0),
            (N_CIRCLE, 3, 600, 0, 0)
        ]
    );
}

/// Writes the fixture fonts into the directory named by DUYDAS_FIXTURE_DIR, to check the
/// expectations above against another shaper.
#[test]
#[ignore = "writes fixture fonts; run by hand"]
fn write_fixtures_for_comparison() {
    let Some(dir) = std::env::var_os("DUYDAS_FIXTURE_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    for (name, data) in [
        ("test.ttf", test_font_data()),
        ("legacy.ttf", legacy_font_data()),
        ("spaceless.ttf", spaceless_font_data()),
        ("parity.ttf", parity_font_data()),
        ("masks.ttf", mask_font_data()),
        ("fallback.ttf", fallback_font_data()),
    ] {
        std::fs::write(dir.join(name), data).expect("the fixture is written");
    }
}

#[test]
fn dropped_ignorables_pass_their_cluster_on() {
    let font = Font::from_data(spaceless_font_data(), 0).expect("the font parses");
    let shaped = |text: &str| {
        let glyphs = run(&font, text, &latin());
        (ids(&glyphs), clusters(&glyphs))
    };
    // Each dropped space merges into the next glyph, so a takes the first cluster.
    assert_eq!(shaped("\u{200B}\u{200B}a"), (vec![1], vec![0]));
    assert_eq!(shaped("\u{200B}a\u{200B}a"), (vec![1, 1], vec![0, 7]));
    assert_eq!(shaped("\u{200B}\u{200B}"), (vec![], vec![]));
    // A long run of them takes linear time.
    let text = "\u{200B}".repeat(200_000) + "a";
    assert_eq!(shaped(&text), (vec![1], vec![0]));
}

#[test]
fn itemizing_takes_linear_time() {
    let (latin_font, kana) = (test_font(), kana_font());
    // Every cluster starts a new script and font run: 200,000 runs.
    let text = "aあ".repeat(100_000);
    let started = std::time::Instant::now();
    let runs = itemize(&text, &[&latin_font, &kana]);
    assert_eq!(runs.len(), 200_000);
    assert_eq!(runs[199_999].range, text.len() - 3..text.len());
    // Quadratic work would take minutes; linear work takes milliseconds even unoptimized.
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn scripts_and_languages() {
    assert_eq!(Script::of('ş').iso15924(), "Latn");
    assert_eq!(Script::of('あ').iso15924(), "Hira");
    assert_eq!(Script::of('ア').opentype_tag(), Some(*b"kana"));
    assert_eq!(Script::of('漢').opentype_tag(), Some(*b"hani"));
    assert_eq!(Script::of('Ж').opentype_tag(), Some(*b"cyrl"));
    assert_eq!(Script::of('1'), Script::COMMON);
    assert_eq!(Script::of('\u{301}'), Script::INHERITED);
    assert_eq!(Script::of('\u{378}'), Script::UNKNOWN);
    assert_eq!(Script::COMMON.opentype_tag(), None);
    assert_eq!(Script::from_iso15924("Zzzz"), Some(Script::UNKNOWN));
    assert_eq!(Script::from_iso15924("nope"), None);
    let tag = |s: &str| Language::from_bcp47(s).opentype_tag();
    assert_eq!(tag("tr-TR"), Some(*b"TRK "));
    assert_eq!(tag("TR"), Some(*b"TRK "));
    assert_eq!(tag("ja"), Some(*b"JAN "));
    assert_eq!(tag("en_US"), Some(*b"ENG "));
    assert_eq!(tag("az-Latn-AZ"), Some(*b"AZE "));
    assert_eq!(tag("zh-Hant-TW"), Some(*b"ZHT "));
    assert_eq!(tag("zh-HK"), Some(*b"ZHH "));
    assert_eq!(tag("zh-CN"), Some(*b"ZHS "));
    assert_eq!(tag("xx"), None);
    assert_eq!(tag(""), None);
}

#[test]
fn canonical_decompositions_and_compositions() {
    assert_eq!(decomposition('\u{E9}'), Some(('e', Some('\u{301}'))));
    assert_eq!(decomposition('\u{130}'), Some(('I', Some('\u{307}'))));
    assert_eq!(decomposition('\u{212B}'), Some(('\u{C5}', None)));
    assert_eq!(
        decomposition('\u{AC00}'),
        Some(('\u{1100}', Some('\u{1161}')))
    );
    assert_eq!(
        decomposition('\u{AC01}'),
        Some(('\u{AC00}', Some('\u{11A8}')))
    );
    assert_eq!(decomposition('a'), None);
    assert_eq!(composition('e', '\u{301}'), Some('\u{E9}'));
    assert_eq!(composition('I', '\u{307}'), Some('\u{130}'));
    // Composition exclusions do not compose.
    assert_eq!(composition('\u{915}', '\u{93C}'), None);
    assert_eq!(composition('\u{C5}', '\u{301}'), Some('\u{1FA}'));
}

const SAMPLES: [&str; 9] = [
    "Şişli'de ığdır İzmir ĞÜŞİÖÇ fiyat ffi office",
    "The quick brown fox jumps over the lazy dog. AVATAR Wave To",
    "日本語のテキストを表示します。カタカナ、ひらがな、漢字！",
    "Ελληνικά κείμενα και Кириллица текст",
    "e\u{301}a\u{323}\u{302}o\u{308}\u{304} \u{1E09}\u{2003}x\u{200D}y\u{200C}z",
    "葛\u{E0100}葛\u{E0101}辻\u{FE00}👨\u{200D}👩\u{200D}👧 🇹🇷 1️⃣",
    "한국어 텍스트 각",
    "\u{301}\u{301}\u{301} leading marks",
    "a\u{300}\u{301}\u{302}\u{303}\u{304}\u{305}\u{306}\u{307}\u{308}\u{309}\u{30A}\u{30B}\u{30C}",
];

/// Shapes sample text with every installed face: glyphs exist, clusters are grapheme
/// boundaries in order, and no real font exhausts the step budget.
#[test]
fn installed_fonts_shape() {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect(&dir, 0, &mut files);
    }
    files.sort();
    let started = std::time::Instant::now();
    let (mut faces, mut runs, mut failures) = (0, 0, Vec::new());
    let boundaries: Vec<Vec<usize>> = SAMPLES
        .iter()
        .map(|s| {
            let mut b = vec![0];
            let mut at = 0;
            for g in crate::graphemes(s) {
                at += g.len();
                b.push(at);
            }
            b
        })
        .collect();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let data: Arc<[u8]> = bytes.into();
        let Some(count) = Font::face_count(&data) else {
            continue;
        };
        for index in 0..count.min(64) {
            let Ok(font) = Font::from_data(data.clone(), index) else {
                continue;
            };
            faces += 1;
            for (sample, bounds) in SAMPLES.iter().zip(&boundaries) {
                for language in ["", "tr", "ja"] {
                    let script = sample
                        .chars()
                        .map(Script::of)
                        .find(|s| s.is_specific())
                        .unwrap_or(Script::COMMON);
                    let options = ShapeOptions {
                        script,
                        language: Language::from_bcp47(language),
                        features: &[],
                    };
                    let (glyphs, limits) = shape_with_limits(&font, sample, &options);
                    runs += 1;
                    let place = || format!("{}:{index} {sample:?} {language}", path.display());
                    if limits.exhausted() {
                        failures.push(format!("{}: step budget exhausted", place()));
                    }
                    if glyphs.is_empty() {
                        failures.push(format!("{}: no glyphs", place()));
                    }
                    if let Some(g) = glyphs.iter().find(|g| g.glyph.0 >= font.glyph_count()) {
                        failures.push(format!("{}: glyph {} out of range", place(), g.glyph.0));
                    }
                    if glyphs.windows(2).any(|w| w[0].cluster > w[1].cluster)
                        || glyphs.iter().any(|g| !bounds.contains(&g.cluster))
                    {
                        failures.push(format!("{}: clusters {:?}", place(), clusters(&glyphs)));
                    }
                }
            }
        }
    }
    eprintln!(
        "shaped {runs} runs with {faces} faces from {} files in {:?}",
        files.len(),
        started.elapsed()
    );
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Writes this engine's output for every installed face, a set of strings, languages and
/// feature settings to the file named by DUYDAS_SHAPE_DUMP, one run a line, for comparison
/// with another shaper.
#[test]
#[ignore = "writes a comparison file; run by hand"]
fn dump_shaping_for_comparison() {
    use std::fmt::Write as _;
    let Some(out_path) = std::env::var_os("DUYDAS_SHAPE_DUMP") else {
        return;
    };
    // DUYDAS_SHAPE_TEXTS names a file of strings, one a line as hexadecimal code points, to
    // use instead of the built-in ones; DUYDAS_SHAPE_FILTER keeps the font paths containing it.
    let random = match std::env::var_os("DUYDAS_SHAPE_TEXTS") {
        Some(path) => std::fs::read_to_string(path)
            .expect("the texts are readable")
            .lines()
            .map(|line| {
                line.split_whitespace()
                    .filter_map(|h| char::from_u32(u32::from_str_radix(h, 16).ok()?))
                    .collect()
            })
            .collect(),
        None => random_texts(40),
    };
    let filter = std::env::var("DUYDAS_SHAPE_FILTER").unwrap_or_default();
    let mut texts: Vec<&str> = Vec::new();
    if std::env::var_os("DUYDAS_SHAPE_TEXTS").is_none() {
        texts.extend(SAMPLES);
        texts.extend(COMPARISON);
    }
    texts.extend(random.iter().map(String::as_str));
    let configs: [(&str, &str); 9] = [
        ("en", ""),
        ("tr", ""),
        ("ja", ""),
        ("sr", ""),
        ("ro", "smcp"),
        ("en", "-liga,-kern"),
        ("en", "onum,tnum,frac"),
        ("de", "salt=2,ss01,c2sc"),
        ("zh-Hant", "dlig,hlig,case"),
    ];
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect(&dir, 0, &mut files);
    }
    files.sort();
    files.retain(|p| p.to_string_lossy().contains(&filter));
    let mut out = String::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let data: Arc<[u8]> = bytes.into();
        let Some(count) = Font::face_count(&data) else {
            continue;
        };
        for index in 0..count.min(64) {
            let Ok(font) = Font::from_data(data.clone(), index) else {
                continue;
            };
            for text in &texts {
                let script = text
                    .chars()
                    .map(Script::of)
                    .find(|s| s.is_specific())
                    .unwrap_or(Script::COMMON);
                let mut bounds = vec![0];
                for g in crate::graphemes(text) {
                    bounds.push(bounds.last().copied().unwrap_or(0) + g.len());
                }
                let bounds: Vec<String> = bounds.iter().map(ToString::to_string).collect();
                let hex: Vec<String> = text
                    .chars()
                    .map(|c| format!("{:X}", u32::from(c)))
                    .collect();
                for (language, settings) in configs {
                    let features: Vec<Feature> = settings
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            let (name, value) = match s.split_once('=') {
                                Some((n, v)) => (n, v.parse().expect("a feature value")),
                                None if s.starts_with('-') => (&s[1..], 0),
                                None => (s, 1),
                            };
                            Feature::new(
                                name.as_bytes().try_into().expect("a four-letter tag"),
                                value,
                            )
                        })
                        .collect();
                    let options = ShapeOptions {
                        script,
                        language: Language::from_bcp47(language),
                        features: &features,
                    };
                    let cells: Vec<String> = shape(&font, text, &options)
                        .iter()
                        .map(|g| {
                            format!(
                                "{}:{}:{}:{}:{}:{}",
                                g.glyph.0,
                                g.cluster,
                                g.x_advance,
                                g.y_advance,
                                g.x_offset,
                                g.y_offset
                            )
                        })
                        .collect();
                    let _ = writeln!(
                        out,
                        "{}\t{index}\t{}\t{language}\t{settings}\t{}\t{}\t{}",
                        path.display(),
                        script.iso15924(),
                        hex.join(" "),
                        cells.join(","),
                        bounds.join(" ")
                    );
                }
            }
        }
    }
    std::fs::write(out_path, out).expect("the dump is written");
}

/// Deterministic random strings from the scripts the engine handles, with marks, joiners,
/// variation selectors and spaces mixed in.
fn random_texts(count: usize) -> Vec<String> {
    const RANGES: [(u32, u32, u32); 16] = [
        (0x20, 0x7E, 30),
        (0xA0, 0x17F, 12),
        (0x180, 0x24F, 4),
        (0x300, 0x36F, 10),
        (0x370, 0x3FF, 6),
        (0x400, 0x4FF, 6),
        (0x1E00, 0x1EFF, 5),
        (0x2000, 0x206F, 4),
        (0x3000, 0x30FF, 6),
        (0x4E00, 0x4F00, 5),
        (0xAC00, 0xAC40, 2),
        (0x1100, 0x11FF, 2),
        (0xFE00, 0xFE0F, 1),
        (0x200C, 0x200D, 2),
        (0xFF00, 0xFFEF, 2),
        (0x1F300, 0x1F64F, 2),
    ];
    let total: u32 = RANGES.iter().map(|r| r.2).sum();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..count)
        .map(|_| {
            let len = 8 + next() % 40;
            (0..len)
                .filter_map(|_| {
                    let mut pick = (next() % u64::from(total)) as u32;
                    let &(lo, hi, _) = RANGES
                        .iter()
                        .find(|r| {
                            if pick < r.2 {
                                true
                            } else {
                                pick -= r.2;
                                false
                            }
                        })
                        .expect("a range for every pick");
                    char::from_u32(lo + (next() % u64::from(hi - lo + 1)) as u32)
                })
                .collect()
        })
        .collect()
}

const COMPARISON: [&str; 21] = [
    "fi fl ffi ffl fj Th st ct",
    "Tr Ty Yo Wa LT AV To P. F, V.",
    "İstanbul’da “tırnak” — ½ 1/2 3×4",
    "ǅ ǈ ǋ Å Å Ω Ω ﬁ",
    "a\u{308}\u{301} q\u{307}\u{323} u\u{31B}\u{301}",
    "가각 한글 ㄱㅏ \u{1100}\u{1161}\u{11A8}",
    "ｱｲｳ ＡＢＣ 〜「」『』、。・ー",
    "x\u{FE0F} ❤\u{FE0F} 🏳\u{FE0F}\u{200D}🌈",
    "Ἀθῆναι ᾅ ῷ",
    "Ёжик ў Ї ґ Ӂ",
    "Ծ Ձ ა ბ",
    "1\u{2009}000\u{202F}€ \u{2007}9 a\u{A0}b",
    "\u{200B}a\u{2060}b\u{AD}c\u{34F}d",
    "ﬀ ﬃ Ǆ ĳ",
    "ñ ç ğ ş ı ö ü â î û",
    "漢字かな交じり文、﨑 髙 𠮷",
    // Old Hangul and partial syllables (jamo features), and tone marks.
    "\u{1112}\u{119E}\u{11AB} \u{1100}\u{1161}\u{11F0} \u{AC00}\u{11F0} \u{AC00}\u{11A8} \u{A960}\u{1161} \u{115F}\u{1160} \u{1100}\u{D7B0}",
    "\u{D55C}\u{302E} \u{1112}\u{119E}\u{302F} \u{1100}\u{1161}\u{302E} a\u{302E} \u{302F}\u{302E}",
    // Joiners before marks, which mark positioning does not pass over.
    "a\u{200D}\u{301} x\u{200D}\u{200D}\u{308} i\u{200C}\u{323} f\u{301}i ffi\u{323}\u{301}",
    // Automatic fractions around U+2044 FRACTION SLASH.
    "1\u{2044}2 12\u{2044}345 \u{2044}7 8\u{2044} 1\u{2044}2\u{2044}3 １\u{2044}２",
    // Jamo just outside the ranges that form syllables.
    "\u{A97D}\u{1161}\u{302E} \u{1100}\u{D7C7}\u{11A8} \u{A97C}\u{1161}",
];
