//! Rendering tests: rasterizer output, damage, and the invariant everything rests on — an
//! incrementally rendered frame is identical to the same frame drawn from scratch.

use std::sync::Arc;

use crate::{Canvas, Color, IRect, Image, Mask, Radii, Rect, Renderer, Scene, ScrollLayer};

const BG: Color = Color::BLACK;

fn draw(width: u32, height: u32, build: impl FnOnce(&mut Scene)) -> Canvas {
    let mut canvas = Canvas::new(width, height, BG).expect("canvas");
    let mut scene = Scene::new();
    build(&mut scene);
    Renderer::new().render(&mut scene, &mut canvas, BG);
    canvas
}

fn channel(p: u32) -> u32 {
    p & 0xFF
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }

    fn unit(&mut self) -> f32 {
        self.below(64) as f32 / 64.0
    }
}

#[test]
fn fractional_edges_get_area_coverage() {
    let c = draw(6, 3, |s| {
        s.fill(Rect::new(1.5, 1.0, 2.0, 1.0), Color::WHITE)
    });
    let row: Vec<u32> = (0..6).map(|x| c.pixel(x, 1).expect("pixel")).collect();
    assert_eq!(row, [0, 0x80_8080, 0xFF_FFFF, 0x80_8080, 0, 0]);
    assert_eq!(c.pixel(2, 0), Some(0));
    let quarter = draw(2, 2, |s| {
        s.fill(Rect::new(0.5, 0.5, 0.5, 0.5), Color::WHITE)
    });
    assert_eq!(quarter.pixel(0, 0), Some(0x40_4040));
}

#[test]
fn translucent_fills_blend_over_the_canvas() {
    let c = draw(2, 1, |s| {
        s.fill(Rect::new(0.0, 0.0, 2.0, 1.0), Color::hex(0x0000FF));
        s.fill(Rect::new(0.0, 0.0, 1.0, 1.0), Color::rgba(255, 0, 0, 128));
    });
    assert_eq!(c.pixel(0, 0), Some(0x80_007F));
    assert_eq!(c.pixel(1, 0), Some(0x00_00FF));
}

#[test]
fn circles_are_symmetric_and_have_the_right_area() {
    let c = draw(20, 20, |s| {
        s.fill_rounded(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Radii::all(10.0),
            Color::WHITE,
        );
    });
    let at = |x: u32, y: u32| channel(c.pixel(x, y).expect("pixel"));
    let mut total = 0;
    for y in 0..20 {
        for x in 0..20 {
            let v = at(x, y);
            assert_eq!(v, at(19 - x, y), "({x}, {y})");
            assert_eq!(v, at(x, 19 - y), "({x}, {y})");
            assert_eq!(v, at(y, x), "({x}, {y})");
            total += v;
        }
    }
    let area = f64::from(total) / 255.0;
    let exact = std::f64::consts::PI * 100.0;
    assert!((area - exact).abs() < exact * 0.01, "{area} vs {exact}");
    assert_eq!(at(10, 10), 255);
    assert_eq!(at(0, 0), 0);
}

#[test]
fn oversized_radii_are_clamped() {
    let huge = draw(30, 10, |s| {
        s.fill_rounded(
            Rect::new(0.0, 0.0, 30.0, 10.0),
            Radii::all(1e9),
            Color::WHITE,
        );
    });
    let pill = draw(30, 10, |s| {
        s.fill_rounded(
            Rect::new(0.0, 0.0, 30.0, 10.0),
            Radii::all(5.0),
            Color::WHITE,
        );
    });
    assert_eq!(huge, pill);
    let bad = draw(8, 8, |s| {
        let radii = Radii {
            top_left: f32::NAN,
            top_right: -3.0,
            bottom_right: f32::INFINITY,
            bottom_left: 0.0,
        };
        s.fill_rounded(Rect::new(0.0, 0.0, 8.0, 8.0), radii, Color::WHITE);
    });
    assert!(
        bad.pixels().iter().all(|&p| p == 0xFF_FFFF),
        "invalid radii are square"
    );
}

