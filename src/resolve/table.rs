//! The order apk keeps its names in.
//!
//! apk holds every name it reads in a hash table of 20,000 chained buckets,
//! and where it goes through names in the table's order, it visits the
//! buckets in ascending order and, within one, the name it read last first.
//! A name's bucket is its 32-bit MurmurHash3 (the x86 variant) with seed
//! 5381, modulo 20,000.
//!
//! Each part is measured, as the order of packages one placement triggers
//! (`install-order/packages-one-placement-triggers-in-apk-s-table-order`):
//! the size and every name's bucket from 300,000 names loaded forward and
//! in reverse, whose orders differ exactly within a bucket, and the hash on
//! 3,000,000 names. apk-tools 2.12.14 to 3.0.9 keep the same table.

/// The number of buckets in apk's table of names.
const BUCKETS: u32 = 20_000;

/// The seed apk hashes names with.
const SEED: u32 = 5381;

/// Where apk's table holds `name`: its bucket, and its place in the bucket
/// given `read`, the place of the name in the order apk read names in.
/// Positions compare in the order apk visits names.
pub(super) fn position(name: &[u8], read: usize) -> (u32, core::cmp::Reverse<usize>) {
    (bucket(name), core::cmp::Reverse(read))
}

/// The bucket of apk's table that holds `name`.
fn bucket(name: &[u8]) -> u32 {
    murmur3(name, SEED) % BUCKETS
}

/// MurmurHash3, the x86 variant with a 32-bit result, of `bytes`.
fn murmur3(bytes: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    fn scramble(word: u32) -> u32 {
        word.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2)
    }

    let mut hash = seed;
    let (blocks, tail) = bytes.as_chunks::<4>();
    for block in blocks {
        hash ^= scramble(u32::from_le_bytes(*block));
        hash = hash
            .rotate_left(13)
            .wrapping_mul(5)
            .wrapping_add(0xe654_6b64);
    }
    if !tail.is_empty() {
        // The last one to three bytes, the first lowest.
        let word = tail
            .iter()
            .rev()
            .fold(0, |word, &byte| (word << 8) | u32::from(byte));
        hash ^= scramble(word);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the hash takes the length modulo 2^32"
    )]
    let length = bytes.len() as u32;
    hash ^= length;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn murmur3_matches_its_published_values() {
        assert_eq!(murmur3(b"", 0), 0);
        assert_eq!(murmur3(b"", 1), 0x514e_28b7);
        assert_eq!(murmur3(b"", 0xffff_ffff), 0x81f1_6f39);
        assert_eq!(murmur3(b"\0\0\0\0", 0), 0x2362_f9de);
        assert_eq!(murmur3(b"aaaa", 0x9747_b28c), 0x5a97_808a);
        assert_eq!(murmur3(b"abc", 0), 0xb3dd_93fa);
        assert_eq!(murmur3(b"Hello, world!", 0x9747_b28c), 0x2488_4cba);
    }

    #[test]
    fn buckets_are_apk_s() {
        // Measured with apk-tools 3.0.9: the buckets of three names, and
        // the order of single letters, which their buckets give.
        assert_eq!(bucket(b"pyc"), 5158);
        assert_eq!(bucket(b"python3-pyc"), 12615);
        assert_eq!(bucket(b"py3-setuptools-pyc"), 143);
        let mut letters: Vec<u8> = (b'a'..=b'z').collect();
        letters.sort_by_key(|letter| bucket(&[*letter]));
        assert_eq!(letters, b"wxhijrofecpsulyntadkgmqvbz");
    }

    #[test]
    fn a_name_read_later_comes_first_in_its_bucket() {
        assert_eq!(bucket(b"ka"), bucket(b"kb2304"));
        assert!(position(b"kb2304", 1) < position(b"ka", 0));
        assert!(position(b"ka", 1) < position(b"kb2304", 0));
    }
}
