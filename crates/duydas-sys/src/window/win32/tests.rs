//! Backend tests. The event-loop tests create real (hidden) windows and drive them with the
//! same messages the OS sends.

use std::cell::Cell;
use std::ffi::c_void;

use super::ffi::{self, Handle};
use super::{ime, keys, point_from_lparam, present};
use crate::window::{
    self, Context, Error, Event, Frame, Handler, Key, Modifiers, Rect, WindowOptions,
};

fn lparam_point(x: i16, y: i16) -> isize {
    ((usize::from(y as u16) << 16) | usize::from(x as u16)) as isize
}

#[test]
fn virtual_keys_map_to_keys() {
    assert_eq!(keys::key_from_vk(0x4B), Key::Character('k'));
    assert_eq!(keys::key_from_vk(0x37), Key::Character('7'));
    assert_eq!(keys::key_from_vk(ffi::VK_RETURN), Key::Enter);
    assert_eq!(keys::key_from_vk(ffi::VK_F1), Key::F(1));
    assert_eq!(keys::key_from_vk(ffi::VK_F24), Key::F(24));
    assert_eq!(keys::key_from_vk(ffi::VK_PROCESSKEY), Key::ImeProcess);
    assert_eq!(keys::key_from_vk(0xBA), Key::Other(0xBA));
}

#[test]
fn key_parameters_decode() {
    // Numpad Enter: scan code 0x1C with the extended bit, auto-repeat.
    let lparam = (0x1C << 16) | (1 << 24) | (1 << 30) | 1;
    let event = keys::key_event(
        usize::from(ffi::VK_RETURN),
        lparam,
        true,
        Modifiers::default(),
    );
    assert_eq!(event.key, Key::Enter);
    assert_eq!(event.scancode, 0xE01C);
    assert!(event.repeat);
    let released = keys::key_event(
        usize::from(ffi::VK_RETURN),
        lparam,
        false,
        Modifiers::default(),
    );
    assert!(!released.repeat);
}

#[test]
fn utf16_units_assemble_into_chars() {
    let high = Cell::new(None);
    assert_eq!(keys::utf16_char(&high, 0x011F), Some('ğ'));
    assert_eq!(keys::utf16_char(&high, 0xD83D), None);
    assert_eq!(keys::utf16_char(&high, 0xDE00), Some('😀'));
    // An unpaired low surrogate, and a high surrogate followed by a normal unit.
    assert_eq!(keys::utf16_char(&high, 0xDE00), None);
    assert_eq!(keys::utf16_char(&high, 0xD83D), None);
    assert_eq!(keys::utf16_char(&high, 0x0061), Some('a'));
    assert_eq!(keys::utf16_char(&high, 0xDE00), None);
}

#[test]
fn points_are_signed() {
    assert_eq!(point_from_lparam(lparam_point(-5, 7)), (-5, 7));
    assert_eq!(
        point_from_lparam(lparam_point(32767, -32768)),
        (32767, -32768)
    );
}

#[test]
fn ime_cursor_offsets_convert_to_bytes() {
    let text = "日本😀a";
    assert_eq!(ime::byte_offset(text, 0), 0);
    assert_eq!(ime::byte_offset(text, 1), 3);
    assert_eq!(ime::byte_offset(text, 2), 6);
    assert_eq!(ime::byte_offset(text, 4), 10);
    assert_eq!(ime::byte_offset(text, 99), text.len());
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: Handle) -> Handle;
    fn CreateDIBSection(
        hdc: Handle,
        info: *const ffi::BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    fn SelectObject(hdc: Handle, object: Handle) -> Handle;
    fn DeleteObject(object: Handle) -> i32;
    fn DeleteDC(hdc: Handle) -> i32;
}