#[test]
fn borders_cover_only_the_ring() {
    let c = draw(12, 12, |s| {
        s.stroke_rounded(
            Rect::new(1.0, 1.0, 10.0, 10.0),
            Radii::ZERO,
            2.0,
            Color::WHITE,
        );
    });
    for y in 0..12 {
        for x in 0..12 {
            let ring = (1..11).contains(&x)
                && (1..11).contains(&y)
                && !((3..9).contains(&x) && (3..9).contains(&y));
            let expected = if ring { 0xFF_FFFF } else { 0 };
            assert_eq!(c.pixel(x, y), Some(expected), "({x}, {y})");
        }
    }
    // A border wider than half the box fills it.
    let solid = draw(6, 6, |s| {
        s.stroke_rounded(
            Rect::new(0.0, 0.0, 6.0, 6.0),
            Radii::ZERO,
            4.0,
            Color::WHITE,
        );
    });
    assert!(solid.pixels().iter().all(|&p| p == 0xFF_FFFF));
}

#[test]
fn masks_blend_their_coverage() {
    let mask = Arc::new(Mask::new(3, 1, vec![0, 128, 255]).expect("mask"));
    let c = draw(5, 2, |s| {
        s.draw_mask(&mask, 1, 1, Color::hex(0xFF8000));
        s.draw_mask(&mask, -2, 0, Color::WHITE); // only its last column is on the canvas
        s.draw_mask(&mask, 4, 5, Color::WHITE); // entirely off the canvas
    });
    assert_eq!(c.pixel(0, 0), Some(0xFF_FFFF));
    assert_eq!(c.pixel(1, 1), Some(0));
    assert_eq!(c.pixel(2, 1), Some(0x80_4000));
    assert_eq!(c.pixel(3, 1), Some(0xFF_8000));
}

#[test]
fn images_copy_at_native_size_and_filter_when_scaled() {
    let rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
    ];
    let image = Arc::new(Image::from_rgba8(2, 2, &rgba).expect("image"));
    let c = draw(4, 4, |s| {
        s.draw_image(&image, Rect::new(1.0, 1.0, 2.0, 2.0), Radii::ZERO)
    });
    assert_eq!(c.pixel(1, 1), Some(0xFF_0000));
    assert_eq!(c.pixel(2, 1), Some(0x00_FF00));
    assert_eq!(c.pixel(1, 2), Some(0x00_00FF));
    assert_eq!(c.pixel(2, 2), Some(0x80_8080));
    assert_eq!(c.pixel(0, 0), Some(0));

    let flat = Arc::new(Image::from_premultiplied(1, 1, vec![0xFF33_6699]).expect("image"));
    let big = draw(7, 5, |s| {
        s.draw_image(&flat, Rect::new(0.0, 0.0, 7.0, 5.0), Radii::ZERO)
    });
    assert!(big.pixels().iter().all(|&p| p == 0x33_6699));
    let round = draw(10, 10, |s| {
        s.draw_image(&flat, Rect::new(0.0, 0.0, 10.0, 10.0), Radii::all(5.0));
    });
    assert_eq!(round.pixel(0, 0), Some(0), "corners are cut");
    assert_eq!(round.pixel(5, 5), Some(0x33_6699));
}

