# 0018. Spike: virtualized message list

- Status: Measured on Windows: passes on wgpu, fails on tiny-skia; macOS/Linux pending
- Date: 2026-11-22

## Context

Iced has no variable-height virtual list. Chat needs bottom anchoring and a stable scroll position when older history is prepended. Pass criterion: scroll frame time p99 ≤ 16.7 ms with no stall > 50 ms, at 10k messages including prepends, resize and 200% scale.

## Decision

Prototype in `duydas-ui::virtual_list` (reusable) plus a scripted benchmark in `spikes/virtual-list`, run with `cargo xtask vlist`:

- `HeightIndex`: row heights in a Fenwick tree, so offset ↔ row lookups are O(log n). Rows start at an estimate and take their measured height from Iced's `sensor` when they come within the overscan of the viewport.
- Only the rows inside the viewport plus 400 px overscan are built (~21–23 rows out of 10,000+). Two spacers stand in for the rest.
- The list is a bottom-anchored `scrollable`, so offsets count from the end and content added above the viewport does not move it.
- Script: 1.5 s idle, 8 s scrolling up at 2,400 px/s through 10k messages, jump to the oldest loaded message, 8 s more scrolling with 50-message history pages prepended on demand, jump to the newest, two window resizes (600 and 1,300 px wide), then 4 s more scrolling. Synthetic messages mix Turkish, Japanese and English, with 5% multi-paragraph.
- Prepend stability is checked on the row at the top of the viewport: its distance from the viewport edge must be the same before and after the prepend.

### Results (2026-11-22)

Machine: Windows 11, i5-11400H, wgpu on the Intel UHD (low-power adapter), 144 Hz display, 900×700 logical window. "Cost" runs disable vsync (`ICED_PRESENT_MODE=no_vsync`), so intervals show how long a frame takes instead of the display cadence. tiny-skia presents without vsync, so its intervals are already cost.

| Renderer | Scale | Scroll p50 | Scroll p99 | Max | Stalls > 50 ms | Prepend frame max |
|---|---|---|---|---|---|---|
| wgpu, vsync | 1× | 8.7 ms | 15.1 ms | 33.7 ms | 0 | 13.1 ms |
| wgpu, vsync | 2× | 9.0 ms | 14.7 ms | 16.9 ms | 0 | 17.6 ms |
| wgpu, no vsync (cost) | 1× | 1.2 ms | **3.3 ms** | 27.5 ms | 0 | 3.1 ms |
| wgpu, no vsync (cost) | 2× | 1.6 ms | **3.6 ms** | 10.0 ms | 0 | 4.7 ms |
| tiny-skia | 1× | 14.2 ms | **26.3 ms** | 39.9 ms | 0 | 25.3 ms |
| tiny-skia | 2× | 32.6 ms | **60.7 ms** | 69.2 ms | 51 | 49.4 ms |

Building the view takes ≤ 68 µs at p99 in every configuration. Single slow frames (up to 34 ms) occur on wgpu but never reach the 50 ms stall threshold. All 6 prepends per run kept the top row exactly in place (0.0 px shift). The check uses the height model, so it catches scroll-offset changes, not shifts caused by re-measuring rows inside the viewport.

**Verdict: pass on wgpu with about 4× headroom; fail on tiny-skia.** A full CPU repaint of a scrolling window costs 14–33 ms at p50, so the CPU renderer cannot be the default (ADR 0017 is revised accordingly).

### Findings that shape the Phase 1 widget

1. **`snap_to` offsets are relative to the anchor.** On a bottom-anchored list, `snap_to_end` goes to the *top* (relative 1.0 from the bottom), and `snap_to` leaves a relative offset, so after "jump to oldest" every prepend kept the view glued to the new top. Without the fix, 72 of 80 prepends moved the view (by up to 3,456 px) and history loaded page after page. `virtual_list::keep_position` converts to an absolute offset with every prepend. Wheel and scrollbar-drag input already store absolute offsets (verified in `iced_widget` 0.14 `scrollable.rs`).
2. **`keyed_column` assumes one widget type** (its diff skips the type check, and a mismatch panics with "Downcast on stateless state"), and it only matches one contiguous inserted or removed range by key. When the window slides, row state is diffed **by position**. Rows must not hold state that matters (focus, inline editing) until the Phase 1 widget reconciles children by message ID.
3. **Appending while scrolled up moves the reader**, because the bottom-relative offset stays fixed while content grows below. The same applies to late measurements below the viewport. Phase 1 needs an anchor-row model: keep a visible message ID plus its offset across every mutation, and keep a separate "follow live edge" mode.
4. **Every built row must be measured, not only visible ones.** If only visible rows are measured and the estimates (or heights left over from a different width) are wrong, every built row can sit outside the viewport and the window never corrects itself. Iced measures sensor anticipation from row corners, so a finite distance misses rows taller than the viewport; the prototype uses an effectively unlimited distance.
5. **The viewport size comes only from `on_scroll`**, which Iced suppresses when the content fits. The Phase 1 widget must get the viewport size from layout.
6. **A prepend rebuilds the tree, which is O(n).** That is microseconds at 10k rows, but the total cost grows quadratically with loaded history. If profiling shows it, switch to page-local indexes.

### Not measured

- macOS (Metal) and Linux (Vulkan/X11/Wayland).
- Very large windows (4K at 200%).
- Real fonts and emoji fallback chains (the spike uses the default font).
- The cost of shaping long messages on first show.
- Resize frame time: Iced only redraws on change, so redraw intervals across a resize are not frame times. The spike checks that both resizes happened and that scrolling afterwards stays within budget.

## Consequences

- Virtualization is feasible in Iced 0.14 without forking. Phase 1 turns the prototype into a proper widget that addresses findings 2–5.
- The renderer decision moves to wgpu (ADR 0017).
- `cargo xtask vlist` stays as the regression benchmark. Once CI runner variance is known, it can gate the cost runs.
