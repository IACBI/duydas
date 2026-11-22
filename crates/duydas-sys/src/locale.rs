//! The user's preferred UI languages, most preferred first, as BCP 47-like tags.
//!
//! - Windows: `GetUserPreferredUILanguages` (kernel32).
//! - macOS: `CFLocaleCopyPreferredLanguages` (CoreFoundation).
//! - Other Unix: `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG`, as gettext reads them.

/// Preferred UI languages, most preferred first. Empty if the system reports none.
pub fn preferred_languages() -> Vec<String> {
    let mut tags = platform::preferred_languages();
    if tags.is_empty() {
        tags = from_env(|name| std::env::var(name).ok());
    }
    tags.retain(|t| !t.is_empty());
    tags
}

/// gettext's lookup (glibc `guess_category_value`): with no locale or the "C"/"POSIX" locale in
/// the first of `LC_ALL`, `LC_MESSAGES` and `LANG`, programs stay untranslated. Otherwise `LANGUAGE`, a
/// colon-separated priority list in which "C" or "POSIX" ends the list, replaces the locale.
/// "C.UTF-8" counts as the C locale for the locale variables.
fn from_env(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let non_empty = |name: &str| get(name).filter(|v| !v.is_empty());
    let locale = non_empty("LC_ALL")
        .or_else(|| non_empty("LC_MESSAGES"))
        .or_else(|| non_empty("LANG"));
    let Some(locale) = locale.filter(|l| !is_c_locale(l)) else {
        return Vec::new();
    };
    match non_empty("LANGUAGE") {
        Some(list) => list
            .split(':')
            .filter(|t| !t.is_empty())
            // glibc compares the raw entry, so only a bare "C" or "POSIX" ends the list.
            .take_while(|t| !matches!(*t, "C" | "POSIX"))
            .map(normalize)
            .collect(),
        None => vec![normalize(&locale)],
    }
}

fn is_c_locale(name: &str) -> bool {
    let base = name.split(['.', '@']).next().unwrap_or(name);
    base == "C" || base == "POSIX"
}

/// `tr_TR.UTF-8@euro` → `tr-TR`.
fn normalize(posix: &str) -> String {
    posix
        .split(['.', '@'])
        .next()
        .unwrap_or(posix)
        .replace('_', "-")
}

#[cfg(windows)]
mod platform {
    const MUI_LANGUAGE_NAME: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserPreferredUILanguages(
            flags: u32,
            num_languages: *mut u32,
            buffer: *mut u16,
            buffer_len: *mut u32,
        ) -> i32;
    }

    pub(super) fn preferred_languages() -> Vec<String> {
        // The list can change between the size query and the fetch; retry a few times.
        for _ in 0..3 {
            let mut count = 0_u32;
            let mut len = 0_u32;
            // SAFETY: a null buffer with length 0 asks for the required length; both out-pointers
            // point to live locals for the duration of the call.
            let ok = unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &raw mut count,
                    std::ptr::null_mut(),
                    &raw mut len,
                )
            };
            if ok == 0 || len == 0 {
                return Vec::new();
            }
            let mut buffer = vec![0_u16; len as usize];
            // SAFETY: the buffer holds exactly len UTF-16 units, as the call requires, and the
            // out-pointers are live locals.
            let ok = unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &raw mut count,
                    buffer.as_mut_ptr(),
                    &raw mut len,
                )
            };
            if ok != 0 {
                buffer.truncate(len as usize);
                // A double-NUL-terminated list of NUL-separated names.
                return buffer
                    .split(|unit| *unit == 0)
                    .filter(|name| !name.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect();
            }
        }
        Vec::new()
    }
}

#[cfg(target_vendor = "apple")]
mod platform {
    use std::ffi::{CStr, c_char, c_void};

