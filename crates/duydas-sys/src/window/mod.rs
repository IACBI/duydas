//! A native window, its event loop, input, IME, clipboard and frame presentation.
//!
//! Duydaş is a single-window app, so this module manages exactly one window per thread. The
//! loop blocks in the OS until something happens: an idle window costs no CPU and no wakeups.
//!
//! Frames are CPU-rendered BGRX pixel buffers (`0x00RRGGBB` per pixel). [`Context::present`]
//! copies only the damaged rectangles to the screen.
//!
//! Coordinates and sizes are physical pixels; divide by [`Context::scale_factor`] for logical
//! units.
//!
//! ```no_run
//! use duydas_sys::window::{self, Context, Event, Handler, WindowOptions};
//!
//! struct App;
//!
//! impl Handler for App {
//!     fn event(&mut self, cx: &mut Context, event: Event) {
//!         if let Event::CloseRequested = event {
//!             cx.exit();
//!         }
//!     }
//! }
//!
//! let options = WindowOptions {
//!     title: "Duydaş".to_owned(),
//!     visible: true,
//!     ..WindowOptions::default()
//! };
//! window::run(options, App).expect("window");
//! ```

use std::fmt;
use std::marker::PhantomData;

#[cfg(windows)]
#[path = "win32/mod.rs"]
mod imp;

#[cfg(not(windows))]
#[path = "unsupported.rs"]
mod imp;

/// How the window starts.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowOptions {
    /// Title bar text.
    pub title: String,
    /// Initial client size in logical pixels (clamped to what the OS supports).
    pub size: (u32, u32),
    /// Smallest client size the user can resize to, in logical pixels.
    pub min_size: Option<(u32, u32)>,
    /// Show the window at once. To never show an unpainted window, start hidden and, on the
    /// first [`Event::RedrawRequested`], call [`Context::set_visible`] and then
    /// [`Context::present`] in the same event: the window appears with that frame.
    pub visible: bool,
    /// Ask the OS for a dark title bar.
    pub dark_title_bar: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            size: (1024, 720),
            min_size: Some((480, 360)),
            visible: false,
            dark_title_bar: false,
        }
    }
}

/// Receives events on the window's thread.
pub trait Handler {
    /// Handles one event. Calls on `cx` that make the OS send messages synchronously (resizing,
    /// for example) queue the resulting events; they arrive after this call returns, never
    /// re-entrantly.
    fn event(&mut self, cx: &mut Context, event: Event);
}

/// Runs the window until [`Context::exit`] is called, then returns the handler.
///
/// # Errors
/// [`Error::AlreadyRunning`] if this thread already runs a window, [`Error::Unsupported`] on
/// platforms without a backend yet, or [`Error::Os`] if the OS refuses to create the window.
///
/// # Panics
/// A panic in the handler closes the window and is resumed from this function, so it reaches the
/// caller (or a test harness) with its original message.
pub fn run<H: Handler + 'static>(options: WindowOptions, handler: H) -> Result<H, Error> {
    imp::run(&options, handler)
}

/// Access to the window from inside [`Handler::event`].
#[derive(Debug)]
pub struct Context {
    imp: imp::Context,
    /// The window belongs to one thread.
    _not_send: PhantomData<*const ()>,
}

impl Context {
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "no backend constructs a Context")
    )]
    pub(crate) fn new(imp: imp::Context) -> Self {
        Self {
            imp,
            _not_send: PhantomData,
        }
    }

    /// Client area size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.imp.size()
    }

    /// Physical pixels per logical pixel (1.0 at 96 DPI).
    pub fn scale_factor(&self) -> f32 {
        self.imp.scale_factor()
    }

    /// Asks for an [`Event::RedrawRequested`]. Requests made before the OS gets around to
    /// painting merge into one.
    pub fn request_redraw(&mut self) {
        self.imp.request_redraw();
    }

    /// Copies the damaged rectangles of `frame` to the window. Rectangles are clipped to the
    /// frame; an empty list presents nothing.
    ///
    /// # Errors
    /// [`Error::Os`] if the OS rejects the copy (for example while the session is locked).
    pub fn present(&mut self, frame: Frame<'_>, damage: &[Rect]) -> Result<(), Error> {
        self.imp.present(frame, damage)
    }

    /// Shows or hides the window.
    pub fn set_visible(&mut self, visible: bool) {
        self.imp.set_visible(visible);
    }

    /// Sets the title bar text.
    pub fn set_title(&mut self, title: &str) {
        self.imp.set_title(title);
    }

    /// Sets the mouse cursor shown over the client area.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.imp.set_cursor(cursor);
    }

    /// Enables or disables input methods, e.g. off while no text field has focus.
    ///
    /// Disabling during a composition confirms it, so the user's text is kept: its
    /// [`ImeEvent::Commit`] and [`ImeEvent::End`] arrive after the current event. Route them to
    /// the editor that was composing, which may no longer have focus.
    pub fn set_ime_allowed(&mut self, allowed: bool) {
        self.imp.set_ime_allowed(allowed);
    }

    /// Tells the input method where the text cursor is, so its candidate window opens next to
    /// it instead of covering it.
    pub fn set_ime_cursor_area(&mut self, area: Rect) {
        self.imp.set_ime_cursor_area(area);
    }

    /// The clipboard's text, if it holds any.
    pub fn clipboard_text(&mut self) -> Option<String> {
        self.imp.clipboard_text()
    }

    /// Replaces the clipboard's contents with `text`.
    ///
    /// # Errors
    /// [`Error::Os`] if the clipboard stays locked by another program or memory runs out.
    pub fn set_clipboard_text(&mut self, text: &str) -> Result<(), Error> {
        self.imp.set_clipboard_text(text)
    }

    /// A handle other threads can use to wake the loop.
    pub fn proxy(&self) -> Proxy {
        Proxy {
            imp: self.imp.proxy(),
        }
    }

    /// Closes the window and makes [`run`] return. No further events are delivered.
    pub fn exit(&mut self) {
        self.imp.exit();
    }

    #[cfg(all(test, windows))]
    pub(crate) fn imp(&self) -> &imp::Context {
        &self.imp
    }
}

