//! OpenType layout tables: GDEF, GSUB and GPOS (ADR 0027).
//!
//! The tables are read in place and applied to a buffer of glyphs, following HarfBuzz where
//! the specification leaves room: lookups run over the whole buffer in lookup-list order,
//! default-ignorable characters are passed over when they do not match, and marks remember
//! which ligature component they followed. Every read is bounds-checked, and the work is
//! bounded: the buffer length, the glyphs one rule's input may match, the nesting depth and
//! the steps of shaping one run (which every glyph visited, rule, record and attachment step
//! spends), and per font the reading behind lookup selection and digests. A malformed or
//! hostile font cannot make shaping run away.

mod buffer;
mod gpos;
mod gsub;
mod kern;

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use super::reader::{i16_at, u16_at, u32_at};

use buffer::Buffer;

pub(crate) use gpos::{Position, propagate_attachments};
pub(crate) use kern::Kern;

/// GDEF glyph classes.
pub(crate) const BASE: u8 = 1;
pub(crate) const LIGATURE: u8 = 2;
pub(crate) const MARK: u8 = 3;

/// The Hangul jamo features, as a character's jamo kind (0 for none).
pub(crate) const LJMO: u8 = 1;
pub(crate) const VJMO: u8 = 2;
pub(crate) const TJMO: u8 = 3;

/// HarfBuzz's ligature properties: the ligature id in the top three bits, a flag for the
/// ligature glyph itself, and four bits holding a ligature's component count or, for the
/// glyphs inside it, the 1-based component they follow.
const IS_LIG_BASE: u8 = 0x10;

/// Lookup flags.
const RIGHT_TO_LEFT: u16 = 0x0001;
const IGNORE_BASE_GLYPHS: u16 = 0x0002;
const IGNORE_LIGATURES: u16 = 0x0004;
const IGNORE_MARKS: u16 = 0x0008;
const IGNORE_FLAGS: u16 = IGNORE_BASE_GLYPHS | IGNORE_LIGATURES | IGNORE_MARKS;
const USE_MARK_FILTERING_SET: u16 = 0x0010;
const MARK_ATTACHMENT_TYPE: u16 = 0xFF00;

/// How deep contextual lookups may nest, and how many glyphs one context or ligature may
/// match, as in HarfBuzz.
const MAX_NESTING: u8 = 64;
const MAX_CONTEXT: usize = 64;

/// One glyph of the shaping buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlyphInfo {
    pub(crate) glyph: u16,
    /// Byte offset of the start of the glyph's cluster in the shaped text.
    pub(crate) cluster: usize,
    /// GDEF glyph class: 0 unclassified, then [`BASE`], [`LIGATURE`], [`MARK`], 4 component.
    pub(crate) class: u8,
    /// GDEF mark attachment class.
    pub(crate) mark_class: u8,
    /// The ligature the glyph is or belongs to, as HarfBuzz keeps it (see [`IS_LIG_BASE`]);
    /// read through [`GlyphInfo::lig_id`], [`GlyphInfo::lig_comp`] and
    /// [`GlyphInfo::components`].
    pub(crate) lig_props: u8,
    /// The features that apply to the glyph: it shares a bit with the masks of their lookups.
    pub(crate) mask: u32,
    /// A default-ignorable character that no substitution has replaced: matching passes over
    /// it unless a rule names it, and it is drawn invisible.
    pub(crate) ignorable: bool,
    /// ZERO WIDTH NON-JOINER, which substitution passes over only in backtrack and lookahead,
    /// so it breaks ligatures.
    pub(crate) zwnj: bool,
    /// ZERO WIDTH JOINER, which the `mark` and `mkmk` features do not pass over.
    pub(crate) zwj: bool,
    /// One of several glyphs a multiple substitution put in place of one; marks attach to
    /// the first of them.
    pub(crate) multiplied: bool,
    /// COMBINING GRAPHEME JOINER, Mongolian variation selectors and tags, which substitution
    /// does not pass over.
    pub(crate) hidden: bool,
    /// The character's General_Category is a mark (M*), and its canonical combining class;
    /// fallback mark positioning reads them.
    pub(crate) unicode_mark: bool,
    pub(crate) combining_class: u8,
    /// The character's General_Category is Mn.
    pub(crate) nonspacing_mark: bool,
    /// How to size a space the font has no glyph for (see `shaping`).
    pub(crate) space: u8,
}

impl GlyphInfo {
    /// The ligature the glyph is or belongs to (0 for none).
    pub(crate) fn lig_id(&self) -> u8 {
        self.lig_props >> 5
    }

    /// For a glyph inside a ligature, the 1-based component it follows (0 for none, and for
    /// the ligature itself).
    pub(crate) fn lig_comp(&self) -> u8 {
        if self.lig_props & IS_LIG_BASE != 0 {
            0
        } else {
            self.lig_props & 0x0F
        }
    }

    /// How many components the glyph stands for: a ligature's count, kept in four bits (so
    /// 16 reads as 0), or 1.
    pub(crate) fn components(&self) -> u8 {
        if self.class == LIGATURE && self.lig_props & IS_LIG_BASE != 0 {
            self.lig_props & 0x0F
        } else {
            1
        }
    }

    /// Makes the glyph a ligature with `components` components.
    fn set_ligature(&mut self, id: u8, components: usize) {
        self.lig_props = (id << 5) | IS_LIG_BASE | nibble(components);
    }

    /// Makes the glyph follow component `component` of ligature `id`.
    fn set_component(&mut self, id: u8, component: usize) {
        self.lig_props = (id << 5) | nibble(component);
    }
}

/// The low four bits of a count, as HarfBuzz keeps ligature components.
fn nibble(n: usize) -> u8 {
    (n & 0x0F) as u8
}

/// The layout tables of one font.
#[derive(Clone, Copy, Default)]
pub(crate) struct Tables<'a> {
    pub(crate) gdef: Gdef<'a>,
    pub(crate) gsub: Option<&'a [u8]>,
    pub(crate) gpos: Option<&'a [u8]>,
    pub(crate) kern: Option<&'a [u8]>,
    /// What the font keeps between shapings; without it, digests are worked out each time.
    pub(crate) cache: Option<&'a Cache>,
}

/// A filter over glyph ids, after HarfBuzz's set digest: three 64-bit masks, each indexed by
/// a different slice of the id. A glyph it rules out is certainly not in the set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Digest([u64; 3]);

const DIGEST_SHIFTS: [u32; 3] = [0, 4, 9];

/// How many coverage entries a font's digests may read in all; past that, lookups let every
/// glyph through. Lookups can share a coverage, so the reads are bounded per font rather than
/// by the font's size.
const DIGEST_WORK: usize = 1 << 22;

/// How many subtable digests a font keeps (24 bytes each); past that, lookups keep a filter
/// that lets every glyph through.
const DIGEST_SLOTS: usize = 1 << 16;

/// How many records lookup selection may read for one plan: language system entries,
/// lookup indices and FeatureVariations conditions. A font with more gets the lookups read
/// so far.
const PLAN_WORK: usize = 1 << 20;

impl Digest {
    const ALL: Self = Self([u64::MAX; 3]);

    fn add(&mut self, glyph: u16) {
        for (mask, shift) in self.0.iter_mut().zip(DIGEST_SHIFTS) {
            *mask |= 1 << ((u32::from(glyph) >> shift) & 63);
        }
    }

    fn add_range(&mut self, first: u16, last: u16) {
        for (mask, shift) in self.0.iter_mut().zip(DIGEST_SHIFTS) {
            let (a, b) = (u32::from(first) >> shift, u32::from(last) >> shift);
            if b - a >= 63 {
                *mask = u64::MAX;
            } else {
                for v in a..=b {
                    *mask |= 1 << (v & 63);
                }
            }
        }
    }

    pub(crate) fn may_have(&self, glyph: u16) -> bool {
        self.0
            .iter()
            .zip(DIGEST_SHIFTS)
            .all(|(mask, shift)| mask & (1 << ((u32::from(glyph) >> shift) & 63)) != 0)
    }

