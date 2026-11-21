# 0018. Spike: virtualized message list

- Status: Pending
- Date: 2026-11-21

## Context

Iced has no variable-height virtual list. Chat needs bottom anchoring and stable scroll position when older history is prepended.

## Decision

Prototype a custom widget in `duydas-ui` with a measured-height cache; test 10k messages, prepend, resize and 200% scale on both renderers. Pass = scroll frame time p99 ≤ 16.7 ms with no stall > 50 ms.

## Consequences

Feeds the renderer decision in ADR 0017.
