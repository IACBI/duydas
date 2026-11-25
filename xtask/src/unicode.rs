//! `cargo xtask unicode-gen [--check]`: generates duydas-text's Unicode property tables and
//! conformance test data from the Unicode Character Database.
//!
//! The UCD files are downloaded once into `target/ucd/<version>` with curl. `--check` fails
//! if the committed files differ from what the data produces, instead of writing them.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "18.0.0";
const BASE_URL: &str = "https://www.unicode.org/Public";
/// The emoji data HarfBuzz 10.4.0 generated its Extended_Pictographic table from. Its
/// grapheme rule (an emoji after ZWJ continues the cluster) uses that set, which Unicode 18
/// narrowed, so shaping keeps it as a flag of its own; segmentation uses `VERSION`.
const HARFBUZZ_EMOJI_VERSION: &str = "16.0.0";

const LINE_BREAK: &str = "LineBreak.txt";
const EAST_ASIAN_WIDTH: &str = "EastAsianWidth.txt";
const GENERAL_CATEGORY: &str = "extracted/DerivedGeneralCategory.txt";
const EMOJI: &str = "emoji/emoji-data.txt";
const GRAPHEME: &str = "auxiliary/GraphemeBreakProperty.txt";
const WORD: &str = "auxiliary/WordBreakProperty.txt";
const CORE: &str = "DerivedCoreProperties.txt";
const SCRIPTS: &str = "Scripts.txt";
const ALIASES: &str = "PropertyValueAliases.txt";
const UNICODE_DATA: &str = "UnicodeData.txt";
const NORMALIZATION: &str = "DerivedNormalizationProps.txt";
const BIDI_CLASS: &str = "extracted/DerivedBidiClass.txt";
const BIDI_BRACKETS: &str = "BidiBrackets.txt";
const BIDI_MIRRORING: &str = "BidiMirroring.txt";
const BIDI_TEST: &str = "BidiTest.txt";
const BIDI_CHARACTER_TEST: &str = "BidiCharacterTest.txt";

/// The bidirectional conformance tests are too large to commit (about 14 MB together), so
/// the repository keeps a sample of each (see [`bidi_sample`]) and CI runs the whole files
/// from `target/ucd`: one test line in `every`, and every BidiTest case longer than the
/// exhaustive ones (more than four classes).
const BIDI_SAMPLES: &[(&str, &str, usize)] = &[
    (BIDI_TEST, "bidi_test_sample.txt", 64),
    (BIDI_CHARACTER_TEST, "bidi_character_test_sample.txt", 32),
];

/// Bidi_Class values in the order of the generated `BidiClass` enum.
const BIDI_CLASSES: [&str; 23] = [
    "L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S", "WS", "ON", "LRE", "LRO",
    "RLE", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI",
];
/// Conformance tests and where their test lines go.
const TESTS: &[(&str, &str)] = &[
    ("auxiliary/GraphemeBreakTest.txt", "grapheme_break_test.txt"),
    ("auxiliary/WordBreakTest.txt", "word_break_test.txt"),
    ("auxiliary/LineBreakTest.txt", "line_break_test.txt"),
];

const CRATE: &str = "crates/duydas-text";
const CODE_POINTS: usize = 0x11_0000;
const BLOCK: usize = 64;

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");
    let ucd = root.join("target").join("ucd").join(VERSION);
    let inputs = [
        LINE_BREAK,
        EAST_ASIAN_WIDTH,
        GENERAL_CATEGORY,
        EMOJI,
        GRAPHEME,
        WORD,
        CORE,
        SCRIPTS,
        ALIASES,
        UNICODE_DATA,
        NORMALIZATION,
        BIDI_CLASS,
        BIDI_BRACKETS,
        BIDI_MIRRORING,
    ];
    let tests = TESTS
        .iter()
        .map(|(name, _)| name)
        .chain(BIDI_SAMPLES.iter().map(|(name, _, _)| name));
    for name in inputs.iter().chain(tests) {
        fetch(&ucd, VERSION, name)?;
    }
    let harfbuzz_ucd = root.join("target").join("ucd").join(HARFBUZZ_EMOJI_VERSION);
    fetch(&harfbuzz_ucd, HARFBUZZ_EMOJI_VERSION, EMOJI)?;
    let harfbuzz_emoji = std::fs::read_to_string(harfbuzz_ucd.join(EMOJI))
        .map_err(|e| format!("read {HARFBUZZ_EMOJI_VERSION} {EMOJI}: {e}"))?;
    let read = |name: &str| {
        std::fs::read_to_string(ucd.join(name)).map_err(|e| format!("read {name}: {e}"))
    };
    let mut outputs = vec![
        (
            PathBuf::from(CRATE).join("src").join("tables.rs"),
            tables(&read)?,
        ),
        (
            PathBuf::from(CRATE).join("src").join("shaping_tables.rs"),
            shaping_tables(&read, &harfbuzz_emoji)?,
        ),
        (
            PathBuf::from(CRATE).join("src").join("bidi_tables.rs"),
            bidi_tables(&read)?,
        ),
    ];
    for (name, out, every) in BIDI_SAMPLES {
        outputs.push((
            PathBuf::from(CRATE).join("tests").join("data").join(out),
            bidi_sample(&read(name)?, *every),
        ));
    }
    for (name, out) in TESTS {
        outputs.push((
            PathBuf::from(CRATE).join("tests").join("data").join(out),
            test_lines(&read(name)?),
        ));
    }
    let mut stale = Vec::new();
    for (path, content) in outputs {
        let full = root.join(&path);
        if check {
            if std::fs::read_to_string(&full).ok().as_deref() != Some(content.as_str()) {
                stale.push(path.display().to_string());
            }
        } else {
            if let Some(dir) = full.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            // Written beside the target and renamed, so a reader never sees half a file.
            let partial = full.with_extension(format!("part{}", std::process::id()));
            if let Err(e) =
                std::fs::write(&partial, content).and_then(|()| std::fs::rename(&partial, &full))
            {
                let _ = std::fs::remove_file(&partial);
                return Err(format!("{}: {e}", full.display()));
            }
            println!("wrote {}", path.display());
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "out of date, run cargo xtask unicode-gen: {}",
            stale.join(", ")
        ))
    }
}