    fn intersects(&self, other: &Self) -> bool {
        self.0.iter().zip(other.0).all(|(a, b)| a & b != 0)
    }

    fn of_glyphs<'a>(info: impl IntoIterator<Item = &'a GlyphInfo>) -> Self {
        let mut digest = Self::default();
        for g in info {
            digest.add(g.glyph);
        }
        digest
    }

    /// The glyphs a lookup can start matching at: the union of each subtable's first
    /// coverage. Anything it cannot read lets every glyph through.
    fn of_lookup(t: &[u8], lookup: &Lookup, kind: Kind, budget: &Budget<'_>) -> Filter {
        let extension = match kind {
            Kind::Gsub => 7,
            Kind::Gpos => 9,
        };
        if !budget.reserve_slots(lookup.subtables) {
            return Filter::pass_through(lookup.subtables);
        }
        let subtables: Box<[Self]> = lookup
            .subtables(t, extension)
            .map(|subtable| match subtable {
                // A subtable that cannot be read never applies.
                None => Self::default(),
                Some((kind_of, at)) => Self::of_subtable(t, kind, kind_of, at, budget),
            })
            .collect();
        let mut digest = Self::default();
        for d in &subtables {
            for (a, b) in digest.0.iter_mut().zip(d.0) {
                *a |= b;
            }
        }
        Filter {
            digest,
            subtables,
            count: lookup.subtables,
        }
    }

    /// The glyphs a subtable can start matching at: its first coverage.
    fn of_subtable(t: &[u8], kind: Kind, subtable: u16, at: usize, budget: &Budget<'_>) -> Self {
        let context = matches!((kind, subtable), (Kind::Gsub, 5) | (Kind::Gpos, 7));
        let chain = matches!((kind, subtable), (Kind::Gsub, 6) | (Kind::Gpos, 8));
        let slot = match u16_at(t, at) {
            Some(3) if context => (u16_at(t, at + 2).unwrap_or(0) > 0).then_some(at + 6),
            Some(3) if chain => u16_at(t, at + 2).and_then(|backtrack| {
                let input = at + 4 + 2 * usize::from(backtrack);
                (u16_at(t, input)? > 0).then_some(input + 2)
            }),
            Some(_) => Some(at + 2),
            None => None,
        };
        let mut digest = Self::default();
        let coverage = slot.and_then(|s| u16_at(t, s)).filter(|&o| o != 0);
        match coverage.and_then(|o| digest.add_coverage(t, at + usize::from(o), budget)) {
            Some(()) => digest,
            None => Self::ALL,
        }
    }

    fn add_coverage(&mut self, t: &[u8], at: usize, budget: &Budget<'_>) -> Option<()> {
        let count = usize::from(u16_at(t, at + 2)?);
        if !budget.reserve_work(count + 1) {
            return None;
        }
        match u16_at(t, at)? {
            1 => {
                for i in 0..count {
                    self.add(u16_at(t, at + 4 + 2 * i)?);
                }
            }
            2 => {
                for i in 0..count {
                    let (first, last) = (u16_at(t, at + 4 + 6 * i)?, u16_at(t, at + 6 + 6 * i)?);
                    // A range ending before it starts holds nothing.
                    if first <= last {
                        self.add_range(first, last);
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }
}

/// A lookup's digest and those of its subtables, in the order they are tried; without
/// subtable digests, every subtable is tried.
#[derive(Clone, Debug)]
pub(crate) struct Filter {
    digest: Digest,
    subtables: Box<[Digest]>,
    /// How many subtables the lookup has, read or not: what trying them all costs.
    count: usize,
}

impl Filter {
    fn pass_through(count: usize) -> Self {
        Self {
            digest: Digest::ALL,
            subtables: Box::default(),
            count,
        }
    }
}

/// What working out digests may still spend, shared by every thread shaping with a font.
/// Work is reserved before it is done, so threads together stay within the limits.
struct Budget<'a> {
    work: &'a AtomicUsize,
    slots: &'a AtomicUsize,
}

impl Budget<'_> {
    fn reserve(counter: &AtomicUsize, amount: usize, limit: usize) -> bool {
        let mut spent = counter.load(Ordering::Relaxed);
        loop {
            let Some(total) = spent.checked_add(amount).filter(|&total| total <= limit) else {
                return false;
            };
            match counter.compare_exchange_weak(spent, total, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return true,
                Err(now) => spent = now,
            }
        }
    }

    fn reserve_work(&self, amount: usize) -> bool {
        Self::reserve(self.work, amount, DIGEST_WORK)
    }

    fn reserve_slots(&self, amount: usize) -> bool {
        Self::reserve(self.slots, amount, DIGEST_SLOTS)
    }
}

/// What a font keeps between shapings: each lookup's digest, worked out the first time the
/// lookup runs.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    gsub: OnceLock<Box<[OnceLock<Filter>]>>,
    gpos: OnceLock<Box<[OnceLock<Filter>]>>,
    /// Coverage entries and subtables the digests have read, out of [`DIGEST_WORK`].
    spent: AtomicUsize,
    /// Subtable digests kept, out of [`DIGEST_SLOTS`].
    slots: AtomicUsize,
    /// The plans of recent shapings, oldest first.
    plans: Mutex<Vec<Arc<Plan>>>,
}

/// How many plans a font keeps.
const PLANS: usize = 16;

/// What a script, language and feature requests pick from a font: the lookups of each table
/// and where each feature's value sits in a glyph's mask, as HarfBuzz's shape plan.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    scripts: Vec<[u8; 4]>,
    language: Languages,
    requests: Vec<Request>,
    simple: bool,
    pub(crate) gsub: Lookups,
    pub(crate) gpos: Lookups,
    /// GPOS has a `kern` feature for this script and language, and the plan keeps it.
    pub(crate) gpos_kern: bool,
    /// The GPOS script whose language systems the plan uses: one of the requested scripts,
    /// or `DFLT`, `dflt` or `latn`; none if GPOS has none of them.
    pub(crate) gpos_script: Option<[u8; 4]>,
    /// The mask every glyph starts with: the global bit and each global feature's value.
    pub(crate) global_mask: u32,
    features: Vec<Mapped>,
}

impl Plan {
    /// The mask bits of feature `tag`, or 0 if the plan leaves it out.
    pub(crate) fn mask(&self, tag: [u8; 4]) -> u32 {
        self.features
            .iter()
            .find(|f| f.tag == tag)
            .map_or(0, |f| f.mask)
    }

    /// The mask that gives feature `tag` value 1, or 0 if the plan leaves it out.
    pub(crate) fn one_mask(&self, tag: [u8; 4]) -> u32 {
        self.features
            .iter()
            .find(|f| f.tag == tag)
            .map_or(0, |f| f.mask & (1 << f.shift))
    }
}

/// The plan for `requests` under `scripts` and `language`, from the font's recent plans
/// when it has one for them. `simple` says the requests are HarfBuzz's defaults alone, which
/// it keeps in order instead of sorting them by tag (the order allocates mask bits).
pub(crate) fn plan(
    tables: &Tables<'_>,
    scripts: &[[u8; 4]],
    language: Languages,
    requests: &[Request],
    simple: bool,
) -> Arc<Plan> {
    let matches = |p: &Plan| {
        p.scripts == scripts
            && p.language == language
            && p.requests == requests
            && p.simple == simple
    };
    let lock = || {
        tables
            .cache
            .map(|c| c.plans.lock().unwrap_or_else(PoisonError::into_inner))
    };
    if let Some(plans) = lock()
        && let Some(plan) = plans.iter().rev().find(|p| matches(p))
    {
        return plan.clone();
    }
    let plan = Arc::new(build_plan(
        tables.gsub,
        tables.gpos,
        scripts,
        language,
        requests,
        simple,
    ));
    if let Some(mut plans) = lock() {
        if plans.len() == PLANS {
            plans.remove(0);
        }
        plans.push(plan.clone());
    }
    plan
}

