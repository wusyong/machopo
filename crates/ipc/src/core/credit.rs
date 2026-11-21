//! The arithmetic of a pipe's credit, proved with Verus: how much may
//! still be sent ([`room`]), what a send and a report of bytes read leave
//! unread ([`sent`], [`read`]), and when the reader reports ([`report`],
//! [`threshold`]). A mistake here is a sender past its pipe's capacity, or
//! one that waits for a report that never comes.
//!
//! What is inside `verus!` is what runs: a plain build erases the specs and
//! proofs and compiles the rest. `python3 scripts/test.py ipc verus` checks
//! the proofs (`cargo verus verify -p ipc`).

use vstd::prelude::*;

verus! {

/// The bytes a sender may still send: its pipe's capacity less what it
/// sent that the reader hasn't said it read.
#[expect(clippy::implicit_saturating_sub, reason = "the two cases are the two the proof is of")]
pub fn room(capacity: u64, unread: u64) -> (room: u64)
    ensures
        unread <= capacity ==> room == capacity - unread,
        unread > capacity ==> room == 0,
{
    if unread <= capacity {
        #[expect(clippy::arithmetic_side_effects, reason = "Verus proves it: unread <= capacity")]
        {
            capacity - unread
        }
    } else {
        0
    }
}

/// What is unread once `len` more bytes are sent: `None` when that is
/// more than the pipe holds.
pub fn sent(capacity: u64, unread: u64, len: u64) -> (after: Option<u64>)
    ensures
        match after {
            Some(unread_after) => unread_after == unread + len && unread_after <= capacity,
            None => unread + len > capacity,
        },
{
    if unread <= capacity && len <= room(capacity, unread) {
        #[expect(clippy::arithmetic_side_effects, reason = "Verus proves it: unread + len <= capacity")]
        {
            Some(unread + len)
        }
    } else {
        None
    }
}

/// What is unread once the reader says it read `count` more bytes: `None`
/// when it wasn't sent that many.
pub fn read(unread: u64, count: u64) -> (after: Option<u64>)
    ensures
        match after {
            Some(unread_after) => unread_after == unread - count,
            None => count > unread,
        },
{
    if count <= unread {
        #[expect(clippy::arithmetic_side_effects, reason = "Verus proves it: count <= unread")]
        {
            Some(unread - count)
        }
    } else {
        None
    }
}

/// How many bytes a reader lets go unreported before it reports: half the
/// pipe's capacity, and never none. No more than the capacity, so a
/// sender out of room has a report coming once the reader has read that
/// much of what it holds.
pub fn threshold(capacity: u64) -> (threshold: u64)
    ensures
        threshold >= 1,
        capacity >= 1 ==> threshold <= capacity,
        capacity >= 2 ==> threshold == capacity / 2,
{
    if capacity >= 2 {
        capacity / 2
    } else {
        1
    }
}

/// A reader that had `unreported` bytes read and not yet reported reads
/// `count` more: what is unreported after, and the count to report now,
/// if it reached `threshold`. Every byte read is reported once. (Counts
/// past what a `u64` holds, which no pipe reaches, are reported as the
/// most one holds.)
pub fn report(threshold: u64, unreported: u64, count: u64) -> (after: (u64, Option<u64>))
    ensures
        unreported + count <= u64::MAX ==> match after.1 {
            Some(reported) => reported == unreported + count && reported >= threshold && after.0 == 0,
            None => after.0 == unreported + count && after.0 < threshold,
        },
{
    match unreported.checked_add(count) {
        Some(total) => if total >= threshold {
            (0, Some(total))
        } else {
            (total, None)
        },
        None => (0, Some(u64::MAX)),
    }
}

} // verus!
