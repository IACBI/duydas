//! Interactive check of the window backend, and its startup/idle measurements.
//!
//! - No arguments: a window with a square that follows the pointer (only the damaged rectangles
//!   are presented). Typed text, IME preedit (in brackets) and commits appear in the title bar;
//!   Ctrl+V pastes, Ctrl+C copies the typed text, Escape clears it.
//! - --first-frame: prints the time from `main` to the first successful present, then exits.
//! - --idle SECS: after the first frame, counts events for SECS seconds of no input, then exits.
//! - --present-bench: times full-window presents against small damaged rectangles, then exits.

#![allow(clippy::print_stdout, reason = "example output")]

use std::time::{Duration, Instant};

use duydas_sys::window::{
    self, Context, Cursor, Event, Frame, Handler, ImeEvent, Key, Rect, WindowOptions,
};

const SQUARE: u32 = 48;

enum Mode {
    Interactive,
    FirstFrame,
    Idle(u64),
    PresentBench,
}

struct Demo {
    mode: Mode,
    started: Instant,
    pixels: Vec<u32>,
    size: (u32, u32),
    square: Option<Rect>,
    typed: String,
    preedit: String,
    shown: bool,
    idle_events: Option<u64>,
    idle_log: Vec<String>,
}

impl Demo {
    fn background(x: u32, y: u32, size: (u32, u32)) -> u32 {
        let r = 0x14 + (x * 0x18 / size.0.max(1));
        let g = 0x16 + (y * 0x14 / size.1.max(1));
        let b = 0x22;
        (r << 16) | (g << 8) | b
    }

    fn paint_rect(&mut self, rect: Rect, color: Option<u32>) {
        let Some(r) = rect.clip(self.size.0, self.size.1) else {
            return;
        };
        for y in r.y as u32..(r.y as u32 + r.height) {
            let row = (y * self.size.0) as usize;
            for x in r.x as u32..(r.x as u32 + r.width) {
                self.pixels[row + x as usize] =
                    color.unwrap_or_else(|| Self::background(x, y, self.size));
            }
        }
    }

    fn full(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.size.0,
            height: self.size.1,
        }
    }

    fn present(&self, cx: &mut Context, damage: &[Rect]) -> bool {
        let Some(frame) = Frame::new(&self.pixels, self.size.0, self.size.1) else {
            return false;
        };
        match cx.present(frame, damage) {
            Ok(()) => true,
            Err(error) => {
                println!("present failed: {error}");
                false
            }
        }
    }

    /// Average time per present in microseconds, or None if any present failed.
    fn time_presents(&self, cx: &mut Context, damage: &[Rect], runs: u32) -> Option<f64> {
        let start = Instant::now();
        for _ in 0..runs {
            if !self.present(cx, damage) {
                return None;
            }
        }
        Some(start.elapsed().as_secs_f64() * 1e6 / f64::from(runs))
    }

    fn update_title(&self, cx: &mut Context) {
        let mut title = format!("Duydaş window check — {}", self.typed);
        if !self.preedit.is_empty() {
            title.push_str(&format!("[{}]", self.preedit));
        }
        cx.set_title(&title);
    }
}

