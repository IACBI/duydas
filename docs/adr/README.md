# Architecture Decision Records

Each ADR records one decision: context, decision, consequences. Copy [`template.md`](template.md) for new ones. Superseded ADRs stay, marked as such.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-iced-toolkit.md) | Use Iced 0.14 from crates.io | Accepted; being replaced by in-house code (0021) |
| [0002](0002-workspace.md) | One Cargo workspace for client, server and shared crates | Accepted; dependency and `unsafe` rules revised by 0021 |
| [0003](0003-elm-ui-headless-sdk.md) | Elm-architecture UI over a headless client SDK | Accepted |
| [0004](0004-protobuf-wire.md) | Protobuf over WSS + HTTPS | Accepted |
| [0005](0005-server-monolith-durable-log.md) | Single modular server with a transactional event log | Accepted for topology and delivery; library choices revised by 0021 |
| [0006](0006-voice-str0m-sfu.md) | Voice SFU built on str0m | Accepted for the SFU design; media stack revised by 0021 |
| [0007](0007-fluent-icu4x.md) | Fluent messages with typed keys; ICU4X formatting | Superseded by 0022 |
| [0008](0008-local-cache-keychain.md) | SQLite cache, tokens in the OS keychain | Accepted |
| [0009](0009-protection-matrix.md) | Protection matrix and E2EE scope | Accepted |
| [0010](0010-distribution-update-trust.md) | Velopack packages, Flatpak, and TUF-style update metadata | Accepted |
| [0011](0011-no-telemetry.md) | No telemetry by default | Accepted |
| [0012](0012-permissions.md) | Deny-by-default u64 permission flags | Accepted |
| [0013](0013-markdown.md) | pulldown-cmark with a strict rendered subset | Accepted; not implemented, parser to be revisited under 0021 |
| [0014](0014-license.md) | Apache-2.0 | Accepted |
| [0015](0015-spike-text-and-ime.md) | Spike: Turkish/Japanese text and IME | Pending: needs manual runs |
| [0016](0016-spike-accessibility.md) | Spike: screen-reader support | Pending |
| [0017](0017-spike-renderer-baseline.md) | Spike: renderer, startup and memory baseline | Windows measured: low-power wgpu with DX12 recommended for Iced; startup open; macOS and Linux pending |
| [0018](0018-spike-virtual-list.md) | Spike: virtualized message list | Windows measured: pass on wgpu, fail on tiny-skia; macOS and Linux pending |
| [0019](0019-spike-audio-pipeline.md) | Spike: audio pipeline | Pending |
| [0020](0020-spike-update-trust.md) | Spike: Velopack + update metadata verification | Pending |
| [0021](0021-in-house-implementation.md) | Write the stack in-house; libraries only for Opus and cryptography | Accepted |
| [0022](0022-in-house-localization.md) | In-house FTL parser, resolver and locale data | Accepted |
| [0023](0023-platform-layer.md) | In-house platform layer, starting with Win32 | Accepted |
| [0024](0024-cpu-renderer.md) | In-house CPU renderer with damage tracking and scroll blitting | Accepted |
| [0025](0025-text-segmentation.md) | In-house Unicode text segmentation | Accepted |
| [0026](0026-font-parsing-and-rasterization.md) | In-house OpenType parsing and glyph rasterization | Accepted |
| [0027](0027-text-shaping-and-font-fallback.md) | In-house text shaping and font fallback | Accepted; right-to-left text added by 0028 |
| [0028](0028-bidi-and-right-to-left-shaping.md) | In-house bidirectional text and right-to-left shaping | Accepted |
