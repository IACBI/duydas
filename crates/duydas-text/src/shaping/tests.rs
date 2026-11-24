//! Shaping tests: a hand-built font with known substitutions and positions, the Unicode
//! data behind normalization, and every installed font.

use std::sync::Arc;

use super::normalize::{composition, decomposition};
use super::*;
use crate::font::layout::tests::{
    Anchors, alternates, cursive, gdef, layout_table, ligatures, lookup, mark_attach,
    mark_to_ligature, multiple, pair, simple_table, single,
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
/// A variation selector after a joiner does not keep the letter before the joiner from
/// being decomposed: HarfBuzz maps a base and its marks as a variation sequence only when
/// the selector is among those marks. The test font has o and the diaeresis but not ö.
#[test]
fn selectors_after_joiners_leave_letters_decomposable() {
    let font = test_font();
    let glyphs = ids(&run(&font, "\u{F6}\u{200D}\u{FE0F}", &latin()));
    assert_eq!(glyphs, [O, DIAERESIS, SPACE_GLYPH, SPACE_GLYPH]);
    let glyphs = ids(&run(&font, "\u{F6}\u{FE0F}", &latin()));
    assert_eq!(glyphs, [0, SPACE_GLYPH]);
}

/// A font without GDEF for COMBINING GRAPHEME JOINER: a, b, à, grave (class 230), dot below
/// (220), U+0315 (232) and CGJ, with IgnoreMarks ligatures `a b` and `à b`.
fn joiner_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x61, 0x62, 2),
            (0xE0, 0xE0, 9),
            (0x300, 0x300, 4),
            (0x315, 0x315, 5),
            (0x323, 0x323, 8),
            (0x34F, 0x34F, 6),
        ]),
    )]);
    let gsub = simple_table(
        &[*b"liga"],
        vec![lookup(
            4,
            0x8,
            vec![ligatures(&[(&[2, 3], JOINED), (&[9, 3], JOINED_GRAVE)])],
        )],
    );
    let metrics = vec![(500u16, 0i16); 11];
    tt_font(
        &vec![Vec::new(); 11],
        &metrics,
        &[(b"cmap", cmap), (b"GSUB", gsub)],
    )
}

const JOINED: u16 = 7;
const JOINED_GRAVE: u16 = 10;

/// Without GDEF, glyph classes come from Unicode, and COMBINING GRAPHEME JOINER is a base
/// even though it is a nonspacing mark. Between marks of classes 232 and 230, which
/// canonical ordering would swap, it keeps them apart, so an IgnoreMarks ligature cannot
/// reach over it; between marks of classes 230 and 232, already in order, it does nothing
/// and is passed over. Whether it keeps marks apart is decided before marks compose again:
/// in a CGJ dot-below b, the grave (230) before the joiner still counts once à composes.
/// HarfBuzz 10.4.0 shapes these texts the same way.
#[test]
fn grapheme_joiners_are_not_marks_without_gdef() {
    let font = Font::from_data(joiner_font_data(), 0).expect("the font parses");
    let kept = ids(&run(&font, "a\u{315}\u{34F}\u{300}b", &latin()));
    assert_eq!(kept, [2, 5, SPACE_GLYPH, 4, 3]);
    // a and grave compose to à, which ligates with b over the joiner.
    let joined = ids(&run(&font, "a\u{300}\u{34F}\u{315}b", &latin()));
    assert_eq!(joined, [JOINED_GRAVE, SPACE_GLYPH, 5]);
    let joined = ids(&run(&font, "a\u{34F}b", &latin()));
    assert_eq!(joined, [JOINED, SPACE_GLYPH]);
    let kept = ids(&run(&font, "a\u{300}\u{34F}\u{323}b", &latin()));
    assert_eq!(kept, [9, SPACE_GLYPH, 8, 3]);
    let kept = ids(&run(&font, "\u{E0}\u{34F}\u{323}b", &latin()));
    assert_eq!(kept, [9, SPACE_GLYPH, 8, 3]);
}

/// A font covers a letter and its marks when shaping composes them into a glyph it has, so
/// fallback picks the font that draws é over one with only e, and the font with 가 over
/// one with only its first jamo. Coverage follows what shaping maps, so a font that would
/// decompose é does not cover it before a variation selector.
#[test]
fn fallback_counts_composed_glyphs() {
    let font = |map: &[(u32, u32, u32)]| {
        let cmap = cmap_table(&[(3, 10, format12(map))]);
        let data = tt_font(&vec![Vec::new(); 3], &[(500, 0); 3], &[(b"cmap", cmap)]);
        Font::from_data(data, 0).expect("the font parses")
    };
    let only_e = font(&[(0x65, 0x65, 1)]);
    let precomposed = font(&[(0x65, 0x65, 1), (0xE9, 0xE9, 2)]);
    let runs = itemize("e\u{301}", &[&only_e, &precomposed]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, 1);
    assert_eq!(ids(&run(&precomposed, "e\u{301}", &latin())), [2]);

    // Before a variation selector é is not decomposed, so a font with e and U+0301 but no é
    // does not cover it (HarfBuzz 10.4.0 draws .notdef there too).
    let decomposed = font(&[(0x65, 0x65, 1), (0x301, 0x301, 2)]);
    assert_eq!(ids(&run(&decomposed, "\u{E9}\u{FE0F}", &latin()))[0], 0);
    let runs = itemize("\u{E9}\u{FE0F}", &[&decomposed, &precomposed]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, 1);
    assert_eq!(itemize("\u{E9}", &[&decomposed, &precomposed])[0].font, 0);

    // Hangul jamo count as covered by a font with the syllable they compose into.
    let only_l = font(&[(0x1100, 0x1100, 1)]);
    let syllable = font(&[(0xAC00, 0xAC00, 1)]);
    let runs = itemize("\u{1100}\u{1161}", &[&only_l, &syllable]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, 1);
    assert_eq!(runs[0].script.opentype_tag(), Some(*b"hang"));
}

