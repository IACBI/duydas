//! Layout tests, on hand-built fonts whose advances are exact in binary at the test size.

use std::time::{Duration, Instant};

use super::*;
use crate::font::layout::tests::{layout_table, ligatures, lookup, pair};
use crate::font::tests::{cmap_table, collect, font_dirs, format12, hhea, tt_font};

/// 1,000 units per em at this size is 1/64 px a unit, so every sum below is exact.
const SIZE: f32 = 15.625;

// Glyphs of the Latin font: a to z from 2, the fi ligature, Hebrew alef to tav from 29,
// parentheses, digits from 58.
const F: u16 = 2 + 5;
const I: u16 = 2 + 8;
const FI: u16 = 28;
const ALEF: u16 = 29;
const OPEN: u16 = 56;
const CLOSE: u16 = 57;

/// Font units at the test size, in pixels.
fn px(units: i32) -> f32 {
    units as f32 / 64.0
}

/// Letters are 500 units wide, the space 250, the fi ligature 700, Hebrew letters 600 and the
/// parentheses 300; ascender 800, descender 200.
fn latin() -> Font {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[
            (0x20, 0x20, 1),
            (0x28, 0x28, u32::from(OPEN)),
            (0x29, 0x29, u32::from(CLOSE)),
            (0x30, 0x39, 58),
            (0x61, 0x7A, 2),
            (0x5D0, 0x5EA, u32::from(ALEF)),
        ]),
    )]);
    let gsub = layout_table(
        *b"latn",
        &[],
        &[(*b"liga", &[0])],
        vec![lookup(4, 0, vec![ligatures(&[(&[F, I], FI)])])],
    );
    let advances: Vec<(u16, i16)> = (0..68u16)
        .map(|g| match g {
            1 => 250,
            FI => 700,
            29..=55 => 600,
            OPEN | CLOSE => 300,
            _ => 500,
        })
        .map(|a| (a, 0))
        .collect();
    let data = tt_font(
        &vec![Vec::new(); advances.len()],
        &advances,
        &[(b"cmap", cmap), (b"GSUB", gsub)],
    );
    Font::from_data(data, 0).expect("the Latin font parses")
}

/// A space, あ and 。 (1,000 units); ascender 900, descender 300 and a line gap of 100.
fn kana() -> Font {
    let cmap = cmap_table(&[(
        3,
        10,
        format12(&[(0x20, 0x20, 1), (0x3002, 0x3002, 3), (0x3042, 0x3042, 2)]),
    )]);
    let data = tt_font(
        &vec![Vec::new(); 4],
        &[(500, 0), (250, 0), (1000, 0), (1000, 0)],
        &[(b"cmap", cmap), (b"hhea", hhea(900, -300, 100, 4))],
    );
    Font::from_data(data, 0).expect("the kana font parses")
}

/// `a` and `b` (glyphs 1 and 2) and glyph 3, 500, 500 and 6,000 units wide, with no space
/// glyph, and the tables given.
fn ab(tables: &[(&[u8; 4], Vec<u8>)]) -> Font {
    ab_with([500, 500, 500, 6000], tables)
}

/// [`ab`] with other advances for the .notdef glyph, a, b and glyph 3.
fn ab_with(advances: [u16; 4], tables: &[(&[u8; 4], Vec<u8>)]) -> Font {
    let mut extra = vec![(
        b"cmap",
        cmap_table(&[(3, 10, format12(&[(0x61, 0x62, 1)]))]),
    )];
    extra.extend(tables.iter().cloned());
    let metrics: Vec<(u16, i16)> = advances.iter().map(|&a| (a, 0)).collect();
    let data = tt_font(&vec![Vec::new(); 4], &metrics, &extra);
    Font::from_data(data, 0).expect("the font parses")
}

fn options() -> TextOptions<'static> {
    TextOptions {
        size: SIZE,
        ..TextOptions::default()
    }
}

fn shaped(text: &str) -> ShapedText {
    shaped_with(text, &options())
}

fn shaped_with(text: &str, options: &TextOptions<'_>) -> ShapedText {
    ShapedText::new(text, &[&latin(), &kana()], options)
}

/// Each line's bytes, as pairs (Clippy reads an array of one range as a mistake).
fn ranges(layout: &Layout) -> Vec<(usize, usize)> {
    layout
        .lines()
        .iter()
        .map(|l| (l.range.start, l.range.end))
        .collect()
}

fn line_glyphs(layout: &Layout, line: usize) -> &[PlacedGlyph] {
    &layout.glyphs()[layout.lines()[line].glyphs.clone()]
}

fn ids(glyphs: &[PlacedGlyph]) -> Vec<u16> {
    glyphs.iter().map(|g| g.glyph.0).collect()
}

fn clusters(glyphs: &[PlacedGlyph]) -> Vec<usize> {
    glyphs.iter().map(|g| g.cluster).collect()
}

/// Whether a grapheme cluster is text that draws something, which can make a line overflow.
fn draws(grapheme: &str) -> bool {
    kind(grapheme) == Kind::Text
        && !grapheme
            .chars()
            .all(|c| char_info(c) & DEFAULT_IGNORABLE != 0)
}