impl Cache {
    fn digests(&self, kind: Kind, t: &[u8]) -> &[OnceLock<Filter>] {
        let cell = match kind {
            Kind::Gsub => &self.gsub,
            Kind::Gpos => &self.gpos,
        };
        cell.get_or_init(|| {
            let count = u16_at(t, 8)
                .and_then(|list| u16_at(t, usize::from(list)))
                .unwrap_or(0);
            (0..count).map(|_| OnceLock::new()).collect()
        })
    }
}

/// One lookup a plan applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selected {
    pub(crate) index: u16,
    /// The glyphs it applies to and matches as input: those whose mask shares a bit with
    /// it. Under it, a glyph's mask holds the value of the feature that selected it (an
    /// alternate's 1-based index).
    pub(crate) mask: u32,
    /// Whether input matching passes over ZERO WIDTH JOINER, and context matching over
    /// ZERO WIDTH NON-JOINER (HarfBuzz's automatic joiners); `mark` and `mkmk` handle joiners
    /// themselves.
    pub(crate) auto_zwj: bool,
    pub(crate) auto_zwnj: bool,
    /// Value 255 picks an alternate at random (`rand`).
    pub(crate) random: bool,
}

impl Selected {
    #[cfg(test)]
    pub(crate) fn new(index: u16) -> Self {
        Self {
            index,
            mask: GLOBAL_BIT,
            auto_zwj: true,
            auto_zwnj: true,
            random: false,
        }
    }
}

/// The lookups a plan selects in one table, in the order they apply: in GSUB `rvrn`'s first,
/// then the rest, each stage in lookup-list order, as HarfBuzz stages them (a lookup both
/// stages select applies twice); in GPOS all in lookup-list order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lookups(pub(crate) Vec<Selected>);

/// A feature a shaper asks for, with HarfBuzz's feature flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) tag: [u8; 4],
    /// 0 turns the feature off; above 1, it picks an alternate.
    pub(crate) value: u32,
    /// The feature applies to every glyph; otherwise only to those a shaper gives its mask.
    pub(crate) global: bool,
    /// Input matching does not pass over joiners by itself (`mark` and `mkmk`).
    pub(crate) manual_joiners: bool,
    /// Value 255 picks an alternate at random (`rand`).
    pub(crate) random: bool,
    /// The plan keeps the feature even if the font lacks it, as something else stands in
    /// for it (`kern`, by the legacy table).
    pub(crate) fallback: bool,
}

impl Request {
    /// A feature on every glyph.
    pub(crate) const fn global(tag: [u8; 4], value: u32) -> Self {
        Self {
            tag,
            value,
            global: true,
            manual_joiners: false,
            random: false,
            fallback: false,
        }
    }

    /// A feature on the glyphs a shaper gives its mask.
    pub(crate) const fn masked(tag: [u8; 4]) -> Self {
        Self {
            global: false,
            ..Self::global(tag, 1)
        }
    }
}

/// Where a feature's value sits in a glyph's mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mapped {
    tag: [u8; 4],
    mask: u32,
    shift: u32,
}

/// The mask bit of features with value 1 on every glyph, as HarfBuzz's global bit.
pub(crate) const GLOBAL_BIT: u32 = 1 << GLOBAL_SHIFT;
const GLOBAL_SHIFT: u32 = 31;
/// HarfBuzz keeps glyph flags in the low three bits and allocates feature bits after them,
/// at most eight to a feature.
const FIRST_FEATURE_BIT: u32 = 4;
const MAX_FEATURE_BITS: u32 = 8;

/// A feature in the plan's list after HarfBuzz merges a tag's requests.
struct Wanted {
    tag: [u8; 4],
    global: bool,
    max: u32,
    default: u32,
    manual_joiners: bool,
    random: bool,
    fallback: bool,
    /// The GSUB stage: `rvrn` comes before the rest.
    stage: u8,
}

/// One table's language system: each tag's first feature in it, and its required feature.
#[derive(Default)]
struct LangSys {
    features: HashMap<[u8; 4], u16>,
    required: Option<(u16, Option<[u8; 4]>)>,
}

impl LangSys {
    fn new(t: &[u8], scripts: &[[u8; 4]], language: Languages, spend: &dyn Fn() -> bool) -> Self {
        let read = || -> Option<Self> {
            let lang_sys = lang_sys(t, scripts, language)?;
            let feature_list = usize::from(u16_at(t, 6)?);
            let feature_count = u16_at(t, feature_list)?;
            let tag_of = |feature: u16| -> Option<[u8; 4]> {
                let record = feature_list + 2 + 6 * usize::from(feature);
                (feature < feature_count)
                    .then(|| t.get(record..record + 4)?.try_into().ok())
                    .flatten()
            };
            let mut features = HashMap::new();
            let count = usize::from(u16_at(t, lang_sys + 4)?);
            // Backwards, so that a tag's first feature wins.
            for k in (0..count).rev() {
                if !spend() {
                    break;
                }
                let Some(feature) = u16_at(t, lang_sys + 6 + 2 * k) else {
                    continue;
                };
                if let Some(tag) = tag_of(feature) {
                    features.insert(tag, feature);
                }
            }
            let required = match u16_at(t, lang_sys + 2)? {
                0xFFFF => None,
                feature => Some((feature, tag_of(feature))),
            };
            Some(Self { features, required })
        };
        read().unwrap_or_default()
    }
}

/// Builds a plan as HarfBuzz's map builder: merges each tag's requests (the last global
/// setting wins), gives each feature the font has mask bits for its value, and collects the
/// lookups of each stage in lookup-list order, a lookup that several features select once.
fn build_plan(
    gsub: Option<&[u8]>,
    gpos: Option<&[u8]>,
    scripts: &[[u8; 4]],
    language: Languages,
    requests: &[Request],
    simple: bool,
) -> Plan {
    let work = Cell::new(PLAN_WORK);
    let spend = || {
        let left = work.get();
        work.set(left.saturating_sub(1));
        left > 0
    };
    let mut list: Vec<&Request> = requests.iter().filter(|r| r.tag != [0; 4]).collect();
    if !simple {
        list.sort_by_key(|r| r.tag);
    }
    let mut wanted: Vec<Wanted> = Vec::new();
    for r in list {
        let stage = u8::from(r.tag != *b"rvrn");
        match wanted.last_mut() {
            Some(w) if w.tag == r.tag => {
                if r.global {
                    w.global = true;
                    (w.max, w.default) = (r.value, r.value);
                } else {
                    w.global = false;
                    w.max = w.max.max(r.value);
                }
                w.fallback |= r.fallback;
                w.stage = w.stage.min(stage);
            }
            _ => wanted.push(Wanted {
                tag: r.tag,
                global: r.global,
                max: r.value,
                default: if r.global { r.value } else { 0 },
                manual_joiners: r.manual_joiners,
                random: r.random,
                fallback: r.fallback,
                stage,
            }),
        }
    }

    let tables = [gsub, gpos];
    let systems = tables.map(|t| {
        t.map_or_else(LangSys::default, |t| {
            LangSys::new(t, scripts, language, &spend)
        })
    });
    // Each mapped feature with its feature index in each table and its GSUB stage.
    let mut mapped: Vec<(Mapped, [Option<u16>; 2], u8, &Wanted)> = Vec::new();
    let mut required_stage = [0u8; 2];
    let mut global_mask = GLOBAL_BIT;
    let mut next_bit = FIRST_FEATURE_BIT;
    for w in &wanted {
        let one_bit = w.global && w.max == 1;
        let bits = if one_bit {
            0
        } else {
            (u32::BITS - w.max.leading_zeros()).min(MAX_FEATURE_BITS)
        };
        if w.max == 0 || next_bit + bits >= GLOBAL_SHIFT {
            continue;
        }
        let stages = [w.stage, 0];
        let mut index = [None, None];
        for (k, system) in systems.iter().enumerate() {
            if system.required.is_some_and(|(_, tag)| tag == Some(w.tag)) {
                required_stage[k] = stages[k];
            }
            index[k] = system.features.get(&w.tag).copied();
        }
        if index == [None, None] && !w.fallback {
            continue;
        }
        let (mask, shift) = if one_bit {
            (GLOBAL_BIT, GLOBAL_SHIFT)
        } else {
            let shift = next_bit;
            next_bit += bits;
            let mask = (1 << next_bit) - (1 << shift);
            global_mask |= (w.default << shift) & mask;
            (mask, shift)
        };
        let feature = Mapped {
            tag: w.tag,
            mask,
            shift,
        };
        mapped.push((feature, index, w.stage, w));
    }

    let select = |k: usize, stages: u8| -> Lookups {
        let Some(t) = tables[k] else {
            return Lookups::default();
        };
        let substitutions = feature_substitutions(t, &spend);
        let mut chosen: Vec<Selected> = Vec::new();
        for stage in 0..stages {
            let start = chosen.len();
            if let Some((feature, _)) = systems[k].required
                && required_stage[k] == stage
            {
                add_lookups(
                    t,
                    feature,
                    substitutions,
                    Selected::global(),
                    &spend,
                    &mut chosen,
                );
            }
            for (feature, index, gsub_stage, w) in &mapped {
                let Some(index) = index[k] else { continue };
                if [*gsub_stage, 0][k] != stage {
                    continue;
                }
                let selected = Selected {
                    index: 0,
                    mask: feature.mask,
                    auto_zwj: !w.manual_joiners,
                    auto_zwnj: !w.manual_joiners,
                    random: w.random,
                };
                add_lookups(t, index, substitutions, selected, &spend, &mut chosen);
            }
            // In lookup-list order, a lookup that several features select once: for the
            // glyphs of each, passing over joiners only if each does.
            let mut stage_lookups = chosen.split_off(start);
            stage_lookups.sort_by_key(|s| s.index);
            for s in stage_lookups {
                let fresh = chosen.len() == start;
                match chosen.last_mut() {
                    Some(last) if !fresh && last.index == s.index => {
                        last.mask |= s.mask;
                        last.auto_zwj &= s.auto_zwj;
                        last.auto_zwnj &= s.auto_zwnj;
                    }
                    _ => chosen.push(s),
                }
            }
        }
        Lookups(chosen)
    };
    let gsub_lookups = select(0, 2);
    let gpos_lookups = select(1, 1);
    let gpos_kern = mapped
        .iter()
        .any(|(f, index, _, _)| f.tag == *b"kern" && index[1].is_some());
    Plan {
        scripts: scripts.to_vec(),
        language,
        requests: requests.to_vec(),
        simple,
        gsub: gsub_lookups,
        gpos: gpos_lookups,
        gpos_kern,
        gpos_script: gpos
            .and_then(|t| chosen_script(t, scripts))
            .map(|(tag, _)| tag),
        global_mask,
        features: mapped.into_iter().map(|(f, _, _, _)| f).collect(),
    }
}

