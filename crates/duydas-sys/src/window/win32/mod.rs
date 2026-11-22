//! Win32 backend: one window, a blocking `GetMessageW` loop and a window procedure that turns
//! messages into [`Event`]s.
//!
//! Events are queued and delivered by `pump`. Win32 calls the handler makes can re-enter the
//! window procedure (resizing sends `WM_SIZE` synchronously, for example); those events wait in
//! the queue until the handler returns. The window procedure also pumps, so events keep flowing
//! inside the OS's modal loops (while the user drags the window border).

mod clipboard;
mod ffi;
mod ime;
mod keys;
mod present;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use self::ffi::Handle;
use super::{
    Cursor, Error, Event, Frame, Handler, ImeEvent, KeyEvent, PointerButton, Rect, WindowOptions,
};

const WM_APP_WAKE: u32 = ffi::WM_APP;
const WM_APP_EXIT: u32 = ffi::WM_APP + 1;

/// Candidate list indices 0–3 as a mask, for placing every list an IME may open.
const ALL_CANDIDATE_LISTS: usize = 0b1111;

/// USER limits windows to about 32k pixels per side; clamping to it keeps frame arithmetic in
/// range.
const MAX_WINDOW_EXTENT: f32 = 32_767.0;

/// "DuydasWindow" as a NUL-terminated UTF-16 string.
static CLASS_NAME: [u16; 13] = ascii_wide("DuydasWindow");

const fn ascii_wide<const N: usize>(text: &str) -> [u16; N] {
    let bytes = text.as_bytes();
    let mut out = [0_u16; N];
    let mut i = 0;
    while i < bytes.len() {
        out[i] = bytes[i] as u16;
        i += 1;
    }
    out
}

