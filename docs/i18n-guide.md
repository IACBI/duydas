# Localization guide

Duydaş launches in **English (en-US)**, **Turkish (tr-TR)** and **Japanese (ja-JP)**. en-US is the source and fallback language.

## How it works

- Messages are [Fluent](https://projectfluent.org/) files in `locales/<tag>/`, one file per feature area (`common.ftl`, `chat.ftl`, `settings.ftl`, …).
- The build generates a `Key` enum from `locales/en-US`, so code refers to `Key::ComposerPlaceholder` instead of a string id. A typo is a compile error.
- All locales are embedded in the binary. Switching language builds a new `Localizer` and the UI redraws; no restart and no file I/O.
- The UI language follows the operating system (`tr`, `tr-CY` → tr-TR; `ja` → ja-JP; anything unsupported → en-US) until the user picks one.
- A message missing in a translation falls back to en-US for that message only. Release builds must have no fallbacks; `cargo xtask i18n-check` and the test suite enforce it.

## Adding or changing UI text (developers)

1. Add the message to the right `locales/en-US/*.ftl` with a comment that explains where it appears and documents every variable:

   ```ftl
   # Placeholder in the empty message box.
   # Variables:
   #   $channel (String) - channel name without the leading #, e.g. "general"
   composer-placeholder = Message #{ $channel }
   ```

2. Use it: `l10n.tr_args(Key::ComposerPlaceholder, &args!(channel = name))`.
3. Add tr-TR and ja-JP translations, or say in the PR that they are needed.
4. Run `cargo xtask i18n-check`.

Rules:

- Never build sentences by concatenating strings; give translators the whole sentence with variables.
- Pass **numbers** as integers when a plural form depends on them. An integer variable selects the plural variant and is inserted with locale grouping (`1.234` in Turkish).
- Format dates with `l10n.format_date`, never by hand.
- Uppercase with `l10n.uppercase`. `str::to_uppercase` turns Turkish `i` into `I` instead of `İ`.
- Don't inflect the product name inside Turkish sentences (`Duydaş'a`); phrase around it.
- Limits on user input count grapheme clusters, not bytes or `char`s.

## Translating (no Rust needed)

1. Copy the en-US message id and translate the text after `=`. Keep `{ $variables }` and `{ message-references }` as they are.
2. Use your language's plural categories. Turkish usually needs no plural selector after a number (`{ $count } mesaj`); Japanese has none (`{ $count } 件のメッセージ`).
3. Run `cargo xtask i18n-check`. It reports missing messages, unknown variables and messages that don't exist in en-US.
4. Try the pseudo-locale (`[Þšéûðö]` in debug builds) to see whether a layout can absorb longer text.

## Adding a language

Open a [new language issue](https://github.com/IACBI/duydas/issues/new?template=new_language.yml), then add `locales/<tag>/` with all files, add the variant to `UiLocale` in `crates/duydas-i18n/src/locale.rs`, and add the language's CLDR rules (plural categories, grouping separator, long date pattern and month names, any casing rules) to `crates/duydas-i18n/src/format.rs` with tests.

## Text input (IME) test matrix

The composer must handle input methods correctly. **Enter while a composition is active must commit the composition and never send the message.** Run this matrix on each release candidate and record results in the PR:

| Platform | Input method | Preedit shown inline | Candidate window at caret | Enter commits, does not send | Shift+Enter newline | After commit, Enter sends |
|---|---|---|---|---|---|---|
| Windows 11 | Microsoft IME (Japanese) | | | | | |
| macOS | Japanese Input (Romaji) | | | | | |
| Linux Wayland (GNOME) | IBus + Mozc | | | | | |
| Linux Wayland (KDE) | fcitx5 + Mozc | | | | | |
| Linux X11 | fcitx5 + Mozc | | | | | |

Also check: switching keyboard layouts with Win+Space while the window is focused (Iced issue #3189), DPI changes while composing, and switching UI language during a composition.

## Fonts

Japanese text needs Japanese glyph forms. cosmic-text picks its Han fallback font from the system locale, so on an English system kanji can render with Chinese shapes. Bundling a Noto Sans JP subset and ordering fonts by UI language is planned; see [ADR 0015](adr/0015-spike-text-and-ime.md).

## Supported syntax

Duydaş parses FTL itself (ADR 0022). It supports messages, terms, attributes, comments, multiline text, variables, message and term references, string and number literals, and select expressions (plural categories and exact numbers such as `[0]`). It does not support functions such as `NUMBER()` or parameterized terms; the code passes values in already formatted, e.g. `$countText` next to `$count`. Unsupported syntax fails `cargo xtask i18n-check` with a line and column.
