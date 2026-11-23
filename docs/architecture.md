# Architecture

This document is the map. Decisions and their reasoning live in [`adr/`](adr/README.md).

## Shape of the system

```
┌──────────────────────────────── duydas-desktop ────────────────────────────────┐
│  view(state) ← update(state, message)        duydas-ui (tokens, widgets)       │
│        ▲                     │               duydas-i18n (own FTL + CLDR data) │
│        │ Subscription        │ Task                                            │
│  ┌─────┴─────────────────────▼──────┐   ┌───────────────────────────────────┐  │
│  │ duydas-client (headless)         │   │ duydas-voice (audio threads,      │  │
│  │ gateway state machine, outbox,   │   │ Opus, NS/AEC, WebRTC transport)   │  │
│  │ REST, local cache, keychain      │   └───────────────┬───────────────────┘  │
│  └───────────┬──────────────────────┘                   │                      │
└──────────────┼──────────────────────────────────────────┼──────────────────────┘
               │ WSS (protobuf events) + HTTPS            │ DTLS-SRTP over UDP / ICE-TCP
┌──────────────▼──────────────────────────────────────────▼──────────────────────┐
│ duydas-server: auth · gateway · rest · sfu · media           PostgreSQL        │
└────────────────────────────────────────────────────────────────────────────────┘
```

This is the planned shape: application boundaries and protocols, not current behaviour. Today the desktop shell runs on Iced. ADR 0021 sets the order in which existing dependencies are replaced and earlier library choices revised, keeping only Opus and audited cryptography from outside. The crate table records what exists today.

## Crates

| Crate | Role | Status |
|---|---|---|
| `duydas-core` | Pure domain logic: permissions, ids, validation. No I/O. | Permissions (hand-written flags) |
| `duydas-proto` | Protobuf schema (`proto/`) and generated types. | Protocol version constant and a `Hello` schema stub; generated types planned |
| `duydas-i18n` | FTL parser and resolver, typed keys, negotiation, number/date/case formatting. No third-party crates (ADR 0022). | **Complete for Phase 0** |
| `duydas-sys` | Hand-written OS bindings; the only crate allowed `unsafe` (ADR 0021). | System UI languages; window, input, IME, clipboard and presentation on Windows (ADR 0023) |
| `duydas-client` | Headless SDK: connection, outbox, cache, sync. | Documentation stub; Phase 1 |
| `duydas-voice` | Audio pipeline and WebRTC transport. | Documentation stub; audio spike pending (ADR 0019), voice in Phase 4 |
| `duydas-render` | CPU renderer: canvas, display list, rasterizers, damage tracking and scroll blitting. No runtime dependencies, no `unsafe` (ADR 0024). | Step 3 done, exercised by the `scroll` example (which uses `duydas-sys` as a dev-dependency); the app still draws with Iced |
| `duydas-text` | Text stack: Unicode segmentation (grapheme clusters, words, line breaking with Japanese rules), OpenType parsing and glyph rasterization; shaping, font fallback and layout to come. No dependencies, no `unsafe` (ADR 0025, 0026). | Segmentation (Unicode 18.0) and fonts (TrueType and CFF outlines, coverage masks), standalone; shaping, fallback, layout and app integration planned |
| `duydas-ui` | Design tokens, theme, custom widgets. | Tokens, Iced theme and the `virtual_list` prototype |
| `apps/duydas-desktop` | The Iced application. | Phase 0 shell |
| `apps/duydas-server` | Self-hostable server binary. | Stub that prints a notice; Phase 1 |
| `xtask` | `i18n-check`, `perf`, benchmarks, later `proto-gen` and release helpers. | i18n-check, perf, vlist, render-bench, unicode-gen |
| `spikes/virtual-list` | Phase 0 scroll benchmark on Iced (ADR 0018); not shipped. | Measured on Windows; macOS and Linux pending |

## Key invariants

These are design requirements. The current shell already keeps the UI a function of state; client I/O, durable delivery, authorization and real-time audio arrive with the phases below.

- **The UI is a function of state.** All I/O happens in `duydas-client` and `duydas-voice`; the UI receives events through bounded channels and sends commands as `Task`s.
- **Message delivery is durable.** The server writes each message and its event-log entry in one transaction and assigns a per-channel sequence number. In-process broadcast only wakes up connections; clients resume from their last sequence, and a connection that falls behind gets an explicit resync. Clients deduplicate with client-generated send ids.
- **Authorization has one entry point.** Every REST and gateway handler calls `authorize(resource, action)`; permission-sensitive writes re-check inside the transaction, and revocation closes affected live subscriptions and voice sessions.
- **No hardcoded UI strings.** The build generates `Key` from the English FTL files, and `cargo xtask i18n-check` checks locale coverage and flags literals passed to UI calls.
- **Audio callbacks never allocate or lock.** Real-time threads exchange data with the rest of the app through lock-free ring buffers.

## Version skew

Planned policy: a client supports the current and previous server protocol version (N-1), and vice versa. Unknown event types are ignored; errors carry stable codes that clients localize.

## Roadmap

These phases are product milestones. ADR 0021 tracks the in-house replacements separately (issue #4), and its toolkit work pushes the timeline below back.

| Phase | Outcome |
|---|---|
| 0 | Foundations: workspace, i18n, CI, governance, feasibility spikes with go/no-go decisions |
| 1 | Vertical slice: minimal server, durable send/replay, real client shell, cache and logout policy |
| 2 | Invite-only text alpha: roles, invites, moderation basics, TOTP, self-hosting guide |
| 3 | Messaging completeness: replies, reactions, uploads, notifications, Turkish/Japanese search |
| 4 | Voice beta: SFU, devices, push-to-talk, noise suppression, echo cancellation, TURN |
| 5 | End-to-end encryption track: MLS direct messages, SFrame calls, passkeys, external review |
| 6 | Release engineering and 1.0: signed packages, verified updates, accessibility gate |
