//! A profile database's thread (the cookie store's, the HTTP cache's): told
//! to close as machopo ends, and waited for with a deadline; one given up
//! on then isn't waited for again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use crossbeam_channel::{Receiver, RecvTimeoutError};

pub(crate) struct DatabaseThread {
    thread: Option<JoinHandle<()>>,
    /// Disconnected as the thread ends: what [`ended_by`](Self::ended_by)
    /// waits for, with a deadline a join doesn't have.
    ended: Receiver<()>,
    /// Told to close as machopo ends ([`closing`](Self::closing)); set
    /// through the network context's shared handle, so atomic.
    closed: AtomicBool,
}

impl DatabaseThread {
    /// Runs `run` on a thread named `name`.
    pub(crate) fn spawn(name: &str, run: impl FnOnce() + Send + 'static) -> std::io::Result<Self> {
        let (end, ended) = crossbeam_channel::bounded::<()>(0);
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                // Dropped as the thread ends, a panic's unwinding too.
                let _end = end;
                run();
            })?;
        Ok(Self {
            thread: Some(thread),
            ended,
            closed: AtomicBool::new(false),
        })
    }

    /// The thread is being told to close as machopo ends.
    pub(crate) fn closing(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    /// Whether the thread has ended by `deadline`.
    pub(crate) fn ended_by(&self, deadline: Instant) -> bool {
        matches!(
            self.ended.recv_deadline(deadline),
            Err(RecvTimeoutError::Disconnected)
        )
    }

    /// Waits for the thread to end (it has been told to), unless it was
    /// told to close as machopo ends and hasn't yet: that one was given up
    /// on, and machopo ends without it, else the end would wait for a
    /// disk that hangs after all. Never on the thread itself: nothing of
    /// its holds this.
    pub(crate) fn join(&mut self) {
        if *self.closed.get_mut() && !self.ended_by(Instant::now()) {
            return;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A thread told to wait for nothing more is joined.
    #[test]
    fn a_thread_is_waited_for() {
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let mut thread = DatabaseThread::spawn("held", move || {
            let _ = held.recv();
        })
        .unwrap();
        assert!(!thread.ended_by(Instant::now() + Duration::from_millis(20)));
        drop(release);
        thread.join();
        assert!(thread.ended_by(Instant::now()));
    }

    /// One closed as machopo ends that hasn't ended by then is let go: the
    /// join comes back though the thread never ends.
    #[test]
    fn a_closed_thread_given_up_on_is_let_go() {
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let thread = DatabaseThread::spawn("hung", move || {
            let _ = held.recv();
        })
        .unwrap();
        thread.closing();
        assert!(!thread.ended_by(Instant::now() + Duration::from_millis(20)));
        let joined = std::thread::spawn(move || {
            let mut thread = thread;
            thread.join();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !joined.is_finished() {
            assert!(Instant::now() < deadline, "the join waited for it");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(release);
    }
}
