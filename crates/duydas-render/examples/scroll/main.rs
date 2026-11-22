//! Scroll benchmark for the CPU renderer (ADR 0024), replaying the ADR 0018 session.
//!
//! A chat window (server rail, channel list, header, composer) over 14,000 synthetic Turkish,
//! Japanese and English messages: idle, scroll up for 8 s at 2,400 px/s, jump to the oldest
//! loaded message and keep scrolling while older pages of 50 are prepended, jump to the newest,
//! resize to 600 and 1,300 px wide, scroll for 4 s more. Prints one result line.
//!
//! - default: headless at 60 simulated frames per second; times building the scene plus
//!   rendering it.
//! - --full: redraw the whole canvas every frame, the cost without damage tracking.
//! - --scale N: UI scale (default 1); 900 × 700 logical pixels.
//! - --window: run in a real window and present each frame's damage. Frame times are the
//!   intervals between redraws, as in ADR 0018. The layout uses --scale whatever the display's
//!   scale, and the resize phases are skipped (the window cannot be resized from here).
//!
//! Text uses synthetic glyph masks of realistic size and coverage. Shaping and rasterizing real
//! fonts belong to the text stack (ADR 0021 step 4) and are not measured here.

#![allow(
    clippy::print_stdout,
    reason = "the result line on stdout is the benchmark's output"
)]

mod corpus;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use duydas_render::{
    Canvas, Color, IRect, Image, Mask, Radii, Rect, RenderStats, Renderer, Scene, ScrollLayer,
};
use duydas_sys::window::{self, Context, Event, Frame, Handler, WindowOptions};

const LOADED: usize = 10_000;
const HISTORY: usize = 4_000;
const PAGE: usize = 50;
/// Logical pixels per second.
const SPEED: f32 = 2_400.0;
const MAX_DT: f32 = 0.05;
const STALL_MS: f64 = 50.0;
/// ADR 0018: p99 frame time within one 60 Hz frame.
const BUDGET_P99_MS: f64 = 16.7;
const FPS: u32 = 60;
const LOGICAL: (u32, u32) = (900, 700);
const LIST: u64 = 1;

const RAIL: Color = Color::hex(0x1E1F22);
const SIDEBAR: Color = Color::hex(0x2B2D31);
const CHAT: Color = Color::hex(0x313338);
const LINE: Color = Color::hex(0x26272B);
const FIELD: Color = Color::hex(0x383A40);
const TEXT: Color = Color::hex(0xDBDEE1);
const DIM: Color = Color::hex(0x949BA4);
const ACCENT: Color = Color::hex(0x5865F2);
const AUTHOR_COLORS: [Color; 4] = [
    Color::hex(0xF0B232),
    Color::hex(0x23A55A),
    Color::hex(0xEB459E),
    Color::hex(0x00A8FC),
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Warmup,
    ScrollUp,
    Top,
    ScrollAtTop,
    Jump,
    Narrow,
    Wide,
    ScrollAfterResize,
    Done,
}

