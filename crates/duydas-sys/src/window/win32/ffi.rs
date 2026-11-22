//! Hand-written Win32 declarations: only what the window backend uses, checked against the
//! Windows SDK headers (winuser.h, wingdi.h, imm.h, dwmapi.h, libloaderapi.h, winbase.h).
//!
//! Handles are pointer-sized integers; 0 is null. Functions that take only plain values and
//! cannot cause undefined behavior are declared `safe`.

use std::ffi::c_void;

/// HWND, HDC, HINSTANCE, HCURSOR, HGLOBAL, HIMC, …
pub(super) type Handle = isize;

pub(super) type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Point {
    pub(super) x: i32,
    pub(super) y: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Rect {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

#[repr(C)]
#[derive(Debug)]
pub(super) struct WndClassExW {
    pub(super) size: u32,
    pub(super) style: u32,
    pub(super) wnd_proc: Option<WndProc>,
    pub(super) cls_extra: i32,
    pub(super) wnd_extra: i32,
    pub(super) instance: Handle,
    pub(super) icon: Handle,
    pub(super) cursor: Handle,
    pub(super) background: Handle,
    pub(super) menu_name: *const u16,
    pub(super) class_name: *const u16,
    pub(super) icon_small: Handle,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct Msg {
    pub(super) hwnd: Handle,
    pub(super) message: u32,
    pub(super) wparam: usize,
    pub(super) lparam: isize,
    pub(super) time: u32,
    pub(super) pt: Point,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct PaintStruct {
    pub(super) hdc: Handle,
    pub(super) erase: i32,
    pub(super) paint: Rect,
    pub(super) restore: i32,
    pub(super) inc_update: i32,
    pub(super) reserved: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct BitmapInfoHeader {
    pub(super) size: u32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) planes: u16,
    pub(super) bit_count: u16,
    pub(super) compression: u32,
    pub(super) size_image: u32,
    pub(super) x_pels_per_meter: i32,
    pub(super) y_pels_per_meter: i32,
    pub(super) clr_used: u32,
    pub(super) clr_important: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct BitmapInfo {
    pub(super) header: BitmapInfoHeader,
    /// RGBQUAD[1]; unused for 32-bit BI_RGB.
    pub(super) colors: [u32; 1],
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct MinMaxInfo {
    pub(super) reserved: Point,
    pub(super) max_size: Point,
    pub(super) max_position: Point,
    pub(super) min_track_size: Point,
    pub(super) max_track_size: Point,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct TrackMouseEvent {
    pub(super) size: u32,
    pub(super) flags: u32,
    pub(super) hwnd: Handle,
    pub(super) hover_time: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct CompositionForm {
    pub(super) style: u32,
    pub(super) current_pos: Point,
    pub(super) area: Rect,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct CandidateForm {
    pub(super) index: u32,
    pub(super) style: u32,
    pub(super) current_pos: Point,
    pub(super) area: Rect,
}

// Layouts must match the SDK exactly; these catch a wrong field type or order.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<WndClassExW>() == 80);
    assert!(size_of::<Msg>() == 48);
    assert!(std::mem::offset_of!(Msg, pt) == 36);
    assert!(std::mem::offset_of!(PaintStruct, paint) == 12);
    assert!(size_of::<PaintStruct>() == 72);
    assert!(size_of::<TrackMouseEvent>() == 24);
};
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<WndClassExW>() == 48);
    assert!(size_of::<Msg>() == 28);
    assert!(std::mem::offset_of!(Msg, pt) == 20);
    assert!(std::mem::offset_of!(PaintStruct, paint) == 8);
    assert!(size_of::<PaintStruct>() == 64);
    assert!(size_of::<TrackMouseEvent>() == 16);
};
const _: () = {
    assert!(align_of::<WndClassExW>() == align_of::<usize>());
    assert!(align_of::<Msg>() == align_of::<usize>());
    assert!(align_of::<PaintStruct>() == align_of::<usize>());
    assert!(align_of::<TrackMouseEvent>() == align_of::<usize>());
    assert!(size_of::<BitmapInfoHeader>() == 40);
    assert!(size_of::<MinMaxInfo>() == 40);
    assert!(size_of::<CompositionForm>() == 28);
    assert!(size_of::<CandidateForm>() == 32);
};

// Window messages.
pub(super) const WM_DESTROY: u32 = 0x0002;
pub(super) const WM_SIZE: u32 = 0x0005;
pub(super) const WM_SETFOCUS: u32 = 0x0007;
pub(super) const WM_KILLFOCUS: u32 = 0x0008;
pub(super) const WM_PAINT: u32 = 0x000F;
pub(super) const WM_CLOSE: u32 = 0x0010;
pub(super) const WM_ERASEBKGND: u32 = 0x0014;
pub(super) const WM_SETCURSOR: u32 = 0x0020;
pub(super) const WM_GETMINMAXINFO: u32 = 0x0024;
pub(super) const WM_NCCREATE: u32 = 0x0081;
pub(super) const WM_NCDESTROY: u32 = 0x0082;
pub(super) const WM_KEYDOWN: u32 = 0x0100;
pub(super) const WM_KEYUP: u32 = 0x0101;
pub(super) const WM_CHAR: u32 = 0x0102;
pub(super) const WM_SYSKEYDOWN: u32 = 0x0104;
pub(super) const WM_SYSKEYUP: u32 = 0x0105;
pub(super) const WM_SYSCHAR: u32 = 0x0106;
pub(super) const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
pub(super) const WM_IME_ENDCOMPOSITION: u32 = 0x010E;
pub(super) const WM_IME_COMPOSITION: u32 = 0x010F;
pub(super) const WM_MOUSEMOVE: u32 = 0x0200;
pub(super) const WM_LBUTTONDOWN: u32 = 0x0201;
pub(super) const WM_LBUTTONUP: u32 = 0x0202;
pub(super) const WM_RBUTTONDOWN: u32 = 0x0204;
pub(super) const WM_RBUTTONUP: u32 = 0x0205;
pub(super) const WM_MBUTTONDOWN: u32 = 0x0207;
pub(super) const WM_MBUTTONUP: u32 = 0x0208;
pub(super) const WM_MOUSEWHEEL: u32 = 0x020A;
pub(super) const WM_XBUTTONDOWN: u32 = 0x020B;
pub(super) const WM_XBUTTONUP: u32 = 0x020C;
pub(super) const WM_MOUSEHWHEEL: u32 = 0x020E;
pub(super) const WM_CAPTURECHANGED: u32 = 0x0215;
pub(super) const WM_IME_SETCONTEXT: u32 = 0x0281;
pub(super) const WM_IME_NOTIFY: u32 = 0x0282;
pub(super) const WM_MOUSELEAVE: u32 = 0x02A3;
pub(super) const WM_DPICHANGED: u32 = 0x02E0;
pub(super) const WM_APP: u32 = 0x8000;

// Window styles, ShowWindow and SetWindowPos.
pub(super) const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub(super) const CW_USEDEFAULT: i32 = i32::MIN;
pub(super) const SW_HIDE: i32 = 0;
pub(super) const SW_SHOW: i32 = 5;
pub(super) const SW_SHOWNA: i32 = 8;
pub(super) const SWP_NOMOVE: u32 = 0x0002;
pub(super) const SWP_NOZORDER: u32 = 0x0004;
pub(super) const SWP_NOACTIVATE: u32 = 0x0010;
pub(super) const SIZE_MINIMIZED: usize = 1;
pub(super) const PM_NOREMOVE: u32 = 0x0000;
pub(super) const HTCLIENT: u16 = 1;
pub(super) const TME_LEAVE: u32 = 0x0002;
pub(super) const XBUTTON1: u16 = 0x0001;
pub(super) const WHEEL_DELTA: f32 = 120.0;
/// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, a pseudo-handle.
pub(super) const DPI_PER_MONITOR_AWARE_V2: Handle = -4;
pub(super) const USER_DEFAULT_SCREEN_DPI: u32 = 96;

// Cursors (MAKEINTRESOURCE ids).
pub(super) const IDC_ARROW: usize = 32512;
pub(super) const IDC_IBEAM: usize = 32513;
pub(super) const IDC_WAIT: usize = 32514;
pub(super) const IDC_SIZEWE: usize = 32644;
pub(super) const IDC_SIZENS: usize = 32645;
pub(super) const IDC_NO: usize = 32648;
pub(super) const IDC_HAND: usize = 32649;

// Virtual keys.
pub(super) const VK_BACK: u16 = 0x08;
pub(super) const VK_TAB: u16 = 0x09;
pub(super) const VK_RETURN: u16 = 0x0D;
pub(super) const VK_SHIFT: u16 = 0x10;
pub(super) const VK_CONTROL: u16 = 0x11;
pub(super) const VK_MENU: u16 = 0x12;
pub(super) const VK_CAPITAL: u16 = 0x14;
pub(super) const VK_ESCAPE: u16 = 0x1B;
pub(super) const VK_SPACE: u16 = 0x20;
pub(super) const VK_PRIOR: u16 = 0x21;
pub(super) const VK_NEXT: u16 = 0x22;
pub(super) const VK_END: u16 = 0x23;
pub(super) const VK_HOME: u16 = 0x24;
pub(super) const VK_LEFT: u16 = 0x25;
pub(super) const VK_UP: u16 = 0x26;
pub(super) const VK_RIGHT: u16 = 0x27;
pub(super) const VK_DOWN: u16 = 0x28;
pub(super) const VK_INSERT: u16 = 0x2D;
pub(super) const VK_DELETE: u16 = 0x2E;
pub(super) const VK_LWIN: u16 = 0x5B;
pub(super) const VK_RWIN: u16 = 0x5C;
pub(super) const VK_APPS: u16 = 0x5D;
pub(super) const VK_F1: u16 = 0x70;
pub(super) const VK_F24: u16 = 0x87;
pub(super) const VK_PROCESSKEY: u16 = 0xE5;

// GDI.
pub(super) const BI_RGB: u32 = 0;
pub(super) const DIB_RGB_COLORS: u32 = 0;

// Clipboard and global memory.
pub(super) const CF_UNICODETEXT: u32 = 13;
pub(super) const GMEM_MOVEABLE: u32 = 0x0002;
pub(super) const GMEM_ZEROINIT: u32 = 0x0040;

// IME.
pub(super) const GCS_COMPSTR: u32 = 0x0008;
pub(super) const GCS_CURSORPOS: u32 = 0x0080;
pub(super) const GCS_RESULTSTR: u32 = 0x0800;
pub(super) const CFS_POINT: u32 = 0x0002;
pub(super) const CFS_EXCLUDE: u32 = 0x0080;
pub(super) const IACE_DEFAULT: u32 = 0x0010;
/// Bit 31 of WM_IME_SETCONTEXT's LPARAM; unsigned so it also fits a 32-bit LPARAM.
pub(super) const ISC_SHOWUICOMPOSITIONWINDOW: usize = 0x8000_0000;
pub(super) const IMN_OPENCANDIDATE: usize = 0x0005;
pub(super) const NI_COMPOSITIONSTR: u32 = 0x0015;
pub(super) const CPS_COMPLETE: u32 = 0x0001;

// DWM.
pub(super) const DWMWA_CLOAK: u32 = 13;
pub(super) const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) safe fn GetLastError() -> u32;
    pub(super) fn GetModuleHandleW(name: *const u16) -> Handle;
    pub(super) fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    pub(super) fn GlobalLock(memory: Handle) -> *mut c_void;
    pub(super) fn GlobalUnlock(memory: Handle) -> i32;
    pub(super) fn GlobalSize(memory: Handle) -> usize;
    pub(super) fn GlobalFree(memory: Handle) -> Handle;
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(super) fn RegisterClassExW(class: *const WndClassExW) -> u16;
    #[allow(clippy::too_many_arguments, reason = "Win32 signature")]
    pub(super) fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    pub(super) fn DestroyWindow(hwnd: Handle) -> i32;
    pub(super) fn DefWindowProcW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> isize;
    pub(super) fn GetMessageW(msg: *mut Msg, hwnd: Handle, min: u32, max: u32) -> i32;
    pub(super) fn PeekMessageW(msg: *mut Msg, hwnd: Handle, min: u32, max: u32, remove: u32)
    -> i32;
    pub(super) fn TranslateMessage(msg: *const Msg) -> i32;
    pub(super) fn DispatchMessageW(msg: *const Msg) -> isize;
    pub(super) fn PostMessageW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> i32;
    pub(super) safe fn PostQuitMessage(exit_code: i32);
    pub(super) fn ShowWindow(hwnd: Handle, cmd: i32) -> i32;
    pub(super) fn IsWindowVisible(hwnd: Handle) -> i32;
    pub(super) fn SetForegroundWindow(hwnd: Handle) -> i32;
    pub(super) fn SetWindowTextW(hwnd: Handle, text: *const u16) -> i32;
    pub(super) fn SetWindowPos(
        hwnd: Handle,
        insert_after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    pub(super) fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
    pub(super) fn InvalidateRect(hwnd: Handle, rect: *const Rect, erase: i32) -> i32;
    pub(super) fn BeginPaint(hwnd: Handle, paint: *mut PaintStruct) -> Handle;
    pub(super) fn EndPaint(hwnd: Handle, paint: *const PaintStruct) -> i32;
    pub(super) fn GetDC(hwnd: Handle) -> Handle;
    pub(super) fn ReleaseDC(hwnd: Handle, hdc: Handle) -> i32;
    pub(super) fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    pub(super) fn SetCursor(cursor: Handle) -> Handle;
    pub(super) fn SetCapture(hwnd: Handle) -> Handle;
    pub(super) safe fn GetCapture() -> Handle;
    pub(super) safe fn ReleaseCapture() -> i32;
    pub(super) fn TrackMouseEvent(track: *mut TrackMouseEvent) -> i32;
    pub(super) safe fn GetKeyState(virtual_key: i32) -> i16;
    pub(super) fn SetThreadDpiAwarenessContext(context: Handle) -> Handle;
    pub(super) fn GetDpiForWindow(hwnd: Handle) -> u32;
    pub(super) fn AdjustWindowRectExForDpi(
        rect: *mut Rect,
        style: u32,
        menu: i32,
        ex_style: u32,
        dpi: u32,
    ) -> i32;
    pub(super) fn OpenClipboard(owner: Handle) -> i32;
    pub(super) safe fn CloseClipboard() -> i32;
    pub(super) fn EmptyClipboard() -> i32;
    pub(super) fn GetClipboardData(format: u32) -> Handle;
    pub(super) fn SetClipboardData(format: u32, memory: Handle) -> Handle;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    #[allow(clippy::too_many_arguments, reason = "Win32 signature")]
    pub(super) fn SetDIBitsToDevice(
        hdc: Handle,
        x_dest: i32,
        y_dest: i32,
        width: u32,
        height: u32,
        x_src: i32,
        y_src: i32,
        start_scan: u32,
        lines: u32,
        bits: *const c_void,
        info: *const BitmapInfo,
        color_use: u32,
    ) -> i32;
}

#[link(name = "imm32")]
unsafe extern "system" {
    pub(super) fn ImmGetContext(hwnd: Handle) -> Handle;
    pub(super) fn ImmReleaseContext(hwnd: Handle, himc: Handle) -> i32;
    pub(super) fn ImmGetCompositionStringW(
        himc: Handle,
        index: u32,
        buffer: *mut c_void,
        buffer_len: u32,
    ) -> i32;
    pub(super) fn ImmSetCompositionWindow(himc: Handle, form: *mut CompositionForm) -> i32;
    pub(super) fn ImmSetCandidateWindow(himc: Handle, form: *mut CandidateForm) -> i32;
    pub(super) fn ImmNotifyIME(himc: Handle, action: u32, index: u32, value: u32) -> i32;
    pub(super) fn ImmAssociateContextEx(hwnd: Handle, himc: Handle, flags: u32) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    pub(super) fn DwmSetWindowAttribute(
        hwnd: Handle,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> i32;
}
