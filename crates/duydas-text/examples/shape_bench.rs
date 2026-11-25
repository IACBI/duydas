//! Shaping, font fallback and layout timings.
//!
//! ```text
//! cargo run --release -p duydas-text --example shape_bench -- [FONT[:INDEX] ...]
//! ```
//!
//! Without fonts it uses Segoe UI, Yu Gothic and, if Acrobat is installed, Minion Pro (CFF).
//! Each message is itemized over the fonts in order, then each run is shaped. Layout shapes
//! the messages as paragraphs over the fonts, then breaks them into lines at two widths.

#![allow(clippy::print_stdout, reason = "benchmark output")]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use duydas_text::font::Font;
use duydas_text::layout::{Align, ShapedText, TextOptions};
use duydas_text::shaping::{Language, Script, ShapeOptions, itemize, shape};

/// Chat messages: Turkish with its dotted and dotless i, Japanese with kana and kanji, English
/// with ligature and kerning pairs, and decomposed marks.
const MESSAGES: [(&str, &str); 4] = [
    (
        "tr",
        "Şişli'de ılık bir gün; İzmir'e yarın gidiyoruz, ğüşiöç ĞÜŞİÖÇ. Saat 14:30'da buluşalım mı?",
    ),
    (
        "ja",
        "明日の会議は午後三時からです。資料を確認しておいてください！よろしくお願いします。",
    ),
    (
        "en",
        "Office affinity: AVATAR, Tokyo, \"quoted\" — fine, 1/2 off; the quick brown fox jumps.",
    ),
    (
        "tr",
        "Cafe\u{301} au lait, n\u{303}, Ğ (G\u{306}), I\u{307}stanbul: decomposed marks everywhere.",
    ),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut specs: Vec<String> = std::env::args().skip(1).collect();
    if specs.is_empty() {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_owned()) + r"\Fonts\";
        specs = vec![format!("{dir}segoeui.ttf"), format!("{dir}YuGothM.ttc:0")];
        let minion = r"C:\Program Files\Adobe\Acrobat DC\Resource\Font\MinionPro-Regular.otf";
        if std::path::Path::new(minion).exists() {
            specs.push(minion.to_owned());
        }
    }
    let mut fonts = Vec::new();
    for spec in &specs {
        let (path, index) = match spec.rsplit_once(':') {
            Some((p, i)) if !i.is_empty() && i.chars().all(|c| c.is_ascii_digit()) => {
                (p, i.parse()?)
            }
            _ => (spec.as_str(), 0),
        };
        let data: Arc<[u8]> = std::fs::read(path)?.into();
        fonts.push(Font::from_data(data, index)?);
    }

    // Each font alone, over every message.
    for font in &fonts {
        let name = font.full_name().unwrap_or_default();
        println!("{name}");
        for (language, text) in MESSAGES {
            let script = text
                .chars()
                .map(Script::of)
                .find(|s| s.is_specific())
                .unwrap_or(Script::COMMON);
            let options = ShapeOptions {
                script,
                language: Language::from_bcp47(language),
                ..ShapeOptions::default()
            };
            let (time, glyphs) = time(|| shape(font, text, &options).len());
            let chars = text.chars().count();
            println!(
                "  {language} {chars:>3} chars -> {glyphs:>3} glyphs  {:>6.1} µs  {:>5.0} ns/char",
                time * 1e6,
                time * 1e9 / chars as f64
            );
        }
    }

    for font in &fonts {
        let options = ShapeOptions {
            script: Script::of('a'),
            ..ShapeOptions::default()
        };
        let (one, _) = time(|| shape(font, "a", &options).len());
        println!(
            "{}: one character {:.1} µs",
            font.full_name().unwrap_or_default(),
            one * 1e6
        );
    }

    // Itemizing over the fonts in order, then shaping each run.
    let refs: Vec<&Font> = fonts.iter().collect();
    let mixed: String = MESSAGES.iter().map(|m| m.1).collect::<Vec<_>>().join(" ");
    let (itemize_time, runs) = time(|| itemize(&mixed, &refs).len());
    let (total, glyphs) = time(|| {
        itemize(&mixed, &refs)
            .into_iter()
            .map(|run| {
                let options = ShapeOptions {
                    script: run.script,
                    ..ShapeOptions::default()
                };
                shape(refs[run.font], &mixed[run.range], &options).len()
            })
            .sum::<usize>()
    });
    let chars = mixed.chars().count();
    println!(
        "fallback over {} fonts: {chars} chars, {runs} runs, {glyphs} glyphs; itemize {:.1} µs, itemize and shape {:.1} µs ({:.0} ns/char)",
        refs.len(),
        itemize_time * 1e6,
        total * 1e6,
        total * 1e9 / chars as f64
    );

    // Paragraph layout: shaping for layout, then line breaking at a narrow and a wide column.
    let options = TextOptions {
        size: 15.0,
        ..TextOptions::default()
    };
    for (language, text) in MESSAGES.iter().copied().chain([("mixed", mixed.as_str())]) {
        let chars = text.chars().count();
        let (shape_time, shaped) = time(|| ShapedText::new(text, &refs, &options));
        let mut line = format!(
            "layout {language:>5} {chars:>3} chars: shape {:>6.1} µs ({:>4.0} ns/char)",
            shape_time * 1e6,
            shape_time * 1e9 / chars as f64
        );
        for width in [240.0, 640.0] {
            let (layout_time, lines) =
                time(|| shaped.layout(Some(width), Align::Start).lines().len());
            line += &format!(
                "; {width} px: {lines} lines in {:>5.1} µs ({:>3.0} ns/char)",
                layout_time * 1e6,
                layout_time * 1e9 / chars as f64
            );
        }
        println!("{line}");
    }
    Ok(())
}

/// The median time of one call over enough calls for a stable reading, and its result.
fn time<R>(mut f: impl FnMut() -> R) -> (f64, R) {
    let mut samples = Vec::new();
    let result = f();
    for _ in 0..31 {
        let start = Instant::now();
        let mut calls = 0u32;
        while start.elapsed().as_micros() < 2000 {
            black_box(f());
            calls += 1;
        }
        samples.push(start.elapsed().as_secs_f64() / f64::from(calls));
    }
    samples.sort_by(f64::total_cmp);
    (samples[samples.len() / 2], result)
}
