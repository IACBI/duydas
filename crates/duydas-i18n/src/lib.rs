//! Localization for Duydaş.
//!
//! - Messages live in `locales/<tag>/*.ftl` (Fluent syntax, parsed by our own [`ftl`] module) and
//!   are embedded at compile time.
//! - [`Key`] is generated from the en-US source locale, so referencing a message that does not
//!   exist is a compile error.
//! - [`Localizer`] bundles messages, number/date formatting and case mapping for one UI locale. Switching
//!   language means building a new `Localizer` and swapping it in; nothing else needs restarting.
//!
//! ```
//! use duydas_i18n::{Key, Localizer, UiLocale, args};
//!
//! let l10n = Localizer::new(UiLocale::JaJp);
//! assert_eq!(l10n.tr(Key::SettingsTitle), "設定");
//! let shown = l10n.format_integer(1234);
//! assert_eq!(
//!     l10n.tr_args(Key::MessageCount, &args!(count = 1234, countText = shown)),
//!     "1,234 件のメッセージ"
//! );
//! ```

mod bundle;
mod format;
pub mod ftl;
mod locale;
mod localizer;
mod pseudo;
pub mod schema;

use std::borrow::Cow;

pub use locale::{UiLocale, detect_system_locale, negotiate};
pub use localizer::{Localizer, validate_shipped_bundles};

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/// A value passed into a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value<'a> {
    /// Text, inserted as is (never pseudo-localized).
    Str(Cow<'a, str>),
    /// An integer: selects plural variants and is inserted with locale grouping.
    Int(i64),
}

impl<'a> From<&'a str> for Value<'a> {
    fn from(value: &'a str) -> Self {
        Value::Str(Cow::Borrowed(value))
    }
}

impl From<String> for Value<'_> {
    fn from(value: String) -> Self {
        Value::Str(Cow::Owned(value))
    }
}

impl<'a> From<&'a String> for Value<'a> {
    fn from(value: &'a String) -> Self {
        Value::Str(Cow::Borrowed(value))
    }
}

macro_rules! int_value {
    ($($t:ty),*) => {$(
        impl From<$t> for Value<'_> {
            fn from(value: $t) -> Self {
                Value::Int(i64::from(value))
            }
        }

        impl From<&$t> for Value<'_> {
            fn from(value: &$t) -> Self {
                Value::Int(i64::from(*value))
            }
        }
    )*};
}
int_value!(i8, i16, i32, i64, u8, u16, u32);

macro_rules! saturating_value {
    ($($t:ty),*) => {$(
        impl From<$t> for Value<'_> {
            /// Saturates at `i64::MAX`, far beyond any count the UI shows.
            fn from(value: $t) -> Self {
                Value::Int(i64::try_from(value).unwrap_or(i64::MAX))
            }
        }

        impl From<&$t> for Value<'_> {
            /// Saturates at `i64::MAX`, far beyond any count the UI shows.
            fn from(value: &$t) -> Self {
                Value::from(*value)
            }
        }
    )*};
}
saturating_value!(u64, usize, u128);

macro_rules! signed_saturating_value {
    ($($t:ty),*) => {$(
        impl From<$t> for Value<'_> {
            /// Saturates at the `i64` bounds, far beyond any count the UI shows.
            fn from(value: $t) -> Self {
                Value::Int(i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX }))
            }
        }

        impl From<&$t> for Value<'_> {
            /// Saturates at the `i64` bounds, far beyond any count the UI shows.
            fn from(value: &$t) -> Self {
                Value::from(*value)
            }
        }
    )*};
}
signed_saturating_value!(isize, i128);

/// Named values for a message, built with [`args!`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args<'a> {
    entries: Vec<(&'a str, Value<'a>)>,
}

impl<'a> Args<'a> {
    /// No values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `name`, replacing an earlier value.
    pub fn set(&mut self, name: &'a str, value: impl Into<Value<'a>>) {
        let value = value.into();
        match self.entries.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((name, value)),
        }
    }

    /// The value for `name`.
    pub fn get(&self, name: &str) -> Option<&Value<'a>> {
        self.entries
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }

    /// All values in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &Value<'a>)> {
        self.entries.iter().map(|(n, v)| (*n, v))
    }
}

/// Builds [`Args`] from `name = value` pairs.
///
/// ```
/// let args = duydas_i18n::args!(channel = "general", count = 2);
/// assert_eq!(args.iter().count(), 2);
/// ```
#[macro_export]
macro_rules! args {
    ($($name:ident = $value:expr),* $(,)?) => {{
        let mut args = $crate::Args::new();
        $( args.set(stringify!($name), $value); )*
        args
    }};
}

/// Locale tags that have embedded resources, in the order the build script found them.
pub fn embedded_locales() -> impl Iterator<Item = &'static str> {
    EMBEDDED.iter().map(|(tag, _)| *tag)
}

pub(crate) fn embedded_sources(tag: &str) -> Option<&'static [(&'static str, &'static str)]> {
    EMBEDDED
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, files)| *files)
}