/// Whether the cells of line `n` but those that hang at its end lie in its content box.
fn in_box(layout: &Layout, text: &str, n: usize) -> bool {
    let line = &layout.lines[n];
    let cells = layout.line_cells[n].clone();
    let mut content = cells.end;
    while content > cells.start
        && !draws(&text[layout.cells[content - 1].start..layout.cell_end(content - 1)])
    {
        content -= 1;
    }
    let slack = 1.0 / 1024.0;
    layout.cells[cells.start..content]
        .iter()
        .all(|c| c.left >= line.x - slack && c.right <= line.x + line.width + slack)
}

/// Walks the caret over the whole layout from the near end of its first line, to the right
/// with `right`, and checks that it stops and passes every edge of every cell that takes room,
/// but where it comes into a line at the offset that the line before ends at.
fn walk(layout: &Layout, right: bool, context: &str) {
    let mut position = layout.hit_line(
        0,
        if right {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        },
    );
    let mut seen: Vec<(usize, f32)> = Vec::new();
    let limit = 2 * layout.cells.len() + layout.lines.len() + 2;
    loop {
        let caret = layout.caret(position);
        seen.push((caret.line, caret.x));
        let next = if right {
            layout.move_right(position)
        } else {
            layout.move_left(position)
        };
        if next == position {
            break;
        }
        position = next;
        assert!(seen.len() <= limit, "{context}: the caret does not stop");
    }
    for (n, order) in layout.line_order.iter().enumerate() {
        let order = &layout.order[order.clone()];
        let entry = if right {
            order.first().map(|&k| layout.cells[k].left)
        } else {
            order.last().map(|&k| layout.cells[k].right)
        };
        for &k in order {
            for x in [layout.cells[k].left, layout.cells[k].right] {
                assert!(
                    seen.iter().any(|&(l, at)| l == n && at == x) || n > 0 && Some(x) == entry,
                    "{context}: line {n} at {x} is not visited"
                );
            }
        }
    }
}

#[test]
fn lines_break_at_the_last_opportunity_that_fits() {
    let layout = shaped("aaa bbb ccc").layout(Some(px(3800)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 8), (8, 11)]);
    let lines = layout.lines();
    // The space at the end of the first line hangs: it is not in the width.
    assert_eq!((lines[0].width, lines[1].width), (px(3250), px(1500)));
    assert_eq!((lines[0].top, lines[1].top), (0.0, 15.625));
    assert_eq!((lines[0].height, lines[0].baseline), (15.625, 12.5));
    assert_eq!(lines[1].baseline, 28.125);
    assert_eq!(line_glyphs(&layout, 1)[0].x, 0.0);
    assert_eq!(line_glyphs(&layout, 1)[0].y, 28.125);
    assert_eq!(layout.width(), px(3250));
    assert_eq!(layout.height(), 31.25);

    // A line that fits exactly stays one line.
    assert_eq!(
        ranges(&shaped("aaa bbb").layout(Some(px(3250)), Align::Start)),
        [(0, 7)]
    );
    assert_eq!(
        ranges(&shaped("aaa bbb").layout(Some(px(3248)), Align::Start)),
        [(0, 4), (4, 7)]
    );
    // Without a width, only line breaks end lines.
    assert_eq!(
        ranges(&shaped("aaa bbb ccc").layout(None, Align::Start)),
        [(0, 11)]
    );
}

#[test]
fn words_wider_than_the_line_break_between_graphemes() {
    let layout = shaped("aaaaaaaaaa").layout(Some(px(1100)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 2), (2, 4), (4, 6), (6, 8), (8, 10)]);
    // A line holds at least one grapheme cluster, even when it is wider.
    let layout = shaped("ae\u{301}").layout(Some(0.0), Align::Start);
    assert_eq!(ranges(&layout), [(0, 1), (1, 4)]);
    assert_eq!(
        ranges(&shaped("ab").layout(Some(f32::NAN), Align::Start)),
        [(0, 1), (1, 2)]
    );
    // An infinite width is no width.
    assert_eq!(
        ranges(&shaped("aaa bbb").layout(Some(f32::INFINITY), Align::Start)),
        [(0, 7)]
    );
}

#[test]
fn line_breaks_end_lines_and_draw_nothing() {
    let layout = shaped("ab\ncd\r\n").layout(None, Align::Start);
    assert_eq!(ranges(&layout), [(0, 3), (3, 7), (7, 7)]);
    assert_eq!(layout.glyphs().len(), 4);
    assert_eq!(layout.caret(Position::new(7)).line, 2);
    assert_eq!(layout.caret(Position::new(7)).x, 0.0);
    let before_break = layout.caret(Position::new(2));
    assert_eq!((before_break.line, before_break.x), (0, px(1000)));
    // Clicking past a line's end stops before its line break.
    assert_eq!(layout.hit(100.0, 1.0), Position::new(2));
    assert_eq!(layout.hit(100.0, 20.0), Position::new(5));
    assert_eq!(layout.line_end(0), Position::new(2));
    assert_eq!(layout.line_end(1), Position::new(5));
    assert_eq!(layout.line_end(2), Position::new(7));
    assert_eq!(layout.line_start(1), Position::new(3));
}