/// A font without GPOS whose acute has a single-point contour above its ink: the box its
/// header records reaches 300 units higher than what it draws.
fn extents_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[(0x20, 0x20, 1), (0x61, 0x61, 2), (0x301, 0x301, 3)]),
    )]);
    let glyphs = vec![
        Vec::new(),
        Vec::new(),
        simple_glyph(&[&[
            (0, 0, true),
            (0, 700, true),
            (500, 700, true),
            (500, 0, true),
        ]]),
        simple_glyph(&[
            &[
                (100, 550, true),
                (100, 700, true),
                (200, 700, true),
                (200, 550, true),
            ],
            &[(150, 1000, true)],
        ]),
    ];
    let metrics = [(500, 0), (250, 0), (500, 0), (0, 100)];
    tt_font(&glyphs, &metrics, &[(b"cmap", cmap)])
}

/// Fallback positioning reads a TrueType glyph's box from its header, as HarfBuzz does, so a
/// second acute stacks above the first one's single-point contour. HarfBuzz 10.4.0 gives
/// these positions.
#[test]
fn fallback_marks_use_the_header_box() {
    let font = Font::from_data(extents_font_data(), 0).expect("the font parses");
    let glyphs = run(&font, "a\u{301}\u{301}", &latin());
    assert_eq!(
        positions(&glyphs),
        [(2, 500, 0, 0), (3, 0, -400, 212), (3, 0, -400, 724)]
    );
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
        ..ShapeOptions::default()
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

// Glyphs of the right-to-left font.
const R_ALEF: u16 = 2;
const R_BET: u16 = 3;
const R_GIMEL: u16 = 4;
const R_VAV: u16 = 5;
const R_YOD: u16 = 6;
const R_QAMATS: u16 = 7;
const R_HIRIQ: u16 = 8;
const R_PAREN_LEFT: u16 = 9;
const R_PAREN_RIGHT: u16 = 10;
const R_BRACKET: u16 = 11;
const R_BRACKET_RTLM: u16 = 12;
const R_GIMEL_RTLA: u16 = 13;
const R_GIMEL_LTRA: u16 = 14;
const R_ONE: u16 = 15;
const R_TWO: u16 = 16;
const R_SLASH: u16 = 17;
const R_ONE_NUMR: u16 = 18;
const R_TWO_DNOM: u16 = 19;
const R_ONE_DNOM: u16 = 20;
const R_TWO_NUMR: u16 = 21;

/// A Hebrew font with layout tables for `hebr`: direction forms (`rtla`, `ltra`, and
/// `rtlm` for the bracket, whose mirror image it lacks), fraction forms, cursive attachment
/// (one lookup flagged right to left) and marks above the letters.
fn rtl_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x28, 0x29, u32::from(R_PAREN_LEFT)),
            (0x31, 0x32, u32::from(R_ONE)),
            (0x5B, 0x5B, u32::from(R_BRACKET)),
            (0x5B4, 0x5B4, u32::from(R_HIRIQ)),
            (0x5B8, 0x5B8, u32::from(R_QAMATS)),
            (0x5D0, 0x5D2, u32::from(R_ALEF)),
            (0x5D5, 0x5D5, u32::from(R_VAV)),
            (0x5D9, 0x5D9, u32::from(R_YOD)),
            (0x2044, 0x2044, u32::from(R_SLASH)),
        ]),
    )]);
    let classes: Vec<(u16, u16)> = (1..22)
        .map(|g| match g {
            R_QAMATS | R_HIRIQ => (g, 3),
            _ => (g, 1),
        })
        .collect();
    let gsub = layout_table(
        *b"hebr",
        &[],
        &[
            (*b"dnom", &[3]),
            (*b"ltra", &[4]),
            (*b"numr", &[2]),
            (*b"rtla", &[0]),
            (*b"rtlm", &[1]),
        ],
        vec![
            lookup(1, 0, vec![single(&[(R_GIMEL, R_GIMEL_RTLA)])]),
            lookup(1, 0, vec![single(&[(R_BRACKET, R_BRACKET_RTLM)])]),
            lookup(
                1,
                0,
                vec![single(&[(R_ONE, R_ONE_NUMR), (R_TWO, R_TWO_NUMR)])],
            ),
            lookup(
                1,
                0,
                vec![single(&[(R_ONE, R_ONE_DNOM), (R_TWO, R_TWO_DNOM)])],
            ),
            lookup(1, 0, vec![single(&[(R_GIMEL, R_GIMEL_LTRA)])]),
        ],
    );
    let marks = [(R_QAMATS, 0, (100, 0)), (R_HIRIQ, 0, (50, -10))];
    let bases: Vec<Anchors> = [R_ALEF, R_BET, R_GIMEL, R_GIMEL_RTLA, R_VAV, R_YOD]
        .map(|g| (g, vec![Some((300, -50))]))
        .to_vec();
    let gpos = layout_table(
        *b"hebr",
        &[],
        &[(*b"curs", &[0, 1]), (*b"mark", &[2])],
        vec![
            lookup(
                3,
                0,
                vec![cursive(&[
                    (R_ALEF, Some((500, 100)), Some((0, 50))),
                    (R_BET, Some((550, 0)), Some((20, 80))),
                    (R_GIMEL_RTLA, Some((600, 30)), Some((10, 0))),
                ])],
            ),
            lookup(
                3,
                1,
                vec![cursive(&[
                    (R_VAV, Some((400, 60)), Some((30, 10))),
                    (R_YOD, Some((420, 0)), Some((5, 90))),
                ])],
            ),
            lookup(4, 0, vec![mark_attach(&marks, &bases, 1)]),
        ],
    );
    let advances = [
        500, 250, 500, 550, 600, 300, 250, 0, 0, 300, 300, 300, 310, 610, 590, 500, 500, 100, 400,
        400, 410, 420,
    ];
    let metrics: Vec<(u16, i16)> = advances.iter().map(|&a| (a, 0)).collect();
    tt_font(
        &vec![Vec::new(); advances.len()],
        &metrics,
        &[
            (b"cmap", cmap),
            (b"GDEF", gdef(&classes, &[], &[])),
            (b"GSUB", gsub),
            (b"GPOS", gpos),
        ],
    )
}

