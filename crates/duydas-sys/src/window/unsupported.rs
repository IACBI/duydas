//! Placeholder for platforms whose backend is not written yet (Wayland/X11 and Cocoa are next).

use super::{Error, Frame, Handler, Rect, WindowOptions};

/// No context can exist without a backend.
#[derive(Debug)]
pub(crate) enum Context {}

/// No proxy can exist without a backend.
#[derive(Debug, Clone)]
pub(crate) enum Proxy {}

pub(crate) fn run<H: Handler + 'static>(_options: &WindowOptions, _handler: H) -> Result<H, Error> {
    Err(Error::Unsupported)
}

impl Context {
    pub(crate) fn size(&self) -> (u32, u32) {
        match *self {}
    }
    pub(crate) fn scale_factor(&self) -> f32 {
        match *self {}
    }
    pub(crate) fn request_redraw(&mut self) {
        match *self {}
    }
    pub(crate) fn present(&mut self, _frame: Frame<'_>, _damage: &[Rect]) -> Result<(), Error> {
        match *self {}
    }
    pub(crate) fn set_visible(&mut self, _visible: bool) {
        match *self {}
    }
    pub(crate) fn set_title(&mut self, _title: &str) {
        match *self {}
    }
    pub(crate) fn set_cursor(&mut self, _cursor: super::Cursor) {
        match *self {}
    }
    pub(crate) fn set_ime_allowed(&mut self, _allowed: bool) {
        match *self {}
    }
    pub(crate) fn set_ime_cursor_area(&mut self, _area: Rect) {
        match *self {}
    }
    pub(crate) fn clipboard_text(&mut self) -> Option<String> {
        match *self {}
    }
    pub(crate) fn set_clipboard_text(&mut self, _text: &str) -> Result<(), Error> {
        match *self {}
    }
    pub(crate) fn proxy(&self) -> Proxy {
        match *self {}
    }
    pub(crate) fn exit(&mut self) {
        match *self {}
    }
}

impl Proxy {
    pub(crate) fn wake(&self) -> bool {
        match *self {}
    }
}
