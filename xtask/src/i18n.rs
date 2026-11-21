use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../../crates/duydas-i18n/src/schema.rs"]
#[allow(dead_code, unreachable_pub)]
mod schema;

use schema::MessageInfo;

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
        for (line_no, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if passes_literal_to_sink(line) {
                errors.push(format!(
                    "{rel}:{}: hardcoded UI string; add an FTL message and use Key::…",
                    line_no + 1
                ));
            }
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

/// True if `line` calls one of [`TEXT_SINKS`] with a string literal, e.g. `text("Hi")`,
/// `text::<Theme>("Hi")` or `.placeholder( "Hi")`, but not `context("Hi")`.
fn passes_literal_to_sink(line: &str) -> bool {
    let bytes = line.as_bytes();
    TEXT_SINKS.iter().any(|sink| {
        line.match_indices(sink).any(|(start, _)| {
            let before = start.checked_sub(1).map(|i| bytes[i]);
            if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return false;
            }
            let mut rest = &line[start + sink.len()..];
            if let Some(after_turbofish) = rest.strip_prefix("::<") {
                let mut depth = 1usize;
                let mut end = None;
                for (i, c) in after_turbofish.char_indices() {
                    match c {
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let Some(end) = end else { return false };
                rest = &after_turbofish[end..];
            }
            rest.strip_prefix('(')
                .is_some_and(|args| args.trim_start().starts_with('"'))
        })
    })
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
    use super::passes_literal_to_sink as hit;

    #[test]
    fn detects_literals_passed_to_text_sinks() {
        assert!(hit(r#"text("Hello")"#));
        assert!(hit(r#"    .placeholder( "Type here")"#));
        assert!(hit(r#"iced::widget::text::<Theme, Renderer>("Hi")"#));
        assert!(hit(r#"button("Send").on_press(m)"#));
    }

    #[test]
    fn ignores_localized_and_unrelated_calls() {
        assert!(!hit("text(l.tr(Key::SettingsTitle))"));
        assert!(!hit(r#"context("not ui")"#));
        assert!(!hit(r#"my_text("helper")"#));
        assert!(!hit("text(m.as_str())"));
    }
}
