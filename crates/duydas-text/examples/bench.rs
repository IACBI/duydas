//! Segmentation throughput on mixed Turkish, Japanese and English chat text:
//! `cargo run --release -p duydas-text --example bench`.

#![allow(clippy::print_stdout, reason = "a benchmark reports on stdout")]

use std::hint::black_box;
use std::time::Instant;

use duydas_text::{
    Strictness, graphemes, line_breaks, next_grapheme_boundary, prev_grapheme_boundary,
    prev_word_boundary, words,
};

const SAMPLES: &[&str] = &[
    "Yarın akşam İstanbul'da buluşalım mı? Saat 19.30 gibi Kadıköy'de olurum 🙂",
    "了解です！明日の会議は「第3会議室」で、資料は共有フォルダに置いておきました。",
    "Did you see the release notes? The new build fixes the IME candidate window (finally).",
    "Şu bağlantıya bak: https://example.com/belgeler/2026/rapor.pdf — özellikle 4. bölüm önemli.",
    "ちょっと待ってね、コーヒー買ってくる☕ すぐ戻ります。",
    "👨\u{200D}👩\u{200D}👧 Aile fotoğrafları yüklendi, 🇹🇷🇯🇵 bayraklar da var.",
];

fn main() {
    let mut text = String::new();
    while text.len() < 4 << 20 {
        for sample in SAMPLES {
            text.push_str(sample);
            text.push('\n');
        }
    }
    let mb = text.len() as f64 / 1e6;
    let measure = |name: &str, f: &dyn Fn() -> usize| {
        let mut best = f64::MAX;
        let mut count = 0;
        for _ in 0..5 {
            let start = Instant::now();
            count = black_box(f());
            best = best.min(start.elapsed().as_secs_f64());
        }
        println!("{name:<24} {:>7.0} MB/s  ({count} segments)", mb / best);
    };
    measure("graphemes", &|| graphemes(&text).count());
    measure("words", &|| words(&text).count());
    measure("line_breaks strict", &|| {
        line_breaks(&text, Strictness::Strict).count()
    });
    measure("line_breaks loose", &|| {
        line_breaks(&text, Strictness::Loose).count()
    });
    measure("next_grapheme_boundary", &|| {
        let mut index = 0;
        let mut count = 0;
        while index < text.len() {
            index = next_grapheme_boundary(&text, index);
            count += 1;
        }
        count
    });
    measure("prev_word_boundary", &|| {
        let mut index = text.len();
        let mut count = 0;
        while index > 0 {
            index = prev_word_boundary(&text, index);
            count += 1;
        }
        count
    });
    // Stepping back through one long run: emoji are each their own cluster; flags pair up
    // from the start of the run, so every step looks back to it (quadratic by design).
    for (name, unit, n) in [
        ("emoji", "😀", 100_000),
        ("Hangul syllables", "각", 100_000),
        ("Devanagari letters with nukta", "\u{0915}\u{093C}", 100_000),
        ("emoji with two ZWJs", "😀\u{200D}\u{200D}", 100_000),
        ("flags", "🇹🇷", 2_000),
    ] {
        let run = unit.repeat(n);
        let start = Instant::now();
        let mut index = run.len();
        let mut steps = 0;
        while index > 0 {
            index = prev_grapheme_boundary(&run, index);
            steps += 1;
        }
        let per_step = start.elapsed().as_secs_f64() * 1e9 / f64::from(steps);
        println!("prev_grapheme_boundary   {per_step:>7.0} ns/step through {n} {name}");
    }
    for (name, unit, n) in [("periods", ".", 100_000), ("\"a.1.\"", "a.1.", 25_000)] {
        let run = unit.repeat(n);
        let start = Instant::now();
        let mut index = run.len();
        let mut steps = 0;
        while index > 0 {
            index = prev_word_boundary(&run, index);
            steps += 1;
        }
        let per_step = start.elapsed().as_secs_f64() * 1e9 / f64::from(steps);
        println!("prev_word_boundary       {per_step:>7.0} ns/step through {n} {name}");
    }
}
