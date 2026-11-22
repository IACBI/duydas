//! Input method (IMM32) integration: composition strings, caret position and candidate window
//! placement.

use std::ptr;

use super::ffi::{self, Handle};
use crate::window::{ImeEvent, Rect};

/// The window's input context, released on drop.
struct InputContext {
    hwnd: Handle,
    himc: Handle,
}

impl InputContext {
    fn get(hwnd: Handle) -> Option<Self> {
        // SAFETY: hwnd is our live window.
        let himc = unsafe { ffi::ImmGetContext(hwnd) };
        // Build the guard only on success: dropping it releases.
        if himc == 0 {
            None
        } else {
            Some(Self { hwnd, himc })
        }
    }

    fn string(&self, kind: u32) -> Option<String> {
        // SAFETY: a null buffer with length 0 asks for the size in bytes.
        let bytes = unsafe { ffi::ImmGetCompositionStringW(self.himc, kind, ptr::null_mut(), 0) };
        // Negative values are IMM_ERROR_* codes.
        let bytes = usize::try_from(bytes).ok()?;
        let mut buffer = vec![0_u16; bytes.div_ceil(2)];
        let capacity = u32::try_from(buffer.len() * 2).ok()?;
        // SAFETY: buffer is writable for exactly capacity bytes.
        let copied = unsafe {
            ffi::ImmGetCompositionStringW(self.himc, kind, buffer.as_mut_ptr().cast(), capacity)
        };
        buffer.truncate(usize::try_from(copied).ok()? / 2);
        Some(String::from_utf16_lossy(&buffer))
    }

    /// Caret position in UTF-16 units.
    fn cursor(&self) -> Option<usize> {
        // SAFETY: GCS_CURSORPOS returns the position itself; no buffer is written.
        let units = unsafe {
            ffi::ImmGetCompositionStringW(self.himc, ffi::GCS_CURSORPOS, ptr::null_mut(), 0)
        };
        usize::try_from(units).ok()
    }
}

impl Drop for InputContext {
    fn drop(&mut self) {
        // SAFETY: releases the context obtained in get, once.
        unsafe { ffi::ImmReleaseContext(self.hwnd, self.himc) };
    }
}

/// The events of one WM_IME_COMPOSITION.
#[derive(Debug, Default)]
pub(super) struct Composition {
    pub(super) events: Vec<ImeEvent>,
    /// The message carried a result string we could not read. Let DefWindowProcW handle the
    /// message, so the text still arrives (as characters) instead of being lost.
    pub(super) unread_result: bool,
}

/// Reads one WM_IME_COMPOSITION. A commit comes before the next preedit, since an IME can
/// commit text and start a new composition in one message.
pub(super) fn composition_events(hwnd: Handle, flags: u32) -> Composition {
    let mut out = Composition::default();
    if flags == 0 {
        // The composition was cancelled or cleared.
        out.events.push(ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        });
        return out;
    }
    let has_result = flags & ffi::GCS_RESULTSTR != 0;
    let Some(context) = InputContext::get(hwnd) else {
        out.unread_result = has_result;
        return out;
    };
    if has_result {
        match context.string(ffi::GCS_RESULTSTR) {
            Some(text) if !text.is_empty() => out.events.push(ImeEvent::Commit(text)),
            Some(_) => {}
            None => out.unread_result = true,
        }
    }
    // Any other change (caret moved, attributes, clause boundaries) while composing: re-read
    // the composition, so a caret-only update never erases the preedit.
    // A failed read reports nothing rather than an empty preedit that would erase it.
    if (flags & ffi::GCS_COMPSTR != 0 || !has_result)
        && let Some(text) = context.string(ffi::GCS_COMPSTR)
    {
        let cursor = context.cursor().map(|units| byte_offset(&text, units));
        out.events.push(ImeEvent::Preedit { text, cursor });
    }
    out
}

/// Finishes an active composition, as if the user confirmed it: its text arrives as a commit.
pub(super) fn complete_composition(hwnd: Handle) {
    if let Some(context) = InputContext::get(hwnd) {
        // SAFETY: a live input context; plain values.
        unsafe { ffi::ImmNotifyIME(context.himc, ffi::NI_COMPOSITIONSTR, ffi::CPS_COMPLETE, 0) };
    }
}

/// Converts a UTF-16 offset into a byte offset in `text`, clamped to its length.
pub(super) fn byte_offset(text: &str, utf16_units: usize) -> usize {
    let mut units = 0;
    for (index, c) in text.char_indices() {
        if units >= utf16_units {
            return index;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// Places the composition window at the caret and keeps candidate lists from covering the
/// caret's line. `lists` is a bit mask of candidate list indices (0–3) to place.
pub(super) fn set_cursor_area(hwnd: Handle, area: Rect, lists: usize) {
    let Some(context) = InputContext::get(hwnd) else {
        return;
    };
    let right = area
        .x
        .saturating_add(i32::try_from(area.width).unwrap_or(i32::MAX));
    let bottom = area
        .y
        .saturating_add(i32::try_from(area.height).unwrap_or(i32::MAX));
    let position = ffi::Point {
        x: area.x,
        y: area.y,
    };
    let mut composition = ffi::CompositionForm {
        style: ffi::CFS_POINT,
        current_pos: position,
        area: ffi::Rect::default(),
    };
    // SAFETY: composition is a live, initialized COMPOSITIONFORM that IMM32 only reads.
    unsafe { ffi::ImmSetCompositionWindow(context.himc, &raw mut composition) };
    for index in (0..4_u32).filter(|i| lists & (1 << i) != 0) {
        let mut candidate = ffi::CandidateForm {
            index,
            style: ffi::CFS_EXCLUDE,
            current_pos: position,
            area: ffi::Rect {
                left: area.x,
                top: area.y,
                right,
                bottom,
            },
        };
        // SAFETY: candidate is a live, initialized CANDIDATEFORM that IMM32 only reads.
        unsafe { ffi::ImmSetCandidateWindow(context.himc, &raw mut candidate) };
    }
}
