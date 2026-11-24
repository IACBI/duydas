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
  - A line that spans paragraphs is reordered one paragraph at a time. After L1 the characters X9 sets aside follow the character before them again, and at the start of a line they take the paragraph level, as at the start of a paragraph.
  - Mirroring (L4) is left to shaping, through `mirror` (Bidi_Mirroring_Glyph). L3 does not apply: shaping places each mark on its base, in either direction.
  - Levels are per byte, like every offset in the crate. The work is linear in the text: the bracket stack holds 63 entries as BD16 says, bracket pairs are kept in the order of their opening brackets, embedding depth stops at 125, and a call to a line function costs the line's length plus a binary search over the paragraphs.
  - Bidi_Class, Bidi_Paired_Bracket and Bidi_Mirroring_Glyph are generated from the UCD by `cargo xtask unicode-gen` (`bidi_tables.rs`).
- **Right-to-left shaping, identical to HarfBuzz 10.4.0** for the scripts ADR 0027 covers and for Hebrew. `ShapeOptions::direction` takes the run's direction; the glyphs always come out in visual order, left to right, with clusters that do not increase in a right-to-left run. As HarfBuzz does:
  - HarfBuzz merges the clusters of its grapheme clusters (its continuation rule: marks, ZWJ and an emoji after it, emoji modifiers, the second regional indicator), in either direction. Here a cluster is the union of the graphemes and HarfBuzz's grapheme clusters that overlap, so neither is split between two clusters. "Emoji" is Extended_Pictographic in the Unicode 16.0 emoji data HarfBuzz 10.4.0 was built with, which `unicode-gen` keeps as a shaping flag of its own: Unicode 18 removed many characters (★, for one) that HarfBuzz still counts, while graphemes use Unicode 18;
  - a run is shaped in its script's own direction: text written the other way is first reversed by HarfBuzz's grapheme clusters, and the glyphs are reversed into visual order at the end only when the script's direction is right to left. Digits or regional indicators without letters in a right-to-left script are not reversed;
  - right to left, characters with a mirror image are replaced by it, and `rtlm` (its bit for value 1) applies to those without one in the font; `rtla` and `rtlm` take the place of `ltra` and `ltrm`;
  - automatic fractions in backward text swap `numr` and `dnom`;
  - cursive attachment follows the lookup's right-to-left flag, mark attachments add the advances on the other side, and legacy `kern` tables kern in visual order;
  - without GPOS, fallback mark positioning places marks from the other side, with ligature components in reverse order and HarfBuzz's signed arithmetic for a component that wrapped to 0;
  - combining classes are HarfBuzz's modified classes (Hebrew, Arabic, Syriac, Thai, Lao, Tibetan), which decide mark order and fallback placement;
  - Hebrew gets HarfBuzz's Hebrew shaper: points compose into presentation forms when the selected plan has no `mark` feature (in GSUB or GPOS), a meteg (or another mark below) after patah or qamats and sheva or hiriq moves before the sheva or hiriq, merging their clusters, and GPOS applies only when the font has a `hebr` script;
  - cluster merges follow HarfBuzz's buffer edits, so they match in either direction: mark reordering, Hebrew's swap, and in Hangul a composed syllable, the jamo of one, and a tone mark moved in front of its syllable. The dotted circle a lone tone mark gets keeps the mark's properties, as in HarfBuzz, so marks before it sort after both.

  As in HarfBuzz, a character and the marks right after it are decomposed together. A character on its own, such as a letter before a joiner, keeps the glyph the font has. Font fallback counts a Hebrew letter and point as covered when the font has their presentation form and its plan has no `mark` feature (in GSUB or GPOS), because shaping then composes them. `itemize` knows neither the language nor the features, so it judges with the defaults; a font with `mark` in only some languages can be picked for a language that does not compose.
- **Not supported yet:**
  - Arabic, Syriac and the other joining scripts, which need HarfBuzz's Arabic shaper (joining forms, `stch`, mark reordering): they shape right to left with the default features, without joining, and are not compared;
  - the shapers ADR 0027 lists, and vertical text;
  - font fallback by direction: `itemize` does not know a run's direction, so it does not count a character as covered by its mirror image. Paragraph layout (ADR 0029) itemizes each bidi run and will pass the direction.