/// A handler with its concrete type erased, so it can live in thread-local state and be handed
/// back to the caller of `run`.
trait ErasedHandler {
    fn event(&mut self, cx: &mut super::Context, event: Event);
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<H: Handler + 'static> ErasedHandler for H {
    fn event(&mut self, cx: &mut super::Context, event: Event) {
        Handler::event(self, cx, event);
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Per-thread window state. Never hold a `RefCell` borrow across a Win32 call: the call can
/// re-enter the window procedure.
struct State {
    /// A `run` is active on this thread, from entry until teardown has finished.
    active: Cell<bool>,
    hwnd: Cell<Handle>,
    /// Creation finished; events may be delivered.
    ready: Cell<bool>,
    /// The message loop runs, so a posted exit is picked up and WM_DESTROY should end it.
    looping: Cell<bool>,
    dispatching: Cell<bool>,
    /// Non-zero while a multi-message OS operation runs whose events must arrive together.
    hold: Cell<u32>,
    exiting: Cell<bool>,
    queue: RefCell<VecDeque<Event>>,
    handler: RefCell<Option<Box<dyn ErasedHandler>>>,
    panic: RefCell<Option<Box<dyn Any + Send>>>,
    size: Cell<(u32, u32)>,
    reported_size: Cell<Option<(u32, u32)>>,
    dpi: Cell<u32>,
    min_size: Cell<Option<(u32, u32)>>,
    cursor: Cell<Cursor>,
    ime_allowed: Cell<bool>,
    ime_area: Cell<Option<Rect>>,
    composing: Cell<bool>,
    high_surrogate: Cell<Option<u16>>,
    pointer_inside: Cell<bool>,
    pointer: Cell<(i32, i32)>,
    /// Held mouse buttons, one bit per [`PointerButton`].
    buttons: Cell<u8>,
    /// A hidden window asked for a redraw; the loop delivers it once nothing else is waiting.
    redraw_deferred: Cell<bool>,
    reveal_pending: Cell<bool>,
    wake: RefCell<Option<Arc<WakeTarget>>>,
}

impl State {
    const fn new() -> Self {
        Self {
            active: Cell::new(false),
            hwnd: Cell::new(0),
            ready: Cell::new(false),
            looping: Cell::new(false),
            dispatching: Cell::new(false),
            hold: Cell::new(0),
            exiting: Cell::new(false),
            queue: RefCell::new(VecDeque::new()),
            handler: RefCell::new(None),
            panic: RefCell::new(None),
            size: Cell::new((0, 0)),
            reported_size: Cell::new(None),
            dpi: Cell::new(ffi::USER_DEFAULT_SCREEN_DPI),
            min_size: Cell::new(None),
            cursor: Cell::new(Cursor::Default),
            ime_allowed: Cell::new(true),
            ime_area: Cell::new(None),
            composing: Cell::new(false),
            high_surrogate: Cell::new(None),
            pointer_inside: Cell::new(false),
            pointer: Cell::new((0, 0)),
            buttons: Cell::new(0),
            redraw_deferred: Cell::new(false),
            reveal_pending: Cell::new(false),
            wake: RefCell::new(None),
        }
    }

    fn reset(&self) {
        self.hwnd.set(0);
        self.ready.set(false);
        self.looping.set(false);
        self.dispatching.set(false);
        self.hold.set(0);
        self.exiting.set(false);
        self.queue.borrow_mut().clear();
        self.size.set((0, 0));
        self.reported_size.set(None);
        self.dpi.set(ffi::USER_DEFAULT_SCREEN_DPI);
        self.min_size.set(None);
        self.cursor.set(Cursor::Default);
        self.ime_allowed.set(true);
        self.ime_area.set(None);
        self.composing.set(false);
        self.high_surrogate.set(None);
        self.pointer_inside.set(false);
        self.pointer.set((0, 0));
        self.buttons.set(0);
        self.redraw_deferred.set(false);
        self.reveal_pending.set(false);
        *self.wake.borrow_mut() = None;
    }

    fn contains(&self, (x, y): (i32, i32)) -> bool {
        let (width, height) = self.size.get();
        u32::try_from(x).is_ok_and(|x| x < width) && u32::try_from(y).is_ok_and(|y| y < height)
    }

    fn scale(&self) -> f32 {
        self.dpi.get() as f32 / ffi::USER_DEFAULT_SCREEN_DPI as f32
    }

    fn push(&self, event: Event) {
        if self.exiting.get() {
            return;
        }
        let mut queue = self.queue.borrow_mut();
        // Live resizing produces bursts of sizes; only the latest matters.
        if let (Event::Resized { .. }, Some(Event::Resized { .. })) = (&event, queue.back()) {
            queue.pop_back();
        }
        queue.push_back(event);
    }

    fn push_resize(&self) {
        let size = self.size.get();
        if self.reported_size.get() != Some(size) {
            self.reported_size.set(Some(size));
            self.push(Event::Resized {
                width: size.0,
                height: size.1,
            });
        }
    }

    /// Delivers queued events unless a delivery is already running further up the stack.
    fn pump(&self) {
        if !self.ready.get() || self.dispatching.get() || self.hold.get() > 0 {
            return;
        }
        self.dispatching.set(true);
        loop {
            if self.exiting.get() {
                self.queue.borrow_mut().clear();
                break;
            }
            let Some(event) = self.queue.borrow_mut().pop_front() else {
                break;
            };
            let Some(mut handler) = self.handler.borrow_mut().take() else {
                break;
            };
            let mut cx = super::Context::new(Context {
                hwnd: self.hwnd.get(),
            });
            let result = panic::catch_unwind(AssertUnwindSafe(|| handler.event(&mut cx, event)));
            *self.handler.borrow_mut() = Some(handler);
            // Not when the handler closed the window or panicked: it is about to be destroyed.
            if self.reveal_pending.replace(false) && !self.exiting.get() {
                reveal(self.hwnd.get());
            }
            if let Err(payload) = result {
                self.fail(payload);
            }
        }
        self.dispatching.set(false);
    }

    /// Records the first panic and closes the window; `run` resumes the panic.
    fn fail(&self, payload: Box<dyn Any + Send>) {
        let mut slot = self.panic.borrow_mut();
        if slot.is_none() {
            *slot = Some(payload);
        } else {
            // Keep the first panic. Dropping a second payload can itself panic, which inside
            // the window procedure would abort the process, so leak it.
            std::mem::forget(payload);
        }
        drop(slot);
        self.request_exit();
    }

    fn request_exit(&self) {
        if self.exiting.replace(true) {
            return;
        }
        self.queue.borrow_mut().clear();
        if !self.looping.get() {
            // Still creating the window: run sees `exiting` and tears down.
            return;
        }
        // Destroying through a posted message (not here, possibly deep inside a handler) also
        // ends a modal resize loop cleanly.
        // SAFETY: plain values; if the window is already gone the call fails harmlessly.
        let posted = unsafe { ffi::PostMessageW(self.hwnd.get(), WM_APP_EXIT, 0, 0) };
        if posted == 0 {
            // The queue is full. The quit flag needs no queue space; run destroys the window
            // after the loop ends.
            ffi::PostQuitMessage(0);
        }
    }
}

thread_local! {
    static STATE: State = const { State::new() };
}

pub(crate) fn run<H: Handler + 'static>(options: &WindowOptions, handler: H) -> Result<H, Error> {
    STATE.with(|state| {
        if state.active.replace(true) {
            return Err(Error::AlreadyRunning);
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            run_erased(state, options, Box::new(handler))
        }));
        state.active.set(false);
        let handler = result.unwrap_or_else(|payload| panic::resume_unwind(payload))?;
        let handler = handler
            .into_any()
            .downcast::<H>()
            .unwrap_or_else(|_| unreachable!("the handler passed in is handed back"));
        Ok(*handler)
    })
}