#[test]
fn right_to_left_paragraphs_start_on_the_right() {
    // Alef bet, a space, gimel dalet.
    let text = "\u{5D0}\u{5D1} \u{5D2}\u{5D3}";
    let layout = shaped(text).layout(Some(px(10000)), Align::Start);
    let line = &layout.lines()[0];
    assert_eq!(line.direction, Direction::RightToLeft);
    assert_eq!((line.x, line.width), (px(10000 - 2650), px(2650)));
    assert_eq!(clusters(line_glyphs(&layout, 0)), [7, 5, 4, 2, 0]);
    assert_eq!(line_glyphs(&layout, 0)[0].x, line.x);

    // The space at the end of a right-to-left line hangs on its left.
    let layout = shaped(text).layout(Some(px(1500)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 5), (5, 9)]);
    let first = line_glyphs(&layout, 0);
    assert_eq!(clusters(first), [4, 2, 0]);
    assert_eq!(layout.lines()[0].x, px(300));
    assert_eq!(first[0].x, px(50));
    // The empty line after a final line break continues the paragraph's direction.
    let layout = shaped("\u{5D0}\n").layout(Some(px(1000)), Align::Start);
    assert_eq!(layout.lines()[1].direction, Direction::RightToLeft);
    assert_eq!(layout.caret(Position::new(3)).x, px(1000));
}

#[test]
fn mixed_lines_are_reordered_and_carets_follow_affinity() {
    // abc, a space, alef bet gimel, a space, def.
    let text = "abc \u{5D0}\u{5D1}\u{5D2} def";
    let layout = shaped(text).layout(None, Align::Start);
    assert_eq!(
        clusters(layout.glyphs()),
        [0, 1, 2, 3, 8, 6, 4, 10, 11, 12, 13]
    );
    // At the start of the Hebrew word: its leading (right) edge, or the space's trailing edge.
    assert_eq!(layout.caret(Position::new(4)).x, px(3550));
    assert_eq!(layout.caret(Position::upstream(4)).x, px(1750));
    assert_eq!(layout.caret(Position::new(10)).x, px(3550));
    assert_eq!(layout.caret(Position::upstream(10)).x, px(1750));
    // Where both affinities are one place, hit testing gives the downstream one.
    assert_eq!(layout.hit(px(1100), 1.0), Position::new(2));
    assert_eq!(layout.hit(px(3500), 1.0), Position::new(4));
    assert_eq!(layout.hit(px(1800), 1.0), Position::upstream(10));

    // The selection of c, the space and alef is two rectangles.
    let rects = layout.selection(2..6);
    let spans: Vec<(f32, f32)> = rects.iter().map(|r| (r.x, r.width)).collect();
    assert_eq!(spans, [(px(1000), px(750)), (px(2950), px(600))]);
    assert!(rects.iter().all(|r| r.y == 0.0 && r.height == 15.625));
    assert!(layout.selection(3..3).is_empty());
}

#[test]
fn arrow_keys_move_one_grapheme_on_screen() {
    let text = "abc \u{5D0}\u{5D1}\u{5D2} def";
    let layout = shaped(text).layout(None, Align::Start);
    let mut edges: Vec<f32> = vec![0.0];
    let mut x = 0;
    for advance in [500, 500, 500, 250, 600, 600, 600, 250, 500, 500, 500] {
        x += advance;
        edges.push(px(x));
    }
    let mut position = Position::new(0);
    let mut seen = vec![layout.caret(position).x];
    for _ in 0..11 {
        position = layout.move_right(position);
        seen.push(layout.caret(position).x);
    }
    assert_eq!(seen, edges);
    assert_eq!(layout.move_right(position), position);
    let mut back = Vec::new();
    for _ in 0..11 {
        position = layout.move_left(position);
        back.push(layout.caret(position).x);
    }
    edges.pop();
    edges.reverse();
    assert_eq!(back, edges);
    assert_eq!(position, Position::new(0));

    // Where a line wraps, the arrow moves past the shared offset to the next grapheme.
    let layout = shaped("aaa bbb").layout(Some(px(2000)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 4), (4, 7)]);
    assert_eq!(layout.line_end(0), Position::upstream(4));
    assert_eq!(layout.move_right(Position::upstream(4)), Position::new(5));
    assert_eq!(layout.move_left(Position::new(4)), Position::new(3));
    assert_eq!(layout.move_right(Position::new(3)), Position::upstream(4));
    assert_eq!(layout.caret(Position::upstream(4)).line, 0);
    assert_eq!(layout.caret(Position::new(4)).line, 1);
}

#[test]
fn ligatures_split_their_width_between_graphemes() {
    let layout = shaped("fi").layout(None, Align::Start);
    assert_eq!(ids(layout.glyphs()), [FI]);
    assert_eq!(layout.caret(Position::new(1)).x, px(350));
    assert_eq!(layout.hit(px(100), 1.0), Position::new(0));
    assert_eq!(layout.hit(px(300), 1.0), Position::new(1));
    assert_eq!(layout.selection(1..2)[0].x, px(350));
}

#[test]
fn runs_cut_by_a_break_are_shaped_again() {
    // fi is one ligature on a line wide enough, and comes apart where the line breaks it.
    let layout = shaped("fifi").layout(Some(px(700)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 2), (2, 4)]);
    assert_eq!(ids(layout.glyphs()), [FI, FI]);
    let layout = shaped("fifi").layout(Some(px(500)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 1), (1, 2), (2, 3), (3, 4)]);
    assert_eq!(ids(layout.glyphs()), [F, I, F, I]);
    // x and f measure 850 units in the ligature's shares but 1,000 once shaped alone, so the
    // break moves back.
    let layout = shaped("xfi").layout(Some(px(900)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 1), (1, 3)]);
    assert_eq!(ids(layout.glyphs()), [2 + 23, FI]);
}

