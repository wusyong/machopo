//! The arithmetic a pool of regions rests on, proved with Verus: the size
//! a region is made at for a length ([`class_len`]), which free region a
//! length goes in ([`best_fit`]), and whether one more region stays under
//! what a peer may have ([`admits`]). A mistake here (a region a byte too
//! short for what is written into it, one handed out twice) is a write
//! past a mapping or two messages' bytes in one place, and shows to no
//! test that doesn't happen on the length it takes.
//!
//! What is inside `verus!` is what runs: a plain build erases the specs and
//! proofs and compiles the rest. `python3 scripts/test.py ipc verus` checks
//! the proofs (`cargo verus verify -p ipc`).

use vstd::prelude::*;

verus! {

/// The smallest region: what is shorter goes in the message itself.
pub const MIN_REGION_LEN: usize = 0x1_0000;

/// The largest region.
pub const MAX_REGION_LEN: usize = 0x4000_0000;

/// A length regions are made at: [`MIN_REGION_LEN`] doubled some number of
/// times, up to [`MAX_REGION_LEN`].
pub open spec fn is_class(n: int) -> bool
    decreases n,
{
    n == MIN_REGION_LEN || (MIN_REGION_LEN < n <= MAX_REGION_LEN && n % 2 == 0 && is_class(n / 2))
}

/// The length a region for `len` bytes is made at: the smallest class
/// that holds them, or `None` when no region is that large.
pub fn class_len(len: usize) -> (class: Option<usize>)
    ensures
        match class {
            Some(c) => is_class(c as int) && len <= c && (c == MIN_REGION_LEN || c / 2 < len),
            None => len > MAX_REGION_LEN,
        },
{
    if len > MAX_REGION_LEN {
        return None;
    }
    let mut c = MIN_REGION_LEN;
    while c < len
        invariant
            len <= MAX_REGION_LEN,
            MIN_REGION_LEN <= c <= MAX_REGION_LEN,
            is_class(c as int),
            c == MIN_REGION_LEN || c / 2 < len,
        decreases MAX_REGION_LEN - c,
    {
        proof {
            // `c < len <= MAX` and both ends are classes: the next class
            // is no more than MAX.
            lemma_class_below_max_doubles(c as int);
        }
        #[expect(clippy::arithmetic_side_effects, reason = "Verus proves it: 2 * c <= MAX_REGION_LEN")]
        {
            c *= 2;
        }
    }
    Some(c)
}

/// A class under the largest has its double at or under it.
proof fn lemma_class_below_max_doubles(c: int)
    requires
        is_class(c),
        c < MAX_REGION_LEN,
    ensures
        2 * c <= MAX_REGION_LEN,
        is_class(2 * c),
{
    lemma_class_divides_max(c);
    assert(2 * c > MIN_REGION_LEN);
    assert((2 * c) % 2 == 0);
    assert((2 * c) / 2 == c);
}

/// A power of two, `2^k`.
spec fn pow2(k: nat) -> int
    decreases k,
{
    if k == 0 {
        1
    } else {
        2 * pow2((k - 1) as nat)
    }
}

/// Every class is `2^k` for a `k` from 16 to 30, so one under the largest
/// is at most half of it.
proof fn lemma_class_divides_max(c: int)
    requires
        is_class(c),
        c < MAX_REGION_LEN,
    ensures
        2 * c <= MAX_REGION_LEN,
{
    lemma_class_is_pow2(c);
    let k = choose|k: nat| 16 <= k <= 30 && c == pow2(k);
    lemma_pow2_values();
    lemma_pow2_monotonic(k, 30);
    if k == 30 {
        assert(false);
    } else {
        lemma_pow2_monotonic(k + 1, 30);
        assert(pow2(k + 1) == 2 * pow2(k));
    }
}

proof fn lemma_pow2_values()
    ensures
        pow2(16) == 0x1_0000,
        pow2(30) == 0x4000_0000,
{
    reveal_with_fuel(pow2, 32);
}

proof fn lemma_pow2_monotonic(a: nat, b: nat)
    requires
        a <= b,
    ensures
        pow2(a) <= pow2(b),
        pow2(a) > 0,
    decreases b,
{
    if a == 0 {
        if b > 0 {
            lemma_pow2_monotonic(0, (b - 1) as nat);
        }
    } else if a < b {
        lemma_pow2_monotonic(a, (b - 1) as nat);
    } else {
        lemma_pow2_monotonic((a - 1) as nat, (a - 1) as nat);
    }
}

proof fn lemma_class_is_pow2(c: int)
    requires
        is_class(c),
    ensures
        exists|k: nat| 16 <= k <= 30 && c == pow2(k),
    decreases c,
{
    lemma_pow2_values();
    if c == MIN_REGION_LEN {
        assert(c == pow2(16));
    } else {
        lemma_class_is_pow2(c / 2);
        let k = choose|k: nat| 16 <= k <= 30 && c / 2 == pow2(k);
        assert(c == pow2(k + 1));
        if k == 30 {
            assert(false);
        }
    }
}

/// A region as the pool's search sees it: how long it is, and whether
/// something written into it is still the peer's to read.
pub struct Slot {
    pub len: usize,
    pub out: bool,
}

/// Slot `i` can take `want` bytes: nothing is out in it, and it is long
/// enough.
pub open spec fn takes(slots: Seq<Slot>, i: int, want: usize) -> bool {
    !slots[i].out && slots[i].len >= want
}

/// The free region `want` bytes go in: the shortest that holds them (the
/// first of those), or `None` when no free one does.
pub fn best_fit(slots: &[Slot], want: usize) -> (found: Option<usize>)
    ensures
        match found {
            Some(i) => i < slots.len() && takes(slots@, i as int, want) && forall|j: int|
                0 <= j < slots.len() && #[trigger] takes(slots@, j, want) ==> slots[i as int].len <= slots[j].len,
            None => forall|j: int| 0 <= j < slots.len() ==> !#[trigger] takes(slots@, j, want),
        },
{
    let mut found: Option<usize> = None;
    let mut i = 0;
    while i < slots.len()
        invariant
            i <= slots.len(),
            match found {
                Some(b) => b < i && takes(slots@, b as int, want) && forall|j: int|
                    0 <= j < i && #[trigger] takes(slots@, j, want) ==> slots[b as int].len <= slots[j].len,
                None => forall|j: int| 0 <= j < i ==> !#[trigger] takes(slots@, j, want),
            },
        decreases slots.len() - i,
    {
        let slot = &slots[i];
        if !slot.out && slot.len >= want {
            let better = match found {
                Some(b) => slot.len < slots[b].len,
                None => true,
            };
            if better {
                found = Some(i);
            }
        }
        #[expect(clippy::arithmetic_side_effects, reason = "Verus proves it: i < slots.len()")]
        {
            i += 1;
        }
    }
    found
}

/// Whether a peer holding `total` bytes of regions, of the `limit` it may,
/// can be given `len` more.
pub fn admits(total: usize, limit: usize, len: usize) -> (ok: bool)
    ensures
        ok == (total + len <= limit),
{
    match total.checked_add(len) {
        Some(sum) => sum <= limit,
        None => false,
    }
}

} // verus!
