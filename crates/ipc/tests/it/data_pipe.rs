//! `ipc::data_pipe`: what is written comes out in order, at most the
//! capacity unread at a time; the consumer is told when there is more; each
//! end sees the other go.
//!
//! The producer's write is polled by hand (a no-op waker): whether it is
//! still waiting is then exact, with no threads or timing.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use bytes::Bytes;
use ipc::data_pipe::{self, Closed, Read};

fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

/// Writes `data` with nothing waiting on the other end: it has to fit.
fn write_now(producer: &mut data_pipe::Producer, data: &'static [u8]) {
    let mut write = pin!(producer.write(Bytes::from_static(data)));
    assert_eq!(poll(write.as_mut()), Poll::Ready(Ok(())));
}

fn data(read: Read) -> Vec<u8> {
    match read {
        Read::Data(bytes) => bytes.to_vec(),
        other => panic!("expected data, got {other:?}"),
    }
}

/// How many times a watcher was called.
fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    (calls, move || {
        #[expect(clippy::disallowed_methods, reason = "a test's count of a few calls")]
        counted.fetch_add(1, Ordering::SeqCst);
    })
}

#[test]
fn what_is_written_comes_out_in_order() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    write_now(&mut producer, b"hello, ");
    write_now(&mut producer, b"world");
    assert_eq!(data(consumer.read(64)), b"hello, ");
    assert_eq!(data(consumer.read(64)), b"world");
    assert_eq!(consumer.read(64), Read::Empty);
}

#[test]
fn a_read_takes_at_most_max_bytes() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    write_now(&mut producer, b"abcdef");
    assert_eq!(data(consumer.read(4)), b"abcd");
    assert_eq!(data(consumer.read(4)), b"ef");
    // At least one byte, whatever it asks for.
    write_now(&mut producer, b"gh");
    assert_eq!(data(consumer.read(0)), b"g");
}

#[test]
fn a_write_waits_for_room_and_never_overfills() {
    let (mut producer, mut consumer) = data_pipe::create(4);
    let mut write = pin!(producer.write(Bytes::from_static(b"0123456789")));
    // As much as fits goes in at once; the rest waits.
    assert!(poll(write.as_mut()).is_pending());
    assert_eq!(data(consumer.read(100)), b"0123");
    assert_eq!(consumer.read(100), Read::Empty);
    // The next four.
    assert!(poll(write.as_mut()).is_pending());
    assert_eq!(data(consumer.read(1)), b"4");
    // One byte of room: one more byte goes in, no more.
    assert!(poll(write.as_mut()).is_pending());
    assert_eq!(data(consumer.read(100)), b"567");
    assert_eq!(data(consumer.read(100)), b"8");
    assert_eq!(poll(write.as_mut()), Poll::Ready(Ok(())));
    assert_eq!(data(consumer.read(100)), b"9");
    assert_eq!(consumer.read(100), Read::Empty);
}

#[test]
fn a_pipe_holds_at_least_a_byte() {
    let (mut producer, mut consumer) = data_pipe::create(0);
    write_now(&mut producer, b"a");
    assert_eq!(data(consumer.read(8)), b"a");
}

#[test]
fn the_end_comes_after_what_was_written() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    write_now(&mut producer, b"last");
    drop(producer);
    assert_eq!(data(consumer.read(64)), b"last");
    assert_eq!(consumer.read(64), Read::Done);
    assert_eq!(consumer.read(64), Read::Done);
}

#[test]
fn an_empty_write_is_nothing() {
    let (mut producer, mut consumer) = data_pipe::create(1);
    write_now(&mut producer, b"x");
    // Even with the pipe full.
    write_now(&mut producer, b"");
    assert_eq!(data(consumer.read(8)), b"x");
    assert_eq!(consumer.read(8), Read::Empty);
}

#[test]
fn a_write_fails_once_the_consumer_is_gone() {
    let (mut producer, consumer) = data_pipe::create(2);
    {
        let mut write = pin!(producer.write(Bytes::from_static(b"abc")));
        assert!(poll(write.as_mut()).is_pending());
        // Waiting for room that never comes: it ends.
        drop(consumer);
        assert_eq!(poll(write.as_mut()), Poll::Ready(Err(Closed)));
    }
    let mut write = pin!(producer.write(Bytes::from_static(b"d")));
    assert_eq!(poll(write.as_mut()), Poll::Ready(Err(Closed)));
}

