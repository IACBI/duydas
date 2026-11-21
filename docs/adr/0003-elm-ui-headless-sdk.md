# 0003. Elm-architecture UI over a headless client SDK

- Status: Accepted
- Date: 2026-11-21

## Context

UI frameworks become hard to test when networking and storage leak into view code.

## Decision

The Iced app owns UI state only. `duydas-client` (tokio) owns connections, the durable outbox, REST and the SQLite cache, and talks to the UI through bounded channels exposed as an Iced `Subscription`. It cancels work on account or server switch.

## Consequences

The SDK is testable without a window; backpressure is explicit.
