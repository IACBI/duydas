# 0026. In-house OpenType parsing and glyph rasterization

- Status: Accepted
- Date: 2026-11-23

## Context

ADR 0021 step 4 replaces cosmic-text. ADR 0025 added Unicode segmentation. This is the next part: reading fonts and turning glyphs into the coverage masks that `duydas-render` draws with `Scene::draw_mask` (ADR 0024).

Layout needs four things from a font:

- which glyph a character maps to, including Japanese ideographic variation sequences;
- glyph advances and line metrics;
- glyph outlines;
- anti-aliased glyph images.

The fonts that matter are:

- the system fonts on Windows, which use TrueType outlines and come as single fonts or collections (`.ttc`: Yu Gothic, Meiryo, MS Gothic);
- CFF-based OpenType fonts (`.otf`), including the CID-keyed fonts used for CJK;
- fonts we bundle later.

Rendering the system fonts surfaced two facts that shape the font plan:

- **Noto Sans JP has no Turkish letters.** Its `cmap` lacks ş, ı, İ and Ğ (U+015F, U+0131, U+0130, U+011E), checked against an independent reading of the table. Noto Sans JP alone cannot render Turkish, so the UI needs a Latin font first and font fallback. (Some Japanese fonts do cover Turkish; Yu Gothic does.)
- **The installed Noto Sans JP is a variable font whose default instance is Thin** (weight 100). Bundling it would need variation support or static instances.

## Decision

- **`duydas_text::font`, with no dependencies and no `unsafe`.**
  - `Font` parses a face of TrueType (`glyf`) or CFF data from a `.ttf`, `.otf` or `.ttc`, held in an `Arc<[u8]>` and read on demand.
  - `Rasterizer` turns a glyph into a `GlyphImage`: one coverage byte per pixel, the format `duydas_render::Mask` takes.
- **Malformed fonts fail, never panic.**
  - Every read goes through a bounds-checked reader, with checked offset arithmetic, so 32-bit targets cannot overflow either.
  - The extents of the tables this module reads are checked once, when the font is parsed. Each CFF INDEX entry must stay inside its INDEX.
  - The glyph count is the smaller of `maxp`'s and what `loca` or the CharStrings INDEX holds, so every glyph id the font hands out has an entry in the outline table. What the entry holds is checked when the glyph is read, and a broken glyph fails on its own.
  - A font whose `hmtx` lacks an advance and a left side bearing for every glyph is rejected.
  - The work and memory per glyph are bounded:
    - 2²⁰ points and components per TrueType glyph, with composites nested at most 8 deep;
    - 2²⁰ operators per CFF glyph, subroutines nested at most 10 deep, and a stack of 48;
    - 1 MiB of CFF Private DICTs per face, each parsed once however many font DICTs share it;
    - images up to 8,192 px a side and 4,194,304 px, at sizes up to 4,096 px per em, reaching at most 2¹⁶ px from the pen, where `f32` pixel coordinates round by at most 1/256 px;
    - 2²⁴ steps of flattening and filling per glyph (each piece of a curve, and each pixel row and column an edge crosses), so a glyph that reuses a tall component or a flat curve many times cannot take billions of steps within the other limits.
- **Character mapping.**
  - Formats 0, 4, 6, 12 and 13 are read, preferring full-Unicode subtables (format 12, then 13), then BMP ones, then Windows symbol fonts (which map ASCII at U+F000), then Mac Roman for ASCII.
  - A subtable whose arrays do not fit is skipped rather than hiding a usable one. Format 4's 16-bit length is ignored, because large subtables overflow it; the subtable runs to where the next one starts.
  - Format 14 gives `Font::glyph_variant`, for kanji variants such as 葛 with VS17.
- **Metrics.**
  - Line metrics come from `OS/2`'s typographic values when the font sets USE_TYPO_METRICS, and from `hhea` otherwise, as browsers and FreeType do. If `hhea` has neither ascender nor descender, the typographic values are used, then the Windows ones, as in FreeType.
  - x-height, cap height, underline and strikeout come from `OS/2` and `post`.
  - Weight, width, italic and monospace are kept for picking faces.
  - Names (family, style, full, PostScript) prefer US English, then other English locales, then Unicode, then other Windows languages (in the Unicode or symbol encodings, both UTF-16), then Mac Roman, which is decoded with Apple's mapping.
- **TrueType outlines.**
  - Simple glyphs follow the on- and off-curve rules: two off-curve points imply one halfway between them, and a contour may start off the curve.
  - Composite glyphs are assembled as points, applying each component's scale or 2×2 transform and its offset, scaled when flagged. A component can also be placed by matching one of its points to a point of the components before it in the same composite. Matching a phantom point (the metrics points after a glyph's own) fails with `Unsupported`; no glyph checked below does it.
  - The glyph's origin is its `hmtx` left side bearing to the left of the box in its header, and a composite takes the origin of a component flagged USE_MY_METRICS, untransformed, as FreeType does. Advances still come from `hmtx`, as HarfBuzz takes them; FreeType would take that component's. The outline is moved so the origin is at 0, as FreeType does. In nearly every font the bearing equals the box's left edge and nothing moves.
