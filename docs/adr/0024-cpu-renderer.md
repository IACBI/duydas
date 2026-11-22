# 0024. In-house CPU renderer with damage tracking and scroll blitting

- Status: Accepted
- Date: 2026-11-22

## Context

ADR 0021 step 3 replaces wgpu and tiny-skia. The measurements so far pull in two directions:

- ADR 0017: the CPU path reached the first frame about 10× sooner than wgpu and used an order of magnitude less memory (82 ms and 10 MB against 531 ms and 127 MB).
- ADR 0018: tiny-skia, redrawing the whole window every frame, missed the scrolling budget (p99 26.3 ms at 1×, 60.7 ms at 2×, with 51 stalls). wgpu passed.

ADR 0021 therefore required a CPU renderer that redraws only what changed and scrolls by moving pixels. The platform layer (ADR 0023) already presents only damaged rectangles.

The renderer must draw:

- rectangles, rounded rectangles and borders;
- glyph and icon masks;
- images (avatars);
- clips and scrolling content.

It may use no dependencies and no `unsafe`, and a frame drawn in pieces must be pixel-identical to the same frame drawn from scratch.

## Decision

- **A new crate, `duydas-render`, with no dependencies and no `unsafe`.**
  - Its `Canvas` holds opaque `0x00RRGGBB` pixels, the layout `duydas_sys::window::Frame` presents without conversion.
  - The app records each frame into a `Scene` (a display list), and the `Renderer` draws it.
- **Exact geometry.**
  - Commands are snapped to 1/64 px in `i64` fixed point when recorded. Snapping rounds half up on the exact value (far edges use an error-free sum), so geometry one pixel apart snaps one pixel apart.
  - Geometry beyond ±10⁹ px is dropped, and so is content under translations beyond 2⁴⁰ px. Inside those limits nothing saturates or overflows.
  - Coverage is computed in `f64`, relative to the pixel holding the shape's top-left corner. A shape moved by whole pixels therefore produces exactly the same pixels, moved. Scroll blitting depends on this.
- **Rasterizers.**
  - Straight edges get exact area coverage. Corners use the distance to the arc, capped by the rectangle's coverage. A border is the outer shape minus the inner one.
  - Masks are blended at whole pixels.
  - Images at their own size are copied texel for texel, through rounded corners if any; other sizes use bilinear filtering. `Image::resized` area-averages ahead of time with exact integer weights and bounded scratch memory.
  - Blending is premultiplied source-over on sRGB values, as browsers and design tools do it.
- **Damage tracking.**
  - Every 64 × 64 tile gets a 64-bit fingerprint of the commands that touch it, in order. Tiles whose fingerprint differs from the previous frame's are redrawn.
  - Differing tiles are merged in one pass: runs along each row, joined with identical runs in the row above. More than 32 rectangles collapse into their bounding box.
  - A fingerprint collision (about 2⁻⁶⁴ per tile and frame) would leave one tile stale until it changes again. That risk is documented on `Renderer`.
- **Scroll layers.**
  - A `ScrollLayer` records its content in content coordinates over an opaque background. Its fingerprints are taken in content coordinates, so scrolling leaves them unchanged.
  - When a layer keeps its id, viewport and background and moves by less than its height, the renderer:
    1. moves its rows;
    2. compares tiles against the previous commands, moved the same way;
    3. redraws the exposed strip;
    4. redraws anything painted above the layer (an overlay such as a "jump to present" bar), both where it was and where its pixels were moved.
  - Nested layers, translucent backgrounds and reused ids fall back to drawing as a clip.
  - A chat history should anchor content coordinates at its newest message, so prepending older messages changes nothing on screen and costs nothing.
- **Trusting the canvas.** A canvas has an identity and a revision. The revision advances on every render and every outside change. When either differs from what the renderer saw last, it redraws everything. A second renderer, a replaced canvas or a `clear` therefore costs time but never shows stale pixels.
- **Presenting.** `Renderer::damage()` lists the rectangles that changed, including moved areas, capped at 32. A failed present must be followed by `invalidate()`, so the next frame is complete.
- **No GPU path.** The measurements below do not call for one (ADR 0021).

## Measurements

Windows 11, i5-11400H, release build of `crates/duydas-render/examples/scroll`. The benchmark replays the ADR 0018 session with the same corpus and script:

