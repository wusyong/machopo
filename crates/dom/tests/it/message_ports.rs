//! Message ports across DOMs (each on a thread of its own, as a page's and
//! its worker's): a port's end ships in a message with the messages that
//! wait for it, and its new port gets them, and those posted after, as its
//! thread's tasks (numbered there), that thread woken; an end closed
//! (`close()`, its port collected, its DOM gone, the message it was in
//! lost) leaves the other port getting what came before, then nothing, and
//! no longer kept alive once its thread's task for its waker ran
//! (`Cx::port_messages_arrived`, which the tests call where the host's
//! scheduler would); a port shipped through the port it is entangled with
//! is lost with the message ("doomed").

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use dom::{Cx, Dom, DomString, PortEnd, SerializedMessage, TargetRef, TaskQueue};
use elise::{Gc, Persistent, WeakPersistent};

fn message(byte: u8, ports: Vec<PortEnd>) -> SerializedMessage {
    SerializedMessage {
        bytes: vec![byte],
        transferred: None,
        ports,
    }
}

fn context<'gc>(cx: &Cx<'_, 'gc>) -> Gc<'gc, dom::ExecutionContext> {
    let document = cx.create_document(true);
    cx.window_context(cx.create_window(document))
}

/// A DOM whose port waker counts its calls.
fn counted() -> (Dom, Arc<AtomicUsize>) {
    let mut dom = Dom::with_platform(Default::default(), Arc::new(elise::InlinePlatform));
    let woken = Arc::new(AtomicUsize::new(0));
    dom.set_port_waker(Arc::new({
        let woken = woken.clone();
        #[expect(clippy::disallowed_methods, reason = "a test's count of a few wakes")]
        move || {
            woken.fetch_add(1, Ordering::SeqCst);
        }
    }));
    (dom, woken)
}

/// The bytes of the messages `context` has for the port it was given
/// (`port`), taken in order.
fn take_all<'gc>(
    cx: &Cx<'_, 'gc>,
    context: Gc<'gc, dom::ExecutionContext>,
    port: Gc<'gc, dom::EventTargetImpl>,
) -> Vec<u8> {
    let mut got = Vec::new();
    while let Some(taken) = cx.take_posted_message(context) {
        assert!(matches!(taken.target, TargetRef::EventTarget(target) if Gc::ptr_eq(target, port)));
        got.extend(taken.payload.unwrap().bytes);
    }
    got
}

/// A port shipped to another thread's DOM: what was posted to it before
/// comes there in order, then what is posted after, which wakes that
/// thread, each a message task of that DOM's; and back.
#[test]
fn a_port_s_messages_follow_it_to_another_thread() {
    let (mut here, woken_here) = counted();
    let (to_there, from_here) = mpsc::channel::<PortEnd>();
    let (to_here, from_there) = mpsc::channel::<()>();
    let (go, wait) = mpsc::channel::<()>();
    let one = here.mutate(|cx| {
        let channel = cx.create_message_channel(Some(context(cx)));
        let (one, two) = cx.message_channel_ports(channel);
        cx.post_port_message(one, message(1, Vec::new()));
        cx.post_port_message(one, message(2, Vec::new()));
        to_there.send(cx.ship_message_port(two)).unwrap();
        cx.start_message_port(one);
        Persistent::new(cx.gc, one)
    });
    let there = std::thread::spawn(move || {
        let (mut dom, woken) = counted();
        let end = from_here.recv().unwrap();
        let port = dom.mutate(|cx| {
            let context = context(cx);
            let port = cx.receive_message_ports(context, vec![end])[0];
            cx.start_message_port(port);
            assert_eq!(
                cx.next_task(context, cx.dom_now() + 1.0)
                    .map(|task| task.queue),
                Some(TaskQueue::Message)
            );
            assert_eq!(take_all(cx, context, port), [1, 2]);
            (
                Persistent::new(cx.gc, context),
                Persistent::new(cx.gc, port),
            )
        });
        to_here.send(()).unwrap();
        wait.recv().unwrap();
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        dom.mutate(|cx| {
            let (context, port) = (port.0.get(cx.gc), port.1.get(cx.gc));
            // Not numbered till the task for its waker.
            assert!(cx.take_posted_message(context).is_none());
            cx.port_messages_arrived(context);
            assert_eq!(take_all(cx, context, port), [3]);
            cx.post_port_message(port, message(4, Vec::new()));
        });
    });
    from_there.recv().unwrap();
    here.mutate(|cx| cx.post_port_message(one.get(cx.gc), message(3, Vec::new())));
    go.send(()).unwrap();
    there.join().unwrap();
    // For its message, and as its DOM went (its port's end closed).
    assert_eq!(woken_here.load(Ordering::SeqCst), 2);
    here.mutate(|cx| {
        let one = one.get(cx.gc);
        let context = cx.message_port_owner(one).unwrap();
        cx.port_messages_arrived(context);
        assert_eq!(take_all(cx, context, one), [4]);
    });
}