#[test]
fn the_watcher_is_told_once_the_empty_pipe_has_data() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    let (calls, on_readable) = counter();
    consumer.watch(on_readable);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    write_now(&mut producer, b"a");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // Not again until a read finds the pipe empty.
    write_now(&mut producer, b"b");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(data(consumer.read(1)), b"a");
    write_now(&mut producer, b"c");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(data(consumer.read(64)), b"b");
    assert_eq!(data(consumer.read(64)), b"c");
    assert_eq!(consumer.read(64), Read::Empty);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    write_now(&mut producer, b"d");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_watcher_is_told_at_once_of_what_is_there() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    write_now(&mut producer, b"a");
    let (calls, on_readable) = counter();
    consumer.watch(on_readable);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    write_now(&mut producer, b"b");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn the_watcher_is_told_of_the_end() {
    let (producer, mut consumer) = data_pipe::create(64);
    let (calls, on_readable) = counter();
    consumer.watch(on_readable);
    drop(producer);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(consumer.read(64), Read::Done);

    // An end that came before watching is there at once.
    let (producer, mut consumer) = data_pipe::create(64);
    drop(producer);
    let (calls, on_readable) = counter();
    consumer.watch(on_readable);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_new_watcher_replaces_the_old() {
    let (mut producer, mut consumer) = data_pipe::create(64);
    let (old_calls, old) = counter();
    consumer.watch(old);
    let (new_calls, new) = counter();
    consumer.watch(new);
    write_now(&mut producer, b"a");
    assert_eq!(old_calls.load(Ordering::SeqCst), 0);
    assert_eq!(new_calls.load(Ordering::SeqCst), 1);
}

/// A producer on a tokio runtime of its own thread, and a consumer told by
/// its watcher: all of a body comes through a pipe much smaller than it.
#[test]
fn a_body_goes_from_one_thread_to_another() {
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let (mut producer, mut consumer) = data_pipe::create(4096);
    let sent = Bytes::from(body.clone());
    let writer = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime");
        runtime.block_on(async move {
            for chunk in sent.chunks(10_000) {
                producer
                    .write(sent.slice_ref(chunk))
                    .await
                    .expect("the consumer reads it all");
            }
        });
    });
    let (readable, wait) = std::sync::mpsc::channel();
    consumer.watch(move || {
        let _ = readable.send(());
    });
    let mut received = Vec::new();
    loop {
        match consumer.read(1500) {
            Read::Data(bytes) => received.extend_from_slice(&bytes),
            Read::Empty => wait
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("told of more"),
            Read::Done => break,
        }
    }
    writer.join().expect("the writer");
    assert_eq!(received, body);
}

/// Whether a waker was woken.
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Polls `future` with a waker that says whether it was woken since.
fn poll_watched<F: Future>(future: Pin<&mut F>) -> (Poll<F::Output>, Arc<Woken>) {
    let woken = Arc::new(Woken(AtomicBool::new(false)));
    let waker = Waker::from(woken.clone());
    (future.poll(&mut Context::from_waker(&waker)), woken)
}

#[test]
fn a_waiting_write_is_woken_by_a_read() {
    let (mut producer, mut consumer) = data_pipe::create(2);
    let mut write = pin!(producer.write(Bytes::from_static(b"abc")));
    let (polled, woken) = poll_watched(write.as_mut());
    assert!(polled.is_pending());
    assert!(!woken.0.load(Ordering::SeqCst));
    assert_eq!(data(consumer.read(1)), b"a");
    assert!(woken.0.load(Ordering::SeqCst));
    assert_eq!(poll(write.as_mut()), Poll::Ready(Ok(())));
}

#[test]
fn a_waiting_write_is_woken_when_the_consumer_goes() {
    let (mut producer, consumer) = data_pipe::create(2);
    let mut write = pin!(producer.write(Bytes::from_static(b"abc")));
    let (polled, woken) = poll_watched(write.as_mut());
    assert!(polled.is_pending());
    drop(consumer);
    assert!(woken.0.load(Ordering::SeqCst));
    assert_eq!(poll(write.as_mut()), Poll::Ready(Err(Closed)));
}
