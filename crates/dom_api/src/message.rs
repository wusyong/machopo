//! HTML's messages (Blink's `DOMWindow::postMessage`, `MessagePort`,
//! `MessageChannel`, `MessageEvent`): `window.postMessage`, a channel's
//! ports, and the events that deliver them. The message is serialized
//! before the operation (the front end's structured clone, which may run
//! script: `Front::message_arg`); in it, the ports transferred are shipped
//! (new ones take their places) and the message waits in the target's
//! queue (`dom`'s `message`), until the host runs it as a task
//! (`run_posted_messages`): the engine deserializes it in the target's
//! realm (`ScriptHost::deserialize_message`) and a `message` event, or a
//! `messageerror` one if it can't, is dispatched at the window or the port.

use std::cell::Cell;

use dom::{
    DomString, Event, EventFlags, EventTargetImpl, GlobalRoot, MessageChannel,
    MessageEventInit as Init, SerializedMessage, TargetRef, Window, names,
};
use elise::{Gc, Persistent};

use crate::declare::TypeDesc;
use crate::interfaces::{MessageEventInit, WindowPostMessageOptions};
use crate::typed::{ArgType, Dictionary, Front, FrontTypes, OpFront, ScriptHost, TransferArg};
use crate::{ApiCx, ApiError, DomException, DomStr, ExceptionName, dispatch};

/// A message argument, serialized with its transfer list (HTML's
/// `StructuredSerializeWithTransfer`): `window.postMessage`'s
/// (`(message, targetOrigin, transfer)` or `(message, options)`) or a
/// port's (`(message, transfer)` or `(message, options)`).
pub struct WindowMessage;
pub struct PortMessage;

/// A message as the front end took it (`N` its nodes' and objects' type,
/// `S` its strings').
pub struct MessageTaken<N, S> {
    /// Taken by the operation, which posts it.
    payload: Cell<Option<SerializedMessage>>,
    /// The ports transferred, in the order the serialization numbers them.
    ports: Vec<N>,
    target_origin: TargetOrigin<S>,
}

enum TargetOrigin<S> {
    Arg(S),
    Option(String),
    None,
}

/// A message in the operation.
pub struct MessageParam<'a, 'gc> {
    pub(crate) payload: &'a Cell<Option<SerializedMessage>>,
    pub(crate) ports: Vec<Gc<'gc, EventTargetImpl>>,
    target_origin: Option<&'a str>,
}

impl<'gc> MessageParam<'_, 'gc> {
    /// The message, taken, with the ends of the ports it transfers: those
    /// are shipped (detached) whether or not it goes anywhere (HTML's
    /// transfer steps come with its serialization).
    pub(crate) fn ship(&self, cx: &ApiCx<'_, 'gc>) -> Option<SerializedMessage> {
        let ends: Vec<_> = self
            .ports
            .iter()
            .map(|&port| cx.ship_message_port(port))
            .collect();
        let mut payload = self.payload.take()?;
        payload.ports = ends;
        Some(payload)
    }
}

impl ArgType for WindowMessage {
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = false;
    type Taken<T: FrontTypes> = MessageTaken<T::Node, T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = MessageParam<'a, 'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        // WebIDL's overloads: a string second, or the options.
        let (target_origin, transfer) = if front.is_dictionary_like(index.saturating_add(1)) {
            let options = WindowPostMessageOptions::read(front, index.saturating_add(1))?;
            (
                TargetOrigin::Option(options.targetOrigin),
                TransferArg::Options(index.saturating_add(1)),
            )
        } else {
            (
                TargetOrigin::Arg(front.string_arg(index.saturating_add(1))?),
                TransferArg::Sequence(index.saturating_add(2)),
            )
        };
        let (payload, ports) = front.message_arg(index, transfer, false)?;
        // The incumbent realm's window: the message's `source` and origin.
        front.prepare_incumbent_window();
        Some(MessageTaken {
            payload: Cell::new(Some(payload)),
            ports,
            target_origin,
        })
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<MessageParam<'a, 'gc>, ApiError> {
        Ok(param(op, cx, taken))
    }
}

