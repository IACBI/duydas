//! Layout tests on hand-built GDEF, GSUB and GPOS tables, and the builders the shaping
//! tests share.

use super::*;

// ---- Building tables ----

/// A table field: a word, an offset to a child table (16 or 32 bits, relative to the start
/// of the table holding it), or a null offset.
pub(crate) enum N {
    W(u16),
    I(i16),
    O(T),
    O32(T),
    Null,
}

/// A table: its fields, then its child tables in order.
pub(crate) struct T(pub(crate) Vec<N>);

impl T {
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut slots = Vec::new();
        for field in &self.0 {
            match field {
                N::W(v) => out.extend(v.to_be_bytes()),
                N::I(v) => out.extend(v.to_be_bytes()),
                N::O(_) => {
                    slots.push(out.len());
                    out.extend([0, 0]);
                }
                N::O32(_) => {
                    slots.push(out.len());
                    out.extend([0, 0, 0, 0]);
                }
                N::Null => out.extend([0, 0]),
            }
        }
        let mut slot = slots.into_iter();
        for field in &self.0 {
            let (N::O(child) | N::O32(child)) = field else {
                continue;
            };
            let at = slot.next().expect("a slot for every offset");
            let offset = out.len();
            if matches!(field, N::O(_)) {
                out[at..at + 2].copy_from_slice(
                    &u16::try_from(offset)
                        .expect("a 16-bit offset")
                        .to_be_bytes(),
                );
            } else {
                out[at..at + 4].copy_from_slice(
                    &u32::try_from(offset)
                        .expect("a 32-bit offset")
                        .to_be_bytes(),
                );
            }
            out.extend(child.bytes());
        }
        out
    }
}

use N::{I, Null, O, O32, W};

fn words(values: &[u16]) -> impl Iterator<Item = N> + '_ {
    values.iter().map(|&v| W(v))
}

pub(crate) fn coverage(glyphs: &[u16]) -> T {
    let mut sorted = glyphs.to_vec();
    sorted.sort_unstable();
    let mut fields = vec![W(1), W(sorted.len() as u16)];
    fields.extend(words(&sorted));
    T(fields)
}

/// A format 2 coverage of glyph ranges.
fn coverage_ranges(ranges: &[(u16, u16)]) -> T {
    let mut fields = vec![W(2), W(ranges.len() as u16)];
    let mut index = 0;
    for &(start, end) in ranges {
        fields.extend([W(start), W(end), W(index)]);
        index += end - start + 1;
    }
    T(fields)
}

/// A format 2 ClassDef with one range per glyph.
pub(crate) fn class_def(classes: &[(u16, u16)]) -> T {
    let mut sorted = classes.to_vec();
    sorted.sort_unstable();
    let mut fields = vec![W(2), W(sorted.len() as u16)];
    for (glyph, class) in sorted {
        fields.extend([W(glyph), W(glyph), W(class)]);
    }
    T(fields)
}

/// A GSUB or GPOS table: one script (with a default language system holding every feature,
/// and the given language systems), the features, and the lookups.
pub(crate) fn layout_table(
    script: [u8; 4],
    languages: &[([u8; 4], u16, &[u16])],
    features: &[([u8; 4], &[u16])],
    lookups: Vec<T>,
) -> Vec<u8> {
    layout_table_with(script, languages, features, lookups, None)
}

/// A layout table as [`layout_table`], at version 1.1 when it has feature variations.
fn layout_table_with(
    script: [u8; 4],
    languages: &[([u8; 4], u16, &[u16])],
    features: &[([u8; 4], &[u16])],
    lookups: Vec<T>,
    variations: Option<T>,
) -> Vec<u8> {
    let tag = |t: [u8; 4]| {
        [
            W(u16::from_be_bytes([t[0], t[1]])),
            W(u16::from_be_bytes([t[2], t[3]])),
        ]
    };
    let lang_sys = |required: u16, indices: &[u16]| {
        let mut fields = vec![Null, W(required), W(indices.len() as u16)];
        fields.extend(words(indices));
        T(fields)
    };
    let all: Vec<u16> = (0..features.len() as u16).collect();
    let mut script_table = vec![O(lang_sys(0xFFFF, &all)), W(languages.len() as u16)];
    for &(language, required, indices) in languages {
        script_table.extend(tag(language));
        script_table.push(O(lang_sys(required, indices)));
    }
    let mut script_list = vec![W(1)];
    script_list.extend(tag(script));
    script_list.push(O(T(script_table)));
    let mut feature_list = vec![W(features.len() as u16)];
    for &(feature, indices) in features {
        feature_list.extend(tag(feature));
        let mut fields = vec![Null, W(indices.len() as u16)];
        fields.extend(words(indices));
        feature_list.push(O(T(fields)));
    }
    let mut lookup_list = vec![W(lookups.len() as u16)];
    lookup_list.extend(lookups.into_iter().map(O));
    let mut header = vec![
        W(1),
        W(u16::from(variations.is_some())),
        O(T(script_list)),
        O(T(feature_list)),
        O(T(lookup_list)),
    ];
    header.extend(variations.map(O32));
    T(header).bytes()
}

/// A table under `DFLT` whose lookups each belong to the feature tagged at the same index;
/// lookups that share a tag share one feature (only a language system's first feature with a
/// tag counts).
pub(crate) fn simple_table(tags: &[[u8; 4]], lookups: Vec<T>) -> Vec<u8> {
    let mut grouped: Vec<([u8; 4], Vec<u16>)> = Vec::new();
    for (i, &tag) in (0u16..).zip(tags) {
        match grouped.iter_mut().find(|(t, _)| *t == tag) {
            Some((_, indices)) => indices.push(i),
            None => grouped.push((tag, vec![i])),
        }
    }
    let features: Vec<([u8; 4], &[u16])> = grouped.iter().map(|(t, i)| (*t, &i[..])).collect();
    layout_table(*b"DFLT", &[], &features, lookups)
}

pub(crate) fn lookup(kind: u16, flags: u16, subtables: Vec<T>) -> T {
    let mut fields = vec![W(kind), W(flags), W(subtables.len() as u16)];
    fields.extend(subtables.into_iter().map(O));
    T(fields)
}

