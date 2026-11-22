# 0021. Write the stack in-house; libraries only for Opus and cryptography

- Status: Accepted
- Date: 2026-11-22
- Supersedes: ADR 0007; parts of ADR 0001, 0002, 0005 and 0006 as each replacement lands

## Context

The maintainer wants to own the whole codebase and not depend on third-party crates. The two exceptions the maintainer chose are the areas where a home-grown version would be a liability, not an asset:

- **the Opus codec**, a standardized codec with decades of tuning;
- **cryptography**: TLS, and DTLS-SRTP for voice.

Hand-written crypto would need an independent audit before anyone could trust it.

## Decision

- **New code adds no crates.** Existing dependencies are replaced one area at a time. Each replacement keeps the existing tests passing, so those tests act as its specification.
- **Allowed libraries:**
  - Opus: `libopus` through a thin binding;
  - cryptography: TLS, DTLS-SRTP, and the primitives they need, from audited libraries.

  Each one needs an entry in `deny.toml` and a line in this ADR.
- **Operating-system access goes through `duydas-sys`.** It is the only crate allowed `unsafe`. Every block carries a `SAFETY:` comment and does one unsafe operation, enforced by `clippy::undocumented_unsafe_blocks` and `clippy::multiple_unsafe_ops_per_block`. Each unsafe block is wrapped in a safe function. The rest of the workspace keeps `unsafe_code = "forbid"`.
- **Tools are not dependencies.** cargo-deny, typos, nextest and the Codex CLI run in CI or locally and never ship.

### Order of replacement

| Step | Area | Replaces | State |
|---|---|---|---|
| 1 | Localization, permissions, system language | fluent-bundle, fluent-syntax, fluent-langneg, unic-langid, ICU4X, sys-locale, bitflags | **Done** (ADR 0022) |
| 2 | Platform layer: windows, input, clipboard, IME. Win32 first, then Wayland/X11 and Cocoa | winit (via Iced) | **Win32 done** (ADR 0023); Wayland/X11 and Cocoa next |
| 3 | Renderer: CPU rasterizer with damage tracking and scroll blitting; a GPU path only if measurements require it | wgpu, tiny-skia | |
| 4 | Text: OpenType parsing, glyph rasterization, font fallback, UAX #14 line breaking (Japanese line-breaking rules included) | cosmic-text | |
| 5 | Widgets, layout and the Elm-style update loop; port of the virtual list (ADR 0018) | iced | |
| 6 | Accessibility: UI Automation, NSAccessibility, AT-SPI | (missing in Iced) | |
| 7 | Audio I/O (WASAPI, CoreAudio, ALSA/PipeWire), echo cancellation, noise suppression, jitter buffer, RTP/RTCP, ICE/STUN | cpal and the str0m media stack (ADR 0006) | |
| 8 | Event loop (IOCP, kqueue, epoll), HTTP/1.1, WebSocket, server storage | tokio, axum, sqlx (ADR 0005) | |

Iced stays in `duydas-ui` and `duydas-desktop` until steps 2–5 reach feature parity with today's shell. The switch happens in one change, behind the same `App` update/view structure.

## Consequences

- The planned phase timeline no longer holds. Steps 2–6 are a GUI toolkit, the largest piece of work in the project, and take months.
- Every replaced area needs its own tests and fuzzing. The FTL parser gets a fuzz target alongside the other parsers in Phase 3.
- ADR 0018 showed that a CPU renderer which repaints the whole window fails the scrolling budget. Step 3 must therefore repaint only damaged regions and blit on scroll from the start.
- Security review effort shifts from vetting dependencies to reviewing our own code. Codex reviews (and later an external review) cover each step.