impl ArgType for PortMessage {
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = false;
    type Taken<T: FrontTypes> = MessageTaken<T::Node, T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = MessageParam<'a, 'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        let (payload, ports) = front.message_arg(
            index,
            TransferArg::SequenceOrOptions(index.saturating_add(1)),
            true,
        )?;
        Some(MessageTaken {
            payload: Cell::new(Some(payload)),
            ports,
            target_origin: TargetOrigin::None,
        })
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<MessageParam<'a, 'gc>, ApiError> {
        Ok(param(op, cx, taken))
    }
}

fn param<'a, 'gc, O: OpFront + 'a>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    taken: &'a MessageTaken<O::Node, O::Str>,
) -> MessageParam<'a, 'gc> {
    MessageParam {
        payload: &taken.payload,
        ports: taken
            .ports
            .iter()
            .map(|port| op.object::<EventTargetImpl>(cx, port))
            .collect(),
        target_origin: match &taken.target_origin {
            TargetOrigin::Arg(origin) => Some(op.str_param(origin)),
            TargetOrigin::Option(origin) => Some(origin),
            TargetOrigin::None => None,
        },
    }
}

/// `window.postMessage(message, targetOrigin, transfer)` (HTML's window
/// post message steps): `targetOrigin` `*` (any), `/` (the caller's
/// origin) or a URL (else a `SyntaxError`); the ports transferred are
/// shipped; the message goes to the window's queue if its document is of
/// that origin, from the caller's window and origin.
pub(crate) fn window_post_message<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Option<Gc<'gc, dom::Window>>,
    this: Gc<'gc, Window>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    let incumbent = window.and_then(|window| cx.document_origin(cx.window_document(window)));
    let target = match message.target_origin.unwrap_or("/") {
        "*" => None,
        "/" => incumbent.clone(),
        url => Some(
            url::Url::parse(url)
                .map_err(|_| {
                    ApiError::Dom(DomException {
                        name: ExceptionName::Syntax,
                        message: format!(
                            "Invalid target origin '{url}' in a call to 'postMessage'."
                        ),
                    })
                })?
                .origin(),
        ),
    };
    let Some(payload) = message.ship(cx) else {
        return Ok(());
    };
    // Not of that origin: the message is lost (the ports with it).
    if let Some(target) = &target
        && crate::origin(cx, cx.window_document(this)).as_ref() != Some(target)
    {
        return Ok(());
    }
    let origin = incumbent.map_or_else(|| "null".to_owned(), |origin| origin.ascii_serialization());
    cx.post_window_message(this, payload, DomString::new(&origin), window);
    Ok(())
}

/// A port's `postMessage(message, transfer)`: the ports transferred are
/// shipped, and the message goes to the port this one is entangled with
/// (lost if none).
pub(crate) fn port_post_message<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    if let Some(payload) = message.ship(cx) {
        cx.post_port_message(this, payload);
    }
    Ok(())
}

/// `new MessageChannel()`: two new ports of the caller's realm, entangled.
pub(crate) fn construct_message_channel<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, MessageChannel>, ApiError> {
    let context = op.current_context(cx);
    Ok(cx.create_message_channel(context))
}

pub(crate) fn port1<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MessageChannel>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(cx.message_channel_ports(this).0)
}

pub(crate) fn port2<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MessageChannel>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(cx.message_channel_ports(this).1)
}

/// A port's `start()`: its messages come from now on.
pub(crate) fn start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    cx.start_message_port(this);
    Ok(())
}

/// A port's `close()`: neither end gets a message again.
pub(crate) fn close<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    cx.close_message_port(this);
    Ok(())
}

/// `new MessageEvent(type, eventInitDict)`: its `data` (null without one),
/// `origin` and `lastEventId`. Its init's `source` and `ports` aren't read
/// yet: none.
pub(crate) fn construct_message_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &MessageEventInit<O::Any>,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_message_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_context(cx),
        Init {
            origin: DomString::new(&init.origin),
            last_event_id: DomString::new(&init.lastEventId),
            source: None,
            ports: Vec::new(),
        },
    );
    if let Some(data) = init.data {
        let edge = cx.message_data_edge(message_data(cx, event));
        op.store_any(cx, edge, data);
    }
    Ok(event)
}

fn message_data<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> Gc<'gc, dom::MessageEventData> {
    cx.message_event_data(event)
        .expect("the front end's checks: a message event")
}

pub(crate) fn data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.message_data_edge(message_data(cx, this)))
}

pub(crate) fn origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.message_origin(message_data(cx, this)).clone(),
    ))
}

