//! One turn of a store: its future polled until nothing in it moves.
//! A store runs only when it has something to do (a script to run, an event
//! for it, a wait of its guest's over) and is put back after; what the
//! guest's tasks wait for is the embedder's to know, so nothing outside the
//! turn needs its waker.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

/// Whether the future asked to be polled again (a guest that yields, a
/// host function's future that became ready in this poll).
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Polls `future` until it is done, or pending with nothing more to do in
/// this turn (it didn't wake its waker during the last poll).
pub fn poll_until_idle<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    let woken = Arc::new(Woken(AtomicBool::new(false)));
    let waker = Waker::from(woken.clone());
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return Poll::Ready(output);
        }
        if !woken.0.swap(false, Ordering::Relaxed) {
            return Poll::Pending;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::pin;

    use super::*;

    /// Pending `wakes` times, waking itself each time, then pending
    /// without waking `sleeps` times, then done; counts its polls.
    struct Steps {
        wakes: u32,
        sleeps: u32,
        polls: u32,
    }

    impl Future for Steps {
        type Output = u32;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
            self.polls += 1;
            if self.wakes > 0 {
                self.wakes -= 1;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else if self.sleeps > 0 {
                self.sleeps -= 1;
                Poll::Pending
            } else {
                Poll::Ready(self.polls)
            }
        }
    }

    #[test]
    fn a_future_that_wakes_itself_is_polled_again() {
        let future = pin!(Steps {
            wakes: 3,
            sleeps: 0,
            polls: 0
        });
        assert_eq!(poll_until_idle(future), Poll::Ready(4));
    }

    #[test]
    fn a_turn_ends_at_a_poll_that_woke_nothing() {
        let mut future = pin!(Steps {
            wakes: 2,
            sleeps: 1,
            polls: 0
        });
        assert_eq!(poll_until_idle(future.as_mut()), Poll::Pending);
        assert_eq!(future.polls, 3);
        // The next turn takes it from there; a wake of the turn before
        // doesn't carry over.
        assert_eq!(poll_until_idle(future.as_mut()), Poll::Ready(4));
    }
}