fn run_erased(
    state: &State,
    options: &WindowOptions,
    handler: Box<dyn ErasedHandler>,
) -> Result<Box<dyn ErasedHandler>, Error> {
    let class = register_class()?;
    // Scoped to this thread rather than set process-wide, so it holds even when a manifest or
    // a host already chose another awareness. Windows created now are per-monitor v2.
    // SAFETY: a documented pseudo-handle; returns the previous context, or null if unsupported.
    let previous_dpi_context =
        unsafe { ffi::SetThreadDpiAwarenessContext(ffi::DPI_PER_MONITOR_AWARE_V2) };
    if previous_dpi_context == 0 {
        return Err(os_error("SetThreadDpiAwarenessContext"));
    }

    state.reset();
    *state.handler.borrow_mut() = Some(handler);
    state.min_size.set(options.min_size);
    let wake = Arc::new(WakeTarget::default());
    *state.wake.borrow_mut() = Some(Arc::clone(&wake));

    let title = wide(&options.title);
    // SAFETY: class and title are NUL-terminated and outlive the call; the remaining arguments
    // are plain values or null.
    let hwnd = unsafe {
        ffi::CreateWindowExW(
            0,
            class,
            title.as_ptr(),
            ffi::WS_OVERLAPPEDWINDOW,
            ffi::CW_USEDEFAULT,
            ffi::CW_USEDEFAULT,
            ffi::CW_USEDEFAULT,
            ffi::CW_USEDEFAULT,
            0,
            0,
            instance(),
            ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        let error = os_error("CreateWindowExW");
        return finish(state, previous_dpi_context, Err(error));
    }
    state.hwnd.set(hwnd);
    wake.lock().hwnd = hwnd;

    // SAFETY: hwnd is the live window created above.
    let dpi = unsafe { ffi::GetDpiForWindow(hwnd) };
    if dpi != 0 {
        state.dpi.set(dpi);
    }
    let (width, height) = outer_size(state, options.size);
    // SAFETY: hwnd is live; the flags keep position and z-order.
    let sized = unsafe {
        ffi::SetWindowPos(
            hwnd,
            0,
            0,
            0,
            width,
            height,
            ffi::SWP_NOMOVE | ffi::SWP_NOZORDER | ffi::SWP_NOACTIVATE,
        )
    };
    if sized == 0 {
        let error = os_error("SetWindowPos");
        return finish(state, previous_dpi_context, Err(error));
    }
    if options.dark_title_bar {
        let enabled: i32 = 1;
        // SAFETY: the value points to a live i32 whose size we pass; older Windows versions
        // reject the attribute, which is harmless.
        unsafe {
            ffi::DwmSetWindowAttribute(
                hwnd,
                ffi::DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const enabled).cast(),
                4,
            )
        };
    }
    state.size.set(client_size(hwnd));
    if state.exiting.get() {
        // Our window procedure panicked during creation.
        return finish(state, previous_dpi_context, Ok(()));
    }

    // Start with exactly one scale and one size event, whatever creation produced.
    state.queue.borrow_mut().clear();
    state.reported_size.set(None);
    state.push(Event::ScaleFactorChanged {
        scale: state.scale(),
    });
    state.push_resize();
    state.ready.set(true);
    state.looping.set(true);
    if options.visible {
        // SAFETY: hwnd is live.
        unsafe { ffi::ShowWindow(hwnd, ffi::SW_SHOW) };
    }
    state.pump();

    let mut msg = ffi::Msg::default();
    let outcome = loop {
        if state.redraw_deferred.get() {
            // SAFETY: msg is a live, writable MSG; PM_NOREMOVE only looks.
            let waiting = unsafe { ffi::PeekMessageW(&raw mut msg, 0, 0, 0, ffi::PM_NOREMOVE) };
            if waiting == 0 {
                // Nothing else is waiting (input, wakes, a quit): an animating hidden window
                // never starves them.
                state.redraw_deferred.set(false);
                state.push(Event::RedrawRequested);
                state.pump();
                continue;
            }
        }
        // SAFETY: msg is a live, writable MSG.
        let result = unsafe { ffi::GetMessageW(&raw mut msg, 0, 0, 0) };
        if result == 0 {
            break Ok(());
        }
        if result == -1 {
            break Err(os_error("GetMessageW"));
        }
        // SAFETY: msg was filled in by GetMessageW.
        unsafe { ffi::TranslateMessage(&raw const msg) };
        // SAFETY: as above; dispatches to our window procedure.
        unsafe { ffi::DispatchMessageW(&raw const msg) };
    };
    finish(state, previous_dpi_context, outcome)
}

/// Destroys the window if it still exists (the loop can also end through a foreign WM_QUIT),
/// returns the thread to its idle state and hands back the handler, or resumes its panic.
fn finish(
    state: &State,
    previous_dpi_context: Handle,
    outcome: Result<(), Error>,
) -> Result<Box<dyn ErasedHandler>, Error> {
    state.looping.set(false);
    state.exiting.set(true);
    let hwnd = state.hwnd.get();
    if hwnd != 0 {
        // SAFETY: our window, destroyed on its owner thread. With `looping` off, WM_DESTROY
        // posts no WM_QUIT that could end this thread's next loop early.
        unsafe { ffi::DestroyWindow(hwnd) };
    }
    if let Some(wake) = state.wake.borrow().as_ref() {
        wake.lock().hwnd = 0;
    }
    // SAFETY: restores the context returned by SetThreadDpiAwarenessContext in run_erased.
    unsafe { ffi::SetThreadDpiAwarenessContext(previous_dpi_context) };
    let handler = state.handler.borrow_mut().take();
    let payload = state.panic.borrow_mut().take();
    state.reset();
    if let Some(payload) = payload {
        drop_quietly(handler);
        panic::resume_unwind(payload);
    }
    outcome?;
    Ok(handler.unwrap_or_else(|| unreachable!("the handler is only taken while it runs")))
}

