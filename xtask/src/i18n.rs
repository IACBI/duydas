use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use duydas_i18n::UiLocale;
use duydas_i18n::schema::{self, MessageInfo};

const SOURCE_LOCALE: &str = "en-US";

/// Directories whose Rust code renders UI and must not contain literal UI strings.
const UI_CODE_DIRS: &[&str] = &["apps/duydas-desktop/src", "crates/duydas-ui/src"];

/// Functions and methods that take display text. A string literal as their first argument is a
/// hardcoded UI string.
const TEXT_SINKS: &[&str] = &[
    "text",
    "button",
    "tooltip",
    "text_input",
    "placeholder",
    "title",
];

pub(crate) fn check(root: &Path, strict: bool) -> Result<(), String> {
    let locales_dir = root.join("locales");
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let locales = load_locales(&locales_dir, &mut errors)?;
    for locale in UiLocale::SHIPPED {
        if !locales.contains_key(locale.tag()) {
            errors.push(format!(
                "shipped locale {} has no locales/{} directory",
                locale.tag(),
                locale.tag()
            ));
        }
    }
    // Full Fluent validation (duplicate terms, resolver-level problems) on top of the schema diff.
    if let Err(bundle_errors) = duydas_i18n::validate_shipped_bundles() {
        errors.extend(bundle_errors);
    }
    let source = locales
        .get(SOURCE_LOCALE)
        .ok_or_else(|| format!("missing source locale {SOURCE_LOCALE}"))?;

    for (tag, messages) in &locales {
        if tag == SOURCE_LOCALE {
            continue;
        }
        for (id, src) in source {
            let Some(msg) = messages.get(id) else {
                errors.push(format!("{tag}: missing message `{id}`"));
                continue;
            };
            if src.has_value != msg.has_value {
                errors.push(format!(
                    "{tag}: `{id}` value presence differs from {SOURCE_LOCALE}"
                ));
            }
            if src.attributes != msg.attributes {
                errors.push(format!(
                    "{tag}: `{id}` attributes {:?} differ from {SOURCE_LOCALE} {:?}",
                    msg.attributes, src.attributes
                ));
            }
            for var in msg.variables.difference(&src.variables) {
                errors.push(format!(
                    "{tag}: `{id}` uses ${var}, which {SOURCE_LOCALE} never passes"
                ));
            }
            for var in src.variables.difference(&msg.variables) {
                // Allowed (e.g. a language without a plural form may drop a count), but suspicious.
                warnings.push(format!("{tag}: `{id}` does not use ${var}"));
            }
        }
        for id in messages.keys().filter(|id| !source.contains_key(*id)) {
            errors.push(format!("{tag}: `{id}` does not exist in {SOURCE_LOCALE}"));
        }
    }

    let code = collect_rust_sources(root);
    for id in source.keys() {
        let variant = format!("Key::{}", schema::variant_name(id));
        let used = code.iter().any(|(_, text)| {
            text.match_indices(&variant).any(|(i, _)| {
                // `Key::Foo` must not match `Key::FooBar`.
                !text[i + variant.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            })
        });
        if !used {
            warnings.push(format!("unused message `{id}` ({variant})"));
        }
    }

    for (path, text) in &code {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if !UI_CODE_DIRS.iter().any(|dir| rel.starts_with(dir)) {
            continue;
        }
        let code = strip_comments(text);
        for offset in literal_sink_calls(&code) {
            let line_no = code[..offset].matches('\n').count() + 1;
            errors.push(format!(
                "{rel}:{line_no}: hardcoded UI string; add an FTL message and use Key::…"
            ));
        }
    }

    for w in &warnings {
        println!("warning: {w}");
    }
    for e in &errors {
        println!("error: {e}");
    }
    let total_keys = source.len();
    println!(
        "i18n-check: {} locales, {total_keys} messages, {} errors, {} warnings",
        locales.len(),
        errors.len(),
        warnings.len()
    );
    if !errors.is_empty() || (strict && !warnings.is_empty()) {
        return Err("localization check failed".into());
    }
    Ok(())
}

/// Byte offsets where one of [`TEXT_SINKS`] is called with a string literal as its first
/// argument: `text("Hi")`, `text::<Theme>("Hi")`, `.placeholder(\n r#"Hi"#)`, but not
/// `context("Hi")` or `text(l.tr(..))`. Expects comments already blanked by [`strip_comments`].
fn literal_sink_calls(code: &str) -> Vec<usize> {
    let bytes = code.as_bytes();
    let mut hits = Vec::new();
    for sink in TEXT_SINKS {
        for (start, _) in code.match_indices(sink) {
            let before = start.checked_sub(1).map(|i| bytes[i]);
            if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                continue;
            }
            let mut rest = &code[start + sink.len()..];
            if let Some(generics) = rest.strip_prefix("::<") {
                let Some(end) = matching_angle(generics) else {
                    continue;
                };
                rest = &generics[end..];
            }
            let Some(args) = rest.strip_prefix('(') else {
                continue;
            };
            let arg = args.trim_start();
            let raw = arg.strip_prefix('r').map(|r| r.trim_start_matches('#'));
            if arg.starts_with('"') || raw.is_some_and(|r| r.starts_with('"')) {
                hits.push(start);
            }
        }
    }
    hits.sort_unstable();
    hits
}