/// Presents into an off-screen top-down bitmap and reads it back, so orientation and damage
/// clipping are checked without a visible window.
#[test]
fn present_copies_exactly_the_damage() {
    const W: u32 = 5;
    const H: u32 = 4;
    let source: Vec<u32> = (1..=W * H).collect();
    let frame = Frame::new(&source, W, H).expect("frame");

    // SAFETY: null creates a memory DC compatible with the screen.
    let dc = unsafe { CreateCompatibleDC(0) };
    assert_ne!(dc, 0);
    let info = ffi::BitmapInfo {
        header: ffi::BitmapInfoHeader {
            size: size_of::<ffi::BitmapInfoHeader>() as u32,
            width: W as i32,
            height: -(H as i32),
            planes: 1,
            bit_count: 32,
            compression: ffi::BI_RGB,
            ..Default::default()
        },
        colors: [0],
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: info is initialized and bits is a live out-pointer.
    let bitmap = unsafe { CreateDIBSection(dc, &raw const info, 0, &raw mut bits, 0, 0) };
    assert!(bitmap != 0 && !bits.is_null());
    // SAFETY: dc and bitmap are the live objects created above.
    let previous = unsafe { SelectObject(dc, bitmap) };

    let damage = [
        Rect {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
        },
        Rect {
            x: 4,
            y: 3,
            width: 9,
            height: 9,
        },
        Rect {
            x: -3,
            y: 2,
            width: 4,
            height: 1,
        },
        Rect {
            x: 50,
            y: 50,
            width: 1,
            height: 1,
        },
    ];
    present::blit(dc, frame, &damage).expect("blit");

    // SAFETY: the DIB section holds W * H zero-initialized 32-bit pixels at bits, alive until
    // DeleteObject below; GDI work is complete after the synchronous blit.
    let target = unsafe { std::slice::from_raw_parts(bits.cast::<u32>(), (W * H) as usize) };
    let mut expected = vec![0_u32; (W * H) as usize];
    for (x, y) in [(1, 0), (2, 0), (4, 3), (0, 2)] {
        let i = (y * W + x) as usize;
        expected[i] = source[i];
    }
    let actual: Vec<u32> = target.iter().map(|p| p & 0x00FF_FFFF).collect();
    assert_eq!(actual, expected);

    // SAFETY: restores and frees the objects created above, once each.
    unsafe { SelectObject(dc, previous) };
    // SAFETY: as above.
    unsafe { DeleteObject(bitmap) };
    // SAFETY: as above.
    unsafe { DeleteDC(dc) };
}

#[derive(Default)]
struct Recorder {
    events: Vec<Event>,
    started: bool,
}

impl Handler for Recorder {
    fn event(&mut self, cx: &mut Context, event: Event) {
        if !self.started && matches!(event, Event::Resized { .. }) {
            self.started = true;
            let imp = cx.imp();
            imp.send(ffi::WM_CHAR, 0x011F, 0);
            imp.send(ffi::WM_CHAR, 0xD83D, 0);
            imp.send(ffi::WM_CHAR, 0xDE00, 0);
            imp.send(ffi::WM_CHAR, 0x08, 0);
            // Repeat counts: Windows merges auto-repeats when the app falls behind.
            imp.send(ffi::WM_CHAR, 0x61, 3);
            imp.send(
                ffi::WM_KEYDOWN,
                usize::from(ffi::VK_RETURN),
                (0x1C << 16) | 2,
            );
            imp.send(ffi::WM_KEYDOWN, usize::from(ffi::VK_PROCESSKEY), 1);
            imp.send(ffi::WM_MOUSEMOVE, 0, lparam_point(-5, 7));
            imp.send(ffi::WM_MOUSEWHEEL, usize::from((-240_i16) as u16) << 16, 0);
            // Re-enters the window procedure with WM_SIZE while this handler runs.
            imp.resize_outer(640, 480);
            imp.send(ffi::WM_CLOSE, 0, 0);
        }
        if event == Event::CloseRequested {
            cx.exit();
        }
        self.events.push(event);
    }
}

#[test]
fn messages_become_events_in_order_without_reentrancy() {
    let recorder = window::run(WindowOptions::default(), Recorder::default()).expect("run");
    let events = recorder.events;
    assert!(
        matches!(events[0], Event::ScaleFactorChanged { .. }),
        "{events:?}"
    );
    assert!(matches!(events[1], Event::Resized { .. }), "{events:?}");
    let rest: Vec<_> = events[2..]
        .iter()
        .map(|e| match e {
            Event::Key(k) => format!(
                "key {:?} {:#x} {} {}",
                k.key, k.scancode, k.pressed, k.repeat
            ),
            Event::Resized { .. } => "resized".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        rest,
        [
            "Char('ğ')",
            "Char('😀')",
            "Char('a')",
            "Char('a')",
            "Char('a')",
            "key Enter 0x1c true false",
            "key Enter 0x1c true true",
            "key ImeProcess 0x0 true false",
            "PointerMoved { x: -5, y: 7 }",
            "Wheel { x: 0.0, y: -2.0 }",
            "resized",
            "CloseRequested",
        ]
    );
}

struct Waker {
    wakes: u32,
    proxy: Option<window::Proxy>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handler for Waker {
    fn event(&mut self, cx: &mut Context, event: Event) {
        match event {
            Event::Resized { .. } if self.proxy.is_none() => {
                let proxy = cx.proxy();
                self.proxy = Some(proxy.clone());
                self.thread = Some(std::thread::spawn(move || {
                    for _ in 0..3 {
                        // Later wakes may race with the window closing; only the first must land.
                        proxy.wake();
                    }
                }));
            }
            Event::Wake => {
                self.wakes += 1;
                cx.exit();
            }
            _ => {}
        }
    }
}

#[test]
fn proxies_wake_the_loop_from_other_threads() {
    let waker = Waker {
        wakes: 0,
        proxy: None,
        thread: None,
    };
    let mut waker = window::run(WindowOptions::default(), waker).expect("run");
    assert_eq!(waker.wakes, 1, "the handler exits on the first wake");
    waker
        .thread
        .take()
        .expect("thread")
        .join()
        .expect("waking thread");
    let proxy = waker.proxy.expect("proxy");
    assert!(!proxy.wake(), "a destroyed window must not accept wakes");
}

#[derive(Debug)]
struct Panicker;

impl Handler for Panicker {
    fn event(&mut self, _cx: &mut Context, _event: Event) {
        panic!("handler failed on purpose");
    }
}

#[derive(Debug)]
struct ExitAtOnce;

impl Handler for ExitAtOnce {
    fn event(&mut self, cx: &mut Context, _event: Event) {
        cx.exit();
    }
}

#[test]
fn handler_panics_reach_the_caller_and_the_thread_recovers() {
    let result = std::panic::catch_unwind(|| window::run(WindowOptions::default(), Panicker));
    let payload = result.expect_err("panic propagates");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"handler failed on purpose")
    );
    assert!(window::run(WindowOptions::default(), ExitAtOnce).is_ok());
}

#[derive(Default)]
struct Nested {
    inner: Option<Result<(), Error>>,
}

impl Handler for Nested {
    fn event(&mut self, cx: &mut Context, _event: Event) {
        if self.inner.is_none() {
            self.inner = Some(window::run(WindowOptions::default(), ExitAtOnce).map(|_| ()));
        }
        cx.exit();
    }
}

#[test]
fn one_window_per_thread() {
    let nested = window::run(WindowOptions::default(), Nested::default()).expect("run");
    assert_eq!(nested.inner, Some(Err(Error::AlreadyRunning)));
}

struct ClipboardRoundTrip {
    read: Option<String>,
}

impl Handler for ClipboardRoundTrip {
    fn event(&mut self, cx: &mut Context, _event: Event) {
        if self.read.is_none() {
            cx.set_clipboard_text("Duydaş ğİ 日本 😀")
                .expect("set clipboard");
            self.read = cx.clipboard_text();
        }
        cx.exit();
    }
}

/// Replaces the user's clipboard, so it only runs when DUYDAS_TEST_CLIPBOARD is set (CI sets it).
#[test]
fn clipboard_round_trips_unicode() {
    if std::env::var_os("DUYDAS_TEST_CLIPBOARD").is_none() {
        return;
    }
    let result =
        window::run(WindowOptions::default(), ClipboardRoundTrip { read: None }).expect("run");
    assert_eq!(result.read.as_deref(), Some("Duydaş ğİ 日本 😀"));
}

struct HiddenAnimation {
    redraws: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handler for HiddenAnimation {
    fn event(&mut self, cx: &mut Context, event: Event) {
        match event {
            Event::Resized { .. } if self.thread.is_none() => {
                let proxy = cx.proxy();
                self.thread = Some(std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    proxy.wake();
                }));
                cx.request_redraw();
            }
            Event::RedrawRequested => {
                self.redraws += 1;
                cx.request_redraw();
            }
            Event::Wake => cx.exit(),
            _ => {}
        }
    }
}

