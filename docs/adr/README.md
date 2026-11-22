# Architecture Decision Records

Each ADR records one decision: context, decision, consequences. Copy [`template.md`](template.md) for new ones. Superseded ADRs stay, marked as such.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-iced-toolkit.md) | Use Iced 0.14 from crates.io | Accepted |
| [0002](0002-workspace.md) | One Cargo workspace for client, server and shared crates | Accepted |
| [0003](0003-elm-ui-headless-sdk.md) | Elm-architecture UI over a headless client SDK | Accepted |
| [0004](0004-protobuf-wire.md) | Protobuf over WSS + HTTPS | Accepted |
| [0005](0005-server-monolith-durable-log.md) | Single modular server with a transactional event log | Accepted |
| [0006](0006-voice-str0m-sfu.md) | Voice SFU built on str0m | Accepted |
| [0007](0007-fluent-icu4x.md) | Fluent messages with typed keys; ICU4X formatting | Accepted |
| [0008](0008-local-cache-keychain.md) | SQLite cache, tokens in the OS keychain | Accepted |
| [0009](0009-protection-matrix.md) | Protection matrix and E2EE scope | Accepted |
| [0010](0010-distribution-update-trust.md) | Velopack packages, Flatpak, and TUF-style update metadata | Accepted |
| [0011](0011-no-telemetry.md) | No telemetry by default | Accepted |
| [0012](0012-permissions.md) | Deny-by-default u64 permission flags | Accepted |
| [0013](0013-markdown.md) | pulldown-cmark with a strict rendered subset | Accepted |
| [0014](0014-license.md) | Apache-2.0 | Accepted |
| [0015](0015-spike-text-and-ime.md) | Spike: Turkish/Japanese text and IME | Pending: needs manual runs |
| [0016](0016-spike-accessibility.md) | Spike: screen-reader support | Pending |
| [0017](0017-spike-renderer-baseline.md) | Spike: renderer, startup and memory baseline | Revised: wgpu low-power (DX12 on Windows); open startup question |
| [0018](0018-spike-virtual-list.md) | Spike: virtualized message list | Measured on Windows: pass on wgpu, fail on tiny-skia |
| [0019](0019-spike-audio-pipeline.md) | Spike: audio pipeline | Pending |
| [0020](0020-spike-update-trust.md) | Spike: Velopack + update metadata verification | Pending |