// Glyphs of the old-style Hebrew font.
const H_ALEF: u16 = 2;
const H_BET: u16 = 3;
const H_VAV: u16 = 4;
const H_PATAH: u16 = 5;
const H_HOLAM: u16 = 6;
const H_DAGESH: u16 = 7;
const H_SHEVA: u16 = 8;
const H_METEG: u16 = 9;
const H_ALEF_PATAH: u16 = 10;
const H_VAV_HOLAM: u16 = 11;
const H_ALEF_DAGESH: u16 = 12;

/// A Hebrew font in the old style, with outlines, presentation forms and a legacy kern
/// table (alef bet -100, bet alef -60, and with `cross_stream` a subtable raising vav by
/// 50 after bet), and no layout tables but GPOS `mark` for Latin when `latin_marks`.
fn old_hebrew_font_data(cross_stream: bool, latin_marks: bool) -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x5B0, 0x5B0, u32::from(H_SHEVA)),
            (0x5B7, 0x5B7, u32::from(H_PATAH)),
            (0x5B9, 0x5B9, u32::from(H_HOLAM)),
            (0x5BC, 0x5BC, u32::from(H_DAGESH)),
            (0x5BD, 0x5BD, u32::from(H_METEG)),
            (0x5D0, 0x5D1, u32::from(H_ALEF)),
            (0x5D5, 0x5D5, u32::from(H_VAV)),
            (0xFB2E, 0xFB2E, u32::from(H_ALEF_PATAH)),
            (0xFB30, 0xFB30, u32::from(H_ALEF_DAGESH)),
            (0xFB4B, 0xFB4B, u32::from(H_VAV_HOLAM)),
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
    let letter = rect(50, 0, 550, 600);
    let below = rect(-200, -150, -100, -50);
    let glyphs = vec![
        Vec::new(),
        Vec::new(),
        letter.clone(),
        letter.clone(),
        rect(50, 0, 250, 600),
        below.clone(),
        rect(-150, 650, -100, 700),
        rect(-100, 200, -50, 250),
        below.clone(),
        below,
        letter.clone(),
        letter.clone(),
        letter,
    ];
    let advances = [500, 250, 600, 600, 300, 0, 0, 0, 0, 0, 600, 300, 600];
    let metrics: Vec<(u16, i16)> = advances
        .iter()
        .zip([0, 0, 50, 50, 50, -200, -150, -100, -200, -200, 50, 50, 50])
        .map(|(&a, l)| (a, l))
        .collect();
    let subtable = |coverage: u16, pairs: &[(u16, u16, i16)]| {
        let n = pairs.len() as u16;
        let selector = n.ilog2() as u16;
        let range = 6 << selector;
        let mut s = Vec::new();
        for v in [0, 14 + 6 * n, coverage, n, range, selector, 6 * n - range] {
            s.extend(v.to_be_bytes());
        }
        for &(left, right, value) in pairs {
            s.extend(left.to_be_bytes());
            s.extend(right.to_be_bytes());
            s.extend(value.to_be_bytes());
        }
        s
    };
    let mut kern = vec![0, 0, 0, if cross_stream { 2 } else { 1 }];
    kern.extend(subtable(
        0x0001,
        &[(H_ALEF, H_BET, -100), (H_BET, H_ALEF, -60)],
    ));
    if cross_stream {
        kern.extend(subtable(0x0005, &[(H_BET, H_VAV, 50)]));
    }
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"cmap", cmap), (b"kern", kern)];
    if latin_marks {
        let marks = [(H_PATAH, 0, (0, 0)), (H_DAGESH, 0, (0, 0))];
        let gpos = layout_table(
            *b"latn",
            &[],
            &[(*b"mark", &[0])],
            vec![lookup(
                4,
                0,
                vec![mark_attach(&marks, &[(H_ALEF, vec![Some((300, 0))])], 1)],
            )],
        );
        tables.push((b"GPOS", gpos));
    }
    tt_font(&glyphs, &metrics, &tables)
}

fn hebrew(direction: Direction) -> ShapeOptions<'static> {
    ShapeOptions {
        script: Script::of('\u{5D0}'),
        language: Language::from_bcp47("he"),
        direction,
        ..ShapeOptions::default()
    }
}

fn shaped_rtl(font: &Font, text: &str, direction: Direction) -> Vec<(u16, usize, i32, i32, i32)> {
    run(font, text, &hebrew(direction))
        .iter()
        .map(|g| (g.glyph.0, g.cluster, g.x_advance, g.x_offset, g.y_offset))
        .collect()
}