#[test]
fn tabs_advance_to_the_next_stop() {
    let four = TextOptions {
        tab_size: 4,
        ..options()
    };
    let x_of_b = |text: &str| {
        let layout = shaped_with(text, &four).layout(None, Align::Start);
        layout.glyphs().last().expect("a glyph").x
    };
    // Stops every four spaces, 1,000 units.
    assert_eq!(x_of_b("a\tb"), px(1000));
    assert_eq!(x_of_b("aaa\tb"), px(2000));
    // A stop less than half a space away is passed over.
    assert_eq!(x_of_b("aa(((\tb"), px(3000));
    let none = TextOptions {
        tab_size: 0,
        ..options()
    };
    let layout = shaped_with("a\tb", &none).layout(None, Align::Start);
    assert_eq!(layout.glyphs()[1].x, px(500));
}

#[test]
fn line_heights_follow_the_tallest_font() {
    let layout = shaped("a \u{3042}").layout(None, Align::Start);
    let fonts: Vec<usize> = layout.glyphs().iter().map(|g| g.font).collect();
    assert_eq!(fonts, [0, 0, 1]);
    let line = &layout.lines()[0];
    // The kana font's line gap is split above and below.
    assert_eq!((line.baseline, line.height), (14.84375, 20.3125));
    for (height, baseline, total) in [
        (LineHeight::Relative(1.5), 16.40625, 23.4375),
        (LineHeight::Absolute(30.0), 19.6875, 30.0),
    ] {
        let options = TextOptions {
            line_height: height,
            ..options()
        };
        let layout = shaped_with("a \u{3042}", &options).layout(None, Align::Start);
        let line = &layout.lines()[0];
        assert_eq!((line.baseline, line.height), (baseline, total));
    }
    // A line in the first font alone is the first font's height.
    let layout = shaped("a").layout(None, Align::Start);
    assert_eq!(layout.lines()[0].height, 15.625);
}

#[test]
fn lines_align_across_the_width_or_the_widest_line() {
    let x = |align: Align, width: Option<f32>| {
        let layout = shaped("aa\nb").layout(width, align);
        layout.lines().iter().map(|l| l.x).collect::<Vec<f32>>()
    };
    assert_eq!(x(Align::Start, None), [0.0, 0.0]);
    assert_eq!(x(Align::Center, None), [0.0, px(250)]);
    assert_eq!(x(Align::Right, None), [0.0, px(500)]);
    assert_eq!(x(Align::End, None), [0.0, px(500)]);
    assert_eq!(x(Align::Center, Some(px(2000))), [px(500), px(750)]);
    let rtl = |align: Align| {
        let layout = shaped("\u{5D0}").layout(Some(px(2000)), align);
        layout.lines()[0].x
    };
    assert_eq!(rtl(Align::Start), px(1400));
    assert_eq!(rtl(Align::End), 0.0);
    assert_eq!(rtl(Align::Left), 0.0);
}

#[test]
fn empty_text_has_one_empty_line() {
    let layout = shaped("").layout(Some(px(1000)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 0)]);
    assert_eq!(layout.lines()[0].height, 15.625);
    let caret = layout.caret(Position::new(0));
    assert_eq!((caret.x, caret.line, caret.height), (0.0, 0, 15.625));
    assert_eq!(layout.move_right(Position::new(0)), Position::new(0));
    assert_eq!(layout.hit(5.0, 5.0), Position::new(0));
    let rtl = TextOptions {
        direction: Some(Direction::RightToLeft),
        ..options()
    };
    let layout = shaped_with("", &rtl).layout(Some(px(1000)), Align::Start);
    assert_eq!(layout.caret(Position::new(0)).x, px(1000));
    // Without fonts there is nothing to draw, but lines and carets still work.
    let layout = ShapedText::new("ab\nc", &[], &options()).layout(None, Align::Start);
    assert_eq!(ranges(&layout), [(0, 3), (3, 4)]);
    assert!(layout.glyphs().is_empty());
    assert_eq!(layout.caret(Position::new(4)).line, 1);
}

#[test]
fn right_to_left_runs_mirror_and_find_fonts_by_mirror_image() {
    let rtl = TextOptions {
        direction: Some(Direction::RightToLeft),
        ..options()
    };
    let layout = shaped_with("(\u{5D0})", &rtl).layout(None, Align::Start);
    assert_eq!(ids(layout.glyphs()), [OPEN, ALEF, CLOSE]);
    assert_eq!(clusters(layout.glyphs()), [3, 1, 0]);

    // A font with only the closing parenthesis draws an opening one right to left.
    let cmap = cmap_table(&[(3, 10, format12(&[(0x29, 0x29, 1)]))]);
    let data = tt_font(
        &vec![Vec::new(); 2],
        &[(500, 0), (300, 0)],
        &[(b"cmap", cmap)],
    );
    let closing = Font::from_data(data, 0).expect("the font parses");
    let fonts = [&kana(), &closing];
    assert_eq!(
        crate::shaping::itemize_with("(", &fonts, Direction::RightToLeft)[0].font,
        1
    );
    assert_eq!(crate::shaping::itemize("(", &fonts)[0].font, 0);
    let layout = ShapedText::new("\u{5D0}(", &fonts, &options()).layout(None, Align::Start);
    assert_eq!(
        layout
            .glyphs()
            .iter()
            .find(|g| g.cluster == 2)
            .map(|g| g.font),
        Some(1)
    );
}