- 14,000 Turkish, Japanese and English messages;
- 8 s scrolling at 2,400 px/s, then a jump to the oldest loaded message;
- 8 s more scrolling, with older pages of 50 prepended;
- a jump to the newest message, resizes to 600 and 1,300 px, then 4 s more scrolling.

The window is 900 × 700 logical pixels with a server rail, a channel list, a header, a composer, avatars, and an overlay bar while scrolled away from the newest message.

Headless, `cargo xtask render-bench`, five runs. A frame is building the scene plus rendering it; 1× is 900 × 700 px and 2× is 1800 × 1400 px:

| Mode | Scale | p50 | p99: median (range) | Worst frame | Stalls > 50 ms |
|---|---|---|---|---|---|
| Damage tracking | 1× | 0.23–0.46 ms | 1.5 ms (0.9–3.0) | 14.8 ms | 0 |
| Damage tracking | 2× | 0.9–2.1 ms | 2.5 ms (2.0–7.7) | 41.6 ms | 0 |
| Full redraw | 1× | 0.7–1.2 ms | 1.8 ms (1.5–3.2) | 5.7 ms | 0 |
| Full redraw | 2× | 3.6–4.4 ms | 7.2 ms (6.5–11.8) | 54 ms (a resize) | 1 run of 5 |

- With damage tracking, a scroll frame redraws 9.1% of the window's pixels on average and moves 52.8%.
- Jump frames take at most 3.2 ms.
- Resize frames take 13–47 ms, because the benchmark re-measures all 10,000 loaded messages at once. The real list measures lazily (ADR 0018).
- Earlier runs on the same machine under background load pushed the full-redraw 2× case to p99 37–55 ms, with frames up to 166 ms. Damage tracking stayed at or below 4.9 ms p99 in every run.

In a real window, presented through ADR 0023 without vsync. Frame times are intervals between redraws, as in ADR 0018. The 125% display gives a window of 1125 × 875 physical pixels:

| Mode | Layout scale | p50 | p99 | Worst | Render p99 | Present p99 |
|---|---|---|---|---|---|---|
| Damage tracking | 1× | 1.2 ms | 2.5 ms | 32.5 ms | 1.2 ms | 1.6 ms |
| Damage tracking | 2× | 0.7 ms | 1.9 ms | 17.1 ms | 0.8 ms | 1.3 ms |
| Full redraw | 1× | 2.1 ms | 3.9 ms | 5.3 ms | 2.5 ms | 1.9 ms |

None of the windowed runs had a stall or a failed present.

These numbers are not directly comparable with ADR 0018, whose spike ran Iced's whole pipeline: widget tree, layout, text shaping and glyph rasterization. Here the glyphs are synthetic masks of realistic size and coverage, cached as a glyph atlas would cache them. Real shaping and rasterization arrive with the text stack (step 4).

## Consequences

- **Verified by tests (37):**
  - exact coverage of fractional edges, corners, borders, masks and images;
  - whole-pixel translation invariance over random shapes;
  - 600 random frames (scrolls, jumps, content edits, inserts, overlays, viewport changes, outside canvas changes, resizes) checked pixel for pixel against full redraws, with every changed pixel inside the reported damage;
  - extreme offsets and viewports, deep nesting, a second renderer on the same canvas, and fragmented damage;
  - bounded-memory resizing of 1 × 16,384 images.
- **Reviewed:** three Codex review rounds. Their findings are fixed and covered by tests:
  - saturation at extreme coordinates;
  - `f32` precision and a rounding tie in edge snapping;
  - a 32-bit allocation panic in resizing;
  - gaps in the benchmark's checks.
- **Moving pixels still means presenting them.** A scroll frame re-presents the list's viewport, about 1.3–1.6 ms at p99 in the window runs. Scrolling the window's own pixels on the OS side could avoid that, and can be added to `duydas-sys` if a measurement asks for it.
- **Single-threaded.** Tiles are independent, so they could be drawn in parallel later.
- **Not covered yet:**
  - horizontal scrolling;
  - gradients, shadows, arbitrary paths and transforms;
  - color glyphs (emoji can be drawn as images until the text stack decides).
- **Next:** the text stack (step 4) draws through `Scene::draw_mask`, and the widget layer (step 5) owns scenes and scroll layers. Iced is removed when both reach parity.
