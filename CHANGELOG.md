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