- **CFF outlines.** A Type 2 charstring interpreter for plain and CID-keyed fonts, choosing local subroutines by font DICT.
  - Hints and the width operand are skipped: widths come from `hmtx`, and there is no hinting.
  - FontMatrix is applied with FreeType's arithmetic: each matrix is normalized by its vertical scale, and glyphs are rescaled by the ratio of the top DICT's units per em to the font DICT's. So a plain font's matrix contributes only its shape (an oblique, say). A CID font's font DICT matrix applies within the top DICT's and keeps its scale relative to it: with no top matrix, a font DICT that says 1/1000 em in a 2,048-unit font scales its glyphs by 2.048. As in FreeType, the top DICT's translation is dropped when a font DICT has a matrix of its own; FreeType also rounds translations to whole units, which this code does not.
  - Operands and the current point are kept in `f64`, which holds charstring numbers (16.16 fixed point) exactly.
  - As in FreeType, a charstring that ends without `endchar` ends the glyph as `endchar` would, a subroutine that ends without `return` returns, `return` outside a subroutine is an error, and a moveto given extra operands takes the topmost ones. Operands an operator does not use are dropped; FreeType rejects some of those charstrings.
  - The accented `endchar` (seac) and the arithmetic operators fail with `Unsupported`. None of the 2 million glyphs checked below uses them.
- **Rasterization.**
  - Curves are flattened to within 1/32 px. A curve that would need more than 4,096 pieces fails with `TooLarge`; it would exceed the image limits anyway.
  - Each pixel gets the signed area the outline covers in it, accumulated per row and computed in `f64` where the slope needs it; the font-rs method.
  - The area is made positive and capped at full coverage, as font-rs and stb_truetype do. That is exact for contours that do not overlap, which is how fonts draw glyphs, and fills the inside of overlapping ones. An edge pixel that two overlapping contours share can come out darker or lighter than the non-zero winding rule gives, and two contours wound in opposite directions that meet inside one pixel without overlapping cancel there, leaving it lighter.
  - There is no hinting. Glyphs keep their designed shapes and are placed at fractional pixel positions; callers pass the pen's fraction, such as a quarter pixel.
- **Not supported yet:**
  - CFF2;
  - font variations (a variable font gives its default instance);
  - color glyphs (`COLR`/`CPAL`, `sbix`, `CBDT`) and bitmap strikes;
  - hinting.

  CFF2 and bitmap-only fonts fail with `Unsupported`, and fonts with no outlines at all with `MissingTable`.

## Verification

- **Hand-built fonts** with known outlines, in unit tests:
  - TrueType simple and composite glyphs, with short, repeated and same-as-before coordinate encodings, flag repeats that overrun the points, components placed by matching points (numbered within each nested composite), a phantom point, glyphs that are only a header, outlines moved by the left side bearing, and USE_MY_METRICS;
  - every `cmap` format we read and their order of preference, symbol fonts, variation sequences, broken subtables, format 12 and 13 subtables declaring more groups than they hold, and a variation selector whose mapping array is cut short, next to usable ones, a format 4 length too small for its arrays, and a format 4 glyph array read up to the next subtable but not into it;
  - collections, including a collection index whose offset would overflow 32 bits;
  - CFF path, hint, subroutine and flex operators, charstrings without `endchar`, subroutines without `return` and a `return` outside one, movetos with extra operands, 1,024 steps of 1/65,536 that add up, an oblique FontMatrix, CID font DICT matrices with and without a scale of their own, the top DICT's translation dropped under a font DICT matrix (a unit test of the matrix arithmetic), CharStrings offsets that point outside their INDEX, and a Private DICT shared by font DICTs (parsed once) or past the budget;
  - a CID-keyed CFF whose glyphs use different font DICTs;
  - glyph counts limited by the outline table, short `hmtx` tables, metric fallbacks (including a Windows descent of 32,768), and Mac Roman, British English and symbol-encoded names.
