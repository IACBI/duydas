//! Font tests: hand-built fonts with known outlines, malformed data, and every font installed
//! on the machine running the tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::cmap::{Cmap, Variation};
use super::reader::Span;
use super::*;

// ---- Recording outlines ----

#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}
use Cmd::{C, L, M, Q, Z};

#[derive(Debug, Default)]
struct Rec(Vec<Cmd>);

impl OutlineSink for Rec {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(M(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(L(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(Q(x1, y1, x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(C(x1, y1, x2, y2, x, y));
    }
    fn close(&mut self) {
        self.0.push(Z);
    }
}

fn outline(font: &Font, glyph: u16) -> Result<Vec<Cmd>, FontError> {
    let mut rec = Rec::default();
    font.outline(GlyphId(glyph), &mut rec)?;
    Ok(rec.0)
}

// ---- Building fonts ----

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// An sfnt whose table offsets count from `base`, the face's position in the file.
fn sfnt_at(base: usize, version: &[u8; 4], tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = version.to_vec();
    out.extend(u16s(&[tables.len() as u16, 0, 0, 0]));
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend(u32s(&[0, (base + offset) as u32, data.len() as u32]));
        body.extend_from_slice(data);
        let padded = data.len().next_multiple_of(4);
        body.resize(body.len() + padded - data.len(), 0);
        offset += padded;
    }
    out.extend(body);
    out
}

fn sfnt(version: &[u8; 4], tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    sfnt_at(0, version, tables)
}

fn head(units_per_em: u16, long_loca: bool) -> Vec<u8> {
    let mut t = vec![0u8; 54];
    t[..4].copy_from_slice(&[0, 1, 0, 0]);
    t[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    t[18..20].copy_from_slice(&units_per_em.to_be_bytes());
    t[50..52].copy_from_slice(&u16::from(long_loca).to_be_bytes());
    t
}

fn hhea(ascender: i16, descender: i16, line_gap: i16, long_metrics: u16) -> Vec<u8> {
    let mut t = vec![0u8; 36];
    t[..4].copy_from_slice(&[0, 1, 0, 0]);
    t[4..6].copy_from_slice(&ascender.to_be_bytes());
    t[6..8].copy_from_slice(&descender.to_be_bytes());
    t[8..10].copy_from_slice(&line_gap.to_be_bytes());
    t[34..36].copy_from_slice(&long_metrics.to_be_bytes());
    t
}

fn maxp(glyphs: u16) -> Vec<u8> {
    let mut t = vec![0, 0, 0x50, 0];
    t.extend(glyphs.to_be_bytes());
    t
}

/// Long metrics with these advances and no side bearings, then a zero bearing for each of the
/// remaining glyphs.
fn hmtx(advances: &[u16], glyphs: usize) -> Vec<u8> {
    let mut out: Vec<u8> = advances.iter().flat_map(|&a| u16s(&[a, 0])).collect();
    out.resize(out.len() + 2 * glyphs.saturating_sub(advances.len()), 0);
    out
}

fn cmap_table(subtables: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = u16s(&[0, subtables.len() as u16]);
    let mut offset = 4 + 8 * subtables.len();
    for (platform, encoding, data) in subtables {
        out.extend(u16s(&[*platform, *encoding]));
        out.extend(u32s(&[offset as u32]));
        offset += data.len();
    }
    for (_, _, data) in subtables {
        out.extend(data);
    }
    out
}

enum Segment {
    Delta(u16, u16, u16),
    Array(u16, Vec<u16>),
}

fn format4(segments: &[Segment]) -> Vec<u8> {
    let mut ends = Vec::new();
    let mut starts = Vec::new();
    let mut deltas = Vec::new();
    let mut ranges = Vec::new();
    let mut arrays: Vec<u16> = Vec::new();
    let count = segments.len() + 1;
    for (i, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Delta(start, end, delta) => {
                starts.push(*start);
                ends.push(*end);
                deltas.push(*delta);
                ranges.push(0);
            }
            Segment::Array(start, glyphs) => {
                starts.push(*start);
                ends.push(start + glyphs.len() as u16 - 1);
                deltas.push(0);
                // From this segment's idRangeOffset entry to its first glyph in the array.
                ranges.push((2 * (count - i + arrays.len())) as u16);
                arrays.extend(glyphs);
            }
        }
    }
    starts.push(0xFFFF);
    ends.push(0xFFFF);
    deltas.push(1);
    ranges.push(0);
    let mut body = u16s(&[(2 * count) as u16, 0, 0, 0]);
    body.extend(u16s(&ends));
    body.extend(u16s(&[0]));
    body.extend(u16s(&starts));
    body.extend(u16s(&deltas));
    body.extend(u16s(&ranges));
    body.extend(u16s(&arrays));
    let mut out = u16s(&[4, (body.len() + 6) as u16, 0]);
    out.extend(body);
    out
}

fn format12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut out = u16s(&[12, 0]);
    out.extend(u32s(&[
        (16 + 12 * groups.len()) as u32,
        0,
        groups.len() as u32,
    ]));
    for &(start, end, glyph) in groups {
        out.extend(u32s(&[start, end, glyph]));
    }
    out
}

fn u24(v: u32) -> [u8; 3] {
    let [_, a, b, c] = v.to_be_bytes();
    [a, b, c]
}

/// Format 14 with one selector: default ranges and glyph mappings.
fn format14(selector: u32, defaults: &[(u32, u8)], mappings: &[(u32, u16)]) -> Vec<u8> {
    let default_at = 10 + 11;
    let default_len = 4 + 4 * defaults.len();
    let mapping_at = default_at + default_len;
    let total = mapping_at + 4 + 5 * mappings.len();
    let mut out = u16s(&[14]);
    out.extend(u32s(&[total as u32, 1]));
    out.extend(u24(selector));
    out.extend(u32s(&[default_at as u32, mapping_at as u32]));
    out.extend(u32s(&[defaults.len() as u32]));
    for &(start, extra) in defaults {
        out.extend(u24(start));
        out.push(extra);
    }
    out.extend(u32s(&[mappings.len() as u32]));
    for &(code, glyph) in mappings {
        out.extend(u24(code));
        out.extend(glyph.to_be_bytes());
    }
    out
}

/// A simple glyph, encoded with short, repeated and same-as-before coordinates where possible.
fn simple_glyph(contours: &[&[(i16, i16, bool)]]) -> Vec<u8> {
    let points: Vec<(i16, i16, bool)> = contours.iter().flat_map(|c| c.iter().copied()).collect();
    let x_min = points.iter().map(|p| p.0).min().unwrap_or(0);
    let x_max = points.iter().map(|p| p.0).max().unwrap_or(0);
    let y_min = points.iter().map(|p| p.1).min().unwrap_or(0);
    let y_max = points.iter().map(|p| p.1).max().unwrap_or(0);
    let mut out = (contours.len() as i16).to_be_bytes().to_vec();
    for v in [x_min, y_min, x_max, y_max] {
        out.extend(v.to_be_bytes());
    }
    let mut end = 0u16;
    for contour in contours {
        end += contour.len() as u16;
        out.extend((end - 1).to_be_bytes());
    }
    out.extend(u16s(&[0]));
    let (mut flags, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
    let (mut px, mut py) = (0i16, 0i16);
    for &(x, y, on) in &points {
        let mut flag = u8::from(on);
        let mut coordinate = |d: i16, short: u8, same: u8, out: &mut Vec<u8>| {
            if d == 0 {
                flag |= same;
            } else if d.unsigned_abs() < 256 {
                flag |= short;
                if d > 0 {
                    flag |= same;
                }
                out.push(d.unsigned_abs() as u8);
            } else {
                out.extend(d.to_be_bytes());
            }
        };
        coordinate(x - px, 0x02, 0x10, &mut xs);
        coordinate(y - py, 0x04, 0x20, &mut ys);
        (px, py) = (x, y);
        flags.push(flag);
    }
    // Runs of equal flags use the repeat flag.
    let mut i = 0;
    while i < flags.len() {
        let mut run = 1;
        while i + run < flags.len() && flags[i + run] == flags[i] && run < 256 {
            run += 1;
        }
        if run > 1 {
            out.push(flags[i] | 0x08);
            out.push((run - 1) as u8);
        } else {
            out.push(flags[i]);
        }
        i += run;
    }
    out.extend(xs);
    out.extend(ys);
    out
}

/// A composite glyph from (flags, glyph, arguments and transform bytes) parts. The MORE flag
/// is set on all but the last.
fn composite_glyph(parts: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = (-1i16).to_be_bytes().to_vec();
    out.extend([0u8; 8]);
    for (i, (flags, glyph, rest)) in parts.iter().enumerate() {
        let more = if i + 1 < parts.len() { 0x20 } else { 0 };
        out.extend(u16s(&[flags | more, *glyph]));
        out.extend(rest);
    }
    out
}

fn words(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn glyf_loca(glyphs: &[Vec<u8>], long: bool) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = Vec::new();
    let mut offsets = vec![0usize];
    for g in glyphs {
        glyf.extend(g);
        glyf.resize(glyf.len().next_multiple_of(2), 0);
        offsets.push(glyf.len());
    }
    let loca = if long {
        offsets
            .iter()
            .flat_map(|&o| (o as u32).to_be_bytes())
            .collect()
    } else {
        offsets
            .iter()
            .flat_map(|&o| ((o / 2) as u16).to_be_bytes())
            .collect()
    };
    (glyf, loca)
}

fn name_table(records: &[(u16, u16, u16, u16, &str)]) -> Vec<u8> {
    let mut storage = Vec::new();
    let mut out = u16s(&[0, records.len() as u16, (6 + 12 * records.len()) as u16]);
    for &(platform, encoding, language, id, text) in records {
        let bytes: Vec<u8> = if platform == 1 {
            text.bytes().collect()
        } else {
            text.encode_utf16().flat_map(u16::to_be_bytes).collect()
        };
        out.extend(u16s(&[
            platform,
            encoding,
            language,
            id,
            bytes.len() as u16,
            storage.len() as u16,
        ]));
        storage.extend(bytes);
    }
    out.extend(storage);
    out
}

fn os2(
    weight: u16,
    selection: u16,
    typo: (i16, i16, i16),
    x_height: i16,
    cap_height: i16,
) -> Vec<u8> {
    let mut t = vec![0u8; 96];
    t[..2].copy_from_slice(&4u16.to_be_bytes());
    t[4..6].copy_from_slice(&weight.to_be_bytes());
    t[6..8].copy_from_slice(&5u16.to_be_bytes());
    t[26..28].copy_from_slice(&40i16.to_be_bytes());
    t[28..30].copy_from_slice(&250i16.to_be_bytes());
    t[62..64].copy_from_slice(&selection.to_be_bytes());
    t[68..70].copy_from_slice(&typo.0.to_be_bytes());
    t[70..72].copy_from_slice(&typo.1.to_be_bytes());
    t[72..74].copy_from_slice(&typo.2.to_be_bytes());
    t[86..88].copy_from_slice(&x_height.to_be_bytes());
    t[88..90].copy_from_slice(&cap_height.to_be_bytes());
    t
}

fn post(position: i16, thickness: i16, fixed: bool) -> Vec<u8> {
    let mut t = vec![0u8; 32];
    t[..4].copy_from_slice(&[0, 3, 0, 0]);
    t[8..10].copy_from_slice(&position.to_be_bytes());
    t[10..12].copy_from_slice(&thickness.to_be_bytes());
    t[12..16].copy_from_slice(&u32::from(fixed).to_be_bytes());
    t
}

const SQUARE: &[(i16, i16, bool)] = &[
    (0, 0, true),
    (0, 500, true),
    (500, 500, true),
    (500, 0, true),
];
const F2DOT14_HALF: i16 = 8192;
const F2DOT14_ONE: i16 = 16384;

/// The TrueType fixture's glyphs:
/// 0 empty; 1 square; 2 off-curve points only; 3 the square at half size, moved; 4 glyph 3
/// and the square side by side; 5 a composite of itself; 6 the square rotated 90°; 7 the
/// square scaled 1.5× with a scaled offset; 8 and 9 one curve, starting on and off the curve;
/// 10 and 11 empty.
fn glyf_glyphs() -> Vec<Vec<u8>> {
    const WORDS_XY: u16 = 0x0001 | 0x0002;
    vec![
        Vec::new(),
        simple_glyph(&[SQUARE]),
        simple_glyph(&[&[
            (250, 0, false),
            (500, 250, false),
            (250, 500, false),
            (0, 250, false),
        ]]),
        composite_glyph(&[(
            WORDS_XY | 0x0008,
            1,
            [words(&[100, 200]), words(&[F2DOT14_HALF])].concat(),
        )]),
        composite_glyph(&[
            (WORDS_XY, 3, words(&[0, 0])),
            (WORDS_XY, 1, words(&[600, 0])),
        ]),
        composite_glyph(&[(WORDS_XY, 5, words(&[0, 0]))]),
        composite_glyph(&[(
            WORDS_XY | 0x0080,
            1,
            words(&[500, 0, 0, F2DOT14_ONE, -F2DOT14_ONE, 0]),
        )]),
        composite_glyph(&[(WORDS_XY | 0x0008 | 0x0800, 1, words(&[10, 20, 24576]))]),
        simple_glyph(&[&[(0, 0, true), (250, 500, false), (500, 0, true)]]),
        simple_glyph(&[&[(250, 500, false), (500, 0, true), (0, 0, true)]]),
        Vec::new(),
        Vec::new(),
    ]
}

fn glyf_cmap() -> Vec<u8> {
    cmap_table(&[
        (
            3,
            1,
            format4(&[Segment::Delta(0x41, 0x41, 1u16.wrapping_sub(0x41))]),
        ),
        (
            3,
            10,
            format12(&[
                (0x41, 0x42, 1),
                (0x845B, 0x845B, 9),
                (0x8FBA, 0x8FBA, 11),
                (0x1F600, 0x1F600, 8),
            ]),
        ),
        (0, 5, format14(0xE0100, &[(0x8FBA, 0)], &[(0x845B, 10)])),
    ])
}

fn glyf_font_with(long_loca: bool, extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let glyphs = glyf_glyphs();
    let (glyf, loca) = glyf_loca(&glyphs, long_loca);
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"OS/2", os2(700, 1 << 7 | 1, (750, -250, 100), 450, 650)),
        (b"cmap", glyf_cmap()),
        (b"glyf", glyf),
        (b"head", head(1000, long_loca)),
        (b"hhea", hhea(800, -200, 0, 3)),
        (b"hmtx", hmtx(&[0, 500, 600], glyphs.len())),
        (b"loca", loca),
        (b"maxp", maxp(glyphs.len() as u16)),
        (
            b"name",
            name_table(&[
                (1, 0, 0, 1, "Mac Family"),
                (3, 1, 0x411, 1, "テスト"),
                (3, 1, 0x409, 1, "Test Sans"),
                (3, 1, 0x409, 2, "Bold"),
                (3, 1, 0x409, 16, "Test"),
                (3, 1, 0x409, 4, "Test Sans Bold"),
                (3, 1, 0x409, 6, "TestSans-Bold"),
            ]),
        ),
        (b"post", post(-100, 50, false)),
    ];
    tables.extend(extra.iter().cloned());
    sfnt(&[0, 1, 0, 0], &tables)
}

fn glyf_font() -> Vec<u8> {
    glyf_font_with(false, &[])
}

/// A TrueType font with these glyphs and (advance, left side bearing) metrics, 1,000 units
/// per em; `extra` tables replace the defaults.
fn tt_font(glyphs: &[Vec<u8>], metrics: &[(u16, i16)], extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let (glyf, loca) = glyf_loca(glyphs, false);
    let mut hmtx: Vec<u8> = metrics
        .iter()
        .flat_map(|&(advance, lsb)| [advance.to_be_bytes(), lsb.to_be_bytes()].concat())
        .collect();
    hmtx.resize(
        hmtx.len() + 2 * glyphs.len().saturating_sub(metrics.len()),
        0,
    );
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"cmap", glyf_cmap()),
        (b"glyf", glyf),
        (b"head", head(1000, false)),
        (b"hhea", hhea(800, -200, 0, metrics.len() as u16)),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp(glyphs.len() as u16)),
    ];
    for (tag, data) in extra {
        match tables.iter_mut().find(|(t, _)| t == tag) {
            Some(slot) => slot.1 = data.clone(),
            None => tables.push((tag, data.clone())),
        }
    }
    tables.sort_by_key(|(tag, _)| **tag);
    sfnt(&[0, 1, 0, 0], &tables)
}

