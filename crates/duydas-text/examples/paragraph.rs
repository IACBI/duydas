//! Lays out paragraphs of Turkish, Japanese, Hebrew and English with fonts installed on the
//! machine and draws them into a PNG, with each line's box, a selection and carets, to inspect
//! line breaking, bidirectional reordering and caret placement by eye.
//!
//! ```text
//! cargo run --release -p duydas-text --example paragraph -- [OUT.png] [FONT[:INDEX] ...]
//! ```
//!
//! Without fonts it uses Segoe UI and Yu Gothic from the Windows font folder.

#![allow(clippy::print_stdout, reason = "example output")]

use std::sync::Arc;

use duydas_text::font::{Font, Rasterizer};
use duydas_text::layout::{Align, Position, ShapedText, TextOptions};

const PARAGRAPHS: [(&str, Align); 5] = [
    (
        "Duydaş'a hoş geldin! Bu mesaj satır sonunda düzgünce kırılmalı: İıĞğŞşÇçÖöÜü. \
         Çok uzun bir bağlantı da kırılır: https://example.com/çok/uzun/bir/yol/adı",
        Align::Start,
    ),
    (
        "日本語のテキスト「東京へ行った。」は、行頭に句読点や小さい「っ」が来ないように折り返されます。",
        Align::Start,
    ),
    (
        "שלום עולם! This line mixes עברית with English (and 123 numbers) in one paragraph.",
        Align::Start,
    ),
    (
        "\u{5E9}\u{5DC}\u{5D5}\u{5DD}, \u{5D6}\u{5D5}\u{5D4}\u{5D9} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \
         \u{5DE}\u{5D9}\u{5DE}\u{5D9}\u{5DF} \u{5DC}\u{5E9}\u{5DE}\u{5D0}\u{5DC} (with English) \
         \u{5D5}\u{5DE}\u{5E1}\u{5E4}\u{5E8} 2026.",
        Align::Start,
    ),
    (
        "Ortalanmış satırlar\tsekme\tdurakları ile\nve elle kırılmış satır.",
        Align::Center,
    ),
];
const SIZE: f32 = 18.0;
const WIDTH: f32 = 420.0;
const MARGIN: usize = 16;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .unwrap_or_else(|| "target/paragraph.png".to_owned());
    let mut specs: Vec<String> = args.collect();
    if specs.is_empty() {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_owned()) + r"\Fonts\";
        specs = ["segoeui.ttf", "YuGothM.ttc:0"]
            .iter()
            .map(|f| format!("{dir}{f}"))
            .collect();
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
        fonts.push(Font::from_data(data, index)?);
    }
    let list: Vec<&Font> = fonts.iter().collect();
    let options = TextOptions {
        size: SIZE,
        tab_size: 4,
        ..TextOptions::default()
    };

    let layouts: Vec<_> = PARAGRAPHS
        .iter()
        .map(|&(text, align)| {
            let shaped = ShapedText::new(text, &list, &options);
            (text, shaped.layout(Some(WIDTH), align))
        })
        .collect();
    let width = WIDTH as usize + 2 * MARGIN;
    let height = layouts
        .iter()
        .map(|(_, l)| l.height().ceil() as usize + MARGIN)
        .sum::<usize>()
        + MARGIN;
    let mut canvas = Canvas {
        width,
        height,
        pixels: vec![255; width * height],
    };
    let mut rasterizer = Rasterizer::new();
    let mut top = MARGIN as f32;
    for (text, layout) in &layouts {
        let left = MARGIN as f32;
        // The wrap width, line boxes, a selection of the second fifth of the text and carets
        // at its ends.
        canvas.frame(left, top, WIDTH, layout.height(), 200);
        for line in layout.lines() {
            canvas.frame(left + line.x, top + line.top, line.width, line.height, 225);
        }
        let start = text.len() / 5;
        let end = text.ceil_char_boundary(2 * text.len() / 5);
        let start = text.floor_char_boundary(start);
        for rect in layout.selection(start..end) {
            canvas.fill(left + rect.x, top + rect.y, rect.width, rect.height, 215);
        }
        for position in [Position::new(start), Position::new(end)] {
            let caret = layout.caret(position);
            canvas.fill(left + caret.x - 0.5, top + caret.top, 1.0, caret.height, 0);
        }
        for glyph in layout.glyphs() {
            let font = &fonts[glyph.font];
            let x = left + glyph.x;
            let y = top + glyph.y;
            // Quarter-pixel horizontal positioning, as a glyph cache would bin it.
            let x = (x * 4.0).round() / 4.0;
            let Some(image) = rasterizer.rasterize(font, glyph.glyph, SIZE, x.fract(), 0.0)? else {
                continue;
            };
            canvas.blend(&image, x.floor() as i64, y.round() as i64);
        }
        println!(
            "{} lines, {:.1} × {:.1} px: {:?}",
            layout.lines().len(),
            layout.width(),
            layout.height(),
            layout
                .lines()
                .iter()
                .map(|l| &text[l.range.clone()])
                .collect::<Vec<_>>()
        );
        top += layout.height().ceil() + MARGIN as f32;
    }
    std::fs::write(&out, png(width, height, &canvas.pixels))?;
    println!("wrote {out} ({width}×{height})");
    Ok(())
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, gray: u8) {
        let (x0, y0) = (x.round().max(0.0) as usize, y.round().max(0.0) as usize);
        let x1 = ((x + w).round().max(0.0) as usize)
            .min(self.width)
            .max(x0 + 1);
        let y1 = ((y + h).round().max(0.0) as usize).min(self.height);
        for row in y0..y1 {
            for col in x0..x1.min(self.width) {
                let p = &mut self.pixels[row * self.width + col];
                *p = (*p).min(gray);
            }
        }
    }

    fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, gray: u8) {
        self.fill(x, y, w, 1.0, gray);
        self.fill(x, y + h - 1.0, w, 1.0, gray);
        self.fill(x, y, 1.0, h, gray);
        self.fill(x + w - 1.0, y, 1.0, h, gray);
    }

    fn blend(&mut self, image: &duydas_text::font::GlyphImage, x: i64, baseline: i64) {
        let left = x + i64::from(image.left);
        let top = baseline + i64::from(image.top);
        for row in 0..i64::from(image.height) {
            for col in 0..i64::from(image.width) {
                let (px, py) = (left + col, top + row);
                if px < 0 || py < 0 || px as usize >= self.width || py as usize >= self.height {
                    continue;
                }
                let coverage = u32::from(image.data[(row * i64::from(image.width) + col) as usize]);
                let pixel = &mut self.pixels[py as usize * self.width + px as usize];
                *pixel = (u32::from(*pixel) * (255 - coverage) / 255) as u8;
            }
        }
    }
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
