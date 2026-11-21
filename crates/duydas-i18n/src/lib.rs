//! Localization for Duydaş.
//!
//! - Messages live in `locales/<tag>/*.ftl` (Fluent) and are embedded at compile time.
//! - [`Key`] is generated from the en-US source locale, so referencing a message that does not
//!   exist is a compile error.
//! - [`Localizer`] bundles messages, ICU4X formatters and case mapping for one UI locale. Switching
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

mod locale;
mod localizer;
mod pseudo;
pub mod schema;

pub use fluent_bundle::{FluentArgs, FluentValue};
pub use locale::{UiLocale, detect_system_locale, negotiate};
pub use localizer::{Localizer, validate_shipped_bundles};

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/// Builds [`FluentArgs`] from `name = value` pairs.
///
/// ```
/// let args = duydas_i18n::args!(channel = "general", count = 2);
/// assert_eq!(args.iter().count(), 2);
/// ```
#[macro_export]
macro_rules! args {
    ($($name:ident = $value:expr),* $(,)?) => {{
        let mut args = $crate::FluentArgs::new();
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