/// Right-to-left shaping of Hebrew with GPOS, checked against HarfBuzz 10.4.0 on the same
/// font: glyphs come out reversed with descending clusters, cursive attachment joins the
/// other way, marks follow their base, brackets mirror (or take `rtlm` without a mirror
/// glyph), and `rtla` replaces `ltra`. Left to right, Hebrew is reversed by grapheme and
/// shaped the same way, unless it is only digits.
#[test]
fn right_to_left_runs_shape_like_harfbuzz() {
    use Direction::{LeftToRight as Ltr, RightToLeft as Rtl};
    let font = Font::from_data(rtl_font_data(), 0).expect("the font parses");
    let shaped = |text: &str, direction| shaped_rtl(&font, text, direction);
    assert_eq!(
        shaped("\u{5D0}\u{5D1}\u{5D2}", Rtl),
        [
            (R_GIMEL_RTLA, 4, 600, 0, 100),
            (R_BET, 2, 530, -20, 50),
            (R_ALEF, 0, 500, 0, 0)
        ]
    );
    assert_eq!(
        shaped("\u{5D0}\u{5D1}\u{5D2}", Ltr),
        [
            (R_ALEF, 0, 500, 0, -20),
            (R_BET, 2, 530, -20, 0),
            (R_GIMEL_LTRA, 4, 590, 0, 0)
        ]
    );
    // A lookup flagged right to left hangs the first glyph from the second.
    assert_eq!(
        shaped("\u{5D5}\u{5D9}", Rtl),
        [(R_YOD, 2, 420, 0, 0), (R_VAV, 0, 270, -30, -10)]
    );
    assert_eq!(
        shaped("\u{5D5}\u{5D9}", Ltr),
        [(R_VAV, 0, 400, 0, 0), (R_YOD, 2, 245, -5, -30)]
    );
    assert_eq!(
        shaped("\u{5D0}\u{5B8}\u{5D1}\u{5B4}\u{5B8}", Rtl),
        [
            (R_HIRIQ, 4, 0, 250, -40),
            (R_QAMATS, 4, 0, 200, -50),
            (R_BET, 4, 550, 0, 0),
            (R_QAMATS, 0, 0, 200, -50),
            (R_ALEF, 0, 500, 0, 0)
        ]
    );
    assert_eq!(
        shaped("(\u{5D0})[", Rtl),
        [
            (R_BRACKET_RTLM, 4, 310, 0, 0),
            (R_PAREN_LEFT, 3, 300, 0, 0),
            (R_ALEF, 1, 500, 0, 0),
            (R_PAREN_RIGHT, 0, 300, 0, 0)
        ]
    );
    // Digits alone stay left to right in a Hebrew run. Right to left, the text stays in
    // logical order while the features apply, so HarfBuzz gives the digits before the
    // slash the denominator form and those after it the numerator form.
    assert_eq!(
        shaped("1\u{2044}2", Ltr),
        [
            (R_ONE_NUMR, 0, 400, 0, 0),
            (R_SLASH, 1, 100, 0, 0),
            (R_TWO_DNOM, 4, 400, 0, 0)
        ]
    );
    assert_eq!(
        shaped("1\u{2044}2", Rtl),
        [
            (R_TWO_NUMR, 4, 420, 0, 0),
            (R_SLASH, 1, 100, 0, 0),
            (R_ONE_DNOM, 0, 410, 0, 0)
        ]
    );
    assert_eq!(
        shaped("\u{5D0} 1\u{2044}2", Ltr),
        [
            (R_ALEF, 0, 500, 0, 0),
            (SPACE_GLYPH, 2, 250, 0, 0),
            (R_ONE_NUMR, 3, 400, 0, 0),
            (R_SLASH, 4, 100, 0, 0),
            (R_TWO_DNOM, 7, 400, 0, 0)
        ]
    );
}

/// Hebrew in an old font, checked against HarfBuzz 10.4.0: without GPOS mark positioning
/// points compose into presentation forms, meteg goes before sheva, the kern table kerns
/// in visual order and marks fall back to their combining classes. A font with a `mark`
/// feature (here only for Latin, so GPOS is left out) keeps the points apart.
#[test]
fn old_hebrew_fonts_shape_like_harfbuzz() {
    use Direction::{LeftToRight as Ltr, RightToLeft as Rtl};
    let font = Font::from_data(old_hebrew_font_data(false, false), 0).expect("the font parses");
    let shaped = |text: &str, direction| shaped_rtl(&font, text, direction);
    assert_eq!(
        shaped("\u{5D0}\u{5D1}\u{5D0}", Rtl),
        [
            (H_ALEF, 4, 550, 0, 0),
            (H_BET, 2, 520, -50, 0),
            (H_ALEF, 0, 570, -30, 0)
        ]
    );
    assert_eq!(
        shaped("\u{5D0}\u{5B7}\u{5D1}", Rtl),
        [(H_BET, 4, 600, 0, 0), (H_ALEF_PATAH, 0, 600, 0, 0)]
    );
    assert_eq!(
        shaped("\u{5D5}\u{5B9}\u{5D0}\u{5BC}", Ltr),
        [(H_VAV_HOLAM, 0, 300, 0, 0), (H_ALEF_DAGESH, 4, 600, 0, 0)]
    );
    assert_eq!(
        shaped("\u{5D1}\u{5B7}\u{5B0}\u{5BD}", Rtl),
        [
            (H_SHEVA, 0, 0, 450, -336),
            (H_METEG, 0, 0, 450, -174),
            (H_PATAH, 0, 0, 450, -12),
            (H_BET, 0, 600, 0, 0)
        ]
    );
    assert_eq!(
        shaped("\u{5D0}\u{5B0}\u{5B7}\u{5BD}", Rtl),
        [
            (H_SHEVA, 0, 0, 450, -174),
            (H_METEG, 0, 0, 450, -12),
            (H_ALEF_PATAH, 0, 600, 0, 0)
        ]
    );
    // A meteg before its letter has a cluster of its own. Left to right it is reversed to
    // after the points, and moving it before the sheva merges the two clusters.
    assert_eq!(
        shaped("\u{5BD}\u{5D0}\u{5B7}\u{5B0}", Ltr),
        [
            (H_SHEVA, 0, 0, 450, -174),
            (H_METEG, 0, 0, 450, -12),
            (H_ALEF_PATAH, 0, 600, 0, 0)
        ]
    );
    assert_eq!(
        shaped("\u{5BD}\u{5D0}\u{5B7}\u{5B0}", Rtl),
        [
            (H_SHEVA, 2, 0, 450, -12),
            (H_ALEF_PATAH, 2, 600, 0, 0),
            (H_METEG, 0, 0, 0, 0)
        ]
    );
    let cross = Font::from_data(old_hebrew_font_data(true, false), 0).expect("the font parses");
    assert_eq!(
        shaped_rtl(&cross, "\u{5D0}\u{5D1}\u{5D5}\u{5B7}", Ltr),
        [
            (H_ALEF, 0, 550, 0, 0),
            (H_BET, 2, 550, -50, 0),
            (H_PATAH, 4, 0, 0, 0),
            (H_VAV, 4, 300, 0, 50)
        ]
    );
    assert_eq!(
        shaped_rtl(&cross, "\u{5D0}\u{5D1}\u{5D5}\u{5B7}", Rtl),
        [
            (H_PATAH, 4, 0, 0, 0),
            (H_VAV, 4, 300, 0, 0),
            (H_BET, 2, 570, 0, 0),
            (H_ALEF, 0, 570, -30, 0)
        ]
    );
    let marked = Font::from_data(old_hebrew_font_data(false, true), 0).expect("the font parses");
    assert_eq!(
        shaped_rtl(&marked, "\u{5D0}\u{5B7}\u{5D1}\u{5BC}", Rtl),
        [
            (H_DAGESH, 4, 0, 375, 0),
            (H_BET, 4, 570, 0, 0),
            (H_PATAH, 0, 0, 450, -12),
            (H_ALEF, 0, 570, -30, 0)
        ]
    );
}