/// Wakes the event loop from any thread.
#[derive(Debug, Clone)]
pub struct Proxy {
    imp: imp::Proxy,
}

impl Proxy {
    /// Delivers an [`Event::Wake`]. Wakes sent before the loop handles the first one merge
    /// into one event. Returns false once the window is gone; a wake that races with the
    /// window closing may be accepted and then dropped.
    pub fn wake(&self) -> bool {
        self.imp.wake()
    }
}

/// A frame to present: `width * height` pixels, row-major, top row first, `0x00RRGGBB`.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pixels: &'a [u32],
    width: u32,
    height: u32,
}

impl<'a> Frame<'a> {
    /// Wraps a pixel buffer, or `None` if it holds fewer than `width * height` pixels or a
    /// dimension does not fit the OS's signed 32-bit coordinates.
    pub fn new(pixels: &'a [u32], width: u32, height: u32) -> Option<Self> {
        let fits = i32::try_from(width).is_ok() && i32::try_from(height).is_ok();
        let needed = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        (fits && pixels.len() >= needed).then_some(Self {
            pixels,
            width,
            height,
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, at least `width * height` of them.
    pub fn pixels(&self) -> &'a [u32] {
        self.pixels
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Rect {
    /// The part of this rectangle inside `0..width` × `0..height`, if any.
    pub fn clip(self, width: u32, height: u32) -> Option<Rect> {
        let x0 = i64::from(self.x).max(0);
        let y0 = i64::from(self.y).max(0);
        let x1 = (i64::from(self.x) + i64::from(self.width)).min(i64::from(width));
        let y1 = (i64::from(self.y) + i64::from(self.height)).min(i64::from(height));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        // All four values lie in 0..=u32::MAX after clamping to the frame.
        Some(Rect {
            x: i32::try_from(x0).ok()?,
            y: i32::try_from(y0).ok()?,
            width: u32::try_from(x1 - x0).ok()?,
            height: u32::try_from(y1 - y0).ok()?,
        })
    }
}

/// Something that happened to the window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// The client area changed size (physical pixels). Also sent once at start.
    Resized {
        /// New width.
        width: u32,
        /// New height.
        height: u32,
    },
    /// The window moved to a display with a different scale. Also sent once at start.
    ScaleFactorChanged {
        /// Physical pixels per logical pixel.
        scale: f32,
    },
    /// The window needs to be drawn: present a full frame.
    RedrawRequested,
    /// The user asked to close the window. Call [`Context::exit`] to close it.
    CloseRequested,
    /// The window gained (true) or lost keyboard focus.
    Focused(bool),
    /// A key went down or up.
    Key(KeyEvent),
    /// A character was typed (after keyboard layout and dead keys; control characters are
    /// excluded, see [`Event::Key`] for those keys). Input-method text normally arrives as
    /// [`ImeEvent::Commit`]; input methods that type characters directly, without a
    /// composition, send it here.
    Char(char),
    /// The pointer moved inside the client area, or anywhere while a button is held.
    PointerMoved {
        /// Horizontal position.
        x: i32,
        /// Vertical position.
        y: i32,
    },
    /// The pointer left the client area.
    PointerLeft,
    /// A mouse button was pressed or released.
    PointerButton {
        /// Which button.
        button: PointerButton,
        /// Pressed (true) or released.
        pressed: bool,
        /// Horizontal position.
        x: i32,
        /// Vertical position.
        y: i32,
    },
    /// Another window or the system took the mouse while buttons were held. Treat every
    /// button as released and cancel the drag; this is not a click.
    PointerCaptureLost,
    /// The wheel or touchpad scrolled, in notches: 1.0 is one wheel detent, fractions come
    /// from high-resolution devices. Positive `y` means the wheel turned away from the user
    /// (scroll content up, toward the start); positive `x` means right. How far a notch
    /// scrolls is the toolkit's decision.
    Wheel {
        /// Horizontal notches.
        x: f32,
        /// Vertical notches.
        y: f32,
    },
    /// Input method activity.
    Ime(ImeEvent),
    /// [`Proxy::wake`] was called.
    Wake,
}

/// Input method (IME) events, e.g. Japanese kana-kanji conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImeEvent {
    /// A composition started.
    Start,
    /// The text being composed changed. Draw it inline at the text cursor, underlined; it is
    /// not part of the document yet.
    Preedit {
        /// The composition text.
        text: String,
        /// Caret position inside `text` as a byte offset, if the IME reports one.
        cursor: Option<usize>,
    },
    /// Text the user accepted; insert it.
    Commit(String),
    /// The composition ended; clear any preedit.
    End,
}

/// A key press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key, according to the active keyboard layout.
    pub key: Key,
    /// Hardware scan code (extended keys have `0xE000` set), independent of layout.
    pub scancode: u16,
    /// Pressed (true) or released.
    pub pressed: bool,
    /// An auto-repeat of a key that is held down.
    pub repeat: bool,
    /// Modifier keys held at the time.
    pub modifiers: Modifiers,
}

/// A key, as the active keyboard layout names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// A letter or digit key as a lowercase ASCII character (`'k'` for Ctrl+K on any layout
    /// that has a K key). Use [`Event::Char`] for typed text.
    Character(char),
    /// Enter or numpad Enter.
    Enter,
    /// Tab.
    Tab,
    /// Space bar.
    Space,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Function key F1–F24.
    F(u8),
    /// Either Shift key.
    Shift,
    /// Either Control key.
    Control,
    /// Either Alt key.
    Alt,
    /// The Windows / Command / Super key.
    Logo,
    /// Caps Lock.
    CapsLock,
    /// The context menu key.
    ContextMenu,
    /// The input method consumed this key (it is composing). Never treat it as a shortcut or as
    /// Enter: the user is confirming a conversion, not sending a message.
    ImeProcess,
    /// Any other key, by its platform key code.
    Other(u32),
}

