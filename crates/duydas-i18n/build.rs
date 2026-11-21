//! Generates the typed `Key` enum from the en-US source locale and embeds every locale's FTL.

#![allow(clippy::unwrap_used, reason = "writing into a String is infallible")]

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
    let source = locales.get(SOURCE_LOCALE).unwrap_or_else(|| {
        panic!(
            "source locale {SOURCE_LOCALE} missing in {}",
            locales_dir.display()
        )
    });

    let mut messages = Vec::new();
    for (file, path) in source {
        let text = fs::read_to_string(path).expect("read ftl");
        let parsed = schema::collect_messages(&text)
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
        for (file, path) in files {
            // Embedding the text itself (a Debug-escaped literal) keeps the generated code free of
            // machine-specific absolute paths.
            let text = fs::read_to_string(path).expect("read ftl");
            writeln!(out, "        ({file:?}, {text:?}),").unwrap();
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("generated.rs");
    fs::write(dest, out).expect("write generated.rs");
}

fn read_locales(dir: &Path) -> BTreeMap<String, BTreeMap<String, PathBuf>> {
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
                files.insert(name, path);
            }
        }
        locales.insert(tag, files);
    }
    locales
}
