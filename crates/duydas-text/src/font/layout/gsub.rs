//! Glyph substitution (GSUB) lookups 1 to 4 and 8; contexts are in the parent module.

use super::super::reader::u16_at;
use super::{Apply, BASE, GlyphInfo, LIGATURE, Lookup, MARK, coverage, offset};

impl Apply<'_, '_> {
    /// Applies a substitution subtable of type `kind` at glyph `i`.
    pub(super) fn substitute_at(
        &mut self,
        lookup: &Lookup,
        kind: u16,
        at: usize,
        i: usize,
        depth: u8,
    ) -> Option<usize> {
        let t = self.t;
        let glyph = self.info[i].glyph;
        match kind {
            1 => {
                let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), glyph)?;
                let new = match u16_at(t, at)? {
                    1 => glyph.wrapping_add(u16_at(t, at + 4)?),
                    2 if index < u16_at(t, at + 4)? => u16_at(t, at + 6 + 2 * usize::from(index))?,
                    _ => return None,
                };
                self.replace(i, new);
                Some(i + 1)
            }
            2 => {
                let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), glyph)?;
                if u16_at(t, at)? != 1 {
                    return None;
                }
                let sequence = offset(
                    t,
                    at,
                    at + 6 + 2 * usize::from(index),
                    u16_at(t, at + 4)?,
                    index,
                )?;
                let count = usize::from(u16_at(t, sequence)?);
                // The limit comes before reading the sequence.
                if self.info.len() + count.saturating_sub(1) > self.limits.max_len {
                    return None;
                }
                self.charge(count);
                let glyphs = (0..count)
                    .map(|k| u16_at(t, sequence + 2 + 2 * k))
                    .collect::<Option<Vec<u16>>>()?;
                self.multiple(i, &glyphs)
            }
            3 => {
                let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), glyph)?;
                if u16_at(t, at)? != 1 {
                    return None;
                }
                let set = offset(
                    t,
                    at,
                    at + 6 + 2 * usize::from(index),
                    u16_at(t, at + 4)?,
                    index,
                )?;
                let count = u32::from(u16_at(t, set)?);
                if count == 0 {
                    return None;
                }
                // As HarfBuzz: the glyph's mask bits under the lookup's, shifted down, which
                // mixes the values if several features with values select the lookup.
                let mut choice = (self.mask & self.info[i].mask)
                    .checked_shr(self.mask.trailing_zeros())
                    .unwrap_or(0);
                if choice == RANDOM && self.random {
                    choice = self.random_number() % count + 1;
                }
                if choice == 0 || choice > count {
                    return None;
                }
                let new = u16_at(t, set + 2 + 2 * (choice - 1) as usize)?;
                self.replace(i, new);
                Some(i + 1)
            }
            4 => self.ligature(lookup, at, i),
            // Reverse chaining applies only on its own, never from a context.
            8 if depth == 0 => self.reverse_chain(lookup, at, i),
            _ => None,
        }
    }

    /// Puts `glyph` at `i`. A substituted default ignorable is drawn like any glyph from then
    /// on, as in HarfBuzz.
    fn replace(&mut self, i: usize, glyph: u16) {
        let info = &mut self.info[i];
        info.glyph = glyph;
        info.ignorable = false;
        let class = info.class;
        self.gdef.classify(info, class);
    }

    /// Replaces glyph `i` with a sequence; an empty sequence deletes it, as HarfBuzz allows.
    /// As in HarfBuzz, the glyphs of a longer sequence remember their place in it (unless
    /// the glyph belonged to a ligature), and those replacing a ligature become bases.
    fn multiple(&mut self, i: usize, glyphs: &[u16]) -> Option<usize> {
        match glyphs {
            [] => {
                let rewritten = super::delete_glyph(self.info, i);
                self.charge(rewritten);
                Some(i)
            }
            [glyph] => {
                self.replace(i, *glyph);
                Some(i + 1)
            }
            _ => {
                let template = self.info[i];
                let class = if template.class == LIGATURE {
                    BASE
                } else {
                    template.class
                };
                let gdef = self.gdef;
                let replacement: Vec<GlyphInfo> = glyphs
                    .iter()
                    .enumerate()
                    .map(|(k, &glyph)| {
                        let mut info = GlyphInfo {
                            glyph,
                            ignorable: false,
                            multiplied: true,
                            ..template
                        };
                        // Glyphs of a ligature stay with it (HarfBuzz issue 3069).
                        if template.lig_id() == 0 {
                            info.set_component(0, k);
                        }
                        gdef.classify(&mut info, class);
                        info
                    })
                    .collect();
                self.info.replace_with(i, &replacement);
                Some(i + glyphs.len())
            }
        }
    }

    fn ligature(&mut self, lookup: &Lookup, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        if u16_at(t, at)? != 1 {
            return None;
        }
        let set = offset(
            t,
            at,
            at + 6 + 2 * usize::from(index),
            u16_at(t, at + 4)?,
            index,
        )?;
        for r in 0..usize::from(u16_at(t, set)?) {
            if !self.spend() {
                return None;
            }
            let ligature = set + usize::from(u16_at(t, set + 2 + 2 * r)?);
            let glyph = u16_at(t, ligature)?;
            let Some(more) = usize::from(u16_at(t, ligature + 2)?).checked_sub(1) else {
                continue;
            };
            if more == 0 {
                self.replace(i, glyph);
                return Some(i + 1);
            }
            let component = |k: usize| u16_at(t, ligature + 4 + 2 * k);
            if let Some(matched) =
                self.match_input(lookup, i, more, |k, info| component(k) == Some(info.glyph))
            {
                self.form_ligature(&matched, glyph);
                // As HarfBuzz, go on after the last component: the glyphs passed over
                // between the components are not tried again.
                return Some(matched[matched.len() - 1] + 2 - matched.len());
            }
        }
        None
    }

    /// Replaces the matched glyphs with a ligature. The glyphs between them stay, and the
    /// marks among them, and those after that followed the last component, learn which
    /// component of the new ligature they follow, as in HarfBuzz.
    fn form_ligature(&mut self, matched: &[usize], glyph: u16) {
        let (first, last) = (matched[0], matched[matched.len() - 1]);
        let rest_marks = matched[1..].iter().all(|&p| self.info[p].class == MARK);
        let base_ligature = self.info[first].class == BASE && rest_marks;
        let mark_ligature = self.info[first].class == MARK && rest_marks;
        let is_ligature = !base_ligature && !mark_ligature;
        let lig_id = if is_ligature { self.limits.lig_id() } else { 0 };
        let total: usize = matched
            .iter()
            .map(|&p| usize::from(self.info[p].components()))
            .sum();

        self.merge_clusters(first, last + 1);

        let mut last_lig_id = self.info[first].lig_id();
        let mut last_comps = usize::from(self.info[first].components());
        let mut so_far = last_comps;
        for &p in &matched[1..] {
            let start = matched.iter().rev().find(|&&q| q < p).map_or(first, |&q| q) + 1;
            if is_ligature {
                for q in start..p {
                    let mark = &mut self.info[q];
                    let this = match usize::from(mark.lig_comp()) {
                        0 => last_comps,
                        c => c,
                    };
                    mark.set_component(lig_id, so_far - last_comps + this.min(last_comps));
                }
            }
            last_lig_id = self.info[p].lig_id();
            last_comps = usize::from(self.info[p].components());
            so_far += last_comps;
        }
        if !mark_ligature && last_lig_id != 0 {
            let mut renumbered = 0;
            for q in last + 1..self.info.len() {
                let mark = &mut self.info[q];
                if mark.lig_id() != last_lig_id || mark.lig_comp() == 0 {
                    break;
                }
                let this = usize::from(mark.lig_comp());
                mark.set_component(lig_id, so_far - last_comps + this.min(last_comps));
                renumbered += 1;
            }
            // Ligatures that take in ligatures can renumber the same long run of marks again
            // and again.
            self.charge(renumbered);
        }

        let info = &mut self.info[first];
        info.glyph = glyph;
        info.ignorable = false;
        info.multiplied = false;
        if is_ligature {
            info.set_ligature(lig_id, total);
            // A ligature that starts with a nonspacing mark counts as a letter from now on,
            // as HarfBuzz makes it Lo: fallback positioning puts marks on it.
            if info.nonspacing_mark {
                info.nonspacing_mark = false;
                info.unicode_mark = false;
                info.combining_class = 0;
            }
        }
        let class = if is_ligature { LIGATURE } else { info.class };
        self.gdef.classify(info, class);
        for &p in matched[1..].iter().rev() {
            self.info.remove(p);
        }
    }

    /// Gives glyphs `start..end` the smallest of their clusters, as HarfBuzz's
    /// merge_clusters: the merge reaches on over the glyphs after them that share the last
    /// one's cluster, and back over those before them that share the first one's. It spends
    /// a step for each glyph it touches, as that reach can cover a long cluster many times.
    fn merge_clusters(&mut self, start: usize, end: usize) {
        let mut cluster = self.info[start].cluster;
        for k in start + 1..end {
            cluster = cluster.min(self.info[k].cluster);
        }
        let mut end = end;
        if cluster != self.info[end - 1].cluster {
            while end < self.info.len() && self.info[end - 1].cluster == self.info[end].cluster {
                end += 1;
            }
        }
        let mut start = start;
        if cluster != self.info[start].cluster {
            while start > 0 && self.info[start - 1].cluster == self.info[start].cluster {
                start -= 1;
            }
        }
        for k in start..end {
            self.info[k].cluster = cluster;
        }
        self.charge(end - start);
    }

    /// Reverse chaining single substitution (type 8).
    fn reverse_chain(&mut self, lookup: &Lookup, at: usize, i: usize) -> Option<usize> {
        let t = self.t;
        if u16_at(t, at)? != 1 {
            return None;
        }
        let index = coverage(t, at + usize::from(u16_at(t, at + 2)?), self.info[i].glyph)?;
        let backtrack = usize::from(u16_at(t, at + 4)?);
        let ahead_at = at + 6 + 2 * backtrack;
        let lookahead = usize::from(u16_at(t, ahead_at)?);
        let subs_at = ahead_at + 2 + 2 * lookahead;
        if index >= u16_at(t, subs_at)? {
            return None;
        }
        let covered = |list: usize, k: usize, g: u16| {
            u16_at(t, list + 2 * k)
                .and_then(|o| coverage(t, at + usize::from(o), g))
                .is_some()
        };
        if !self.match_backtrack(lookup, i, backtrack, |k, info| {
            covered(at + 6, k, info.glyph)
        }) || !self.match_lookahead(lookup, i, lookahead, |k, info| {
            covered(ahead_at + 2, k, info.glyph)
        }) {
            return None;
        }
        let new = u16_at(t, subs_at + 2 + 2 * usize::from(index))?;
        self.replace(i, new);
        Some(i)
    }
}

/// The alternate value that `rand` sets: pick one at random.
const RANDOM: u32 = 255;

impl Apply<'_, '_> {
    /// HarfBuzz's random numbers: a Lehmer generator in 32-bit unsigned arithmetic.
    fn random_number(&mut self) -> u32 {
        self.random_state = self.random_state.wrapping_mul(48_271) % 2_147_483_647;
        self.random_state
    }
}