/// Downloads one UCD file unless it is already there. The versioned URL never changes.
fn fetch(dir: &Path, version: &str, name: &str) -> Result<(), String> {
    let path = dir.join(name);
    if path.exists() {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    // A name of its own per process, published only once complete, so concurrent runs never
    // see or write each other's partial files.
    let partial = path.with_extension(format!("part{}", std::process::id()));
    let url = format!("{BASE_URL}/{version}/ucd/{name}");
    let status = Command::new("curl")
        .args(["--proto", "=https", "--tlsv1.2", "-sSfL", "-o"])
        .arg(&partial)
        .arg(&url)
        .status();
    let result = match status {
        Err(e) => Err(format!("run curl: {e}")),
        Ok(status) if !status.success() => Err(format!("curl {url}: {status}")),
        Ok(_) => match std::fs::rename(&partial, &path) {
            Ok(()) => return Ok(()),
            // Another run published the same file first.
            Err(_) if path.exists() => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        },
    };
    let _ = std::fs::remove_file(&partial);
    result
}

/// One data line: a code point range and its fields after the range.
struct Entry<'a> {
    first: usize,
    last: usize,
    fields: Vec<&'a str>,
}

/// The `@missing` defaults, then the explicit entries, both in file order.
fn entries(text: &str) -> Result<(Vec<Entry<'_>>, Vec<Entry<'_>>), String> {
    let mut defaults = Vec::new();
    let mut explicit = Vec::new();
    for line in text.lines() {
        let (data, target) = match line.strip_prefix("# @missing:") {
            Some(rest) => (rest, &mut defaults),
            None => (line.split('#').next().unwrap_or(""), &mut explicit),
        };
        if data.trim().is_empty() {
            continue;
        }
        let mut fields = data.split(';').map(str::trim);
        let range = fields.next().unwrap_or("");
        let (first, last) = range.split_once("..").unwrap_or((range, range));
        let parse = |s: &str| {
            usize::from_str_radix(s, 16)
                .ok()
                .filter(|&cp| cp < CODE_POINTS)
                .ok_or_else(|| format!("bad code point {s:?} in {line:?}"))
        };
        target.push(Entry {
            first: parse(first)?,
            last: parse(last)?,
            fields: fields.collect(),
        });
    }
    Ok((defaults, explicit))
}

/// Interns property value names, so each code point stores a small index.
#[derive(Default)]
struct Names {
    names: Vec<String>,
    index: HashMap<String, u8>,
}

impl Names {
    fn id(&mut self, name: &str) -> Result<u8, String> {
        if let Some(&id) = self.index.get(name) {
            return Ok(id);
        }
        let id = u8::try_from(self.names.len()).map_err(|_| "too many values".to_owned())?;
        self.names.push(name.to_owned());
        self.index.insert(name.to_owned(), id);
        Ok(id)
    }
}

fn field<'a>(fields: &[&'a str], i: usize) -> Result<&'a str, String> {
    fields
        .get(i)
        .copied()
        .ok_or_else(|| "missing field".to_owned())
}