/// A handler that requests its next frame from every redraw (an animation) must not starve
/// wakes and other messages while the window is hidden.
#[test]
fn hidden_animations_let_other_messages_through() {
    let animation = HiddenAnimation {
        redraws: 0,
        thread: None,
    };
    let mut animation = window::run(WindowOptions::default(), animation).expect("run");
    animation
        .thread
        .take()
        .expect("thread")
        .join()
        .expect("waking thread");
    assert!(animation.redraws > 1, "redraws: {}", animation.redraws);
}

#[derive(Debug, Default)]
struct ForeignQuit {
    quit_sent: bool,
}

impl Handler for ForeignQuit {
    fn event(&mut self, _cx: &mut Context, _event: Event) {
        if !self.quit_sent {
            self.quit_sent = true;
            ffi::PostQuitMessage(0);
        }
    }
}

/// A WM_QUIT the window did not ask for still destroys the window, and leaves no quit message
/// behind that would end the thread's next loop at once.
#[test]
fn a_foreign_quit_tears_down_and_the_thread_runs_again() {
    let quit = window::run(WindowOptions::default(), ForeignQuit::default()).expect("run");
    assert!(quit.quit_sent);
    let recorder =
        window::run(WindowOptions::default(), CloseOnSecondEvent::default()).expect("run");
    assert!(
        recorder.events >= 2,
        "the second loop ran: {}",
        recorder.events
    );
}

