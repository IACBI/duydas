//! A small, fast, deterministic hash for frame fingerprints.
//!
//! A collision makes one tile keep stale pixels until its content changes again, so a 64-bit
//! multiplicative hash with a final avalanche step is enough. The hash never leaves the process.

const K: u64 = 0xF135_7AEA_2E62_A9C5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Hasher(u64);

impl Hasher {
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    #[inline]
    pub(crate) const fn write(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(K).rotate_left(26);
    }

    pub(crate) const fn finish(self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        h ^ (h >> 33)
    }
}

/// Folds an item into a running hash; the order of items matters.
#[inline]
pub(crate) const fn mix(acc: u64, item: u64) -> u64 {
    (acc ^ item).wrapping_mul(K).rotate_left(29)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_content_change_the_hash() {
        let hash = |values: &[u64]| {
            let mut h = Hasher::new(1);
            for &v in values {
                h.write(v);
            }
            h.finish()
        };
        assert_ne!(hash(&[1, 2]), hash(&[2, 1]));
        assert_ne!(hash(&[1, 2]), hash(&[1, 3]));
        assert_eq!(hash(&[7, 8, 9]), hash(&[7, 8, 9]));
        assert_ne!(mix(mix(0, 1), 2), mix(mix(0, 2), 1));
    }
}
