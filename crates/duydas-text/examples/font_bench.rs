//! Font parsing, character lookup and rasterization timings.
//!
//! ```text
//! cargo run --release -p duydas-text --example font_bench -- [FONT[:INDEX] ...]
//! ```
//!
//! Without fonts it uses Segoe UI, Yu Gothic and, if Acrobat is installed, Minion Pro (CFF).

#![allow(clippy::print_stdout, reason = "benchmark output")]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use duydas_text::font::{Font, GlyphId, Rasterizer};

const TEXT: &str = "Duydaş'a hoş geldiniz! İstanbul'da ılık bir gün. 日本語のテキスト「東京へ行った。」\
                    The quick brown fox jumps over the lazy dog 0123456789.";
const SIZES: [f32; 4] = [12.0, 16.0, 24.0, 48.0];

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
    let mut rasterizer = Rasterizer::new();
    for spec in &specs {
        let (path, index) = match spec.rsplit_once(':') {
            Some((p, i)) if !i.is_empty() && i.chars().all(|c| c.is_ascii_digit()) => {
                (p, i.parse()?)
            }
            _ => (spec.as_str(), 0),
        };
        let data: Arc<[u8]> = std::fs::read(path)?.into();
        const PARSES: u32 = 200;
        let start = Instant::now();
        for _ in 0..PARSES {
            black_box(Font::from_data(data.clone(), index)?);
        }
        let parse = start.elapsed().as_secs_f64() * 1e6 / f64::from(PARSES);
        let font = Font::from_data(data, index)?;
        let name = font.full_name().unwrap_or_default();
        println!(
            "{name} ({} glyphs, {} KB): parse {parse:.1} µs",
            font.glyph_count(),
            std::fs::metadata(path)?.len() / 1024
        );

        let chars: Vec<char> = TEXT.chars().collect();
        let rounds = 2000;
        let start = Instant::now();
        for _ in 0..rounds {
            for &c in &chars {
                black_box(font.glyph_index(black_box(c)));
            }
        }
        let lookup = start.elapsed().as_secs_f64() * 1e9 / (rounds * chars.len()) as f64;
        println!("  glyph_index            {lookup:>7.1} ns/char");

        for size in SIZES {
            let start = Instant::now();
            let (mut glyphs, mut pixels, mut errors) = (0u64, 0u64, 0u64);
            for g in 0..font.glyph_count() {
                match rasterizer.rasterize(&font, GlyphId(g), size, 0.25, 0.0) {
                    Ok(Some(image)) => {
                        glyphs += 1;
                        pixels += u64::from(image.width * image.height);
                        black_box(image);
                    }
                    Ok(None) => {}
                    Err(_) => errors += 1,
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            println!(
                "  rasterize {size:>4} px      {:>7.2} µs/glyph  {:>6.0} Mpx/s  ({glyphs} glyphs with ink, {errors} errors)",
                elapsed * 1e6 / glyphs.max(1) as f64,
                pixels as f64 / elapsed / 1e6
            );
        }
    }
    Ok(())
}
