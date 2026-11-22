//! Spike for ADR 0018: a virtualized, bottom-anchored chat history.
//!
//! Loads 10,000 synthetic messages, runs a scripted session and prints one result line: scroll up
//! through history while older pages are prepended, jump to the end, resize the window, scroll
//! again. `DUYDAS_SCALE` sets the UI scale factor. Run through `cargo xtask vlist`.
//!
//! The scrolled-to-top phase starts from the relative offset `snap_to` leaves behind (a "jump to
//! oldest" button), the worst case for prepends; see `virtual_list::keep_position`.
//!
//! Frame intervals come from `window::frames()`, i.e. redraw timestamps. With vsync on they show
//! cadence, not cost; `cargo xtask vlist` also runs wgpu without vsync to expose cost.

#![allow(
    clippy::print_stdout,
    reason = "the result line on stdout is the spike's output"
)]

mod corpus;

use std::cell::RefCell;
use std::time::{Duration, Instant};

use duydas_ui::virtual_list::{self, HeightIndex};
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset, Viewport};
use iced::widget::{Id, column, container, operation, text};
use iced::{Element, Fill, Size, Subscription, Task, window};

const LOADED: usize = 10_000;
const HISTORY: usize = 4_000;
const PAGE: usize = 50;
const ESTIMATE: f32 = 64.0;
const OVERSCAN: f32 = 400.0;
const SCROLL_SPEED: f32 = 2_400.0;
const MAX_STEP: f32 = SCROLL_SPEED * 0.05;
const STALL_MS: f32 = 50.0;
const CHECK_TIMEOUT_FRAMES: u8 = 5;

fn main() -> iced::Result {
    let scale = std::env::var("DUYDAS_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    iced::application(Spike::new, Spike::update, Spike::view)
        .subscription(Spike::subscription)
        .scale_factor(move |_| scale)
        .window_size((900.0, 700.0))
        .run()
}

#[derive(Debug)]
struct Item {
    id: u64,
    author: &'static str,
    body: String,
}

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
            Phase::ScrollUp => 8_000,
            Phase::ScrollAtTop => 8_000,
            Phase::Top | Phase::Jump | Phase::Narrow | Phase::Wide => 800,
            Phase::ScrollAfterResize => 4_000,
            Phase::Done => u64::MAX,
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

/// A prepend whose effect has not been observed yet: the row at the top of the viewport and its
/// distance from the viewport's top edge, which must not change. Positions come from the height
/// model, so this catches scroll-offset changes (the relative-offset trap), not shifts from rows
/// re-measured inside the viewport.
#[derive(Debug, Clone, Copy)]
struct PendingCheck {
    anchor_id: u64,
    anchor_y: f32,
    frames_left: u8,
}

/// What the previous frame did, to sort its interval into the right bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FrameKind {
    Idle,
    Scroll,
    Prepend,
}

#[derive(Debug, Default)]
struct Stats {
    scroll_ms: Vec<f32>,
    prepend_ms: Vec<f32>,
    widths: Vec<f32>,
    view_us: RefCell<Vec<f32>>,
    rows_built: RefCell<Vec<usize>>,
    measurements: usize,
    prepends: usize,
    stable: usize,
    moved: usize,
    max_shift: f32,
    unobserved: usize,
}

#[derive(Debug, Clone)]
enum Message {
    WindowOpened(Option<window::Id>),
    Frame(Instant),
    Tick,
    Scrolled(Viewport),
    Measured(u64, f32),
}

#[derive(Debug)]
struct Spike {
    all: Vec<Item>,
    first: usize,
    heights: HeightIndex,
    viewport: Option<Viewport>,
    window: Option<window::Id>,
    phase: Phase,
    phase_started: Instant,
    last_frame: Option<Instant>,
    last_kind: FrameKind,
    pending: Option<PendingCheck>,
    /// `DUYDAS_NO_PIN` turns off `keep_position` to show what it prevents.
    pin: bool,
    stats: Stats,
}

fn list_id() -> Id {
    Id::new("history")
}