fn lookup_filtered(kind: u16, subtables: Vec<T>, set: u16) -> T {
    let mut table = lookup(kind, USE_MARK_FILTERING_SET, subtables);
    table.0.push(W(set));
    table
}

fn extension(kind: u16, subtable: T) -> T {
    T(vec![W(1), W(kind), O32(subtable)])
}

/// A GDEF table: glyph classes, mark attachment classes and mark glyph sets.
pub(crate) fn gdef(
    classes: &[(u16, u16)],
    mark_classes: &[(u16, u16)],
    sets: &[&[u16]],
) -> Vec<u8> {
    let optional = |c: &[(u16, u16)]| if c.is_empty() { Null } else { O(class_def(c)) };
    let mark_sets = if sets.is_empty() {
        Null
    } else {
        let mut fields = vec![W(1), W(sets.len() as u16)];
        fields.extend(sets.iter().map(|s| O32(coverage(s))));
        O(T(fields))
    };
    T(vec![
        W(1),
        W(2),
        optional(classes),
        Null,
        Null,
        optional(mark_classes),
        mark_sets,
    ])
    .bytes()
}

// GSUB subtables.

pub(crate) fn single(map: &[(u16, u16)]) -> T {
    let mut map = map.to_vec();
    map.sort_unstable();
    let from: Vec<u16> = map.iter().map(|m| m.0).collect();
    let mut fields = vec![W(2), O(coverage(&from)), W(map.len() as u16)];
    fields.extend(map.iter().map(|m| W(m.1)));
    T(fields)
}

fn sequences(format_one: bool, map: &[(u16, &[u16])]) -> T {
    let mut map = map.to_vec();
    map.sort_unstable();
    let from: Vec<u16> = map.iter().map(|m| m.0).collect();
    let mut fields = vec![
        W(u16::from(format_one)),
        O(coverage(&from)),
        W(map.len() as u16),
    ];
    for (_, glyphs) in map {
        let mut sequence = vec![W(glyphs.len() as u16)];
        sequence.extend(words(glyphs));
        fields.push(O(T(sequence)));
    }
    T(fields)
}

pub(crate) fn multiple(map: &[(u16, &[u16])]) -> T {
    sequences(true, map)
}

/// An alternate substitution subtable, laid out as a multiple substitution one.
pub(crate) fn alternates(map: &[(u16, &[u16])]) -> T {
    sequences(true, map)
}

pub(crate) fn ligatures(ligatures: &[(&[u16], u16)]) -> T {
    let mut firsts: Vec<u16> = ligatures.iter().map(|l| l.0[0]).collect();
    firsts.sort_unstable();
    firsts.dedup();
    let mut fields = vec![W(1), O(coverage(&firsts)), W(firsts.len() as u16)];
    for first in firsts {
        let set: Vec<N> = ligatures
            .iter()
            .filter(|l| l.0[0] == first)
            .map(|(components, glyph)| {
                let mut lig = vec![W(*glyph), W(components.len() as u16)];
                lig.extend(words(&components[1..]));
                O(T(lig))
            })
            .collect();
        let mut set_fields = vec![W(set.len() as u16)];
        set_fields.extend(set);
        fields.push(O(T(set_fields)));
    }
    T(fields)
}

/// A format 3 chained context.
pub(crate) fn chain(
    back: &[&[u16]],
    input: &[&[u16]],
    ahead: &[&[u16]],
    records: &[(u16, u16)],
) -> T {
    let mut fields = vec![W(3)];
    for list in [back, input, ahead] {
        fields.push(W(list.len() as u16));
        fields.extend(list.iter().map(|g| O(coverage(g))));
    }
    fields.push(W(records.len() as u16));
    for &(sequence, lookup) in records {
        fields.extend([W(sequence), W(lookup)]);
    }
    T(fields)
}

// GPOS subtables.

fn anchor(x: i16, y: i16) -> T {
    T(vec![W(1), I(x), I(y)])
}

fn anchors(row: &[Option<(i16, i16)>]) -> impl Iterator<Item = N> + '_ {
    row.iter().map(|a| a.map_or(Null, |(x, y)| O(anchor(x, y))))
}

pub(crate) fn pair(pairs: &[(u16, u16, i16)]) -> T {
    let mut firsts: Vec<u16> = pairs.iter().map(|p| p.0).collect();
    firsts.sort_unstable();
    firsts.dedup();
    let mut fields = vec![
        W(1),
        O(coverage(&firsts)),
        W(4),
        W(0),
        W(firsts.len() as u16),
    ];
    for first in firsts {
        let mut set: Vec<(u16, i16)> = pairs
            .iter()
            .filter(|p| p.0 == first)
            .map(|p| (p.1, p.2))
            .collect();
        set.sort_unstable();
        let mut set_fields = vec![W(set.len() as u16)];
        for (second, advance) in set {
            set_fields.extend([W(second), I(advance)]);
        }
        fields.push(O(T(set_fields)));
    }
    T(fields)
}

/// A cursive attachment subtable (lookup type 3): each glyph with its entry and exit anchors.
pub(crate) fn cursive(glyphs: &[EntryExit]) -> T {
    let mut sorted = glyphs.to_vec();
    sorted.sort_unstable_by_key(|g| g.0);
    let covered: Vec<u16> = sorted.iter().map(|g| g.0).collect();
    let mut fields = vec![W(1), O(coverage(&covered)), W(sorted.len() as u16)];
    for (_, entry, exit) in sorted {
        fields.extend(anchors(&[entry, exit]));
    }
    T(fields)
}

/// A glyph with its cursive entry and exit anchors.
pub(crate) type EntryExit = (u16, Option<(i16, i16)>, Option<(i16, i16)>);

fn mark_array(marks: &[(u16, u16, (i16, i16))]) -> T {
    let mut fields = vec![W(marks.len() as u16)];
    for &(_, class, (x, y)) in marks {
        fields.extend([W(class), O(anchor(x, y))]);
    }
    T(fields)
}