#[test]
fn shapes_moved_by_whole_pixels_render_identically() {
    let mut rng = Rng(0x1234_5678);
    let mask = Arc::new(Mask::new(4, 3, (0..12).map(|_| rng.byte()).collect()).expect("mask"));
    let pixels: Vec<u8> = (0..36).map(|_| rng.byte()).collect();
    let image = Arc::new(Image::from_rgba8(3, 3, &pixels).expect("image"));
    for _ in 0..40 {
        let (x, y) = (2.0 + rng.unit() * 3.0, 2.0 + rng.unit() * 3.0);
        let (w, h) = (3.0 + rng.unit() * 20.0, 3.0 + rng.unit() * 20.0);
        let radius = rng.unit() * 12.0;
        let (dx, dy) = (rng.below(30) as i32, rng.below(30) as i32);
        let build = |s: &mut Scene, ox: f32, oy: f32| {
            let color = Color::rgba(200, 100, 50, 190);
            s.fill_rounded(Rect::new(x + ox, y + oy, w, h), Radii::all(radius), color);
            let border = Rect::new(x + ox, y + oy, w, h);
            s.stroke_rounded(border, Radii::all(radius), 1.25, Color::WHITE);
            s.draw_image(
                &image,
                Rect::new(x + ox, y + oy, w * 0.5, h * 0.5),
                Radii::all(radius),
            );
            s.draw_mask(&mask, 3 + ox as i32, 3 + oy as i32, Color::hex(0x00FF00));
        };
        let a = draw(64, 64, |s| build(s, 0.0, 0.0));
        let b = draw(64, 64, |s| build(s, dx as f32, dy as f32));
        for py in 0..30 {
            for px in 0..30 {
                let moved = b.pixel(px + dx as u32, py + dy as u32);
                assert_eq!(a.pixel(px, py), moved, "({px}, {py}) moved by ({dx}, {dy})");
            }
        }
    }
}

#[derive(Clone)]
struct Row {
    height: i32,
    color: Color,
    radius: f32,
    frac: f32,
    mask: usize,
    image: bool,
    border: bool,
    clipped: bool,
}

#[derive(Clone)]
struct World {
    rows: Vec<Row>,
    offset: i32,
    overlay: bool,
    viewport: IRect,
}

struct Assets {
    masks: Vec<Arc<Mask>>,
    image: Arc<Image>,
}

fn random_row(rng: &mut Rng) -> Row {
    Row {
        height: 6 + rng.below(30) as i32,
        color: Color::rgba(rng.byte(), rng.byte(), rng.byte(), 128 + rng.byte() / 2),
        radius: rng.unit() * 8.0,
        frac: rng.unit(),
        mask: rng.below(3) as usize,
        image: rng.below(3) == 0,
        border: rng.below(3) == 0,
        clipped: rng.below(5) == 0,
    }
}

/// A chat-like frame: a static panel, a scrolling list of rows and an overlay on top of it.
fn build(world: &World, assets: &Assets, scene: &mut Scene) {
    scene.fill(Rect::new(0.0, 0.0, 200.0, 140.0), Color::hex(0x2B2D31));
    scene.fill_rounded(
        Rect::new(2.25, 3.5, 30.0, 100.4),
        Radii::all(4.0),
        Color::hex(0x5865F2),
    );
    scene.push_scroll_layer(ScrollLayer {
        id: 9,
        viewport: world.viewport,
        offset: world.offset,
        background: Color::hex(0x101418),
    });
    let mut y = 0;
    for row in &world.rows {
        let top = y as f32;
        let h = row.height as f32;
        let card = Rect::new(2.0 + row.frac, top + 1.3, 100.0, h - 2.6);
        if row.clipped {
            scene.push_clip(IRect::new(0, y, 60, y + row.height / 2));
        }
        scene.fill_rounded(card, Radii::all(row.radius), row.color);
        if row.border {
            scene.stroke_rounded(
                card,
                Radii::all(row.radius),
                1.5,
                Color::rgba(255, 255, 255, 180),
            );
        }
        scene.draw_mask(&assets.masks[row.mask], 10, y + 2, Color::hex(0xDBDEE1));
        if row.image {
            let avatar = Rect::new(80.5, top + 1.0, 12.0, 12.0);
            scene.draw_image(&assets.image, avatar, Radii::all(6.0));
        }
        if row.clipped {
            scene.pop_clip();
        }
        y += row.height;
    }
    scene.pop_scroll_layer();
    if world.overlay {
        let pill = Rect::new(60.5, 50.25, 50.0, 14.0);
        scene.fill_rounded(pill, Radii::all(7.0), Color::rgba(200, 40, 40, 200));
        scene.draw_mask(&assets.masks[0], 70, 52, Color::WHITE);
    }
}