impl Selected {
    /// The settings of the required feature's lookups.
    const fn global() -> Self {
        Self {
            index: 0,
            mask: GLOBAL_BIT,
            auto_zwj: true,
            auto_zwnj: true,
            random: false,
        }
    }
}

/// Appends the lookups of feature `feature` (or of the table `substitutions` give it at the
/// font's default instance) with the settings of `selected`.
fn add_lookups(
    t: &[u8],
    feature: u16,
    substitutions: Option<usize>,
    selected: Selected,
    spend: &dyn Fn() -> bool,
    chosen: &mut Vec<Selected>,
) {
    let mut read = || -> Option<()> {
        let feature_list = usize::from(u16_at(t, 6)?);
        if feature >= u16_at(t, feature_list)? {
            return None;
        }
        let lookup_count = u16_at(t, usize::from(u16_at(t, 8)?))?;
        let record = feature_list + 2 + 6 * usize::from(feature);
        let substituted = substitutions.and_then(|s| substitute_feature(t, s, feature, spend));
        let table = match substituted {
            Some(table) => table,
            None => feature_list + usize::from(u16_at(t, record + 4)?),
        };
        let count = usize::from(u16_at(t, table + 2)?);
        for k in 0..count {
            if !spend() {
                break;
            }
            let index = u16_at(t, table + 4 + 2 * k)?;
            if index < lookup_count {
                chosen.push(Selected { index, ..selected });
            }
        }
        Some(())
    };
    read();
}

/// The plan of `features` (tag and value, each on every glyph) for one table, for tests.
#[cfg(test)]
pub(crate) fn select(
    table: &[u8],
    gsub: bool,
    scripts: &[[u8; 4]],
    language: Option<[u8; 4]>,
    features: &[([u8; 4], u32)],
) -> Plan {
    let requests: Vec<Request> = features
        .iter()
        .map(|&(tag, value)| Request::global(tag, value))
        .collect();
    let (s, p) = if gsub {
        (Some(table), None)
    } else {
        (None, Some(table))
    };
    build_plan(s, p, scripts, [language, None], &requests, false)
}

/// OpenType language system tags to try in order, as HarfBuzz gives some languages a
/// fallback (`ZHTM`, then `ZHH ` for Chinese in Macao).
pub(crate) type Languages = [Option<[u8; 4]>; 2];

/// The language system for `scripts` (then `DFLT`, `dflt` and `latn`) and the first of
/// `language` the script has (or the script's default).
fn lang_sys(t: &[u8], scripts: &[[u8; 4]], language: Languages) -> Option<usize> {
    let (_, script) = chosen_script(t, scripts)?;
    language
        .iter()
        .flatten()
        .find_map(|&tag| tagged(t, script + 2, script, tag))
        .or_else(|| match u16_at(t, script)? {
            0 => None,
            offset => Some(script + usize::from(offset)),
        })
}

/// The first of `scripts`, then `DFLT`, `dflt` and `latn`, that the table has, with its
/// Script table.
fn chosen_script(t: &[u8], scripts: &[[u8; 4]]) -> Option<([u8; 4], usize)> {
    if u16_at(t, 0)? != 1 {
        return None;
    }
    let script_list = usize::from(u16_at(t, 4)?);
    scripts
        .iter()
        .chain(&[*b"DFLT", *b"dflt", *b"latn"])
        .find_map(|&tag| Some((tag, tagged(t, script_list, script_list, tag)?)))
}

/// The table at the offset of the record tagged `tag` in a list of (tag, Offset16) records
/// whose count is at `list`; offsets are relative to `base`. A null offset counts as absent.
fn tagged(t: &[u8], list: usize, base: usize, tag: [u8; 4]) -> Option<usize> {
    let count = usize::from(u16_at(t, list)?);
    (0..count).find_map(|i| {
        let record = list + 2 + 6 * i;
        if t.get(record..record + 4)? != tag {
            return None;
        }
        match u16_at(t, record + 4)? {
            0 => None,
            offset => Some(base + usize::from(offset)),
        }
    })
}

/// The feature substitutions a version 1.1 table makes at the font's default instance: the
/// first FeatureVariations record whose conditions hold with every axis at its default.
fn feature_substitutions(t: &[u8], spend: &dyn Fn() -> bool) -> Option<usize> {
    if u16_at(t, 2)? < 1 {
        return None;
    }
    let variations = within(t, 0, u32_at(t, 10)?)?;
    let count = u32_at(t, variations + 4)? as usize;
    for i in 0..count {
        let record = variations + 8 + 8 * i;
        let holds = match u32_at(t, record)? {
            0 => true,
            conditions => {
                let set = within(t, variations, conditions)?;
                let n = usize::from(u16_at(t, set)?);
                (0..n).all(|k| {
                    let condition = u32_at(t, set + 2 + 4 * k).and_then(|o| within(t, set, o));
                    // Format 1 is an axis range; at the default instance every axis is at 0.
                    spend()
                        && condition.is_some_and(|c| {
                            u16_at(t, c) == Some(1)
                                && i16_at(t, c + 4).is_some_and(|min| min <= 0)
                                && i16_at(t, c + 6).is_some_and(|max| max >= 0)
                        })
                })
            }
        };
        if !spend() {
            return None;
        }
        if holds {
            return match u32_at(t, record + 4)? {
                0 => None,
                o => within(t, variations, o),
            };
        }
    }
    None
}

