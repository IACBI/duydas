//! Clipboard text (CF_UNICODETEXT).

use std::ffi::c_void;
use std::thread;
use std::time::Duration;

use super::ffi::{self, Handle};
use super::os_error;
use crate::window::Error;

/// The open clipboard, closed on drop.
struct Open;

impl Open {
    /// Another program may hold the clipboard for a moment; retry briefly.
    fn new(hwnd: Handle) -> Option<Self> {
        for _ in 0..10 {
            // SAFETY: hwnd is our window (or 0, which opens it for the task).
            if unsafe { ffi::OpenClipboard(hwnd) } != 0 {
                return Some(Open);
            }
            thread::sleep(Duration::from_millis(2));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        ffi::CloseClipboard();
    }
}

/// Locked global memory, unlocked on drop.
struct Locked {
    memory: Handle,
    data: *mut c_void,
}

impl Locked {
    fn new(memory: Handle) -> Option<Self> {
        // SAFETY: memory is a movable global memory handle.
        let data = unsafe { ffi::GlobalLock(memory) };
        // Build the guard only on success: dropping it unlocks.
        if data.is_null() {
            None
        } else {
            Some(Self { memory, data })
        }
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: balances the GlobalLock in new.
        unsafe { ffi::GlobalUnlock(self.memory) };
    }
}

pub(super) fn text(hwnd: Handle) -> Option<String> {
    let _open = Open::new(hwnd)?;
    // SAFETY: the clipboard is open; the returned handle stays owned by the clipboard.
    let memory = unsafe { ffi::GetClipboardData(ffi::CF_UNICODETEXT) };
    if memory == 0 {
        return None;
    }
    // SAFETY: memory is a valid global memory handle.
    let size = unsafe { ffi::GlobalSize(memory) };
    let locked = Locked::new(memory)?;
    let start = locked.data.cast::<u16>();
    // A slice may not exceed isize::MAX bytes; no real block comes close.
    if !start.is_aligned() || size > isize::MAX as usize {
        return None;
    }
    // GlobalSize may exceed what the owner wrote, so never view the whole block as a slice:
    // read unit by unit up to the terminator, bounded by the block's end.
    let capacity = size / 2;
    let mut len = 0;
    while len < capacity {
        // SAFETY: len < capacity keeps the pointer inside the locked block.
        let unit = unsafe { start.add(len) };
        // SAFETY: unit is inside the block and aligned (checked above).
        if unsafe { unit.read() } == 0 {
            break;
        }
        len += 1;
    }
    // SAFETY: the first len units were just read (written by the owner, before the
    // terminator or the block's end); the slice ends before Locked unlocks.
    let units = unsafe { std::slice::from_raw_parts(start, len) };
    Some(String::from_utf16_lossy(units))
}

pub(super) fn set_text(hwnd: Handle, text: &str) -> Result<(), Error> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let open = Open::new(hwnd).ok_or_else(|| os_error("OpenClipboard"))?;
    // SAFETY: plain values.
    let memory =
        unsafe { ffi::GlobalAlloc(ffi::GMEM_MOVEABLE | ffi::GMEM_ZEROINIT, wide.len() * 2) };
    if memory == 0 {
        return Err(os_error("GlobalAlloc"));
    }
    let free = |error: Error| {
        // SAFETY: memory is ours until SetClipboardData succeeds.
        unsafe { ffi::GlobalFree(memory) };
        Err(error)
    };
    {
        let Some(locked) = Locked::new(memory) else {
            return free(os_error("GlobalLock"));
        };
        // SAFETY: the block holds wide.len() u16 values (allocated above), is aligned for u16,
        // and does not overlap wide.
        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), locked.data.cast::<u16>(), wide.len());
        }
    }
    // SAFETY: the clipboard is open by this thread.
    if unsafe { ffi::EmptyClipboard() } == 0 {
        return free(os_error("EmptyClipboard"));
    }
    // SAFETY: memory is an unlocked movable block; on success the system takes ownership.
    if unsafe { ffi::SetClipboardData(ffi::CF_UNICODETEXT, memory) } == 0 {
        return free(os_error("SetClipboardData"));
    }
    drop(open);
    Ok(())
}
