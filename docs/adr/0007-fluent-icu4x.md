# 0007. Fluent messages with typed keys; ICU4X formatting

- Status: Superseded by [ADR 0022](0022-in-house-localization.md)
- Date: 2026-11-21

## Context

Localization must exist from day one, switch live, handle plurals and Turkish casing, and catch mistakes before release.

## Decision

Fluent (`fluent-bundle` concurrent bundles, `fluent-langneg` 0.13 to share `unic-langid`, `sys-locale`) with FTL embedded at compile time. `build.rs` generates a `Key` enum and expected variables from en-US. ICU4X 2.x compiled data for numbers, dates and case mapping (`sync` feature, so `Localizer` is `Send + Sync`). Bidi isolation marks are off until an RTL locale ships.

## Consequences

Missing keys fail compilation or CI. ICU4X adds binary size; tracked in `docs/performance.md`.
