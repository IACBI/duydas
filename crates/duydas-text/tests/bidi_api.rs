//! The bidirectional algorithm's public API on hand-written cases: paragraphs, byte levels,
//! line levels (rule L1), visual runs and order (L2), mirroring, depth limits, long input and
//! invariants over random text. The conformance tests are in `bidi.rs`.

use duydas_text::bidi::{Bidi, Direction, MAX_DEPTH, VisualRun, mirror, visual_order};

fn runs(bidi: &Bidi<'_>, line: std::ops::Range<usize>) -> Vec<(std::ops::Range<usize>, u8)> {
    bidi.visual_runs(line)
        .into_iter()
        .map(|VisualRun { range, level }| (range, level))
        .collect()
}

#[test]
fn paragraphs_end_after_separators() {
    let text = "abc\r\ndef\n\u{5D0}\u{5D1}\u{2029}1";
    let bidi = Bidi::new(text, None);
    let paragraphs: Vec<_> = bidi
        .paragraphs()
        .iter()
        .map(|p| (p.range.clone(), p.direction()))
        .collect();
    assert_eq!(
        paragraphs,
        [
            (0..5, Direction::LeftToRight),
            (5..9, Direction::LeftToRight),
            (9..16, Direction::RightToLeft),
            (16..17, Direction::LeftToRight),
        ]
    );
    // A separator takes its paragraph's level.
    assert_eq!(bidi.levels()[13..16], [1, 1, 1]);
    // A direction given applies to every paragraph.
    let forced = Bidi::new(text, Some(Direction::RightToLeft));
    assert!(forced.paragraphs().iter().all(|p| p.level == 1));
    assert_eq!(forced.paragraphs().len(), 4);
    // A lone carriage return ends a paragraph too, and a trailing separator leaves no empty one.
    let bidi = Bidi::new("a\rb\n", None);
    let ranges: Vec<_> = bidi.paragraphs().iter().map(|p| p.range.clone()).collect();
    assert_eq!(ranges, [0..2, 2..4]);
}

#[test]
fn empty_text_has_one_empty_paragraph() {
    let bidi = Bidi::new("", None);
    assert_eq!(bidi.paragraphs().len(), 1);
    assert_eq!(bidi.paragraphs()[0].range, 0..0);
    assert_eq!(bidi.paragraphs()[0].direction(), Direction::LeftToRight);
    assert!(bidi.levels().is_empty());
    assert!(bidi.line_levels(0..0).is_empty());
    assert!(bidi.visual_runs(0..10).is_empty());
    let bidi = Bidi::new("", Some(Direction::RightToLeft));
    assert_eq!(bidi.paragraphs()[0].level, 1);
}