pub(crate) fn last_event_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.message_last_event_id(message_data(cx, this)).clone(),
    ))
}

pub(crate) fn source<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<TargetRef<'gc>>, ApiError> {
    Ok(cx.message_source(message_data(cx, this)))
}

pub(crate) fn ports<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Vec<Gc<'gc, EventTargetImpl>>, ApiError> {
    Ok(cx.message_ports(message_data(cx, this)))
}

/// `initMessageEvent(...)`: nothing while it is dispatched; else DOM's
/// "initialize", and its data, origin, last event ID, source and ports
/// those given (Blink's `MessageEvent::initMessageEvent`).
#[expect(
    clippy::too_many_arguments,
    reason = "the legacy method's eight parameters, and the front end and cx"
)]
pub(crate) fn init_message_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    data: crate::typed::AnyArg<O::Any>,
    origin: &str,
    last_event_id: &str,
    source: Option<Gc<'gc, Window>>,
    ports: Vec<Gc<'gc, EventTargetImpl>>,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    cx.set_message_event_data(
        this,
        Init {
            origin: DomString::new(origin),
            last_event_id: DomString::new(last_event_id),
            source: source.map(TargetRef::Window),
            ports,
        },
    );
    let edge = cx.message_data_edge(message_data(cx, this));
    op.store_any(cx, edge, data);
    Ok(())
}

/// Runs `global`'s messages (the posted message task source's tasks):
/// each deserialized in its realm (`ScriptHost::
/// deserialize_message`) as a trusted `message` event's `data` (a
/// `messageerror` if it can't be), dispatched at the window or the port it
/// is for (a worker's global for one posted to it), then a microtask
/// checkpoint. How many ran.
pub fn run_posted_messages<H: ScriptHost>(host: &mut H, global: &GlobalRoot) -> usize {
    let mut ran = 0usize;
    while run_posted_message(host, global, None) {
        ran = ran.saturating_add(1);
    }
    ran
}

/// Delivers `global`'s message numbered `number`, or its first
/// (`run_posted_messages`'s one) if the DOM's pass lets it run. Whether
/// one was.
pub fn run_posted_message<H: ScriptHost>(
    host: &mut H,
    global: &GlobalRoot,
    number: Option<u64>,
) -> bool {
    {
        let taken = host.mutate(|cx| {
            let context = cx.global_context(global.get(cx.gc));
            let taken = match number {
                Some(number) => cx.take_posted_message_numbered(context, number),
                None => cx.take_posted_message(context),
            }?;
            let ports: Vec<_> = taken
                .ports
                .iter()
                .map(|&port| Persistent::new(cx.gc, port))
                .collect();
            Some((
                cx.root_target(taken.target),
                taken.payload,
                taken.origin,
                taken.source.map(|source| Persistent::new(cx.gc, source)),
                ports,
            ))
        });
        let Some((target, payload, origin, source, ports)) = taken else {
            return false;
        };
        // For a `Worker` terminated meanwhile: dropped (HTML's
        // `terminate()` empties its port's message queue); not for one whose
        // worker closed itself after posting it.
        let terminated = host.read(|cx| match target.get(cx.gc) {
            TargetRef::EventTarget(worker) => {
                cx.is_worker(worker) && cx.worker_messages_dropped(worker)
            }
            _ => false,
        });
        if terminated {
            return true;
        }
        let value = payload.and_then(|payload| host.deserialize_message(global, payload, &ports));
        let error = value.is_none();
        let event = host.mutate_storing(|cx, store| {
            let event = cx.create_message_event(
                if error {
                    names::event::MESSAGEERROR
                } else {
                    names::event::MESSAGE
                },
                EventFlags {
                    trusted: true,
                    ..EventFlags::default()
                },
                Some(cx.global_context(global.get(cx.gc))),
                Init {
                    origin,
                    last_event_id: DomString::new(""),
                    source: source.map(|source| TargetRef::Window(source.get(cx.gc))),
                    ports: ports.iter().map(|port| port.get(cx.gc)).collect(),
                },
            );
            if let Some(value) = value {
                store(cx.message_data_edge(message_data(cx, event)), value);
            }
            Persistent::new(cx.gc, event)
        });
        dispatch(host, &target, &event);
        host.perform_microtask_checkpoint();
        true
    }
}
