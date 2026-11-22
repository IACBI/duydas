# 0022. In-house FTL parser, resolver and locale data

- Status: Accepted
- Date: 2026-11-22
- Supersedes: ADR 0007

## Context

ADR 0021 removes Fluent and ICU4X. The app still needs everything ADR 0007 provided: typed keys, live switching, plurals, locale-aware numbers and dates, Turkish casing, and validation before release.

## Decision

- **`duydas-i18n::ftl`** parses the Fluent 1.0 syntax translators already know. It covers messages, terms, attributes, comments, multiline patterns with Fluent's indentation rules, string and number literals, variables, message and term references, and select expressions. Functions (`NUMBER()`) and parameterized terms are rejected with a clear error: values are formatted in code and passed in (`$countText`). The parser is std-only, because `build.rs` compiles it to generate `Key`.
- **A resolver** (`bundle.rs`) follows Fluent's semantics:
  - the first variant whose key matches wins, in source order (exact numbers and plural categories alike); plural categories use CLDR operands, so `-1` is `one` in English and Turkish, and the literal `1.0` is `other` in English but `one` in Turkish;
  - terms do not see the caller's variables, and a select on one inside a term takes the default variant; term attributes can be used only as selectors;
  - a message resolves at most 100 placeables, references included, which bounds the work a translation can cause;
  - cycles, missing variables and missing references are errors, so the localizer falls back to English instead of showing partial text.
- **The parser** limits placeable nesting to 16. It recovers from an error the way Fluent does: the broken entry is skipped up to the next line that can start an entry.
- **Validation** (`schema::validate`) catches syntax errors, duplicate ids, dangling message, term or attribute references, and reference cycles in any variant. `build.rs` runs it on every locale, so a binary cannot embed broken FTL. `cargo xtask i18n-check` adds the cross-locale checks (missing keys, unknown variables).
- **Locale data is hand-written from CLDR** for en, tr and ja in `format.rs`:
  - plural rules;
  - grouping separators (minimum grouping digits 1);
  - long date patterns and month names;
  - Turkish dotted/dotless i casing. Other characters use std's full Unicode case mapping.
- **Negotiation** matches language plus region, then language alone, in preference order, and accepts POSIX names such as `tr_TR.UTF-8@euro`.
- **The system language** comes from `duydas-sys::locale`:
  - Windows: `GetUserPreferredUILanguages`;
  - macOS: `CFLocaleCopyPreferredLanguages`;
  - other Unix: gettext's rules. A "C" or "POSIX" locale means untranslated (English). Otherwise `LANGUAGE` replaces the locale, and a "C" entry ends that list.

## Consequences

- Adding a language means adding its CLDR rules (plural, grouping, date pattern, any casing rules) to `format.rs` with tests, besides its FTL files. `docs/i18n-guide.md` lists this.
- The supported syntax is smaller than full Fluent. Translators get an explicit error rather than silently different behavior.
- Dates before 1 CE return `None`; eras are not supported.
- `Args` takes integers (`u64` and `usize` saturate at `i64::MAX`) and strings. Fractional numbers are formatted in code and passed as strings.
- Binary size drops by the size of the ICU4X data tables.