/// A small generator, so the random tests need no dependency and repeat exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_layouts_keep_their_invariants() {
    const PIECES: [&str; 17] = [
        "a", "b", " ", " ", "\u{5D0}", "\u{5D1}", "1", "(", ")", "\t", "\n", "\u{3042}",
        "e\u{301}", "\u{200F}", "\u{200B}", "\u{202C}", "\u{1C}",
    ];
    let (latin, kana) = (latin(), kana());
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for case in 0..400 {
        let text: String = (0..rng.below(40))
            .map(|_| PIECES[rng.below(PIECES.len())])
            .collect();
        let direction = [
            None,
            Some(Direction::LeftToRight),
            Some(Direction::RightToLeft),
        ][case % 3];
        let options = TextOptions {
            direction,
            ..options()
        };
        let shaped = ShapedText::new(&text, &[&latin, &kana], &options);
        let max = (case % 5 != 0).then(|| px(rng.below(5000) as i32));
        let layout = shaped.layout(max, [Align::Start, Align::Center, Align::Right][case % 3]);
        let lines = layout.lines();
        let context = format!("{text:?} at {max:?}");
        assert_eq!(lines[0].range.start, 0, "{context}");
        assert_eq!(
            lines.last().map(|l| l.range.end),
            Some(text.len()),
            "{context}"
        );
        for pair in lines.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start, "{context}");
            assert_eq!(pair[0].top + pair[0].height, pair[1].top, "{context}");
        }
        for (n, line) in lines.iter().enumerate() {
            let graphemes = crate::graphemes(&text[line.range.clone()])
                .filter(|g| draws(g))
                .count();
            if let Some(max) = max {
                assert!(
                    line.width <= max + EPSILON || graphemes <= 1,
                    "{context}: line {n}"
                );
            }
            assert!(in_box(&layout, &text, n), "{context}: line {n}");
            let glyphs = line_glyphs(&layout, n);
            for pair in glyphs.windows(2) {
                assert!(pair[0].x <= pair[1].x, "{context}: line {n}");
            }
            assert!(
                glyphs.iter().all(|g| line.range.contains(&g.cluster)),
                "{context}"
            );
        }
        // Every caret position is hit again at its caret.
        let boundaries: Vec<usize> = (0..=text.len())
            .filter(|&i| crate::is_grapheme_boundary(&text, i))
            .collect();
        for &i in &boundaries {
            for position in [Position::new(i), Position::upstream(i)] {
                let caret = layout.caret(position);
                let hit = layout.hit(caret.x, caret.top + caret.height / 2.0);
                let again = layout.caret(hit);
                assert_eq!(
                    (again.line, again.x),
                    (caret.line, caret.x),
                    "{context}: {position:?}"
                );
            }
        }
        // With one direction for every paragraph, the arrow keys pass every edge and reach an
        // end. Between paragraphs of opposite directions they can go back and forth.
        match direction {
            Some(direction) => walk(&layout, direction == Direction::LeftToRight, &context),
            None => {
                let mut position = Position::new(0);
                for _ in 0..2 * boundaries.len() {
                    position = layout.move_right(layout.move_left(layout.move_right(position)));
                }
            }
        }
        let _ = layout.selection(0..text.len());
    }
}

#[test]
fn long_text_lays_out_in_linear_time() {
    let unit = "aaaa bbbb \u{5D0}\u{5D1}\u{5D2} 123 (x)\t";
    let text = unit.repeat(4000);
    let start = Instant::now();
    let shaped = shaped(&text);
    for width in [None, Some(px(20000)), Some(px(3000))] {
        let layout = shaped.layout(width, Align::Start);
        assert!(!layout.lines().is_empty());
    }
    assert!(start.elapsed() < Duration::from_secs(30));
}