/// Mark-to-base (lookup type 4) or mark-to-mark (6): marks sorted by glyph, with class and
/// anchor; bases sorted by glyph, with an anchor per class.
/// A glyph with an anchor (or none) for each mark class.
pub(crate) type Anchors = (u16, Vec<Option<(i16, i16)>>);
/// A ligature glyph with anchors for each of its components.
pub(crate) type LigatureAnchors = (u16, Vec<Vec<Option<(i16, i16)>>>);

pub(crate) fn mark_attach(marks: &[(u16, u16, (i16, i16))], bases: &[Anchors], classes: u16) -> T {
    let mark_glyphs: Vec<u16> = marks.iter().map(|m| m.0).collect();
    let base_glyphs: Vec<u16> = bases.iter().map(|b| b.0).collect();
    let mut base_array = vec![W(bases.len() as u16)];
    for (_, row) in bases {
        base_array.extend(anchors(row));
    }
    T(vec![
        W(1),
        O(coverage(&mark_glyphs)),
        O(coverage(&base_glyphs)),
        W(classes),
        O(mark_array(marks)),
        O(T(base_array)),
    ])
}

pub(crate) fn mark_to_ligature(
    marks: &[(u16, u16, (i16, i16))],
    ligatures: &[LigatureAnchors],
    classes: u16,
) -> T {
    let mark_glyphs: Vec<u16> = marks.iter().map(|m| m.0).collect();
    let lig_glyphs: Vec<u16> = ligatures.iter().map(|l| l.0).collect();
    let mut lig_array = vec![W(ligatures.len() as u16)];
    for (_, components) in ligatures {
        let mut attach = vec![W(components.len() as u16)];
        for row in components {
            attach.extend(anchors(row));
        }
        lig_array.push(O(T(attach)));
    }
    T(vec![
        W(1),
        O(coverage(&mark_glyphs)),
        O(coverage(&lig_glyphs)),
        W(classes),
        O(mark_array(marks)),
        O(T(lig_array)),
    ])
}

// ---- Applying ----

const TEST: [u8; 4] = *b"test";

fn buffer(gdef: &Gdef<'_>, glyphs: &[u16]) -> Vec<GlyphInfo> {
    glyphs
        .iter()
        .enumerate()
        .map(|(i, &glyph)| {
            let mut info = GlyphInfo {
                glyph,
                cluster: i,
                mask: GLOBAL_BIT,
                ..GlyphInfo::default()
            };
            gdef.classify(&mut info, BASE);
            info
        })
        .collect()
}

fn all_features(table: &[u8]) -> Vec<([u8; 4], u32)> {
    let list = usize::from(u16_at(table, 6).expect("a feature list"));
    (0..usize::from(u16_at(table, list).expect("a feature count")))
        .map(|i| {
            (
                table[list + 2 + 6 * i..list + 6 + 6 * i]
                    .try_into()
                    .expect("a tag"),
                1,
            )
        })
        .collect()
}

fn run_gsub(table: &[u8], gdef: &[u8], info: &mut Vec<GlyphInfo>) -> Limits {
    let tables = Tables {
        gdef: Gdef::new(gdef),
        gsub: Some(table),
        gpos: None,
        kern: None,
        cache: None,
    };
    let lookups = select(table, true, &[], None, &all_features(table)).gsub;
    let mut limits = Limits::new(info.len());
    substitute(&tables, &lookups, info, &mut limits);
    limits
}

fn gsub(table: &[u8], gdef_table: &[u8], glyphs: &[u16]) -> Vec<GlyphInfo> {
    let mut info = buffer(&Gdef::new(gdef_table), glyphs);
    run_gsub(table, gdef_table, &mut info);
    info
}

fn glyphs(info: &[GlyphInfo]) -> Vec<u16> {
    info.iter().map(|g| g.glyph).collect()
}

fn clusters(info: &[GlyphInfo]) -> Vec<usize> {
    info.iter().map(|g| g.cluster).collect()
}

/// Positions `glyphs` with the given advances; (x advance, x offset, y offset) per glyph,
/// attachments resolved.
fn gpos(
    table: &[u8],
    gdef_table: &[u8],
    info: &mut Vec<GlyphInfo>,
    advances: &[i32],
) -> Vec<(i32, i32, i32)> {
    let tables = Tables {
        gdef: Gdef::new(gdef_table),
        gsub: None,
        gpos: Some(table),
        kern: None,
        cache: None,
    };
    let lookups = select(table, true, &[], None, &all_features(table)).gsub;
    let mut pos: Vec<Position> = advances
        .iter()
        .map(|&a| Position::with_advance(a))
        .collect();
    let mut limits = Limits::new(info.len());
    position(&tables, &lookups, info, &mut pos, &mut limits, false);
    propagate_attachments(&mut pos, false);
    pos.iter()
        .map(|p| (p.x_advance, p.x_offset, p.y_offset))
        .collect()
}

fn gpos_glyphs(
    table: &[u8],
    gdef_table: &[u8],
    glyph_ids: &[u16],
    advances: &[i32],
) -> Vec<(i32, i32, i32)> {
    let mut info = buffer(&Gdef::new(gdef_table), glyph_ids);
    gpos(table, gdef_table, &mut info, advances)
}

// ---- Feature selection ----

