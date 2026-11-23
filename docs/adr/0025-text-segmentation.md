# 0025. In-house Unicode text segmentation

- Status: Accepted
- Date: 2026-11-23

## Context

ADR 0021 step 4 replaces cosmic-text and the crates it brings in (unicode-segmentation, unicode-linebreak, and others) with an in-house text stack. Segmentation is its base. Everything above it needs to know where text may be split:

- **The composer.** It moves the cursor and deletes by user-perceived character, so `İ` written as I + U+0307, a flag or a family emoji goes as one unit. It also moves and selects by word.
- **Layout.** It wraps lines only where UAX #14 allows. Japanese needs its line-breaking rules (kinsoku): no closing bracket, small kana or 。 at the start of a line, and no opening bracket at the end.
- **Fonts and shaping**, which come next. They work in grapheme clusters, so a combining mark never ends up in a different font run from its base.

## Decision

- **A new crate, `duydas-text`, with no dependencies and no `unsafe`.** It works on Unicode 18.0, the current version, and takes byte offsets into UTF-8 text throughout.
- **Grapheme clusters: extended grapheme clusters (UAX #29).** This includes Unicode 18's revised GB9c for Indic conjuncts.
  - `graphemes` iterates over clusters.
  - `next_grapheme_boundary`, `prev_grapheme_boundary` and `is_grapheme_boundary` start from any index. They step back only to the nearest position where a boundary is certain: a control character, or an ordinary character that no rule can join to what precedes it.
  - So the cost depends on the cluster, not the text length: about 25–40 ns a step through 100,000 emoji, Hangul syllables or Devanagari letters with nukta. The exception is a run of regional indicators: flags pair up from the start of the run, so each step looks back to it. Stepping through 2,000 flags in a row costs about 11 µs a step. The editor can cache boundaries if that ever matters.
- **Words: the default UAX #29 rules.**
  - `words` returns every segment, and `is_word` tells words from spaces and punctuation, by the Unicode 18 Alphabetic and numeric properties. It does not use `char::is_alphanumeric`, whose Unicode version depends on the compiler.
  - Letters, digits, katakana runs, `İstanbul'da` and `1.000,5` stay together. Each kanji and each hiragana character is a word of its own.
  - `next_word_boundary` and `prev_word_boundary` start from any index, the same way. A position is certain when no rule could join the characters around it. Starting there loses nothing, because the rules that look further back only join where a rule at that position could have joined too. A step costs as much as the word: about 45 ns through 100,000 periods or alternating letters, digits and periods.
  - Splitting Japanese into real words needs a dictionary. UAX #29 leaves that to tailoring, and we do not ship one.
- **Line breaking: every UAX #14 rule, in order.**
  - Breaking runs as a lazy iterator over LB9 units (a character plus its combining marks), looking at most two units ahead.
  - LB1's default resolution happens when the tables are generated: AI, SG and XX become AL. SA (Thai, Lao, Khmer, Myanmar) becomes CM for combining marks and AL otherwise. `Strictness` resolves CJ (small kana and ー):
    - `Strict`, the default, treats them as non-starters, so they stay with the character before them. A space before one can still allow a break, unless an earlier rule forbids it (LB16, after a closing bracket). This is the UAX #14 default, and what CSS `line-break: normal` and `strict` do;
    - `Loose` treats them as ideographs, UAX #14's tailoring for relaxed kana rules. This is the small-kana part of CSS `line-break: loose`.
  - The breaker reports opportunities and mandatory breaks. When a word is wider than the line, layout breaks it at grapheme boundaries.
- **The standards disagree in rare cases.** UAX #14 allows a break between a space and a combining mark after it (LB10), and a UAX #29 word boundary can fall between two consonants that a virama joins into one cluster. The functions follow their standards. Layout and the editor place soft wraps and caret stops only at grapheme boundaries too, walking the grapheme boundaries alongside rather than asking `is_grapheme_boundary` for each position.
- **Data comes from the Unicode Character Database, generated and committed.**
  - `cargo xtask unicode-gen` downloads the pinned version's files over HTTPS with curl and writes `src/tables.rs`. `--check` verifies the committed tables match.
  - The tables hold Line_Break, Grapheme_Cluster_Break, Word_Break, Indic_Conjunct_Break, and Extended_Pictographic. They also hold whether East_Asian_Width is wide, whether General_Category is Pi, Pf or Cn, and whether a character is Alphabetic or a number (for `is_word`).
  - The 149 distinct combinations are stored once, in a palette, behind a three-level trie: about 35 KB, three array reads per lookup.
  - Unicode data is used under the Unicode License v3, copied into the crate as `LICENSE-UNICODE` and referenced from `NOTICE`.
- **Conformance.** The official GraphemeBreakTest, WordBreakTest and LineBreakTest files are committed as `tests/data/` (0.6 MB), stripped to their test lines. Every case passes: 853 grapheme, 1,944 word and 19,346 line break cases.

## Measurements

`cargo run --release -p duydas-text --example bench`: 4 MB of mixed Turkish, Japanese and English chat text with emoji, Windows 11, i5-11400H, best of five.

| Operation | Throughput |
|---|---|
| `graphemes` | 141–156 MB/s |
| `next_grapheme_boundary`, stepping through the whole text | 72–82 MB/s |
| `words` | 70–79 MB/s |
| `prev_word_boundary`, stepping back through the whole text | 39–48 MB/s |
| `line_breaks`, either strictness | 60–76 MB/s |

These are the ranges over six runs. An earlier run on the same machine measured up to 40% higher. Breaking a 4 KB message into lines takes under 70 µs.

## Consequences

- **Upgrading Unicode** means changing the version in `xtask/src/unicode.rs`, running `cargo xtask unicode-gen` and fixing whatever the new conformance tests catch. A new property value fails the build until its enum gains the variant.
- **Thai, Lao, Khmer and Myanmar text** breaks only at spaces and punctuation, because those scripts need dictionary-based word breaking (Line_Break=SA resolves to AL).
- **Not included yet:**
  - sentence boundaries;
  - the rest of CSS `line-break: loose`: breaks before iteration marks, inseparable characters, centered punctuation and suffixes;
  - bidirectional text (UAX #9), which layout needs for Arabic and Hebrew messages and which comes with it.
- **Not run in CI:** `unicode-gen --check`, because it downloads from unicode.org. The committed tables are reviewed as code, and the conformance tests run on every push.
- **Next in step 4:** OpenType parsing, glyph rasterization, font fallback and shaping, then paragraph layout. Each part gets its own ADR.