impl Phase {
    fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Phase::Warmup => 1_500,
            Phase::ScrollUp | Phase::ScrollAtTop => 8_000,
            Phase::Top | Phase::Jump | Phase::Narrow | Phase::Wide => 800,
            Phase::ScrollAfterResize => 4_000,
            Phase::Done => 0,
        })
    }

    fn next(self) -> Self {
        match self {
            Phase::Warmup => Phase::ScrollUp,
            Phase::ScrollUp => Phase::Top,
            Phase::Top => Phase::ScrollAtTop,
            Phase::ScrollAtTop => Phase::Jump,
            Phase::Jump => Phase::Narrow,
            Phase::Narrow => Phase::Wide,
            Phase::Wide => Phase::ScrollAfterResize,
            Phase::ScrollAfterResize | Phase::Done => Phase::Done,
        }
    }

    fn scrolls(self) -> bool {
        matches!(
            self,
            Phase::ScrollUp | Phase::ScrollAtTop | Phase::ScrollAfterResize
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Space,
    Latin,
    Wide,
    Emoji,
}

fn class(c: char) -> Class {
    match c {
        ' ' | '\t' => Class::Space,
        '\u{3000}'..='\u{9FFF}' | '\u{FF00}'..='\u{FFEF}' => Class::Wide,
        c if c > '\u{1F000}' => Class::Emoji,
        _ => Class::Latin,
    }
}

/// Synthetic glyphs: masks with the size and stroke coverage of real ones, cached per char.
struct Font {
    px: f32,
    cache: HashMap<char, Option<Arc<Mask>>>,
}

impl Font {
    fn new(size: f32, scale: f32) -> Self {
        Self {
            px: size * scale,
            cache: HashMap::new(),
        }
    }

    fn advance(&self, c: char) -> i32 {
        let em = match class(c) {
            Class::Space => 0.28,
            Class::Latin => 0.55,
            Class::Wide => 1.0,
            Class::Emoji => 1.2,
        };
        (em * self.px).round().max(1.0) as i32
    }

    fn mask(&mut self, c: char) -> Option<Arc<Mask>> {
        let (px, advance) = (self.px, self.advance(c));
        self.cache
            .entry(c)
            .or_insert_with(|| synth_glyph(c, px, advance))
            .clone()
    }
}

fn synth_glyph(c: char, px: f32, advance: i32) -> Option<Arc<Mask>> {
    let kind = class(c);
    if kind == Class::Space || c == '\n' {
        return None;
    }
    let bearing = (0.08 * px).round() as i32;
    let width = (advance - bearing).max(1) as u32;
    let height = match kind {
        Class::Latin => 0.72 * px,
        Class::Wide => 0.88 * px,
        _ => px,
    }
    .round()
    .max(1.0) as u32;
    let mut state = u64::from(c).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };
    let strokes = match kind {
        Class::Latin => 2 + (next() * 2.0) as usize,
        _ => 4 + (next() * 4.0) as usize,
    };
    let (w, h) = (width as f32, height as f32);
    let segments: Vec<[f32; 4]> = (0..strokes)
        .map(|_| [next() * w, next() * h, next() * w, next() * h])
        .collect();
    let half = (0.085 * px).max(1.0) / 2.0;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = segments
                .iter()
                .map(|&s| segment_distance(cx, cy, s))
                .fold(f32::INFINITY, f32::min);
            data.push(((half + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    Mask::new(width, height, data).map(Arc::new)
}

fn segment_distance(px: f32, py: f32, [x0, y0, x1, y1]: [f32; 4]) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 {
        (((px - x0) * dx + (py - y0) * dy) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - x0 - t * dx).hypot(py - y0 - t * dy)
}

/// Greedy line breaking: Latin words stay whole unless longer than a line, CJK and emoji break
/// anywhere. Calls `place` with each glyph's x and line; returns the number of lines.
fn layout(text: &str, font: &Font, width: i32, mut place: impl FnMut(char, i32, i32)) -> i32 {
    let (mut x, mut line) = (0, 0);
    let mut word: Vec<(char, i32)> = Vec::new();
    let mut emit = |word: &mut Vec<(char, i32)>, x: &mut i32, line: &mut i32| {
        let w: i32 = word.iter().map(|g| g.1).sum();
        if *x > 0 && *x + w > width {
            *line += 1;
            *x = 0;
        }
        for &(c, a) in word.iter() {
            if *x > 0 && *x + a > width {
                *line += 1;
                *x = 0;
            }
            place(c, *x, *line);
            *x += a;
        }
        word.clear();
    };
    for c in text.chars() {
        if c == '\n' {
            emit(&mut word, &mut x, &mut line);
            line += 1;
            x = 0;
            continue;
        }
        match class(c) {
            Class::Space => {
                emit(&mut word, &mut x, &mut line);
                if x > 0 {
                    x += font.advance(c);
                }
            }
            Class::Latin => word.push((c, font.advance(c))),
            Class::Wide | Class::Emoji => {
                emit(&mut word, &mut x, &mut line);
                word.push((c, font.advance(c)));
                emit(&mut word, &mut x, &mut line);
            }
        }
    }
    emit(&mut word, &mut x, &mut line);
    line + 1
}

struct Glyph {
    mask: Arc<Mask>,
    x: i32,
    y: i32,
    color: Color,
}

struct Message {
    author: usize,
    body: String,
}

/// A loaded message laid out for the current width. Content coordinates count up from the
/// newest message's bottom edge (0), so prepending older messages moves nothing.
struct Laid {
    message: usize,
    top: i32,
    height: i32,
    /// Built when the message first becomes visible.
    glyphs: Option<Vec<Glyph>>,
}

struct Fonts {
    body: Font,
    author: Font,
    small: Font,
}

/// Device-pixel metrics for one scale.
#[derive(Clone, Copy)]
struct Metrics {
    scale: f32,
}

impl Metrics {
    fn px(self, logical: f32) -> i32 {
        (logical * self.scale).round() as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Idle,
    Scroll,
    Prepend,
}

struct Session {
    m: Metrics,
    width: u32,
    height: u32,
    messages: Vec<Message>,
    first: usize,
    laid: Vec<Laid>,
    offset: i32,
    fonts: Fonts,
    avatars: Vec<Arc<Image>>,
    chrome: Vec<Glyph>,
    pill: Vec<Glyph>,
    prepends: usize,
    /// Prepends after which the top row was not where it had been.
    moved: usize,
    widths: Vec<u32>,
}

impl Session {
    fn new(scale: f32, width: u32, height: u32) -> Self {
        let m = Metrics { scale };
        let avatar = m.px(36.0).max(1) as u32;
        let mut session = Self {
            m,
            width,
            height,
            messages: generate(HISTORY + LOADED),
            first: HISTORY,
            laid: Vec::new(),
            offset: 0,
            fonts: Fonts {
                body: Font::new(15.0, scale),
                author: Font::new(14.0, scale),
                small: Font::new(12.0, scale),
            },
            avatars: (0..corpus::AUTHORS.len())
                .map(|i| Arc::new(synth_avatar(i, avatar)))
                .collect(),
            chrome: Vec::new(),
            pill: Vec::new(),
            prepends: 0,
            moved: 0,
            widths: Vec::new(),
        };
        session.resize(width, height);
        session
    }

    fn viewport(&self) -> IRect {
        let m = self.m;
        IRect::new(
            m.px(312.0),
            m.px(48.0),
            self.width as i32,
            (self.height as i32 - m.px(68.0)).max(m.px(48.0)),
        )
    }

    fn text_width(&self) -> i32 {
        (self.viewport().width() as i32 - self.m.px(64.0 + 16.0)).max(self.m.px(40.0))
    }

    fn message_height(&self, index: usize) -> i32 {
        let m = self.m;
        let lines = layout(
            &self.messages[index].body,
            &self.fonts.body,
            self.text_width(),
            |_, _, _| {},
        );
        (m.px(8.0 + 20.0 + 8.0) + lines * m.px(22.0)).max(m.px(52.0))
    }

    /// Content y of the viewport's top when showing the newest message.
    fn bottom(&self) -> i32 {
        -(self.viewport().height() as i32)
    }

    fn content_top(&self) -> i32 {
        self.laid.first().map_or(0, |l| l.top)
    }

    /// Lays everything out for a new size, staying at the bottom if it was there.
    fn resize(&mut self, width: u32, height: u32) {
        let at_bottom = self.laid.is_empty() || self.offset >= self.bottom();
        self.width = width;
        self.height = height;
        if !self.widths.contains(&width) {
            self.widths.push(width);
        }
        let mut bottom = 0;
        let loaded = self.first..self.messages.len();
        let heights: Vec<i32> = loaded.clone().map(|i| self.message_height(i)).collect();
        let mut laid = Vec::with_capacity(heights.len());
        for (message, height) in loaded.zip(heights).rev() {
            laid.push(Laid {
                message,
                top: bottom - height,
                height,
                glyphs: None,
            });
            bottom -= height;
        }
        laid.reverse();
        self.laid = laid;
        self.offset = if at_bottom {
            self.bottom()
        } else {
            self.offset.clamp(self.content_top(), self.bottom())
        };
        self.build_chrome();
    }

    fn build_chrome(&mut self) {
        let m = self.m;
        let mut chrome = Vec::new();
        let channels = [
            "# genel",
            "# duyurular",
            "# yardım-masası",
            "# 日本語",
            "# geliştirme",
            "# tasarım",
            "# off-topic",
            "# çeviri-ekibi",
            "# リリース",
            "# ses-testleri",
        ];
        for (i, name) in channels.iter().enumerate() {
            let y = m.px(56.0 + 34.0 * i as f32);
            text_glyphs(&mut self.fonts.body, name, m.px(88.0), y, DIM, &mut chrome);
        }
        let viewport = self.viewport();
        text_glyphs(
            &mut self.fonts.body,
            "# genel",
            viewport.x0 + m.px(16.0),
            m.px(16.0),
            TEXT,
            &mut chrome,
        );
        let placeholder = "#genel kanalına mesaj gönder";
        let y = self.height as i32 - m.px(68.0) + m.px(26.0);
        text_glyphs(
            &mut self.fonts.body,
            placeholder,
            viewport.x0 + m.px(32.0),
            y,
            DIM,
            &mut chrome,
        );
        self.chrome = chrome;
        let mut pill = Vec::new();
        let label = "Eski mesajlara bakıyorsun — 最新に移動";
        text_glyphs(
            &mut self.fonts.small,
            label,
            viewport.x0 + m.px(28.0),
            viewport.y0 + m.px(14.0),
            Color::WHITE,
            &mut pill,
        );
        self.pill = pill;
    }

    /// The message at the top of the viewport and its distance from the viewport's top edge.
    fn anchor(&self) -> Option<(usize, i32)> {
        let i = self
            .laid
            .partition_point(|l| l.top + l.height <= self.offset);
        self.laid.get(i).map(|l| (l.message, l.top - self.offset))
    }

    fn jump_to_top(&mut self) {
        self.offset = self.content_top();
    }

    fn jump_to_bottom(&mut self) {
        self.offset = self.bottom();
    }

    /// Scrolls up by `dy` device pixels, or prepends a page when the top of the loaded
    /// history comes within two screens.
    fn scroll(&mut self, dy: i32) -> Kind {
        let screen = self.viewport().height() as i32;
        if self.offset - self.content_top() < 2 * screen && self.first > 0 {
            let count = PAGE.min(self.first);
            let start = self.first - count;
            let mut top = self.content_top();
            let mut page: Vec<Laid> = (start..self.first)
                .rev()
                .map(|message| {
                    let height = self.message_height(message);
                    top -= height;
                    Laid {
                        message,
                        top,
                        height,
                        glyphs: None,
                    }
                })
                .collect();
            page.reverse();
            let anchor = self.anchor();
            self.laid.splice(0..0, page);
            self.first = start;
            self.prepends += 1;
            if self.anchor() != anchor {
                self.moved += 1;
            }
            return Kind::Prepend;
        }
        self.offset = (self.offset - dy).max(self.content_top());
        Kind::Scroll
    }

    fn build(&mut self, scene: &mut Scene) {
        let m = self.m;
        let (w, h) = (self.width as f32, self.height as f32);
        let viewport = self.viewport();
        let vx = viewport.x0 as f32;
        scene.fill(Rect::new(0.0, 0.0, m.px(72.0) as f32, h), RAIL);
        for i in 0..8 {
            let size = m.px(48.0) as f32;
            let y = m.px(12.0 + 56.0 * i as f32) as f32;
            let color = AUTHOR_COLORS[i % AUTHOR_COLORS.len()];
            let icon = Rect::new(m.px(12.0) as f32, y, size, size);
            scene.fill_rounded(icon, Radii::all(m.px(16.0) as f32), color);
        }
        scene.fill(
            Rect::new(m.px(72.0) as f32, 0.0, m.px(240.0) as f32, h),
            SIDEBAR,
        );
        scene.fill(Rect::new(vx, 0.0, w - vx, viewport.y0 as f32), CHAT);
        scene.fill(
            Rect::new(
                vx,
                viewport.y0 as f32 - m.px(1.0) as f32,
                w - vx,
                m.px(1.0) as f32,
            ),
            LINE,
        );

        scene.push_scroll_layer(ScrollLayer {
            id: LIST,
            viewport,
            offset: self.offset,
            background: CHAT,
        });
        let end = self.offset + viewport.height() as i32;
        let start = self
            .laid
            .partition_point(|l| l.top + l.height <= self.offset);
        let avatar = m.px(36.0) as f32;
        for i in start..self.laid.len() {
            if self.laid[i].top >= end {
                break;
            }
            self.ensure_glyphs(i);
            let laid = &self.laid[i];
            let top = laid.top;
            let author = self.messages[laid.message].author;
            let face = Rect::new(
                vx + m.px(16.0) as f32,
                (top + m.px(8.0)) as f32,
                avatar,
                avatar,
            );
            scene.draw_image(&self.avatars[author], face, Radii::all(avatar / 2.0));
            for g in laid.glyphs.iter().flatten() {
                scene.draw_mask(&g.mask, viewport.x0 + g.x, top + g.y, g.color);
            }
        }
        scene.pop_scroll_layer();

        if self.offset < self.bottom() {
            let pill = Rect::new(
                vx + m.px(16.0) as f32,
                (viewport.y0 + m.px(8.0)) as f32,
                (viewport.width() as i32 - m.px(32.0)) as f32,
                m.px(28.0) as f32,
            );
            scene.fill_rounded(pill, Radii::all(m.px(8.0) as f32), ACCENT.with_alpha(235));
            for g in &self.pill {
                scene.draw_mask(&g.mask, g.x, g.y, g.color);
            }
        }
        scene.fill(
            Rect::new(vx, viewport.y1 as f32, w - vx, h - viewport.y1 as f32),
            CHAT,
        );
        let field = Rect::new(
            vx + m.px(16.0) as f32,
            (viewport.y1 + m.px(12.0)) as f32,
            (viewport.width() as i32 - m.px(32.0)) as f32,
            m.px(44.0) as f32,
        );
        scene.fill_rounded(field, Radii::all(m.px(8.0) as f32), FIELD);
        for g in &self.chrome {
            scene.draw_mask(&g.mask, g.x, g.y, g.color);
        }
    }

    /// Lays out the glyphs of loaded message i, relative to the list's left edge and the
    /// message's top, if not done yet.
    fn ensure_glyphs(&mut self, i: usize) {
        if self.laid[i].glyphs.is_some() {
            return;
        }
        let m = self.m;
        let width = self.text_width();
        let message = &self.messages[self.laid[i].message];
        let mut glyphs = Vec::new();
        let x0 = m.px(64.0);
        let name = corpus::AUTHORS[message.author];
        let color = AUTHOR_COLORS[message.author % AUTHOR_COLORS.len()];
        let end = text_glyphs(
            &mut self.fonts.author,
            name,
            x0,
            m.px(10.0),
            color,
            &mut glyphs,
        );
        let stamp = end + m.px(8.0);
        text_glyphs(
            &mut self.fonts.small,
            "14:32",
            stamp,
            m.px(12.0),
            DIM,
            &mut glyphs,
        );
        let line_height = m.px(22.0);
        let top = m.px(8.0 + 20.0);
        let mut placed = Vec::new();
        layout(&message.body, &self.fonts.body, width, |c, x, line| {
            placed.push((c, x, line));
        });
        for (c, x, line) in placed {
            if let Some(mask) = self.fonts.body.mask(c) {
                let y = top + line * line_height + (line_height - mask.height() as i32) / 2;
                glyphs.push(Glyph {
                    mask,
                    x: x0 + x,
                    y,
                    color: TEXT,
                });
            }
        }
        self.laid[i].glyphs = Some(glyphs);
    }
}

/// Lays out one line of text at (x, y); returns the x after its last glyph.
fn text_glyphs(
    font: &mut Font,
    text: &str,
    x: i32,
    y: i32,
    color: Color,
    out: &mut Vec<Glyph>,
) -> i32 {
    let mut pen = x;
    for c in text.chars() {
        if let Some(mask) = font.mask(c) {
            out.push(Glyph {
                mask,
                x: pen,
                y,
                color,
            });
        }
        pen += font.advance(c);
    }
    pen
}

fn synth_avatar(index: usize, size: u32) -> Image {
    let base = AUTHOR_COLORS[index % AUTHOR_COLORS.len()];
    let mut pixels = Vec::with_capacity((size * size) as usize);
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 / s, y as f32 / s);
            let shade = 0.7 + 0.3 * (1.0 - fy);
            let blob = ((fx - 0.5).hypot(fy - 0.42) < 0.2) as u8 as f32 * 0.35;
            let channel = |v: u8| ((f32::from(v) * shade + 255.0 * blob).min(255.0)) as u32;
            pixels.push(
                0xFF00_0000 | (channel(base.r) << 16) | (channel(base.g) << 8) | channel(base.b),
            );
        }
    }
    Image::from_premultiplied(size, size, pixels).unwrap_or_else(|| unreachable!("valid size"))
}

/// Deterministic messages: mostly one or two phrases, sometimes several paragraphs (the ADR
/// 0018 generator).
fn generate(count: usize) -> Vec<Message> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    (0..count)
        .map(|_| {
            let phrases = match next(20) {
                0 => 6 + next(6),
                1..=4 => 2 + next(2),
                _ => 1,
            };
            let mut body = String::new();
            for p in 0..phrases {
                if p > 0 {
                    body.push_str(if next(3) == 0 { "\n\n" } else { " " });
                }
                body.push_str(corpus::PHRASES[next(corpus::PHRASES.len())]);
            }
            Message {
                author: next(corpus::AUTHORS.len()),
                body,
            }
        })
        .collect()
}