/// Right to left, a removed default ignorable merges into its neighbour as HarfBuzz's
/// in-place deletion does (checked against HarfBuzz 10.4.0), and many of them before a
/// long cluster take linear time.
#[test]
fn right_to_left_dropped_ignorables_merge_backward() {
    let font = Font::from_data(spaceless_font_data(), 0).expect("the font parses");
    let options = ShapeOptions {
        direction: Direction::RightToLeft,
        ..latin()
    };
    let shaped = |text: &str| {
        let glyphs = run(&font, text, &options);
        (ids(&glyphs), clusters(&glyphs))
    };
    assert_eq!(shaped("\u{2060}a"), (vec![1], vec![0]));
    assert_eq!(shaped("a\u{2060}a"), (vec![1, 1], vec![1, 0]));
    let started = std::time::Instant::now();
    let text = "\u{2060}".repeat(100_000) + "a" + &"\u{301}".repeat(100_000);
    let (glyphs, clusters) = shaped(&text);
    assert_eq!(glyphs.len(), 100_001);
    assert!(clusters.iter().all(|&c| c == 0));
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

/// HarfBuzz shapes a run in its script's direction, reversing text written the other way,
/// except digits (or regional indicators) without letters in a right-to-left script.
#[test]
fn runs_are_shaped_in_their_script_direction() {
    assert!(numeric("12"));
    assert!(numeric("\u{1F1F9}\u{1F1F7}"));
    assert!(numeric("1\u{2044}2 %"));
    assert!(!numeric("1a"));
    assert!(!numeric("\u{5D0}1"));
    assert!(!numeric("()"));
    assert_eq!(
        Script::of('\u{5D0}').horizontal_direction(),
        Some(Direction::RightToLeft)
    );
    assert_eq!(
        Script::of('\u{627}').horizontal_direction(),
        Some(Direction::RightToLeft)
    );
    assert_eq!(
        Script::of('a').horizontal_direction(),
        Some(Direction::LeftToRight)
    );
    assert_eq!(
        Script::COMMON.horizontal_direction(),
        Some(Direction::LeftToRight)
    );
    assert_eq!(Script::of('\u{16A0}').horizontal_direction(), None);
    // Latin shaped right to left: clusters reversed as HarfBuzz reverses them (a ZWNJ on its
    // own), marks kept after their base. The ZWNJ's cluster is that of its grapheme, where
    // HarfBuzz gives it its own (4): the documented difference in cluster boundaries.
    let font = test_font();
    let options = ShapeOptions {
        direction: Direction::RightToLeft,
        ..latin()
    };
    let glyphs = run(&font, "e\u{301}a\u{200C}o", &options);
    assert_eq!(ids(&glyphs), [O, SPACE_GLYPH, A, E_ACUTE]);
    assert_eq!(clusters(&glyphs), [7, 3, 3, 0]);
}

// Glyphs of the review font.
const V_BRACKET: u16 = 2;
const V_ALEF: u16 = 3;
const V_A: u16 = 4;
const V_ACUTE: u16 = 5;
const V_ANGSTROM: u16 = 6;
const V_A_RING: u16 = 7;
const V_DIAERESIS: u16 = 8;
const V_GRIN: u16 = 9;
const V_BRACKET_ALTERNATES: u16 = 12;
const V_NA: u16 = 15;
const V_NAK: u16 = 16;
const V_TONE: u16 = 17;
const V_CIRCLE: u16 = 18;
const V_LIGATURE: u16 = 19;

/// A font for corners of HarfBuzz's behaviour found in review: `rtlm` alternates for a
/// bracket it has no mirror for, a ligature of 18 components that marks can follow (only
/// four bits of a component are kept) and no GPOS, so marks fall back to the glyphs' boxes,
/// the Angstrom sign and the letter it decomposes to, an emoji and a star (pictographic in
/// HarfBuzz's emoji data, not in Unicode 18's), jamo, two syllables, a tone mark and a
/// dotted circle.
fn review_font_data() -> Vec<u8> {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x5B, 0x5B, u32::from(V_BRACKET)),
            (0x61, 0x61, u32::from(V_A)),
            (0xC5, 0xC5, u32::from(V_A_RING)),
            (0x301, 0x301, u32::from(V_ACUTE)),
            (0x308, 0x308, u32::from(V_DIAERESIS)),
            (0x5D0, 0x5D0, u32::from(V_ALEF)),
            (0x1100, 0x1100, 10),
            (0x1161, 0x1161, 11),
            (0x11A8, 0x11A8, 14),
            (0x212B, 0x212B, u32::from(V_ANGSTROM)),
            (0x25CC, 0x25CC, u32::from(V_CIRCLE)),
            (0x2605, 0x2605, u32::from(V_GRIN)),
            (0x302E, 0x302E, u32::from(V_TONE)),
            (0xB098, 0xB099, u32::from(V_NA)),
            (0x1F600, 0x1F600, u32::from(V_GRIN)),
        ]),
    )]);
    let classes: Vec<(u16, u16)> = (1..20)
        .map(|g| match g {
            V_ACUTE | V_DIAERESIS => (g, 3),
            V_LIGATURE => (g, 2),
            _ => (g, 1),
        })
        .collect();
    let alternates_of: Vec<u16> = (V_BRACKET_ALTERNATES..V_BRACKET_ALTERNATES + 7).collect();
    let components = [V_A; 18];
    let gsub = simple_table(
        &[*b"liga", *b"rtlm"],
        vec![
            lookup(4, 8, vec![ligatures(&[(&components, V_LIGATURE)])]),
            lookup(3, 0, vec![alternates(&[(V_BRACKET, &alternates_of)])]),
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
    let mut glyphs = vec![Vec::new(), Vec::new()];
    glyphs.extend((2..20).map(|g| match g {
        V_ACUTE | V_DIAERESIS => rect(-150, 550, -50, 700),
        V_LIGATURE => rect(0, 0, 1800, 700),
        _ => rect(50, 0, 450, 500),
    }));
    let metrics: Vec<(u16, i16)> = (0..20u16)
        .map(|g| match g {
            1 => (250, 0),
            V_ACUTE | V_DIAERESIS => (0, -150),
            V_LIGATURE => (1800, 0),
            _ => (500, 50),
        })
        .collect();
    tt_font(
        &glyphs,
        &metrics,
        &[
            (b"cmap", cmap),
            (b"GDEF", gdef(&classes, &[], &[])),
            (b"GSUB", gsub),
        ],
    )
}

/// Corners found in review, checked against HarfBuzz 10.4.0 on the same font.
#[test]
fn review_cases_shape_like_harfbuzz() {
    use Direction::{LeftToRight as Ltr, RightToLeft as Rtl};
    let font = Font::from_data(review_font_data(), 0).expect("the font parses");
    let shaped = |text: &str, script: char, features: &[Feature], direction| {
        let options = ShapeOptions {
            script: Script::of(script),
            features,
            direction,
            ..ShapeOptions::default()
        };
        run(&font, text, &options)
            .iter()
            .map(|g| (g.glyph.0, g.cluster, g.x_advance, g.x_offset, g.y_offset))
            .collect::<Vec<_>>()
    };
    // The bracket's mask gets rtlm's bit for value 1 whatever the value given, on top of
    // the value the caller's setting gives every glyph: alternate 1, then 4 | 1 and 7 | 1.
    let bracket = |n: u16| V_BRACKET_ALTERNATES + n - 1;
    assert_eq!(
        shaped("[\u{5D0}", '\u{5D0}', &[], Rtl),
        [(V_ALEF, 1, 500, 0, 0), (bracket(1), 0, 500, 0, 0)]
    );
    for value in [4, 7] {
        let rtlm = [Feature::new(*b"rtlm", value)];
        assert_eq!(
            shaped("[\u{5D0}", '\u{5D0}', &rtlm, Rtl),
            [
                (V_ALEF, 1, 500, 0, 0),
                (bracket(value as u16 | 1), 0, 500, 0, 0)
            ]
        );
    }
    assert_eq!(
        shaped("[\u{5D0}", '\u{5D0}', &[], Ltr),
        [(V_BRACKET, 0, 500, 0, 0), (V_ALEF, 1, 500, 0, 0)]
    );
    // A mark after component 16 of 18 keeps component 0 (four bits), which fallback
    // positioning takes as -1: a component's width before the ligature, or after it in
    // right-to-left text (Hebrew here; Latin shaped right to left is reversed first, so the
    // mark follows component 3). After component 2 it sits over the second eighteenth.
    let text = "a".repeat(16) + "\u{301}aa";
    assert_eq!(
        shaped(&text, 'a', &[], Ltr),
        [(V_LIGATURE, 0, 1800, 0, 0), (V_ACUTE, 0, 0, -800, 212)]
    );
    assert_eq!(
        shaped(&text, '\u{5D0}', &[], Rtl),
        [(V_ACUTE, 0, 0, 1000, 212), (V_LIGATURE, 0, 1800, 0, 0)]
    );
    assert_eq!(
        shaped(&text, 'a', &[], Rtl),
        [(V_LIGATURE, 0, 1800, 0, 0), (V_ACUTE, 0, 0, -1250, 212)]
    );
    let text = "aa\u{301}".to_string() + &"a".repeat(16);
    assert_eq!(
        shaped(&text, 'a', &[], Ltr),
        [(V_LIGATURE, 0, 1800, 0, 0), (V_ACUTE, 0, 0, -350, 212)]
    );
    // A letter before a joiner is mapped on its own, so the Angstrom sign the font has is
    // kept; before a mark it is decomposed, as a base with marks is.
    assert_eq!(
        shaped("\u{212B}\u{200D}\u{308}", 'a', &[], Ltr),
        [
            (V_ANGSTROM, 0, 500, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_DIAERESIS, 0, 0, 350, -244)
        ]
    );
    assert_eq!(
        shaped("\u{212B}\u{308}", 'a', &[], Ltr),
        [(V_A_RING, 0, 500, 0, 0), (V_DIAERESIS, 0, 0, -150, 12)]
    );
    // An emoji after ZWJ continues HarfBuzz's grapheme cluster though it starts one of
    // Unicode's, so the two graphemes share a cluster; right to left that keeps clusters
    // from increasing. A ZWNJ after it, in the emoji's grapheme but not in HarfBuzz's,
    // shares it too, so no grapheme is split between clusters (HarfBuzz gives it 8).
    assert_eq!(
        shaped("a\u{200D}\u{1F600}", 'a', &[], Ltr),
        [
            (V_A, 0, 500, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_GRIN, 0, 500, 0, 0)
        ]
    );
    assert_eq!(
        shaped("a\u{200D}\u{1F600}\u{200C}", 'a', &[], Rtl),
        [
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_A, 0, 500, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_GRIN, 0, 500, 0, 0)
        ]
    );
    assert_eq!(
        shaped("\u{1100}\u{1161}\u{200D}\u{1F600}", 'a', &[], Rtl),
        [
            (11, 0, 500, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_GRIN, 0, 500, 0, 0),
            (10, 0, 500, 0, 0)
        ]
    );
    // HarfBuzz takes a star after ZWJ as pictographic (Unicode 16 emoji data), so it
    // continues the cluster, though Unicode 18 starts a grapheme there.
    assert_eq!(
        shaped("a\u{200D}\u{2605}", 'a', &[], Rtl),
        [
            (V_A, 0, 500, 0, 0),
            (SPACE_GLYPH, 0, 0, 0, 0),
            (V_GRIN, 0, 500, 0, 0)
        ]
    );
    // Hangul edits merge clusters as HarfBuzz's buffer does: a tone mark moved in front of
    // its syllable (here after reversal), a syllable composed of jamo from two clusters.
    assert_eq!(
        shaped("\u{302E}\u{B098}", '\u{B098}', &[], Rtl),
        [(V_TONE, 0, 500, 0, 0), (V_NA, 0, 500, 0, 0)]
    );
    assert_eq!(
        shaped("\u{11A8}\u{301}\u{B098}", '\u{B098}', &[], Rtl),
        [(V_NAK, 0, 500, 0, 0), (V_ACUTE, 0, 0, 0, 0)]
    );
    // A tone mark with no syllable before it gets a dotted circle that keeps the mark's
    // combining class, so the acute before them sorts after both.
    assert_eq!(
        shaped("\u{B098}\u{301}\u{302E}", '\u{B098}', &[], Ltr),
        [
            (V_NA, 0, 500, 0, 0),
            (V_TONE, 0, 500, 0, 0),
            (V_CIRCLE, 0, 500, 0, 0),
            (V_ACUTE, 0, 0, 0, 0)
        ]
    );
}

