//! Deterministic SplitMix64 pseudo-random number generator for cluster bootstrap resampling.

#[derive(Clone, Debug)]
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniformly samples an index in `0..upper` without modulo bias.
    pub(crate) fn next_index(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        if upper <= 1 {
            return 0;
        }
        let bound = upper as u64;
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u64();
            if r >= threshold {
                return (r % bound) as usize;
            }
        }
    }
}