#[test]
fn levels_are_per_byte() {
    // a, alef (two bytes), b, and a four-byte emoji, which is neutral between Ls.
    let bidi = Bidi::new("a\u{5D0}b\u{1F600}c", None);
    assert_eq!(bidi.levels(), [0, 1, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(runs(&bidi, 0..9), [(0..1, 0), (1..3, 1), (3..9, 0)]);
}

#[test]
fn right_to_left_lines_display_runs_reversed() {
    // Alef bet, space, abc, space, gimel dalet: the spaces sit between R and L, so they
    // take the paragraph's level, and the Latin word is raised to 2.
    let text = "\u{5D0}\u{5D1} abc \u{5D2}\u{5D3}";
    let bidi = Bidi::new(text, None);
    assert_eq!(bidi.paragraphs()[0].level, 1);
    assert_eq!(
        runs(&bidi, 0..text.len()),
        [(8..13, 1), (5..8, 2), (0..5, 1)]
    );
    // Numbers in right-to-left text keep their digits left to right.
    let text = "\u{5D0} 12 34";
    let bidi = Bidi::new(text, None);
    assert_eq!(
        runs(&bidi, 0..text.len()),
        [(6..8, 2), (5..6, 1), (3..5, 2), (0..3, 1)]
    );
}

#[test]
fn line_levels_reset_trailing_whitespace_and_separators() {
    // In a right-to-left paragraph, the space between two Latin words is raised with them,
    // until a line ends after it.
    let bidi = Bidi::new("abc def", Some(Direction::RightToLeft));
    assert_eq!(bidi.levels(), [2; 7]);
    assert_eq!(bidi.line_levels(0..4), [2, 2, 2, 1]);
    assert_eq!(bidi.line_levels(4..7), [2, 2, 2]);
    assert_eq!(runs(&bidi, 0..4), [(3..4, 1), (0..3, 2)]);
    // A tab always takes the paragraph level, and so does whitespace before it.
    let bidi = Bidi::new("a \tb", Some(Direction::RightToLeft));
    assert_eq!(bidi.line_levels(0..4), [2, 1, 1, 2]);
    // A line is clamped to the text and to character boundaries.
    let bidi = Bidi::new("\u{5D0}\u{5D1}", None);
    assert_eq!(bidi.line_levels(1..99), [1, 1, 1, 1]);
    assert_eq!(runs(&bidi, 0..3), [(0..2, 1)]);
    let (start, end) = (3, 1);
    assert!(runs(&bidi, start..end).is_empty());
}

#[test]
fn lines_across_paragraphs_use_each_paragraph_level() {
    let text = "abc \n\u{5D0} ";
    let bidi = Bidi::new(text, None);
    assert_eq!(bidi.line_levels(0..text.len()), [0, 0, 0, 0, 0, 1, 1, 1]);
    // Each paragraph is reordered on its own: two right-to-left paragraphs stay in order.
    let text = "\u{5D0}\r\n\u{5D1}";
    let bidi = Bidi::new(text, None);
    assert_eq!(runs(&bidi, 0..text.len()), [(0..4, 1), (4..6, 1)]);
}

#[test]
fn removed_characters_take_the_level_before_them() {
    // a, RLE, b, PDF, c: b is raised to 2 inside the embedding; RLE and PDF, set aside by
    // rule X9, take the level of the character before them.
    let text = "a\u{202B}b\u{202C}c";
    let bidi = Bidi::new(text, None);
    assert_eq!(bidi.levels(), [0, 0, 0, 0, 2, 2, 2, 2, 0]);
    // At the end of a line they are whitespace for rule L1.
    let bidi = Bidi::new("a\u{202B}b\u{202C}", None);
    assert_eq!(bidi.line_levels(0..8), [0, 0, 0, 0, 2, 0, 0, 0]);
    // Elsewhere they follow the character before them, also when L1 has reset it: the soft
    // hyphen stays with the tab rather than move with the Hebrew after it.
    let text = "\u{202B}\t\u{AD}\u{5D0}";
    let bidi = Bidi::new(text, Some(Direction::LeftToRight));
    assert_eq!(bidi.levels()[3..6], [1, 1, 1]);
    assert_eq!(bidi.line_levels(0..text.len()), [0, 0, 0, 0, 0, 0, 1, 1]);
    // An isolate's text is resolved on its own.
    let text = "\u{5D0} \u{2066}abc\u{2069}";
    let bidi = Bidi::new(text, None);
    assert_eq!(bidi.paragraphs()[0].level, 1);
    assert_eq!(bidi.levels()[6..9], [2, 2, 2]);
}

#[test]
fn mirrored_characters() {
    assert_eq!(mirror('('), Some(')'));
    assert_eq!(mirror(')'), Some('('));
    assert_eq!(mirror('['), Some(']'));
    assert_eq!(mirror('\u{AB}'), Some('\u{BB}'));
    assert_eq!(mirror('\u{2208}'), Some('\u{220B}'));
    assert_eq!(mirror('a'), None);
    assert_eq!(mirror('\u{5D0}'), None);
}

#[test]
fn visual_order_reverses_from_the_highest_level() {
    assert_eq!(visual_order(&[]), Vec::<usize>::new());
    assert_eq!(visual_order(&[0, 0]), [0, 1]);
    assert_eq!(visual_order(&[1, 1, 1]), [2, 1, 0]);
    assert_eq!(visual_order(&[0, 1, 1, 0]), [0, 2, 1, 3]);
    assert_eq!(visual_order(&[1, 2, 2, 1]), [3, 1, 2, 0]);
    assert_eq!(visual_order(&[2, 2, 0]), [0, 1, 2]);
}

#[test]
fn embeddings_stop_at_the_maximum_depth() {
    for opener in [
        '\u{202A}', '\u{202B}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
    ] {
        let text = opener.to_string().repeat(1000) + "a\u{5D0}1";
        let bidi = Bidi::new(&text, None);
        let highest = bidi.levels().iter().copied().max().unwrap_or(0);
        assert!(
            highest <= MAX_DEPTH + 1,
            "{opener:?} reached level {highest}"
        );
        assert!(
            highest >= MAX_DEPTH - 1,
            "{opener:?} stopped at level {highest}"
        );
    }
}

#[test]
fn long_text_takes_linear_time() {
    let started = std::time::Instant::now();
    let pieces = [
        "(a \u{5D0}[1 2]) ",
        "\u{2067}\u{627}\u{660}\u{2069} ",
        "\u{202B}x\u{202C}\t",
        "\u{5D1}\u{300}, $5 ",
    ];
    let text: String = pieces.iter().cycle().take(40_000).copied().collect();
    let bidi = Bidi::new(&text, None);
    assert_eq!(bidi.levels().len(), text.len());
    let runs = bidi.visual_runs(0..text.len());
    assert_eq!(
        runs.iter().map(|r| r.range.len()).sum::<usize>(),
        text.len()
    );
    // Brackets nested deeper than the 63 BD16 tracks, and isolates never closed.
    let deep = "(".repeat(100_000) + "\u{5D0}" + &")".repeat(100_000);
    assert_eq!(Bidi::new(&deep, None).levels().len(), deep.len());
    let open = "\u{2067}a".repeat(100_000);
    assert_eq!(Bidi::new(&open, None).levels().len(), open.len());
    // Many bracket pairs in one sequence, and many paragraphs laid out line by line.
    let pairs = "(())\u{5D0}".repeat(50_000);
    assert_eq!(Bidi::new(&pairs, None).levels().len(), pairs.len());
    let lines = "a\u{5D0}\n".repeat(100_000);
    let bidi = Bidi::new(&lines, None);
    let shown: usize = bidi
        .paragraphs()
        .iter()
        .map(|p| bidi.visual_runs(p.range.clone()).len())
        .sum();
    // a, alef, and the line feed at the paragraph level: three runs a line.
    assert_eq!(shown, 300_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

/// A small xorshift generator, so the cases are the same on every run.
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn random_text_keeps_the_invariants() {
    const POOL: [char; 28] = [
        'a', 'Z', '\u{5D0}', '\u{627}', '1', '\u{660}', '\u{6F1}', '+', '$', ',', '\u{300}',
        '\u{AD}', '\n', '\r', '\u{2029}', '\t', ' ', '!', '(', ')', '[', ']', '\u{202A}',
        '\u{202B}', '\u{202C}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2000 {
        let length = random.below(24);
        let text: String = (0..length)
            .map(|_| POOL[random.below(POOL.len())])
            .collect();
        for direction in [
            None,
            Some(Direction::LeftToRight),
            Some(Direction::RightToLeft),
        ] {
            let bidi = Bidi::new(&text, direction);
            let levels = bidi.levels();
            assert_eq!(levels.len(), text.len());
            // The paragraphs tile the text.
            let mut end = 0;
            for p in bidi.paragraphs() {
                assert_eq!(p.range.start, end, "{text:?}");
                end = p.range.end;
                assert!(p.level <= 1);
                if let Some(d) = direction {
                    assert_eq!(p.direction(), d);
                }
                // No level is below its paragraph's.
                assert!(levels[p.range.clone()].iter().all(|&l| l >= p.level));
            }
            assert_eq!(end, text.len());
            // Every byte of a character has the same level.
            for (i, c) in text.char_indices() {
                let at = &levels[i..i + c.len_utf8()];
                assert!(at.iter().all(|&l| l == at[0]), "{text:?}");
            }
            assert!(levels.iter().all(|&l| l <= MAX_DEPTH + 1));
            // Any line's runs cover it once, each run on character boundaries and one level.
            let a = random.below(text.len() + 1);
            let b = random.below(text.len() + 1);
            let (start, end) = (a.min(b), a.max(b));
            let line_levels = bidi.line_levels(start..end);
            let runs = bidi.visual_runs(start..end);
            let mut covered: Vec<_> = runs.iter().map(|r| r.range.clone()).collect();
            covered.sort_by_key(|r| r.start);
            let mut at = covered.first().map_or(0, |r| r.start);
            for r in &covered {
                assert_eq!(r.start, at, "{text:?} {start}..{end}");
                assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
                at = r.end;
            }
            assert_eq!(
                covered.iter().map(|r| r.len()).sum::<usize>(),
                line_levels.len()
            );
            // The runs of each paragraph, in visual order, are those visual_order gives, and
            // the paragraphs keep their order.
            let mut shown = Vec::new();
            for p in bidi.paragraphs() {
                let mine: Vec<_> = covered
                    .iter()
                    .filter(|r| p.range.contains(&r.start))
                    .cloned()
                    .collect();
                let logical: Vec<u8> = mine
                    .iter()
                    .map(|r| runs.iter().find(|v| v.range == *r).map_or(0, |v| v.level))
                    .collect();
                shown.extend(visual_order(&logical).into_iter().map(|i| mine[i].clone()));
            }
            assert_eq!(
                shown,
                runs.iter().map(|r| r.range.clone()).collect::<Vec<_>>()
            );
        }
    }
}