#[test]
fn incremental_frames_match_full_redraws() {
    let mut rng = Rng(0x00C0_FFEE_D00D);
    let masks = (0..3)
        .map(|_| {
            let data = (0..63).map(|_| rng.byte()).collect();
            Arc::new(Mask::new(7, 9, data).expect("mask"))
        })
        .collect();
    let pixels: Vec<u8> = (0..100).map(|_| rng.byte()).collect();
    let assets = Assets {
        masks,
        image: Arc::new(Image::from_rgba8(5, 5, &pixels).expect("image")),
    };
    let mut world = World {
        rows: (0..60).map(|_| random_row(&mut rng)).collect(),
        offset: 200,
        overlay: true,
        viewport: IRect::new(40, 8, 150, 110),
    };
    let mut size = (200, 140);
    let mut canvas = Canvas::new(size.0, size.1, BG).expect("canvas");
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    let (mut blitted, mut partial) = (0, 0);
    for frame in 0..600 {
        match rng.below(20) {
            0..=8 => world.offset += rng.below(31) as i32 - 15,
            9 => world.offset += rng.below(400) as i32 - 200,
            10 => {
                let i = rng.below(world.rows.len() as u64) as usize;
                world.rows[i].color = Color::rgba(rng.byte(), rng.byte(), rng.byte(), 255);
            }
            11 => world.overlay = !world.overlay,
            12 => {
                let i = rng.below(world.rows.len() as u64) as usize;
                world.rows.insert(i, random_row(&mut rng));
            }
            13 => canvas.clear(IRect::new(0, 0, 50, 50), Color::WHITE),
            14 if rng.below(4) == 0 => {
                size = (150 + rng.below(100) as u32, 100 + rng.below(80) as u32);
                canvas = Canvas::new(size.0, size.1, BG).expect("canvas");
            }
            15 => world.viewport.y1 = 90 + rng.below(40) as i32,
            _ => {}
        }
        let before = canvas.pixels().to_vec();
        let resized = before.len() != (size.0 * size.1) as usize;
        build(&world, &assets, &mut scene);
        let stats = renderer.render(&mut scene, &mut canvas, BG);
        assert!(scene.is_empty(), "render takes the commands");
        let expected = draw(size.0, size.1, |s| build(&world, &assets, s));
        assert!(
            canvas == expected,
            "frame {frame} differs from a full redraw ({stats:?})"
        );
        if stats.scrolled > 0 {
            blitted += 1;
        }
        if !stats.full {
            partial += 1;
        }
        if !resized {
            let damage = renderer.damage();
            for (i, (old, new)) in before.iter().zip(canvas.pixels()).enumerate() {
                if old != new {
                    let (x, y) = ((i % size.0 as usize) as i32, (i / size.0 as usize) as i32);
                    let inside = damage
                        .iter()
                        .any(|r| r.contains(&IRect::new(x, y, x + 1, y + 1)));
                    assert!(
                        inside,
                        "frame {frame}: ({x}, {y}) changed outside the damage"
                    );
                }
            }
        }
    }
    assert!(blitted > 150, "only {blitted} frames scrolled by blitting");
    assert!(partial > 400, "only {partial} frames were partial");
}

#[test]
fn unchanged_frames_draw_nothing_and_small_changes_stay_small() {
    let frame = |s: &mut Scene, color: Color| {
        s.fill(Rect::new(0.0, 0.0, 300.0, 200.0), Color::hex(0x313338));
        for i in 0..10 {
            let y = i as f32 * 20.0;
            s.fill_rounded(
                Rect::new(10.0, y + 2.0, 200.0, 16.0),
                Radii::all(4.0),
                Color::hex(0x404249),
            );
        }
        s.fill(Rect::new(200.0, 150.0, 10.0, 10.0), color);
    };
    let mut canvas = Canvas::new(300, 200, BG).expect("canvas");
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    frame(&mut scene, Color::WHITE);
    assert!(renderer.render(&mut scene, &mut canvas, BG).full);
    frame(&mut scene, Color::WHITE);
    let stats = renderer.render(&mut scene, &mut canvas, BG);
    assert_eq!((stats.full, stats.repainted, stats.draws), (false, 0, 0));
    assert!(renderer.damage().is_empty());
    frame(&mut scene, Color::hex(0xFF0000));
    let stats = renderer.render(&mut scene, &mut canvas, BG);
    assert_eq!(renderer.damage(), [IRect::new(192, 128, 256, 192)]);
    assert_eq!(stats.repainted, 64 * 64);
    // A different background redraws everything.
    frame(&mut scene, Color::hex(0xFF0000));
    assert!(renderer.render(&mut scene, &mut canvas, Color::WHITE).full);
    // So does invalidating.
    renderer.invalidate();
    frame(&mut scene, Color::hex(0xFF0000));
    assert!(renderer.render(&mut scene, &mut canvas, Color::WHITE).full);
}

