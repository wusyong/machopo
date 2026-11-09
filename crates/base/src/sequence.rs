//! Sequence numbers, for IDs and counters (Chromium's `base::
//! AtomicSequenceNumber`, its `GetNext` is `get_next`): each call hands out the next one, and running
//! out panics rather than wrapping, which would hand out a number in use.
//! An ID that is to come round again says how itself.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// A number that counts up by one: what `get_next()` hands out.
pub trait Counter: Copy + std::fmt::Debug {
    const ZERO: Self;
    fn checked_increment(self) -> Option<Self>;
}

macro_rules! counter {
    ($($int:ty),*) => {$(
        impl Counter for $int {
            const ZERO: Self = 0;
            fn checked_increment(self) -> Option<Self> {
                self.checked_add(1)
            }
        }
    )*};
}

counter!(u8, u16, u32, u64, usize, i32, i64);

/// The numbers from a first one up, one at a time, on one thread: it panics
/// once there is no next one (its type's largest is never handed out).
#[derive(Debug, Clone)]
pub struct SequenceNumber<T> {
    next: T,
}

impl<T: Counter> SequenceNumber<T> {
    /// From 0.
    pub const fn new() -> Self {
        Self { next: T::ZERO }
    }

    /// From `first`.
    pub const fn starting_at(first: T) -> Self {
        Self { next: first }
    }

    /// The next number.
    ///
    /// # Panics
    ///
    /// If its type has no number after it.
    pub fn get_next(&mut self) -> T {
        let value = self.next;
        self.next = value
            .checked_increment()
            .expect("sequence numbers exhausted");
        value
    }

    /// What `get_next` hands out next, without handing it out.
    pub fn peek(&self) -> T {
        self.next
    }
}

impl<T: Counter> Default for SequenceNumber<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// [`SequenceNumber`] for any thread, in a `static` (Chromium's
/// `base::AtomicSequenceNumber`): no number is handed out twice, whichever
/// thread asks; it panics once there is no next one.
#[derive(Debug)]
pub struct AtomicSequenceNumber<T: AtomicCounter> {
    next: T::Atomic,
}

/// A number [`AtomicSequenceNumber`] hands out, and the atomic it keeps it
/// in.
pub trait AtomicCounter: Counter {
    type Atomic;
    fn update(atomic: &Self::Atomic, f: impl FnMut(Self) -> Option<Self>) -> Result<Self, Self>;
}

macro_rules! atomic_counter {
    ($($int:ty => $atomic:ty),*) => {$(
        impl AtomicCounter for $int {
            type Atomic = $atomic;
            fn update(
                atomic: &$atomic,
                f: impl FnMut(Self) -> Option<Self>,
            ) -> Result<Self, Self> {
                // No other memory waits on it: only the number is shared.
                atomic.try_update(Ordering::Relaxed, Ordering::Relaxed, f)
            }
        }

        impl AtomicSequenceNumber<$int> {
            /// From 0.
            pub const fn new() -> Self {
                Self { next: <$atomic>::new(0) }
            }

            /// From `first`.
            pub const fn starting_at(first: $int) -> Self {
                Self { next: <$atomic>::new(first) }
            }
        }

        impl Default for AtomicSequenceNumber<$int> {
            fn default() -> Self {
                Self::new()
            }
        }
    )*};
}

atomic_counter!(u32 => AtomicU32, u64 => AtomicU64, usize => AtomicUsize);

impl<T: AtomicCounter> AtomicSequenceNumber<T> {
    /// The next number.
    ///
    /// # Panics
    ///
    /// If its type has no number after it.
    pub fn get_next(&self) -> T {
        T::update(&self.next, T::checked_increment).expect("sequence numbers exhausted")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_after_another() {
        let mut numbers = SequenceNumber::<u32>::new();
        assert_eq!(
            (numbers.get_next(), numbers.get_next(), numbers.peek()),
            (0, 1, 2)
        );
        let mut ids = SequenceNumber::starting_at(7_u64);
        assert_eq!((ids.get_next(), ids.get_next()), (7, 8));
        let atomic = AtomicSequenceNumber::<u32>::starting_at(1);
        assert_eq!((atomic.get_next(), atomic.get_next()), (1, 2));
        assert_eq!(AtomicSequenceNumber::<usize>::new().get_next(), 0);
    }

    #[test]
    #[should_panic(expected = "sequence numbers exhausted")]
    fn running_out_panics() {
        let mut numbers = SequenceNumber::starting_at(u8::MAX - 1);
        assert_eq!(numbers.get_next(), u8::MAX - 1);
        numbers.get_next();
    }

    #[test]
    #[should_panic(expected = "sequence numbers exhausted")]
    fn running_out_panics_on_any_thread() {
        let numbers = AtomicSequenceNumber::<u32>::starting_at(u32::MAX - 1);
        assert_eq!(numbers.get_next(), u32::MAX - 1);
        numbers.get_next();
    }

    #[test]
    fn no_number_twice_across_threads() {
        static NUMBERS: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::new();
        let handed: Vec<u64> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| (0..1000).map(|_| NUMBERS.get_next()).collect::<Vec<_>>()))
                .collect();
            threads
                .into_iter()
                .flat_map(|t| t.join().unwrap())
                .collect()
        });
        let mut sorted = handed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 4000);
    }
}
