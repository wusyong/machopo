//! WASI 0.3 for the guests (their libc imports it): wasmtime-wasi's, but
//! for the monotonic clock, which is here. wasmtime-wasi's waits on tokio's
//! timers, and the thread that runs the guests has no tokio runtime: a
//! guest's wait is a deadline the embedder reads (`GuestClock::next_deadline`)
//! and runs the store again at (`GuestClock::wake_due`), as it does its own
//! timers. Standard output and error go to the embedder's sink a line at a
//! time; there are no files, no sockets and no arguments.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use tokio::io::AsyncWrite;
use wasmtime::component::{Accessor, HasData, Linker};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::clocks::{WasiClocks, WasiClocksView};
use wasmtime_wasi::p3::bindings::clocks::{monotonic_clock, system_clock, types};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};

/// What the guest's clock reads to (nanoseconds): `performance.now()`'s
/// coarsening, so a guest times nothing finer than a page's script does.
pub const CLOCK_RESOLUTION: u64 = 100_000;

/// The guest's monotonic clock: the embedder's time (nanoseconds from
/// wherever it starts), and the waits the guest's tasks are in.
pub struct GuestClock {
    now: Box<dyn Fn() -> u64 + Send>,
    waits: Vec<Wait>,
    next_wait: u64,
}

/// A task waiting for `deadline`. `waker` is wasmtime's for the host
/// function's future (it marks the future to be polled in the store's next
/// turn), not the turn's own, which goes with the turn.
struct Wait {
    id: u64,
    deadline: u64,
    waker: Waker,
}

impl GuestClock {
    /// A clock reading `now` (nanoseconds, never going back).
    pub fn new(now: impl Fn() -> u64 + Send + 'static) -> Self {
        Self {
            now: Box::new(now),
            waits: Vec::new(),
            next_wait: 0,
        }
    }

    /// The earliest deadline a task of the guest waits for, on the
    /// embedder's time: when to run the store again.
    pub fn next_deadline(&self) -> Option<u64> {
        self.waits.iter().map(|wait| wait.deadline).min()
    }

    /// Wakes the waits whose deadlines have come, before a turn of the
    /// store (which then polls them); whether there were any.
    pub fn wake_due(&mut self) -> bool {
        let now = (self.now)();
        let before = self.waits.len();
        self.waits.retain(|wait| {
            let due = wait.deadline <= now;
            if due {
                wait.waker.wake_by_ref();
            }
            !due
        });
        self.waits.len() != before
    }

    /// One poll of a wait for `deadline`: done once the time has come, else
    /// kept (under `id`, given at its first poll) with the waker to wake
    /// then. A wait dropped before (its task cancelled) stays until its
    /// deadline, and costs one turn that finds nothing to do.
    fn poll_wait(&mut self, id: &mut Option<u64>, deadline: u64, waker: &Waker) -> Poll<()> {
        if deadline <= (self.now)() {
            if let Some(id) = *id {
                self.waits.retain(|wait| wait.id != id);
            }
            return Poll::Ready(());
        }
        match *id {
            // Still kept: only the waker may have changed.
            Some(known) => match self.waits.iter_mut().find(|wait| wait.id == known) {
                Some(wait) => wait.waker.clone_from(waker),
                // `wake_due` took it, and the time went back: kept again.
                None => self.waits.push(Wait {
                    id: known,
                    deadline,
                    waker: waker.clone(),
                }),
            },
            None => {
                let new = self.next_wait;
                // One per wait: a store making 2^64 of them isn't one.
                self.next_wait = new.wrapping_add(1);
                *id = Some(new);
                self.waits.push(Wait {
                    id: new,
                    deadline,
                    waker: waker.clone(),
                });
            }
        }
        Poll::Pending
    }
}

/// A store's data with the guest's clock.
pub trait GuestClockView: Send {
    fn guest_clock(&mut self) -> &mut GuestClock;
}

/// `wasi:clocks/monotonic-clock` on a `GuestClock`.
struct GuestClocks;

impl HasData for GuestClocks {
    type Data<'a> = &'a mut GuestClock;
}

impl monotonic_clock::Host for &mut GuestClock {
    fn now(&mut self) -> wasmtime::Result<monotonic_clock::Mark> {
        let now = (self.now)();
        // Down to the resolution: the remainder is below `now`.
        Ok(now - now % CLOCK_RESOLUTION)
    }

    fn get_resolution(&mut self) -> wasmtime::Result<types::Duration> {
        Ok(CLOCK_RESOLUTION)
    }
}

/// Waits until `deadline` of the store's clock, the store borrowed at each
/// poll only.
async fn wait<U>(store: &Accessor<U, GuestClocks>, deadline: u64) {
    let mut id = None;
    std::future::poll_fn(|cx| {
        store.with(|mut access| access.get().poll_wait(&mut id, deadline, cx.waker()))
    })
    .await;
}

impl<U> monotonic_clock::HostWithStore<U> for GuestClocks {
    async fn wait_until(
        store: &Accessor<U, Self>,
        when: monotonic_clock::Mark,
    ) -> wasmtime::Result<()> {
        wait(store, when).await;
        Ok(())
    }

    async fn wait_for(store: &Accessor<U, Self>, duration: types::Duration) -> wasmtime::Result<()> {
        // A wait past the clock's end never ends, as its deadline never comes.
        let deadline = store.with(|mut access| (access.get().now)().saturating_add(duration));
        wait(store, deadline).await;
        Ok(())
    }
}

/// Which of the guest's outputs a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Stdout,
    Stderr,
}