/// `base` plus a 32-bit offset, if that lands inside the table. Positions inside a table
/// leave room for the 16-bit offsets and small multiples added to them, even on 32-bit
/// targets.
fn within(t: &[u8], base: usize, offset: u32) -> Option<usize> {
    base.checked_add(usize::try_from(offset).ok()?)
        .filter(|&at| at < t.len())
}

/// The alternate table a FeatureTableSubstitution gives a feature, if any.
fn substitute_feature(
    t: &[u8],
    substitutions: usize,
    feature: u16,
    spend: &dyn Fn() -> bool,
) -> Option<usize> {
    let count = usize::from(u16_at(t, substitutions + 4)?);
    (0..count).take_while(|_| spend()).find_map(|k| {
        let record = substitutions + 6 + 6 * k;
        (u16_at(t, record)? == feature).then(|| within(t, substitutions, u32_at(t, record + 2)?))?
    })
}

/// The glyph definition table, or an empty one.
#[derive(Clone, Copy, Default)]
pub(crate) struct Gdef<'a> {
    table: &'a [u8],
    classes: usize,
    mark_classes: usize,
    mark_sets: usize,
}

impl<'a> Gdef<'a> {
    pub(crate) fn new(table: &'a [u8]) -> Self {
        if u16_at(table, 0) != Some(1) {
            return Self::default();
        }
        let offset = |at| u16_at(table, at).map_or(0, usize::from);
        Self {
            table,
            classes: offset(4),
            mark_classes: offset(10),
            mark_sets: if u16_at(table, 2).unwrap_or(0) >= 2 {
                offset(12)
            } else {
                0
            },
        }
    }

    fn mark_class(&self, glyph: u16) -> u8 {
        match self.mark_classes {
            0 => 0,
            at => u8::try_from(class(self.table, at, glyph)).unwrap_or(0),
        }
    }

    fn in_mark_set(&self, set: u16, glyph: u16) -> bool {
        let sets = self.mark_sets;
        if sets == 0 || u16_at(self.table, sets) != Some(1) {
            return false;
        }
        if set >= u16_at(self.table, sets + 2).unwrap_or(0) {
            return false;
        }
        u32_at(self.table, sets + 4 + 4 * usize::from(set))
            .and_then(|o| within(self.table, sets, o))
            .and_then(|at| coverage(self.table, at, glyph))
            .is_some()
    }

    /// Sets a glyph's classes; `fallback` is its class when the font has no glyph classes.
    pub(crate) fn classify(&self, info: &mut GlyphInfo, fallback: u8) {
        info.class = match self.classes {
            0 => fallback,
            at => u8::try_from(class(self.table, at, info.glyph))
                .ok()
                .filter(|&c| c <= 4)
                .unwrap_or(0),
        };
        info.mark_class = self.mark_class(info.glyph);
    }
}

/// The coverage index of `glyph` in the Coverage table at `at`.
pub(crate) fn coverage(t: &[u8], at: usize, glyph: u16) -> Option<u16> {
    let count = usize::from(u16_at(t, at + 2)?);
    match u16_at(t, at)? {
        1 => {
            let (mut lo, mut hi) = (0, count);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let g = u16_at(t, at + 4 + 2 * mid)?;
                if g < glyph {
                    lo = mid + 1;
                } else if g > glyph {
                    hi = mid;
                } else {
                    return u16::try_from(mid).ok();
                }
            }
            None
        }
        2 => {
            let r = range(t, at + 4, count, glyph)?;
            u16_at(t, r + 4)?.checked_add(glyph - u16_at(t, r)?)
        }
        _ => None,
    }
}

/// The class of `glyph` in the ClassDef table at `at`; 0 for glyphs it does not list.
pub(crate) fn class(t: &[u8], at: usize, glyph: u16) -> u16 {
    let class = || -> Option<u16> {
        match u16_at(t, at)? {
            1 => {
                let start = u16_at(t, at + 2)?;
                let i = glyph
                    .checked_sub(start)
                    .filter(|&i| i < u16_at(t, at + 4).unwrap_or(0))?;
                u16_at(t, at + 6 + 2 * usize::from(i))
            }
            2 => u16_at(
                t,
                range(t, at + 4, usize::from(u16_at(t, at + 2)?), glyph)? + 4,
            ),
            _ => None,
        }
    };
    class().unwrap_or(0)
}

/// A ClassDef at the Offset16 in `slot` relative to `base`: `Some(None)` for a null offset,
/// which puts every glyph in class 0.
fn class_def(t: &[u8], base: usize, slot: usize) -> Option<Option<usize>> {
    match u16_at(t, slot)? {
        0 => Some(None),
        o => Some(Some(base + usize::from(o))),
    }
}

/// The class of `glyph` in an optional ClassDef.
fn class_in(t: &[u8], def: Option<usize>, glyph: u16) -> u16 {
    def.map_or(0, |at| class(t, at, glyph))
}

/// The (start, end, value) range record holding `glyph`, among `count` sorted records at `at`.
fn range(t: &[u8], at: usize, count: usize, glyph: u16) -> Option<usize> {
    let (mut lo, mut hi) = (0, count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let r = at + 6 * mid;
        if u16_at(t, r + 2)? < glyph {
            lo = mid + 1;
        } else if u16_at(t, r)? > glyph {
            hi = mid;
        } else {
            return Some(r);
        }
    }
    None
}

/// The table at the Offset16 in `slot`, relative to `base`, if `index` is below `count` and the
/// offset is not null.
fn offset(t: &[u8], base: usize, slot: usize, count: u16, index: u16) -> Option<usize> {
    if index >= count {
        return None;
    }
    match u16_at(t, slot)? {
        0 => None,
        o => Some(base + usize::from(o)),
    }
}

/// A lookup's header.
#[derive(Clone, Copy)]
struct Lookup {
    kind: u16,
    flags: u16,
    filter: u16,
    /// The lookup table; subtable offsets are relative to it.
    base: usize,
    subtables: usize,
}

fn lookup(t: &[u8], index: u16) -> Option<Lookup> {
    let list = usize::from(u16_at(t, 8)?);
    if index >= u16_at(t, list)? {
        return None;
    }
    let base = list + usize::from(u16_at(t, list + 2 + 2 * usize::from(index))?);
    let flags = u16_at(t, base + 2)?;
    let subtables = usize::from(u16_at(t, base + 4)?);
    let filter = if flags & USE_MARK_FILTERING_SET != 0 {
        u16_at(t, base + 6 + 2 * subtables)?
    } else {
        0
    };
    Some(Lookup {
        kind: u16_at(t, base)?,
        flags,
        filter,
        base,
        subtables,
    })
}

impl Lookup {
    /// The subtables' types and positions, with extension subtables resolved to the
    /// subtable they point to (`None` for one that cannot be read); `extension` is the
    /// table's extension type (GSUB 7, GPOS 9).
    fn subtables(
        self,
        t: &[u8],
        extension: u16,
    ) -> impl Iterator<Item = Option<(u16, usize)>> + '_ {
        (0..self.subtables).map(move |i| {
            let at = self.base + usize::from(u16_at(t, self.base + 6 + 2 * i)?);
            if self.kind != extension {
                return Some((self.kind, at));
            }
            if u16_at(t, at)? != 1 {
                return None;
            }
            let kind = u16_at(t, at + 2)?;
            let target = within(t, at, u32_at(t, at + 4)?)?;
            (kind != extension).then_some((kind, target))
        })
    }
}

/// Which table is being applied.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Gsub,
    Gpos,
}

/// What shaping one buffer may cost, after HarfBuzz's limits: a budget of steps, where a
/// step is one glyph visited by a lookup, one subtable or rule tried, or one glyph passed
/// over while matching, and a maximum buffer length. When the steps run out, the remaining
/// lookups do nothing.
pub(crate) struct Limits {
    budget: Cell<usize>,
    max_len: usize,
    next_lig_id: u8,
}