#[test]
fn scrolling_repaints_only_the_exposed_rows() {
    let frame = |s: &mut Scene, offset: i32| {
        s.push_scroll_layer(ScrollLayer {
            id: 1,
            viewport: IRect::new(0, 0, 256, 192),
            offset,
            background: Color::hex(0x1E1F22),
        });
        for i in -50..50 {
            let y = i as f32 * 30.0;
            s.fill_rounded(
                Rect::new(8.0, y + 3.5, 200.0, 24.0),
                Radii::all(5.0),
                Color::hex(0x5865F2),
            );
        }
        s.pop_scroll_layer();
    };
    let mut canvas = Canvas::new(256, 256, BG).expect("canvas");
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    frame(&mut scene, 0);
    renderer.render(&mut scene, &mut canvas, BG);
    for (offset, exposed) in [(-17, 17), (-17, 0), (40, 57), (39, 1)] {
        frame(&mut scene, offset);
        let stats = renderer.render(&mut scene, &mut canvas, BG);
        assert_eq!(stats.repainted, 256 * exposed, "offset {offset}");
        if exposed > 0 {
            assert_eq!(stats.scrolled, 256 * (192 - exposed));
            assert!(renderer.damage().contains(&IRect::new(0, 0, 256, 192)));
        }
    }
    // A jump further than the viewport is tall redraws the viewport's tiles instead.
    frame(&mut scene, 5000);
    let stats = renderer.render(&mut scene, &mut canvas, BG);
    assert_eq!((stats.scrolled, stats.repainted), (0, 256 * 192));
}

/// Renders each frame incrementally and checks it against a full redraw.
fn check_frames(width: u32, height: u32, frames: &[&dyn Fn(&mut Scene)]) {
    let mut canvas = Canvas::new(width, height, BG).expect("canvas");
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    for (i, frame) in frames.iter().enumerate() {
        frame(&mut scene);
        renderer.render(&mut scene, &mut canvas, BG);
        let expected = draw(width, height, frame);
        assert!(canvas == expected, "frame {i} differs from a full redraw");
    }
}

fn layer(viewport: IRect, offset: i32, background: Color) -> ScrollLayer {
    ScrollLayer {
        id: 1,
        viewport,
        offset,
        background,
    }
}

#[test]
fn extreme_offsets_scroll_exactly() {
    let frame = |offset: i32| {
        move |s: &mut Scene| {
            s.push_scroll_layer(layer(IRect::new(0, 0, 4, 8), offset, Color::BLACK));
            s.fill(Rect::new(0.0, -33_554_432.0, 4.0, 4.0), Color::WHITE);
            s.pop_scroll_layer();
        }
    };
    check_frames(4, 8, &[&frame(-33_554_432), &frame(-33_554_433)]);
    let far = |offset: i32| {
        move |s: &mut Scene| {
            s.push_scroll_layer(layer(IRect::new(0, 0, 4, 8), offset, Color::BLACK));
            s.draw_mask(
                &Arc::new(Mask::new(1, 1, vec![255]).expect("mask")),
                0,
                i32::MIN,
                Color::WHITE,
            );
            s.pop_scroll_layer();
        }
    };
    check_frames(4, 8, &[&far(i32::MIN), &far(i32::MIN + 1), &far(i32::MAX)]);
}

