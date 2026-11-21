use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use icu_casemap::CaseMapper;
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::YMD;
use icu_datetime::input::Date;
use icu_decimal::DecimalFormatter;
use icu_decimal::input::Decimal;
use icu_locale_core::Locale;

use crate::{Key, UiLocale, embedded_sources, pseudo};

type Bundle = FluentBundle<FluentResource>;

/// Everything needed to render UI text for one locale.
///
/// Cheap to read and `Send + Sync`; build a new one to switch language.
pub struct Localizer {
    ui_locale: UiLocale,
    bundle: Bundle,
    /// `None` when the active locale is the fallback itself.
    fallback: Option<Bundle>,
    icu_locale: Locale,
    decimal: DecimalFormatter,
    date: DateTimeFormatter<YMD>,
}

impl std::fmt::Debug for Localizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Localizer")
            .field("ui_locale", &self.ui_locale)
            .finish_non_exhaustive()
    }
}

impl Localizer {
    /// Builds a localizer for `ui_locale`.
    ///
    /// # Panics
    /// Panics if embedded FTL fails to parse or ICU4X compiled data lacks the locale. Both are
    /// checked by tests and `cargo xtask i18n-check`, so a release build cannot hit them.
    pub fn new(ui_locale: UiLocale) -> Self {
        let bundle = build_bundle(ui_locale);
        let fallback = (ui_locale.resource_tag() != UiLocale::FALLBACK.tag())
            .then(|| build_bundle(UiLocale::FALLBACK));
        let icu_locale = Locale::try_from_str(ui_locale.resource_tag())
            .unwrap_or_else(|_| unreachable!("static tags are valid"));
        let decimal = DecimalFormatter::try_new(icu_locale.clone().into(), Default::default())
            .expect("ICU4X compiled data includes decimal symbols for shipped locales");
        let date = DateTimeFormatter::try_new(icu_locale.clone().into(), YMD::long())
            .expect("ICU4X compiled data includes date patterns for shipped locales");
        Self {
            ui_locale,
            bundle,
            fallback,
            icu_locale,
            decimal,
            date,
        }
    }

    /// Builds a localizer for the operating system's preferred language.
    pub fn system() -> Self {
        Self::new(crate::detect_system_locale())
    }

    /// The active UI locale.
    pub fn ui_locale(&self) -> UiLocale {
        self.ui_locale
    }

    /// Formats a message that takes no variables.
    pub fn tr(&self, key: Key) -> String {
        self.format(key, None)
    }

    /// Formats a message with variables; see [`crate::args!`].
    pub fn tr_args(&self, key: Key, args: &FluentArgs<'_>) -> String {
        if cfg!(debug_assertions) {
            for expected in key.variables() {
                debug_assert!(
                    args.get(*expected).is_some(),
                    "message {} expects ${expected}",
                    key.id()
                );
            }
        }
        self.format(key, Some(args))
    }

    /// Formats an integer with locale grouping (`1,234` / `1.234` / `1,234`).
    pub fn format_integer(&self, value: i64) -> String {
        self.decimal.format(&Decimal::from(value)).to_string()
    }

    /// Formats a calendar date in the locale's long style
    /// (`November 21, 2026` / `21 Kasım 2026` / `2026年11月21日`).
    ///
    /// Returns `None` for an invalid date.
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> Option<String> {
        let date = Date::try_new_iso(year, month, day).ok()?;
        Some(self.date.format(&date).to_string())
    }

    /// Uppercases with the locale's rules (Turkish: `i` → `İ`). Never use `str::to_uppercase`
    /// on user-visible text.
    pub fn uppercase(&self, text: &str) -> String {
        CaseMapper::new()
            .uppercase_to_string(text, &self.icu_locale.id)
            .into_owned()
    }

    /// Keys missing from the active locale (they render from the fallback instead).
    pub fn missing_keys(&self) -> Vec<Key> {
        Key::ALL
            .iter()
            .copied()
            .filter(|key| !self.bundle.has_message(key.id()))
            .collect()
    }

    fn format(&self, key: Key, args: Option<&FluentArgs<'_>>) -> String {
        let bundle = if self.bundle.has_message(key.id()) {
            &self.bundle
        } else if let Some(fallback) = &self.fallback {
            fallback
        } else {
            return key.id().to_owned();
        };
        let Some(pattern) = bundle.get_message(key.id()).and_then(|m| m.value()) else {
            return key.id().to_owned();
        };
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned();
        debug_assert!(errors.is_empty(), "formatting {}: {errors:?}", key.id());
        if self.ui_locale == UiLocale::Pseudo {
            pseudo::transform(&text)
        } else {
            text
        }
    }
}

