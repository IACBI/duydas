# Performance budgets

Budgets are **provisional** until every Phase 0 baseline is in. After that they are frozen, and CI gates on regressions once runner variance has been measured.

## Measurement markers

- `launch`: process spawn, measured by `cargo xtask perf` from outside the app.
- `first_redraw`: the app prints `duydas-perf first_redraw_ms=…` on its first `RedrawRequested` when `DUYDAS_PERF=1` is set. Iced emits that event just before presenting, so it marks the first frame attempt; confirming the actual present needs renderer instrumentation (planned with the Phase 1 perf gate).
- `interactive`: cached channel visible and composer focusable (Phase 1, once there is real content).
- Memory: Windows private bytes and working set, macOS `phys_footprint` (via `footprint`), Linux PSS. GPU driver allocations count toward Windows private bytes, so renderer choice dominates that number (see baseline below).

## Budgets

| Metric | Target |
|---|---|
| Warm launch → first_redraw | ≤ 300 ms p50 / 600 ms p95 |
| Warm launch → interactive | ≤ 500 ms p50 |
| Idle memory (1 community, 1 channel) | ≤ 90 MB |
| Heavy (20 communities, 50k cached messages, in voice) | ≤ 200 MB |
| Idle, focused, no activity | 0 redraws; < 1 wakeup/s outside heartbeat |
| Minimized with gateway connected | < 0.2% of one core over 10 min |
| Keystroke → present | ≤ 2 frames p95 at 60 Hz |
| Scrolling 10k messages incl. history prepend, 200% scale | frame time p99 ≤ 16.7 ms, no stall > 50 ms |
| Message commit → peer receipt (same region) | ≤ 150 ms p95 |
| Voice mouth-to-ear (physical loopback, 40 ms RTT, 1% loss) | ≤ 150 ms p95 |
| Voice CPU (5 incoming speakers, noise suppression on) | < 4% of one core |
| Package size | Windows ≤ 30 MB, macOS ≤ 35 MB |
| Idle network (both directions, heartbeat included) | ≤ 1 KB/min |
| Server, 2 vCPU / 4 GB | 10k idle sockets; 1k-member channel at 5 msg/s with p99 commit→receipt < 50 ms |

## Baseline: Phase 0 shell (2026-11-21)

Machine: Windows 11 Pro, Intel Core i5-11400H, 32 GB RAM, NVIDIA RTX 3050 Ti Laptop + Intel UHD. Release build (thin LTO, `panic = "abort"`), shell with sidebar, text and composer, 1100×720 window. Warm runs; the first cold run is excluded from p50.

| Renderer | spawn → first redraw (p50) | Private bytes | Working set |
|---|---|---|---|
| wgpu, `HighPerformance` (Iced default → discrete GPU) | 790 ms | 246 MB | 219 MB |
| wgpu, `WGPU_POWER_PREF=low` (integrated GPU) | 531 ms | 127 MB | 172 MB |
| tiny-skia (CPU), `ICED_BACKEND=tiny-skia` | **82 ms** | **10 MB** | **24 MB** |

Binary size: 9.0 MB (both renderers compiled in).

Findings, recorded in [ADR 0017](adr/0017-spike-renderer-baseline.md):

1. The wgpu path misses the startup and memory budgets on this machine, and Iced 0.14 asks for the **high-performance** GPU by default, which wakes the discrete GPU on laptops. A chat app should never do that.
2. The CPU renderer is about 10× faster to first frame and an order of magnitude smaller. It still needs frame-time measurements for scrolling at large window sizes and 200% scale (spike c) before it becomes the default.

Reproduce:

```sh
cargo xtask perf --runs 5
ICED_BACKEND=tiny-skia cargo xtask perf --runs 5
WGPU_POWER_PREF=low cargo xtask perf --runs 5
```