/// Lays out mixed text with every installed face, alone, at several widths: lines cover the
/// text in order, fit unless they hold one grapheme cluster that does not, keep their
/// glyphs' clusters, and every caret position is hit again at its caret.
#[test]
fn installed_fonts_lay_out() {
    const TEXT: &str = "Duydaş'a hoş geldin, İıĞğ! The quick brown fox, AVATAR office.\n\
        日本語のテキスト「東京へ行った。」かな、カナ。\r\n\
        שָׁלוֹם (עוֹלָם) [א\u{5B0}\u{5B7}\u{5BD}] 12 1\u{2044}2 and English\u{2029}\
        \u{301}\u{301} e\u{301}\tx\u{200D}y 👨\u{200D}👩\u{200D}👧 🇹🇷 葛\u{E0100} 한국어 각 \u{3000}end ";
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect(&dir, 0, &mut files);
    }
    files.sort();
    let boundaries: Vec<usize> = (0..=TEXT.len())
        .filter(|&i| crate::is_grapheme_boundary(TEXT, i))
        .collect();
    let options = TextOptions {
        size: 16.0,
        ..TextOptions::default()
    };
    let started = Instant::now();
    let (mut faces, mut failures) = (0, Vec::new());
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let data: std::sync::Arc<[u8]> = bytes.into();
        let Some(count) = Font::face_count(&data) else {
            continue;
        };
        for index in 0..count.min(64) {
            let Ok(font) = Font::from_data(data.clone(), index) else {
                continue;
            };
            faces += 1;
            let shaped = ShapedText::new(TEXT, &[&font], &options);
            for max in [None, Some(0.0), Some(37.0), Some(120.0), Some(400.0)] {
                let layout = shaped.layout(max, Align::Start);
                let name = format!("{}:{index} at {max:?}", path.display());
                let lines = layout.lines();
                let mut end = 0;
                for (n, line) in lines.iter().enumerate() {
                    let text_graphemes = crate::graphemes(&TEXT[line.range.clone()])
                        .filter(|g| draws(g))
                        .count();
                    let fits = max.is_none_or(|m| line.width <= m + EPSILON);
                    let clusters = line_glyphs(&layout, n)
                        .iter()
                        .all(|g| line.range.contains(&g.cluster));
                    let stacked = n == 0 || lines[n - 1].top + lines[n - 1].height == line.top;
                    if line.range.start != end
                        || !stacked
                        || !(fits || text_graphemes <= 1)
                        || !clusters
                        || !in_box(&layout, TEXT, n)
                    {
                        failures.push(format!("{name}: line {n} {:?}", line.range));
                    }
                    end = line.range.end;
                }
                if end != TEXT.len() {
                    failures.push(format!("{name}: lines end at {end}"));
                }
                if max != Some(120.0) {
                    continue;
                }
                for &i in &boundaries {
                    for position in [Position::new(i), Position::upstream(i)] {
                        let caret = layout.caret(position);
                        let again =
                            layout.caret(layout.hit(caret.x, caret.top + caret.height / 2.0));
                        if (again.line, again.x) != (caret.line, caret.x) {
                            failures.push(format!("{name}: caret {position:?}"));
                        }
                    }
                }
            }
        }
    }
    eprintln!("{faces} faces laid out in {:?}", started.elapsed());
    assert!(
        failures.is_empty(),
        "{} failures: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}

#[test]
fn arrow_keys_come_into_a_line_at_its_near_end() {
    // The Hebrew word wraps; moving right, the caret comes into it at its left end, after
    // its last letter, and goes through both letters.
    let layout = shaped("aaa \u{5D0}\u{5D1}").layout(Some(px(1500)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 4), (4, 8)]);
    let mut position = Position::upstream(4);
    let mut seen = Vec::new();
    loop {
        let next = layout.move_right(position);
        if next == position {
            break;
        }
        position = next;
        seen.push(position.index);
    }
    assert_eq!(seen, [8, 6, 4]);
    assert_eq!(layout.move_left(Position::new(8)), Position::upstream(4));

    // The same across a line break.
    let options = TextOptions {
        direction: Some(Direction::LeftToRight),
        ..options()
    };
    let layout = shaped_with("a\n\u{5D0}\u{5D1}", &options).layout(None, Align::Start);
    assert_eq!(layout.move_right(Position::new(1)), Position::new(6));
    assert_eq!(layout.move_left(Position::new(6)), Position::new(1));
}

#[test]
fn a_last_line_that_comes_out_wider_backs_off_to_an_opportunity() {
    // "xfi" breaks inside the word, so the second line starts with an i that was half of
    // the fi ligature: 350 units measured, 500 shaped alone. "i a" then comes out wider
    // than the width, and moves its break back to the opportunity after the space.
    let layout = shaped("xfi a").layout(Some(px(1150)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 2), (2, 4), (4, 5)]);
}

#[test]
fn lines_that_keep_coming_out_too_wide_are_bisected() {
    // Sixty a's make one 6,000-unit ligature, so each a measures 100 units; any shorter
    // line has none and takes 500 units an a. Backing off 16 times is not enough.
    let gsub = layout_table(
        *b"latn",
        &[],
        &[(*b"liga", &[0])],
        vec![lookup(4, 0, vec![ligatures(&[(&[1; 60], 3)])])],
    );
    let font = ab(&[(b"GSUB", gsub)]);
    let text = "a".repeat(60);
    let shaped = ShapedText::new(&text, &[&font], &options());
    assert_eq!(ids(shaped.layout(None, Align::Start).glyphs()), [3]);
    let layout = shaped.layout(Some(px(4000)), Align::Start);
    let expected: Vec<(usize, usize)> = (0..60).step_by(8).map(|s| (s, (s + 8).min(60))).collect();
    assert_eq!(ranges(&layout), expected);
    assert!(layout.lines().iter().all(|l| l.width <= px(4000)));
}

