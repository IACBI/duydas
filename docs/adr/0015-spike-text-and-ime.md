# 0015. Spike: Turkish/Japanese text and IME

- Status: Pending: needs manual runs
- Date: 2026-11-21

## Context

Japanese IME behavior and glyph selection cannot be verified headlessly. Code reading shows Iced 0.14 `text_editor` does not suppress key bindings during preedit, and cosmic-text chooses the Han fallback from the system locale.

## Decision

The desktop shell tracks composition state from `InputMethod` events. Enter sends only when the composer is focused and no composition is active; while the flag is set, Enter is swallowed (neither send nor newline), because the subscription-driven flag can trail the widget by one event and a stale flag must cost at most one extra key press. A Send button gives a path that does not depend on key-event ordering. Run the IME matrix in `docs/i18n-guide.md` on Windows (MS-IME), macOS and Linux (IBus/fcitx5), plus mixed-Han text on a non-Japanese system. Pass = every cell passes; otherwise patch upstream or pin a fixed revision (ADR 0001).

## Consequences

Results go here. Until then IME correctness is **unverified**.
