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
2. The CPU renderer is about 10× faster to first frame and an order of magnitude smaller, **but it fails the scrolling budget** (next section). The default is therefore wgpu on the low-power adapter, with DX12 on Windows (83 MB private in a same-session comparison, against 126 MB for wgpu's default backend). Startup remains open; see ADR 0017.

Reproduce:

```sh
cargo xtask perf --runs 5
ICED_BACKEND=tiny-skia cargo xtask perf --runs 5
WGPU_POWER_PREF=low cargo xtask perf --runs 5
WGPU_POWER_PREF=low WGPU_BACKEND=dx12 cargo xtask perf --runs 5
```

## Baseline: scrolling 10k messages (2026-11-22)

`cargo xtask vlist` runs `spikes/virtual-list` (a scripted scroll through 10,000 messages, with history prepends, jumps and two resizes) on each renderer at 1× and 2× scale. Interval statistics come from redraw timestamps. With vsync on they show cadence (144 Hz here), so the runner also runs wgpu with `ICED_PRESENT_MODE=no_vsync` to show cost.

| Renderer | Scale | p99 | Max | Stalls > 50 ms |
|---|---|---|---|---|
| wgpu low-power, vsync | 1× / 2× | 15.1 / 14.7 ms | 33.7 / 16.9 ms | 0 / 0 |
| wgpu low-power, no vsync (cost) | 1× / 2× | 3.3 / 3.6 ms | 27.5 / 10.0 ms | 0 / 0 |
| tiny-skia | 1× / 2× | 26.3 / 60.7 ms | 39.9 / 69.2 ms | 0 / 51 |

Details and the design findings behind these numbers are in [ADR 0018](adr/0018-spike-virtual-list.md).

## Baseline: in-house window layer (2026-11-22)

`examples/window.rs` in duydas-sys, release build, same machine as above. The window shows a full-window gradient and no text, at 1280×900 physical.

| | Command | Value |
|---|---|---|
| `main` → first successful present, p50 / p95 of 21 runs | `-- --first-frame` | 14 ms / 16 ms |
| One present: full frame / one 48×48 damage rectangle | `-- --present-bench` | about 1.7 ms / 30 µs |
| Private bytes / working set | `-- --idle 6` | 5.8 MB / 14.2 MB |
| CPU used over a 3 s idle sample | `-- --idle 6` | 0.0 ms |
| Events while idle | `-- --idle 6` | none after the focus and paint that follow showing |

Run with `cargo run --release -p duydas-sys --example window -- <flag>`. The in-process clock starts at `main`, while the Iced baseline above starts at process spawn, so the two are not comparable. Design and caveats: [ADR 0023](adr/0023-platform-layer.md).

## Baseline: in-house CPU renderer (2026-11-22)

`cargo xtask render-bench` runs `crates/duydas-render/examples/scroll`, which replays the ADR 0018 session (same corpus and script) through `duydas-render`. It checks the damage-tracking runs against the scrolling budget and fails if either misses it; full redraws are printed for comparison. Same machine as above, five runs, release build, headless. A frame is building the scene plus rendering it.

| Mode | Scale | p99: median (range) | Worst frame | Stalls &gt; 50 ms |
|---|---|---|---|---|
| Damage tracking | 1× / 2× | 1.5 ms (0.9–3.0) / 2.5 ms (2.0–7.7) | 14.8 / 41.6 ms | 0 / 0 |
| Full redraw | 1× / 2× | 1.8 ms (1.5–3.2) / 7.2 ms (6.5–11.8) | 5.7 / 54 ms | 0 / 1 run of 5 |

In a real window (`--window`, presented through `duydas-sys`, 1125 × 875 physical pixels at 125%), damage tracking measured p99 2.5 ms at 1× and 1.9 ms at 2× layout scale, presents included, with no stalls.

Text uses synthetic glyph masks, so shaping and glyph rasterization are not measured yet; the numbers do not compare one-to-one with the Iced runs above. Details: [ADR 0024](adr/0024-cpu-renderer.md).