#[test]
fn graphemes_that_draw_nothing_take_no_room() {
    // With no space glyph, U+200B is dropped and its cluster merges with the a's, whose
    // width it must not share.
    let font = ab(&[]);
    let layout =
        ShapedText::new("\u{200B}a\u{200B}a", &[&font], &options()).layout(None, Align::Start);
    assert_eq!(clusters(layout.glyphs()), [0, 7]);
    let x = |i| layout.caret(Position::new(i)).x;
    assert_eq!(
        [x(0), x(3), x(4), x(7), x(8)],
        [0.0, 0.0, px(500), px(500), px(1000)]
    );
    assert!(layout.selection(0..3).is_empty());
    assert_eq!(layout.selection(3..4)[0].width, px(500));
    assert_eq!(layout.move_right(Position::new(0)), Position::new(4));
    assert_eq!(layout.move_right(Position::new(4)), Position::new(8));

    // Nor do they stop whitespace from hanging, or make a line overflow.
    let layout = shaped("a \u{202C}").layout(None, Align::Start);
    assert_eq!(layout.lines()[0].width, px(500));
    let layout = shaped("aaa \u{202C}").layout(Some(px(1500)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 7)]);
}

#[test]
fn a_cluster_that_moves_the_pen_back_takes_no_room() {
    // Kerning takes 750 units off a's 500 before b.
    let gpos = layout_table(
        *b"latn",
        &[],
        &[(*b"kern", &[0])],
        vec![lookup(2, 0, vec![pair(&[(1, 2, -750)])])],
    );
    let font = ab(&[(b"GPOS", gpos)]);
    let layout = ShapedText::new("ab", &[&font], &options()).layout(None, Align::Start);
    let glyphs = layout.glyphs();
    assert_eq!((glyphs[0].x, glyphs[1].x), (0.0, px(-250)));
    let x = |i| layout.caret(Position::new(i)).x;
    assert_eq!([x(0), x(1), x(2)], [0.0, 0.0, px(250)]);
    assert!(layout.selection(0..1).is_empty());
    let all = layout.selection(0..2);
    assert_eq!((all.len(), all[0].x, all[0].width), (1, 0.0, px(250)));
    for i in 0..=2 {
        let caret = layout.caret(Position::new(i));
        assert_eq!(layout.caret(layout.hit(caret.x, 1.0)).x, caret.x);
    }
}