impl Limits {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            budget: Cell::new(len.saturating_mul(4096).max(1 << 20)),
            max_len: len.saturating_mul(32).max(1 << 14),
            next_lig_id: 1,
        }
    }

    /// Spends `steps`; false if the budget had run out before.
    pub(crate) fn spend(&self, steps: usize) -> bool {
        let left = self.budget.get();
        self.budget.set(left.saturating_sub(steps));
        left > 0
    }

    /// Whether the budget ran out.
    #[cfg(test)]
    pub(crate) fn exhausted(&self) -> bool {
        self.budget.get() == 0
    }

    /// A ligature id: HarfBuzz keeps three bits of it, so ids cycle through 1 to 7.
    fn lig_id(&mut self) -> u8 {
        let id = self.next_lig_id;
        self.next_lig_id = id % 7 + 1;
        id
    }
}

/// Applies the GSUB lookups to `info`.
pub(crate) fn substitute(
    tables: &Tables<'_>,
    lookups: &Lookups,
    info: &mut Vec<GlyphInfo>,
    limits: &mut Limits,
) {
    let Some(t) = tables.gsub else { return };
    let mut buffer = Buffer::new(std::mem::take(info));
    let mut pos = Vec::new();
    Apply {
        t,
        kind: Kind::Gsub,
        gdef: tables.gdef,
        cache: tables.cache,
        digests: tables.cache.map(|c| c.digests(Kind::Gsub, t)),
        buffer: Digest::of_glyphs(buffer.iter()),
        info: &mut buffer,
        pos: &mut pos,
        limits,
        auto_zwj: true,
        auto_zwnj: true,
        random: false,
        random_state: 1,
        mask: GLOBAL_BIT,
        attached: false,
        last_base: None,
        last_base_until: 0,
        backward: false,
    }
    .run_all(lookups);
    *info = buffer.into_vec();
}

/// Applies the GPOS lookups, adjusting `pos` (one entry per glyph of `info`); `backward`
/// says `info` is right-to-left text in logical order. Returns whether a glyph was attached
/// to another, so attachments need resolving.
pub(crate) fn position(
    tables: &Tables<'_>,
    lookups: &Lookups,
    info: &mut Vec<GlyphInfo>,
    pos: &mut Vec<Position>,
    limits: &mut Limits,
    backward: bool,
) -> bool {
    let Some(t) = tables.gpos else {
        return false;
    };
    debug_assert_eq!(info.len(), pos.len());
    let mut buffer = Buffer::new(std::mem::take(info));
    let mut apply = Apply {
        t,
        kind: Kind::Gpos,
        gdef: tables.gdef,
        cache: tables.cache,
        digests: tables.cache.map(|c| c.digests(Kind::Gpos, t)),
        buffer: Digest::of_glyphs(buffer.iter()),
        info: &mut buffer,
        pos,
        limits,
        auto_zwj: true,
        auto_zwnj: true,
        random: false,
        random_state: 1,
        mask: GLOBAL_BIT,
        attached: false,
        last_base: None,
        last_base_until: 0,
        backward,
    };
    apply.run_all(lookups);
    let attached = apply.attached;
    *info = buffer.into_vec();
    attached
}

/// Applies lookups of one table to a buffer.
struct Apply<'a, 'b> {
    t: &'a [u8],
    kind: Kind,
    gdef: Gdef<'a>,
    /// The font's cache and its lookup digests for this table, by lookup index.
    cache: Option<&'a Cache>,
    digests: Option<&'a [OnceLock<Filter>]>,
    /// Every glyph of the buffer, and some no longer in it.
    buffer: Digest,
    info: &'b mut Buffer,
    pos: &'b mut Vec<Position>,
    limits: &'b mut Limits,
    /// The current lookup's joiner and random settings (see [`Selected`]).
    auto_zwj: bool,
    auto_zwnj: bool,
    random: bool,
    /// HarfBuzz's random number state for `rand`, which starts at 1 for each run.
    random_state: u32,
    /// The glyphs the current lookup applies to (see [`Selected::mask`]).
    mask: u32,
    /// Whether a glyph has been attached to another.
    attached: bool,
    /// HarfBuzz's memory of the base a mark attaches to: the last one found, and the
    /// glyph up to which the search for it went, so marks after it search only the glyphs
    /// in between.
    last_base: Option<usize>,
    last_base_until: usize,
    /// The buffer holds right-to-left text in logical order, which cursive attachment
    /// joins the other way.
    backward: bool,
}

