//! Draws sample text with fonts installed on the machine into a PNG, to inspect glyph
//! rasterization by eye.
//!
//! ```text
//! cargo run --release -p duydas-text --example specimen -- [OUT.png] [FONT[:INDEX] ...]
//! ```
//!
//! Without fonts it uses a set of Windows fonts. Layout is naive (one glyph per character, no
//! kerning or shaping): it shows the rasterizer, not the text stack.

#![allow(clippy::print_stdout, reason = "example output")]

use std::sync::Arc;

use duydas_text::font::{Font, Rasterizer};

const LINES: [&str; 3] = [
    "Duydaş: Hızlı kahverengi tilki İıĞğŞşÇçÖöÜü 0123456789",
    "日本語のテキスト「東京へ行った。」かな・カナ・漢字",
    "The quick brown fox jumps over the lazy dog. (fi, —, “”)",
];
const SIZES: [f32; 4] = [11.0, 14.0, 20.0, 36.0];
const WIDTH: usize = 1400;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .unwrap_or_else(|| "target/specimen.png".to_owned());
    let mut specs: Vec<String> = args.collect();
    if specs.is_empty() {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_owned()) + r"\Fonts\";
        specs = [
            "segoeui.ttf",
            "YuGothM.ttc:0",
            "meiryo.ttc:0",
            "NotoSansJP-VF.ttf",
            "consola.ttf",
            "times.ttf",
        ]
        .iter()
        .map(|f| format!("{dir}{f}"))
        .collect();
        let minion = r"C:\Program Files\Adobe\Acrobat DC\Resource\Font\MinionPro-Regular.otf";
        if std::path::Path::new(minion).exists() {
            specs.push(minion.to_owned());
        }
    }
    let mut fonts = Vec::new();
    for spec in &specs {
        let (path, index) = match spec.rsplit_once(':') {
            Some((p, i)) if i.chars().all(|c| c.is_ascii_digit()) && !i.is_empty() => {
                (p, i.parse()?)
            }
            _ => (spec.as_str(), 0),
        };
        let data: Arc<[u8]> = std::fs::read(path)?.into();
        let font = Font::from_data(data, index)?;
        println!(
            "{path}:{index}: {:?} {:?} {:?}",
            font.family_name(),
            font.style(),
            font.metrics()
        );
        fonts.push(font);
    }

    let row_height = |size: f32| (size * 1.5).ceil() as usize;
    let height: usize = fonts.len()
        * SIZES
            .iter()
            .map(|&s| row_height(s) * LINES.len())
            .sum::<usize>()
        + 16;
    let mut canvas = vec![255u8; WIDTH * height];
    let mut rasterizer = Rasterizer::new();
    let mut y = 8usize;
    for font in &fonts {
        let units = f32::from(font.metrics().units_per_em);
        for &size in &SIZES {
            for line in LINES {
                let baseline = y as f32 + size * 1.15;
                let mut pen = 8.0f32;
                for c in line.chars() {
                    let Some(glyph) = font.glyph_index(c) else {
                        pen += size * 0.5;
                        continue;
                    };
                    // Quarter-pixel horizontal positioning, as a glyph cache would bin it.
                    let x = (pen * 4.0).round() / 4.0;
                    let image = rasterizer.rasterize(font, glyph, size, x.fract(), 0.0)?;
                    if let Some(image) = image {
                        let left = x.floor() as i64 + i64::from(image.left);
                        let top = baseline as i64 + i64::from(image.top);
                        for row in 0..image.height as i64 {
                            for col in 0..image.width as i64 {
                                let (px, py) = (left + col, top + row);
                                if px < 0 || py < 0 || px as usize >= WIDTH || py as usize >= height
                                {
                                    continue;
                                }
                                let coverage = u32::from(
                                    image.data[(row * i64::from(image.width) + col) as usize],
                                );
                                let pixel = &mut canvas[py as usize * WIDTH + px as usize];
                                *pixel = (u32::from(*pixel) * (255 - coverage) / 255) as u8;
                            }
                        }
                    }
                    pen += f32::from(font.advance(glyph).unwrap_or(0)) * size / units;
                }
                y += row_height(size);
            }
        }
    }
    std::fs::write(&out, png(WIDTH, height, &canvas))?;
    println!("wrote {out} ({WIDTH}×{height})");
    Ok(())
}

/// A minimal grayscale PNG with uncompressed deflate blocks.
fn png(width: usize, height: usize, gray: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width + 1) * height);
    for row in gray.chunks_exact(width) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&(b << 16 | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    for (kind, body) in [(b"IHDR", ihdr), (b"IDAT", zlib), (b"IEND", Vec::new())] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(&body);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
