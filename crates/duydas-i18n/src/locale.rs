use crate::format::Lang;

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

    /// The language whose formatting and plural rules apply.
    pub(crate) const fn lang(self) -> Lang {
        match self {
            UiLocale::EnUs | UiLocale::Pseudo => Lang::En,
            UiLocale::TrTr => Lang::Tr,
            UiLocale::JaJp => Lang::Ja,
        }
    }

    /// Language and region subtags of the resource tag, lowercase.
    fn subtags(self) -> (&'static str, &'static str) {
        match self {
            UiLocale::EnUs | UiLocale::Pseudo => ("en", "us"),
            UiLocale::TrTr => ("tr", "tr"),
            UiLocale::JaJp => ("ja", "jp"),
        }
    }
}

/// Picks the best shipped locale for a list of requested tags (most preferred first).
///
/// For each requested tag in order, an exact language-and-region match wins, then any shipped
/// locale with the same language; so `tr` and `tr-CY` resolve to tr-TR and `ja` to ja-JP.
/// Accepts POSIX forms such as `tr_TR.UTF-8@euro`. Falls back to [`UiLocale::FALLBACK`].
pub fn negotiate<S: AsRef<str>>(requested: &[S]) -> UiLocale {
    for tag in requested {
        let Some((language, region)) = parse_tag(tag.as_ref()) else {
            continue;
        };
        let (language, region) = (language.as_str(), region.as_deref());
        let same_language = || {
            UiLocale::SHIPPED
                .into_iter()
                .filter(move |l| l.subtags().0 == language)
        };
        if let Some(exact) = same_language().find(|l| Some(l.subtags().1) == region) {
            return exact;
        }
        if let Some(close) = same_language().next() {
            return close;
        }
    }
    UiLocale::FALLBACK
}

/// Extracts the lowercase language subtag and, if present, the region subtag from a BCP 47 or
/// POSIX locale name: `language[-script][-region](-variant)*(-singleton(-ext)+)*[-x(-private)+]`.
/// Malformed tags, "C" and "POSIX" yield nothing.
fn parse_tag(tag: &str) -> Option<(String, Option<String>)> {
    let tag = tag.split(['.', '@']).next()?.trim();
    let mut parts = tag.split(['-', '_']).peekable();
    let alpha = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
    let alnum = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());

    let language = parts.next()?;
    if !(2..=3).contains(&language.len()) || !alpha(language) {
        return None;
    }
    parts.next_if(|s| s.len() == 4 && alpha(s));
    let region = parts
        .next_if(|s| (s.len() == 2 && alpha(s)) || (s.len() == 3 && digits(s)))
        .map(str::to_ascii_lowercase);
    while parts
        .next_if(|s| {
            alnum(s)
                && ((5..=8).contains(&s.len())
                    || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit()))
        })
        .is_some()
    {}
    while let Some(singleton) = parts.next() {
        if singleton.len() != 1 || !alnum(singleton) {
            return None;
        }
        let private = singleton.eq_ignore_ascii_case("x");
        let min = if private { 1 } else { 2 };
        let mut count = 0;
        while parts
            .next_if(|s| alnum(s) && (min..=8).contains(&s.len()) && (private || s.len() > 1))
            .is_some()
        {
            count += 1;
        }
        if count == 0 {
            return None;
        }
        if private && parts.peek().is_some() {
            return None;
        }
    }
    Some((language.to_ascii_lowercase(), region))
}

/// Negotiates against the operating system's preferred languages.
pub fn detect_system_locale() -> UiLocale {
    negotiate(&duydas_sys::locale::preferred_languages())
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
        assert_eq!(negotiate(&["tr_TR.UTF-8@euro"]), UiLocale::TrTr);
        assert_eq!(negotiate(&["ja-Jpan-JP"]), UiLocale::JaJp);
    }

    #[test]
    fn ignores_unusable_tags() {
        assert_eq!(
            negotiate(&["C", "POSIX", "", "x-private", "tr"]),
            UiLocale::TrTr
        );
        assert_eq!(negotiate(&["zh-Hant-TW"]), UiLocale::EnUs);
        assert_eq!(negotiate(&["ja-!", "tr-TR"]), UiLocale::TrTr);
        assert_eq!(negotiate(&["ja--JP", "tr"]), UiLocale::TrTr);
        // A region after a variant is not a region: falls back to the language.
        assert_eq!(negotiate(&["ja-u", "tr-TR"]), UiLocale::TrTr);
        assert_eq!(negotiate(&["ja-polyton-TR", "tr"]), UiLocale::TrTr);
        assert_eq!(negotiate(&["ja-JP-u-ca-japanese-x-dev"]), UiLocale::JaJp);
        assert_eq!(negotiate(&["en-Latn-US-valencia"]), UiLocale::EnUs);
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