#[test]
fn features_pick_lookups_by_script_language_and_value() {
    let table = layout_table(
        *b"latn",
        &[(*b"TRK ", 2, &[1, 3])],
        &[
            (*b"liga", &[1]),
            (*b"locl", &[0]),
            (*b"kern", &[2]),
            (*b"liga", &[3, 1]),
        ],
        (0..4)
            .map(|_| lookup(1, 0, vec![single(&[(1, 2)])]))
            .collect(),
    );
    let on = |tags: &[&[u8; 4]]| -> Vec<([u8; 4], u32)> { tags.iter().map(|&&t| (t, 1)).collect() };
    let all = on(&[b"liga", b"locl", b"kern"]);
    let pick_in = |gsub: bool,
                   scripts: &[[u8; 4]],
                   language: Option<[u8; 4]>,
                   features: &[([u8; 4], u32)]| {
        let plan = select(&table, gsub, scripts, language, features);
        let lookups = if gsub { &plan.gsub } else { &plan.gpos };
        // The value each lookup sees: a glyph's mask bits under the lookup's.
        lookups
            .0
            .iter()
            .map(|s| {
                (
                    s.index,
                    (s.mask & plan.global_mask) >> s.mask.trailing_zeros(),
                )
            })
            .collect::<Vec<_>>()
    };
    let pick = |scripts: &[[u8; 4]], language: Option<[u8; 4]>, features: &[([u8; 4], u32)]| {
        pick_in(false, scripts, language, features)
    };
    // The default language system lists every feature, but as in HarfBuzz only its first
    // `liga` counts; lookups come in lookup order, once.
    assert_eq!(pick(&[*b"latn"], None, &all), [(0, 1), (1, 1), (2, 1)]);
    // Turkish: its own features, and its required feature even when not asked for. In GSUB
    // a required feature nothing asked for goes in the first stage, before the rest, as in
    // HarfBuzz.
    assert_eq!(
        pick_in(true, &[*b"latn"], Some(*b"TRK "), &on(&[b"locl"])),
        [(2, 1), (0, 1)]
    );
    assert_eq!(
        pick_in(true, &[*b"latn"], Some(*b"TRK "), &on(&[b"kern", b"locl"])),
        [(0, 1), (2, 1)]
    );
    assert_eq!(
        pick(&[*b"latn"], Some(*b"TRK "), &on(&[b"locl"])),
        [(0, 1), (2, 1)]
    );
    assert_eq!(
        pick(&[*b"latn"], Some(*b"TRK "), &on(&[b"liga"])),
        [(1, 1), (2, 1), (3, 1)]
    );
    // A language the font lacks gets the default system; a script it lacks falls back to
    // latn after DFLT and dflt.
    assert_eq!(pick(&[*b"latn"], Some(*b"JAN "), &on(&[b"kern"])), [(2, 1)]);
    assert_eq!(pick(&[*b"cyrl"], None, &on(&[b"kern"])), [(2, 1)]);
    // The last setting of a tag wins (HarfBuzz puts the caller's after its defaults), 0
    // turns it off, and the value reaches the lookup.
    assert_eq!(pick(&[], None, &[(*b"liga", 1), (*b"liga", 0)]), []);
    assert_eq!(pick(&[], None, &[(*b"liga", 0), (*b"liga", 1)]), [(1, 1)]);
    assert_eq!(pick(&[], None, &[(*b"locl", 3)]), [(0, 3)]);
    // A malformed table selects nothing.
    assert_eq!(select(&table[..12], true, &[], None, &all).gsub.0, []);
    assert_eq!(select(&[], true, &[], None, &all).gsub.0, []);
}

// ---- Substitution ----

#[test]
fn single_substitutions() {
    let delta = T(vec![W(1), O(coverage(&[1, 0xFFFF])), I(2)]);
    let table = simple_table(
        &[TEST, TEST],
        vec![
            lookup(1, 0, vec![delta]),
            lookup(1, 0, vec![single(&[(3, 7)])]),
        ],
    );
    // The delta wraps around 65536; glyphs the coverage leaves out are untouched. The
    // second lookup then turns the 3 into 7.
    assert_eq!(glyphs(&gsub(&table, &[], &[1, 0xFFFF, 2])), [7, 1, 2]);
    // Lookups apply in order over the whole buffer: 1 becomes 3, then 3 becomes 7.
    assert_eq!(glyphs(&gsub(&table, &[], &[1, 3])), [7, 7]);
}

#[test]
fn feature_variations_apply_at_the_default_instance() {
    // A record whose single condition (axis 0 within [min, max]) swaps feature 0 for one
    // that names `lookup`.
    let record = |min: i16, max: i16, lookup: u16| {
        let condition = T(vec![W(1), W(0), I(min), I(max)]);
        let feature = T(vec![Null, W(1), W(lookup)]);
        [
            O32(T(vec![W(1), O32(condition)])),
            O32(T(vec![W(1), W(0), W(1), W(0), O32(feature)])),
        ]
    };
    let lookups = || {
        vec![
            lookup(1, 0, vec![single(&[(1, 5)])]),
            lookup(1, 0, vec![single(&[(1, 6)])]),
            lookup(1, 0, vec![single(&[(1, 7)])]),
        ]
    };
    let table = |records: Vec<[N; 2]>| {
        let mut variations = vec![W(1), W(0), W(0), W(records.len() as u16)];
        variations.extend(records.into_iter().flatten());
        layout_table_with(
            *b"DFLT",
            &[],
            &[(TEST, &[0])],
            lookups(),
            Some(T(variations)),
        )
    };
    // The first record holding at the default instance (every axis at 0) wins; one that
    // needs the axis moved does not.
    let both = table(vec![record(0x2000, 0x4000, 1), record(-0x4000, 0, 2)]);
    assert_eq!(glyphs(&gsub(&both, &[], &[1])), [7]);
    let moved = table(vec![record(0x2000, 0x4000, 1)]);
    assert_eq!(glyphs(&gsub(&moved, &[], &[1])), [5]);
}

#[test]
fn multiple_substitutions_keep_the_cluster() {
    let table = simple_table(
        &[TEST],
        vec![lookup(2, 0, vec![multiple(&[(1, &[4, 5, 6]), (2, &[])])])],
    );
    let info = gsub(&table, &[], &[3, 1, 3]);
    assert_eq!(glyphs(&info), [3, 4, 5, 6, 3]);
    assert_eq!(clusters(&info), [0, 1, 1, 1, 2]);
    // An empty sequence deletes the glyph; a cluster with nothing before it passes on.
    let info = gsub(&table, &[], &[2, 3, 2, 3]);
    assert_eq!(glyphs(&info), [3, 3]);
    assert_eq!(clusters(&info), [0, 3]);
}

#[test]
fn alternates_follow_the_feature_value() {
    let alternates = sequences(true, &[(1, &[10, 11, 12])]);
    let table = simple_table(&[*b"salt"], vec![lookup(3, 0, vec![alternates])]);
    let run = |value: u32| {
        let tables = Tables {
            gsub: Some(&table[..]),
            ..Tables::default()
        };
        let mut info = buffer(&Gdef::default(), &[1]);
        let plan = select(&table, true, &[], None, &[(*b"salt", value)]);
        info[0].mask = plan.global_mask;
        substitute(&tables, &plan.gsub, &mut info, &mut Limits::new(1));
        info[0].glyph
    };
    assert_eq!([run(1), run(2), run(3), run(4)], [10, 11, 12, 1]);
}

