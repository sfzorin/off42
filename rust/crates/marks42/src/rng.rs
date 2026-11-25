//! Deterministic random numbers (SplitMix64), identical to Kit42 `SeededRandom`: the same seed gives the same
//! signature design or stamp impression on macOS and Windows.

#[derive(Clone, Debug)]
pub struct SeededRandom {
    state: u64,
}

impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15) }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in 0..1.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform value in `low..=high` (same arithmetic as Swift `range(_:)`).
    pub fn range(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    pub fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize].clone()
    }

    /// Index chosen with the given relative weights.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let mut x = self.unit() * weights.iter().sum::<f64>();
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }

    /// A fresh non-zero seed from the clock and address entropy (each insertion is a new impression).
    pub fn fresh_seed() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let local = 0u8;
        let mut r = SeededRandom::new(nanos ^ (&local as *const u8 as u64).rotate_left(17));
        r.next_u64().max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_kit42_splitmix_sequence() {
        // Reference values printed by Kit42 SeededRandom (Swift) for the same seeds.
        let mut r = SeededRandom::new(42);
        assert_eq!([r.next_u64(), r.next_u64(), r.next_u64()], [2949826092126892291, 5139283748462763858, 6349198060258255764]);
        assert!((SeededRandom::new(2424868).unit() - 0.811817994873566).abs() < 1e-15);
    }
}