/// The other end closed by `close()` or its DOM going: the port gets what
/// came before, then nothing, its thread woken; its context kept, it is no
/// longer kept alive once that came.
#[test]
fn a_port_whose_other_end_closed_gets_what_came_before() {
    for drop_dom in [false, true] {
        let (mut here, woken) = counted();
        let (to_there, from_here) = mpsc::channel::<PortEnd>();
        let (kept, port, weak) = here.mutate(|cx| {
            let context = context(cx);
            let channel = cx.create_message_channel(Some(context));
            let (one, two) = cx.message_channel_ports(channel);
            cx.start_message_port(one);
            to_there.send(cx.ship_message_port(two)).unwrap();
            (
                Persistent::new(cx.gc, context),
                Persistent::new(cx.gc, one),
                WeakPersistent::new(cx.gc, one),
            )
        });
        std::thread::spawn(move || {
            let mut dom = Dom::new();
            let end = from_here.recv().unwrap();
            dom.mutate(|cx| {
                let port = cx.receive_message_ports(context(cx), vec![end])[0];
                cx.post_port_message(port, message(5, Vec::new()));
                if !drop_dom {
                    cx.close_message_port(port);
                }
            });
        })
        .join()
        .unwrap();
        // Woken for the message, and for the close.
        assert_eq!(woken.load(Ordering::SeqCst), 2);
        here.mutate(|cx| {
            let one = port.get(cx.gc);
            let context = cx.message_port_owner(one).unwrap();
            cx.port_messages_arrived(context);
            assert!(cx.has_posted_message(context));
            assert_eq!(take_all(cx, context, one), [5]);
            assert!(!cx.has_posted_message(context));
        });
        drop(port);
        here.collect();
        assert!(
            here.read(|cx| weak.get(cx.gc).is_none()),
            "drop_dom: {drop_dom}"
        );
        drop(kept);
    }
}

/// A port collected (unreachable, never started) closes its end: the port
/// it is entangled with, started, is let go once its thread's task for its
/// waker ran (woken by the close, on this thread too).
#[test]
fn a_collected_port_closes_its_end() {
    let (mut dom, woken) = counted();
    let (context, weak) = dom.mutate(|cx| {
        let context = context(cx);
        let channel = cx.create_message_channel(Some(context));
        let (one, _) = cx.message_channel_ports(channel);
        cx.start_message_port(one);
        (
            Persistent::new(cx.gc, context),
            WeakPersistent::new(cx.gc, one),
        )
    });
    dom.collect();
    // Kept by its context's list till it looks.
    assert!(dom.read(|cx| weak.get(cx.gc).is_some()));
    assert_eq!(woken.load(Ordering::SeqCst), 1);
    dom.mutate(|cx| assert!(!cx.has_posted_message(context.get(cx.gc))));
    dom.mutate(|cx| cx.port_messages_arrived(context.get(cx.gc)));
    dom.collect();
    assert!(dom.read(|cx| weak.get(cx.gc).is_none()));
}

/// A port shipped through the port it is entangled with (HTML's
/// "doomed"): the message is lost, and the pipe closes: the port is let go
/// once its thread's task for its waker ran.
#[test]
fn a_port_shipped_through_its_own_pipe_is_lost() {
    let (mut dom, _) = counted();
    let (context, weak) = dom.mutate(|cx| {
        let context = context(cx);
        let channel = cx.create_message_channel(Some(context));
        let (one, two) = cx.message_channel_ports(channel);
        cx.start_message_port(one);
        let end = cx.ship_message_port(two);
        cx.post_port_message(one, message(6, vec![end]));
        assert!(!cx.has_posted_message(context));
        cx.port_messages_arrived(context);
        (
            Persistent::new(cx.gc, context),
            WeakPersistent::new(cx.gc, one),
        )
    });
    dom.collect();
    assert!(dom.read(|cx| weak.get(cx.gc).is_none()));
    drop(context);
}

