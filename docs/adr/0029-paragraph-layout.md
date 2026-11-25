# 0029. Paragraph layout

- Status: Accepted
- Date: 2026-11-25

## Context

ADR 0025 to 0028 give the parts of the text stack: segmentation, fonts, shaping in both directions and bidirectional levels. To show a message, the app needs lines. Text has to break at a width and each line has to be reordered for display, with its glyphs placed. Then screen points and text positions have to map both ways, for the caret, clicks and selection. The requirements:

- chat messages in Turkish, Japanese and English, with pasted right-to-left text;
- the composer, which edits text: the caret, selection, arrow keys, and spaces typed at the end of a line, which must stay visible;
- cheap relayout when the window is resized, since every visible message wraps again;
- no dependencies, no `unsafe`, and bounded work on hostile text.

## Decision

- **`duydas_text::layout`, in two steps.**
  - `ShapedText::new(text, fonts, options)` does what does not depend on the width:
    - resolves the embedding levels (ADR 0028) and finds the grapheme clusters and the line break opportunities (ADR 0025), keeping only the opportunities at grapheme boundaries;
    - splits the text into items of one level, script and font, with no tab or line break inside, and shapes them. Levels are taken after rule L1 at each mandatory break, so only a wrap can change them. Fonts are picked by `itemize_with` for the item's direction.
    - gives each grapheme the width it takes in its item's glyphs. A ligature's width is split equally between the graphemes it draws.

    It keeps a copy of the text and of the fonts, which share their data, so a widget can keep it with the message.
  - `ShapedText::layout(max_width, align)` breaks the lines, reorders and places them. It reuses the glyphs of whole items, and shapes again the pieces of the items that its breaks cut, on every call.
- **Line breaking.**
  - Lines break greedily, at the last opportunity that fits.
  - Whitespace (Bidi_Class WS) and tabs at the end of a line hang past the width, as in CSS `white-space: pre-wrap`. They are left out of the line's width, stay selectable, and the caret can sit after them. Graphemes made only of default-ignorable characters, which draw nothing (U+200B, the bidi controls), hang as well, and never make a line overflow.
  - A word wider than the line breaks between grapheme clusters, as CSS `overflow-wrap: break-word` does, so a long link wraps.
  - Mandatory breaks end lines and draw nothing: newlines, CR LF, and the other characters of Line_Break BK, CR, LF and NL. Text that ends with one has an empty last line, where the caret goes.
  - Lines are measured with the item's shaping. A line break, or a change of level under L1, can cut an item. The pieces are then shaped again from their own text, so every line draws exactly what `shape` gives for its pieces. A ligature split by a break comes apart.
  - Shaped alone, a line can come out wider than measured, for example when it loses kerning with the line before, or starts with half of a ligature. Its break then moves back, to an earlier opportunity or inside the word, up to 16 times. Moving it back only past whitespace or a line break, which take no room, is not tried. If the line is still too wide (a ligature that forms only over the whole line, say), a shorter line that fits is found by bisection, with at least one grapheme that takes room. Shaping makes the width grow unevenly with the length, so that is not always the longest line that would fit. So no line is wider than the width unless it holds a single grapheme that takes room, and a line is shaped at most 16 times plus the logarithm of its length more.
  - Tolerances are 1/64 px.
- **Reordering.**
  - Each line takes its levels from `Bidi::line_levels` (rule L1) and gives every grapheme the level of its first character, so a run never splits a grapheme.
  - Runs of one level are put in display order with `visual_order` (rule L2), one paragraph at a time.
  - Mirroring (L4) is left to shaping, since right-to-left runs are shaped right to left.
  - A line's direction is its paragraph's. The empty line after a final line break continues the last paragraph.
  - Paragraphs share a line only after U+001C to U+001E, which end a bidi paragraph without breaking the line. Such a line takes the direction of its last paragraph, and the paragraphs follow each other in it, so what hangs at the end of the last is at the end of the line.
- **Tabs** advance to the next tab stop, every `tab_size` widths of the first font's space (8 by default; 0 gives tabs no room). The stop must be at least half a space away, as in CSS.
  - Stops are measured from the line's start in logical order. After L1 a tab is at the paragraph's level, so what comes before it on the line is shown before it in the paragraph's direction.