/// Fills `values` from a file: `select` maps an entry's fields to a value, or None to skip it.
fn assign<T: Copy>(
    values: &mut [T],
    text: &str,
    mut select: impl FnMut(&[&str]) -> Result<Option<T>, String>,
) -> Result<(), String> {
    let (defaults, explicit) = entries(text)?;
    for entry in defaults.iter().chain(&explicit) {
        if let Some(value) = select(&entry.fields)? {
            values[entry.first..=entry.last].fill(value);
        }
    }
    Ok(())
}

/// A UCD value name as a Rust variant: `Regional_Indicator` becomes `RegionalIndicator`,
/// `ZWJ` becomes `Zwj` and `ALetter` stays.
fn variant(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let upper = part
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            if upper && part.len() > 1 {
                let (head, tail) = part.split_at(1);
                format!("{head}{}", tail.to_ascii_lowercase())
            } else {
                part.to_owned()
            }
        })
        .collect()
}

const EAST_ASIAN: u8 = 1;
const INITIAL_PUNCTUATION: u8 = 2;
const FINAL_PUNCTUATION: u8 = 4;
const EXTENDED_PICTOGRAPHIC: u8 = 8;
const UNASSIGNED: u8 = 16;
const WORD_CHARACTER: u8 = 32;

fn tables(read: &dyn Fn(&str) -> Result<String, String>) -> Result<String, String> {
    let mut gc = vec![*b"Cn"; CODE_POINTS];
    assign(&mut gc, &read(GENERAL_CATEGORY)?, |f| {
        let value = field(f, 0)?.as_bytes();
        Ok(Some([value[0], value.get(1).copied().unwrap_or(b' ')]))
    })?;

    let mut flags = vec![0u8; CODE_POINTS];
    for (cp, cat) in gc.iter().enumerate() {
        flags[cp] |= match cat {
            b"Pi" => INITIAL_PUNCTUATION,
            b"Pf" => FINAL_PUNCTUATION,
            b"Cn" => UNASSIGNED,
            b"Nd" | b"Nl" | b"No" => WORD_CHARACTER,
            _ => 0,
        };
    }
    let mut wide = vec![false; CODE_POINTS];
    assign(&mut wide, &read(EAST_ASIAN_WIDTH)?, |f| {
        Ok(Some(matches!(field(f, 0)?, "F" | "W" | "H")))
    })?;
    let mut pictographic = vec![false; CODE_POINTS];
    assign(&mut pictographic, &read(EMOJI)?, |f| {
        Ok((field(f, 0)? == "Extended_Pictographic").then_some(true))
    })?;
    for cp in 0..CODE_POINTS {
        if wide[cp] {
            flags[cp] |= EAST_ASIAN;
        }
        if pictographic[cp] {
            flags[cp] |= EXTENDED_PICTOGRAPHIC;
        }
    }

    // LB1's default resolution, except CJ, which the line breaker tailors at run time.
    let mut lb_names = Names::default();
    let mut lb = vec![0u8; CODE_POINTS];
    let lb_text = read(LINE_BREAK)?;
    let mut lb_raw = vec![""; CODE_POINTS];
    {
        let (defaults, explicit) = entries(&lb_text)?;
        for entry in defaults.iter().chain(&explicit) {
            let value = field(&entry.fields, 0)?;
            lb_raw[entry.first..=entry.last].fill(value);
        }
    }
    for cp in 0..CODE_POINTS {
        let resolved = match lb_raw[cp] {
            "AI" | "SG" | "XX" => "AL",
            "SA" if matches!(&gc[cp], b"Mn" | b"Mc") => "CM",
            "SA" => "AL",
            other => other,
        };
        lb[cp] = lb_names.id(&variant(resolved))?;
    }

    let mut gcb_names = Names::default();
    let other = gcb_names.id("Other")?;
    let mut gcb = vec![other; CODE_POINTS];
    assign(&mut gcb, &read(GRAPHEME)?, |f| {
        Ok(Some(gcb_names.id(&variant(field(f, 0)?))?))
    })?;

    let mut wb_names = Names::default();
    let other = wb_names.id("Other")?;
    let mut wb = vec![other; CODE_POINTS];
    assign(&mut wb, &read(WORD)?, |f| {
        Ok(Some(wb_names.id(&variant(field(f, 0)?))?))
    })?;

    let core = read(CORE)?;
    let mut alphabetic = vec![false; CODE_POINTS];
    assign(&mut alphabetic, &core, |f| {
        Ok((f.len() == 1 && field(f, 0)? == "Alphabetic").then_some(true))
    })?;
    for (cp, &is) in alphabetic.iter().enumerate() {
        if is {
            flags[cp] |= WORD_CHARACTER;
        }
    }

    let mut incb_names = Names::default();
    let none = incb_names.id("None")?;
    let mut incb = vec![none; CODE_POINTS];
    assign(&mut incb, &core, |f| {
        if field(f, 0)? != "InCB" {
            return Ok(None);
        }
        Ok(Some(incb_names.id(&variant(field(f, 1)?))?))
    })?;

    // Distinct property combinations, in code point order, and each code point's index.
    let mut palette: Vec<u64> = Vec::new();
    let mut palette_index: HashMap<u64, u16> = HashMap::new();
    let mut values = vec![0u16; CODE_POINTS];
    for cp in 0..CODE_POINTS {
        let key = u64::from(lb[cp])
            | u64::from(gcb[cp]) << 8
            | u64::from(wb[cp]) << 16
            | u64::from(incb[cp]) << 24
            | u64::from(flags[cp]) << 32;
        let next = u16::try_from(palette.len()).map_err(|_| "palette too large".to_owned())?;
        let id = *palette_index.entry(key).or_insert_with(|| {
            palette.push(key);
            next
        });
        values[cp] = id;
    }

    // Three levels: 4096-code-point planes of 64 blocks of 64 code points, deduplicated.
    let (leaves, leaf_index) = dedup(values.chunks(BLOCK));
    let (mids, mid_index) = dedup(leaf_index.chunks(BLOCK));
    let top = mid_index;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated by `cargo xtask unicode-gen` from the Unicode Character Database {VERSION}.\n\
         // Do not edit. Unicode data © Unicode, Inc., used under the Unicode License v3\n\
         // (see LICENSE-UNICODE).\n\n\
         use crate::props::{{Gcb, InCb, Lb, Props, Wb}};\n\n\
         /// The Unicode version these tables come from.\n\
         pub(crate) const UNICODE_VERSION: (u8, u8, u8) = ({});\n\n\
         /// The properties of one code point.\n\
         pub(crate) fn props(c: char) -> Props {{\n    \
         let cp = c as usize;\n    \
         let mid = usize::from(TOP[cp >> 12]) * {BLOCK} + ((cp >> 6) & {mask});\n    \
         let leaf = usize::from(MID[mid]) * {BLOCK} + (cp & {mask});\n    \
         PALETTE[usize::from(LEAF[leaf])]\n}}\n",
        VERSION.replace('.', ", "),
        mask = BLOCK - 1,
    );
    let _ = writeln!(out, "static PALETTE: [Props; {}] = [", palette.len());
    for key in &palette {
        let name =
            |names: &Names, shift: u32| names.names[usize::from((key >> shift) as u8)].clone();
        let _ = writeln!(
            out,
            "    Props::new(Lb::{}, Gcb::{}, Wb::{}, InCb::{}, {}),",
            name(&lb_names, 0),
            name(&gcb_names, 8),
            name(&wb_names, 16),
            name(&incb_names, 24),
            (key >> 32) as u8
        );
    }
    out.push_str("];\n\n");
    array(&mut out, "TOP", &top, mids.len() / BLOCK);
    array(&mut out, "MID", &mids, leaves.len() / BLOCK);
    array(&mut out, "LEAF", &leaves, palette.len());
    Ok(out)
}