    type CFIndex = isize;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyPreferredLanguages() -> *const c_void;
        fn CFArrayGetCount(array: *const c_void) -> CFIndex;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: CFIndex) -> *const c_void;
        fn CFStringGetLength(string: *const c_void) -> CFIndex;
        fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut c_char,
            size: CFIndex,
            encoding: u32,
        ) -> u8;
        fn CFRelease(object: *const c_void);
    }

    /// Releases a CoreFoundation object we own (a "Copy" result) when dropped.
    struct Owned(*const c_void);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the pointer came from a CF "Copy" function, is non-null, and is released once.
            unsafe { CFRelease(self.0) }
        }
    }

    pub(super) fn preferred_languages() -> Vec<String> {
        // SAFETY: no arguments; returns an owned CFArray of CFStrings, or null.
        let array = unsafe { CFLocaleCopyPreferredLanguages() };
        if array.is_null() {
            return Vec::new();
        }
        let array = Owned(array);
        // SAFETY: array.0 is a valid CFArray for the lifetime of the guard.
        let count = unsafe { CFArrayGetCount(array.0) };
        let mut out = Vec::new();
        for index in 0..count {
            // SAFETY: index is in 0..count; the returned CFString is borrowed from the array,
            // which outlives this loop.
            let string = unsafe { CFArrayGetValueAtIndex(array.0, index) };
            if string.is_null() {
                continue;
            }
            // SAFETY: string is a valid CFString owned by the array, which outlives this loop.
            let length = unsafe { CFStringGetLength(string) };
            // SAFETY: a pure computation on its arguments; returns kCFNotFound (-1) on overflow.
            let max = unsafe { CFStringGetMaximumSizeForEncoding(length, UTF8) };
            // One more byte for the NUL terminator.
            let Some(size) = usize::try_from(max).ok().and_then(|m| m.checked_add(1)) else {
                continue;
            };
            let Ok(size_arg) = CFIndex::try_from(size) else {
                continue;
            };
            let mut buffer: Vec<c_char> = vec![0; size];
            // SAFETY: buffer is writable for exactly size bytes, which is the size we pass.
            let ok = unsafe { CFStringGetCString(string, buffer.as_mut_ptr(), size_arg, UTF8) };
            if ok == 0 {
                continue;
            }
            let bytes: Vec<u8> = buffer.iter().map(|c| *c as u8).collect();
            if let Ok(tag) = CStr::from_bytes_until_nul(&bytes) {
                out.push(tag.to_string_lossy().into_owned());
            }
        }
        out
    }
}

#[cfg(not(any(windows, target_vendor = "apple")))]
mod platform {
    pub(super) fn preferred_languages() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn gettext_order_and_normalization() {
        assert_eq!(
            from_env(env(&[("LANG", "tr_TR.UTF-8@euro")])),
            vec!["tr-TR"]
        );
        assert_eq!(
            from_env(env(&[
                ("LANG", "en_US.UTF-8"),
                ("LC_ALL", "ja_JP.UTF-8"),
                ("LANGUAGE", "tr:en_GB")
            ])),
            vec!["tr", "en-GB"]
        );
        assert_eq!(
            from_env(env(&[("LC_MESSAGES", "ja_JP.UTF-8"), ("LANG", "tr_TR")])),
            vec!["ja-JP"]
        );
    }

    #[test]
    fn c_locale_disables_language_list() {
        assert!(from_env(env(&[("LANG", "C.UTF-8"), ("LANGUAGE", "tr")])).is_empty());
        assert!(from_env(env(&[("LC_ALL", "POSIX")])).is_empty());
        assert!(from_env(env(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "C:tr")])).is_empty());
        assert_eq!(
            from_env(env(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "ja:POSIX:tr")])),
            vec!["ja"]
        );
        assert_eq!(
            from_env(env(&[
                ("LANG", "en_US.UTF-8"),
                ("LANGUAGE", "POSIX.UTF-8:ja")
            ])),
            vec!["POSIX", "ja"]
        );
        assert!(from_env(env(&[])).is_empty());
    }

    #[test]
    fn system_query_does_not_panic() {
        for tag in preferred_languages() {
            assert!(!tag.is_empty());
            assert!(!tag.contains('\0'));
        }
    }
}
