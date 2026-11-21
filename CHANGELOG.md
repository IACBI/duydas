# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Cargo workspace: client, server, shared crates and `xtask`.
- `duydas-i18n`: Fluent messages with compile-time typed keys, system language detection, live language switching, ICU4X number/date formatting, Turkish-aware case mapping and a pseudo-locale.
- en-US, tr-TR and ja-JP seed translations.
- Desktop shell with language picker, formatting preview and an IME-aware composer.
- `cargo xtask i18n-check` and `cargo xtask perf`.
- CI (fmt, clippy, tests on Windows/macOS/Linux, i18n, cargo-deny, rustdoc, MSRV), CodeQL and Dependabot.
- Architecture docs, ADRs, threat model draft, performance budgets and contributor guides.
- Send button in the composer.

### Fixed

- Enter no longer sends a draft when the composer is unfocused, and never sends or inserts a newline during an IME composition.
- Messages missing a value or failing to resolve now fall back to English instead of showing partial text.
- Message counts use locale grouping (`1.234 mesaj`).
- Pseudo-locale no longer mangles interpolated user data.
- Generated localization code no longer embeds machine-specific absolute paths.
- `xtask perf` validates `--runs`, times out when no marker arrives, always cleans up the child process and uses the executable path Cargo reports.
- The hardcoded-string lint handles multi-line calls, raw strings and comments.

### Security

- CI installs typos and cargo-deny through `taiki-e/install-action`, which verifies SHA256 checksums of downloaded binaries.
