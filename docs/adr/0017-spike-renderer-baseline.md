# 0017. Spike: renderer, startup and memory baseline

- Status: Revised after ADR 0018: measured on Windows; macOS/Linux pending
- Date: 2026-11-21 (revised 2026-11-22)

## Context

Budgets: ≤ 300 ms to first frame, ≤ 90 MB idle (excluding GPU driver allocations), and scroll frame time p99 ≤ 16.7 ms. Iced 0.14 requests the high-performance GPU by default, and on Windows wgpu picks its backend itself. Iced exposes both choices only through environment variables (`WGPU_POWER_PREF`, `WGPU_BACKEND`, `ICED_BACKEND`).

## Decision

### Measurements (`cargo xtask perf`, Phase 0 shell)

First session, 2026-11-21:

| Renderer | First redraw (p50) | Private memory |
|---|---|---|
| wgpu, high performance | 790 ms | 246 MB |
| wgpu, low power | 531 ms | 127 MB |
| tiny-skia (CPU) | 82 ms | 10 MB |

Second session, 2026-11-22, same machine. Everything ran about 2× slower than in the first session, so compare the numbers within this table only:

| Renderer | First redraw (p50, 5 runs) | Private memory | Working set |
|---|---|---|---|
| wgpu, low power, default backend | 999 ms | 126 MB | 172 MB |
| wgpu, low power, `WGPU_BACKEND=dx12` | 963 ms | **83 MB** | 89 MB |
| tiny-skia | 202 ms | 10 MB | 24 MB |

ADR 0018 then showed that tiny-skia **fails** the scrolling budget: p99 26 ms at 1×, and 61 ms with 51 stalls at 2×. wgpu passes with a frame cost of about 3.5 ms at p99.

### Direction

- Default to **wgpu on the low-power adapter**. Never request the discrete GPU.
- On Windows, use **DX12**, which is 43 MB lighter than the default backend here and inside the memory budget.
- Keep tiny-skia only as the fallback when no adapter is available. It still fails scrolling, but a slow app beats no app.

### Open problems for the go/no-go

1. **Startup.** wgpu adapter and device setup costs 450–800 ms more than the CPU path, so first redraw misses the 300 ms budget on this machine. The options are: revise the budget for the GPU path, which the plan allows once with data; or show a lightweight first frame while the device initializes, which Iced 0.14 cannot do and would need upstream work.
2. **Selecting adapter and backend without environment variables.** Setting them from inside the process is `unsafe` in edition 2024, and the workspace forbids `unsafe`. The options:
   - an upstream Iced change that exposes both in its settings (preferred);
   - a single audited `set_var` as the first statement of `main`, which is sound because no other thread exists yet, but needs an ADR 0002 exception;
   - a launcher that re-executes itself with the variables set, at the cost of one extra process start.

## Consequences

- The CPU renderer is no longer the default candidate.
- Package size stays at about 9 MB with both renderers.
- macOS (Metal) and Linux (Vulkan, X11 and Wayland) need the same measurements before the budgets are frozen.
