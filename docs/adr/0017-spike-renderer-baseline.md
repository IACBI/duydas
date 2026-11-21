# 0017. Spike: renderer, startup and memory baseline

- Status: Measured on Windows; macOS/Linux pending
- Date: 2026-11-21

## Context

Budgets: ≤ 300 ms to first frame, ≤ 90 MB idle. Iced 0.14 requests the high-performance GPU by default.

## Decision

Measured with `cargo xtask perf` (see `docs/performance.md`): wgpu high-performance 790 ms / 246 MB private; wgpu low-power 531 ms / 127 MB; tiny-skia CPU renderer 82 ms / 10 MB. **Direction:** make the CPU renderer the default and never request the discrete GPU, pending scroll frame-time results from ADR 0018. Iced 0.14 only selects the backend through the `ICED_BACKEND` env var, and setting env vars is `unsafe` in edition 2024, so the mechanism is either a build without the `wgpu` feature or an upstream change exposing the preference.

## Consequences

If CPU rendering meets frame-time budgets at 200% scale, drop wgpu from default builds (also shrinking the binary).