/// Where the guest's output goes: each line, without its newline.
pub type OutputSink = Arc<dyn Fn(Output, &str) + Send + Sync>;

/// The guest's standard output or error, to the sink.
struct OutputStream {
    output: Output,
    sink: OutputSink,
}

impl IsTerminal for OutputStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for OutputStream {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(LineWriter {
            output: self.output,
            sink: self.sink.clone(),
            line: Vec::new(),
        })
    }
}

/// Gives the sink what is written a line at a time, at once (it never
/// waits); a last line with no newline goes as the writer does (the
/// guest's stream closed, or its store gone): WASI's streams have no flush.
struct LineWriter {
    output: Output,
    sink: OutputSink,
    line: Vec<u8>,
}

impl LineWriter {
    fn give(&mut self) {
        (self.sink)(self.output, &String::from_utf8_lossy(&self.line));
        self.line.clear();
    }
}

impl AsyncWrite for LineWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        for &byte in bytes {
            if byte == b'\n' {
                self.give();
            } else {
                self.line.push(byte);
            }
        }
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        if !self.line.is_empty() {
            self.give();
        }
    }
}

/// A store's WASI: wasmtime-wasi's context (the output to `sink`, nothing
/// else given) and the guest's clock.
pub struct WasmWasi {
    ctx: WasiCtx,
    table: ResourceTable,
    clock: GuestClock,
}

impl WasmWasi {
    /// `now` is the embedder's monotonic time, in nanoseconds.
    pub fn new(now: impl Fn() -> u64 + Send + 'static, sink: OutputSink) -> Self {
        let ctx = WasiCtx::builder()
            .stdout(OutputStream {
                output: Output::Stdout,
                sink: sink.clone(),
            })
            .stderr(OutputStream {
                output: Output::Stderr,
                sink,
            })
            .build();
        Self {
            ctx,
            table: ResourceTable::new(),
            clock: GuestClock::new(now),
        }
    }

    /// For the data's `WasiView`.
    pub fn view(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }

    pub fn clock(&mut self) -> &mut GuestClock {
        &mut self.clock
    }
}

/// Adds WASI 0.3 to `linker`: wasmtime-wasi's interfaces, the monotonic
/// clock's replaced by the guest's clock.
pub fn add_wasi_to_linker<T>(linker: &mut Linker<T>) -> wasmtime::Result<()>
where
    T: WasiView + GuestClockView + 'static,
{
    wasmtime_wasi::p3::cli::add_to_linker(linker)?;
    wasmtime_wasi::p3::random::add_to_linker(linker)?;
    wasmtime_wasi::p3::filesystem::add_to_linker(linker)?;
    wasmtime_wasi::p3::sockets::add_to_linker(linker)?;
    types::add_to_linker::<_, WasiClocks>(linker, T::clocks)?;
    system_clock::add_to_linker::<_, WasiClocks>(linker, T::clocks)?;
    monotonic_clock::add_to_linker::<_, GuestClocks>(linker, T::guest_clock)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A clock at the time `time` holds.
    fn clock(time: &Arc<AtomicU64>) -> GuestClock {
        let time = time.clone();
        GuestClock::new(move || time.load(Ordering::Relaxed))
    }

    #[test]
    fn the_next_deadline_is_the_earliest_wait() {
        let time = Arc::new(AtomicU64::new(100));
        let mut clock = clock(&time);
        let (mut first, mut second, mut third) = (None, None, None);
        assert!(clock.poll_wait(&mut first, 500, Waker::noop()).is_pending());
        assert!(clock.poll_wait(&mut second, 300, Waker::noop()).is_pending());
        assert!(clock.poll_wait(&mut third, 400, Waker::noop()).is_pending());
        assert_eq!(clock.next_deadline(), Some(300));
        // Polled again, a wait is still one wait.
        assert!(clock.poll_wait(&mut second, 300, Waker::noop()).is_pending());
        assert_eq!(clock.waits.len(), 3);

        time.store(300, Ordering::Relaxed);
        assert!(clock.wake_due());
        assert_eq!(clock.next_deadline(), Some(400));
        assert!(clock.poll_wait(&mut second, 300, Waker::noop()).is_ready());
        assert_eq!(clock.waits.len(), 2);
    }

    /// A wait polled after its deadline with no `wake_due` between (a poll
    /// nothing asked for) is over, and no longer kept.
    #[test]
    fn a_wait_over_is_no_longer_kept() {
        let time = Arc::new(AtomicU64::new(100));
        let mut clock = clock(&time);
        let mut id = None;
        assert!(clock.poll_wait(&mut id, 200, Waker::noop()).is_pending());
        time.store(200, Ordering::Relaxed);
        assert!(clock.poll_wait(&mut id, 200, Waker::noop()).is_ready());
        assert_eq!(clock.next_deadline(), None);
        assert!(!clock.wake_due());
    }

    /// A wait woken whose time then isn't there yet (the time went back) is
    /// kept again, once.
    #[test]
    fn a_wait_woken_too_early_is_kept_again() {
        let time = Arc::new(AtomicU64::new(100));
        let mut clock = clock(&time);
        let mut id = None;
        assert!(clock.poll_wait(&mut id, 200, Waker::noop()).is_pending());
        time.store(200, Ordering::Relaxed);
        assert!(clock.wake_due());
        time.store(150, Ordering::Relaxed);
        assert!(clock.poll_wait(&mut id, 200, Waker::noop()).is_pending());
        assert!(clock.poll_wait(&mut id, 200, Waker::noop()).is_pending());
        assert_eq!(clock.waits.len(), 1);
        assert_eq!(clock.next_deadline(), Some(200));
    }
}