- **Documented differences:**
  - clusters are UAX #29 grapheme clusters (ADR 0027), merged with HarfBuzz's grapheme clusters where those span two (a mark after a control character, an emoji after a ZWJ). HarfBuzz's are finer at ZWNJ and between jamo; where one of its grapheme clusters overlaps two graphemes, all of them share one cluster here, while HarfBuzz may give a later character of the second grapheme a cluster of its own. In right-to-left text with a font that has no space glyph, the default ignorables the font drops merge into a neighbouring cluster, and the neighbour differs. These are all 324 cluster differences below, in 12 icon fonts with no U+0020;
  - with COLR v1 color glyphs, HarfBuzz takes glyph extents from the paint tree, and its glyph 0 record gives empty extents. Fallback marks in Segoe UI Emoji are therefore placed from different boxes (the 36 offset differences below).

## Verification

- **Conformance.** All cases of Unicode 18.0's BidiTest.txt and BidiCharacterTest.txt pass. The repository keeps a sample of each, run on every pull request and push to `main`; the `Unicode` workflow (`.github/workflows/unicode.yml`) regenerates the tables with `unicode-gen --check` and runs the full files weekly and on pull requests that touch them.
- **HarfBuzz 10.4.0 on installed fonts.** The dump test now shapes each text in 12 settings, three of them right to left (Hebrew, fractions, no kerning), and adds Hebrew, Arabic and mixed texts: **403,788 runs** (302,841 left to right, 100,947 right to left) over 437 faces. 1,080 differ:
  - 720 in Arabic, which has no Arabic shaper yet;
  - 324 clusters, and 36 Hebrew mark offsets in Segoe UI Emoji, the two documented differences above.
  Everything else matches: glyphs, advances and offsets, and clusters as ADR 0027 compares them. Both sides' clusters are floored to the grapheme starts before them, leaving out a grapheme start that is a mark after a control or format character; this hides where HarfBuzz is finer than a grapheme, while a merge across graphemes must match exactly. Compared raw, clusters differ wherever HarfBuzz's are finer.
- **Synthetic fonts.** All of these match HarfBuzz in glyphs, advances and offsets:
  - 152 right-to-left cases built with fontTools: mirroring with and without `rtlm`, cursive chains in both directions, marks on ligatures, cross-stream `kern`, fallback marks, Hebrew compositions, numeric runs;
  - 84 cases from the review: `rtlm` values, ligatures of 18 components with marks, letters before joiners, emoji after ZWJ, marks after controls;
  - 55 on the unit tests' fixture fonts;
  - 815 more on installed fonts (Malgun Gothic, Arial, Times New Roman, Segoe UI, Segoe UI Symbol) and the Hebrew fixture font, 807 of them generated: Hangul jamo, syllables and tone marks with marks and joiners; Hebrew letters and points with controls and joiners; and ZWJ before each of 142 characters that are pictographic in Unicode 16's emoji data and not in 18's, in both directions.

  Clusters match too, except where HarfBuzz's are finer than a grapheme (140 of the generated Hangul cases, between jamo; never in the other order). The left-to-right cases of ADR 0027 still match.
- **Unit tests.**
  - Five shaping tests whose expected output comes from HarfBuzz: GPOS with cursive, marks, mirroring, `rtla`/`ltra` and fractions; an old Hebrew font with presentation forms, meteg, fallback marks and a cross-stream `kern`; dropped ignorables; run direction; the review's corners.
  - Tests of font fallback for Hebrew, of the budget at the boundary of a `kern` pass (with and without a cross-stream subtable) and of a cursive attachment that reverses a chain, comparing whole positions and attachments, and of the cursive chain's measure.
  - 12 tests of the bidi API: paragraphs, per-byte levels, L1 at line ends, separators and line starts, visual runs per paragraph, isolates and embeddings, mirroring, the depth limit, long input (many lines, brackets and nestings) within a time limit, and random texts in each paragraph direction, whose runs are checked against runs built from `line_levels` and whose order against L2 applied to the levels directly.