#[test]
fn ligatures_merge_clusters_and_remember_marks() {
    // f=1 i=2 l=3, ffi=10 fi=11; acute=20 is a mark; ZWNJ=30 and ZWJ=31.
    let table = simple_table(
        &[*b"liga"],
        vec![lookup(
            4,
            IGNORE_MARKS,
            vec![ligatures(&[(&[1, 1, 2], 10), (&[1, 2], 11)])],
        )],
    );
    let gdef_table = gdef(&[(1, 1), (2, 1), (10, 2), (11, 2), (20, 3)], &[], &[]);
    let info = gsub(&table, &gdef_table, &[1, 1, 2, 3, 1, 2]);
    assert_eq!(glyphs(&info), [10, 3, 11]);
    assert_eq!(clusters(&info), [0, 3, 4]);
    assert_eq!(info[0].class, LIGATURE);
    assert_eq!((info[0].components(), info[2].components()), (3, 2));
    assert_ne!(info[0].lig_id(), info[2].lig_id());

    // A mark between the components stays, after the ligature, and records the component
    // it followed.
    let info = gsub(&table, &gdef_table, &[1, 20, 2, 20]);
    assert_eq!(glyphs(&info), [11, 20, 20]);
    assert_eq!(clusters(&info), [0, 0, 3]);
    assert_eq!(
        (info[1].lig_id(), info[1].lig_comp()),
        (info[0].lig_id(), 1)
    );
    assert_eq!(info[2].lig_id(), 0);

    // ZERO WIDTH NON-JOINER breaks a ligature; ZERO WIDTH JOINER does not.
    let mut joiners = buffer(&Gdef::new(&gdef_table), &[1, 30, 2, 1, 31, 2]);
    joiners[1].ignorable = true;
    joiners[1].zwnj = true;
    joiners[4].ignorable = true;
    run_gsub(&table, &gdef_table, &mut joiners);
    assert_eq!(glyphs(&joiners), [1, 30, 2, 11, 31]);
}

#[test]
fn ligatures_of_ligatures_renumber_their_marks() {
    // f i forms 11 first, with a mark on i; then 1 + 11 forms 12. The mark followed the
    // second component of 11, which is the third of 12.
    let table = simple_table(
        &[*b"liga", *b"liga"],
        vec![
            lookup(4, IGNORE_MARKS, vec![ligatures(&[(&[1, 2], 11)])]),
            lookup(4, IGNORE_MARKS, vec![ligatures(&[(&[3, 11], 12)])]),
        ],
    );
    let gdef_table = gdef(
        &[(1, 1), (2, 1), (3, 1), (11, 2), (12, 2), (20, 3)],
        &[],
        &[],
    );
    let info = gsub(&table, &gdef_table, &[3, 1, 20, 2, 20]);
    assert_eq!(glyphs(&info), [12, 20, 20]);
    assert_eq!(info[0].components(), 3);
    assert_eq!(
        (info[1].lig_id(), info[1].lig_comp()),
        (info[0].lig_id(), 2)
    );
    assert_eq!(info[2].lig_id(), 0);
}

#[test]
fn contexts_apply_nested_lookups_at_shifting_positions() {
    // In "1 2 3" followed by 4: ligate 1 2 into 9, then substitute what was 3 (now at
    // match position 1) with 7.
    let table = simple_table(
        &[TEST],
        vec![
            lookup(
                6,
                0,
                vec![chain(
                    &[&[5]],
                    &[&[1], &[2], &[3]],
                    &[&[4]],
                    &[(0, 1), (1, 2)],
                )],
            ),
            lookup(4, 0, vec![ligatures(&[(&[1, 2], 9)])]),
            lookup(1, 0, vec![single(&[(3, 7)])]),
        ],
    );
    assert_eq!(glyphs(&gsub(&table, &[], &[5, 1, 2, 3, 4])), [5, 9, 7, 4]);
    // Without the backtrack or lookahead nothing happens.
    assert_eq!(glyphs(&gsub(&table, &[], &[1, 2, 3, 4])), [1, 2, 3, 4]);
    assert_eq!(glyphs(&gsub(&table, &[], &[5, 1, 2, 3])), [5, 1, 2, 3]);

    // Formats 1 and 2 match glyphs and classes.
    let format1 = T(vec![
        W(1),
        O(coverage(&[1])),
        W(1),
        O(T(vec![W(1), O(T(vec![W(2), W(1), W(2), W(1), W(1)]))])),
    ]);
    let format2 = T(vec![
        W(2),
        O(coverage(&[1, 3])),
        O(class_def(&[(1, 1), (3, 1), (2, 2), (4, 2)])),
        W(2),
        Null,
        O(T(vec![W(1), O(T(vec![W(2), W(1), W(2), W(1), W(1)]))])),
    ]);
    // Format 1 matches glyph 1 then glyph 2; format 2 matches class 1 ({1, 3}) then class 2
    // ({2, 4}). Either way the second glyph becomes 8.
    for (context, expected) in [(format1, [1, 8, 3, 4, 2]), (format2, [1, 8, 3, 8, 2])] {
        let nested = lookup(1, 0, vec![single(&[(2, 8), (4, 8)])]);
        let table = simple_table(&[TEST], vec![lookup(5, 0, vec![context]), nested]);
        assert_eq!(glyphs(&gsub(&table, &[], &[1, 2, 3, 4, 2])), expected);
    }
}

#[test]
fn reverse_chaining_runs_from_the_end() {
    // 1 becomes 2 when 1 follows; from the end, the last 1 stays, the middle one changes, and
    // then the first one no longer has a 1 after it.
    let reverse = T(vec![
        W(1),
        O(coverage(&[1])),
        W(0),
        W(1),
        O(coverage(&[1])),
        W(1),
        W(2),
    ]);
    let table = simple_table(&[TEST], vec![lookup(8, 0, vec![reverse])]);
    assert_eq!(glyphs(&gsub(&table, &[], &[1, 1, 1])), [1, 2, 1]);
}