const NONSPACING_MARK: u32 = 1 << 16;
const DEFAULT_IGNORABLE: u32 = 1 << 17;
const MARK: u32 = 1 << 18;
const DECIMAL_NUMBER: u32 = 1 << 19;
const LETTER: u32 = 1 << 20;
const HARFBUZZ_PICTOGRAPHIC: u32 = 1 << 21;

/// The combining class HarfBuzz sorts and positions marks by: Unicode's, with the Hebrew,
/// Arabic and Syriac points renumbered into the order fonts expect, and a few Telugu, Thai,
/// Lao and Tibetan classes moved (HarfBuzz 10.4.0's `_hb_modified_combining_class`).
fn harfbuzz_combining_class(ccc: u32) -> u32 {
    match ccc {
        // Hebrew.
        10 => 22,
        11 => 15,
        12 => 16,
        13 => 17,
        14 => 23,
        15 => 18,
        16 => 19,
        17 => 20,
        18 => 21,
        19 => 14,
        20 => 24,
        21 => 12,
        22 => 25,
        23 => 13,
        24 => 10,
        25 => 11,
        // Arabic.
        27 => 28,
        28 => 29,
        29 => 30,
        30 => 31,
        31 => 32,
        32 => 33,
        33 => 27,
        // Telugu, Thai, Tibetan.
        84 => 4,
        91 => 5,
        103 => 3,
        130 => 132,
        132 => 131,
        other => other,
    }
}