#[derive(Default)]
struct Stats {
    scroll_ms: Vec<f64>,
    prepend_ms: Vec<f64>,
    cost_ms: Vec<f64>,
    present_ms: Vec<f64>,
    first_ms: f64,
    jump_ms: f64,
    resize_ms: f64,
    /// Jump and resize frames: not part of the percentiles, but a stall all the same.
    transition_ms: Vec<f64>,
    present_failures: usize,
    repainted: u64,
    scrolled: u64,
    canvas: u64,
    draws: u64,
}

impl Stats {
    fn record(&mut self, kind: Kind, ms: f64, render: &RenderStats, canvas: u64) {
        match kind {
            Kind::Scroll => self.scroll_ms.push(ms),
            Kind::Prepend => self.prepend_ms.push(ms),
            Kind::Idle => return,
        }
        self.repainted += render.repainted;
        self.scrolled += render.scrolled;
        self.canvas += canvas;
        self.draws += u64::from(render.draws);
    }

    /// The result line. Headless runs must include the resizes; windowed runs must present
    /// every frame.
    fn report(&mut self, label: &str, session: &Session, windowed: bool, complete: bool) -> String {
        self.scroll_ms.sort_by(f64::total_cmp);
        self.cost_ms.sort_by(f64::total_cmp);
        self.present_ms.sort_by(f64::total_cmp);
        let frames = &self.scroll_ms;
        let prepend_max = self.prepend_ms.iter().copied().fold(0.0, f64::max);
        let stalls = frames
            .iter()
            .chain(&self.prepend_ms)
            .chain(&self.transition_ms)
            .filter(|ms| **ms > STALL_MS)
            .count();
        let ok = complete
            && !frames.is_empty()
            && session.prepends > 0
            && session.moved == 0
            && if windowed {
                self.present_failures == 0
            } else {
                session.widths.len() >= 3
            };
        // Judged on unrounded samples.
        let budget = if ok && stalls == 0 && percentile(frames, 0.99) <= BUDGET_P99_MS {
            "pass"
        } else {
            "fail"
        };
        let count = (frames.len() + self.prepend_ms.len()).max(1) as f64;
        let mut line = format!(
            "duydas-render-scroll ok={ok} budget={budget} mode={label} scale={} size={}x{} frames={} \
             p50_ms={:.2} p99_ms={:.2} max_ms={:.2} prepend_max_ms={prepend_max:.2} \
             stalls={stalls} first_ms={:.2} jump_ms={:.2} resize_ms={:.2} \
             repainted_pct={:.1} scrolled_pct={:.1} draws_avg={:.0} prepends={} moved={} widths={}",
            session.m.scale,
            session.width,
            session.height,
            frames.len(),
            percentile(frames, 0.50),
            percentile(frames, 0.99),
            frames.last().copied().unwrap_or(0.0),
            self.first_ms,
            self.jump_ms,
            self.resize_ms,
            100.0 * self.repainted as f64 / self.canvas.max(1) as f64,
            100.0 * self.scrolled as f64 / self.canvas.max(1) as f64,
            self.draws as f64 / count,
            session.prepends,
            session.moved,
            session.widths.len(),
        );
        if windowed {
            line.push_str(&format!(
                " cost_p99_ms={:.2} present_p99_ms={:.2} present_failures={}",
                percentile(&self.cost_ms, 0.99),
                percentile(&self.present_ms, 0.99),
                self.present_failures,
            ));
        }
        line
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

struct Args {
    scale: f32,
    full: bool,
    window: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        scale: 1.0,
        full: false,
        window: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--full" => args.full = true,
            "--window" => args.window = true,
            "--scale" => {
                args.scale = it
                    .next()
                    .and_then(|s| s.parse::<f32>().ok())
                    .filter(|s| s.is_finite() && (0.5..=4.0).contains(s))
                    .ok_or("--scale needs a number from 0.5 to 4")?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            println!("error: {error}");
            std::process::exit(2);
        }
    };
    let label = if args.full { "full" } else { "damage" };
    if args.window {
        match window::run(
            WindowOptions {
                title: "Duydaş render benchmark".into(),
                size: LOGICAL,
                min_size: None,
                visible: false,
                dark_title_bar: true,
            },
            Windowed::new(args.scale, args.full),
        ) {
            Ok(mut done) => {
                let label = format!("window-{label}");
                match &done.session {
                    Some(session) => println!(
                        "{}",
                        done.stats.report(&label, session, true, done.completed)
                    ),
                    None => println!("error: the window never drew"),
                }
            }
            Err(error) => println!("error: {error}"),
        }
    } else {
        headless(args.scale, args.full, label);
    }
}

fn logical_size(scale: f32, width: u32) -> (u32, u32) {
    let m = Metrics { scale };
    (m.px(width as f32) as u32, m.px(LOGICAL.1 as f32) as u32)
}

fn headless(scale: f32, full: bool, label: &str) {
    let (w, h) = logical_size(scale, LOGICAL.0);
    let started = Instant::now();
    let mut session = Session::new(scale, w, h);
    let mut canvas = Canvas::new(w, h, RAIL).unwrap_or_else(|| unreachable!("valid size"));
    let mut renderer = Renderer::new();
    let mut scene = Scene::new();
    let mut stats = Stats::default();
    let mut frame = |session: &mut Session, canvas: &mut Canvas| {
        session.build(&mut scene);
        if full {
            renderer.invalidate();
        }
        renderer.render(&mut scene, canvas, RAIL)
    };
    frame(&mut session, &mut canvas);
    stats.first_ms = started.elapsed().as_secs_f64() * 1e3;
    let step = (SPEED * scale / FPS as f32).round() as i32;
    let mut phase = Phase::Warmup;
    loop {
        phase = phase.next();
        let t = Instant::now();
        match phase {
            Phase::Top | Phase::Jump => {
                if phase == Phase::Top {
                    session.jump_to_top();
                } else {
                    session.jump_to_bottom();
                }
                frame(&mut session, &mut canvas);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                stats.jump_ms = stats.jump_ms.max(ms);
                stats.transition_ms.push(ms);
            }
            Phase::Narrow | Phase::Wide => {
                let width = if phase == Phase::Narrow { 600 } else { 1_300 };
                let (w, h) = logical_size(scale, width);
                session.resize(w, h);
                canvas = Canvas::new(w, h, RAIL).unwrap_or_else(|| unreachable!("valid size"));
                frame(&mut session, &mut canvas);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                stats.resize_ms = stats.resize_ms.max(ms);
                stats.transition_ms.push(ms);
            }
            Phase::Done => break,
            _ => {}
        }
        if phase.scrolls() {
            let frames = phase.duration().as_millis() as u32 * FPS / 1000;
            for _ in 0..frames {
                let t = Instant::now();
                let kind = session.scroll(step);
                let render = frame(&mut session, &mut canvas);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                let area = u64::from(canvas.width()) * u64::from(canvas.height());
                stats.record(kind, ms, &render, area);
            }
        }
    }
    println!("{}", stats.report(label, &session, false, true));
}

/// Real-window mode: the session advances on wall-clock time and every frame is presented.
struct Windowed {
    scale: f32,
    full: bool,
    session: Option<Session>,
    canvas: Option<Canvas>,
    renderer: Renderer,
    scene: Scene,
    stats: Stats,
    phase: Phase,
    phase_started: Instant,
    last_frame: Option<Instant>,
    last_kind: Kind,
    /// The previous frame jumped: its interval is a transition, not a scroll frame.
    jumped: bool,
    /// Scroll distance not yet applied, so the speed does not depend on the frame rate.
    carry: f32,
    /// The whole session ran; a window closed early is not a result.
    completed: bool,
    shown: bool,
}

impl Windowed {
    fn new(scale: f32, full: bool) -> Self {
        Self {
            scale,
            full,
            session: None,
            canvas: None,
            renderer: Renderer::new(),
            scene: Scene::new(),
            stats: Stats::default(),
            phase: Phase::Warmup,
            phase_started: Instant::now(),
            last_frame: None,
            last_kind: Kind::Idle,
            jumped: false,
            carry: 0.0,
            completed: false,
            shown: false,
        }
    }