- **Line metrics** follow CSS's inline model.
  - Each font on the line, and the first font as the strut, gets a box: its ascender and descender, plus half the leading above and half below.
  - `LineHeight::Normal` takes the font's line gap as the leading, if it is positive. `Relative` (a multiple of the size) and `Absolute` (pixels) take what is left after the ascender and descender.
  - The line spans the highest top and the lowest bottom of these boxes. A line in a fallback font with a taller box is taller, and no line is shorter than the first font's.
  - A broken font's ascender below the baseline, or descender above it, counts as 0, and a line's height is never below 0, so lines never stack upwards. A requested height is capped at `MAX_LINE_HEIGHT` (65,536 px).
- **Alignment.**
  - Lines align at the paragraph's start or end, or at the left, right or centre. They align across `max_width`, or across the widest line without one.
  - The width aligned is the line's content: the cells of the graphemes before those that hang, as they are placed. Hanging whitespace sits beyond it, where reordering puts it: at the right of a left-to-right line, at the left of a right-to-left one.
- **Output.** Positions are in pixels, y down, from the layout's top left.
  - Each `Line` has its bytes, its glyphs, its direction, and the x and width of its content. It also has its top, height and baseline.
  - Each `PlacedGlyph` has its font's index in the list, its glyph, its x and y, and its cluster's byte offset, for coloring spans.
- **Caret, hit testing and selection.**
  - **Positions.** A position is a byte offset at a grapheme boundary with an affinity. Downstream goes with the grapheme after the offset, upstream with the one before. The affinity tells apart the two places one offset can have:
    - where a line wraps: the end of the line before, or the start of the next;
    - where the direction changes: the trailing edge of the grapheme before, or the leading edge of the one after.
  - **Cells.** Each grapheme gets a cell: its extent on its line, from its cluster's glyphs, with a ligature split equally.
    - Graphemes that draw nothing take no share of their cluster, as when U+200B is dropped and its cluster merged with a letter's.
    - Cell edges are computed at the glyphs' final pen positions, in the order the glyphs are placed, so neighbouring cells share their edge to the bit.
    - A cluster that moves the pen back (kerning wider than the glyph) takes no room, so cells never overlap. Its glyphs keep the font's positions, but the next piece (a tab, or a run shaped on its own) starts after the cells before it, as the widths that tab stops and line breaking use assume.
    - Edges inside a cluster are kept within it, even where rounding at subnormal sizes would move them out.
    - Each line keeps its cells that take room in order on screen, from their places after alignment, so hit testing and each arrow key press take logarithmic time. Movement compares cell edges, not midpoints, which round at large coordinates.
    - `caret(position)` gives a bar the height of the line.
    - `hit(x, y)` and `hit_line(line, x)` give the nearest grapheme edge, upstream only where that is a different place, compared exactly. A position is never after a line break. Moving up or down a line keeps x through `hit_line`. An x beyond the line, infinite or NaN gives its end on that side.
    - The caret round-trips exactly: `hit_line` at a caret's own x and line gives a position with the same caret. `hit` takes the first line that ends below y, so of lines with no height it cannot reach those that share their y with a later line.
  - **Selection.** `selection(range)` gives one rectangle per stretch of adjacent selected graphemes on each line, as the directions place them; a selection across a direction change has several.
  - **Moving.** `move_left` and `move_right` move one grapheme on screen, passing over graphemes that take no room. At a line's end they go on to the next or previous line, by the paragraph's direction, and come into it at its near end on screen: moving right, at its left end. Where a line wraps they move past the shared offset, so no key press is spent on it; on a line with nothing that takes room they go to its other end. `line_start` and `line_end` give the positions for Home and End.
- **Bounds.** The size is capped at `MAX_GLYPH_SIZE` (4,096 px), the largest the rasterizer draws, so sums of advances stay finite. A size or a line height that is not a positive finite number counts as 0. A width that is not positive counts as 0, where every grapheme that takes room gets a line of its own, and an infinite one as no width.
- **Font fallback by direction**, deferred from ADR 0028.
  - `shaping::itemize_with(text, fonts, direction)` judges coverage in the run's direction. Right to left, a font with a character's mirror image covers it, as shaping draws the mirror image.
  - `itemize` stays left to right.
