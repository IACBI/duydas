use crate::bundle::{Bundle, Transform};
use crate::{Args, Key, UiLocale, embedded_sources, ftl, pseudo, schema};

/// Everything needed to render UI text for one locale.
///
/// Cheap to read and `Send + Sync`; build a new one to switch language.
#[derive(Debug)]
pub struct Localizer {
    ui_locale: UiLocale,
    bundle: Bundle,
    /// `None` when the active locale is the fallback itself.
    fallback: Option<Bundle>,
}

impl Localizer {
    /// Builds a localizer for `ui_locale`.
    ///
    /// # Panics
    /// Panics if embedded FTL fails to parse or validate, which cannot happen in a built crate:
    /// `build.rs` runs the same validation on every locale and fails the build instead.
    pub fn new(ui_locale: UiLocale) -> Self {
        let bundle = build_bundle(ui_locale).unwrap_or_else(|errors| panic!("{errors:?}"));
        let fallback = (ui_locale.resource_tag() != UiLocale::FALLBACK.tag()).then(|| {
            build_bundle(UiLocale::FALLBACK).unwrap_or_else(|errors| panic!("{errors:?}"))
        });
        Self {
            ui_locale,
            bundle,
            fallback,
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
    pub fn tr_args(&self, key: Key, args: &Args<'_>) -> String {
        if cfg!(debug_assertions) {
            for expected in key.variables() {
                debug_assert!(
                    args.get(expected).is_some(),
                    "message {} expects ${expected}",
                    key.id()
                );
            }
        }
        self.format(key, Some(args))
    }

    /// Formats an integer with locale grouping (`1,234` / `1.234` / `1,234`).
    pub fn format_integer(&self, value: i64) -> String {
        self.ui_locale.lang().format_integer(value)
    }

    /// Formats a calendar date in the locale's long style
    /// (`November 21, 2026` / `21 Kasım 2026` / `2026年11月21日`).
    ///
    /// Returns `None` for a date that does not exist.
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> Option<String> {
        self.ui_locale.lang().format_date(year, month, day)
    }

    /// Uppercases with the locale's rules (Turkish: `i` → `İ`). Never use `str::to_uppercase`
    /// on user-visible text.
    pub fn uppercase(&self, text: &str) -> String {
        self.ui_locale.lang().uppercase(text)
    }

    /// Keys whose value is missing from the active locale (they render from the fallback).
    pub fn missing_keys(&self) -> Vec<Key> {
        Key::ALL
            .iter()
            .copied()
            .filter(|key| !self.bundle.has_value(key.id()))
            .collect()
    }

    fn format(&self, key: Key, args: Option<&Args<'_>>) -> String {
        let text = format_value(&self.bundle, key, args)
            .or_else(|| {
                self.fallback
                    .as_ref()
                    .and_then(|f| format_value(f, key, args))
            })
            .unwrap_or_else(|| key.id().to_owned());
        if self.ui_locale == UiLocale::Pseudo {
            pseudo::wrap(&text)
        } else {
            text
        }
    }
}

/// Formats `key`'s value, or `None` if the bundle lacks it or resolution failed (for example a
/// translation using a variable the code does not pass), so the caller falls back to the source
/// locale instead of showing partial text. `cargo xtask i18n-check` reports such translations.
fn format_value(bundle: &Bundle, key: Key, args: Option<&Args<'_>>) -> Option<String> {
    bundle.format(key.id(), args).ok()
}

/// Builds and validates every shipped locale's bundle without panicking.
///
/// # Errors
/// Returns one message per syntax error, duplicate id or dangling reference.
pub fn validate_shipped_bundles() -> Result<(), Vec<String>> {
    let errors: Vec<String> = UiLocale::SHIPPED
        .iter()
        .filter_map(|locale| build_bundle(*locale).err())
        .flatten()
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn build_bundle(locale: UiLocale) -> Result<Bundle, Vec<String>> {
    let tag = locale.resource_tag();
    let sources =
        embedded_sources(tag).ok_or_else(|| vec![format!("no embedded resources for {tag}")])?;
    let mut errors = Vec::new();
    let mut parsed = Vec::with_capacity(sources.len());
    for (file, source) in sources {
        let (resource, syntax) = ftl::parse(source);
        errors.extend(syntax.iter().map(|e| format!("{tag}/{file}:{e}")));
        parsed.push((*file, resource));
    }
    let views: Vec<_> = parsed
        .iter()
        .map(|(file, resource)| (*file, resource))
        .collect();
    errors.extend(
        schema::validate(&views)
            .into_iter()
            .map(|e| format!("{tag}: {e}")),
    );
    if !errors.is_empty() {
        return Err(errors);
    }
    let transform = (locale == UiLocale::Pseudo).then_some(pseudo::accent_text as Transform);
    let mut bundle = Bundle::new(locale.lang(), transform);
    for (_, resource) in parsed {
        bundle.add(resource);
    }
    Ok(bundle)
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
                let mut args = Args::new();
                for var in key.variables() {
                    args.set(var, 2);
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
        let count = |l: &Localizer, n: i64| {
            l.tr_args(
                Key::MessageCount,
                &args!(count = n, countText = l.format_integer(n)),
            )
        };
        let en = Localizer::new(UiLocale::EnUs);
        assert_eq!(count(&en, 1), "1 message");
        assert_eq!(count(&en, 1234), "1,234 messages");
        let tr = Localizer::new(UiLocale::TrTr);
        assert_eq!(count(&tr, 1234), "1.234 mesaj");
        let ja = Localizer::new(UiLocale::JaJp);
        assert_eq!(count(&ja, 1), "1 件のメッセージ");
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
    fn pseudo_locale_leaves_user_data_readable() {
        let text = Localizer::new(UiLocale::Pseudo)
            .tr_args(Key::ComposerPlaceholder, &args!(channel = "general"));
        assert!(text.contains("#general"), "{text}");
        assert!(text.contains("Méššåĝé"), "{text}");
    }

    #[test]
    fn shipped_bundles_validate() {
        assert_eq!(validate_shipped_bundles(), Ok(()));
    }

    #[test]
    fn localizer_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Localizer>();
    }
}
