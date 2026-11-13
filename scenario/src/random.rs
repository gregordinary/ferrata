//! Deterministic choices, so that anything made at random is made again from
//! its seed.
//!
//! [`Rng`] draws from a seed and [`Bytes`] from a fuzzer's input; both are a
//! [`Source`] of choices, so one generator serves a seeded run and a fuzzing
//! run alike. [`hash`] turns text into a number, to seed from a name or to
//! divide names among shards in a way that does not depend on which other
//! names there are.

/// Where choices come from.
pub trait Source {
    /// A number below `bound`, which is at least 1.
    fn below(&mut self, bound: usize) -> usize;

    /// True one time in `odds`.
    fn one_in(&mut self, odds: usize) -> bool {
        self.below(odds) == 0
    }

    /// One of `choices`, which is not empty.
    fn pick<T: Copy>(&mut self, choices: &[T]) -> T {
        choices[self.below(choices.len())]
    }
}

/// SplitMix64: a small generator whose whole state is one number.
///
/// The library's own tests carry a copy (`tests/versions/generate.rs`):
/// they cannot depend on this crate, which depends on the library.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator starting from `seed`.
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl Source for Rng {
    fn below(&mut self, bound: usize) -> usize {
        // The bias of a remainder is immaterial at the bounds used here.
        (self.next_u64() % bound.max(1) as u64) as usize
    }
}

/// Choices read from bytes, as a fuzzer supplies them: each choice takes as
/// many bytes as its bound needs, and once they run out every choice is 0,
/// so a short input makes a small case.
#[derive(Debug, Clone)]
pub struct Bytes<'a> {
    bytes: &'a [u8],
}

impl<'a> Bytes<'a> {
    /// Choices from `bytes`.
    pub fn new(bytes: &'a [u8]) -> Bytes<'a> {
        Bytes { bytes }
    }
}

impl Source for Bytes<'_> {
    fn below(&mut self, bound: usize) -> usize {
        let bound = bound.max(1);
        let mut value = 0usize;
        let mut span = 1usize;
        while span < bound {
            let Some((&byte, rest)) = self.bytes.split_first() else {
                break;
            };
            self.bytes = rest;
            value = (value << 8) | usize::from(byte);
            span = span.saturating_mul(256);
        }
        value % bound
    }
}

/// FNV-1a over `bytes`: a stable hash of text, the same on every platform
/// and in every run.
pub fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_repeats() {
        let draw = |seed| {
            let mut rng = Rng::new(seed);
            (0..8).map(|_| rng.below(1000)).collect::<Vec<_>>()
        };
        assert_eq!(draw(7), draw(7));
        assert_ne!(draw(7), draw(8));
    }

    #[test]
    fn bytes_run_out_into_zeros() {
        let mut bytes = Bytes::new(&[5, 1, 2]);
        assert_eq!(bytes.below(10), 5);
        // A bound above 256 takes two bytes.
        assert_eq!(bytes.below(1000), 258);
        assert_eq!(bytes.below(10), 0);
        assert_eq!(bytes.below(1), 0);
    }

    #[test]
    fn the_hash_is_fnv_1a() {
        // The published test vectors for 64-bit FNV-1a.
        assert_eq!(hash(*b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash(*b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash(*b"foobar"), 0x8594_4171_f739_67e8);
    }
}