/// The tables shaping needs: each code point's script, canonical combining class and flags in
/// a three-level trie like `tables`, the canonical decompositions, and the primary composites.
/// `harfbuzz_emoji` is the emoji data of `HARFBUZZ_EMOJI_VERSION`.
fn shaping_tables(
    read: &dyn Fn(&str) -> Result<String, String>,
    harfbuzz_emoji: &str,
) -> Result<String, String> {
    // Script names to ISO 15924 codes.
    let mut codes: HashMap<String, String> = HashMap::new();
    for line in read(ALIASES)?.lines() {
        let fields: Vec<&str> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .collect();
        if fields.first() == Some(&"sc") && fields.len() >= 3 {
            codes.insert(fields[2].to_owned(), fields[1].to_owned());
        }
    }
    let mut script_names = Names::default();
    let unknown = script_names.id("Zzzz")?;
    let mut script = vec![unknown; CODE_POINTS];
    assign(&mut script, &read(SCRIPTS)?, |f| {
        let name = field(f, 0)?;
        let code = codes
            .get(name)
            .ok_or_else(|| format!("no ISO code for script {name}"))?;
        Ok(Some(script_names.id(code)?))
    })?;
    let common = *script_names.index.get("Zyyy").ok_or("no Common script")?;
    let inherited = *script_names
        .index
        .get("Zinh")
        .ok_or("no Inherited script")?;

    let mut info = vec![0u32; CODE_POINTS];
    let mut decompositions: Vec<(u32, u32, u32)> = Vec::new();
    for line in read(UNICODE_DATA)?.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 6 {
            continue;
        }
        let cp = u32::from_str_radix(fields[0], 16).map_err(|e| format!("{line:?}: {e}"))?;
        let ccc: u32 = fields[3].parse().map_err(|e| format!("{line:?}: {e}"))?;
        info[cp as usize] |= harfbuzz_combining_class(ccc) << 8;
        if fields[2].starts_with('L') {
            info[cp as usize] |= LETTER;
        }
        if matches!(fields[2], "Mn" | "Mc" | "Me") {
            info[cp as usize] |= MARK;
        }
        if fields[2] == "Mn" {
            info[cp as usize] |= NONSPACING_MARK;
        }
        if fields[2] == "Nd" {
            info[cp as usize] |= DECIMAL_NUMBER;
        }
        let decomposition = fields[5];
        if decomposition.is_empty() || decomposition.starts_with('<') {
            continue;
        }
        let parts = decomposition
            .split(' ')
            .map(|p| u32::from_str_radix(p, 16).map_err(|e| format!("{line:?}: {e}")))
            .collect::<Result<Vec<u32>, String>>()?;
        match parts[..] {
            [a] => decompositions.push((cp, a, 0)),
            [a, b] => decompositions.push((cp, a, b)),
            _ => {
                return Err(format!(
                    "canonical decomposition of {cp:04X} is not one or two"
                ));
            }
        }
    }
    let mut ignorable = vec![false; CODE_POINTS];
    assign(&mut ignorable, &read(CORE)?, |f| {
        Ok((f.len() == 1 && field(f, 0)? == "Default_Ignorable_Code_Point").then_some(true))
    })?;
    // The characters HarfBuzz does not hide although the property lists them: the Hangul
    // fillers, which fonts draw as spacing glyphs, the shorthand format controls, and
    // MONGOLIAN FREE VARIATION SELECTOR FOUR (HarfBuzz's list predates it).
    for cp in [
        0x115F, 0x1160, 0x180F, 0x3164, 0xFFA0, 0x1BCA0, 0x1BCA1, 0x1BCA2, 0x1BCA3,
    ] {
        ignorable[cp] = false;
    }
    let mut excluded = vec![false; CODE_POINTS];
    assign(&mut excluded, &read(NORMALIZATION)?, |f| {
        Ok((f.len() == 1 && field(f, 0)? == "Full_Composition_Exclusion").then_some(true))
    })?;
    let mut pictographic = vec![false; CODE_POINTS];
    assign(&mut pictographic, harfbuzz_emoji, |f| {
        Ok((field(f, 0)? == "Extended_Pictographic").then_some(true))
    })?;
    for cp in 0..CODE_POINTS {
        info[cp] |= u32::from(script[cp]);
        if pictographic[cp] {
            info[cp] |= HARFBUZZ_PICTOGRAPHIC;
        }
        if ignorable[cp] {
            info[cp] |= DEFAULT_IGNORABLE;
        }
    }
    let mut compositions: Vec<(u32, u32, u32)> = decompositions
        .iter()
        .filter(|&&(cp, _, b)| b != 0 && !excluded[cp as usize])
        .map(|&(cp, a, b)| (a, b, cp))
        .collect();
    compositions.sort_unstable();
    decompositions.sort_unstable();

    let mut palette: Vec<u32> = Vec::new();
    let mut palette_index: HashMap<u32, u16> = HashMap::new();
    let mut values = vec![0u16; CODE_POINTS];
    for cp in 0..CODE_POINTS {
        let next = u16::try_from(palette.len()).map_err(|_| "palette too large".to_owned())?;
        values[cp] = *palette_index.entry(info[cp]).or_insert_with(|| {
            palette.push(info[cp]);
            next
        });
    }
    let (leaves, leaf_index) = dedup(values.chunks(BLOCK));
    let (mids, mid_index) = dedup(leaf_index.chunks(BLOCK));

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated by `cargo xtask unicode-gen` from the Unicode Character Database {VERSION}.\n\
         // Do not edit. Unicode data © Unicode, Inc., used under the Unicode License v3\n\
         // (see LICENSE-UNICODE).\n\n\
         /// A code point's script (an index into `SCRIPT_CODES`), combining class as HarfBuzz\n\
         /// modifies it (bits 8 to 15), and flags: {MARK:#x} for General_Category M*,\n\
         /// {NONSPACING_MARK:#x} for Mn, {DECIMAL_NUMBER:#x} for Nd, {LETTER:#x} for L*,\n\
         /// {DEFAULT_IGNORABLE:#x} for Default_Ignorable_Code_Point less the characters HarfBuzz\n\
         /// draws, {HARFBUZZ_PICTOGRAPHIC:#x} for Extended_Pictographic in the emoji data of\n\
         /// Unicode {HARFBUZZ_EMOJI_VERSION}, which HarfBuzz 10.4.0 uses.\n\
         pub(crate) fn char_info(c: char) -> u32 {{\n    \
         let cp = c as usize;\n    \
         let mid = usize::from(TOP[cp >> 12]) * {BLOCK} + ((cp >> 6) & {mask});\n    \
         let leaf = usize::from(MID[mid]) * {BLOCK} + (cp & {mask});\n    \
         PALETTE[usize::from(LEAF[leaf])]\n}}\n\n\
         pub(crate) const SCRIPT_COMMON: u8 = {common};\n\
         pub(crate) const SCRIPT_INHERITED: u8 = {inherited};\n\
         pub(crate) const SCRIPT_UNKNOWN: u8 = {unknown};\n\
         pub(crate) const MARK: u32 = {MARK:#x};\n\
         pub(crate) const NONSPACING_MARK: u32 = {NONSPACING_MARK:#x};\n\
         pub(crate) const DEFAULT_IGNORABLE: u32 = {DEFAULT_IGNORABLE:#x};\n\
         pub(crate) const DECIMAL_NUMBER: u32 = {DECIMAL_NUMBER:#x};\n\
         pub(crate) const LETTER: u32 = {LETTER:#x};\n\
         pub(crate) const HARFBUZZ_PICTOGRAPHIC: u32 = {HARFBUZZ_PICTOGRAPHIC:#x};\n",
        mask = BLOCK - 1,
    );
    let _ = writeln!(out, "/// ISO 15924 codes, by script index.");
    let _ = writeln!(
        out,
        "pub(crate) static SCRIPT_CODES: [[u8; 4]; {}] = [",
        script_names.names.len()
    );
    for row in script_names.names.chunks(8) {
        let items: Vec<String> = row.iter().map(|c| format!("*b\"{c}\"")).collect();
        let _ = writeln!(out, "    {},", items.join(", "));
    }
    out.push_str("];\n\n");
    let _ = writeln!(out, "static PALETTE: [u32; {}] = [", palette.len());
    for row in palette.chunks(8) {
        let items: Vec<String> = row.iter().map(|v| format!("{v:#x}")).collect();
        let _ = writeln!(out, "    {},", items.join(", "));
    }
    out.push_str("];\n\n");
    array(&mut out, "TOP", &mid_index, mids.len() / BLOCK);
    array(&mut out, "MID", &mids, leaves.len() / BLOCK);
    array(&mut out, "LEAF", &leaves, palette.len());
    let triples = |out: &mut String, name: &str, doc: &str, rows: &[(u32, u32, u32)]| {
        let _ = writeln!(out, "/// {doc}");
        let _ = writeln!(
            out,
            "pub(crate) static {name}: [(u32, u32, u32); {}] = [",
            rows.len()
        );
        for row in rows.chunks(4) {
            let items: Vec<String> = row
                .iter()
                .map(|(a, b, c)| format!("({a:#x}, {b:#x}, {c:#x})"))
                .collect();
            let _ = writeln!(out, "    {},", items.join(", "));
        }
        out.push_str("];\n\n");
    };
    triples(
        &mut out,
        "DECOMPOSITIONS",
        "Canonical decompositions (code point, first, second or 0), by code point; Hangul\n/// syllables are algorithmic and not listed.",
        &decompositions,
    );
    triples(
        &mut out,
        "COMPOSITIONS",
        "Primary composites (first, second, composite), by first and second.",
        &compositions,
    );
    while out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}

/// The bidirectional tables: each code point's Bidi_Class in a three-level trie like
/// `tables`, the paired brackets and the mirroring glyphs.
fn bidi_tables(read: &dyn Fn(&str) -> Result<String, String>) -> Result<String, String> {
    // Short aliases by long name, and long names (for the docs) by short alias. The
    // `@missing` lines of DerivedBidiClass.txt use long names.
    let mut short: HashMap<String, String> = HashMap::new();
    let mut long_names: HashMap<String, String> = HashMap::new();
    for line in read(ALIASES)?.lines() {
        let fields: Vec<&str> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .collect();
        if fields.first() == Some(&"bc") && fields.len() >= 3 {
            short.insert(fields[2].to_owned(), fields[1].to_owned());
            long_names.insert(fields[1].to_owned(), fields[2].replace('_', " "));
        }
    }
    let class_id = |name: &str| {
        let name = short.get(name).map_or(name, String::as_str);
        BIDI_CLASSES
            .iter()
            .position(|&c| c == name)
            .and_then(|i| u16::try_from(i).ok())
            .ok_or_else(|| format!("unknown Bidi_Class {name}"))
    };
    let mut class = vec![0u16; CODE_POINTS];
    assign(&mut class, &read(BIDI_CLASS)?, |f| {
        Ok(Some(class_id(field(f, 0)?)?))
    })?;
    let pairs = |text: &str, value: fn(&[&str]) -> Result<u32, String>| {
        let mut rows: Vec<(u32, u32)> = Vec::new();
        for line in text.lines() {
            let data = line.split('#').next().unwrap_or("").trim();
            if data.is_empty() {
                continue;
            }
            let fields: Vec<&str> = data.split(';').map(str::trim).collect();
            let cp = u32::from_str_radix(field(&fields, 0)?, 16)
                .map_err(|e| format!("{line:?}: {e}"))?;
            rows.push((cp, value(&fields)?));
        }
        rows.sort_unstable();
        Ok::<_, String>(rows)
    };
    let mirrors = pairs(&read(BIDI_MIRRORING)?, |f| {
        u32::from_str_radix(field(f, 1)?, 16).map_err(|e| e.to_string())
    })?;
    // A bracket's pair, with the high bit set for an opening bracket.
    let brackets = pairs(&read(BIDI_BRACKETS)?, |f| {
        let pair = u32::from_str_radix(field(f, 1)?, 16).map_err(|e| e.to_string())?;
        match field(f, 2)? {
            "o" => Ok(pair | 0x8000_0000),
            "c" => Ok(pair),
            other => Err(format!("bracket type {other}")),
        }
    })?;

    let (leaves, leaf_index) = dedup(class.chunks(BLOCK));
    let (mids, mid_index) = dedup(leaf_index.chunks(BLOCK));
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated by `cargo xtask unicode-gen` from the Unicode Character Database {VERSION}.\n\
         // Do not edit. Unicode data © Unicode, Inc., used under the Unicode License v3\n\
         // (see LICENSE-UNICODE).\n\n\
         /// A character's bidirectional class (Bidi_Class), named by its short alias.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n\
         pub enum BidiClass {{"
    );
    for name in BIDI_CLASSES {
        let long = long_names
            .get(name)
            .ok_or_else(|| format!("no long name for Bidi_Class {name}"))?;
        let _ = writeln!(out, "    /// {long} ({name}).\n    {},", variant(name));
    }
    out.push_str("}\n\n");
    let _ = writeln!(
        out,
        "static CLASSES: [BidiClass; {}] = [",
        BIDI_CLASSES.len()
    );
    for row in BIDI_CLASSES.chunks(8) {
        let items: Vec<String> = row
            .iter()
            .map(|c| format!("BidiClass::{}", variant(c)))
            .collect();
        let _ = writeln!(out, "    {},", items.join(", "));
    }
    out.push_str("];\n\n");
    let _ = writeln!(
        out,
        "/// A code point's Bidi_Class, with the defaults the UCD gives unassigned code points.\n\
         pub(crate) fn bidi_class(c: char) -> BidiClass {{\n    \
         let cp = c as usize;\n    \
         let mid = usize::from(TOP[cp >> 12]) * {BLOCK} + ((cp >> 6) & {mask});\n    \
         let leaf = usize::from(MID[mid]) * {BLOCK} + (cp & {mask});\n    \
         CLASSES[usize::from(LEAF[leaf])]\n}}\n",
        mask = BLOCK - 1,
    );
    array(&mut out, "TOP", &mid_index, mids.len() / BLOCK);
    array(&mut out, "MID", &mids, leaves.len() / BLOCK);
    array(&mut out, "LEAF", &leaves, BIDI_CLASSES.len());
    let rows = |out: &mut String, name: &str, doc: &str, rows: &[(u32, u32)]| {
        let _ = writeln!(out, "/// {doc}");
        let _ = writeln!(
            out,
            "pub(crate) static {name}: [(u32, u32); {}] = [",
            rows.len()
        );
        for row in rows.chunks(6) {
            let items: Vec<String> = row
                .iter()
                .map(|(a, b)| format!("({a:#x}, {b:#x})"))
                .collect();
            let _ = writeln!(out, "    {},", items.join(", "));
        }
        out.push_str("];\n\n");
    };
    rows(
        &mut out,
        "MIRRORS",
        "Bidi_Mirroring_Glyph: (character, mirrored character), by character.",
        &mirrors,
    );
    rows(
        &mut out,
        "BRACKETS",
        "Bidi_Paired_Bracket: (bracket, its pair with bit 31 set when the bracket opens), by\n/// bracket.",
        &brackets,
    );
    while out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}

/// A sample of a bidirectional conformance test: the copyright header, one test line in
/// `every`, BidiTest's cases of more than four classes, and the `@Levels` and `@Reorder`
/// lines the kept cases need.
fn bidi_sample(text: &str, every: usize) -> String {
    let mut out: String = text
        .lines()
        .take_while(|line| line.starts_with('#'))
        .take(5)
        .map(|line| format!("{line}\n"))
        .collect();
    // Only BidiTest has class sequences: its lines hold one semicolon, comments aside.
    let classes = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .find(|data| !data.is_empty() && !data.starts_with('@'))
        .is_some_and(|data| data.matches(';').count() == 1);
    let kept = if classes {
        " and every case of more than four classes"
    } else {
        ""
    };
    out.push_str(&format!(
        "#\n# A sample: one test line in {every}{kept}; comments removed.\n\
         # Generated by `cargo xtask unicode-gen`; the Unicode workflow runs the whole file.\n"
    ));
    let mut headers: Vec<&str> = Vec::new();
    let mut seen = 0usize;
    for line in text.lines() {
        let data = line.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        if let Some(rest) = data.strip_prefix('@') {
            let kind = rest.split(':').next().unwrap_or("");
            headers.retain(|h| !h[1..].starts_with(kind));
            headers.push(data);
            continue;
        }
        seen += 1;
        let classes = data
            .split(';')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .count();
        let long = data.matches(';').count() == 1 && classes > 4;
        if seen.is_multiple_of(every) || long {
            for header in headers.drain(..) {
                out.push_str(header);
                out.push('\n');
            }
            out.push_str(data);
            out.push('\n');
        }
    }
    out
}

/// Deduplicates blocks: the distinct blocks concatenated, and each block's index.
fn dedup<'a>(blocks: impl Iterator<Item = &'a [u16]>) -> (Vec<u16>, Vec<u16>) {
    let mut seen: HashMap<&[u16], u16> = HashMap::new();
    let mut distinct = Vec::new();
    let mut index = Vec::new();
    for block in blocks {
        let next = u16::try_from(seen.len()).unwrap_or(u16::MAX);
        let id = *seen.entry(block).or_insert_with(|| {
            distinct.extend_from_slice(block);
            next
        });
        index.push(id);
    }
    (distinct, index)
}

/// Writes a static array, as u8 when every value fits (`count` distinct values).
fn array(out: &mut String, name: &str, values: &[u16], count: usize) {
    let ty = if count <= 256 { "u8" } else { "u16" };
    let _ = writeln!(out, "static {name}: [{ty}; {}] = [", values.len());
    for row in values.chunks(16) {
        let items: Vec<String> = row.iter().map(u16::to_string).collect();
        let _ = writeln!(out, "    {},", items.join(", "));
    }
    out.push_str("];\n\n");
}

/// A conformance test file reduced to its test lines; the comments (character names and rule
/// numbers) are most of its size. The copyright header stays.
fn test_lines(text: &str) -> String {
    let mut out: String = text
        .lines()
        .take_while(|line| line.starts_with('#'))
        .take(5)
        .map(|line| format!("{line}\n"))
        .collect();
    out.push_str("#\n# Test lines only; comments removed by `cargo xtask unicode-gen`.\n");
    for line in text.lines() {
        let data = line.split('#').next().unwrap_or("").trim();
        if !data.is_empty() {
            out.push_str(data);
            out.push('\n');
        }
    }
    out
}