#[test]
fn extension_lookups_reach_their_subtables() {
    let table = simple_table(
        &[TEST],
        vec![lookup(7, 0, vec![extension(1, single(&[(1, 5)]))])],
    );
    assert_eq!(glyphs(&gsub(&table, &[], &[1, 2])), [5, 2]);
    // An extension may not point to another extension.
    let nested = simple_table(
        &[TEST],
        vec![lookup(
            7,
            0,
            vec![extension(7, extension(1, single(&[(1, 5)])))],
        )],
    );
    assert_eq!(glyphs(&gsub(&nested, &[], &[1])), [1]);
}

/// A mark glyph set whose coverage offset points two bytes short of 2³²: on a 32-bit
/// target, reading the coverage there must fail rather than overflow. The set then holds
/// nothing, so the mark is passed over.
#[test]
fn mark_set_offsets_near_the_address_limit_fail() {
    let mut gdef_table = gdef(&[(1, 1), (2, 1), (20, 3)], &[], &[&[20]]);
    let sets = usize::from(u16_at(&gdef_table, 12).expect("a mark set table"));
    let far = 0xFFFF_FFFE_u32 - u32::try_from(sets).expect("a small offset");
    gdef_table[sets + 4..sets + 8].copy_from_slice(&far.to_be_bytes());
    let lig = ligatures(&[(&[1, 2], 9)]);
    let filtered = simple_table(&[TEST], vec![lookup_filtered(4, vec![lig], 0)]);
    assert_eq!(glyphs(&gsub(&filtered, &gdef_table, &[1, 20, 2])), [9, 20]);
}

/// A class pair record whose position the class counts put just short of 2³²: on a 32-bit
/// target, reading the second value after it must fail rather than overflow (a panic in
/// this debug build).
#[test]
fn pair_records_near_the_address_limit_fail() {
    // Value formats: device tables only (8 bytes, nothing read), then nothing.
    let table = |first: u16, second: u16| {
        let subtable = T(vec![
            W(2),
            O(coverage(&[1])),
            W(0x00F0),
            W(0),
            O(class_def(&[(1, first)])),
            O(class_def(&[(2, second)])),
            W(32768),
            W(32768),
        ]);
        simple_table(&[TEST], vec![lookup(2, 0, vec![subtable])])
    };
    let probe = table(1, 1);
    let at = probe
        .windows(8)
        .position(|w| w[0..2] == [0, 2] && w[4..8] == [0, 0xF0, 0, 0])
        .expect("the subtable");
    // The record is at + 16 + 8k; put it within 8 bytes of 2³², so 8 bytes on overflow.
    let k = (u64::from(u32::MAX) - (at as u64 + 16)) / 8;
    let (first, second) = (k / 32768, k % 32768);
    let hostile = table(
        u16::try_from(first).expect("a class"),
        u16::try_from(second).expect("a class"),
    );
    assert_eq!(hostile.len(), probe.len());
    let gdef_table = gdef(&[(1, 1), (2, 1)], &[], &[]);
    let positioned = gpos_glyphs(&hostile, &gdef_table, &[1, 2], &[500, 500]);
    assert_eq!(positioned, [(500, 0, 0), (500, 0, 0)]);
}

#[test]
fn mark_filtering_and_attachment_classes_skip_marks() {
    // Marks 20 and 21: 20 has attachment class 1 and is in set 0; 21 has class 2.
    let gdef_table = gdef(
        &[(1, 1), (2, 1), (20, 3), (21, 3)],
        &[(20, 1), (21, 2)],
        &[&[20]],
    );
    let lig = || ligatures(&[(&[1, 2], 9)]);
    let filtered = simple_table(&[TEST], vec![lookup_filtered(4, vec![lig()], 0)]);
    // Only marks in the set are matched, so 21 is passed over and 20 blocks.
    assert_eq!(glyphs(&gsub(&filtered, &gdef_table, &[1, 21, 2])), [9, 21]);
    assert_eq!(
        glyphs(&gsub(&filtered, &gdef_table, &[1, 20, 2])),
        [1, 20, 2]
    );
    let by_class = simple_table(&[TEST], vec![lookup(4, 0x0100, vec![lig()])]);
    assert_eq!(glyphs(&gsub(&by_class, &gdef_table, &[1, 21, 2])), [9, 21]);
    assert_eq!(
        glyphs(&gsub(&by_class, &gdef_table, &[1, 20, 2])),
        [1, 20, 2]
    );
}

#[test]
fn digest_limits_keep_results() {
    // Past the digest limits a lookup tries every subtable, so it substitutes as before: the
    // first lookup turns 1 into 2 and 2 into 3 (one subtable per glyph), the second 3 into 4.
    let table = simple_table(
        &[TEST, TEST],
        vec![
            lookup(1, 0, vec![single(&[(1, 2)]), single(&[(2, 3)])]),
            lookup(1, 0, vec![single(&[(3, 4)])]),
        ],
    );
    for (work, slots) in [
        (0, 0),
        (DIGEST_WORK, 0),
        (0, DIGEST_SLOTS),
        (DIGEST_WORK - 1, 0),
    ] {
        let cache = Cache::default();
        cache.spent.store(work, Ordering::Relaxed);
        cache.slots.store(slots, Ordering::Relaxed);
        let tables = Tables {
            gsub: Some(&table[..]),
            cache: Some(&cache),
            ..Tables::default()
        };
        let mut info = buffer(&Gdef::default(), &[1, 2, 5]);
        let mut limits = Limits::new(3);
        let lookups = select(&table, true, &[], None, &all_features(&table)).gsub;
        substitute(&tables, &lookups, &mut info, &mut limits);
        assert_eq!(glyphs(&info), [2, 4, 5], "work {work}, slots {slots}");
        assert!(cache.spent.load(Ordering::Relaxed) <= DIGEST_WORK);
        assert!(cache.slots.load(Ordering::Relaxed) <= DIGEST_SLOTS);
    }
}

