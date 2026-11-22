//! Keyboard translation: virtual keys, scan codes, modifiers and UTF-16 characters.

use std::cell::Cell;

use super::ffi;
use crate::window::{Key, KeyEvent, Modifiers};

/// Modifier state at the time of the message being handled (GetKeyState follows the message
/// queue, not the physical keyboard, which is what event handling needs).
pub(super) fn modifiers() -> Modifiers {
    let down = |vk: u16| ffi::GetKeyState(i32::from(vk)) < 0;
    Modifiers {
        shift: down(ffi::VK_SHIFT),
        control: down(ffi::VK_CONTROL),
        alt: down(ffi::VK_MENU),
        logo: down(ffi::VK_LWIN) || down(ffi::VK_RWIN),
    }
}

/// Decodes WM_(SYS)KEYDOWN/UP parameters.
pub(super) fn key_event(
    wparam: usize,
    lparam: isize,
    pressed: bool,
    modifiers: Modifiers,
) -> KeyEvent {
    let bits = lparam as usize;
    let scan = ((bits >> 16) & 0xFF) as u16;
    let extended = bits & (1 << 24) != 0;
    KeyEvent {
        key: key_from_vk((wparam & 0xFFFF) as u16),
        scancode: if extended { scan | 0xE000 } else { scan },
        pressed,
        // Bit 30 is the previous key state: set on auto-repeat.
        repeat: pressed && bits & (1 << 30) != 0,
        modifiers,
    }
}

pub(super) fn key_from_vk(vk: u16) -> Key {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => {
            Key::Character(char::from(u8::try_from(vk).unwrap_or(b'?')).to_ascii_lowercase())
        }
        ffi::VK_RETURN => Key::Enter,
        ffi::VK_TAB => Key::Tab,
        ffi::VK_SPACE => Key::Space,
        ffi::VK_BACK => Key::Backspace,
        ffi::VK_ESCAPE => Key::Escape,
        ffi::VK_DELETE => Key::Delete,
        ffi::VK_INSERT => Key::Insert,
        ffi::VK_HOME => Key::Home,
        ffi::VK_END => Key::End,
        ffi::VK_PRIOR => Key::PageUp,
        ffi::VK_NEXT => Key::PageDown,
        ffi::VK_LEFT => Key::ArrowLeft,
        ffi::VK_RIGHT => Key::ArrowRight,
        ffi::VK_UP => Key::ArrowUp,
        ffi::VK_DOWN => Key::ArrowDown,
        ffi::VK_F1..=ffi::VK_F24 => Key::F(u8::try_from(vk - ffi::VK_F1 + 1).unwrap_or(0)),
        ffi::VK_SHIFT | 0xA0 | 0xA1 => Key::Shift,
        ffi::VK_CONTROL | 0xA2 | 0xA3 => Key::Control,
        ffi::VK_MENU | 0xA4 | 0xA5 => Key::Alt,
        ffi::VK_LWIN | ffi::VK_RWIN => Key::Logo,
        ffi::VK_CAPITAL => Key::CapsLock,
        ffi::VK_APPS => Key::ContextMenu,
        ffi::VK_PROCESSKEY => Key::ImeProcess,
        other => Key::Other(u32::from(other)),
    }
}

/// Assembles WM_CHAR code units into characters: a high surrogate waits for its low half; an
/// unpaired surrogate is dropped.
pub(super) fn utf16_char(high: &Cell<Option<u16>>, unit: u16) -> Option<char> {
    match unit {
        0xD800..=0xDBFF => {
            high.set(Some(unit));
            None
        }
        0xDC00..=0xDFFF => {
            let first = high.take()?;
            char::decode_utf16([first, unit]).next()?.ok()
        }
        _ => {
            high.set(None);
            char::from_u32(u32::from(unit))
        }
    }
}
