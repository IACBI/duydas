# Architecture

This document is the map. Decisions and their reasoning live in [`adr/`](adr/README.md).

## Shape of the system

```
┌──────────────────────────── duydas-desktop (Iced) ────────────────────────────┐
│  view(state) ← update(state, message)        duydas-ui (tokens, widgets)       │
│        ▲                     │               duydas-i18n (own FTL + CLDR data) │
│        │ Subscription        │ Task                                            │
│  ┌─────┴─────────────────────▼──────┐   ┌───────────────────────────────────┐  │
│  │ duydas-client (tokio, headless)  │   │ duydas-voice (audio threads,      │  │
│  │ gateway state machine, outbox,   │   │ Opus, NS/AEC, str0m WebRTC)       │  │
│  │ REST, SQLite cache, keychain     │   └───────────────┬───────────────────┘  │
│  └───────────┬──────────────────────┘                   │                      │
└──────────────┼──────────────────────────────────────────┼──────────────────────┘
               │ WSS (protobuf events) + HTTPS            │ DTLS-SRTP over UDP / ICE-TCP
┌──────────────▼──────────────────────────────────────────▼──────────────────────┐
│ duydas-server: auth · gateway · rest · sfu · media           PostgreSQL         │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Crates

| Crate | Role | Status |
|---|---|---|
| `duydas-core` | Pure domain logic: permissions, ids, validation. No I/O. | Permissions (hand-written flags) |
| `duydas-proto` | Protobuf schema (`proto/`) and generated types. | Version constant + schema stub |
| `duydas-i18n` | FTL parser and resolver, typed keys, negotiation, number/date/case formatting. No third-party crates (ADR 0022). | **Complete for Phase 0** |
| `duydas-sys` | Hand-written OS bindings; the only crate allowed `unsafe` (ADR 0021). | System UI languages; window, input, IME, clipboard and presentation on Windows (ADR 0023) |
| `duydas-client` | Headless SDK: connection, outbox, cache, sync. | Phase 1 |
| `duydas-voice` | Audio pipeline and WebRTC transport. | Phase 4 (spike in Phase 0) |
| `duydas-ui` | Design tokens, theme, custom widgets. | Tokens + theme |
| `apps/duydas-desktop` | The Iced application. | Phase 0 shell |
| `apps/duydas-server` | Self-hostable server binary. | Phase 1 |
| `xtask` | `i18n-check`, `perf`, later `proto-gen` and release helpers. | i18n-check, perf |

## Key invariants

- **The UI is a function of state.** All I/O happens in `duydas-client` and `duydas-voice`; the UI receives events through bounded channels and sends commands as `Task`s.
- **Message delivery is durable.** The server writes each message and its event-log entry in one transaction and assigns a per-channel sequence number. In-process broadcast only wakes up connections; clients resume from their last sequence, and a connection that falls behind gets an explicit resync. Clients deduplicate with client-generated send ids.
- **Authorization has one entry point.** Every REST and gateway handler calls `authorize(resource, action)`; permission-sensitive writes re-check inside the transaction, and revocation closes affected live subscriptions and voice sessions.
- **No hardcoded UI strings.** Enforced by the generated `Key` enum and `cargo xtask i18n-check`.
- **Audio callbacks never allocate or lock.** Real-time threads exchange data with the rest of the app through lock-free ring buffers.

## Version skew

A client supports the current and previous server protocol version (N-1), and vice versa. Unknown event types are ignored; errors carry stable codes that clients localize.

## Roadmap

| Phase | Outcome |
|---|---|
| 0 | Foundations: workspace, i18n, CI, governance, feasibility spikes with go/no-go decisions |
| 1 | Vertical slice: minimal server, durable send/replay, real client shell, cache and logout policy |
| 2 | Invite-only text alpha: roles, invites, moderation basics, TOTP, self-hosting guide |
| 3 | Messaging completeness: replies, reactions, uploads, notifications, Turkish/Japanese search |
| 4 | Voice beta: SFU, devices, push-to-talk, noise suppression, echo cancellation, TURN |
| 5 | End-to-end encryption track: MLS direct messages, SFrame calls, passkeys, external review |
| 6 | Release engineering and 1.0: signed packages, verified updates, accessibility gate |