/// A glyph with a header and no contours, not even an instruction length.
fn header_only_glyph() -> Vec<u8> {
    vec![0; 10]
}

/// A charstring number.
fn n(v: i32) -> Vec<u8> {
    match v {
        -107..=107 => vec![(v + 139) as u8],
        108..=1131 => {
            let w = v - 108;
            vec![(w / 256 + 247) as u8, (w % 256) as u8]
        }
        -1131..=-108 => {
            let w = -v - 108;
            vec![(w / 256 + 251) as u8, (w % 256) as u8]
        }
        _ => {
            let mut out = vec![28];
            out.extend((v as i16).to_be_bytes());
            out
        }
    }
}

/// Charstring numbers followed by an operator.
fn op(args: &[i32], operator: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = args.iter().flat_map(|&v| n(v)).collect();
    out.extend_from_slice(operator);
    out
}

fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let total: usize = items.iter().map(Vec::len).sum::<usize>() + 1;
    let size = match total {
        0..0x100 => 1,
        0x100..0x1_0000 => 2,
        _ => 3,
    };
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(size as u8);
    let mut offset = 1usize;
    let push = |out: &mut Vec<u8>, offset: usize| {
        out.extend_from_slice(&(offset as u32).to_be_bytes()[4 - size..]);
    };
    push(&mut out, offset);
    for item in items {
        offset += item.len();
        push(&mut out, offset);
    }
    for item in items {
        out.extend(item);
    }
    out
}

fn int5(v: usize) -> Vec<u8> {
    let mut out = vec![29];
    out.extend((v as i32).to_be_bytes());
    out
}

fn cff_plain(charstrings: &[Vec<u8>], global: &[Vec<u8>], local: &[Vec<u8>]) -> Vec<u8> {
    cff_plain_with(charstrings, global, local, &[])
}

/// A plain CFF with `top` added to its top DICT.
fn cff_plain_with(
    charstrings: &[Vec<u8>],
    global: &[Vec<u8>],
    local: &[Vec<u8>],
    extra_top: &[u8],
) -> Vec<u8> {
    let name = index(&[b"T".to_vec()]);
    let strings = index(&[]);
    let gsubrs = index(global);
    let top_index_len = 5 + 17 + extra_top.len();
    let cs_offset = 4 + name.len() + top_index_len + strings.len() + gsubrs.len();
    let cs = index(charstrings);
    let private_offset = cs_offset + cs.len();
    let top = [
        int5(cs_offset),
        vec![17],
        int5(6),
        int5(private_offset),
        vec![18],
        extra_top.to_vec(),
    ]
    .concat();
    assert_eq!(index(std::slice::from_ref(&top)).len(), top_index_len);
    [
        vec![1, 0, 4, 4],
        name,
        index(&[top]),
        strings,
        gsubrs,
        cs,
        [int5(6), vec![19]].concat(),
        index(local),
    ]
    .concat()
}

/// A CID-keyed CFF whose glyphs 0 and 1 use font DICT 0 and glyph 2 font DICT 1.
fn cff_cid(charstrings: &[Vec<u8>], subrs: [&[Vec<u8>]; 2]) -> Vec<u8> {
    cff_cid_with(charstrings, subrs, &[])
}

/// A CID-keyed CFF with `fd_extra` added to both font DICTs.
fn cff_cid_with(charstrings: &[Vec<u8>], subrs: [&[Vec<u8>]; 2], fd_extra: &[u8]) -> Vec<u8> {
    let name = index(&[b"T".to_vec()]);
    let strings = index(&[]);
    let gsubrs = index(&[]);
    let top_index_len = 5 + 37;
    let cs_offset = 4 + name.len() + top_index_len + strings.len() + gsubrs.len();
    let cs = index(charstrings);
    let select_offset = cs_offset + cs.len();
    let select = [
        vec![3],
        u16s(&[2, 0]),
        vec![0],
        u16s(&[2]),
        vec![1],
        u16s(&[3]),
    ]
    .concat();
    let array_offset = select_offset + select.len();
    let subrs0 = index(subrs[0]);
    let array_len = 2 + 1 + 3 + 2 * (11 + fd_extra.len());
    let private0 = array_offset + array_len;
    let private1 = private0 + 6 + subrs0.len();
    let fonts = index(&[
        [int5(6), int5(private0), vec![18], fd_extra.to_vec()].concat(),
        [int5(6), int5(private1), vec![18], fd_extra.to_vec()].concat(),
    ]);
    assert_eq!(fonts.len(), array_len);
    let top = [
        int5(0),
        int5(0),
        int5(0),
        vec![12, 30],
        int5(cs_offset),
        vec![17],
        int5(array_offset),
        vec![12, 36],
        int5(select_offset),
        vec![12, 37],
    ]
    .concat();
    assert_eq!(index(std::slice::from_ref(&top)).len(), top_index_len);
    [
        vec![1, 0, 4, 4],
        name,
        index(&[top]),
        strings,
        gsubrs,
        cs,
        select,
        fonts,
        [int5(6), vec![19]].concat(),
        subrs0,
        [int5(6), vec![19]].concat(),
        index(subrs[1]),
    ]
    .concat()
}