#[test]
fn walking_a_long_line_with_the_arrow_keys_takes_linear_time() {
    let text = "a".repeat(50_000);
    let layout = shaped(&text).layout(None, Align::Start);
    let start = Instant::now();
    let mut position = Position::new(0);
    for _ in 0..text.len() {
        position = layout.move_right(position);
    }
    assert_eq!(position, Position::new(text.len()));
    for _ in 0..text.len() {
        position = layout.move_left(position);
    }
    assert_eq!(position, Position::new(0));
    assert!(start.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_line_holding_two_paragraphs_takes_the_last_ones_direction() {
    // U+001C ends a paragraph without breaking the line. The line takes the direction of
    // its last paragraph, so the space that hangs at that paragraph's end is at the line's
    // end, outside the content box, and the line still fits where its content does.
    let text = "\u{5D0}\u{1C}a ";
    let layout = shaped(text).layout(Some(px(1600)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 5)]);
    let line = &layout.lines()[0];
    assert_eq!(
        (line.direction, line.x, line.width),
        (Direction::LeftToRight, 0.0, px(1600))
    );
    assert!(in_box(&layout, text, 0));
    assert!(layout.cells[0].right <= layout.cells[2].left);

    let text = "a\u{1C}\u{5D0} ";
    let layout = shaped(text).layout(Some(px(1600)), Align::Start);
    assert_eq!(ranges(&layout), [(0, 5)]);
    let line = &layout.lines()[0];
    assert_eq!(
        (line.direction, line.x + line.width),
        (Direction::RightToLeft, px(1600))
    );
    assert!(in_box(&layout, text, 0));
    // The space hangs on the left, and the first paragraph is on the right.
    assert!(layout.cells[3].right <= layout.cells[2].left);
    assert!(layout.cells[2].right <= layout.cells[0].left);
}

#[test]
fn tiny_sizes_keep_distinct_caret_places() {
    let options = TextOptions {
        size: 1.0 / 1024.0,
        ..options()
    };
    let text = "a\u{5D0}b";
    let layout = shaped_with(text, &options).layout(None, Align::Start);
    for i in [0, 1, 3, 4] {
        for position in [Position::new(i), Position::upstream(i)] {
            let caret = layout.caret(position);
            let again = layout.caret(layout.hit_line(0, caret.x));
            assert_eq!(again.x, caret.x, "{position:?}");
        }
    }
    assert_ne!(
        layout.caret(Position::upstream(1)).x,
        layout.caret(Position::new(1)).x
    );
}

#[test]
fn extreme_numbers_give_finite_geometry() {
    let options = TextOptions {
        size: f32::MAX,
        ..options()
    };
    let layout = shaped_with("\t a\u{5D0}", &options).layout(Some(1000.0), Align::Center);
    for line in layout.lines() {
        assert!(line.x.is_finite() && line.width.is_finite() && line.height.is_finite());
    }
    assert!(
        layout
            .glyphs()
            .iter()
            .all(|g| g.x.is_finite() && g.y.is_finite())
    );

    let layout = shaped("ab").layout(None, Align::Start);
    assert_eq!(layout.hit_line(0, f32::INFINITY), Position::new(2));
    assert_eq!(layout.hit_line(0, f32::NEG_INFINITY), Position::new(0));
    assert_eq!(layout.hit_line(0, f32::NAN), Position::new(0));
}

#[test]
fn broken_metrics_never_stack_lines_upwards() {
    // An ascender below the baseline and a descender above it.
    let font = ab(&[(b"hhea", hhea(-800, 200, 0, 4))]);
    let layout = ShapedText::new("a\nb", &[&font], &options()).layout(None, Align::Start);
    let lines = layout.lines();
    assert!(lines.iter().all(|l| l.height >= 0.0));
    assert!(lines.windows(2).all(|p| p[0].top <= p[1].top));
    assert_eq!(layout.hit(0.0, 100.0), layout.line_start(1));
}

#[test]
fn find_break_keeps_opportunities_at_the_end() {
    let shaped = shaped("aa bb cc");
    let n = shaped.graphemes.len();
    assert_eq!(shaped.find_break(0, n, Some(1000.0)), (n, vec![3, 6]));
}

#[test]
fn a_tab_after_a_cluster_that_moves_the_pen_back_reaches_its_stop() {
    // Kerning takes 1,250 units off b's 500 before a: "aba" ends 250 units in but its
    // cells 500 in. With no space glyph, a space is a quarter em, so the stop is at 2,000.
    let gpos = layout_table(
        *b"latn",
        &[],
        &[(*b"kern", &[0])],
        vec![lookup(2, 0, vec![pair(&[(2, 1, -1250)])])],
    );
    let font = ab(&[(b"GPOS", gpos)]);
    let layout = ShapedText::new("aba\tb", &[&font], &options()).layout(None, Align::Start);
    assert_eq!(layout.glyphs().last().map(|g| g.x), Some(px(2000)));
    // What follows such a cluster starts after the cells before it, as measured.
    let shaped = ShapedText::new("aba\u{3042}", &[&font, &kana()], &options());
    assert_eq!(shaped.layout(None, Align::Start).lines()[0].width, px(1500));
    assert_eq!(
        ranges(&shaped.layout(Some(px(1500)), Align::Start)),
        [(0, 6)]
    );
}

#[test]
fn line_heights_stay_finite_and_never_negative() {
    for (size, line_height) in [
        (0.01, LineHeight::Absolute(0.0)),
        (SIZE, LineHeight::Absolute(f32::MAX)),
        (SIZE, LineHeight::Relative(f32::MAX)),
    ] {
        let options = TextOptions {
            size,
            line_height,
            ..options()
        };
        let layout = shaped_with("a\nb\nc", &options).layout(None, Align::Start);
        for line in layout.lines() {
            assert!(line.height >= 0.0 && line.height <= MAX_LINE_HEIGHT);
            assert!(line.top.is_finite() && line.baseline.is_finite());
        }
        assert!(layout.height().is_finite());
    }
}

#[test]
fn a_line_break_stays_with_its_line() {
    // At width 0 the a is too wide whatever happens; moving the break before the newline
    // would not narrow the line.
    let layout = shaped("a\n").layout(Some(0.0), Align::Start);
    assert_eq!(ranges(&layout), [(0, 2), (2, 2)]);
}

#[test]
fn arrow_keys_cross_a_line_with_nothing_that_takes_room() {
    // b takes no room, so at width 0 it is a line of its own with no cell to step over.
    let font = ab_with([500, 500, 0, 0], &[]);
    let layout = ShapedText::new("ab", &[&font], &options()).layout(Some(0.0), Align::Start);
    assert_eq!(ranges(&layout), [(0, 1), (1, 2)]);
    assert_eq!(layout.move_right(Position::upstream(1)), Position::new(2));
    assert_eq!(layout.move_left(Position::new(2)), Position::upstream(1));
}

#[test]
fn subnormal_sizes_keep_cells_in_order() {
    // An eight-a ligature 5 units wide, at a size where a unit is the smallest f32: its
    // shares round up past its end unless clamped.
    let gsub = layout_table(
        *b"latn",
        &[],
        &[(*b"liga", &[0])],
        vec![lookup(4, 0, vec![ligatures(&[(&[1; 8], 3)])])],
    );
    let font = ab_with([500, 500, 500, 5], &[(b"GSUB", gsub)]);
    let options = TextOptions {
        size: f32::from_bits(1000),
        ..options()
    };
    let text = "aaaaaaaab";
    let layout = ShapedText::new(text, &[&font], &options).layout(None, Align::Start);
    assert!(layout.cells.windows(2).all(|p| p[0].right <= p[1].left));
    for i in 0..=text.len() {
        let caret = layout.caret(Position::new(i));
        assert_eq!(layout.caret(layout.hit_line(0, caret.x)).x, caret.x, "{i}");
    }
    walk(&layout, true, "subnormal");
}

#[test]
fn far_alignment_keeps_the_arrow_keys_moving() {
    // At x = 2^28 a step is 32 px, so a shift rounds the 7.8 px cells together; at 2^24
    // the midpoint of a 1 px cell rounds onto its right edge.
    for (size, width, text) in [(SIZE, 268_435_456.0, "aaa"), (2.0, 16_777_216.0, "a")] {
        let options = TextOptions { size, ..options() };
        let layout = shaped_with(text, &options).layout(Some(width), Align::Right);
        for right in [true, false] {
            walk(&layout, right, &format!("{text} at {width}"));
        }
    }
}
