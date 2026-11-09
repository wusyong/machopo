// The fixed names' perfect hash's hash (`names.rs`), shared with `build.rs`,
// which `include!`s this file: the table it makes is only right if both
// hash alike. FxHash's step over 8 bytes at a time (cheap for names, most
// under 16 bytes; SipHash, `phf`'s, cost a `createElement` from JS 250
// instructions more), then murmur3's finish, so that the bits the table
// takes apart are all mixed.

/// `bytes`' hash, with `seed` (the one `build.rs` found a table for). The
/// last 8 bytes or fewer are read as whole loads, overlapping (wyhash's
/// way): copied into a word, a `memcpy` writes them bytes at a time and the
/// word read after can't forward from those stores (70% of the time this
/// took, so, for `getAttribute`).
#[inline]
pub(crate) fn name_hash(bytes: &[u8], seed: u64) -> u64 {
    const K: u64 = 0xf135_7aea_2e62_a9c5;
    let step = |hash: u64, word: u64| (hash.rotate_left(26) ^ word).wrapping_mul(K);
    let mut hash = seed ^ (bytes.len() as u64).wrapping_mul(K);
    let mut rest = bytes;
    while let Some((word, tail)) = rest.split_first_chunk::<8>()
        && !tail.is_empty()
    {
        hash = step(hash, u64::from_le_bytes(*word));
        rest = tail;
    }
    let last = match (rest.first_chunk::<4>(), rest.last_chunk::<4>()) {
        _ if rest.len() == 8 => u64::from_le_bytes(*rest.first_chunk::<8>().unwrap_or(&[0; 8])),
        (Some(first), Some(last)) => {
            u64::from(u32::from_le_bytes(*first)) | u64::from(u32::from_le_bytes(*last)) << 32
        }
        // Fewer than 4: the first, the middle and the last.
        _ => {
            let (first, middle, last) = match *rest {
                [] => return finish(step(hash, 0)),
                [only] => (only, only, only),
                [first, last] => (first, last, last),
                [first, middle, last, ..] => (first, middle, last),
            };
            u64::from(first) | u64::from(middle) << 8 | u64::from(last) << 16
        }
    };
    finish(step(hash, last))
}

/// murmur3's finish: every bit of `hash` mixed into every other.
#[inline]
fn finish(mut hash: u64) -> u64 {
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^ (hash >> 33)
}

/// A hash's bucket (`DISPLACEMENTS`' index, the top 10 bits) and its two
/// values the bucket's displacements place it by.
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the hash's bits taken apart"
)]
pub(crate) fn name_hash_parts(hash: u64) -> (usize, u32, u32) {
    let bucket = (hash >> 54) as usize;
    let f1 = (hash >> 23) as u32 | 1;
    let f2 = hash as u32;
    (bucket, f1, f2)
}

/// Where in a table of `mask + 1` slots a hash's parts land, displaced by
/// `(d1, d2)`.
#[inline]
pub(crate) fn name_hash_slot(f1: u32, f2: u32, (d1, d2): (u16, u16), mask: u32) -> usize {
    (f2.wrapping_add(f1.wrapping_mul(u32::from(d1)))
        .wrapping_add(u32::from(d2))
        & mask) as usize
}
