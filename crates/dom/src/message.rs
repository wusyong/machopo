//! HTML's messages (Blink's `PostMessageHelper`, `MessagePort` and
//! `MessageEvent`): a message posted to a window (`window.postMessage`) or
//! through a port (`MessageChannel`) waits in a queue, its data serialized
//! by the engine (structured clone: the bytes and what the engine
//! transfers with them, which only it reads), until the host runs it as a
//! task of its own (the posted message task source).
//!
//! A message port is an event target that is nothing else
//! (`EventTargetImpl`) with a port's data: the execution context it
//! belongs to, its end of the pipe it shares with the port it is entangled
//! with (`message_pipe`: its port message queue is there), whether its
//! queue is enabled (by `start()` or `onmessage`), and whether it was
//! closed or shipped (transferred in a message: its end goes with the
//! message, and a new port takes its place where it is delivered, its
//! messages still waiting). One enabled with its end, and messages still
//! to come, is in its context's list of ports, which keeps it alive (Blink's
//! `MessagePort::HasPendingActivity`).

use std::any::Any;
use std::cell::Cell;

use elise::{Erased, External, Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use crate::message_pipe::{Home, PortEnd};
use crate::{Cx, DomString, Event, EventTargetImpl, TargetRef, Window, sites};

/// A message's serialization: the engine's bytes (V8's `ValueSerializer`'s),
/// what it transfers with them (`ArrayBuffer`s' backing stores), which only
/// the engine reads, and the ends of the ports shipped with it, in the
/// order the serialization numbers them. All of it may go to another
/// thread.
pub struct SerializedMessage {
    pub bytes: Vec<u8>,
    pub transferred: Option<Box<dyn Any + Send>>,
    pub ports: Vec<PortEnd>,
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
    /// The `Worker` it is for, from its worker; none: for the global of the
    /// execution context whose queue it is in.
    target: Member<EventTargetImpl>,
    /// When it was posted (the DOM's clock) and its task's number
    /// (`tasks`).
    queued: Cell<f64>,
    number: Cell<u64>,
}

/// A message port's data (HTML's `MessagePort`).
#[derive(GcObject)]
pub struct MessagePort {
    /// Its relevant global's execution context: the one its messages are
    /// tasks of.
    owner: Member<crate::ExecutionContext>,
    /// Its end of its pipe: none once it was shipped or closed. Dropped
    /// with it if it is collected, which closes it.
    end: External<Cell<Option<PortEnd>>>,
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
/// Transferred in a message: its end went with it.
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
    /// of `context`'s realm, its data the engine's to set (null till then).
    pub fn create_message_event(
        &self,
        event_type: crate::names::NameId,
        flags: crate::EventFlags,
        context: Option<Gc<'gc, crate::ExecutionContext>>,
        init: MessageEventInit<'gc>,
    ) -> Gc<'gc, Event> {
        let event = self.create_event(event_type, flags, context);
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

    /// Where this DOM's ports are: its thread, and what wakes it
    /// (`Dom::set_port_waker`).
    fn port_home(&self) -> Home {
        Home {
            thread: std::thread::current().id(),
            waker: self.changes.port_waker.clone(),
        }
    }

    /// A new message port of `owner` (HTML's) with `end`, which is now
    /// here.
    fn new_message_port(
        &self,
        owner: Option<Gc<'gc, crate::ExecutionContext>>,
        end: PortEnd,
    ) -> Gc<'gc, EventTargetImpl> {
        end.set_home(Some(self.port_home()));
        let target = self.create_event_target();
        let port = self.gc.alloc_in(
            &sites::OTHER,
            MessagePortInit {
                owner,
                end: External::new(Cell::new(Some(end))),
                state: Cell::new(0),
            },
        );
        target.port.set(self.gc, Some(port));
        target
    }

    /// A new `MessageChannel` of `owner`: two new ports, entangled.
    pub fn create_message_channel(
        &self,
        owner: Option<Gc<'gc, crate::ExecutionContext>>,
    ) -> Gc<'gc, MessageChannel> {
        let (one, two) = PortEnd::pair();
        let (one, two) = (
            self.new_message_port(owner, one),
            self.new_message_port(owner, two),
        );
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

    fn port_data(&self, target: Gc<'gc, EventTargetImpl>) -> Option<Gc<'gc, MessagePort>> {
        target.port.get(self.gc)
    }

    /// `f` of `data`'s end, if it has one.
    fn with_port_end<R>(
        &self,
        data: Gc<'gc, MessagePort>,
        f: impl FnOnce(&PortEnd) -> R,
    ) -> Option<R> {
        let cell = data.into_ref().end.get();
        let end = cell.take()?;
        let result = f(&end);
        cell.set(Some(end));
        Some(result)
    }

    /// Whether `target` is a message port.
    pub fn is_message_port(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        self.port_data(target).is_some()
    }

    /// The execution context `port`'s messages are tasks of.
    pub fn message_port_owner(
        &self,
        port: Gc<'gc, EventTargetImpl>,
    ) -> Option<Gc<'gc, crate::ExecutionContext>> {
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

    /// `port`, enabled, with its end, in its owner's list of ports: alive
    /// while messages may come, its queue looked at.
    fn keep_port(&self, port: Gc<'gc, EventTargetImpl>, data: Gc<'gc, MessagePort>) {
        let state = data.state.get();
        if state & ENABLED == 0 || state & CLOSED != 0 || self.with_port_end(data, |_| ()).is_none()
        {
            return;
        }
        let Some(owner) = data.owner.get(self.gc) else {
            return;
        };
        let ports = match owner.message_ports.get(self.gc) {
            Some(ports) => ports,
            None => {
                let ports = HeapVector::new(self.gc, 1);
                owner.message_ports.set(self.gc, Some(ports));
                ports
            }
        };
        if !list_items(self, Some(ports))
            .iter()
            .any(|&kept| Gc::ptr_eq(kept, port))
        {
            ports.push(self.gc, port);
            // The messages stamped while it wasn't in the list: the host's
            // scheduler took their tasks then and found none, so it is
            // told of them again (`Dom::set_task_waker`).
            for number in self
                .with_port_end(data, PortEnd::stamped_numbers)
                .unwrap_or_default()
            {
                self.wake_for_task(number, None);
            }
        }
    }

    /// `port` out of its owner's list of ports.
    fn forget_port(&self, port: Gc<'gc, EventTargetImpl>) {
        let owner = self
            .port_data(port)
            .and_then(|data| data.owner.get(self.gc));
        let Some(ports) = owner.and_then(|owner| owner.message_ports.get(self.gc)) else {
            return;
        };
        if let Some(index) = list_items(self, Some(ports))
            .iter()
            .position(|&kept| Gc::ptr_eq(kept, port))
        {
            ports.remove(self.gc, index);
        }
    }

    /// `port`'s `close()` (HTML's: closed, disentangled): its end closed,
    /// so neither port gets a message again (the other once it took those
    /// that came before).
    pub fn close_message_port(&self, port: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        data.state.set(data.state.get() | CLOSED);
        self.forget_port(port);
        drop(data.into_ref().end.get().take());
    }

    /// `port` shipped in a message (HTML's transfer steps): its end, for
    /// the message, its messages not delivered yet waiting there; `port`
    /// is detached. (An end of no pipe for one already shipped or closed,
    /// which the serializer turns down.)
    pub fn ship_message_port(&self, port: Gc<'gc, EventTargetImpl>) -> PortEnd {
        let Some(data) = self.port_data(port) else {
            return PortEnd::pair().0;
        };
        self.forget_port(port);
        data.state.set(data.state.get() | SHIPPED);
        let Some(end) = data.into_ref().end.get().take() else {
            return PortEnd::pair().0;
        };
        end.set_home(None);
        end
    }

    /// The ports shipped as `ends`, now `context`'s (HTML's
    /// transfer-receiving steps): new ports with them, entangled as they
    /// were.
    pub fn receive_message_ports(
        &self,
        context: Gc<'gc, crate::ExecutionContext>,
        ends: Vec<PortEnd>,
    ) -> Vec<Gc<'gc, EventTargetImpl>> {
        ends.into_iter()
            .map(|end| self.new_message_port(Some(context), end))
            .collect()
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
    ) {
        let message = self.posted_message(payload, origin, source);
        self.queue_posted_message(self.window_context(window), message);
    }

    /// A message posted to the worker's global `global` by its `Worker`
    /// (HTML's dedicated worker's implicit port): a task of its posted
    /// message task source, of no origin and no source.
    pub fn post_worker_global_message(
        &self,
        global: Gc<'gc, crate::WorkerGlobalScope>,
        payload: SerializedMessage,
    ) {
        let message = self.posted_message(payload, DomString::new(""), None);
        self.queue_posted_message(self.worker_global_context(global), message);
    }

    /// A message posted to `worker` by its worker
    /// (`DedicatedWorkerGlobalScope.postMessage`): a task of the posted
    /// message task source of `worker`'s realm, for it, of no origin and no
    /// source.
    pub fn post_worker_object_message(
        &self,
        worker: Gc<'gc, EventTargetImpl>,
        payload: SerializedMessage,
    ) {
        let Some(context) = self.target_context(TargetRef::EventTarget(worker)) else {
            return;
        };
        let message = self.posted_message(payload, DomString::new(""), None);
        message.target.set(self.gc, Some(worker));
        self.queue_posted_message(context, message);
    }

    fn queue_posted_message(
        &self,
        context: Gc<'gc, crate::ExecutionContext>,
        message: Gc<'gc, PostedMessage>,
    ) {
        let queue = match context.posted_messages.get(self.gc) {
            Some(queue) => queue,
            None => {
                let queue = HeapVector::new(self.gc, 1);
                context.posted_messages.set(self.gc, Some(queue));
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
    ) -> Gc<'gc, PostedMessage> {
        let (queued, number) = self.stamp_queued_task();
        self.gc.alloc_in(
            &sites::OTHER,
            PostedMessageInit {
                payload: External::new(Cell::new(Some(payload))),
                origin: LocalExternal::new(origin),
                source,
                target: None,
                queued: Cell::new(queued),
                number: Cell::new(number),
            },
        )
    }

    /// A message posted through `port` (its `postMessage`): in the pipe,
    /// for the port it is entangled with. Lost if that one is closed, if
    /// `port` has no end (closed, or shipped), or if it ships the port it
    /// is for (HTML's "doomed": the pipe closes).
    pub fn post_port_message(&self, port: Gc<'gc, EventTargetImpl>, payload: SerializedMessage) {
        let Some(data) = self.port_data(port) else {
            return;
        };
        let lost = self.with_port_end(data, |end| {
            if payload
                .ports
                .iter()
                .any(|shipped| end.is_entangled_with(shipped))
            {
                return Some(payload);
            }
            end.post(payload, || self.stamp_queued_task()).err()
        });
        drop(lost);
    }

    /// Whether `context` has a message to deliver: one posted to it, or one
    /// in an enabled port's queue of its.
    pub fn has_posted_message(&self, context: Gc<'gc, crate::ExecutionContext>) -> bool {
        context
            .posted_messages
            .get(self.gc)
            .is_some_and(|queue| !queue.is_empty())
            || list_items(self, context.message_ports.get(self.gc))
                .into_iter()
                .any(|port| {
                    self.port_data(port)
                        .and_then(|data| self.with_port_end(data, PortEnd::has_message))
                        .unwrap_or(false)
                })
    }

    /// `context`'s next message, taken: the first posted, of those posted
    /// to it and those in its enabled ports' queues (the port it is for,
    /// the target), that the host's pass lets run (`tasks`); its payload,
    /// origin, source and ports (new ones of `context`, with the ends it
    /// shipped).
    pub fn take_posted_message(
        &self,
        context: Gc<'gc, crate::ExecutionContext>,
    ) -> Option<TakenMessage<'gc>> {
        let (port, (_, number)) = self.first_message(context)?;
        let (mut payload, origin, source, target) = match port {
            None => {
                let queue = context.posted_messages.get(self.gc)?;
                let index = (0..queue.len()).find(|&index| {
                    queue
                        .get(self.gc, index)
                        .is_some_and(|message| message.number.get() == number)
                })?;
                let message = queue.remove(self.gc, index);
                let target = match message.target.get(self.gc) {
                    Some(worker) => TargetRef::EventTarget(worker),
                    None => self.context_target(context),
                };
                (
                    message.payload.get().take(),
                    message.origin.get().clone(),
                    message.source.get(self.gc),
                    target,
                )
            }
            Some(port) => {
                let data = self.port_data(port)?;
                // (Let go once it has no more to come: `first_message`.)
                let payload = self.with_port_end(data, PortEnd::take)?;
                (
                    payload,
                    DomString::new(""),
                    None,
                    TargetRef::EventTarget(port),
                )
            }
        };
        let ends = payload
            .as_mut()
            .map(|payload| std::mem::take(&mut payload.ports))
            .unwrap_or_default();
        Some(TakenMessage {
            target,
            payload,
            origin,
            source,
            ports: self.receive_message_ports(context, ends),
        })
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// When `context`'s first message was posted (the DOM's clock) and its
    /// number, if the host's pass lets it run (`tasks`).
    pub(crate) fn next_posted_message(
        &self,
        context: Gc<'gc, crate::ExecutionContext>,
    ) -> Option<(f64, u64)> {
        self.first_message(context)
            .map(|(_, (queued, number))| (queued, number))
    }

    /// `context`'s first message to deliver: of the first in its own queue
    /// and those in its enabled ports' (the port), the lowest numbered, if
    /// the host's pass lets it run. The messages that came to its ports
    /// from other threads are numbered now, as tasks of this DOM's; its
    /// ports that get no message again go out of its list.
    fn first_message(
        &self,
        context: Gc<'gc, crate::ExecutionContext>,
    ) -> Option<(Option<Gc<'gc, EventTargetImpl>>, TaskStamp)> {
        // Its own queue's first that may run: the first, or the one the
        // host's scheduler runs.
        let own = context.posted_messages.get(self.gc).and_then(|queue| {
            (0..queue.len())
                .filter_map(|index| queue.get(self.gc, index))
                .map(|message| (None, (message.queued.get(), message.number.get())))
                .find(|&(_, (_, number))| self.task_may_run(number))
        });
        let mut ports = Vec::new();
        for port in list_items(self, context.message_ports.get(self.gc)) {
            let Some(data) = self.port_data(port) else {
                continue;
            };
            let first = self.with_port_end(data, |end| {
                if end.is_done() {
                    return None;
                }
                Some(end.first_stamp(|| self.stamp_queued_task()))
            });
            match first.flatten() {
                Some(Some(stamp)) => ports.push((Some(port), stamp)),
                Some(None) => {}
                None => self.forget_port(port),
            }
        }
        own.into_iter()
            .chain(ports)
            .filter(|&(_, (_, number))| self.task_may_run(number))
            .min_by_key(|&(_, (_, number))| number)
    }
}

/// When a task was queued (the DOM's clock) and its number (`tasks`).
type TaskStamp = (f64, u64);

/// A message taken to be delivered (`Cx::take_posted_message`).
pub struct TakenMessage<'gc> {
    /// The global, the `Worker` or the port it is for.
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