#[test]
fn ligature_lookups_go_on_after_the_last_component() {
    // HarfBuzz 10.4.0, with `sub a b by ab; sub zwj zwsp by zc;` in one lookup, shapes
    // a ZWJ ZWSP b as ab ZWJ ZWSP: the joiners passed over inside the first ligature are
    // not tried again.
    let classes: Vec<(u16, u16)> = vec![(1, 1), (2, 1), (3, 1), (4, 1)];
    let gdef_table = gdef(&classes, &[], &[]);
    let table = simple_table(
        &[*b"liga"],
        vec![lookup(
            4,
            0,
            vec![ligatures(&[(&[1, 4], 10), (&[2, 3], 11)])],
        )],
    );
    let mut info = buffer(&Gdef::new(&gdef_table), &[1, 2, 3, 4]);
    for g in &mut info[1..3] {
        g.ignorable = true;
    }
    info[1].zwj = true;
    run_gsub(&table, &gdef_table, &mut info);
    let glyphs: Vec<u16> = info.iter().map(|g| g.glyph).collect();
    assert_eq!(glyphs, [10, 2, 3]);
}

#[test]
fn ligatures_charge_the_marks_they_renumber() {
    // x i ligate over the marks between them, then f takes the ligature in: the marks,
    // which now follow the last component, are renumbered for the new ligature. Contexts
    // can repeat that over one run of marks, so it spends the budget.
    let classes: Vec<(u16, u16)> = vec![(1, 1), (2, 1), (3, 1), (5, 3), (10, 2), (11, 2)];
    let gdef_table = gdef(&classes, &[], &[]);
    let table = simple_table(
        &[*b"liga", *b"liga"],
        vec![
            lookup(4, IGNORE_MARKS, vec![ligatures(&[(&[2, 3], 10)])]),
            lookup(4, IGNORE_MARKS, vec![ligatures(&[(&[1, 10], 11)])]),
        ],
    );
    let spent = |marks: usize| {
        let mut glyphs = vec![1, 2];
        glyphs.extend(std::iter::repeat_n(5, marks));
        glyphs.push(3);
        let mut info = buffer(&Gdef::new(&gdef_table), &glyphs);
        let limits = run_gsub(&table, &gdef_table, &mut info);
        assert_eq!(info[0].glyph, 11);
        assert!(info[1..].iter().all(|g| g.lig_id() == info[0].lig_id()));
        Limits::new(glyphs.len()).budget.get() - limits.budget.get()
    };
    // A step a mark each: the first match passes over them, the first ligature's cluster
    // merge covers them, the second lookup visits them, the second merge reaches over them
    // (they share the last component's cluster), and they are renumbered. Without the
    // charges for merging and renumbering, about 3 steps a mark.
    assert!(spent(1000) >= 5 * 1000, "{}", spent(1000));
    assert!(spent(2000) >= 5 * 2000, "{}", spent(2000));
}

#[test]
fn nested_edits_far_apart_spend_the_glyphs_they_move() {
    // A context matches 1 and 2 over a run of marks. Its records grow and shrink one end,
    // then the other, again and again, so each pair of edits moves the marks between the
    // ends twice; a last record marks the near end. The moves pass the budget (4,096 steps
    // a glyph) halfway through, and the records stop there: the last one never runs.
    let classes: Vec<(u16, u16)> = vec![(1, 1), (2, 1), (5, 3), (7, 1)];
    let gdef_table = gdef(&classes, &[], &[]);
    let (expand, shrink, mark) = (1, 2, 3);
    let mut records: Vec<(u16, u16)> = (0..2600)
        .flat_map(|_| [(0, expand), (0, shrink), (1, expand), (1, shrink)])
        .collect();
    records.push((0, mark));
    let table = simple_table(
        &[TEST],
        vec![
            lookup(
                6,
                IGNORE_MARKS,
                vec![chain(&[], &[&[1], &[2]], &[], &records)],
            ),
            lookup(2, 0, vec![multiple(&[(1, &[1, 1]), (2, &[2, 2])])]),
            lookup(4, 0, vec![ligatures(&[(&[1, 1], 1), (&[2, 2], 2)])]),
            lookup(1, 0, vec![single(&[(1, 7)])]),
        ],
    );
    let marks = 2_000;
    let mut glyphs = vec![1];
    glyphs.extend(std::iter::repeat_n(5, marks));
    glyphs.push(2);
    let mut info = buffer(&Gdef::new(&gdef_table), &glyphs);
    let limits = run_gsub(&table, &gdef_table, &mut info);
    assert!(limits.exhausted());
    assert_eq!(info[0].glyph, 1, "the records went on past the budget");
}

#[test]
fn nested_lookups_apply_to_the_glyph_their_record_picks() {
    // A context picks a base before 2, and a mark before 2, and calls lookups that skip
    // bases and marks; HarfBuzz 10.4.0 applies both anyway. A ligature that skips marks
    // still reaches over the mark after the glyph it was called on.
    let gdef_table = gdef(
        &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 3), (6, 3), (7, 1)],
        &[],
        &[],
    );
    let table = simple_table(
        &[TEST],
        vec![
            lookup(
                6,
                0,
                vec![
                    chain(&[], &[&[1]], &[&[2]], &[(0, 1)]),
                    chain(&[], &[&[5]], &[&[2]], &[(0, 2)]),
                    chain(&[], &[&[3]], &[&[5]], &[(0, 3)]),
                ],
            ),
            lookup(1, IGNORE_BASE_GLYPHS, vec![single(&[(1, 7)])]),
            lookup(1, IGNORE_MARKS, vec![single(&[(5, 6)])]),
            lookup(4, IGNORE_MARKS, vec![ligatures(&[(&[3, 4], 7)])]),
        ],
    );
    assert_eq!(glyphs(&gsub(&table, &gdef_table, &[1, 2])), [7, 2]);
    assert_eq!(glyphs(&gsub(&table, &gdef_table, &[5, 2])), [6, 2]);
    assert_eq!(glyphs(&gsub(&table, &gdef_table, &[3, 5, 4])), [7, 5]);
}