/// Drops a value whose destructor might panic without letting that panic replace the one
/// being resumed.
fn drop_quietly<T>(value: T) {
    if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(move || drop(value))) {
        std::mem::forget(payload);
    }
}

fn register_class() -> Result<*const u16, Error> {
    static ATOM: OnceLock<Result<u16, Error>> = OnceLock::new();
    let atom = ATOM.get_or_init(|| {
        // SAFETY: IDC_ARROW is a system cursor id passed as MAKEINTRESOURCE.
        let cursor = unsafe { ffi::LoadCursorW(0, ptr::without_provenance(ffi::IDC_ARROW)) };
        let class = ffi::WndClassExW {
            size: size_of::<ffi::WndClassExW>() as u32,
            style: 0,
            wnd_proc: Some(window_proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance: instance(),
            icon: 0,
            cursor,
            background: 0,
            menu_name: ptr::null(),
            class_name: CLASS_NAME.as_ptr(),
            icon_small: 0,
        };
        // SAFETY: class is fully initialized and CLASS_NAME is a static NUL-terminated string.
        let atom = unsafe { ffi::RegisterClassExW(&raw const class) };
        if atom == 0 {
            Err(os_error("RegisterClassExW"))
        } else {
            Ok(atom)
        }
    });
    atom.clone().map(|_| CLASS_NAME.as_ptr())
}

fn instance() -> Handle {
    // SAFETY: null asks for the executable's own module handle, which is never freed.
    unsafe { ffi::GetModuleHandleW(ptr::null()) }
}

fn os_error(call: &'static str) -> Error {
    Error::Os {
        call,
        code: ffi::GetLastError(),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .map(|unit| if unit == 0 { 0xFFFD } else { unit })
        .chain(std::iter::once(0))
        .collect()
}

fn client_size(hwnd: Handle) -> (u32, u32) {
    let mut rect = ffi::Rect::default();
    // SAFETY: rect is a live, writable RECT.
    let ok = unsafe { ffi::GetClientRect(hwnd, &raw mut rect) };
    if ok == 0 {
        return (0, 0);
    }
    (
        u32::try_from(rect.right - rect.left).unwrap_or(0),
        u32::try_from(rect.bottom - rect.top).unwrap_or(0),
    )
}

/// Window size (frame included) for a client size in logical pixels at the current DPI.
fn outer_size(state: &State, logical: (u32, u32)) -> (i32, i32) {
    let scale = state.scale();
    let to_physical = |v: u32| (v as f32 * scale).round().clamp(0.0, MAX_WINDOW_EXTENT) as i32;
    let mut rect = ffi::Rect {
        left: 0,
        top: 0,
        right: to_physical(logical.0),
        bottom: to_physical(logical.1),
    };
    let client = (rect.right, rect.bottom);
    // SAFETY: rect is a live, writable RECT; the other arguments are plain values.
    let adjusted = unsafe {
        ffi::AdjustWindowRectExForDpi(
            &raw mut rect,
            ffi::WS_OVERLAPPEDWINDOW,
            0,
            0,
            state.dpi.get(),
        )
    };
    if adjusted == 0 {
        return client;
    }
    // Both sides are at most MAX_WINDOW_EXTENT plus a frame of a few dozen pixels.
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn loword(value: usize) -> u16 {
    (value & 0xFFFF) as u16
}

fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

/// Signed client coordinates packed in an LPARAM (GET_X_LPARAM / GET_Y_LPARAM).
fn point_from_lparam(lparam: isize) -> (i32, i32) {
    let bits = lparam as usize;
    (
        i32::from(loword(bits) as i16),
        i32::from(hiword(bits) as i16),
    )
}

unsafe extern "system" fn window_proc(
    hwnd: Handle,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    // A panic must never unwind into user32; ours (not the handler's, which pump catches) is
    // recorded and resumed by run like a handler panic.
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        STATE.with(|state| handle(state, hwnd, msg, wparam, lparam))
    }));
    match result {
        Ok(Some(value)) => value,
        Ok(None) => {
            // SAFETY: forwarding the message we received, unchanged.
            unsafe { ffi::DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        Err(payload) => {
            STATE.with(|state| state.fail(payload));
            0
        }
    }
}

/// Handles one message; `None` means "let DefWindowProcW handle it".
fn handle(state: &State, hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> Option<isize> {
    if msg == ffi::WM_NCCREATE {
        state.hwnd.set(hwnd);
        return None;
    }
    if hwnd != state.hwnd.get() {
        return None;
    }
    if msg == ffi::WM_NCDESTROY {
        // The last message: from here on no proxy can reach this handle, which Windows may
        // reuse for another window.
        if let Some(wake) = state.wake.borrow().as_ref() {
            wake.lock().hwnd = 0;
        }
        state.hwnd.set(0);
        return None;
    }
    let result = match msg {
        ffi::WM_SIZE => {
            if wparam != ffi::SIZE_MINIMIZED {
                // LPARAM holds 16-bit sizes; read the exact client size instead.
                state.size.set(client_size(hwnd));
                state.push_resize();
            }
            Some(0)
        }
        ffi::WM_PAINT => {
            let mut paint = ffi::PaintStruct::default();
            // SAFETY: paint is a live, writable PAINTSTRUCT; validating the update region here
            // keeps Windows from sending WM_PAINT again.
            unsafe { ffi::BeginPaint(hwnd, &raw mut paint) };
            // SAFETY: paint was filled in by BeginPaint.
            unsafe { ffi::EndPaint(hwnd, &raw const paint) };
            state.push(Event::RedrawRequested);
            Some(0)
        }
        // Every frame covers the whole client area: never let Windows clear it (flicker).
        ffi::WM_ERASEBKGND => Some(1),
        ffi::WM_CLOSE => {
            state.push(Event::CloseRequested);
            Some(0)
        }
        ffi::WM_DESTROY => {
            if state.looping.get() {
                ffi::PostQuitMessage(0);
            }
            Some(0)
        }
        ffi::WM_SETFOCUS => {
            state.push(Event::Focused(true));
            Some(0)
        }
        ffi::WM_KILLFOCUS => {
            state.high_surrogate.set(None);
            state.push(Event::Focused(false));
            Some(0)
        }
        ffi::WM_KEYDOWN | ffi::WM_SYSKEYDOWN | ffi::WM_KEYUP | ffi::WM_SYSKEYUP => {
            let pressed = matches!(msg, ffi::WM_KEYDOWN | ffi::WM_SYSKEYDOWN);
            let event = keys::key_event(wparam, lparam, pressed, keys::modifiers());
            state.push(Event::Key(event));
            // A slow handler lets Windows merge auto-repeats into one message with a count.
            if pressed {
                for _ in 1..repeat_count(lparam) {
                    state.push(Event::Key(KeyEvent {
                        repeat: true,
                        ..event
                    }));
                }
            }
            // Default processing keeps Alt+F4, Alt+Space, F10 and Alt menu activation working.
            None
        }
        ffi::WM_CHAR => {
            if let Some(c) = keys::utf16_char(&state.high_surrogate, loword(wparam))
                && !c.is_control()
            {
                for _ in 0..repeat_count(lparam) {
                    state.push(Event::Char(c));
                }
            }
            Some(0)
        }
        // Alt+letter is a shortcut, not text. Alt+Space still opens the window menu; other
        // mnemonics are swallowed so Windows does not beep for a menu bar we do not have.
        ffi::WM_SYSCHAR => (wparam != usize::from(ffi::VK_SPACE)).then_some(0),
        ffi::WM_MOUSEMOVE => {
            let point = point_from_lparam(lparam);
            state.pointer.set(point);
            // While a button is held the pointer is captured and can be outside the client
            // area; leave tracking is armed only from inside.
            if state.contains(point) && !state.pointer_inside.replace(true) {
                let mut track = ffi::TrackMouseEvent {
                    size: size_of::<ffi::TrackMouseEvent>() as u32,
                    flags: ffi::TME_LEAVE,
                    hwnd,
                    hover_time: 0,
                };
                // SAFETY: track is a live, initialized TRACKMOUSEEVENT.
                unsafe { ffi::TrackMouseEvent(&raw mut track) };
            }
            let (x, y) = point;
            state.push(Event::PointerMoved { x, y });
            Some(0)
        }
        ffi::WM_MOUSELEAVE => {
            if state.pointer_inside.replace(false) {
                state.push(Event::PointerLeft);
            }
            Some(0)
        }
        ffi::WM_LBUTTONDOWN
        | ffi::WM_LBUTTONUP
        | ffi::WM_RBUTTONDOWN
        | ffi::WM_RBUTTONUP
        | ffi::WM_MBUTTONDOWN
        | ffi::WM_MBUTTONUP
        | ffi::WM_XBUTTONDOWN
        | ffi::WM_XBUTTONUP => {
            let (button, pressed) = match msg {
                ffi::WM_LBUTTONDOWN => (PointerButton::Left, true),
                ffi::WM_LBUTTONUP => (PointerButton::Left, false),
                ffi::WM_RBUTTONDOWN => (PointerButton::Right, true),
                ffi::WM_RBUTTONUP => (PointerButton::Right, false),
                ffi::WM_MBUTTONDOWN => (PointerButton::Middle, true),
                ffi::WM_MBUTTONUP => (PointerButton::Middle, false),
                _ => {
                    let back = hiword(wparam) == ffi::XBUTTON1;
                    let button = if back {
                        PointerButton::Back
                    } else {
                        PointerButton::Forward
                    };
                    (button, msg == ffi::WM_XBUTTONDOWN)
                }
            };
            // Capture keeps drags (text selection, scrollbars) working outside the window.
            let bit = button_bit(button);
            let held = state.buttons.get();
            let point = point_from_lparam(lparam);
            state.pointer.set(point);
            let (x, y) = point;
            state.push(Event::PointerButton {
                button,
                pressed,
                x,
                y,
            });
            if pressed {
                state.buttons.set(held | bit);
                if held == 0 {
                    // SAFETY: hwnd is our live window.
                    unsafe { ffi::SetCapture(hwnd) };
                }
            } else {
                let rest = held & !bit;
                // Update first: releasing capture sends WM_CAPTURECHANGED synchronously.
                state.buttons.set(rest);
                if held != 0 && rest == 0 && ffi::GetCapture() == hwnd {
                    ffi::ReleaseCapture();
                }
                if rest == 0 && !state.contains(point) && state.pointer_inside.replace(false) {
                    state.push(Event::PointerLeft);
                }
            }
            // XBUTTON messages must return TRUE.
            Some(isize::from(matches!(
                msg,
                ffi::WM_XBUTTONDOWN | ffi::WM_XBUTTONUP
            )))
        }
        ffi::WM_CAPTURECHANGED => {
            // Another window (or the system menu) took the mouse mid-drag: the releases will
            // never arrive here.
            if state.buttons.replace(0) != 0 && lparam != hwnd {
                state.push(Event::PointerCaptureLost);
            }
            Some(0)
        }
        ffi::WM_MOUSEWHEEL | ffi::WM_MOUSEHWHEEL => {
            let notches = f32::from(hiword(wparam) as i16) / ffi::WHEEL_DELTA;
            state.push(if msg == ffi::WM_MOUSEWHEEL {
                Event::Wheel { x: 0.0, y: notches }
            } else {
                Event::Wheel { x: notches, y: 0.0 }
            });
            Some(0)
        }
        ffi::WM_SETCURSOR if loword(lparam as usize) == ffi::HTCLIENT => {
            set_os_cursor(state.cursor.get());
            Some(1)
        }
        ffi::WM_GETMINMAXINFO => {
            if let Some(min) = state.min_size.get() {
                let (width, height) = outer_size(state, min);
                let info = lparam as *mut ffi::MinMaxInfo;
                // SAFETY: for WM_GETMINMAXINFO, lparam points to a MINMAXINFO the system owns
                // for the duration of the message.
                unsafe {
                    (*info).min_track_size = ffi::Point {
                        x: width,
                        y: height,
                    };
                }
            }
            Some(0)
        }
        ffi::WM_DPICHANGED => {
            state.dpi.set(u32::from(loword(wparam)).max(1));
            state.push(Event::ScaleFactorChanged {
                scale: state.scale(),
            });
            // SAFETY: for WM_DPICHANGED, lparam points to the suggested window RECT.
            let suggested = unsafe { *(lparam as *const ffi::Rect) };
            // Resizing sends messages synchronously. Hold delivery until the new size is known,
            // so no handler sees the new scale with the old size.
            state.hold.set(state.hold.get() + 1);
            // SAFETY: hwnd is live; the rect comes from the system.
            unsafe {
                ffi::SetWindowPos(
                    hwnd,
                    0,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    ffi::SWP_NOZORDER | ffi::SWP_NOACTIVATE,
                )
            };
            state.hold.set(state.hold.get() - 1);
            state.size.set(client_size(hwnd));
            state.push_resize();
            Some(0)
        }
        ffi::WM_IME_SETCONTEXT if wparam != 0 && state.ime_allowed.get() => {
            // We draw the composition inline, so hide the IME's own composition window but keep
            // its candidate list.
            let lparam = (lparam as usize & !ffi::ISC_SHOWUICOMPOSITIONWINDOW) as isize;
            // SAFETY: forwarding the message with one documented flag cleared.
            Some(unsafe { ffi::DefWindowProcW(hwnd, msg, wparam, lparam) })
        }
        ffi::WM_IME_STARTCOMPOSITION if state.ime_allowed.get() => {
            state.composing.set(true);
            if let Some(area) = state.ime_area.get() {
                ime::set_cursor_area(hwnd, area, ALL_CANDIDATE_LISTS);
            }
            state.push(Event::Ime(ImeEvent::Start));
            Some(0)
        }
        ffi::WM_IME_COMPOSITION if state.ime_allowed.get() => {
            let composition = ime::composition_events(hwnd, lparam as u32);
            for event in composition.events {
                state.push(Event::Ime(event));
            }
            // Handling a result here (instead of DefWindowProcW) stops Windows from re-sending
            // it as WM_IME_CHAR/WM_CHAR. If it could not be read, that resend is the fallback.
            if composition.unread_result {
                None
            } else {
                Some(0)
            }
        }
        ffi::WM_IME_ENDCOMPOSITION if state.ime_allowed.get() => {
            state.composing.set(false);
            state.push(Event::Ime(ImeEvent::End));
            Some(0)
        }
        ffi::WM_IME_NOTIFY if wparam == ffi::IMN_OPENCANDIDATE && state.ime_allowed.get() => {
            // lparam is the mask of candidate lists being opened.
            if let Some(area) = state.ime_area.get() {
                ime::set_cursor_area(hwnd, area, lparam as usize);
            }
            None
        }
        WM_APP_WAKE => {
            if let Some(wake) = state.wake.borrow().as_ref() {
                wake.lock().pending = false;
            }
            state.push(Event::Wake);
            Some(0)
        }
        WM_APP_EXIT => {
            // SAFETY: hwnd is our live window; WM_DESTROY then posts WM_QUIT.
            if unsafe { ffi::DestroyWindow(hwnd) } == 0 {
                // End the loop anyway; run retries the destruction.
                ffi::PostQuitMessage(0);
            }
            Some(0)
        }
        _ => None,
    };
    state.pump();
    result
}

fn repeat_count(lparam: isize) -> u16 {
    loword(lparam as usize).max(1)
}

fn button_bit(button: PointerButton) -> u8 {
    match button {
        PointerButton::Left => 1,
        PointerButton::Right => 1 << 1,
        PointerButton::Middle => 1 << 2,
        PointerButton::Back => 1 << 3,
        PointerButton::Forward => 1 << 4,
    }
}

/// Hides (or reveals) the window's content without hiding the window, so it can be shown and
/// painted before anyone sees it. Returns false where DWM refuses.
fn set_cloaked(hwnd: Handle, cloaked: bool) -> bool {
    let value = i32::from(cloaked);
    // SAFETY: value is a live BOOL whose size we pass.
    let result =
        unsafe { ffi::DwmSetWindowAttribute(hwnd, ffi::DWMWA_CLOAK, (&raw const value).cast(), 4) };
    result >= 0
}

/// Uncloaks a window shown by `set_visible` and activates it, as SW_SHOW would have.
fn reveal(hwnd: Handle) {
    if !set_cloaked(hwnd, false) {
        // Cloaking just succeeded, so this is not expected; never leave the window invisible.
        // SAFETY: hwnd is live; hiding drops the cloak's purpose, showing again is plain.
        unsafe { ffi::ShowWindow(hwnd, ffi::SW_HIDE) };
        // SAFETY: as above.
        unsafe { ffi::ShowWindow(hwnd, ffi::SW_SHOW) };
        return;
    }
    // SAFETY: plain handle value. Fails (harmlessly) when another app holds the foreground,
    // exactly when SW_SHOW would not have activated the window either.
    unsafe { ffi::SetForegroundWindow(hwnd) };
}

fn is_visible(hwnd: Handle) -> bool {
    // SAFETY: plain handle value; returns false for a destroyed window.
    unsafe { ffi::IsWindowVisible(hwnd) != 0 }
}

fn set_os_cursor(cursor: Cursor) {
    let id = match cursor {
        Cursor::Default => ffi::IDC_ARROW,
        Cursor::Text => ffi::IDC_IBEAM,
        Cursor::Pointer => ffi::IDC_HAND,
        Cursor::ResizeHorizontal => ffi::IDC_SIZEWE,
        Cursor::ResizeVertical => ffi::IDC_SIZENS,
        Cursor::NotAllowed => ffi::IDC_NO,
        Cursor::Wait => ffi::IDC_WAIT,
    };
    // SAFETY: a system cursor id passed as MAKEINTRESOURCE; system cursors are shared and never
    // freed.
    let handle = unsafe { ffi::LoadCursorW(0, ptr::without_provenance(id)) };
    // SAFETY: handle is a valid cursor (or null, which hides the cursor).
    unsafe { ffi::SetCursor(handle) };
}

/// The backend half of [`super::Context`]: the window handle, valid while the loop runs.
#[derive(Debug)]
pub(crate) struct Context {
    hwnd: Handle,
}

impl Context {
    fn with_state<T>(&self, f: impl FnOnce(&State) -> T) -> T {
        STATE.with(f)
    }

    fn alive(&self) -> bool {
        self.with_state(|s| s.hwnd.get() == self.hwnd && !s.exiting.get())
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.with_state(|s| s.size.get())
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.with_state(State::scale)
    }

    pub(crate) fn request_redraw(&mut self) {
        if !self.alive() {
            return;
        }
        if is_visible(self.hwnd) {
            // SAFETY: hwnd is live; a null rect invalidates the whole client area.
            unsafe { ffi::InvalidateRect(self.hwnd, ptr::null(), 0) };
        } else {
            // Windows never paints a hidden window, but the first frame is drawn before showing
            // it. The loop delivers the request when no other message is waiting.
            self.with_state(|s| s.redraw_deferred.set(true));
        }
    }

    pub(crate) fn present(&mut self, frame: Frame<'_>, damage: &[Rect]) -> Result<(), Error> {
        if !self.alive() {
            return Ok(());
        }
        match present::present_to_window(self.hwnd, frame, damage) {
            // Nothing reaches the screen while hidden; showing the window triggers a redraw.
            Err(_) if !is_visible(self.hwnd) => Ok(()),
            result => result,
        }
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        if !self.alive() || visible == is_visible(self.hwnd) {
            return;
        }
        let cmd = if !visible {
            if self.with_state(|s| s.reveal_pending.replace(false)) {
                set_cloaked(self.hwnd, false);
            }
            ffi::SW_HIDE
        } else if set_cloaked(self.hwnd, true) {
            // Shown cloaked and inactive until the current event is handled: a present made
            // right after this call lands before the window appears, so no unpainted frame is
            // ever on screen. Activation (focus, input method setup) costs about as much as
            // everything before it, so it waits until the frame is up.
            self.with_state(|s| s.reveal_pending.set(true));
            ffi::SW_SHOWNA
        } else {
            ffi::SW_SHOW
        };
        // SAFETY: hwnd is live.
        unsafe { ffi::ShowWindow(self.hwnd, cmd) };
    }

    pub(crate) fn set_title(&mut self, title: &str) {
        if self.alive() {
            let title = wide(title);
            // SAFETY: title is NUL-terminated and outlives the call.
            unsafe { ffi::SetWindowTextW(self.hwnd, title.as_ptr()) };
        }
    }

    pub(crate) fn set_cursor(&mut self, cursor: Cursor) {
        let inside = self.with_state(|s| {
            s.cursor.set(cursor);
            s.pointer_inside.get()
        });
        if inside && self.alive() {
            set_os_cursor(cursor);
        }
    }

    pub(crate) fn set_ime_allowed(&mut self, allowed: bool) {
        if !self.alive() || self.with_state(|s| s.ime_allowed.get()) == allowed {
            return;
        }
        if !allowed && self.with_state(|s| s.composing.get()) {
            // Keep what the user typed: confirm the composition before detaching the IME. Its
            // commit and end arrive through the usual messages, after the current event.
            ime::complete_composition(self.hwnd);
        }
        let flags = if allowed { ffi::IACE_DEFAULT } else { 0 };
        // SAFETY: hwnd is live; a null context with these flags restores the default context or
        // removes it (disabling the IME for this window).
        if unsafe { ffi::ImmAssociateContextEx(self.hwnd, 0, flags) } == 0 {
            return;
        }
        self.with_state(|s| {
            s.ime_allowed.set(allowed);
            // The IME ignored the request to finish: close the composition for the handler.
            if !allowed && s.composing.replace(false) {
                s.push(Event::Ime(ImeEvent::Preedit {
                    text: String::new(),
                    cursor: None,
                }));
                s.push(Event::Ime(ImeEvent::End));
            }
        });
    }

    pub(crate) fn set_ime_cursor_area(&mut self, area: Rect) {
        if self.alive() {
            self.with_state(|s| s.ime_area.set(Some(area)));
            ime::set_cursor_area(self.hwnd, area, ALL_CANDIDATE_LISTS);
        }
    }

    pub(crate) fn clipboard_text(&mut self) -> Option<String> {
        clipboard::text(self.hwnd)
    }

    pub(crate) fn set_clipboard_text(&mut self, text: &str) -> Result<(), Error> {
        clipboard::set_text(self.hwnd, text)
    }

    pub(crate) fn proxy(&self) -> Proxy {
        let target = self
            .with_state(|s| s.wake.borrow().clone())
            .filter(|_| self.alive())
            .unwrap_or_default();
        Proxy { target }
    }

    pub(crate) fn exit(&mut self) {
        if self.alive() {
            self.with_state(State::request_exit);
        }
    }

    /// Sends a message synchronously, as the OS would.
    #[cfg(test)]
    pub(crate) fn send(&self, msg: u32, wparam: usize, lparam: isize) -> isize {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SendMessageW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> isize;
        }
        // SAFETY: the tests send plain values, or pointers to data that outlives the call.
        unsafe { SendMessageW(self.hwnd, msg, wparam, lparam) }
    }

    /// Resizes the client area synchronously (re-enters the window procedure).
    #[cfg(test)]
    pub(crate) fn resize_outer(&self, width: i32, height: i32) {
        // SAFETY: hwnd is live; plain values.
        unsafe {
            ffi::SetWindowPos(
                self.hwnd,
                0,
                0,
                0,
                width,
                height,
                ffi::SWP_NOMOVE | ffi::SWP_NOZORDER | ffi::SWP_NOACTIVATE,
            )
        };
    }
}

/// Where a [`Proxy`] posts its wakes. The owner thread clears `hwnd` under the lock before
/// the window is destroyed, so a proxy never posts to a handle Windows may have reused.
#[derive(Debug, Default)]
struct WakeTarget {
    inner: Mutex<WakeState>,
}

#[derive(Debug, Default)]
struct WakeState {
    /// 0 once the window is gone.
    hwnd: Handle,
    /// A wake message is in flight, so a burst of wakes posts one message.
    pending: bool,
}

impl WakeTarget {
    fn lock(&self) -> MutexGuard<'_, WakeState> {
        // Nothing panics while holding the lock; recover the plain data if it ever did.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The backend half of [`super::Proxy`].
#[derive(Debug, Clone)]
pub(crate) struct Proxy {
    target: Arc<WakeTarget>,
}

impl Proxy {
    pub(crate) fn wake(&self) -> bool {
        let mut state = self.target.lock();
        if state.hwnd == 0 {
            return false;
        }
        if state.pending {
            return true;
        }
        // SAFETY: plain values. Holding the lock keeps the owner thread from finishing
        // WM_NCDESTROY, so hwnd still names our window.
        let posted = unsafe { ffi::PostMessageW(state.hwnd, WM_APP_WAKE, 0, 0) } != 0;
        state.pending = posted;
        posted
    }
}

#[cfg(test)]
mod tests;