fn otf(cff: Vec<u8>, glyphs: u16) -> Vec<u8> {
    otf_with_units(cff, glyphs, 1000)
}

fn otf_with_units(cff: Vec<u8>, glyphs: u16, units_per_em: u16) -> Vec<u8> {
    sfnt(
        b"OTTO",
        &[
            (b"CFF ", cff),
            (
                b"cmap",
                cmap_table(&[(
                    3,
                    1,
                    format4(&[Segment::Delta(0x41, 0x41, 1u16.wrapping_sub(0x41))]),
                )]),
            ),
            (b"head", head(units_per_em, false)),
            (b"hhea", hhea(800, -200, 0, 1)),
            (b"hmtx", hmtx(&[500], usize::from(glyphs))),
            (b"maxp", maxp(glyphs)),
        ],
    )
}

/// A CFF DICT real number from its nibbles.
fn real(nibbles: &[u8]) -> Vec<u8> {
    let mut out = vec![30];
    for pair in nibbles.chunks(2) {
        out.push(pair[0] << 4 | pair.get(1).copied().unwrap_or(0xF));
    }
    if nibbles.len().is_multiple_of(2) {
        out.push(0xFF);
    }
    out
}

const ENDCHAR: &[u8] = &[14];

/// The plain CFF fixture's glyphs:
/// 0 empty; 1 a square, with a width operand; 2 the same square through hints and both kinds
/// of subroutine; 3 every curve operator; 4 the four flex operators; 5 an accented endchar;
/// 6 an arithmetic operator; 7 a subroutine that calls itself; 8 a stack overflow.
fn cff_charstrings() -> Vec<Vec<u8>> {
    vec![
        ENDCHAR.to_vec(),
        [
            op(&[600, 0, 0], &[21]),
            op(&[500], &[6]),
            op(&[500], &[7]),
            op(&[-500], &[6]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
        [
            op(&[10, 20], &[18]),
            op(&[30, 40], &[23]),
            vec![19, 0xC0],
            op(&[0, 0], &[21]),
            op(&[-107], &[10]),
            op(&[-107], &[29]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
        [
            op(&[0, 0], &[21]),
            op(&[100, 0, 100, 100, 0, 100], &[8]),
            op(&[10, 20, 30, 40, 50], &[27]),
            op(&[5, 10, 20, 30, 40], &[26]),
            op(&[10, 20, 30, 40], &[31]),
            op(&[10, 20, 30, 40, 50], &[30]),
            op(&[1, 2, 3, 4, 5, 6, 7, 8], &[24]),
            op(&[1, 1, 1, 2, 3, 4, 5, 6], &[25]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
        [
            op(&[0, 0], &[21]),
            op(
                &[10, 0, 10, 10, 10, 0, 10, 0, 10, -10, 10, 0, 50],
                &[12, 35],
            ),
            op(&[10, 10, 5, 10, 10, 10, 10], &[12, 34]),
            op(&[10, 5, 10, 5, 10, 10, 10, -5, 10], &[12, 36]),
            op(&[10, 10, 10, 10, 10, 0, 10, -10, 10, -10, 20], &[12, 37]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
        op(&[0, 0, 0, 65, 66], ENDCHAR),
        [op(&[0, 0], &[21]), op(&[-5], &[12, 9]), ENDCHAR.to_vec()].concat(),
        [op(&[-106], &[10]), ENDCHAR.to_vec()].concat(),
        op(&[1; 49], ENDCHAR),
    ]
}

fn cff_font() -> Vec<u8> {
    let local = [op(&[500], &[6, 11]), op(&[-106], &[10, 11])];
    let global = [[op(&[500], &[7]), op(&[-500], &[6, 11])].concat()];
    otf(cff_plain(&cff_charstrings(), &global, &local), 9)
}

fn cid_font() -> Vec<u8> {
    let draw = [
        op(&[0, 0], &[21]),
        op(&[-107], &[10]),
        op(&[500], &[7]),
        ENDCHAR.to_vec(),
    ]
    .concat();
    let cff = cff_cid(
        &[ENDCHAR.to_vec(), draw.clone(), draw],
        [&[op(&[500], &[6, 11])], &[op(&[300], &[6, 11])]],
    );
    otf(cff, 3)
}

fn parse(data: Vec<u8>) -> Font {
    Font::from_data(data, 0).expect("the fixture parses")
}

// ---- TrueType ----

#[test]
fn simple_glyphs_follow_the_on_and_off_curve_rules() {
    for long in [false, true] {
        let font = parse(glyf_font_with(long, &[]));
        assert_eq!(outline(&font, 0), Ok(vec![]));
        assert_eq!(
            outline(&font, 1),
            Ok(vec![
                M(0.0, 0.0),
                L(0.0, 500.0),
                L(500.0, 500.0),
                L(500.0, 0.0),
                Z
            ])
        );
        // Only off-curve points: the contour starts halfway between the last and the first.
        assert_eq!(
            outline(&font, 2),
            Ok(vec![
                M(125.0, 125.0),
                Q(250.0, 0.0, 375.0, 125.0),
                Q(500.0, 250.0, 375.0, 375.0),
                Q(250.0, 500.0, 125.0, 375.0),
                Q(0.0, 250.0, 125.0, 125.0),
                Z,
            ])
        );
        let curve = vec![M(0.0, 0.0), Q(250.0, 500.0, 500.0, 0.0), Z];
        assert_eq!(outline(&font, 8), Ok(curve.clone()));
        // Starting off the curve with an on-curve last point starts at the last point.
        assert_eq!(outline(&font, 9), Ok(curve));
    }
}

#[test]
fn composite_glyphs_apply_their_transforms() {
    let font = parse(glyf_font());
    let half = vec![
        M(100.0, 200.0),
        L(100.0, 450.0),
        L(350.0, 450.0),
        L(350.0, 200.0),
        Z,
    ];
    assert_eq!(outline(&font, 3), Ok(half.clone()));
    let mut both = half;
    both.extend([
        M(600.0, 0.0),
        L(600.0, 500.0),
        L(1100.0, 500.0),
        L(1100.0, 0.0),
        Z,
    ]);
    assert_eq!(outline(&font, 4), Ok(both));
    assert_eq!(
        outline(&font, 6),
        Ok(vec![
            M(500.0, 0.0),
            L(0.0, 0.0),
            L(0.0, 500.0),
            L(500.0, 500.0),
            Z
        ])
    );
    // SCALED_COMPONENT_OFFSET: the offset (10, 20) is scaled too.
    assert_eq!(
        outline(&font, 7),
        Ok(vec![
            M(15.0, 30.0),
            L(15.0, 780.0),
            L(765.0, 780.0),
            L(765.0, 30.0),
            Z
        ])
    );
    assert_eq!(
        outline(&font, 5),
        Err(FontError::Malformed("glyf: composite nesting"))
    );
    assert_eq!(outline(&font, 12), Err(FontError::NoSuchGlyph));
}

#[test]
fn bounds_cover_points_and_control_points() {
    let font = parse(glyf_font());
    let bounds = |g| font.bounds(GlyphId(g)).expect("readable");
    assert_eq!(bounds(0), None);
    assert_eq!(
        bounds(8),
        Some(Bounds {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 500.0,
            y_max: 500.0
        })
    );
}

// ---- Character mapping ----

#[test]
fn components_placed_by_matching_points() {
    // Glyph 3 is the square and, scaled by half, the square again with its first point moved
    // onto the composite's third point (500, 500). Glyph 4 names a point that does not exist.
    let scale = |args: [u8; 2]| [args.to_vec(), words(&[F2DOT14_HALF])].concat();
    let glyphs = [
        Vec::new(),
        simple_glyph(&[SQUARE]),
        header_only_glyph(),
        composite_glyph(&[
            (0x0001 | 0x0002, 1, words(&[0, 0])),
            (0x0008, 1, scale([2, 0])),
        ]),
        composite_glyph(&[
            (0x0001 | 0x0002, 1, words(&[0, 0])),
            (0x0008, 1, scale([9, 0])),
        ]),
    ];
    let font = parse(tt_font(&glyphs, &[(500, 0)], &[]));
    assert_eq!(
        outline(&font, 3),
        Ok(vec![
            M(0.0, 0.0),
            L(0.0, 500.0),
            L(500.0, 500.0),
            L(500.0, 0.0),
            Z,
            M(500.0, 500.0),
            L(500.0, 750.0),
            L(750.0, 750.0),
            L(750.0, 500.0),
            Z,
        ])
    );
    assert_eq!(outline(&font, 4), Err(FontError::Malformed("glyf")));
    assert_eq!(outline(&font, 2), Ok(vec![]));
}

#[test]
fn nested_composites_number_their_own_points() {
    // Glyph 3 aligns by point numbers within itself; glyph 5 places it after a triangle, so
    // its point 2 must still mean its own square's corner, not the triangle's. Glyph 4 names a
    // phantom point.
    let scale = |args: [u8; 2]| [args.to_vec(), words(&[F2DOT14_HALF])].concat();
    let xy = 0x0001 | 0x0002;
    let glyphs = [
        Vec::new(),
        simple_glyph(&[SQUARE]),
        simple_glyph(&[&[(0, 0, true), (0, 300, true), (300, 0, true)]]),
        composite_glyph(&[(xy, 1, words(&[0, 0])), (0x0008, 1, scale([2, 0]))]),
        composite_glyph(&[(xy, 1, words(&[0, 0])), (0x0008, 1, scale([4, 0]))]),
        composite_glyph(&[(xy, 2, words(&[0, 0])), (xy, 3, words(&[1000, 0]))]),
    ];
    let font = parse(tt_font(&glyphs, &[(500, 0)], &[]));
    let moves: Vec<Cmd> = outline(&font, 5)
        .expect("readable")
        .into_iter()
        .filter(|c| matches!(c, M(..)))
        .collect();
    assert_eq!(moves, [M(0.0, 0.0), M(1000.0, 0.0), M(1500.0, 500.0)]);
    assert_eq!(
        outline(&font, 4),
        Err(FontError::Unsupported("glyf: alignment to a phantom point"))
    );
}

#[test]
fn use_my_metrics_gives_the_composite_its_components_origin() {
    // Glyph 1's box starts at 200 and its bearing is 20, so its origin is at 180. Glyph 3 uses
    // glyph 1's metrics, scaled by half and moved 30 units: as in FreeType the origin stays at
    // 180, untransformed, while the component's left edge lands at 130.
    let shifted: Vec<(i16, i16, bool)> =
        SQUARE.iter().map(|&(x, y, on)| (x + 200, y, on)).collect();
    let glyphs = [
        Vec::new(),
        simple_glyph(&[&shifted]),
        simple_glyph(&[SQUARE]),
        composite_glyph(&[
            (
                0x0001 | 0x0002 | 0x0200 | 0x0008,
                1,
                words(&[30, 0, F2DOT14_HALF]),
            ),
            (0x0001 | 0x0002, 2, words(&[300, 600])),
        ]),
    ];
    let font = parse(tt_font(
        &glyphs,
        &[(0, 0), (500, 20), (500, 0), (500, 0)],
        &[],
    ));
    let x_min = |g| {
        font.bounds(GlyphId(g))
            .expect("readable")
            .expect("ink")
            .x_min
    };
    assert_eq!(x_min(1), 20.0);
    assert_eq!(x_min(3), -50.0);

    // An empty component's box starts at 0, so with a bearing of 0 its origin is 0, whatever
    // the composite's own bearing says.
    let shifted: Vec<(i16, i16, bool)> =
        SQUARE.iter().map(|&(x, y, on)| (x + 100, y, on)).collect();
    let glyphs = [
        Vec::new(),
        Vec::new(),
        simple_glyph(&[&shifted]),
        composite_glyph(&[
            (0x0001 | 0x0002 | 0x0200, 1, words(&[0, 0])),
            (0x0001 | 0x0002, 2, words(&[0, 0])),
        ]),
    ];
    let font = parse(tt_font(
        &glyphs,
        &[(0, 0), (500, 0), (500, 100), (500, 80)],
        &[],
    ));
    let b = font.bounds(GlyphId(3)).expect("readable").expect("ink");
    assert_eq!(b.x_min, 100.0);
}

#[test]
fn glyphs_far_from_the_pen_are_too_large() {
    // At 16 units per em and 4,096 px per em, a square 30,000 units out (and with a bearing
    // that says so) lands 7.7 million pixels away, where f32 pixel coordinates are coarser
    // than the tolerance.
    let far: Vec<(i16, i16, bool)> = SQUARE
        .iter()
        .map(|&(x, y, on)| (x / 100 + 30_000, y / 100, on))
        .collect();
    let glyphs = [Vec::new(), simple_glyph(&[&far])];
    let font = parse(tt_font(
        &glyphs,
        &[(1, 0), (1, 30_000)],
        &[(b"head", head(16, false))],
    ));
    let size = Rasterizer::new()
        .rasterize(&font, GlyphId(1), 4096.0, 0.25, 0.0)
        .map(|i| i.map(|i| (i.left, i.width)));
    assert_eq!(size, Err(FontError::TooLarge));
}

#[test]
fn flag_repeats_must_stay_within_the_points() {
    // Two points, but the first flag repeats 255 times.
    let mut glyph = u16s(&[1, 0, 0, 0, 0, 1, 0]);
    glyph.extend([0x01 | 0x08 | 0x10 | 0x20, 255]);
    let font = parse(tt_font(&[Vec::new(), glyph], &[(0, 0)], &[]));
    assert_eq!(outline(&font, 1), Err(FontError::Malformed("glyf")));
}

#[test]
fn outlines_start_at_the_left_side_bearing() {
    // hmtx puts the square's left edge 100 units right of the origin, though its points
    // start at 0; glyph 2 is past the long metrics and uses the short bearing list.
    let mut metrics: Vec<u8> = [500u16.to_be_bytes(), 0i16.to_be_bytes()].concat();
    metrics.extend([500u16.to_be_bytes(), 100i16.to_be_bytes()].concat());
    metrics.extend((-50i16).to_be_bytes());
    let glyphs = [Vec::new(), simple_glyph(&[SQUARE]), simple_glyph(&[SQUARE])];
    let font = parse(tt_font(
        &glyphs,
        &[(500, 0), (500, 100)],
        &[(b"hmtx", metrics)],
    ));
    let left = |g| match font.bounds(GlyphId(g)) {
        Ok(Some(b)) => (b.x_min, b.x_max),
        other => panic!("glyph {g}: {other:?}"),
    };
    assert_eq!(left(1), (100.0, 600.0));
    assert_eq!(left(2), (-50.0, 450.0));
}

#[test]
fn glyph_counts_match_the_outlines_and_metrics_are_required() {
    let glyphs = [Vec::new(), simple_glyph(&[SQUARE])];
    // maxp claims more glyphs than loca locates.
    let font = parse(tt_font(&glyphs, &[(500, 0)], &[(b"maxp", maxp(40))]));
    assert_eq!(font.glyph_count(), 2);
    assert_eq!(font.advance(GlyphId(2)), None);
    // A CFF with fewer charstrings than maxp says.
    let cff = parse(otf(cff_plain(&cff_charstrings()[..2], &[], &[]), 9));
    assert_eq!(cff.glyph_count(), 2);
    // No horizontal metrics at all.
    let error = Font::from_data(
        tt_font(&glyphs, &[(500, 0)], &[(b"hhea", hhea(800, -200, 0, 0))]),
        0,
    );
    assert_eq!(error.err(), Some(FontError::Malformed("hmtx")));
}

#[test]
fn weight_without_os2_follows_the_bold_flag() {
    let glyphs = [Vec::new(), simple_glyph(&[SQUARE])];
    let (glyf, loca) = glyf_loca(&glyphs, false);
    let mut bold_head = head(1000, false);
    bold_head[44..46].copy_from_slice(&1u16.to_be_bytes());
    let font = parse(sfnt(
        &[0, 1, 0, 0],
        &[
            (b"cmap", glyf_cmap()),
            (b"glyf", glyf),
            (b"head", bold_head),
            (b"hhea", hhea(800, -200, 0, 1)),
            (b"hmtx", hmtx(&[500], 2)),
            (b"loca", loca),
            (b"maxp", maxp(2)),
        ],
    ));
    assert_eq!(font.style().weight, 700);
}

#[test]
fn bitmap_only_fonts_are_unsupported() {
    // Apple's bitmap fonts have `bhed` in place of `head`; others have `head` and `EBDT`.
    let apple = sfnt(
        &[0, 1, 0, 0],
        &[(b"bdat", vec![0; 8]), (b"bhed", head(1000, false))],
    );
    let embedded = sfnt(
        &[0, 1, 0, 0],
        &[
            (b"EBDT", vec![0; 8]),
            (b"cmap", glyf_cmap()),
            (b"head", head(1000, false)),
            (b"hhea", hhea(800, -200, 0, 1)),
            (b"hmtx", hmtx(&[500], 1)),
            (b"maxp", maxp(1)),
        ],
    );
    for data in [apple, embedded] {
        assert_eq!(
            Font::from_data(data, 0).err(),
            Some(FontError::Unsupported("bitmap-only font"))
        );
    }
}

#[test]
fn rasterizing_bounds_the_work_per_glyph() {
    // A 1 × 8,000 unit bar, then three levels of composites each using the level below 59
    // times in place: glyph 4 is 205,379 bars on top of each other. Each bar is cheap and the
    // image small, but filling them all would cross 3 billion rows.
    let bar = simple_glyph(&[&[(0, 0, true), (0, 8000, true), (1, 8000, true), (1, 0, true)]]);
    let reuse = |g| composite_glyph(&vec![(0x0001 | 0x0002, g, words(&[0, 0])); 59]);
    let glyphs = [Vec::new(), bar, reuse(1), reuse(2), reuse(3)];
    let font = parse(tt_font(&glyphs, &[(1, 0)], &[]));
    let mut r = Rasterizer::new();
    let image = r.rasterize(&font, GlyphId(2), 1000.0, 0.0, 0.0);
    assert!(matches!(
        image,
        Ok(Some(GlyphImage {
            width: 1,
            height: 8000,
            ..
        }))
    ));
    let start = std::time::Instant::now();
    assert_eq!(
        r.rasterize(&font, GlyphId(4), 1000.0, 0.0, 0.0),
        Err(FontError::TooLarge)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn flat_curves_count_against_the_work_limit() {
    // 10,000 curves whose control points lie 32,000 units out along the baseline: each
    // flattens into about 700 horizontal pieces that add no area. Forty copies would take
    // 280 million steps to flatten.
    let mut flat = Vec::new();
    for _ in 0..10_000 {
        flat.extend([(0, 0, true), (32_000, 0, false)]);
    }
    let glyphs = [
        Vec::new(),
        simple_glyph(&[&flat]),
        composite_glyph(&vec![(0x0001 | 0x0002, 1, words(&[0, 0])); 40]),
    ];
    let font = parse(tt_font(&glyphs, &[(1, 0)], &[]));
    let start = std::time::Instant::now();
    assert_eq!(
        Rasterizer::new().rasterize(&font, GlyphId(2), 1000.0, 0.0, 0.0),
        Err(FontError::TooLarge)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn nearly_horizontal_edges_add_almost_nothing() {
    // A sliver 4,096 units wide and 1 unit tall, flattened by eight nested composites to
    // 2⁻¹¹² units tall and stretched to 3,280 px: its slope overflows f32.
    let sliver = simple_glyph(&[&[(0, 0, true), (4096, -1, true), (0, -1, true)]]);
    let mut glyphs = vec![Vec::new(), sliver];
    for g in 1..=8 {
        glyphs.push(composite_glyph(&[(
            0x0001 | 0x0002 | 0x0040,
            g,
            words(&[0, 0, 24576, 1]),
        )]));
    }
    let font = parse(tt_font(&glyphs, &[(1, 0)], &[]));
    let image = image(&font, 9, 1000.0 / 32.0, 0.0);
    assert!(image.width > 3000);
    assert!(
        image.data.iter().all(|&c| c == 0),
        "a sliver came out visible"
    );
}

#[test]
fn curves_stay_within_the_tolerance_however_tall() {
    // A parabola 1,000 px wide and 2,048 px tall needs 256 pieces to stay within 1/32 px.
    // Its x is 1,000·t, so each segment's parameters are known: halfway between them the
    // curve is farthest from the chord.
    let arch = simple_glyph(&[&[(0, 0, true), (500, 4096, false), (1000, 0, true)]]);
    let font = parse(tt_font(&[Vec::new(), arch], &[(1000, 0)], &[]));
    let segments = Rasterizer::new()
        .flatten(&font, GlyphId(1), 1000.0)
        .expect("fits the limits");
    let curve = |t: f32| (1000.0 * t, -8192.0 * t * (1.0 - t));
    let mut worst = 0.0f32;
    for [x0, y0, x1, y1] in segments {
        let (cx, cy) = curve((x0 + x1) / 2000.0);
        worst = worst.max(((x0 + x1) / 2.0 - cx).hypot((y0 + y1) / 2.0 - cy));
    }
    assert!(worst <= raster::TOLERANCE * 1.01, "strays {worst} px");
    assert_coverage(&font, 1, 1000.0, 0.0);
}

#[test]
fn cmap_prefers_full_unicode_and_reads_variation_sequences() {
    let font = parse(glyf_font());
    assert_eq!(font.glyph_index('A'), Some(GlyphId(1)));
    assert_eq!(font.glyph_index('B'), Some(GlyphId(2)));
    assert_eq!(font.glyph_index('😀'), Some(GlyphId(8)));
    assert_eq!(font.glyph_index('C'), None);
    // 葛 with VS17 has its own glyph; 辺 with VS17 uses its usual one; 東 is not listed.
    assert_eq!(font.glyph_variant('葛', '\u{E0100}'), Some(GlyphId(10)));
    assert_eq!(font.glyph_variant('辺', '\u{E0100}'), Some(GlyphId(11)));
    assert_eq!(font.glyph_variant('東', '\u{E0100}'), None);
    assert_eq!(font.glyph_variant('葛', '\u{E0101}'), None);
}

fn cmap(subtables: &[(u16, u16, Vec<u8>)]) -> (Cmap, Vec<u8>) {
    let table = cmap_table(subtables);
    let span = Span {
        start: 0,
        end: table.len(),
    };
    (Cmap::parse(&table, span).expect("valid cmap"), table)
}

#[test]
fn cmap_format_4_segments_and_glyph_arrays() {
    let (map, data) = cmap(&[(
        3,
        1,
        format4(&[
            Segment::Delta(0x20, 0x20, 3u16.wrapping_sub(0x20)),
            Segment::Array(0x41, vec![5, 6, 0, 7]),
            Segment::Delta(0xF000, 0xF0FF, 0x1000),
        ]),
    )]);
    assert_eq!(map.lookup(&data, 0x20), Some(3));
    assert_eq!(map.lookup(&data, 0x41), Some(5));
    assert_eq!(map.lookup(&data, 0x42), Some(6));
    assert_eq!(map.lookup(&data, 0x43), None);
    assert_eq!(map.lookup(&data, 0x44), Some(7));
    assert_eq!(map.lookup(&data, 0x45), None);
    // Deltas wrap around 65536.
    assert_eq!(map.lookup(&data, 0xF001), Some(1));
    assert_eq!(map.lookup(&data, 0x1F600), None);
    assert_eq!(map.variation(&data, 0x41, 0xFE00), Variation::Missing);
}

#[test]
fn symbol_fonts_map_ascii_through_the_private_use_area() {
    let (map, data) = cmap(&[(
        3,
        0,
        format4(&[Segment::Delta(0xF041, 0xF041, 9u16.wrapping_sub(0xF041))]),
    )]);
    assert_eq!(map.lookup(&data, 0x41), Some(9));
    assert_eq!(map.lookup(&data, 0xF041), Some(9));
}

#[test]
fn cmap_byte_trimmed_and_constant_formats() {
    let mut format0 = u16s(&[0, 262, 0]);
    let mut glyphs = [0u8; 256];
    glyphs[0x41] = 5;
    format0.extend(glyphs);
    let (map, data) = cmap(&[(1, 0, format0)]);
    assert_eq!(map.lookup(&data, 0x41), Some(5));
    // Mac Roman subtables only serve ASCII.
    assert_eq!(map.lookup(&data, 0xC4), None);

    let (map, data) = cmap(&[(3, 1, u16s(&[6, 16, 0, 0x30, 3, 7, 8, 9]))]);
    assert_eq!(
        [0x2F, 0x30, 0x32, 0x33].map(|c| map.lookup(&data, c)),
        [None, Some(7), Some(9), None]
    );

    let mut format13 = u16s(&[13, 0]);
    format13.extend(u32s(&[16 + 12, 0, 1, 0x1_0000, 0x1_FFFF, 4]));
    let (map, data) = cmap(&[(3, 10, format13)]);
    assert_eq!(map.lookup(&data, 0x1_2345), Some(4));
    assert_eq!(map.lookup(&data, 0x2_0000), None);
}

#[test]
fn broken_subtables_do_not_hide_usable_ones() {
    // An empty format 12 would outrank the format 4.
    let mut empty12 = u16s(&[12, 0]);
    empty12.extend(u32s(&[16, 0, 0]));
    let (map, data) = cmap(&[
        (3, 10, empty12),
        (
            3,
            1,
            format4(&[Segment::Delta(0x41, 0x41, 3u16.wrapping_sub(0x41))]),
        ),
    ]);
    assert_eq!(map.lookup(&data, 0x41), Some(3));
    // A symbol font mapping 'A' itself to the missing glyph still finds U+F041.
    let (map, data) = cmap(&[(
        3,
        0,
        format4(&[
            Segment::Delta(0x41, 0x41, 0u16.wrapping_sub(0x41)),
            Segment::Delta(0xF041, 0xF041, 9u16.wrapping_sub(0xF041)),
        ]),
    )]);
    assert_eq!(map.lookup(&data, 0x41), Some(9));
    // A format 12 that declares more groups than it holds.
    let mut short12 = format12(&[(0x42, 0x42, 5)]);
    short12[12..16].copy_from_slice(&2u32.to_be_bytes());
    let (map, data) = cmap(&[
        (3, 10, short12),
        (
            3,
            1,
            format4(&[Segment::Delta(0x41, 0x41, 3u16.wrapping_sub(0x41))]),
        ),
    ]);
    assert_eq!(map.lookup(&data, 0x41), Some(3));
}

#[test]
fn cmap_arrays_stay_inside_their_subtable() {
    // Format 4's glyph array must lie before the next subtable: here the first segment's range
    // offset points just past the subtable, at a format 6 header. The format 4 is skipped, and
    // the Mac Roman subtable serves instead of a glyph read from that header.
    let mut f4 = format4(&[Segment::Delta(0x41, 0x41, 0)]);
    f4[28..30].copy_from_slice(&4u16.to_be_bytes());
    let f6 = u16s(&[6, 12, 0, 0x41, 1, 9]);
    let (map, data) = cmap(&[(3, 1, f4), (1, 0, f6)]);
    assert_eq!(map.lookup(&data, 0x41), Some(9));
    // A format 13 declaring more groups than it holds is skipped like format 12.
    let mut short13 = u16s(&[13, 0]);
    short13.extend(u32s(&[16 + 12, 0, 2, 0x41, 0x41, 4]));
    let (map, data) = cmap(&[
        (0, 6, short13),
        (
            3,
            1,
            format4(&[Segment::Delta(0x41, 0x41, 3u16.wrapping_sub(0x41))]),
        ),
    ]);
    assert_eq!(map.lookup(&data, 0x41), Some(3));
    // A variation selector whose mapping array is cut short has no mappings.
    let mut f14 = format14(0xFE00, &[], &[(0x41, 7)]);
    let (map, data) = cmap(&[(0, 5, f14.clone())]);
    assert!(matches!(
        map.variation(&data, 0x41, 0xFE00),
        Variation::Glyph(7)
    ));
    f14[25..29].copy_from_slice(&2u32.to_be_bytes());
    let (map, data) = cmap(&[(0, 5, f14)]);
    assert!(matches!(
        map.variation(&data, 0x41, 0xFE00),
        Variation::Missing
    ));
}

#[test]
fn cmap_preference_and_format_4_lengths() {
    // Format 13 covers characters past the BMP, so it beats format 4.
    let mut format13 = u16s(&[13, 0]);
    format13.extend(u32s(&[16 + 12, 0, 1, 0x1_0000, 0x1_0000, 4]));
    let (map, data) = cmap(&[
        (0, 6, format13),
        (
            3,
            1,
            format4(&[Segment::Delta(0x41, 0x41, 3u16.wrapping_sub(0x41))]),
        ),
    ]);
    assert_eq!(map.lookup(&data, 0x1_0000), Some(4));
    // Format 12 beats format 13.
    let mut format13 = u16s(&[13, 0]);
    format13.extend(u32s(&[16 + 12, 0, 1, 0x41, 0x41, 4]));
    let (map, data) = cmap(&[(0, 6, format13), (0, 4, format12(&[(0x41, 0x41, 5)]))]);
    assert_eq!(map.lookup(&data, 0x41), Some(5));
    // A format 4 length too small for its arrays, as when a large subtable's length wraps
    // past 65,535, is ignored.
    let mut wrapped = format4(&[Segment::Delta(0x41, 0x41, 3u16.wrapping_sub(0x41))]);
    wrapped[2..4].copy_from_slice(&16u16.to_be_bytes());
    let (map, data) = cmap(&[(3, 1, wrapped)]);
    assert_eq!(map.lookup(&data, 0x41), Some(3));
}

// ---- Metrics and names ----

#[test]
fn metrics_style_and_names() {
    let font = parse(glyf_font());
    assert_eq!(font.glyph_count(), 12);
    assert_eq!(font.advance(GlyphId(1)), Some(500));
    // Glyphs past the long metrics share the last advance.
    assert_eq!(font.advance(GlyphId(9)), Some(600));
    assert_eq!(font.advance(GlyphId(12)), None);
    let m = font.metrics();
    // USE_TYPO_METRICS selects the OS/2 values over hhea's.
    assert_eq!(
        (m.units_per_em, m.ascender, m.descender, m.line_gap),
        (1000, 750, -250, 100)
    );
    assert_eq!((m.x_height, m.cap_height), (Some(450), Some(650)));
    assert_eq!((m.underline_position, m.underline_thickness), (-100, 50));
    assert_eq!((m.strikeout_position, m.strikeout_thickness), (250, 40));
    assert_eq!(
        font.style(),
        FontStyle {
            weight: 700,
            width: 5,
            italic: true,
            monospace: false
        }
    );
    assert_eq!(font.family_name().as_deref(), Some("Test"));
    assert_eq!(font.subfamily_name().as_deref(), Some("Bold"));
    assert_eq!(font.full_name().as_deref(), Some("Test Sans Bold"));
    assert_eq!(font.postscript_name().as_deref(), Some("TestSans-Bold"));
    assert!(!font.is_variable());
}

#[test]
fn hhea_metrics_without_the_typo_flag() {
    let data = sfnt(
        &[0, 1, 0, 0],
        &[
            (b"OS/2", os2(400, 0, (750, -250, 100), 0, 0)),
            (b"cmap", glyf_cmap()),
            (b"fvar", vec![0; 16]),
            (b"glyf", vec![]),
            (b"head", head(2048, false)),
            (b"hhea", hhea(1900, -500, 0, 1)),
            (b"hmtx", hmtx(&[1000], 1)),
            (b"loca", u16s(&[0, 0])),
            (b"maxp", maxp(1)),
            (b"name", name_table(&[(1, 0, 0, 1, "Mac Only")])),
        ],
    );
    let font = parse(data);
    let m = font.metrics();
    assert_eq!((m.ascender, m.descender, m.line_gap), (1900, -500, 0));
    assert_eq!((m.x_height, m.cap_height), (None, None));
    assert!(font.is_variable());
    assert!(!font.style().italic);
    assert_eq!(font.family_name().as_deref(), Some("Mac Only"));
}

#[test]
fn metrics_fall_back_when_hhea_is_empty() {
    let glyphs = [Vec::new()];
    let metrics = |os2_table: Vec<u8>| {
        let data = tt_font(
            &glyphs,
            &[(0, 0)],
            &[(b"hhea", hhea(0, 0, 0, 1)), (b"OS/2", os2_table)],
        );
        let m = parse(data).metrics();
        (m.ascender, m.descender, m.line_gap)
    };
    assert_eq!(metrics(os2(400, 0, (700, -300, 50), 0, 0)), (700, -300, 50));
    let mut windows = os2(400, 0, (0, 0, 0), 0, 0);
    windows[74..76].copy_from_slice(&900u16.to_be_bytes());
    windows[76..78].copy_from_slice(&250u16.to_be_bytes());
    assert_eq!(metrics(windows.clone()), (900, -250, 0));
    windows[76..78].copy_from_slice(&32768u16.to_be_bytes());
    assert_eq!(metrics(windows), (900, -32768, 0));
}

#[test]
fn names_in_mac_roman_and_other_english_locales() {
    let mut mac = name_table(&[(1, 0, 0, 1, "Cafe")]);
    let last = mac.len() - 1;
    mac[last] = 0x8E;
    let glyphs = [Vec::new()];
    let font = parse(tt_font(&glyphs, &[(0, 0)], &[(b"name", mac)]));
    assert_eq!(font.family_name().as_deref(), Some("Café"));
    let names = name_table(&[(3, 1, 0x411, 1, "テスト"), (3, 1, 0x809, 1, "Colour")]);
    let font = parse(tt_font(&glyphs, &[(0, 0)], &[(b"name", names)]));
    assert_eq!(font.family_name().as_deref(), Some("Colour"));
    // Symbol fonts write their Windows names in the symbol encoding, still UTF-16.
    let names = name_table(&[(3, 0, 0x409, 1, "Wingbats")]);
    let font = parse(tt_font(&glyphs, &[(0, 0)], &[(b"name", names)]));
    assert_eq!(font.family_name().as_deref(), Some("Wingbats"));
}

#[test]
fn collection_offsets_are_checked() {
    let mut data = b"ttcf".to_vec();
    data.extend(u32s(&[0x0001_0000, 0x4000_0001, 12]));
    assert_eq!(
        Font::from_data(data, 0x4000_0000).err(),
        Some(FontError::Malformed("collection header"))
    );
}

#[test]
fn collections_hold_several_faces() {
    let tables = |weight| {
        let glyphs = glyf_glyphs();
        let (glyf, loca) = glyf_loca(&glyphs, false);
        vec![
            (b"OS/2", os2(weight, 0, (0, 0, 0), 0, 0)),
            (b"cmap", glyf_cmap()),
            (b"glyf", glyf),
            (b"head", head(1000, false)),
            (b"hhea", hhea(800, -200, 0, 1)),
            (b"hmtx", hmtx(&[500], glyphs.len())),
            (b"loca", loca),
            (b"maxp", maxp(glyphs.len() as u16)),
        ]
    };
    let first = sfnt_at(20, &[0, 1, 0, 0], &tables(400));
    let second = sfnt_at(20 + first.len(), &[0, 1, 0, 0], &tables(700));
    let mut data = b"ttcf".to_vec();
    data.extend(u32s(&[0x0001_0000, 2, 20, (20 + first.len()) as u32]));
    data.extend(first);
    data.extend(second);
    assert_eq!(Font::face_count(&data), Some(2));
    let data: Arc<[u8]> = data.into();
    let weights: Vec<u16> = (0..2)
        .map(|i| {
            Font::from_data(data.clone(), i)
                .expect("face")
                .style()
                .weight
        })
        .collect();
    assert_eq!(weights, [400, 700]);
    assert_eq!(Font::from_data(data, 2).err(), Some(FontError::NoSuchFace));
    assert_eq!(Font::face_count(b"wOFF\0\0\0\0"), None);
    assert_eq!(
        Font::from_data(glyf_font(), 1).err(),
        Some(FontError::NoSuchFace)
    );
}

// ---- CFF ----

#[test]
fn cff_charstrings_draw_lines_and_drop_the_width() {
    let font = parse(cff_font());
    let square = vec![
        M(0.0, 0.0),
        L(500.0, 0.0),
        L(500.0, 500.0),
        L(0.0, 500.0),
        Z,
    ];
    assert_eq!(outline(&font, 0), Ok(vec![]));
    assert_eq!(outline(&font, 1), Ok(square.clone()));
    // Stem hints, a hint mask and both subroutine kinds give the same square.
    assert_eq!(outline(&font, 2), Ok(square));
}

#[test]
fn cff_curve_operators() {
    let font = parse(cff_font());
    assert_eq!(
        outline(&font, 3),
        Ok(vec![
            M(0.0, 0.0),
            C(100.0, 0.0, 200.0, 100.0, 200.0, 200.0),
            C(220.0, 210.0, 250.0, 250.0, 300.0, 250.0),
            C(305.0, 260.0, 325.0, 290.0, 325.0, 330.0),
            C(335.0, 330.0, 355.0, 360.0, 355.0, 400.0),
            C(355.0, 410.0, 375.0, 440.0, 415.0, 490.0),
            C(416.0, 492.0, 419.0, 496.0, 424.0, 502.0),
            L(431.0, 510.0),
            L(432.0, 511.0),
            C(433.0, 513.0, 436.0, 517.0, 441.0, 523.0),
            Z,
        ])
    );
    assert_eq!(
        outline(&font, 4),
        Ok(vec![
            M(0.0, 0.0),
            C(10.0, 0.0, 20.0, 10.0, 30.0, 10.0),
            C(40.0, 10.0, 50.0, 0.0, 60.0, 0.0),
            C(70.0, 0.0, 80.0, 5.0, 90.0, 5.0),
            C(100.0, 5.0, 110.0, 0.0, 120.0, 0.0),
            C(130.0, 5.0, 140.0, 10.0, 150.0, 10.0),
            C(160.0, 10.0, 170.0, 5.0, 180.0, 0.0),
            C(190.0, 10.0, 200.0, 20.0, 210.0, 20.0),
            C(220.0, 10.0, 230.0, 0.0, 250.0, 0.0),
            Z,
        ])
    );
}

#[test]
fn cff_rejects_what_it_cannot_draw() {
    let font = parse(cff_font());
    assert_eq!(
        outline(&font, 5).err(),
        Some(FontError::Unsupported("CFF accented endchar"))
    );
    assert_eq!(
        outline(&font, 6).err(),
        Some(FontError::Unsupported("CFF charstring operator"))
    );
    assert_eq!(
        outline(&font, 7).err(),
        Some(FontError::Malformed("CFF: subroutine nesting"))
    );
    assert_eq!(outline(&font, 8).err(), Some(FontError::Malformed("CFF")));
    assert_eq!(outline(&font, 9).err(), Some(FontError::NoSuchGlyph));
}

#[test]
fn cff_font_matrix_slants_and_scales() {
    let thousandth = real(&[0, 0xA, 0, 0, 1]);
    let zero = vec![139];
    // [0.001 0 0.00025 0.001 0 0]: an oblique, as fonts write it for 1,000 units per em.
    let oblique = [
        thousandth.clone(),
        zero.clone(),
        real(&[0, 0xA, 0, 0, 0, 2, 5]),
        thousandth,
        zero.clone(),
        zero.clone(),
        vec![12, 7],
    ]
    .concat();
    let font = parse(otf(
        cff_plain_with(&cff_charstrings()[..2], &[], &[], &oblique),
        2,
    ));
    assert_eq!(
        outline(&font, 1),
        Ok(vec![
            M(0.0, 0.0),
            L(500.0, 0.0),
            L(625.0, 500.0),
            L(125.0, 500.0),
            Z
        ])
    );
    // A matrix for another units per em keeps only its shape, as in FreeType.
    let half = real(&[0, 0xA, 0, 0, 0, 5]);
    let other = [
        half.clone(),
        zero.clone(),
        zero.clone(),
        half,
        zero.clone(),
        zero,
        vec![12, 7],
    ]
    .concat();
    let font = parse(otf(
        cff_plain_with(&cff_charstrings()[..2], &[], &[], &other),
        2,
    ));
    assert_eq!(outline(&font, 1).map(|o| o[2].clone()), Ok(L(500.0, 500.0)));
}

#[test]
fn cid_font_dict_matrices_keep_their_scale() {
    // Each font DICT says one charstring unit is 1/1000 em, in a font of 2,048 units per em.
    let thousandth = real(&[0, 0xA, 0, 0, 1]);
    let matrix = [
        thousandth.clone(),
        vec![139, 139],
        thousandth,
        vec![139, 139, 12, 7],
    ]
    .concat();
    let draw = [
        op(&[0, 0], &[21]),
        op(&[-107], &[10]),
        op(&[500], &[7]),
        ENDCHAR.to_vec(),
    ]
    .concat();
    let cff = cff_cid_with(
        &[ENDCHAR.to_vec(), draw.clone(), draw],
        [&[op(&[500], &[6, 11])], &[op(&[300], &[6, 11])]],
        &matrix,
    );
    let font = parse(otf_with_units(cff, 3, 2048));
    let b = font.bounds(GlyphId(1)).expect("readable").expect("has ink");
    assert!(
        (b.x_max - 1024.0).abs() < 0.01 && (b.y_max - 1024.0).abs() < 0.01,
        "{b:?}"
    );
    // One unit is a tenth of an em: a hundred font units in a 1,000-unit font.
    let tenth = real(&[0, 0xA, 1]);
    let matrix = [tenth.clone(), vec![139, 139], tenth, vec![139, 139, 12, 7]].concat();
    let draw = [
        op(&[0, 0], &[21]),
        op(&[5], &[6]),
        op(&[5], &[7]),
        ENDCHAR.to_vec(),
    ]
    .concat();
    let cff = cff_cid_with(&[ENDCHAR.to_vec(), draw.clone(), draw], [&[], &[]], &matrix);
    let b = parse(otf(cff, 3))
        .bounds(GlyphId(1))
        .expect("readable")
        .expect("has ink");
    assert!((b.x_max - 500.0).abs() < 0.01, "{b:?}");
}

#[test]
fn cff_charstring_endings_and_extra_operands_follow_freetype() {
    let glyphs = [
        ENDCHAR.to_vec(),
        // A hint takes the width slot; then rmoveto gets three operands and uses the top two,
        // and the charstring ends without endchar.
        [
            op(&[10, 20], &[1]),
            op(&[11, 22, 33], &[21]),
            op(&[10], &[6]),
        ]
        .concat(),
        // Accent operands left at the end still mean an accented endchar.
        op(&[0, 0, 0, 65, 66], &[]),
    ];
    let font = parse(otf(cff_plain(&glyphs, &[], &[]), 3));
    assert_eq!(outline(&font, 1), Ok(vec![M(22.0, 33.0), L(32.0, 33.0), Z]));
    let glyphs = [
        ENDCHAR.to_vec(),
        // hmoveto and vmoveto with extra operands use the top one.
        [
            op(&[10, 20], &[1]),
            op(&[11, 22], &[22]),
            op(&[5], &[7]),
            op(&[1, 2], &[4]),
            op(&[3], &[6]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
        // A subroutine without return returns at its end; return at the top level is an error.
        [op(&[-107], &[10]), op(&[5], &[7]), ENDCHAR.to_vec()].concat(),
        [op(&[0, 0], &[21]), op(&[5], &[6]), vec![11]].concat(),
    ];
    let local = [[op(&[0, 0], &[21]), op(&[5], &[6])].concat()];
    let more = parse(otf(cff_plain(&glyphs, &[], &local), 4));
    assert_eq!(
        outline(&more, 1),
        Ok(vec![
            M(22.0, 0.0),
            L(22.0, 5.0),
            Z,
            M(22.0, 7.0),
            L(25.0, 7.0),
            Z
        ])
    );
    assert_eq!(
        outline(&more, 2),
        Ok(vec![M(0.0, 0.0), L(5.0, 0.0), L(5.0, 5.0), Z])
    );
    assert_eq!(outline(&more, 3), Err(FontError::Malformed("CFF")));
    // Charstring numbers are 16.16 fixed point; 1,024 steps of 1/65,536 add up.
    let step = [vec![255], 1i32.to_be_bytes().to_vec(), vec![6]].concat();
    let glyphs = [
        ENDCHAR.to_vec(),
        [
            op(&[16000, 0], &[21]),
            step.repeat(1024),
            op(&[0, 5], &[5]),
            ENDCHAR.to_vec(),
        ]
        .concat(),
    ];
    let commands = outline(&parse(otf(cff_plain(&glyphs, &[], &[]), 2)), 1).expect("readable");
    assert_eq!(commands[commands.len() - 2], L(16000.0 + 1.0 / 64.0, 5.0));
    assert_eq!(
        outline(&font, 2),
        Err(FontError::Unsupported("CFF accented endchar"))
    );
}

#[test]
fn private_dicts_are_parsed_once_and_bounded() {
    // Both font DICTs name the same 600,000-byte Private DICT, a run of zeros after the CFF;
    // read twice it would exceed the 1 MiB budget. One of 1,100,000 bytes exceeds it alone.
    let glyphs = [ENDCHAR.to_vec(), ENDCHAR.to_vec(), ENDCHAR.to_vec()];
    let private = |size: usize, offset: usize| [int5(size), int5(offset), vec![18]].concat();
    let base = cff_cid_with(&glyphs, [&[], &[]], &private(0, 0)).len();
    for (size, expected) in [(600_000, None), (1_100_000, Some(FontError::TooLarge))] {
        let mut cff = cff_cid_with(&glyphs, [&[], &[]], &private(size, base));
        cff.resize(base + size, 0);
        assert_eq!(Font::from_data(otf(cff, 3), 0).err(), expected, "{size}");
    }
}

#[test]
fn cff_index_entries_stay_inside_their_index() {
    // Offsets [1, 3, 2]: glyph 0 would run past the CharStrings data, glyph 1 backwards.
    let mut cff = cff_plain(&[ENDCHAR.to_vec(), ENDCHAR.to_vec()], &[], &[]);
    let at = cff
        .windows(8)
        .position(|w| w == [0, 2, 1, 1, 2, 3, 14, 14])
        .expect("the CharStrings INDEX");
    cff[at + 4..at + 6].copy_from_slice(&[3, 2]);
    let font = parse(otf(cff, 2));
    assert_eq!(outline(&font, 0), Err(FontError::Malformed("CFF")));
    assert_eq!(outline(&font, 1), Err(FontError::Malformed("CFF")));
}

#[test]
fn cid_fonts_pick_subroutines_by_font_dict() {
    let font = parse(cid_font());
    assert_eq!(
        outline(&font, 1),
        Ok(vec![M(0.0, 0.0), L(500.0, 0.0), L(500.0, 500.0), Z])
    );
    assert_eq!(
        outline(&font, 2),
        Ok(vec![M(0.0, 0.0), L(300.0, 0.0), L(300.0, 500.0), Z])
    );
}

// ---- Rasterization ----

fn image(font: &Font, glyph: u16, size: f32, x: f32) -> GlyphImage {
    Rasterizer::new()
        .rasterize(font, GlyphId(glyph), size, x, 0.0)
        .expect("rasterizes")
        .expect("has ink")
}

#[test]
fn coverage_follows_the_signed_area() {
    let square = |x0: i16, y0: i16, size: i16, clockwise: bool| {
        let mut points = vec![
            (x0, y0, true),
            (x0, y0 + size, true),
            (x0 + size, y0 + size, true),
            (x0 + size, y0, true),
        ];
        if !clockwise {
            points.reverse();
        }
        points
    };
    let glyphs = [
        Vec::new(),
        // A square with a square hole wound the other way.
        simple_glyph(&[&square(0, 0, 1000, true), &square(250, 250, 500, false)]),
        // The same square twice: covered once, not twice.
        simple_glyph(&[&square(0, 0, 1000, true), &square(0, 0, 1000, true)]),
        // Two halves of one pixel wound in opposite directions cancel: the documented cost of
        // taking the absolute signed area.
        simple_glyph(&[&square(0, 0, 125, true), &square(125, 0, 125, false)]),
    ];
    let font = parse(tt_font(&glyphs, &[(0, 0)], &[]));
    let image = |g| {
        let i = Rasterizer::new()
            .rasterize(&font, GlyphId(g), 4.0, 0.0, 0.0)
            .expect("rasterizes")
            .expect("ink");
        (i.width, i.height, i.data)
    };
    #[rustfmt::skip]
    let ring = vec![
        255, 255, 255, 255,
        255,   0,   0, 255,
        255,   0,   0, 255,
        255, 255, 255, 255,
    ];
    assert_eq!(image(1), (4, 4, ring));
    assert_eq!(image(2), (4, 4, vec![255; 16]));
    assert_eq!(image(3).2, [0]);
}

#[test]
fn sizes_and_offsets_must_be_usable() {
    let font = parse(glyf_font());
    let mut r = Rasterizer::new();
    for (size, x, y) in [
        (f32::NAN, 0.0, 0.0),
        (f32::INFINITY, 0.0, 0.0),
        (0.0, 0.0, 0.0),
        (-12.0, 0.0, 0.0),
        (12.0, f32::NAN, 0.0),
        (12.0, 0.0, f32::NEG_INFINITY),
    ] {
        assert_eq!(
            r.rasterize(&font, GlyphId(1), size, x, y).err(),
            Some(FontError::InvalidSize),
            "{size} {x} {y}"
        );
    }
    // Finite, but past where pixel coordinates hold the tolerance.
    assert_eq!(
        r.rasterize(&font, GlyphId(1), 12.0, 1e9, 0.0).err(),
        Some(FontError::TooLarge)
    );
}

#[test]
fn pixel_aligned_squares_are_fully_covered() {
    for font in [parse(glyf_font()), parse(cff_font())] {
        let square = image(&font, 1, 10.0, 0.0);
        assert_eq!(
            (square.left, square.top, square.width, square.height),
            (0, -5, 5, 5)
        );
        assert!(square.data.iter().all(|&c| c == 255));
        // Half a pixel to the right: the edge columns are half covered.
        let moved = image(&font, 1, 10.0, 0.5);
        assert_eq!((moved.left, moved.width), (0, 6));
        for row in moved.data.as_chunks::<6>().0 {
            assert_eq!(*row, [128, 255, 255, 255, 255, 128]);
        }
    }
}

/// The area inside an outline and its perimeter, in font units, from finely subdivided curves.
fn area_and_perimeter(cmds: &[Cmd]) -> (f32, f32) {
    let mut points = Vec::new();
    let mut contours = Vec::new();
    let mut current = (0.0f32, 0.0f32);
    for cmd in cmds {
        match *cmd {
            M(x, y) => {
                if !points.is_empty() {
                    contours.push(std::mem::take(&mut points));
                }
                current = (x, y);
                points.push(current);
            }
            L(x, y) => {
                current = (x, y);
                points.push(current);
            }
            Q(x1, y1, x, y) => {
                for i in 1..=200 {
                    let t = i as f32 / 200.0;
                    let u = 1.0 - t;
                    points.push((
                        u * u * current.0 + 2.0 * u * t * x1 + t * t * x,
                        u * u * current.1 + 2.0 * u * t * y1 + t * t * y,
                    ));
                }
                current = (x, y);
            }
            C(x1, y1, x2, y2, x, y) => {
                for i in 1..=200 {
                    let t = i as f32 / 200.0;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    points.push((
                        a * current.0 + b * x1 + c * x2 + d * x,
                        a * current.1 + b * y1 + c * y2 + d * y,
                    ));
                }
                current = (x, y);
            }
            Z => {}
        }
    }
    contours.push(points);
    let mut area = 0.0;
    let mut perimeter = 0.0;
    for c in &contours {
        let edges = || c.iter().zip(c.iter().cycle().skip(1));
        area += edges()
            .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
            .sum::<f32>()
            .abs()
            / 2.0;
        perimeter += edges()
            .map(|(a, b)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt())
            .sum::<f32>();
    }
    (area, perimeter)
}

/// Checks that a glyph's total coverage matches its area: flattening moves the edge by at most
/// the tolerance, and rounding to bytes moves each edge pixel by at most half a level.
fn assert_coverage(font: &Font, glyph: u16, size: f32, x: f32) {
    let (area, perimeter) = area_and_perimeter(&outline(font, glyph).expect("readable"));
    let img = image(font, glyph, size, x);
    let covered: f32 = img.data.iter().map(|&c| f32::from(c) / 255.0).sum();
    let scale = size / f32::from(font.metrics().units_per_em);
    let edge_pixels = 2.0 * perimeter * scale + 8.0;
    let allowed = perimeter * scale * raster::TOLERANCE + edge_pixels * 0.5 / 255.0;
    let error = (covered - area * scale * scale).abs();
    assert!(
        error <= allowed,
        "glyph {glyph} at {size}px: {error} > {allowed}"
    );
}

#[test]
fn coverage_adds_up_to_the_area() {
    let (glyf, cff) = (parse(glyf_font()), parse(cff_font()));
    for (size, x) in [(10.0f32, 0.0f32), (33.0, 0.25), (120.0, 0.7)] {
        for glyph in [2u16, 8] {
            assert_coverage(&glyf, glyph, size, x);
        }
        assert_coverage(&cff, 3, size, x);
    }
}

#[test]
fn rasterizer_limits() {
    let font = parse(glyf_font());
    let mut r = Rasterizer::new();
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY, MAX_GLYPH_SIZE + 1.0] {
        assert_eq!(
            r.rasterize(&font, GlyphId(1), size, 0.0, 0.0),
            Err(FontError::InvalidSize)
        );
    }
    assert_eq!(
        r.rasterize(&font, GlyphId(1), 10.0, f32::NAN, 0.0),
        Err(FontError::InvalidSize)
    );
    assert_eq!(r.rasterize(&font, GlyphId(0), 10.0, 0.0, 0.0), Ok(None));
    let mut size = |g| {
        r.rasterize(&font, GlyphId(g), 4096.0, 0.0, 0.0)
            .map(|i| i.map(|i| (i.width, i.height)))
    };
    // At a 4,096 px em the square is 2,048 px a side, exactly the pixel limit; glyph 4 is
    // twice as wide.
    assert_eq!(size(1), Ok(Some((2048, 2048))));
    assert_eq!(size(4), Err(FontError::TooLarge));
    assert_eq!(
        r.rasterize(&font, GlyphId(5), 10.0, 0.0, 0.0),
        Err(FontError::Malformed("glyf: composite nesting"))
    );
}

// ---- Malformed data ----

/// Parses, outlines and rasterizes everything a font claims to have; it may fail, never panic.
fn exercise(data: &[u8], rasterizer: &mut Rasterizer) {
    let Some(count) = Font::face_count(data) else {
        let _ = Font::from_data(data.to_vec(), 0);
        return;
    };
    for index in 0..count.min(4) {
        let Ok(font) = Font::from_data(data.to_vec(), index) else {
            continue;
        };
        let _ = (font.family_name(), font.full_name(), font.metrics());
        for c in ['A', 'B', '葛', '😀'] {
            let _ = (font.glyph_index(c), font.glyph_variant(c, '\u{E0100}'));
        }
        for glyph in 0..font.glyph_count().min(16) {
            let _ = font.advance(GlyphId(glyph));
            let _ = font.bounds(GlyphId(glyph));
            let _ = rasterizer.rasterize(&font, GlyphId(glyph), 24.0, 0.3, 0.0);
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn truncated_fonts_fail_cleanly() {
    let mut rasterizer = Rasterizer::new();
    for data in [glyf_font(), cff_font(), cid_font()] {
        for len in 0..data.len() {
            exercise(&data[..len], &mut rasterizer);
        }
    }
}

#[test]
fn corrupted_fonts_fail_cleanly() {
    let mut rasterizer = Rasterizer::new();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for data in [glyf_font(), cff_font(), cid_font()] {
        for _ in 0..3000 {
            let mut copy = data.clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(copy.len());
                copy[at] = match rng.below(4) {
                    0 => 0,
                    1 => 0xFF,
                    _ => rng.next() as u8,
                };
            }
            exercise(&copy, &mut rasterizer);
        }
    }
}

// ---- Installed fonts ----

/// Font directories of the operating system, plus any in `DUYDAS_FONT_DIRS`.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("DUYDAS_FONT_DIRS")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    if cfg!(windows) {
        let windows =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        dirs.push(windows.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
    }
    dirs
}

fn collect(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                collect(&path, depth + 1, out);
            }
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["ttf", "otf", "ttc", "otc"].contains(&e.to_ascii_lowercase().as_str())
        }) {
            out.push(path);
        }
    }
}

/// Checks a simple glyph's header box against its decoded points. The specification asks for
/// the box around every point, but tools record others: FontForge the box of the cubic outline
/// it converted from, which lies inside the points' box, and some icon font tools a box from
/// the origin. All of them contain the explicit on-curve points, which lie on every version of
/// the outline, give or take a unit of rounding; decoding errors in signs, flags or offsets
/// move those points outside it. Returns a description, whether the header is exactly the
/// points' box, and whether it contains the on-curve points.
fn header_box(font: &Font, glyph: GlyphId) -> Option<(String, bool, bool)> {
    let Outlines::Glyf(glyf) = &font.outlines else {
        return None;
    };
    let bytes = glyf.bytes(&font.data, glyph.0).ok()?;
    let value = |at| reader::i16_at(bytes, at).map(f32::from);
    let h = [value(2)?, value(4)?, value(6)?, value(8)?];
    let (all, on) = glyf.boxes(&font.data, glyph.0)?;
    // A glyph drawn with off-curve points alone, such as a circle, has none to check.
    let contains_on_curve = on.is_none_or(|on| {
        on[0] >= h[0] - 1.0 && on[1] >= h[1] - 1.0 && on[2] <= h[2] + 1.0 && on[3] <= h[3] + 1.0
    });
    let description = format!("header {h:?}, points {all:?}, on-curve points {on:?}");
    Some((description, h == all, contains_on_curve))
}

/// Parses every installed font and outlines and rasterizes a sample of each one's glyphs
/// (all of them with `DUYDAS_FONT_FULL` set). Every simple TrueType glyph's points must agree
/// with the box its header records, as `header_box` describes.
#[test]
fn installed_fonts_parse_and_rasterize() {
    let full = std::env::var_os("DUYDAS_FONT_FULL").is_some();
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect(&dir, 0, &mut files);
    }
    files.sort();
    let mut rasterizer = Rasterizer::new();
    let mut failures = Vec::new();
    let (mut faces, mut glyphs, mut unsupported_faces, mut unsupported_glyphs) = (0, 0, 0, 0);
    let (mut boxes, mut implausible, mut tool_boxes) = (0, Vec::new(), 0);
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let data: Arc<[u8]> = bytes.into();
        let Some(count) = Font::face_count(&data) else {
            continue;
        };
        for index in 0..count.min(64) {
            let font = match Font::from_data(data.clone(), index) {
                Ok(font) => font,
                Err(FontError::Unsupported(_)) => {
                    unsupported_faces += 1;
                    continue;
                }
                Err(e) => {
                    failures.push(format!("{}:{index}: {e}", path.display()));
                    continue;
                }
            };
            faces += 1;
            let mut inexact_face = false;
            let n = font.glyph_count();
            let step = if full { 1 } else { usize::from(n / 48).max(1) };
            for g in (0..n).step_by(step) {
                let glyph = GlyphId(g);
                glyphs += 1;
                match font.bounds(glyph) {
                    Ok(_) => {
                        if let Some((description, exact, plausible)) = header_box(&font, glyph) {
                            boxes += 1;
                            inexact_face |= !exact;
                            if !plausible {
                                implausible.push(format!(
                                    "{}:{index} glyph {g}: {description}",
                                    path.display()
                                ));
                            }
                        }
                    }
                    Err(FontError::Unsupported(_)) => {
                        unsupported_glyphs += 1;
                        continue;
                    }
                    Err(e) => {
                        failures.push(format!("{}:{index} glyph {g}: {e}", path.display()));
                        continue;
                    }
                }
                for size in [12.0, 40.0] {
                    match rasterizer.rasterize(&font, glyph, size, 0.25, 0.0) {
                        Ok(Some(img)) => {
                            assert_eq!(img.data.len(), (img.width * img.height) as usize)
                        }
                        Ok(None) => {}
                        Err(e) => failures.push(format!(
                            "{}:{index} glyph {g} at {size}px: {e}",
                            path.display()
                        )),
                    }
                }
            }
            tool_boxes += usize::from(inexact_face);
        }
    }
    eprintln!(
        "{} files, {faces} faces ({unsupported_faces} unsupported), {glyphs} glyphs ({unsupported_glyphs} \
         unsupported), {boxes} simple glyph boxes ({} implausible), {tool_boxes} faces with boxes \
         not exactly by the specification",
        files.len(),
        implausible.len(),
    );
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(implausible.is_empty(), "{implausible:#?}");
}