/// The ports in a message that is lost (the port it was for closed before
/// or after it came) are closed: the ports they were entangled with are
/// let go once their thread's task for its waker ran.
#[test]
fn the_ports_of_a_lost_message_close() {
    let (mut dom, _) = counted();
    let (context, weak) = dom.mutate(|cx| {
        let context = context(cx);
        let (one, two) = cx.message_channel_ports(cx.create_message_channel(Some(context)));
        let mut kept = Vec::new();
        let mut ship = || {
            let (three, four) = cx.message_channel_ports(cx.create_message_channel(Some(context)));
            cx.start_message_port(three);
            kept.push(WeakPersistent::new(cx.gc, three));
            cx.ship_message_port(four)
        };
        // Waiting for `two` as it closes.
        cx.post_port_message(one, message(8, vec![ship()]));
        cx.close_message_port(two);
        // Posted after.
        cx.post_port_message(one, message(9, vec![ship()]));
        cx.port_messages_arrived(context);
        (Persistent::new(cx.gc, context), kept)
    });
    dom.collect();
    for weak in &weak {
        assert!(dom.read(|cx| weak.get(cx.gc).is_none()));
    }
    drop(context);
}

/// A port's messages from another thread wait for its thread's task for
/// its waker; one posted through the other end once that end is shipped to
/// the port's thread (a message of its own thread's, numbered as it is
/// posted) comes after them, in the order they were posted.
#[test]
fn a_message_from_here_comes_after_those_from_another_thread() {
    let (mut here, _) = counted();
    let (to_there, from_here) = mpsc::channel::<PortEnd>();
    let (to_here, from_there) = mpsc::channel::<PortEnd>();
    let (kept, port) = here.mutate(|cx| {
        let context = context(cx);
        let (one, two) = cx.message_channel_ports(cx.create_message_channel(Some(context)));
        cx.start_message_port(one);
        to_there.send(cx.ship_message_port(two)).unwrap();
        (Persistent::new(cx.gc, context), Persistent::new(cx.gc, one))
    });
    std::thread::spawn(move || {
        let mut there = Dom::new();
        let end = from_here.recv().unwrap();
        there.mutate(|cx| {
            let port = cx.receive_message_ports(context(cx), vec![end])[0];
            cx.post_port_message(port, message(1, Vec::new()));
            to_here.send(cx.ship_message_port(port)).unwrap();
        });
    })
    .join()
    .unwrap();
    let end = from_there.recv().unwrap();
    here.mutate(|cx| {
        let (context, port) = (kept.get(cx.gc), port.get(cx.gc));
        let back = cx.receive_message_ports(context, vec![end])[0];
        cx.post_port_message(back, message(2, Vec::new()));
        cx.port_messages_arrived(context);
        assert_eq!(take_all(cx, context, port), [1, 2]);
    });
}

/// The messages that wait for a port as it is shipped are its new DOM's
/// tasks in that DOM's order: after one queued there before they came.
#[test]
fn a_shipped_port_s_messages_are_numbered_where_they_come() {
    let mut here = Dom::new();
    let end = here.mutate(|cx| {
        let (one, two) = cx.message_channel_ports(cx.create_message_channel(Some(context(cx))));
        cx.post_port_message(one, message(1, Vec::new()));
        cx.ship_message_port(two)
    });
    std::thread::spawn(move || {
        let mut there = Dom::new();
        there.mutate(|cx| {
            let document = cx.create_document(true);
            let window = cx.create_window(document);
            let context = cx.window_context(window);
            // Numbered past what `here` numbered.
            for _ in 0..20 {
                cx.post_window_message(window, message(0, Vec::new()), DomString::new(""), None);
                cx.take_posted_message(context);
            }
            cx.post_window_message(window, message(2, Vec::new()), DomString::new(""), None);
            let port = cx.receive_message_ports(context, vec![end])[0];
            cx.start_message_port(port);
            let order: Vec<u8> = std::iter::from_fn(|| cx.take_posted_message(context))
                .map(|taken| taken.payload.unwrap().bytes[0])
                .collect();
            assert_eq!(order, [2, 1]);
        });
    })
    .join()
    .unwrap();
}

/// A port's message posted on its port's thread is numbered as it is
/// posted: before a window's message posted after it, the two of one
/// task source.
#[test]
fn a_message_on_its_port_s_thread_is_numbered_as_it_is_posted() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let context = cx.window_context(window);
        let (one, two) = cx.message_channel_ports(cx.create_message_channel(Some(context)));
        cx.start_message_port(two);
        cx.post_port_message(one, message(1, Vec::new()));
        cx.post_window_message(window, message(2, Vec::new()), DomString::new(""), None);
        let order: Vec<u8> = std::iter::from_fn(|| cx.take_posted_message(context))
            .map(|taken| taken.payload.unwrap().bytes[0])
            .collect();
        assert_eq!(order, [1, 2]);
    });
}