/// Modifier keys held during an event. On Windows, AltGr reports Control and Alt together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub control: bool,
    /// Alt (Option on macOS).
    pub alt: bool,
    /// Windows / Command / Super.
    pub logo: bool,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// Primary (usually left).
    Left,
    /// Secondary (usually right).
    Right,
    /// Wheel button.
    Middle,
    /// Browser "back" side button.
    Back,
    /// Browser "forward" side button.
    Forward,
}

/// Mouse cursor shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Cursor {
    /// The normal arrow.
    #[default]
    Default,
    /// Text selection I-beam.
    Text,
    /// Hand, for links and buttons.
    Pointer,
    /// Horizontal resize.
    ResizeHorizontal,
    /// Vertical resize.
    ResizeVertical,
    /// The action is not allowed.
    NotAllowed,
    /// Busy.
    Wait,
}

/// Why a window operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// This platform has no window backend yet.
    Unsupported,
    /// This thread already runs a window.
    AlreadyRunning,
    /// An OS call failed.
    Os {
        /// The failing function.
        call: &'static str,
        /// The OS error code (`GetLastError` on Windows).
        code: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => f.write_str("no window backend for this platform yet"),
            Error::AlreadyRunning => f.write_str("this thread already runs a window"),
            Error::Os { call, code } => write!(f, "{call} failed with OS error {code}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_check_their_size() {
        let pixels = vec![0_u32; 12];
        assert!(Frame::new(&pixels, 4, 3).is_some());
        assert!(Frame::new(&pixels, 4, 4).is_none());
        assert!(Frame::new(&pixels, u32::MAX, 0).is_none());
        assert!(Frame::new(&[], 0, 0).is_some());
    }

    #[test]
    fn rects_clip_to_the_frame() {
        let r = Rect {
            x: -5,
            y: 2,
            width: 10,
            height: 100,
        };
        assert_eq!(
            r.clip(8, 6),
            Some(Rect {
                x: 0,
                y: 2,
                width: 5,
                height: 4
            })
        );
        let outside = Rect {
            x: 9,
            y: 0,
            width: 3,
            height: 3,
        };
        assert_eq!(outside.clip(8, 6), None);
        let huge = Rect {
            x: i32::MAX,
            y: i32::MAX,
            width: u32::MAX,
            height: u32::MAX,
        };
        assert_eq!(huge.clip(8, 6), None);
    }
}
