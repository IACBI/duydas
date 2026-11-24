# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Cargo workspace: client, server, shared crates and `xtask`.
- `duydas-i18n`: Fluent-syntax messages with compile-time typed keys, system language detection, live language switching, number/date formatting, Turkish-aware case mapping and a pseudo-locale.
- en-US, tr-TR and ja-JP seed translations.
- Desktop shell with language picker, formatting preview and an IME-aware composer.
- `cargo xtask i18n-check` and `cargo xtask perf`.
- CI: formatting, i18n check, typos, cargo-deny, rustdoc and MSRV, plus clippy, nextest and doctests on Windows, macOS and Linux. CodeQL and Dependabot.
- Architecture docs, ADRs, threat model draft, performance budgets and contributor guides.
- Send button in the composer.
- `duydas-ui::virtual_list`: windowing for variable-height, bottom-anchored lists (Fenwick-tree height index, measured rows, prepend-safe positioning).
- `spikes/virtual-list` and `cargo xtask vlist`: scripted 10k-message scroll benchmark across renderers and scale factors.
- `duydas-sys::window`: in-house window layer with a Win32 backend (ADR 0023). It has:
  - a blocking event loop;
  - keyboard and text input, and IME composition with candidate placement;
  - pointer, wheel and capture-loss events;
  - clipboard text and per-monitor DPI;
  - damage-based presentation of CPU-rendered frames.

  In the example on Windows 11: first frame 14 ms (p50) / 16 ms (p95) after `main` over 21 runs, 5.8 MB private memory, 0.0 ms CPU over a 3 s idle sample.
- `duydas-render`: in-house CPU renderer (ADR 0024), with no runtime dependencies and no `unsafe` (its windowed example uses `duydas-sys`). It has:
  - anti-aliased rectangles, rounded rectangles and borders;
  - glyph and icon masks, and images with rounded corners;
  - clips and scroll layers;
  - tile-fingerprint damage tracking, so only damaged tiles are redrawn and presented;
  - scroll blitting, so scrolling moves existing pixels and repaints the exposed rows plus whatever changed.

  Incremental frames are tested pixel for pixel against full redraws.
- `cargo xtask render-bench`: replays the ADR 0018 scrolling session on the new renderer and checks the budget. Headless with synthetic glyphs, median p99 over five runs with damage tracking is 1.5 ms at 1× and 2.5 ms at 2× (budget: 16.7 ms), with no stalls over 50 ms.
- `duydas-text`: in-house Unicode 18.0 text segmentation (ADR 0025), with no dependencies and no `unsafe`. It finds grapheme clusters, word boundaries, and line break opportunities that follow the Japanese line-breaking rules, with a strict and a loose mode for small kana. It passes Unicode 18.0's grapheme, word and line break conformance tests.
- `cargo xtask unicode-gen`: regenerates `duydas-text`'s tables and conformance data from the Unicode Character Database.
- `duydas-text::font`: in-house OpenType parsing and glyph rasterization (ADR 0026), with no dependencies and no `unsafe`. It reads TrueType and CFF outlines (CID-keyed included) from fonts and collections, character maps with Japanese variation sequences, metrics and names, and turns glyphs into anti-aliased coverage masks for `duydas-render`. Malformed fonts fail with an error, with the work per glyph bounded. It reads all 2,051,134 glyphs of 415 installed faces without an error, and rasterizes a glyph in 3–16 µs at 12–48 px.
- `duydas-text::shaping`: in-house text shaping and font fallback (ADR 0027), with no dependencies and no `unsafe`. It applies a font's GSUB and GPOS lookups (ligatures, contextual and language-specific forms, kerning, mark positioning) or its legacy `kern` table, with HarfBuzz's feature planning, normalization and fallback mark positioning, and picks a font from a fallback list for each grapheme cluster. For left-to-right scripts that need no script-specific shaper, its output matches HarfBuzz 10.4.0 in all 275,310 runs over 437 installed faces. A hostile font cannot make it run away: the work per run is bounded as in HarfBuzz.

### Changed

- Third-party crates are being replaced with in-house code (ADR 0021). Done so far: localization (own FTL parser and resolver, hand-written CLDR data for en, tr and ja), permissions flags and system language detection (`duydas-sys`, the only crate allowed `unsafe`). Our crates no longer depend on ICU4X, Fluent, sys-locale, unic-langid or bitflags directly. sys-locale and bitflags remain in `Cargo.lock` through Iced until the in-house UI replaces it.
- The build now fails on invalid FTL in any locale: syntax errors, duplicate ids, references to missing messages, terms or attributes, and reference cycles.

### Fixed

- Plain Enter sends only while the composer is focused and no IME composition is tracked; Shift+Enter is left to Iced. Testing with real input methods is still pending.
- Messages missing a value or failing to resolve now fall back to English instead of showing partial text.
- Message counts use locale grouping (`1.234 mesaj`).
- Pseudo-locale no longer mangles interpolated user data.
- Generated localization code no longer embeds machine-specific absolute paths.
- `xtask perf` drains the child's stderr while waiting, so a chatty app cannot block on a full pipe.
- `xtask` decodes every JSON escape in the executable path Cargo reports, so a target directory with unusual characters works.
- `xtask perf` validates `--runs`, times out when no marker arrives, always cleans up the child process and uses the executable path Cargo reports.
- The hardcoded-string lint handles multi-line calls, raw strings and comments.

### Security

- CI runs clippy on Windows and macOS too, so platform-specific `unsafe` code is linted where it compiles.
- nextest stops a hung test after 2 minutes.
- CI installs typos and cargo-deny through `taiki-e/install-action`, which verifies SHA256 checksums of downloaded binaries.