#[test]
fn wide_layer_backgrounds_cover_what_is_behind_them() {
    let frame = |offset: i32| {
        move |s: &mut Scene| {
            s.fill(Rect::new(0.0, 3.0, 1.0, 1.0), Color::WHITE);
            s.push_scroll_layer(layer(
                IRect::new(-33_554_432, 0, 1, 8),
                offset,
                Color::BLACK,
            ));
            s.pop_scroll_layer();
        }
    };
    check_frames(4, 8, &[&frame(0), &frame(-1)]);
    let translucent = draw(4, 2, |s| {
        let white = Color::rgba(255, 255, 255, 128);
        s.push_scroll_layer(layer(IRect::new(i32::MIN, 0, 1, 2), 0, white));
        s.pop_scroll_layer();
    });
    assert_eq!(translucent.pixel(0, 0), Some(0x80_8080));
    assert_eq!(translucent.pixel(1, 0), Some(0));
}

#[test]
fn square_corners_use_area_coverage() {
    let c = draw(2, 2, |s| {
        s.fill(Rect::new(0.625, 0.625, 1.0, 1.0), Color::WHITE)
    });
    assert_eq!(c.pixel(0, 0), Some(0x24_2424));
    // A small radius never covers more than the rectangle would.
    let round = draw(2, 2, |s| {
        s.fill_rounded(
            Rect::new(0.625, 0.625, 1.0, 1.0),
            Radii::all(0.25),
            Color::WHITE,
        );
    });
    assert!(channel(round.pixel(0, 0).expect("pixel")) <= 0x24);
}

#[test]
fn a_second_renderer_on_the_same_canvas_forces_a_full_redraw() {
    let white = |s: &mut Scene| s.fill(Rect::new(0.0, 0.0, 8.0, 8.0), Color::WHITE);
    let red = |s: &mut Scene| s.fill(Rect::new(0.0, 0.0, 8.0, 8.0), Color::hex(0xFF0000));
    let mut canvas = Canvas::new(8, 8, BG).expect("canvas");
    let (mut a, mut b) = (Renderer::new(), Renderer::new());
    let mut scene = Scene::new();
    white(&mut scene);
    a.render(&mut scene, &mut canvas, BG);
    red(&mut scene);
    b.render(&mut scene, &mut canvas, BG);
    white(&mut scene);
    assert!(a.render(&mut scene, &mut canvas, BG).full);
    assert_eq!(canvas.pixel(3, 3), Some(0xFF_FFFF));
}

#[test]
fn fragmented_damage_collapses_to_few_rectangles() {
    let frame = |s: &mut Scene, lit: bool| {
        for ty in 0..10 {
            for tx in 0..10 {
                if (tx + ty) % 2 == 0 {
                    let color = if lit {
                        Color::WHITE
                    } else {
                        Color::hex(0x404040)
                    };
                    s.fill(
                        Rect::new(tx as f32 * 64.0 + 8.0, ty as f32 * 64.0 + 8.0, 4.0, 4.0),
                        color,
                    );
                }
            }
        }
    };
    let mut canvas = Canvas::new(640, 640, BG).expect("canvas");
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    frame(&mut scene, false);
    renderer.render(&mut scene, &mut canvas, BG);
    frame(&mut scene, true);
    renderer.render(&mut scene, &mut canvas, BG);
    assert!(
        renderer.damage().len() <= 32,
        "{} rectangles",
        renderer.damage().len()
    );
    let expected = draw(640, 640, |s| frame(s, true));
    assert!(canvas == expected);
}

#[test]
fn edges_of_very_large_shapes_keep_their_coverage() {
    let c = draw(3, 4, |s| {
        s.fill(
            Rect::new(0.25, -16_777_216.0, 2.0, 16_777_220.0),
            Color::WHITE,
        )
    });
    for y in 0..4 {
        assert_eq!(c.pixel(0, y).map(channel), Some(191), "row {y}");
        assert_eq!(c.pixel(1, y).map(channel), Some(255), "row {y}");
    }
}
