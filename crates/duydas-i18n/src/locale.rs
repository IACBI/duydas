use unic_langid::LanguageIdentifier;

/// A UI language the app ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiLocale {
    /// English (United States). Source and fallback locale.
    EnUs,
    /// Turkish (Türkiye).
    TrTr,
    /// Japanese (Japan).
    JaJp,
    /// Pseudo-localized English for testing text expansion and missing translations.
    /// Only offered in debug builds.
    Pseudo,
}

impl UiLocale {
    /// Locales offered to users, in picker order.
    pub const SHIPPED: [UiLocale; 3] = [UiLocale::EnUs, UiLocale::TrTr, UiLocale::JaJp];

    /// The fallback used when nothing else matches and for per-message fallback.
    pub const FALLBACK: UiLocale = UiLocale::EnUs;

    /// Locales shown in the language picker (adds [`UiLocale::Pseudo`] in debug builds).
    pub fn selectable() -> Vec<UiLocale> {
        let mut list = Self::SHIPPED.to_vec();
        if cfg!(debug_assertions) {
            list.push(UiLocale::Pseudo);
        }
        list
    }

    /// BCP 47 tag; also the directory name under `locales/`.
    pub const fn tag(self) -> &'static str {
        match self {
            UiLocale::EnUs => "en-US",
            UiLocale::TrTr => "tr-TR",
            UiLocale::JaJp => "ja-JP",
            UiLocale::Pseudo => "en-XA",
        }
    }

    /// The locale whose FTL resources back this UI locale.
    pub(crate) const fn resource_tag(self) -> &'static str {
        match self {
            UiLocale::Pseudo => UiLocale::EnUs.tag(),
            other => other.tag(),
        }
    }

    /// The language's name in that language. Autonyms are deliberately the same in every UI
    /// language so people can find their own language even when the UI is in one they can't read.
    pub const fn autonym(self) -> &'static str {
        match self {
            UiLocale::EnUs => "English",
            UiLocale::TrTr => "Türkçe",
            UiLocale::JaJp => "日本語",
            UiLocale::Pseudo => "[Þšéûðö]",
        }
    }

    /// Parses a BCP 47 tag exactly as returned by [`UiLocale::tag`].
    pub fn from_tag(tag: &str) -> Option<UiLocale> {
        [
            UiLocale::EnUs,
            UiLocale::TrTr,
            UiLocale::JaJp,
            UiLocale::Pseudo,
        ]
        .into_iter()
        .find(|l| l.tag().eq_ignore_ascii_case(tag))
    }

    pub(crate) fn langid(self) -> LanguageIdentifier {
        self.resource_tag()
            .parse()
            .unwrap_or_else(|_| unreachable!("static tags are valid"))
    }
}

/// Picks the best shipped locale for a list of requested tags (most preferred first).
///
/// Matching ignores region when needed, so `tr` and `tr-CY` resolve to tr-TR and `ja` to ja-JP.
/// Falls back to [`UiLocale::FALLBACK`].
pub fn negotiate<S: AsRef<str>>(requested: &[S]) -> UiLocale {
    let requested: Vec<LanguageIdentifier> = requested
        .iter()
        .filter_map(|tag| tag.as_ref().replace('_', "-").parse().ok())
        .collect();
    let available: Vec<LanguageIdentifier> = UiLocale::SHIPPED.iter().map(|l| l.langid()).collect();
    let default = UiLocale::FALLBACK.langid();
    let chosen = fluent_langneg::negotiate_languages(
        &requested,
        &available,
        Some(&default),
        fluent_langneg::NegotiationStrategy::Lookup,
    );
    chosen
        .first()
        .and_then(|id| UiLocale::from_tag(&id.to_string()))
        .unwrap_or(UiLocale::FALLBACK)
}

/// Negotiates against the operating system's preferred languages.
pub fn detect_system_locale() -> UiLocale {
    let system: Vec<String> = sys_locale::get_locales().collect();
    negotiate(&system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_region_variants() {
        assert_eq!(negotiate(&["tr"]), UiLocale::TrTr);
        assert_eq!(negotiate(&["ja"]), UiLocale::JaJp);
        assert_eq!(negotiate(&["ja_JP"]), UiLocale::JaJp);
        assert_eq!(negotiate(&["en-GB"]), UiLocale::EnUs);
    }

    #[test]
    fn respects_preference_order_and_falls_back() {
        assert_eq!(negotiate(&["de-DE", "ja-JP", "tr-TR"]), UiLocale::JaJp);
        assert_eq!(negotiate(&["de-DE", "fr-FR"]), UiLocale::EnUs);
        assert_eq!(negotiate::<&str>(&[]), UiLocale::EnUs);
    }

    #[test]
    fn tags_round_trip() {
        for locale in [
            UiLocale::EnUs,
            UiLocale::TrTr,
            UiLocale::JaJp,
            UiLocale::Pseudo,
        ] {
            assert_eq!(UiLocale::from_tag(locale.tag()), Some(locale));
        }
    }
}
