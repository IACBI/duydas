# 0015. Spike: Turkish/Japanese text and IME

- Status: Pending: needs manual runs
- Date: 2026-11-21

## Context

Japanese IME behavior and glyph selection cannot be verified headlessly. Code reading shows Iced 0.14 `text_editor` does not suppress key bindings during preedit, and cosmic-text chooses the Han fallback from the system locale.

## Decision

The desktop shell tracks composition state from `InputMethod` events and only sends on Enter when no composition is active. Run the IME matrix in `docs/i18n-guide.md` on Windows (MS-IME), macOS and Linux (IBus/fcitx5), plus mixed-Han text on a non-Japanese system. Pass = every cell passes; otherwise patch upstream or pin a fixed revision (ADR 0001).

## Consequences

Results go here. Until then IME correctness is **unverified**.