impl Handler for Demo {
    fn event(&mut self, cx: &mut Context, event: Event) {
        if let Some(count) = self.idle_events.as_mut()
            && event != Event::Wake
        {
            *count += 1;
            self.idle_log.push(format!("{event:?}"));
        }
        match event {
            Event::Resized { width, height } => {
                self.size = (width, height);
                self.pixels = vec![0; (width * height) as usize];
                let full = self.full();
                self.paint_rect(full, None);
                cx.request_redraw();
            }
            Event::RedrawRequested => {
                // Show the window just before its first frame reaches it, so no unpainted
                // window is ever on screen.
                let first = !self.shown;
                if first {
                    self.shown = true;
                    cx.set_visible(true);
                }
                let full = self.full();
                let presented = self.present(cx, &[full]);
                if first {
                    let elapsed = self.started.elapsed();
                    match self.mode {
                        Mode::FirstFrame => {
                            if presented {
                                println!("first_present_ms={:.1}", elapsed.as_secs_f64() * 1e3);
                            }
                            cx.exit();
                        }
                        Mode::PresentBench => {
                            // Measure on the visible window: the wake arrives after the window
                            // has been revealed at the end of this event.
                            cx.proxy().wake();
                        }
                        Mode::Idle(seconds) => {
                            self.idle_events = Some(0);
                            let proxy = cx.proxy();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(seconds));
                                proxy.wake();
                            });
                        }
                        Mode::Interactive => {
                            cx.set_cursor(Cursor::Pointer);
                            println!("first frame after {:.1} ms", elapsed.as_secs_f64() * 1e3);
                        }
                    }
                }
            }
            Event::Wake if matches!(self.mode, Mode::PresentBench) => {
                let full = self.full();
                let small = Rect {
                    x: 100,
                    y: 100,
                    width: SQUARE,
                    height: SQUARE,
                };
                match (
                    self.time_presents(cx, &[full], 200),
                    self.time_presents(cx, &[small], 200),
                ) {
                    (Some(full_us), Some(small_us)) => println!(
                        "present_us {}x{} full={full_us:.0} {SQUARE}x{SQUARE}={small_us:.0}",
                        self.size.0, self.size.1
                    ),
                    _ => println!("present_us failed"),
                }
                cx.exit();
            }
            Event::Wake => {
                if let Some(count) = self.idle_events {
                    // Expected: Focused(true) and one RedrawRequested right after showing.
                    if self.idle_log.len() <= 10 {
                        println!("idle_events={count} {:?}", self.idle_log);
                    } else {
                        println!("idle_events={count} (input arrived during the measurement)");
                    }
                    cx.exit();
                }
            }
            Event::PointerMoved { x, y } => {
                let next = Rect {
                    x: x - SQUARE as i32 / 2,
                    y: y - SQUARE as i32 / 2,
                    width: SQUARE,
                    height: SQUARE,
                };
                let mut damage = vec![next];
                if let Some(old) = self.square.replace(next) {
                    self.paint_rect(old, None);
                    damage.push(old);
                }
                self.paint_rect(next, Some(0x00E8_A33D));
                self.present(cx, &damage);
                cx.set_ime_cursor_area(next);
            }
            Event::Char(c) => {
                self.typed.push(c);
                self.update_title(cx);
            }
            Event::Key(key) if key.pressed => match key.key {
                Key::Escape => {
                    self.typed.clear();
                    self.update_title(cx);
                }
                Key::Backspace => {
                    self.typed.pop();
                    self.update_title(cx);
                }
                Key::Character('v') if key.modifiers.control => {
                    if let Some(text) = cx.clipboard_text() {
                        self.typed.push_str(&text);
                        self.update_title(cx);
                    }
                }
                Key::Character('c') if key.modifiers.control => {
                    if let Err(error) = cx.set_clipboard_text(&self.typed) {
                        println!("copy failed: {error}");
                    }
                }
                _ => {}
            },
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                self.preedit = text;
                self.update_title(cx);
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                self.typed.push_str(&text);
                self.preedit.clear();
                self.update_title(cx);
            }
            Event::Ime(ImeEvent::End) => {
                self.preedit.clear();
                self.update_title(cx);
            }
            Event::PointerCaptureLost => cx.set_cursor(Cursor::Pointer),
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }
}

fn main() {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("--first-frame") => Mode::FirstFrame,
        Some("--idle") => Mode::Idle(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5)),
        Some("--present-bench") => Mode::PresentBench,
        _ => Mode::Interactive,
    };
    let demo = Demo {
        mode,
        started,
        pixels: Vec::new(),
        size: (0, 0),
        square: None,
        typed: String::new(),
        preedit: String::new(),
        shown: false,
        idle_events: None,
        idle_log: Vec::new(),
    };
    let options = WindowOptions {
        title: "Duydaş window check".to_owned(),
        dark_title_bar: true,
        ..WindowOptions::default()
    };
    if let Err(error) = window::run(options, demo) {
        println!("window failed: {error}");
    }
}