/// Font fallback counts what shaping draws for Hebrew: a font without GPOS marks composes
/// a letter and its point into a presentation form, so it covers the pair through that
/// form alone.
#[test]
fn itemizing_hebrew_counts_presentation_forms() {
    let font = |ranges: &[(u32, u32, u32)]| {
        let cmap = cmap_table(&[(3, 10, format12(ranges))]);
        let data = tt_font(
            &vec![Vec::new(); 3],
            &[(500, 0), (250, 0), (600, 0)],
            &[(b"cmap", cmap)],
        );
        Font::from_data(data, 0).expect("the font parses")
    };
    let letters = font(&[(0x20, 0x20, 1), (0x5D0, 0x5D0, 2)]);
    let forms = font(&[(0x20, 0x20, 1), (0xFB2E, 0xFB2E, 2)]);
    let fonts = [&letters, &forms];
    let runs = itemize("\u{5D0}\u{5B7}", &fonts);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, 1);
    let options = ShapeOptions {
        script: runs[0].script,
        direction: Direction::RightToLeft,
        ..ShapeOptions::default()
    };
    assert_eq!(ids(&run(&forms, "\u{5D0}\u{5B7}", &options)), [2]);
    // A letter alone stays with the first font.
    assert_eq!(itemize("\u{5D0}", &fonts)[0].font, 0);
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
        ("extents.ttf", extents_font_data()),
        ("joiner.ttf", joiner_font_data()),
        ("rtl.ttf", rtl_font_data()),
        ("review.ttf", review_font_data()),
        ("hebrew.ttf", old_hebrew_font_data(false, false)),
        ("hebrew-cross.ttf", old_hebrew_font_data(true, false)),
        ("hebrew-latin-marks.ttf", old_hebrew_font_data(false, true)),
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
    // Chinese as HarfBuzz 10.4.0 tags it (its OpenType tags from script and language): an
    // explicit script beats the region, and Macao falls back to Hong Kong.
    let (zhs, zht, zhh) = (Some(*b"ZHS "), Some(*b"ZHT "), Some(*b"ZHH "));
    for (bcp47, tags) in [
        ("zh", [zhs, None]),
        ("zh-SG", [zhs, None]),
        ("zh-TW", [zht, None]),
        ("zh_TW", [zht, None]),
        ("zh-hk", [zhh, None]),
        ("zh-MO", [Some(*b"ZHTM"), zhh]),
        ("zh-Hans", [zhs, None]),
        ("zh-Hant", [zht, None]),
        ("zh-Hans-TW", [zhs, None]),
        ("zh-Hans-HK", [zhs, None]),
        ("zh-Hans-MO", [zhs, None]),
        ("zh-Hant-HK", [zhh, None]),
        ("zh-Hant-MO", [Some(*b"ZHTM"), zhh]),
        ("zh-Hant-CN", [zht, None]),
        ("zh-Latn-TW", [zht, None]),
        ("zh-TW-x-hant", [zht, None]),
    ] {
        assert_eq!(Language::from_bcp47(bcp47).opentype_tags(), tags, "{bcp47}");
    }
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

