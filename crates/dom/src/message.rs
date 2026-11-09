//! HTML's messages (Blink's `PostMessageHelper`, `MessagePort` and
//! `MessageEvent`): a message posted to a window (`window.postMessage`) or
//! through a port (`MessageChannel`) waits in a queue, its data serialized
//! by the engine (structured clone: the bytes and what the engine
//! transfers with them, which only it reads), until the host runs it as a
//! task of its own (the posted message task source).
//!
//! A message port is an event target that is nothing else
//! (`EventTargetImpl`) with a port's data: the window it belongs to, the
//! port it is entangled with, its port message queue (enabled by `start()`
//! or `onmessage`), and whether it was closed or shipped (transferred in a
//! message: a new port takes its place, entangled as it was, with its
//! queue). One entangled and enabled is in its window's list of ports,
//! which keeps it alive while messages may come (Blink's
//! `MessagePort::HasPendingActivity`).

use std::any::Any;
use std::cell::Cell;

use elise::{Erased, External, Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use crate::{Cx, DomString, Event, EventTargetImpl, TargetRef, Window, sites};

/// A message's serialization: the engine's bytes (V8's `ValueSerializer`'s)
/// and what it transfers with them (`ArrayBuffer`s' backing stores), which
/// only the engine reads.
pub struct SerializedMessage {
    pub bytes: Vec<u8>,
    pub transferred: Option<Box<dyn Any + Send>>,
}

/// A message waiting to be delivered (HTML's message of a posted message
/// task, or of a port message queue).
#[derive(GcObject)]
pub struct PostedMessage {
    /// Taken as it is delivered.
    payload: External<Cell<Option<SerializedMessage>>>,
    /// The poster's origin, serialized (a `MessageEvent`'s `origin`); empty
    /// for a port's.
    origin: LocalExternal<DomString>,
    /// The poster's window (a `MessageEvent`'s `source`); none for a port's.
    source: Member<Window>,
    /// The ports transferred with it, new ones taking the place of those
    /// shipped, entangled as they were.
    ports: Member<HeapVector<EventTargetImpl>>,
    /// When it was posted (the DOM's clock) and its task's number
    /// (`tasks`).
    queued: Cell<f64>,
    number: Cell<u64>,
}

/// A message port's data (HTML's `MessagePort`).
#[derive(GcObject)]
pub struct MessagePort {
    /// Its relevant global: the window its messages are tasks of (none
    /// while it travels in a message).
    owner: Member<Window>,
    /// The port it is entangled with, if any.
    entangled: Member<EventTargetImpl>,
    /// Its port message queue.
    queue: Member<HeapVector<PostedMessage>>,
    /// `ENABLED`, `CLOSED`, `SHIPPED`.
    state: Cell<u8>,
}

/// A `MessageEvent`'s data (HTML's).
#[derive(GcObject)]
pub struct MessageEventData {
    /// Its `data`, the engine's value, which only it reads and writes.
    data: JsEdge,
    origin: LocalExternal<DomString>,
    last_event_id: LocalExternal<DomString>,
    /// Its `source`: a window, a port, or none.
    source: Member<Erased>,
    ports: Member<HeapVector<EventTargetImpl>>,
}

/// The port message queue is enabled (`start()`).
const ENABLED: u8 = 1 << 0;
/// `close()`d: it gets no message again.
const CLOSED: u8 = 1 << 1;
/// Transferred in a message: a new port took its place.
const SHIPPED: u8 = 1 << 2;

/// A `MessageChannel` (HTML's): its two ports, entangled.
#[derive(GcObject)]
pub struct MessageChannel {
    port1: Member<EventTargetImpl>,
    port2: Member<EventTargetImpl>,
    wrapper: JsEdge,
}

/// What a `MessageEvent` is made of (`Cx::create_message_event`).
pub struct MessageEventInit<'gc> {
    pub origin: DomString,
    pub last_event_id: DomString,
    pub source: Option<TargetRef<'gc>>,
    pub ports: Vec<Gc<'gc, EventTargetImpl>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new `MessageEvent` of `event_type` (not bubbling, not cancelable)
    /// of `window`, its data the engine's to set (null till then).
    pub fn create_message_event(
        &self,
        event_type: crate::names::NameId,
        flags: crate::EventFlags,
        window: Option<Gc<'gc, Window>>,
        init: MessageEventInit<'gc>,
    ) -> Gc<'gc, Event> {
        let event = self.create_event(event_type, flags, window);
        self.set_message_event_data(event, init);
        event
    }

    /// A `MessageEvent`'s data made anew (`initMessageEvent`'s), its data
    /// the engine's to set again (null till then).
    pub fn set_message_event_data(&self, event: Gc<'gc, Event>, init: MessageEventInit<'gc>) {
        let ports = self.port_list(&init.ports);
        let data = self.gc.alloc_in(
            &sites::OTHER,
            MessageEventDataInit {
                data: (),
                origin: LocalExternal::new(init.origin),
                last_event_id: LocalExternal::new(init.last_event_id),
                source: init.source.map(TargetRef::erase),
                ports: Some(ports),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    fn port_list(
        &self,
        ports: &[Gc<'gc, EventTargetImpl>],
    ) -> Gc<'gc, HeapVector<EventTargetImpl>> {
        let list = HeapVector::new(self.gc, ports.len());
        for &port in ports {
            list.push(self.gc, port);
        }
        list
    }

    /// `event`'s `MessageEvent` data, if it is one.
    pub fn message_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, MessageEventData>> {
        event.data.get(self.gc)?.downcast::<MessageEventData>()
    }

    /// A `MessageEvent`'s reference to its `data`, which only the JS engine
    /// reads and writes.
    pub fn message_data_edge(&self, data: Gc<'gc, MessageEventData>) -> &'gc JsEdge {
        &data.into_ref().data
    }

    pub fn message_origin(&self, data: Gc<'gc, MessageEventData>) -> &'gc DomString {
        data.into_ref().origin.get()
    }

    pub fn message_last_event_id(&self, data: Gc<'gc, MessageEventData>) -> &'gc DomString {
        data.into_ref().last_event_id.get()
    }

    pub fn message_source(&self, data: Gc<'gc, MessageEventData>) -> Option<TargetRef<'gc>> {
        data.source.get(self.gc).map(TargetRef::from_erased)
    }

    pub fn message_ports(&self, data: Gc<'gc, MessageEventData>) -> Vec<Gc<'gc, EventTargetImpl>> {
        list_items(self, data.ports.get(self.gc))
    }

    /// A new message port of `owner` (HTML's), entangled with nothing yet.
    pub fn create_message_port(&self, owner: Option<Gc<'gc, Window>>) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        let port = self.gc.alloc_in(
            &sites::OTHER,
            MessagePortInit {
                owner,
                entangled: None,
                queue: None,
                state: Cell::new(0),
            },
        );
        target.port.set(self.gc, Some(port));
        target
    }

    /// A new `MessageChannel` of `owner`: two new ports, entangled.
    pub fn create_message_channel(
        &self,
        owner: Option<Gc<'gc, Window>>,
    ) -> Gc<'gc, MessageChannel> {
        let (one, two) = (
            self.create_message_port(owner),
            self.create_message_port(owner),
        );
        self.entangle(one, two);
        self.gc.alloc_in(
            &sites::OTHER,
            MessageChannelInit {
                port1: Some(one),
                port2: Some(two),
                wrapper: (),
            },
        )
    }

    /// `channel`'s `port1` and `port2`.
    pub fn message_channel_ports(
        &self,
        channel: Gc<'gc, MessageChannel>,
    ) -> (Gc<'gc, EventTargetImpl>, Gc<'gc, EventTargetImpl>) {
        (
            channel.port1.get(self.gc).expect("a channel's port"),
            channel.port2.get(self.gc).expect("a channel's port"),
        )
    }

    pub(crate) fn message_channel_wrapper_edge(
        &self,
        channel: Gc<'gc, MessageChannel>,
    ) -> &'gc JsEdge {
        &channel.into_ref().wrapper
    }

    fn entangle(&self, one: Gc<'gc, EventTargetImpl>, two: Gc<'gc, EventTargetImpl>) {
        for (port, other) in [(one, two), (two, one)] {
            if let Some(data) = self.port_data(port) {
                data.entangled.set(self.gc, Some(other));
            }
        }
    }

    fn port_data(&self, target: Gc<'gc, EventTargetImpl>) -> Option<Gc<'gc, MessagePort>> {
        target.port.get(self.gc)
    }

    /// Whether `target` is a message port.
    pub fn is_message_port(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        self.port_data(target).is_some()
    }

    /// The window `port`'s messages are tasks of.
    pub fn message_port_owner(&self, port: Gc<'gc, EventTargetImpl>) -> Option<Gc<'gc, Window>> {
        self.port_data(port)?.owner.get(self.gc)
    }

    /// Whether `port` was shipped (transferred) or closed: it can't be
    /// transferred again.
    pub fn message_port_unusable(&self, port: Gc<'gc, EventTargetImpl>) -> bool {
        self.port_data(port)
            .is_none_or(|data| data.state.get() & (SHIPPED | CLOSED) != 0)
    }

    /// `port`'s `start()` (HTML's: its port message queue enabled): its
    /// messages come from now on.
    pub fn start_message_port(&self, port: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        let state = data.state.get();
        if state & (ENABLED | SHIPPED) != 0 {
            return;
        }
        data.state.set(state | ENABLED);
        self.keep_port(port, data);
    }

    /// `port`, enabled and entangled, in its window's list of ports: alive
    /// while messages may come, its queue looked at.
    fn keep_port(&self, port: Gc<'gc, EventTargetImpl>, data: Gc<'gc, MessagePort>) {
        let state = data.state.get();
        if state & ENABLED == 0 || state & CLOSED != 0 || data.entangled.get(self.gc).is_none() {
            return;
        }
        let Some(window) = data.owner.get(self.gc) else {
            return;
        };
        let ports = match window.message_ports.get(self.gc) {
            Some(ports) => ports,
            None => {
                let ports = HeapVector::new(self.gc, 1);
                window.message_ports.set(self.gc, Some(ports));
                ports
            }
        };
        if !list_items(self, Some(ports))
            .iter()
            .any(|&kept| Gc::ptr_eq(kept, port))
        {
            ports.push(self.gc, port);
        }
    }

    /// `port` out of its window's list of ports.
    fn forget_port(&self, port: Gc<'gc, EventTargetImpl>) {
        let window = self
            .port_data(port)
            .and_then(|data| data.owner.get(self.gc));
        let Some(ports) = window.and_then(|window| window.message_ports.get(self.gc)) else {
            return;
        };
        if let Some(index) = list_items(self, Some(ports))
            .iter()
            .position(|&kept| Gc::ptr_eq(kept, port))
        {
            ports.remove(self.gc, index);
        }
    }

    /// `port`'s `close()` (HTML's: closed, disentangled): neither end gets
    /// a message again.
    pub fn close_message_port(&self, port: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        data.state.set(data.state.get() | CLOSED);
        self.disentangle(port);
    }

    /// `port` and the port it is entangled with, disentangled: `port`
    /// (closed) out of its window's list, the other once its queue's
    /// messages, which were sent before, are delivered (HTML's: they
    /// stay in its queue).
    fn disentangle(&self, port: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        if let Some(other) = data.entangled.get(self.gc) {
            if let Some(other_data) = self.port_data(other) {
                other_data.entangled.set(self.gc, None);
            }
            self.forget_port_if_done(other);
        }
        data.entangled.set(self.gc, None);
        self.forget_port(port);
    }

    /// `port`, entangled with none and with no message left, out of its
    /// window's list.
    fn forget_port_if_done(&self, port: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        let waiting = data
            .queue
            .get(self.gc)
            .is_some_and(|queue| !queue.is_empty());
        if data.entangled.get(self.gc).is_none() && !waiting {
            self.forget_port(port);
        }
    }

    /// `port` shipped in a message (HTML's transfer steps): a new port of
    /// no window yet takes its place, entangled with its other end, with
    /// its queue (its messages not delivered yet); `port` is detached.
    pub fn ship_message_port(&self, port: Gc<'gc, EventTargetImpl>) -> Gc<'gc, EventTargetImpl> {
        let shipped = self.create_message_port(None);
        let (Some(data), Some(new)) = (self.port_data(port), self.port_data(shipped)) else {
            return shipped;
        };
        new.queue.set(self.gc, data.queue.get(self.gc));
        data.queue.set(self.gc, None);
        let other = data.entangled.get(self.gc);
        self.forget_port(port);
        data.entangled.set(self.gc, None);
        data.state.set(data.state.get() | SHIPPED);
        if let Some(other) = other {
            self.entangle(shipped, other);
        }
        shipped
    }

    /// `ports`, shipped in a message, now `window`'s (HTML's transfer-receiving
    /// steps).
    fn receive_ports(&self, window: Gc<'gc, Window>, ports: &[Gc<'gc, EventTargetImpl>]) {
        for &port in ports {
            if let Some(data) = self.port_data(port) {
                data.owner.set(self.gc, Some(window));
            }
        }
    }

    /// A message posted to `window` (`window.postMessage`), from `source`
    /// of `origin`: a task of its own of the window's posted message task
    /// source.
    pub fn post_window_message(
        &self,
        window: Gc<'gc, Window>,
        payload: SerializedMessage,
        origin: DomString,
        source: Option<Gc<'gc, Window>>,
        ports: &[Gc<'gc, EventTargetImpl>],
    ) {
        let message = self.posted_message(payload, origin, source, ports);
        let queue = match window.posted_messages.get(self.gc) {
            Some(queue) => queue,
            None => {
                let queue = HeapVector::new(self.gc, 1);
                window.posted_messages.set(self.gc, Some(queue));
                queue
            }
        };
        queue.push(self.gc, message);
    }

    fn posted_message(
        &self,
        payload: SerializedMessage,
        origin: DomString,
        source: Option<Gc<'gc, Window>>,
        ports: &[Gc<'gc, EventTargetImpl>],
    ) -> Gc<'gc, PostedMessage> {
        let ports = self.port_list(ports);
        let (queued, number) = self.stamp_queued_task();
        self.gc.alloc_in(
            &sites::OTHER,
            PostedMessageInit {
                payload: External::new(Cell::new(Some(payload))),
                origin: LocalExternal::new(origin),
                source,
                ports: Some(ports),
                queued: Cell::new(queued),
                number: Cell::new(number),
            },
        )
    }

    /// A message posted through `port` (its `postMessage`): in the port
    /// message queue of the port it is entangled with; none if it is
    /// entangled with none (closed, or never: the message is lost).
    pub fn post_port_message(
        &self,
        port: Gc<'gc, EventTargetImpl>,
        payload: SerializedMessage,
        ports: &[Gc<'gc, EventTargetImpl>],
    ) {
        let Some(target) = self
            .port_data(port)
            .and_then(|data| data.entangled.get(self.gc))
        else {
            return;
        };
        let Some(data) = self.port_data(target) else {
            return;
        };
        let message = self.posted_message(payload, DomString::new(""), None, ports);
        let queue = match data.queue.get(self.gc) {
            Some(queue) => queue,
            None => {
                let queue = HeapVector::new(self.gc, 1);
                data.queue.set(self.gc, Some(queue));
                queue
            }
        };
        queue.push(self.gc, message);
    }

    /// Whether `window` has a message to deliver: one posted to it, or one
    /// in an enabled port's queue of its.
    pub fn has_posted_message(&self, window: Gc<'gc, Window>) -> bool {
        window
            .posted_messages
            .get(self.gc)
            .is_some_and(|queue| !queue.is_empty())
            || list_items(self, window.message_ports.get(self.gc))
                .into_iter()
                .any(|port| {
                    self.port_data(port)
                        .and_then(|data| data.queue.get(self.gc))
                        .is_some_and(|queue| !queue.is_empty())
                })
    }

    /// `window`'s next message, taken: the first posted, of those posted
    /// to it and those in its enabled ports' queues (the port it is for,
    /// the target), that the host's pass lets run (`tasks`); its payload,
    /// origin, source and ports (now the window's).
    pub fn take_posted_message(&self, window: Gc<'gc, Window>) -> Option<TakenMessage<'gc>> {
        let (port, _) = self.first_message(window)?;
        let (message, target) = match port {
            None => {
                let queue = window.posted_messages.get(self.gc)?;
                (queue.remove(self.gc, 0), TargetRef::Window(window))
            }
            Some(port) => {
                let queue = self.port_data(port)?.queue.get(self.gc)?;
                let message = queue.remove(self.gc, 0);
                // Disentangled, its last message: no more to come.
                self.forget_port_if_done(port);
                (message, TargetRef::EventTarget(port))
            }
        };
        let ports = list_items(self, message.ports.get(self.gc));
        self.receive_ports(window, &ports);
        Some(TakenMessage {
            target,
            payload: message.payload.get().take(),
            origin: message.origin.get().clone(),
            source: message.source.get(self.gc),
            ports,
        })
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// When `window`'s first message was posted (the DOM's clock) and its
    /// number, if the host's pass lets it run (`tasks`).
    pub(crate) fn next_posted_message(&self, window: Gc<'gc, Window>) -> Option<(f64, u64)> {
        self.first_message(window)
            .map(|(_, message)| (message.queued.get(), message.number.get()))
    }

    /// `window`'s first message to deliver: of the first in its own queue
    /// and those in its enabled ports' (the port), the lowest numbered, if
    /// the host's pass lets it run.
    fn first_message(
        &self,
        window: Gc<'gc, Window>,
    ) -> Option<(Option<Gc<'gc, EventTargetImpl>>, Gc<'gc, PostedMessage>)> {
        let own = window
            .posted_messages
            .get(self.gc)
            .and_then(|queue| queue.get(self.gc, 0))
            .map(|message| (None, message));
        let ports = list_items(self, window.message_ports.get(self.gc))
            .into_iter()
            .filter_map(|port| {
                let message = self.port_data(port)?.queue.get(self.gc)?.get(self.gc, 0)?;
                Some((Some(port), message))
            });
        own.into_iter()
            .chain(ports)
            .filter(|(_, message)| self.task_may_run(message.number.get()))
            .min_by_key(|(_, message)| message.number.get())
    }
}

/// A message taken to be delivered (`Cx::take_posted_message`).
pub struct TakenMessage<'gc> {
    /// The window or the port it is for.
    pub target: TargetRef<'gc>,
    pub payload: Option<SerializedMessage>,
    pub origin: DomString,
    pub source: Option<Gc<'gc, Window>>,
    pub ports: Vec<Gc<'gc, EventTargetImpl>>,
}

fn list_items<'gc, T: elise::GcObject + 'static>(
    cx: &Cx<'_, 'gc>,
    list: Option<Gc<'gc, HeapVector<T>>>,
) -> Vec<Gc<'gc, T>> {
    list.map_or_else(Vec::new, |list| {
        (0..list.len())
            .filter_map(|index| list.get(cx.gc, index))
            .collect()
    })
}
