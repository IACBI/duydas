//! Validates every locale's FTL, generates the typed `Key` enum from the en-US source locale and
//! embeds the files.

#![allow(clippy::unwrap_used, reason = "writing into a String is infallible")]

#[path = "src/ftl.rs"]
#[allow(dead_code, unreachable_pub)]
mod ftl;
#[path = "src/schema.rs"]
#[allow(dead_code, unreachable_pub)]
mod schema;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

const SOURCE_LOCALE: &str = "en-US";

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let locales_dir = manifest.join("../../locales");
    println!("cargo:rerun-if-changed={}", locales_dir.display());

    let locales = read_locales(&locales_dir);
    validate_all(&locales);
    let source = locales.get(SOURCE_LOCALE).unwrap_or_else(|| {
        panic!(
            "source locale {SOURCE_LOCALE} missing in {}",
            locales_dir.display()
        )
    });

    let mut messages = Vec::new();
    for (file, text) in source {
        let parsed = schema::collect_messages(text)
            .unwrap_or_else(|errors| panic!("{SOURCE_LOCALE}/{file}: {errors:?}"));
        messages.extend(parsed.into_iter().filter(|m| m.has_value));
    }
    messages.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = String::new();
    out.push_str("/// Every message in the source locale. Generated from `locales/en-US/*.ftl`.\n");
    out.push_str(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub enum Key {\n",
    );
    for m in &messages {
        writeln!(out, "    /// `{}`", m.id).unwrap();
        writeln!(out, "    {},", schema::variant_name(&m.id)).unwrap();
    }
    out.push_str("}\n\nimpl Key {\n    /// All keys, sorted by message id.\n    pub const ALL: &'static [Key] = &[\n");
    for m in &messages {
        writeln!(out, "        Key::{},", schema::variant_name(&m.id)).unwrap();
    }
    out.push_str("    ];\n\n    /// The FTL message id.\n    pub const fn id(self) -> &'static str {\n        match self {\n");
    for m in &messages {
        writeln!(
            out,
            "            Key::{} => {:?},",
            schema::variant_name(&m.id),
            m.id
        )
        .unwrap();
    }
    out.push_str("        }\n    }\n\n    /// Variables the source message expects.\n    pub const fn variables(self) -> &'static [&'static str] {\n        match self {\n");
    for m in &messages {
        let vars: Vec<String> = m.variables.iter().map(|v| format!("{v:?}")).collect();
        writeln!(
            out,
            "            Key::{} => &[{}],",
            schema::variant_name(&m.id),
            vars.join(", ")
        )
        .unwrap();
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str("/// Embedded FTL sources: (locale tag, [(file name, contents)]).\npub(crate) static EMBEDDED: &[(&str, &[(&str, &str)])] = &[\n");
    for (locale, files) in &locales {
        writeln!(out, "    ({locale:?}, &[").unwrap();
        for (file, text) in files {
            // Embedding the text itself (a Debug-escaped literal) keeps the generated code free of
            // machine-specific absolute paths.
            writeln!(out, "        ({file:?}, {text:?}),").unwrap();
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("generated.rs");
    fs::write(dest, out).expect("write generated.rs");
}

/// Fails the build if any locale has a syntax error, a duplicate id, a dangling reference or a
/// reference cycle, so the runtime never has to handle broken embedded FTL.
fn validate_all(locales: &Locales) {
    let mut problems = Vec::new();
    for (locale, files) in locales {
        let mut parsed = Vec::new();
        for (file, text) in files {
            let (resource, errors) = ftl::parse(text);
            problems.extend(errors.iter().map(|e| format!("{locale}/{file}:{e}")));
            parsed.push((file.as_str(), resource));
        }
        let views: Vec<(&str, &ftl::Resource)> = parsed.iter().map(|(f, r)| (*f, r)).collect();
        problems.extend(
            schema::validate(&views)
                .into_iter()
                .map(|e| format!("{locale}: {e}")),
        );
    }
    assert!(problems.is_empty(), "invalid FTL:\n{}", problems.join("\n"));
}

/// Locale tag to (file name, contents). Each file is read once, so validation, key generation and
/// embedding all see the same text.
type Locales = BTreeMap<String, BTreeMap<String, String>>;

fn read_locales(dir: &Path) -> Locales {
    let mut locales = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read locales dir") {
        let entry = entry.expect("dir entry");
        if !entry.file_type().expect("file type").is_dir() {
            continue;
        }
        let tag = entry.file_name().to_string_lossy().into_owned();
        let mut files = BTreeMap::new();
        for file in fs::read_dir(entry.path()).expect("read locale dir") {
            let path = file.expect("file entry").path();
            if path.extension().is_some_and(|ext| ext == "ftl") {
                let name = path
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned();
                let text = fs::read_to_string(&path).expect("read ftl");
                files.insert(name, text);
            }
        }
        locales.insert(tag, files);
    }
    locales
}