const SAMPLES: [&str; 11] = [
    "Şişli'de ığdır İzmir ĞÜŞİÖÇ fiyat ffi office",
    "The quick brown fox jumps over the lazy dog. AVATAR Wave To",
    "日本語のテキストを表示します。カタカナ、ひらがな、漢字！",
    "Ελληνικά κείμενα και Кириллица текст",
    "e\u{301}a\u{323}\u{302}o\u{308}\u{304} \u{1E09}\u{2003}x\u{200D}y\u{200C}z",
    "葛\u{E0100}葛\u{E0101}辻\u{FE00}👨\u{200D}👩\u{200D}👧 🇹🇷 1️⃣",
    "한국어 텍스트 각",
    "\u{301}\u{301}\u{301} leading marks",
    "a\u{300}\u{301}\u{302}\u{303}\u{304}\u{305}\u{306}\u{307}\u{308}\u{309}\u{30A}\u{30B}\u{30C}",
    // Hebrew with points (patah, sheva and meteg, which the Hebrew shaper reorders),
    // brackets that mirror, digits and a fraction.
    "שָׁלוֹם (עוֹלָם) [א\u{5B0}\u{5B7}\u{5BD}] 12 1\u{2044}2 בְּרֵאשִׁית",
    "١٢٣ «مرحبا» (abc)",
];

