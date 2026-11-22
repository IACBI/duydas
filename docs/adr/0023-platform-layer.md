# 0023. In-house platform layer, starting with Win32

- Status: Accepted
- Date: 2026-11-22

## Context

ADR 0021 step 2 replaces winit, which reaches us through Iced. Duydaş needs:

- one window;
- keyboard, text and pointer input;
- input methods (Japanese IME), with the candidate window next to the caret;
- clipboard text;
- DPI changes;
- a way to show CPU-rendered frames (step 3).

It must cost nothing while idle. ADR 0018 measured that repainting the whole window each frame fails the scrolling budget, so the presentation path must copy only damaged regions.

## Decision

- **`duydas-sys::window`** has one safe, platform-neutral API and a backend per OS. It provides:
  - `run(options, handler)`, a `Handler` trait and a `Context` for window operations;
  - `Event`, `Proxy` for cross-thread wakeups, and `Frame` plus `present(frame, damage)`.
  - All `unsafe` stays in the backend, under the ADR 0021 rules. Struct layouts are checked against the SDK sizes at compile time, for 64-bit and 32-bit Windows.
- **One window per thread.** `run` refuses a second one. Duydaş is a single-window app.
- **Blocking loop.** The Win32 backend waits in `GetMessageW`, with no timers and no polling.
- **Wakes from other threads.** `Proxy::wake` posts one message, and a burst of wakes merges into one. The owner thread invalidates the target under a lock in `WM_NCDESTROY`, so a proxy never posts to a window handle that Windows may have reused. Once the window is gone, `wake` returns false.
- **No re-entrancy.** Events are queued.
  - Win32 calls made by a handler can re-enter the window procedure (resizing sends `WM_SIZE` synchronously). Their events are delivered after the handler returns, never nested.
  - The window procedure also delivers events, so the app keeps drawing inside Windows' modal loops (dragging the border).
  - A DPI change holds delivery until the window has its new size, so no handler sees the new scale with the old size.
  - A hidden window's redraw requests are delivered only when no other message is waiting, so an animation cannot starve input, wakes or a quit.
- **Teardown** always runs on the owner thread. It works the same when the loop ends normally, through a foreign `WM_QUIT`, or because the message queue was full. It destroys the window if it still exists and leaves no stray `WM_QUIT` behind for the thread's next loop.
- **Panics** in the handler close the window, and `run` resumes them with their original payload. They never unwind into the OS. A second panic during cleanup cannot replace the first.
- **Keyboard:**
  - Keys are reported by layout-aware virtual key and by scan code.
  - Repeats that Windows merged under load are expanded again.
  - Key messages keep their default processing, so Alt+F4, Alt+Space and F10 work.
  - Typed text comes from `WM_CHAR`, with UTF-16 surrogate pairs assembled and control characters removed.
- **IME:**
  - While an IME is composing, the keys it consumes arrive as `Key::ImeProcess` (`VK_PROCESSKEY`), so Enter that confirms a conversion is never taken as "send".
  - The IME's own composition window is hidden, and the app draws the preedit inline. A caret-only update re-reads the composition, so it never erases the preedit.
  - Each candidate list the IME opens is placed with `CFS_EXCLUDE` at the area the app reports.
  - Committed text arrives once, as `ImeEvent::Commit`. If IMM32 cannot return the result string, default processing turns it into characters instead of losing it.
  - Disabling the IME mid-composition first confirms the composition, so the user's text is kept. The commit arrives after the current event, and the app routes it to the editor that was composing.
- **Pointer:**
  - Capture keeps drags working outside the window.
  - `Event::PointerCaptureLost` reports a drag that another window or the system menu ended.
  - Wheel deltas are in notches, and the toolkit decides how far a notch scrolls.
- **DPI:** the window is created under a per-monitor v2 context set on its own thread, so neither a manifest nor a host can change it. `WM_DPICHANGED` applies the suggested rectangle. Sizes are read from `GetClientRect`, not from 16-bit message fields.
- **Presentation:**
  - `SetDIBitsToDevice` copies each damaged rectangle of a top-down 32-bit frame. A test pins down GDI's bottom-up source coordinates by reading pixels back from an off-screen bitmap.
  - `WM_ERASEBKGND` is suppressed, because every frame covers the window.
  - Showing the window cloaks it, without activating it, until the current event is handled. The first frame is presented in the same event, so an unpainted window is never on screen. Activation (focus and input-method setup) takes about as long as everything before it, so it comes after the frame is up. Deferring it halved the time to the first frame, from 31 ms to 16 ms.
  - `WM_PAINT` asks for a full redraw. With desktop composition, Windows keeps every window's last frame, so it sends paints only after resizes, restores and our own invalidations, and most of those need a full frame anyway.

## Measurements

Windows 11, i5-11400H, release build of `examples/window.rs`: an empty window showing a full-window gradient, 1024×720 logical (1280×900 physical at 125%).

| | Value |
|---|---|
| `main` → first successful present, p50 / p95 of 21 runs (in-process clock) | 14 ms / 16 ms |
| Present cost, 1280×900 full frame vs one 48×48 damage rectangle | about 1.7 ms vs 30 µs |
| Private bytes / working set | 5.8 MB / 14.2 MB |
| CPU used over a 3 s idle sample | 0.0 ms |
| Events while idle | none after the focus and the paint that follow showing the window |
| Example binary | 0.2 MB |

For orientation only: the Phase 0 Iced shell reached its first redraw in 82 ms with tiny-skia and 531 ms with wgpu (10 MB and 127 MB private bytes). That shell drew text and layout and was measured from process spawn, so the numbers are not comparable. `cargo xtask perf` will measure the real app once it runs on this layer.

## Consequences

- **Verified by tests:** hidden windows are driven with the messages Windows sends:
  - characters (surrogates and repeat counts), keys, the IME process key;
  - pointer, wheel, capture loss;
  - a re-entrant resize, a DPI change, close;
  - cross-thread wakes, and a hidden animation that must starve neither those wakes nor a quit;
  - a foreign `WM_QUIT`, a handler panic, a nested `run`.

  A clipboard round trip with Turkish, Japanese and emoji text runs in CI.
- **Not verified automatically:** composition with a real Japanese IME, DPI changes between physical monitors, and teardown inside a live modal resize loop. These stay in the manual matrix in `docs/i18n-guide.md`.
- **Minimum Windows version:** Windows 10 1703, the first release with per-monitor v2.
- **Wayland/X11 and Cocoa backends come next.** Until then, `run` returns `Error::Unsupported` on those platforms, and the desktop app keeps using Iced everywhere.
- **Not covered yet:**
  - Text Services Framework features beyond IMM32 (reconversion);
  - touch and pen input;
  - drag and drop;
  - window icon and system theme changes.

  They will be added when the UI needs them.