fn build_bundle(locale: UiLocale) -> Bundle {
    let mut bundle = Bundle::new_concurrent(vec![locale.langid()]);
    // Fluent wraps placeables in FSI/PDI marks for bidi safety. No shipped locale is RTL yet, and
    // the marks show up in copied text and grapheme counts; re-enable when an RTL locale lands.
    bundle.set_use_isolating(false);
    let sources = embedded_sources(locale.resource_tag())
        .unwrap_or_else(|| panic!("no embedded resources for {}", locale.resource_tag()));
    for (file, source) in sources {
        let resource = FluentResource::try_new((*source).to_owned())
            .unwrap_or_else(|(_, errors)| panic!("{}/{file}: {errors:?}", locale.resource_tag()));
        bundle
            .add_resource(resource)
            .unwrap_or_else(|errors| panic!("{}/{file}: {errors:?}", locale.resource_tag()));
    }
    bundle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    #[test]
    fn shipped_locales_are_complete() {
        for locale in UiLocale::SHIPPED {
            let l10n = Localizer::new(locale);
            assert!(
                l10n.missing_keys().is_empty(),
                "{locale:?} missing {:?}",
                l10n.missing_keys()
            );
        }
    }

    #[test]
    fn every_key_formats_in_every_locale() {
        for locale in UiLocale::selectable() {
            let l10n = Localizer::new(locale);
            for key in Key::ALL {
                let mut args = FluentArgs::new();
                for var in key.variables() {
                    args.set(*var, 2);
                }
                let text = l10n.tr_args(*key, &args);
                assert!(
                    !text.is_empty() && text != key.id(),
                    "{locale:?} {}",
                    key.id()
                );
            }
        }
    }

    #[test]
    fn plurals_follow_locale_rules() {
        let en = Localizer::new(UiLocale::EnUs);
        assert_eq!(
            en.tr_args(Key::MessageCount, &args!(count = 1)),
            "1 message"
        );
        assert_eq!(
            en.tr_args(Key::MessageCount, &args!(count = 5)),
            "5 messages"
        );
        let tr = Localizer::new(UiLocale::TrTr);
        assert_eq!(tr.tr_args(Key::MessageCount, &args!(count = 5)), "5 mesaj");
        let ja = Localizer::new(UiLocale::JaJp);
        assert_eq!(
            ja.tr_args(Key::MessageCount, &args!(count = 1)),
            "1 件のメッセージ"
        );
    }

    #[test]
    fn numbers_use_locale_grouping() {
        assert_eq!(
            Localizer::new(UiLocale::EnUs).format_integer(1_234_567),
            "1,234,567"
        );
        assert_eq!(
            Localizer::new(UiLocale::TrTr).format_integer(1_234_567),
            "1.234.567"
        );
        assert_eq!(
            Localizer::new(UiLocale::JaJp).format_integer(1_234_567),
            "1,234,567"
        );
    }

    #[test]
    fn dates_use_locale_patterns() {
        let date = |l| {
            Localizer::new(l)
                .format_date(2026, 11, 21)
                .unwrap_or_default()
        };
        assert_eq!(date(UiLocale::EnUs), "November 21, 2026");
        assert_eq!(date(UiLocale::TrTr), "21 Kasım 2026");
        assert_eq!(date(UiLocale::JaJp), "2026年11月21日");
        assert!(
            Localizer::new(UiLocale::EnUs)
                .format_date(2026, 2, 30)
                .is_none()
        );
    }

    #[test]
    fn uppercase_is_turkish_aware() {
        assert_eq!(
            Localizer::new(UiLocale::TrTr).uppercase("istanbul ılık"),
            "İSTANBUL ILIK"
        );
        assert_eq!(
            Localizer::new(UiLocale::EnUs).uppercase("istanbul"),
            "ISTANBUL"
        );
    }

    #[test]
    fn pseudo_locale_transforms_source_text() {
        let text = Localizer::new(UiLocale::Pseudo).tr(Key::SettingsTitle);
        assert!(text.starts_with('[') && text.ends_with(']'));
        assert_ne!(text, Localizer::new(UiLocale::EnUs).tr(Key::SettingsTitle));
    }

    #[test]
    fn localizer_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Localizer>();
    }
}