- **A faster coverage check.**
  - A lone character with no decomposition, outside Hangul and other than U+2011, can only be mapped to its own glyph, its mirror image or the space. The per-character check already covers those. So font fallback no longer runs the normalization for it, as it did for every character a font lacks.
  - Over 20,623,360 checks the result is unchanged: every code point up to U+2FFFF, in both directions, with the first face of every seventh installed font file.
  - Font fallback for Japanese text over a Latin-first font list takes about 40% less time.
- **`bidi` internals:** the line functions also work on borrowed parts, so a layout keeps the paragraphs and levels without borrowing the text. The public API is unchanged.
- **Not done yet:**
  - styles within a paragraph (spans of other fonts, sizes or colors), justification, hyphenation, truncation with an ellipsis, letter and word spacing, decorations, and vertical text;
  - reuse of glyphs on wrapped lines: a cut item is shaped again, so wrapping costs about one more shaping of the cut lines. HarfBuzz's unsafe-to-break flags would tell which cuts can reuse the glyphs, if relayout needs to be faster;
  - context across a cut: the pieces are shaped alone, without HarfBuzz's pre- and post-context, which matters for joining scripts (no Arabic shaper yet);
  - caret positions inside a ligature are split equally; the GDEF ligature caret list is not read;
  - which graphemes draw nothing is judged from their Unicode properties, not from the glyphs: a font that deletes a letter's glyph with GSUB still gives the letter a share of its cluster, and one that draws U+200B still has it hang;
  - between paragraphs of opposite directions, the arrow keys can go back and forth between two lines, as visual caret movement does in other editors.

## Verification

- **Unit tests** run on hand-built fonts whose advances are exact in binary at the test size, so positions are compared exactly. They cover:
  - breaking at the last opportunity, hanging spaces, exact fits and the 1/64 px tolerance;
  - words broken between graphemes, and widths of zero, NaN and infinity;
  - mandatory breaks, with the caret and clicks around them;
  - right-to-left paragraphs: alignment, hanging spaces on the left, and the last empty line;
  - mixed lines: the glyph order, the caret on both sides of a direction change, hit testing, and a selection in two rectangles;
  - arrow keys across a mixed line and across a wrap;
  - ligature carets, and ligatures cut by a break and shaped again, with the break moving back when a line comes out wider;
  - tab stops, including the half-space rule;
  - line heights with a fallback font in all three modes, and alignment;
  - empty text, text with no fonts, mirroring, and fallback by mirror image;
  - the cases the review found (see Review), each failing before its fix:
    - arrow keys into a wrapped right-to-left word, and across a line with nothing that takes room;
    - a last line that comes out wider, a 60-glyph ligature, and a line break kept with its line at width 0;
    - a dropped U+200B, a negative advance, and a tab after one;
    - 50,000 arrow key presses on one line;
    - lines holding two paragraphs, at a width that just fits;
    - sizes of 1/1024 px and of the smallest subnormal unit, `f32::MAX` sizes and line heights, and alignment at 2^24 and 2^28 px;
    - a font with its ascender below the baseline.
- **Random texts.** 400 texts mix Latin, Hebrew, digits, brackets, tabs, newlines, kana, combining marks, RLM, U+200B, PDF and U+001C. They are laid out in each paragraph direction and alignment at random widths. Each layout is checked for:
  - lines that cover the text in order and stack without gaps;
  - lines within the width unless they hold one grapheme that draws;
  - the content inside each line's box, with only what hangs outside it (containment only: the box may be larger);
  - glyphs in order, with their clusters in their line;
  - the caret round-tripping at every boundary, with both affinities;
  - when every paragraph has one direction, the arrow keys passing every cell edge on screen and reaching an end.