#[derive(Debug, Default)]
struct CloseOnSecondEvent {
    events: u32,
}

impl Handler for CloseOnSecondEvent {
    fn event(&mut self, cx: &mut Context, _event: Event) {
        self.events += 1;
        if self.events == 1 {
            // A wake keeps the loop busy for one more round trip through GetMessageW.
            assert!(cx.proxy().wake());
        } else {
            cx.exit();
        }
    }
}

#[derive(Default)]
struct DpiChange {
    sent: bool,
    seen: Vec<(Event, (u32, u32))>,
}

impl Handler for DpiChange {
    fn event(&mut self, cx: &mut Context, event: Event) {
        if !self.sent && matches!(event, Event::Resized { .. }) {
            self.sent = true;
            let suggested = ffi::Rect {
                left: 100,
                top: 100,
                right: 1000,
                bottom: 800,
            };
            let dpi = 144_usize;
            cx.imp().send(
                ffi::WM_DPICHANGED,
                (dpi << 16) | dpi,
                (&raw const suggested) as isize,
            );
            // Exit on a wake, which arrives after everything the change queued.
            assert!(cx.proxy().wake());
            return;
        }
        if event == Event::Wake {
            cx.exit();
        } else if self.sent {
            self.seen.push((event, cx.size()));
        }
    }
}

/// The new scale arrives before the new size, and by then the window already has its new
/// size: no handler sees the new scale with stale geometry.
#[test]
fn dpi_changes_report_the_scale_with_the_new_size() {
    let change = window::run(WindowOptions::default(), DpiChange::default()).expect("run");
    let seen = change.seen;
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].0, Event::ScaleFactorChanged { scale: 1.5 });
    let Event::Resized { width, height } = seen[1].0 else {
        panic!("expected a resize: {seen:?}");
    };
    assert_eq!(seen[0].1, (width, height), "size seen with the new scale");
    assert_eq!(seen[1].1, (width, height));
}

#[derive(Default)]
struct CaptureLoss {
    started: bool,
    events: Vec<Event>,
}

impl Handler for CaptureLoss {
    fn event(&mut self, cx: &mut Context, event: Event) {
        if !self.started && matches!(event, Event::Resized { .. }) {
            self.started = true;
            let imp = cx.imp();
            imp.send(ffi::WM_LBUTTONDOWN, 0, lparam_point(10, 10));
            // Another window took the mouse mid-drag.
            imp.send(ffi::WM_CAPTURECHANGED, 0, 0);
            imp.send(ffi::WM_LBUTTONUP, 0, lparam_point(10, 10));
            assert!(cx.proxy().wake());
            return;
        }
        if event == Event::Wake {
            cx.exit();
        } else if self.started {
            self.events.push(event);
        }
    }
}

#[test]
fn losing_capture_mid_drag_is_reported() {
    let loss = window::run(WindowOptions::default(), CaptureLoss::default()).expect("run");
    let names: Vec<String> = loss
        .events
        .iter()
        .map(|e| match e {
            Event::PointerButton { pressed, .. } => format!("button pressed={pressed}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "button pressed=true",
            "PointerCaptureLost",
            "button pressed=false"
        ]
    );
}

#[derive(Debug, Default)]
struct QuitDuringAnimation {
    redraws: u32,
}

impl Handler for QuitDuringAnimation {
    fn event(&mut self, cx: &mut Context, event: Event) {
        match event {
            Event::Resized { .. } if self.redraws == 0 => cx.request_redraw(),
            Event::RedrawRequested => {
                self.redraws += 1;
                if self.redraws == 3 {
                    ffi::PostQuitMessage(0);
                }
                cx.request_redraw();
            }
            _ => {}
        }
    }
}

/// A quit message must end the loop even while a hidden window keeps requesting frames.
#[test]
fn hidden_animations_do_not_block_quit() {
    let animation =
        window::run(WindowOptions::default(), QuitDuringAnimation::default()).expect("run");
    assert!(animation.redraws >= 3, "redraws: {}", animation.redraws);
}