- **Performance.** Left-to-right shaping is unchanged: `shape_bench` gives 192–251 ns per character on the ADR 0027 messages after the review's fixes (187–245 before), within its ranges.

## Review

Codex (`gpt-6.2-sol`, read-only, high effort) reviewed the pull request. It found nothing critical or high, 4 medium and 11 low findings, and nothing wrong in rules P2 to I2, UTF-8 clamping or CRLF paragraphs. Each finding was checked against the code and HarfBuzz 10.4.0's source before being fixed, and the new expectations come from HarfBuzz.

- **Fixed, medium:**
  - `visual_runs` reordered a line across paragraph boundaries;
  - the line functions visited every paragraph per line;
  - right-to-left clusters could increase where HarfBuzz's grapheme clusters overlap two of Unicode's (jamo, ZWJ and an emoji); the fix merges them as HarfBuzz does, which also removed 144 cluster differences on installed fonts;
  - font fallback did not count Hebrew presentation forms.
- **Fixed, low:**
  - X9 characters after a separator kept their level after L1;
  - bracket pairs were sorted;
  - automatic `rtlm` set every bit of the feature;
  - normalization grouped by grapheme instead of base and marks;
  - the ligature component that wraps to 0 in fallback positioning;
  - two bulk passes and cursive chains could overshoot the budget;
  - a run's first reservation was sized by bytes;
  - wording in docs, the sample headers and the workflow's paths;
  - a fraction test that could not tell the forms apart.
- **Deferred:** fallback by direction for mirrored characters, to paragraph layout (ADR 0029), where runs have a direction.

A second pass over the fixes found 3 medium and 4 low issues, all fixed:

- clusters taken from HarfBuzz's grapheme clusters could split a grapheme (`a`, ZWJ, an emoji, ZWNJ), so clusters are now the union of both;
- the cursive check did not charge a chain it rejected, and charged two walks of three;
- a cross-stream `kern` pass did not charge hanging every glyph, and fallback positioning scanned each cluster twice;
- X9 characters at the start of a wrapped line kept the level of the line before;
- bracket pairs took memory for the whole sequence, and now take it for the opening brackets;
- the docs overstated the line functions' cost and the comparison, and the sample header counted semicolons in comments;
- tests: Hebrew for the wrapped ligature component (Latin right to left is reversed first), runs checked against an oracle of their own, and the budget checked at the `kern` and cursive call sites.

A third pass found 3 medium and 4 low issues, all fixed, with expectations from HarfBuzz:

- HarfBuzz's continuation rule used Unicode 18's Extended_Pictographic; HarfBuzz 10.4.0 uses Unicode 16's, which still includes characters such as ★ (`a`, ZWJ, ★ right to left came out with ★ first);
- Hangul composition and tone-mark reordering did not merge clusters, so right-to-left clusters could increase;
- Hebrew's meteg swap did not merge clusters, so a meteg reversed from the start of left-to-right text decreased them;
- the cursive check could look at one link with no budget left;
- the `kern` test compared only offsets and advances, the cursive test did not reach the call site, and the random bidi test ordered runs with the function it tested;
- the ADR described the comparison's projection, the final reversal and the Hebrew gate loosely.

Checking the fixes on generated Hangul text found one more difference, fixed: the dotted circle a lone tone mark gets took U+25CC's properties instead of the mark's, so an acute before the mark sorted between them.

## Consequences

- **Next in step 4:** paragraph layout (ADR 0029): line breaking over shaped runs, bidi reordering per line, line metrics, hit testing, caret and selection. Then the glyph cache.
- **Arabic** needs HarfBuzz's Arabic shaper before Arabic messages look right; the bidi and right-to-left parts it needs are in place.
- **Keeping up with HarfBuzz and Unicode** works as in ADR 0027: the dump test and the fixture writer give the data to compare, and `unicode-gen` regenerates the bidi tables with the others. Moving to a new HarfBuzz also means setting `HARFBUZZ_EMOJI_VERSION` in `xtask/src/unicode.rs` to the emoji data named in its `hb-unicode-emoji-table.hh`; upgrading Unicode (ADR 0025) leaves it alone.
