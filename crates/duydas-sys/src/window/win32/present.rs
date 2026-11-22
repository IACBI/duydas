//! Copies damaged rectangles of a CPU-rendered frame to a device context.

use super::ffi::{self, Handle};
use super::os_error;
use crate::window::{Error, Frame, Rect};

pub(super) fn present_to_window(
    hwnd: Handle,
    frame: Frame<'_>,
    damage: &[Rect],
) -> Result<(), Error> {
    // SAFETY: hwnd is our live window; the DC is released below.
    let hdc = unsafe { ffi::GetDC(hwnd) };
    if hdc == 0 {
        return Err(os_error("GetDC"));
    }
    let result = blit(hdc, frame, damage);
    // SAFETY: releases the DC obtained above, once.
    unsafe { ffi::ReleaseDC(hwnd, hdc) };
    result
}

pub(super) fn blit(hdc: Handle, frame: Frame<'_>, damage: &[Rect]) -> Result<(), Error> {
    let (width, height) = (frame.width(), frame.height());
    // Frame guarantees both fit in i32.
    let (signed_width, signed_height) = (width as i32, height as i32);
    let info = ffi::BitmapInfo {
        header: ffi::BitmapInfoHeader {
            size: size_of::<ffi::BitmapInfoHeader>() as u32,
            width: signed_width,
            // Negative height: a top-down bitmap, rows in the order the renderer writes them.
            height: -signed_height,
            planes: 1,
            bit_count: 32,
            compression: ffi::BI_RGB,
            ..Default::default()
        },
        colors: [0],
    };
    for rect in damage {
        let Some(r) = rect.clip(width, height) else {
            continue;
        };
        // SAFETY: info describes width × height 32-bit pixels and Frame guarantees the slice
        // holds at least that many, so GDI never reads past it; r lies inside the frame.
        let lines = unsafe {
            ffi::SetDIBitsToDevice(
                hdc,
                r.x,
                r.y,
                r.width,
                r.height,
                r.x,
                source_y(height, r),
                0,
                height,
                frame.pixels().as_ptr().cast(),
                &raw const info,
                ffi::DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            return Err(os_error("SetDIBitsToDevice"));
        }
    }
    Ok(())
}

/// SetDIBitsToDevice measures the source y from the bitmap's bottom edge even for top-down
/// bitmaps; the present test pins this behavior down.
fn source_y(height: u32, r: Rect) -> i32 {
    // r is clipped to the frame, so this cannot underflow or exceed i32.
    (height - r.height) as i32 - r.y
}