#[test]
fn hostile_lookups_stop() {
    // A context that calls itself and grows the buffer each time.
    let table = simple_table(
        &[TEST],
        vec![
            lookup(6, 0, vec![chain(&[], &[&[1]], &[], &[(0, 1), (0, 0)])]),
            lookup(2, 0, vec![multiple(&[(1, &[1, 1])])]),
        ],
    );
    let mut info = buffer(&Gdef::default(), &[1; 8]);
    run_gsub(&table, &[], &mut info);
    assert!(info.len() <= Limits::new(8).max_len, "{}", info.len());
    assert!(info.len() > 8);

    // A lookup with many subtables whose long backtracks match but whose lookaheads fail, on
    // a long buffer: the step budget ends it.
    let back: [&[u16]; 30] = [&[1]; 30];
    let subtables: Vec<T> = (0..200)
        .map(|_| chain(&back, &[&[1]], &[&[3]], &[]))
        .collect();
    let table = simple_table(
        &[TEST; 50],
        (0..50)
            .map(|_| lookup(6, 0, Vec::new()))
            .chain(std::iter::once(lookup(6, 0, subtables)))
            .collect(),
    );
    let mut info = buffer(&Gdef::default(), &[1; 64]);
    let tables = Tables {
        gsub: Some(&table[..]),
        ..Tables::default()
    };
    let mut limits = Limits::new(64);
    substitute(
        &tables,
        &Lookups(vec![Selected::new(50); 40]),
        &mut info,
        &mut limits,
    );
    assert!(limits.exhausted());
}

// ---- Positioning ----

#[test]
fn single_and_pair_adjustments() {
    let single_pos = T(vec![
        W(1),
        O(coverage(&[3])),
        W(0x0F),
        I(5),
        I(6),
        I(7),
        I(8),
    ]);
    let class_pair = T(vec![
        W(2),
        O(coverage_ranges(&[(4, 5)])),
        W(4),
        W(0),
        O(class_def(&[(4, 1), (5, 1)])),
        O(class_def(&[(1, 1)])),
        W(2),
        W(2),
        I(0),
        I(0),
        I(0),
        I(-40),
    ]);
    let gdef_table = gdef(&[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (20, 3)], &[], &[]);
    let table = simple_table(
        &[*b"kern", *b"kern", *b"kern"],
        vec![
            lookup(2, IGNORE_MARKS, vec![pair(&[(1, 2, -50), (2, 1, 30)])]),
            lookup(1, 0, vec![single_pos]),
            lookup(2, 0, vec![class_pair]),
        ],
    );
    let adv = |p: &[(i32, i32, i32)]| p.iter().map(|q| q.0).collect::<Vec<_>>();
    // 1 2 kerns, then 2 1 kerns (the second glyph has no value, so it is matched again).
    assert_eq!(
        adv(&gpos_glyphs(
            &table,
            &gdef_table,
            &[1, 2, 1],
            &[500, 500, 500]
        )),
        [450, 530, 500]
    );
    // A mark the lookup ignores does not break the pair.
    assert_eq!(
        adv(&gpos_glyphs(
            &table,
            &gdef_table,
            &[1, 20, 2],
            &[500, 0, 500]
        )),
        [450, 0, 500]
    );
    assert_eq!(
        gpos_glyphs(&table, &gdef_table, &[3], &[500]),
        [(507, 5, 6)]
    );
    assert_eq!(
        adv(&gpos_glyphs(&table, &gdef_table, &[5, 1, 4, 2], &[500; 4])),
        [460, 500, 500, 500]
    );
}

#[test]
fn marks_attach_to_bases_ligatures_and_marks() {
    // Base 1 (advance 600), mark 20 and mark 21 (advance 0), ligature 9 with two components.
    let gdef_table = gdef(&[(1, 1), (9, 2), (20, 3), (21, 3)], &[], &[]);
    let marks = [(20, 0, (50, 0)), (21, 0, (40, -10))];
    let table = simple_table(
        &[*b"mark", *b"mkmk", *b"mark"],
        vec![
            lookup(
                4,
                0,
                vec![mark_attach(&marks, &[(1, vec![Some((300, 700))])], 1)],
            ),
            lookup(
                6,
                0,
                vec![mark_attach(
                    &[(21, 0, (40, -10))],
                    &[(20, vec![Some((60, 200))])],
                    1,
                )],
            ),
            lookup(
                5,
                0,
                vec![mark_to_ligature(
                    &marks,
                    &[(9, vec![vec![Some((200, 700))], vec![Some((700, 700))]])],
                    1,
                )],
            ),
        ],
    );
    // The mark sits at the base anchor: 300 - 50 from the base's origin, which is 600 behind.
    assert_eq!(
        gpos_glyphs(&table, &gdef_table, &[1, 20], &[600, 0]),
        [(600, 0, 0), (0, -350, 700)]
    );
    // Mark on mark: 21 attaches to 20's mark2 anchor, moving with 20.
    let p = gpos_glyphs(&table, &gdef_table, &[1, 20, 21], &[600, 0, 0]);
    assert_eq!(p[2], (0, -350 + 60 - 40, 700 + 200 + 10));
    // Mark on a ligature: the component the mark followed, or the last one.
    let mut info = buffer(&Gdef::new(&gdef_table), &[9, 20, 20]);
    info[0].set_ligature(1, 2);
    info[1].set_component(1, 1);
    let p = gpos(&table, &gdef_table, &mut info, &[1000, 0, 0]);
    assert_eq!(p[1], (0, 200 - 50 - 1000, 700));
    assert_eq!(p[2], (0, 700 - 50 - 1000, 700));
}

#[test]
fn cursive_attachment_joins_exit_to_entry() {
    let cursive = T(vec![
        W(1),
        O(coverage(&[1, 2])),
        W(2),
        Null,
        O(anchor(500, 100)),
        O(anchor(20, 0)),
        Null,
    ]);
    let table = simple_table(&[*b"curs"], vec![lookup(3, 0, vec![cursive])]);
    // The first glyph's advance ends at its exit; the second starts at its entry and hangs
    // 100 units up, from the first.
    assert_eq!(
        gpos_glyphs(&table, &[], &[1, 2], &[600, 600]),
        [(500, 0, 0), (580, -20, 100)]
    );
}