/// Index just past the `>` closing a turbofish whose opening `<` was already consumed.
fn matching_angle(s: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Replaces `//` and `/* */` comments with spaces (keeping newlines, so offsets and line numbers
/// survive) while leaving string and char literals intact.
fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while i < chars.len() {
        match (chars[i], chars.get(i + 1).copied()) {
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                let mut depth = 0usize;
                while i < chars.len() {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        out.push_str("  ");
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        out.push_str("  ");
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        out.push(blank(chars[i]));
                        i += 1;
                    }
                }
            }
            ('"', _) => {
                out.push('"');
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i]);
                        i += 1;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    out.push('"');
                    i += 1;
                }
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

type LocaleMessages = BTreeMap<String, MessageInfo>;

fn load_locales(
    dir: &Path,
    errors: &mut Vec<String>,
) -> Result<BTreeMap<String, LocaleMessages>, String> {
    let mut locales = BTreeMap::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let tag = entry.file_name().to_string_lossy().into_owned();
        let mut messages = LocaleMessages::new();
        let mut files: Vec<PathBuf> = fs::read_dir(entry.path())
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "ftl"))
            .collect();
        files.sort();
        let mut seen = BTreeSet::new();
        for file in files {
            let name = format!(
                "{tag}/{}",
                file.file_name().unwrap_or_default().to_string_lossy()
            );
            let text = fs::read_to_string(&file).map_err(|e| format!("{name}: {e}"))?;
            match schema::collect_messages(&text) {
                Ok(parsed) => {
                    for msg in parsed {
                        if !seen.insert(msg.id.clone()) {
                            errors.push(format!("{name}: duplicate message `{}`", msg.id));
                        }
                        messages.insert(msg.id.clone(), msg);
                    }
                }
                Err(parse_errors) => {
                    for e in parse_errors {
                        errors.push(format!("{name}: {e}"));
                    }
                }
            }
        }
        locales.insert(tag, messages);
    }
    Ok(locales)
}

fn collect_rust_sources(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for top in ["apps", "crates"] {
        walk(&root.join(top), &mut out);
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            out.push((path, text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{literal_sink_calls, strip_comments};

    fn hit(src: &str) -> bool {
        !literal_sink_calls(&strip_comments(src)).is_empty()
    }

    #[test]
    fn detects_literals_passed_to_text_sinks() {
        assert!(hit(r#"text("Hello")"#));
        assert!(hit(r#"    .placeholder( "Type here")"#));
        assert!(hit(r#"iced::widget::text::<Theme, Renderer>("Hi")"#));
        assert!(hit(r#"button("Send").on_press(m)"#));
        assert!(hit("text(\n    \"multi-line\"\n)"));
        assert!(hit(r##"text(r#"raw"#)"##));
    }

    #[test]
    fn ignores_localized_unrelated_and_commented_calls() {
        assert!(!hit("text(l.tr(Key::SettingsTitle))"));
        assert!(!hit(r#"context("not ui")"#));
        assert!(!hit(r#"my_text("helper")"#));
        assert!(!hit("text(m.as_str())"));
        assert!(!hit(r#"// text("commented")"#));
        assert!(!hit(r#"/* text("block") */ let a = 1;"#));
        assert!(!hit(r#"let s = "text(\"inside a string\")";"#));
    }

    #[test]
    fn line_numbers_survive_comment_stripping() {
        let src = "/* one\ntwo */\nlet x = text(\"Hi\");\n";
        let code = strip_comments(src);
        let offset = literal_sink_calls(&code)[0];
        assert_eq!(code[..offset].matches('\n').count() + 1, 3);
    }
}