    fn redraw(&mut self, cx: &mut Context) {
        let (Some(session), Some(canvas)) = (self.session.as_mut(), self.canvas.as_mut()) else {
            return;
        };
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        if let Some(last) = self.last_frame {
            let ms = now.duration_since(last).as_secs_f64() * 1e3;
            if self.jumped {
                self.stats.jump_ms = self.stats.jump_ms.max(ms);
                self.stats.transition_ms.push(ms);
            } else {
                let area = u64::from(canvas.width()) * u64::from(canvas.height());
                let render = RenderStats::default();
                self.stats.record(self.last_kind, ms, &render, area);
            }
        }
        self.last_frame = Some(now);
        self.jumped = false;
        if now.duration_since(self.phase_started) >= self.phase.duration() {
            self.phase = self.phase.next();
            self.phase_started = now;
            match self.phase {
                Phase::Top => {
                    session.jump_to_top();
                    self.jumped = true;
                }
                Phase::Jump => {
                    session.jump_to_bottom();
                    self.jumped = true;
                }
                Phase::Done => {
                    self.completed = true;
                    cx.exit();
                    return;
                }
                _ => {}
            }
        }
        self.last_kind = if self.phase.scrolls() {
            self.carry += SPEED * self.scale * dt.min(MAX_DT);
            let dy = self.carry.floor();
            self.carry -= dy;
            session.scroll(dy as i32)
        } else {
            Kind::Idle
        };
        let t = Instant::now();
        session.build(&mut self.scene);
        if self.full {
            self.renderer.invalidate();
        }
        let render = self.renderer.render(&mut self.scene, canvas, RAIL);
        if self.last_kind != Kind::Idle {
            self.stats.cost_ms.push(t.elapsed().as_secs_f64() * 1e3);
            self.stats.repainted += render.repainted;
            self.stats.scrolled += render.scrolled;
            self.stats.draws += u64::from(render.draws);
        }
        let first = !self.shown;
        if first {
            self.shown = true;
            cx.set_visible(true);
            self.stats.first_ms = self.phase_started.elapsed().as_secs_f64() * 1e3;
        }
        let damage: Vec<window::Rect> = self
            .renderer
            .damage()
            .iter()
            .map(|r| window::Rect {
                x: r.x0,
                y: r.y0,
                width: r.width(),
                height: r.height(),
            })
            .collect();
        let t = Instant::now();
        let presented = Frame::new(canvas.pixels(), canvas.width(), canvas.height())
            .is_some_and(|frame| cx.present(frame, &damage).is_ok());
        if !presented {
            // The window missed this frame's changes: draw the next one in full.
            self.renderer.invalidate();
            self.stats.present_failures += 1;
        } else if self.last_kind != Kind::Idle {
            self.stats.present_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
        cx.request_redraw();
    }
}

impl Handler for Windowed {
    fn event(&mut self, cx: &mut Context, event: Event) {
        match event {
            Event::Resized { width, height } if width > 0 && height > 0 => {
                match self.session.as_mut() {
                    Some(session) => session.resize(width, height),
                    None => self.session = Some(Session::new(self.scale, width, height)),
                }
                self.canvas = Canvas::new(width, height, RAIL);
                cx.request_redraw();
            }
            Event::RedrawRequested => self.redraw(cx),
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }
}