/// Shapes sample text with every installed face: glyphs exist, clusters are grapheme
/// boundaries in order (descending right to left), and no real font exhausts the step
/// budget.
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
                for (language, direction) in [
                    ("", Direction::LeftToRight),
                    ("tr", Direction::LeftToRight),
                    ("ja", Direction::LeftToRight),
                    ("", Direction::RightToLeft),
                ] {
                    let script = sample
                        .chars()
                        .map(Script::of)
                        .find(|s| s.is_specific())
                        .unwrap_or(Script::COMMON);
                    let options = ShapeOptions {
                        script,
                        language: Language::from_bcp47(language),
                        features: &[],
                        direction,
                    };
                    let (glyphs, limits) = shape_with_limits(&font, sample, &options);
                    runs += 1;
                    let place = || {
                        format!(
                            "{}:{index} {sample:?} {language} {direction:?}",
                            path.display()
                        )
                    };
                    if limits.exhausted() {
                        failures.push(format!("{}: step budget exhausted", place()));
                    }
                    if glyphs.is_empty() {
                        failures.push(format!("{}: no glyphs", place()));
                    }
                    if let Some(g) = glyphs.iter().find(|g| g.glyph.0 >= font.glyph_count()) {
                        failures.push(format!("{}: glyph {} out of range", place(), g.glyph.0));
                    }
                    let rtl = direction == Direction::RightToLeft;
                    if glyphs.windows(2).any(|w| {
                        (w[0].cluster > w[1].cluster) != rtl && w[0].cluster != w[1].cluster
                    }) || glyphs.iter().any(|g| !bounds.contains(&g.cluster))
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
    let configs: [(&str, &str, Direction); 12] = [
        ("en", "", Direction::LeftToRight),
        ("tr", "", Direction::LeftToRight),
        ("ja", "", Direction::LeftToRight),
        ("sr", "", Direction::LeftToRight),
        ("ro", "smcp", Direction::LeftToRight),
        ("en", "-liga,-kern", Direction::LeftToRight),
        ("en", "onum,tnum,frac", Direction::LeftToRight),
        ("de", "salt=2,ss01,c2sc", Direction::LeftToRight),
        ("zh-Hant", "dlig,hlig,case", Direction::LeftToRight),
        ("he", "", Direction::RightToLeft),
        ("en", "frac", Direction::RightToLeft),
        ("en", "-kern", Direction::RightToLeft),
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
                for (language, settings, direction) in configs {
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
                        direction,
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
                        "{}\t{index}\t{}\t{language}\t{settings}\t{}\t{}\t{}\t{}",
                        path.display(),
                        script.iso15924(),
                        if direction == Direction::RightToLeft {
                            "rtl"
                        } else {
                            "ltr"
                        },
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

const COMPARISON: [&str; 26] = [
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
    // Right-to-left and mixed text: Hebrew points in every order, presentation forms,
    // mirrored brackets, Hebrew digits runs and Latin, Arabic-script and Syriac letters.
    "\u{5D1}\u{5BC}\u{5B8} \u{5E9}\u{5C1}\u{5BC} \u{5D9}\u{5B4} \u{5D5}\u{5B9} \u{5D0}\u{5B7}\u{5B0}\u{5BD}\u{591} \u{FB2A}\u{5BC}",
    "\u{5D0}\u{5B8}\u{5B4}\u{5C5} \u{5E4}\u{5BF} \u{5D3}\u{5BC}\u{5B0}\u{5BD} \u{5F2}\u{5B7} <{[(\u{5D0})]}> \u{2264}\u{226E}",
    "123\u{2044}456 \u{1F1F9}\u{1F1F7} 7,5%",
    "Hello, \u{5E2}\u{5D5}\u{5DC}\u{5DD}! (\u{2018}x\u{2019}) \u{2039}y\u{203A}",
    "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64A}\u{629} \u{710}\u{712}\u{730} \u{7C0}\u{7CA}\u{7EB}",
];