- **Rasterization tests:**
  - pixel-aligned squares are covered exactly, and moved half a pixel, their edge columns are half covered;
  - a hole wound the other way stays empty, a contour drawn twice covers once, and two squares wound in opposite directions that share one pixel cancel, as documented above;
  - sizes that are not finite and positive, and offsets that are not finite, fail with `InvalidSize`, and a glyph past 2¹⁶ px from the pen with `TooLarge`;
  - total coverage stays within the flattening bound (perimeter × 1/32 px) plus byte rounding of the area computed by fine subdivision, for TrueType and CFF curves;
  - a parabola 2,048 px tall strays at most 1/32 px from its flattened chords;
  - a sliver whose slope overflows `f32` leaves the image empty;
  - a glyph stacking 205,379 copies of a 1 × 8,000 px bar, and one made of 400,000 flat curves, fail with `TooLarge` in milliseconds.
- **Robustness:** the hand-built fonts truncated at every length, and 9,000 copies with random bytes changed, parse and rasterize without a panic.
- **Installed fonts.** A test parses every font in the operating system's font directories (the first 64 faces of a collection) and outlines and rasterizes a sample of each face's glyphs. CI runs it on Windows, macOS and Linux. It finds nothing to fail on a machine without readable fonts, so the counts it prints are what show its coverage.
  - With `DUYDAS_FONT_FULL` set, it covers every glyph. On the development machine:
    - 415 faces and 2,051,134 glyphs, with Acrobat's CFF fonts added through `DUYDAS_FONT_DIRS`;
    - no errors, no unsupported glyphs.
  - For 1,847,275 simple TrueType glyphs, the box around the decoded points equals the box in the glyph's header exactly. Nine icon web fonts are the exception: their tools compute boxes from unrounded outlines. The test fails if a face has differing boxes in 1% of its glyphs or fewer, which would point to a decoding error. A face with more is counted as one whose tool computed boxes its own way, and the test fails if more than 5% of faces are.
- **By eye.** The `specimen` example draws Turkish, Japanese and English text in Segoe UI, Yu Gothic, Meiryo, Noto Sans JP, Consolas, Times New Roman and Minion Pro (CFF).
- **Independent review.** Four rounds of Codex review found 17 issues, then 15 more (11 in the code, 4 claims in this record that said more than the code does), then 14 and 5 narrower ones. They are fixed and tested as described above, with these exceptions:
  - the coverage rule is kept and documented;
  - precision far from the pen is handled by the 2¹⁶ px limit rather than by computing pixel coordinates in `f64`, and a glyph whose flattened edge overshoots that limit by a fraction of a pixel fails with `TooLarge`;
  - an `hhea` that declares more long metrics than there are glyphs is clamped rather than rejected;
  - language tags in version 1 `name` tables are not read.

## Measurements

Windows 11, i5-11400H, `cargo run --release -p duydas-text --example font_bench`. Each glyph of the font is rasterized once, with a quarter-pixel offset, and the time is divided by the glyphs that have ink. Ranges over five runs; `glyph_index` is per character of mixed Turkish, Japanese and English text:

| Font | Glyphs | `glyph_index` | 12 px | 16 px | 24 px | 48 px |
|---|---|---|---|---|---|---|
| Segoe UI (TrueType) | 5,395 | 14–16 ns | 3.3–3.4 µs | 3.5–3.7 µs | 4.1–4.3 µs | 6.2–6.7 µs |
| Yu Gothic Medium (TrueType, collection) | 24,411 | 25–39 ns | 8.3–8.4 µs | 9.1–9.5 µs | 10.4–11.4 µs | 15.4–16.3 µs |
| Minion Pro (CFF) | 1,686 | 14–15 ns | 3.5–3.6 µs | 3.9–4.1 µs | 4.7–4.8 µs | 7.2–8.5 µs |

Parsing a face takes 1–13 µs, most of it checking that a format 4 character map's arrays fit; other tables are read when used. Once glyphs are cached (the cache is still to come), each is rasterized once; from these numbers, a screen with 2,000 distinct glyphs at 12–16 px would cost 7–19 ms the first time it appears. That is an estimate, not a measurement.

## Consequences

- **Next in step 4:**
  - shaping (`GSUB`/`GPOS`: kerning, ligatures, marks);
  - font fallback;
  - a glyph cache keyed by font id, glyph, size and subpixel offset;
  - paragraph layout and bidirectional text.
- **Fonts to bundle.**
  - Noto Sans JP alone cannot render Turkish, so the bundle needs a Latin font ahead of the Japanese one, with fallback between them.
  - Its variable version needs variation support (`fvar`, `gvar`, `HVAR`), or static instances made at build time.
- **Overlapping contours.** Variable fonts, once supported, often overlap contours in their instances. If the shared edge pixels show, resolve winding per pixel before integrating coverage.
- **No hinting.** At 100% scale, text is softer than DirectWrite with ClearType. That matches how macOS and Chrome on high-density displays render. Stem darkening or hinting can be added if testing with users asks for it.
- **Color emoji** need `COLR` (Segoe UI Emoji) and later `sbix`/`CBDT`. Until then, emoji show their monochrome outlines where a font has them.
