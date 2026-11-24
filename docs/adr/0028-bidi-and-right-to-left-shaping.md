# 0028. Bidirectional text and right-to-left shaping

- Status: Accepted
- Date: 2026-11-25

## Context

ADR 0027 shapes left-to-right text only. Chat text is not always left to right, even in Duydaş's three languages:

- users paste names, addresses and quotes in Hebrew or Arabic into Turkish or English messages;
- a right-to-left word in a left-to-right sentence, or digits and brackets in right-to-left text, must be displayed in the order readers expect, with brackets mirrored;
- the caret, selection and hit testing in paragraph layout (next step) need each character's embedding level.

Two parts are needed: the Unicode Bidirectional Algorithm (UAX #9), which finds the levels and the display order of runs, and shaping in both directions. HarfBuzz stays the reference for shaping, as in ADR 0027.

## Decision

- **`duydas_text::bidi`, with no dependencies and no `unsafe`: UAX #9 revision 52 (Unicode 18.0).**
  - `Bidi::new(text, direction)` splits text into paragraphs (after each paragraph separator, CR LF counting as one), takes each paragraph's direction from its first strong character (P2, P3) or from the caller (HL1), and resolves a level for every byte (X1–I2, with bracket pairs, BD16, and isolates).
  - `Bidi::line_levels` applies rule L1 to a line: separators, and the whitespace and isolate formatting characters before them or at the end of the line, take the paragraph level. `Bidi::visual_runs` gives a line's runs of one level in display order (L2). `visual_order` reorders any items by level.
  - Characters that rule X9 sets aside (explicit embeddings, overrides, boundary neutrals) take the level of the character before them, so they stay with it (UAX #9, section 5.2).
  - Mirroring (L4) is left to shaping, through `mirror` (Bidi_Mirroring_Glyph). L3 does not apply: marks stay after their base in each shaped run.
  - Levels are per byte, like every offset in the crate. The work is linear in the text: the bracket stack holds 63 entries as BD16 says, and embedding depth stops at 125.
  - Bidi_Class, Bidi_Paired_Bracket and Bidi_Mirroring_Glyph are generated from the UCD by `cargo xtask unicode-gen` (`bidi_tables.rs`).
- **Right-to-left shaping, identical to HarfBuzz 10.4.0** for the scripts ADR 0027 covers and for Hebrew. `ShapeOptions::direction` takes the run's direction; the glyphs always come out in visual order, left to right, with clusters that do not increase in a right-to-left run. As HarfBuzz does:
  - a run is shaped in its script's own direction, so text in the other direction is reversed by grapheme (with HarfBuzz's continuation rule: marks, ZWJ, emoji modifiers and the second regional indicator stay with what comes before) and reversed back afterwards; digits or regional indicators without letters in a right-to-left script are not reversed;
  - right to left, characters with a mirror image are replaced by it, and `rtlm` applies to those without one in the font; `rtla` and `rtlm` take the place of `ltra` and `ltrm`;
  - automatic fractions in backward text swap `numr` and `dnom`;
  - cursive attachment follows the lookup's right-to-left flag, mark attachments add the advances on the other side, and legacy `kern` tables kern in visual order;
  - without GPOS, fallback mark positioning places marks from the other side, with ligature components in reverse order;
  - combining classes are HarfBuzz's modified classes (Hebrew, Arabic, Syriac, Thai, Lao, Tibetan), which decide mark order and fallback placement;
  - Hebrew gets HarfBuzz's Hebrew shaper: points compose into presentation forms when the font has no GPOS `mark` feature, a meteg (or another mark below) after patah or qamats and sheva or hiriq moves before the sheva or hiriq, and GPOS applies only when the font has a `hebr` script;
  - cluster merges follow HarfBuzz's in-place buffer edits, so they match in either direction.
- **Not supported yet:**
  - Arabic, Syriac and the other joining scripts, which need HarfBuzz's Arabic shaper (joining forms, `stch`, mark reordering): they shape right to left with the default features, without joining, and are not compared;
  - the shapers ADR 0027 lists, and vertical text.
- **Documented differences:**
  - clusters are UAX #29 grapheme clusters (ADR 0027), where HarfBuzz's are finer at ZWNJ, between jamo and after controls. In right-to-left text with a font that has no space glyph, the default ignorables it drops merge into a neighbouring cluster, and the neighbour differs (all 468 cluster differences below, in 12 icon fonts with no U+0020);
  - with COLR v1 color glyphs, HarfBuzz takes glyph extents from the paint tree, and its glyph 0 record gives empty extents. Fallback marks in Segoe UI Emoji are therefore placed from different boxes (the 36 offset differences below).

## Verification

- **Conformance.** All cases of Unicode 18.0's BidiTest.txt and BidiCharacterTest.txt pass. The repository keeps a sample of each, run on every push; the `Unicode` workflow (`.github/workflows/unicode.yml`) regenerates the tables with `unicode-gen --check` and runs the full files weekly and on pull requests that touch them.
- **HarfBuzz 10.4.0 on installed fonts.** The dump test now shapes each text in 12 settings, three of them right to left (Hebrew, fractions, no kerning), and adds Hebrew, Arabic and mixed texts: **403,788 runs** (302,841 left to right, 100,947 right to left) over 437 faces. 1,224 differ:
  - 720 in Arabic, which has no Arabic shaper yet;
  - 468 clusters, and 36 Hebrew mark offsets in Segoe UI Emoji, the two documented differences above.
  Everything else matches: glyphs, advances, offsets and clusters (compared strictly, as in ADR 0027).
- **Synthetic fonts.** 152 right-to-left cases built with fontTools (mirroring with and without `rtlm`, cursive chains in both directions, marks on ligatures, cross-stream `kern`, fallback marks, Hebrew compositions, numeric runs) and 28 on the unit tests' fixture fonts match HarfBuzz, clusters included except at ZWNJ. The left-to-right cases of ADR 0027 still match.
- **Unit tests.** Four shaping tests whose expected output comes from HarfBuzz (GPOS with cursive, marks, mirroring, `rtla`/`ltra` and fractions; an old Hebrew font with presentation forms, meteg, fallback marks and a cross-stream `kern`; dropped ignorables; run direction), and 12 tests of the bidi API: paragraphs, per-byte levels, L1 at line ends and separators, visual runs, isolates and embeddings, mirroring, the depth limit, long input in linear time, and invariants over 2,000 random texts in each paragraph direction.
- **Performance.** Left-to-right shaping is unchanged: `shape_bench` gives 187–245 ns per character on the ADR 0027 messages, within its ranges.

## Consequences

- **Next in step 4:** paragraph layout (ADR 0029): line breaking over shaped runs, bidi reordering per line, line metrics, hit testing, caret and selection. Then the glyph cache.
- **Arabic** needs HarfBuzz's Arabic shaper before Arabic messages look right; the bidi and right-to-left parts it needs are in place.
- **Keeping up with HarfBuzz and Unicode** works as in ADR 0027: the dump test and the fixture writer give the data to compare, and `unicode-gen` regenerates the bidi tables with the others.