impl<'a> Apply<'a, '_> {
    fn run_all(&mut self, lookups: &Lookups) {
        for selected in &lookups.0 {
            if self.limits.budget.get() == 0 {
                return;
            }
            self.auto_zwj = selected.auto_zwj;
            self.auto_zwnj = selected.auto_zwnj;
            self.random = selected.random;
            self.mask = selected.mask;
            (self.last_base, self.last_base_until) = (None, 0);
            self.run(selected.index);
        }
    }

    fn spend(&self) -> bool {
        let left = self.limits.budget.get();
        self.limits.budget.set(left.saturating_sub(1));
        left > 0
    }

    /// Spends `steps` already taken.
    fn charge(&self, steps: usize) {
        let left = self.limits.budget.get();
        self.limits.budget.set(left.saturating_sub(steps));
    }

    fn extension(&self) -> u16 {
        match self.kind {
            Kind::Gsub => 7,
            Kind::Gpos => 9,
        }
    }

    /// The glyphs a lookup can start at, from the font's cache when it has one.
    fn filter(&self, index: u16, lookup: &Lookup) -> Cow<'a, Filter> {
        let cached = self
            .cache
            .zip(self.digests)
            .and_then(|(cache, d)| Some((cache, d.get(usize::from(index))?)));
        match cached {
            Some((cache, cell)) => Cow::Borrowed(cell.get_or_init(|| {
                let budget = Budget {
                    work: &cache.spent,
                    slots: &cache.slots,
                };
                Digest::of_lookup(self.t, lookup, self.kind, &budget)
            })),
            None => {
                let (work, slots) = (AtomicUsize::new(0), AtomicUsize::new(0));
                let budget = Budget {
                    work: &work,
                    slots: &slots,
                };
                Cow::Owned(Digest::of_lookup(self.t, lookup, self.kind, &budget))
            }
        }
    }

    /// Runs one lookup over the whole buffer, passing over the glyphs its digest rules out.
    fn run(&mut self, index: u16) {
        let t = self.t;
        let Some(lookup) = lookup(t, index) else {
            return;
        };
        let filter = self.filter(index, &lookup);
        let filter = &*filter;
        // A glyph the digest rules out costs what trying each subtable there would have, so
        // the budget runs out at the same point with or without digests.
        let ruled_out = |this: &Self, i: usize| {
            let info = &this.info[i];
            if !this.masked(info) {
                return true;
            }
            if filter.digest.may_have(info.glyph) {
                return false;
            }
            if !this.skips(info, lookup.flags, lookup.filter) {
                let left = this.limits.budget.get();
                this.limits.budget.set(left.saturating_sub(filter.count));
            }
            true
        };
        if !filter.digest.intersects(&self.buffer) {
            for i in 0..self.info.len() {
                if !self.spend() {
                    return;
                }
                ruled_out(self, i);
            }
            return;
        }
        let mut changed = false;
        let first = lookup.subtables(t, self.extension()).next().flatten();
        if self.kind == Kind::Gsub && matches!(first, Some((8, _))) {
            for i in (0..self.info.len()).rev() {
                if !self.spend() {
                    break;
                }
                if !ruled_out(self, i) {
                    changed |= self
                        .apply_at(&lookup, i, 0, Some(&filter.subtables))
                        .is_some();
                }
            }
        } else {
            let mut i = 0;
            while i < self.info.len() {
                if !self.spend() {
                    break;
                }
                if ruled_out(self, i) {
                    i += 1;
                    continue;
                }
                let len = self.info.len();
                let applied = self.apply_at(&lookup, i, 0, Some(&filter.subtables));
                let moved = self.info.take_moved();
                self.charge(moved);
                changed |= applied.is_some();
                i = match applied {
                    // A deletion leaves the next glyph at `i`.
                    Some(next) if next > i || self.info.len() < len => next,
                    _ => i + 1,
                };
            }
        }
        // Substitutions bring in glyphs the buffer digest has not seen.
        if changed && self.kind == Kind::Gsub {
            self.buffer = Digest::of_glyphs(self.info.iter());
        }
    }

    /// Applies the lookup at glyph `i`: the position to continue from if a subtable applied.
    /// `digests`, one for each subtable, pass over the subtables that cannot match there.
    fn apply_at(
        &mut self,
        lookup: &Lookup,
        i: usize,
        depth: u8,
        digests: Option<&[Digest]>,
    ) -> Option<usize> {
        let info = *self.info.get(i)?;
        // A glyph a context record picks takes the nested lookup whatever its flags skip, as
        // in HarfBuzz; the flags still apply to the glyphs matched after it.
        if depth == 0 && self.skips(&info, lookup.flags, lookup.filter) {
            return None;
        }
        let t = self.t;
        for (k, subtable) in lookup.subtables(t, self.extension()).enumerate() {
            if !self.spend() {
                return None;
            }
            let Some((kind, at)) = subtable else {
                continue;
            };
            if digests
                .and_then(|d| d.get(k))
                .is_some_and(|d| !d.may_have(info.glyph))
            {
                continue;
            }
            let applied = match (self.kind, kind) {
                (Kind::Gsub, 5) | (Kind::Gpos, 7) => self.context(lookup, at, i, depth),
                (Kind::Gsub, 6) | (Kind::Gpos, 8) => self.chain_context(lookup, at, i, depth),
                (Kind::Gsub, _) => self.substitute_at(lookup, kind, at, i, depth),
                (Kind::Gpos, _) => self.position_at(lookup, kind, at, i),
            };
            if applied.is_some() {
                return applied;
            }
        }
        None
    }

    /// Whether the current lookup applies to this glyph.
    fn masked(&self, info: &GlyphInfo) -> bool {
        self.mask & info.mask != 0
    }

    fn skips(&self, info: &GlyphInfo, flags: u16, filter: u16) -> bool {
        match info.class {
            BASE => flags & IGNORE_BASE_GLYPHS != 0,
            LIGATURE => flags & IGNORE_LIGATURES != 0,
            MARK => {
                if flags & IGNORE_MARKS != 0 {
                    return true;
                }
                if flags & USE_MARK_FILTERING_SET != 0 {
                    return !self.gdef.in_mark_set(filter, info.glyph);
                }
                let wanted = (flags & MARK_ATTACHMENT_TYPE) >> 8;
                wanted != 0 && u16::from(info.mark_class) != wanted
            }
            _ => false,
        }
    }

    /// Whether matching may pass over this glyph when it does not match, as HarfBuzz decides:
    /// joiners and other default ignorables pass, except that substitution stops at ZERO
    /// WIDTH NON-JOINER in its input and at hidden characters everywhere, and lookups of
    /// `mark` and `mkmk` stop at ZERO WIDTH JOINER in their input.
    fn passable(&self, info: &GlyphInfo, context: bool) -> bool {
        if !info.ignorable {
            return false;
        }
        let positioning = self.kind == Kind::Gpos;
        (!info.zwnj || positioning || (context && self.auto_zwnj))
            && (!info.zwj || context || self.auto_zwj)
            && (!info.hidden || positioning)
    }

    /// The next glyph after `from` that the lookup does not skip and `matches` accepts, or
    /// `None` if another glyph comes first.
    fn next_match(
        &self,
        lookup: &Lookup,
        from: usize,
        context: bool,
        mut matches: impl FnMut(&GlyphInfo) -> bool,
    ) -> Option<usize> {
        for k in from + 1..self.info.len() {
            if !self.spend() {
                return None;
            }
            let info = &self.info[k];
            if self.skips(info, lookup.flags, lookup.filter) {
                continue;
            }
            // Input glyphs must be ones the lookup applies to; context glyphs need not.
            if (context || self.masked(info)) && matches(info) {
                return Some(k);
            }
            if !self.passable(info, context) {
                return None;
            }
        }
        None
    }

    /// The same, backwards from `from`.
    fn prev_match(
        &self,
        lookup: &Lookup,
        from: usize,
        context: bool,
        mut matches: impl FnMut(&GlyphInfo) -> bool,
    ) -> Option<usize> {
        for k in (0..from).rev() {
            if !self.spend() {
                return None;
            }
            let info = &self.info[k];
            if self.skips(info, lookup.flags, lookup.filter) {
                continue;
            }
            // Input glyphs must be ones the lookup applies to; context glyphs need not.
            if (context || self.masked(info)) && matches(info) {
                return Some(k);
            }
            if !self.passable(info, context) {
                return None;
            }
        }
        None
    }

    /// Matches `count` more input glyphs after `i`: their positions, `i` first.
    fn match_input(
        &self,
        lookup: &Lookup,
        i: usize,
        count: usize,
        mut matches: impl FnMut(usize, &GlyphInfo) -> bool,
    ) -> Option<Vec<usize>> {
        if count >= MAX_CONTEXT {
            return None;
        }
        let mut positions = Vec::with_capacity(count + 1);
        positions.push(i);
        let (first_id, first_comp) = (self.info[i].lig_id(), self.info[i].lig_comp());
        let mut ligature_skipped: Option<bool> = None;
        let mut at = i;
        for k in 0..count {
            at = self.next_match(lookup, at, false, |info| matches(k, info))?;
            let this = self.info[at];
            // As HarfBuzz: marks that followed different components of a ligature the lookup
            // can see do not match together, nor do a mark that followed no component and one
            // that followed a component of another ligature.
            if first_id != 0 && first_comp != 0 {
                if (this.lig_id(), this.lig_comp()) != (first_id, first_comp)
                    && !*ligature_skipped
                        .get_or_insert_with(|| self.ligature_skipped(lookup, i, first_id))
                {
                    return None;
                }
            } else if this.lig_id() != 0 && this.lig_comp() != 0 && this.lig_id() != first_id {
                return None;
            }
            positions.push(at);
        }
        Some(positions)
    }

    /// Whether the lookup skips the ligature that the marks before `i` with ligature id `id`
    /// follow (and those glyphs can be passed over).
    fn ligature_skipped(&self, lookup: &Lookup, i: usize, id: u8) -> bool {
        // HarfBuzz looks among the glyphs already output, and positioning outputs none.
        if self.kind == Kind::Gpos {
            return false;
        }
        let mut j = i;
        while j > 0 && self.info[j - 1].lig_id() == id {
            if !self.spend() {
                return false;
            }
            j -= 1;
            if self.info[j].lig_comp() == 0 {
                return self.skips(&self.info[j], lookup.flags, lookup.filter);
            }
        }
        false
    }

    /// Matches `count` glyphs before `i`, nearest first.
    fn match_backtrack(
        &self,
        lookup: &Lookup,
        i: usize,
        count: usize,
        mut matches: impl FnMut(usize, &GlyphInfo) -> bool,
    ) -> bool {
        let mut at = i;
        for k in 0..count {
            match self.prev_match(lookup, at, true, |info| matches(k, info)) {
                Some(p) => at = p,
                None => return false,
            }
        }
        true
    }

    /// Matches `count` glyphs after `last`.
    fn match_lookahead(
        &self,
        lookup: &Lookup,
        last: usize,
        count: usize,
        mut matches: impl FnMut(usize, &GlyphInfo) -> bool,
    ) -> bool {
        let mut at = last;
        for k in 0..count {
            match self.next_match(lookup, at, true, |info| matches(k, info)) {
                Some(p) => at = p,
                None => return false,
            }
        }
        true
    }

    /// Contextual lookups (GSUB 5, GPOS 7).
    fn context(&mut self, lookup: &Lookup, at: usize, i: usize, depth: u8) -> Option<usize> {
        let t = self.t;
        let glyph = self.info[i].glyph;
        let format = u16_at(t, at)?;
        if format == 3 {
            let count = usize::from(u16_at(t, at + 2)?);
            let records = usize::from(u16_at(t, at + 4)?);
            let covered = |k: usize, g: u16| {
                u16_at(t, at + 6 + 2 * k)
                    .and_then(|o| coverage(t, at + usize::from(o), g))
                    .is_some()
            };
            if count == 0 || !covered(0, glyph) {
                return None;
            }
            let matched =
                self.match_input(lookup, i, count - 1, |k, info| covered(k + 1, info.glyph))?;
            return Some(self.apply_records(&matched, at + 6 + 2 * count, records, depth));
        }
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), glyph)?;
        let (classes, key, sets_at) = match format {
            1 => (None, index, at + 4),
            2 => {
                let classes = class_def(t, at, at + 4)?;
                (Some(classes), class_in(t, classes, glyph), at + 6)
            }
            _ => return None,
        };
        let set = offset(
            t,
            at,
            sets_at + 2 + 2 * usize::from(key),
            u16_at(t, sets_at)?,
            key,
        )?;
        let value = |g: u16| classes.map_or(g, |c| class_in(t, c, g));
        for r in 0..usize::from(u16_at(t, set)?) {
            if !self.spend() {
                return None;
            }
            let rule = set + usize::from(u16_at(t, set + 2 + 2 * r)?);
            let inputs = usize::from(u16_at(t, rule)?);
            let records = usize::from(u16_at(t, rule + 2)?);
            let Some(more) = inputs.checked_sub(1) else {
                continue;
            };
            let input = |k: usize| u16_at(t, rule + 4 + 2 * k);
            if let Some(matched) = self.match_input(lookup, i, more, |k, info| {
                input(k) == Some(value(info.glyph))
            }) {
                return Some(self.apply_records(&matched, rule + 4 + 2 * more, records, depth));
            }
        }
        None
    }

    /// Chained contextual lookups (GSUB 6, GPOS 8).
    fn chain_context(&mut self, lookup: &Lookup, at: usize, i: usize, depth: u8) -> Option<usize> {
        let t = self.t;
        let glyph = self.info[i].glyph;
        let format = u16_at(t, at)?;
        if format == 3 {
            let backtrack = usize::from(u16_at(t, at + 2)?);
            let input_at = at + 4 + 2 * backtrack;
            let inputs = usize::from(u16_at(t, input_at)?);
            let ahead_at = input_at + 2 + 2 * inputs;
            let lookahead = usize::from(u16_at(t, ahead_at)?);
            let records_at = ahead_at + 2 + 2 * lookahead;
            let records = usize::from(u16_at(t, records_at)?);
            let covered = |list: usize, k: usize, g: u16| {
                u16_at(t, list + 2 * k)
                    .and_then(|o| coverage(t, at + usize::from(o), g))
                    .is_some()
            };
            if inputs == 0 || !covered(input_at + 2, 0, glyph) {
                return None;
            }
            let matched = self.match_input(lookup, i, inputs - 1, |k, info| {
                covered(input_at + 2, k + 1, info.glyph)
            })?;
            let last = *matched.last()?;
            if !self.match_backtrack(lookup, i, backtrack, |k, info| {
                covered(at + 4, k, info.glyph)
            }) || !self.match_lookahead(lookup, last, lookahead, |k, info| {
                covered(ahead_at + 2, k, info.glyph)
            }) {
                return None;
            }
            return Some(self.apply_records(&matched, records_at + 2, records, depth));
        }
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), glyph)?;
        let (defs, sets_at) = match format {
            1 => (None, at + 4),
            2 => {
                let def = |k: usize| class_def(t, at, at + 4 + 2 * k);
                (Some([def(0)?, def(1)?, def(2)?]), at + 10)
            }
            _ => return None,
        };
        let value = |which: usize, g: u16| defs.map_or(g, |d| class_in(t, d[which], g));
        let key = if defs.is_some() {
            value(1, glyph)
        } else {
            index
        };
        let set = offset(
            t,
            at,
            sets_at + 2 + 2 * usize::from(key),
            u16_at(t, sets_at)?,
            key,
        )?;
        for r in 0..usize::from(u16_at(t, set)?) {
            if !self.spend() {
                return None;
            }
            let rule = set + usize::from(u16_at(t, set + 2 + 2 * r)?);
            let backtrack = usize::from(u16_at(t, rule)?);
            let input_at = rule + 2 + 2 * backtrack;
            let inputs = usize::from(u16_at(t, input_at)?);
            let Some(more) = inputs.checked_sub(1) else {
                continue;
            };
            let ahead_at = input_at + 2 + 2 * more;
            let lookahead = usize::from(u16_at(t, ahead_at)?);
            let records_at = ahead_at + 2 + 2 * lookahead;
            let records = usize::from(u16_at(t, records_at)?);
            let Some(matched) = self.match_input(lookup, i, more, |k, info| {
                u16_at(t, input_at + 2 + 2 * k) == Some(value(1, info.glyph))
            }) else {
                continue;
            };
            let last = *matched.last()?;
            if self.match_backtrack(lookup, i, backtrack, |k, info| {
                u16_at(t, rule + 2 + 2 * k) == Some(value(0, info.glyph))
            }) && self.match_lookahead(lookup, last, lookahead, |k, info| {
                u16_at(t, ahead_at + 2 + 2 * k) == Some(value(2, info.glyph))
            }) {
                return Some(self.apply_records(&matched, records_at + 2, records, depth));
            }
        }
        None
    }

    /// Applies a context's nested lookups to its matched glyphs, whose positions shift when
    /// a nested substitution changes the buffer's length (as HarfBuzz adjusts them). Returns
    /// the position after the match.
    fn apply_records(&mut self, matched: &[usize], at: usize, count: usize, depth: u8) -> usize {
        let mut positions = matched.to_vec();
        let mut end = positions.last().map_or(0, |&p| p as isize + 1);
        if depth >= MAX_NESTING {
            return end as usize;
        }
        for r in 0..count {
            if !self.spend() {
                break;
            }
            let (Some(sequence), Some(index)) =
                (u16_at(self.t, at + 4 * r), u16_at(self.t, at + 4 * r + 2))
            else {
                break;
            };
            let idx = usize::from(sequence);
            let Some(&p) = positions.get(idx) else {
                continue;
            };
            let Some(nested) = lookup(self.t, index) else {
                continue;
            };
            let before = self.info.len() as isize;
            let applied = self.apply_at(&nested, p, depth + 1, None);
            let moved = self.info.take_moved();
            self.charge(moved);
            if applied.is_none() {
                continue;
            }
            let mut delta = self.info.len() as isize - before;
            if delta == 0 {
                continue;
            }
            end += delta;
            if end < p as isize {
                delta += p as isize - end;
                end = p as isize;
            }
            let tail = idx + 1;
            if delta > 0 {
                if positions.len() + delta as usize > MAX_CONTEXT {
                    break;
                }
                let added = (1..=delta as usize).map(|k| p + k);
                let rest: Vec<usize> = positions[tail..]
                    .iter()
                    .map(|&q| q + delta as usize)
                    .collect();
                positions.truncate(tail);
                positions.extend(added);
                positions.extend(rest);
            } else {
                let removed = ((-delta) as usize).min(positions.len() - tail);
                let rest: Vec<usize> = positions[tail + removed..]
                    .iter()
                    .map(|&q| q.saturating_sub((-delta) as usize))
                    .collect();
                positions.truncate(tail);
                positions.extend(rest);
            }
        }
        (end.max(0) as usize).min(self.info.len())
    }
}

/// Removes glyph `i`, keeping its cluster: a glyph alone in its cluster passes the cluster to
/// the next glyph when no glyph precedes it, as HarfBuzz does.
/// Deletes glyph `i`; returns the glyphs whose cluster it rewrote.
fn delete_glyph(info: &mut Buffer, i: usize) -> usize {
    let cluster = info[i].cluster;
    let shared = (i + 1 < info.len() && info[i + 1].cluster == cluster)
        || (i > 0 && info[i - 1].cluster == cluster);
    if !shared && i == 0 && info.len() > 1 {
        let next = info[1].cluster;
        let mut k = 1;
        while k < info.len() && info[k].cluster == next {
            info[k].cluster = cluster.min(next);
            k += 1;
        }
        info.remove(i);
        return k - 1;
    }
    info.remove(i);
    0
}

#[cfg(test)]
pub(crate) mod tests;