impl Spike {
    fn new() -> (Self, Task<Message>) {
        let all = generate(HISTORY + LOADED);
        let spike = Self {
            first: HISTORY,
            heights: HeightIndex::new(LOADED, ESTIMATE),
            all,
            viewport: None,
            window: None,
            phase: Phase::Warmup,
            phase_started: Instant::now(),
            last_frame: None,
            last_kind: FrameKind::Idle,
            pending: None,
            pin: std::env::var_os("DUYDAS_NO_PIN").is_none(),
            stats: Stats::default(),
        };
        (spike, window::latest().map(Message::WindowOpened))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                self.window = id;
                Task::none()
            }
            Message::Tick => self.advance(Instant::now()),
            Message::Frame(now) => {
                let dt = self.last_frame.map_or(0.0, |last| {
                    now.saturating_duration_since(last).as_secs_f32()
                });
                let ms = dt * 1000.0;
                match self.last_kind {
                    FrameKind::Scroll => self.stats.scroll_ms.push(ms),
                    FrameKind::Prepend => self.stats.prepend_ms.push(ms),
                    // Iced only redraws on change, so idle gaps are not frame times.
                    FrameKind::Idle => {}
                }
                self.last_frame = Some(now);
                self.last_kind = FrameKind::Idle;
                let advanced = self.advance(now);
                Task::batch([advanced, self.step(dt)])
            }
            Message::Scrolled(viewport) => {
                let width = viewport.bounds().width;
                if !self.stats.widths.iter().any(|w| (w - width).abs() < 0.5) {
                    self.stats.widths.push(width);
                }
                if let Some(check) = self.pending.take() {
                    let shift = self
                        .anchor_y(viewport, check.anchor_id)
                        .map_or(f32::INFINITY, |y| (y - check.anchor_y).abs());
                    self.stats.max_shift = self.stats.max_shift.max(shift);
                    if shift <= 1.0 {
                        self.stats.stable += 1;
                    } else {
                        self.stats.moved += 1;
                    }
                }
                self.viewport = Some(viewport);
                Task::none()
            }
            Message::Measured(id, height) => {
                let loaded = &self.all[self.first..];
                if let Ok(i) = loaded.binary_search_by_key(&id, |item| item.id) {
                    self.heights.set(i, height);
                    self.stats.measurements += 1;
                }
                Task::none()
            }
        }
    }

    fn advance(&mut self, now: Instant) -> Task<Message> {
        if self.phase == Phase::Done
            || now.saturating_duration_since(self.phase_started) < self.phase.duration()
        {
            return Task::none();
        }
        self.phase = self.phase.next();
        self.phase_started = now;
        match (self.phase, self.window) {
            // Relative offsets count from the anchor (the bottom): 1.0 is the top of the loaded
            // history. `snap_to` keeps the offset relative, unlike wheel or drag input.
            (Phase::Top, _) => operation::snap_to(list_id(), RelativeOffset { x: 0.0, y: 1.0 }),
            (Phase::Jump, _) => operation::snap_to(list_id(), RelativeOffset::START),
            (Phase::Narrow, Some(id)) => window::resize(id, Size::new(600.0, 700.0)),
            (Phase::Wide, Some(id)) => window::resize(id, Size::new(1300.0, 700.0)),
            (Phase::Done, _) => {
                println!("{}", self.report());
                iced::exit()
            }
            _ => Task::none(),
        }
    }

    /// Scrolls up by one frame's worth, or prepends a page when the top of the loaded history
    /// comes within two screens.
    fn step(&mut self, dt: f32) -> Task<Message> {
        if !self.phase.scrolls() {
            return Task::none();
        }
        let Some(viewport) = self.viewport else {
            return Task::none();
        };
        if let Some(check) = &mut self.pending {
            // Wait for the scrollable to report a viewport after the prepend.
            self.last_kind = FrameKind::Prepend;
            check.frames_left = check.frames_left.saturating_sub(1);
            if check.frames_left == 0 {
                self.pending = None;
                self.stats.unobserved += 1;
            }
            return Task::none();
        }
        let height = viewport.bounds().height;
        let from_bottom = viewport.absolute_offset().y;
        let top = self.heights.total() - height - from_bottom;
        if top < height * 2.0 && self.first >= PAGE {
            let anchor_id = self.all[self.first + self.heights.index_at(top)].id;
            let anchor_y = self.anchor_y(viewport, anchor_id).unwrap_or_default();
            self.first -= PAGE;
            self.heights.prepend(PAGE);
            self.stats.prepends += 1;
            self.last_kind = FrameKind::Prepend;
            self.pending = Some(PendingCheck {
                anchor_id,
                anchor_y,
                frames_left: CHECK_TIMEOUT_FRAMES,
            });
            return if self.pin {
                virtual_list::keep_position(list_id(), from_bottom)
            } else {
                Task::none()
            };
        }
        self.last_kind = FrameKind::Scroll;
        let step = (SCROLL_SPEED * dt).min(MAX_STEP);
        operation::scroll_by(list_id(), AbsoluteOffset { x: 0.0, y: step })
    }

    fn view(&self) -> Element<'_, Message> {
        let started = Instant::now();
        let (from_bottom, height) = self
            .viewport
            .map_or((0.0, 700.0), |v| (v.absolute_offset().y, v.bounds().height));
        let window = self.heights.window(from_bottom, height, OVERSCAN);
        let list = virtual_list::list(
            &self.all[self.first..],
            window,
            |item| item.id,
            row,
            Message::Measured,
        )
        .id(list_id())
        .on_scroll(Message::Scrolled)
        .height(Fill);
        self.stats
            .view_us
            .borrow_mut()
            .push(started.elapsed().as_secs_f32() * 1e6);
        self.stats
            .rows_built
            .borrow_mut()
            .push(window.end - window.start);
        list.into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::frames().map(Message::Frame),
            iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick),
        ])
    }

    /// Distance from the viewport's top edge to the top of row `id`, per the height model.
    fn anchor_y(&self, viewport: Viewport, id: u64) -> Option<f32> {
        let index = self.all[self.first..]
            .binary_search_by_key(&id, |item| item.id)
            .ok()?;
        let top = self.heights.total() - viewport.bounds().height - viewport.absolute_offset().y;
        Some(self.heights.offset_of(index) - top)
    }

    fn report(&self) -> String {
        let mut frames = self.stats.scroll_ms.clone();
        frames.sort_by(f32::total_cmp);
        let prepend_max = self
            .stats
            .prepend_ms
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        // Two resizes plus the initial width; a run that never resized or scrolled is invalid,
        // and with pinning on, every prepend must have been observed and kept the top row still.
        let ok = !frames.is_empty()
            && self.stats.widths.len() >= 3
            && self.stats.prepends > 0
            && (!self.pin || (self.stats.moved == 0 && self.stats.unobserved == 0));
        let mut views = self.stats.view_us.borrow().clone();
        views.sort_by(f32::total_cmp);
        let rows = self.stats.rows_built.borrow();
        let rows_avg = rows.iter().sum::<usize>() as f32 / rows.len().max(1) as f32;
        let stalls = frames.iter().filter(|ms| **ms > STALL_MS).count()
            + self
                .stats
                .prepend_ms
                .iter()
                .filter(|ms| **ms > STALL_MS)
                .count();
        format!(
            "duydas-vlist ok={ok} frames={} p50_ms={:.2} p99_ms={:.2} max_ms={:.2} \
             prepend_max_ms={prepend_max:.2} stalls={stalls} view_p99_us={:.0} \
             rows_avg={rows_avg:.1} widths={} loaded={} measured={} prepends={} stable={} moved={} \
             max_shift_px={:.1} unobserved={} pin={}",
            frames.len(),
            percentile(&frames, 0.50),
            percentile(&frames, 0.99),
            frames.last().copied().unwrap_or(0.0),
            percentile(&views, 0.99),
            self.stats.widths.len(),
            self.heights.len(),
            self.stats.measurements,
            self.stats.prepends,
            self.stats.stable,
            self.stats.moved,
            self.stats.max_shift,
            self.stats.unobserved,
            self.pin,
        )
    }
}

fn row(item: &Item) -> Element<'_, Message> {
    container(
        column![
            text(item.author).size(13),
            text(item.body.as_str()).size(15)
        ]
        .spacing(2),
    )
    .padding([6, 12])
    .width(Fill)
    .into()
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[i]
}

/// Deterministic messages: mostly one or two phrases, sometimes several paragraphs.
fn generate(count: usize) -> Vec<Item> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    (0..count)
        .map(|i| {
            let phrases = match next(20) {
                0 => 6 + next(6),
                1..=4 => 2 + next(2),
                _ => 1,
            };
            let body = (0..phrases)
                .map(|p| {
                    let sep = if p > 0 && next(3) == 0 { "\n\n" } else { " " };
                    let phrase = corpus::PHRASES[next(corpus::PHRASES.len())];
                    if p == 0 {
                        phrase.to_owned()
                    } else {
                        format!("{sep}{phrase}")
                    }
                })
                .collect();
            Item {
                id: i as u64 + 1,
                author: corpus::AUTHORS[next(corpus::AUTHORS.len())],
                body,
            }
        })
        .collect()
}