- **Installed fonts.** Every installed face lays out a mixed text at five widths: Turkish, Japanese, Hebrew with points and brackets, a fraction, leading marks, joiners, emoji ZWJ sequences, a flag, an ideographic variation sequence, Hangul, CR LF, a paragraph separator and a tab. The same checks apply but the arrow keys and the glyph order (marks and kerning move real glyphs' origins back), with the caret checked at one width. On the development machine that is 402 faces. The test found two bugs, both fixed:
  - edges computed apart from the pens drifted by a bit, so a caret did not round-trip;
  - lines that lost kerning at their start came out up to 0.5 px wider than the width, with no earlier opportunity to fall back to.
- **By eye.** `cargo run --release -p duydas-text --example paragraph` lays out Turkish, Japanese, Hebrew and mixed paragraphs with Segoe UI and Yu Gothic. It draws the line boxes, a selection and two carets into a PNG.
  - Japanese lines keep 。 and 」 off their starts.
  - Right-to-left paragraphs align right, with their English runs in order and brackets mirrored.
  - Tabs reach their stops, and a long link breaks after a slash.
- **Performance.** `cargo run --release -p duydas-text --example shape_bench` lays out Turkish, Japanese and English messages of 41–90 characters, and a mixed one of 284, over Segoe UI, Yu Gothic and Minion Pro. Over five runs on the development machine:
  - `ShapedText::new` takes 412–845 ns a character for the Turkish and English messages, 857–1,716 ns for the Japanese one and 426–1,018 ns for the mixed one: 27–72 µs a short message;
  - a layout at 240 px, where every message wraps, takes 400–1,019 ns a character, about as much as shaping, since the cut runs are shaped again;
  - a layout at 640 px, where the short messages fit on one line, takes 95–410 ns a character.

  So a screen of 50 messages of 100 characters would take an estimated 2.1–8.6 ms to shape the first time (2.1–4.2 ms for Latin text, Japanese at the top end), and 2.0–5.1 ms to lay out again at a new width where the messages wrap. Reusing glyphs across cuts, with unsafe-to-break flags, would cut the second number. Figures are in [performance.md](../performance.md).

## Review

The Codex CLI (`gpt-6.2-sol`, high reasoning effort, read-only) reviewed the change and reported 12 findings, each with an input that fails. All 12 held up when checked against the code, and all are fixed:

- **High: the arrow keys could skip a wrapped line.** Crossing a line end, the caret went to the next line's logical start, which in a right-to-left word is its right end, so Right went no further. It now comes into the line at its near end on screen.
- **Medium:**
  - a line that reached the end of its paragraph kept no earlier opportunities, so when shaping made it wider it broke inside a word;
  - once 16 back-offs were used, a line could stay up to three times too wide (now bisected);
  - graphemes dropped by shaping, such as U+200B in a font with no space glyph, took a share of the letter they merged with;
  - negative advances gave overlapping cells, empty selections and wrong hits;
  - each arrow key press sorted the line's cells, so walking a long line took quadratic time;
  - a line holding two paragraphs of opposite directions boxed and aligned the wrong part of itself.
- **Low:**
  - an invisible PDF after a space stopped the space from hanging;
  - a 1/1024 px tolerance merged distinct caret places at tiny sizes;
  - `f32::MAX` sizes gave NaN widths, and an infinite x hit the line's start;
  - a font with a negative ascender gave lines of negative height;
  - the docs said a new width costs line breaking only, and that a width of 0 breaks after every grapheme even where whitespace hangs.

It found no change in coverage from the faster check, and no indexing or subtraction panic.

A second pass reviewed the fixes. It confirmed each of the 12 and reported 8 more cases: 6 are fixed, and 2 are documented as limits.

- **Medium:**
  - a line holding two paragraphs could put its hanging space mid-line and wrap early (the line now takes its last paragraph's direction);
  - the bisection could miss a longer line that fits. Now documented: it finds a line that fits, not always the longest;
  - after a cluster that moves the pen back, a tab stop and line breaking measured from the cells, but the glyphs from the pen;
  - Unicode properties, not glyphs, decide what draws nothing. This is now listed under Not done yet, as following glyphs through GSUB would need shaping to report it.
- **Low:**
  - at subnormal sizes, edges inside a ligature could round past its end;
  - alignment far from the origin could round cells away after the visual order was built, and a midpoint could round onto an edge and stop the arrow keys;
  - a zero line height could round below 0, and huge requested heights overflowed;
  - at width 0, a line break could be moved to a line of its own, and the arrow keys could stop on a line with nothing that takes room.

It also corrected the docs on relayout, on what bisection finds, and on the performance figures.

## Consequences

- **Next in step 4:** the glyph cache. It holds rasterized masks keyed by font, glyph, size and subpixel offset, and feeds `duydas-render`.
- **Then step 5:** widgets keep a `ShapedText` per message and per composer, and call `layout` when the width changes. IME preedit and styled spans will need spans within a paragraph.
- ADR 0027 estimated 2.5 ms to shape a screen of 50 messages. With segmentation, bidi, font fallback and line breaking included, the estimate from measurements is 2.1–8.6 ms, within a frame; see Performance.
