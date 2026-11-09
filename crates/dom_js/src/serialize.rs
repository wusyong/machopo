//! HTML's structured clone of a message (`StructuredSerializeWithTransfer`
//! and `StructuredDeserializeWithTransfer`) with V8's `ValueSerializer`,
//! as Blink's `SerializedScriptValue`:
//! JS values V8 clones itself; a transferred `MessagePort` is written as
//! its place among the message's ports and read back as the port at that
//! place; a transferred `ArrayBuffer` is V8's own transfer, its backing
//! store taken from the buffer (then detached) and given to a new one
//! where it is read. A `DOMException` (WebIDL's `[Serializable]`) is
//! written as its name and message, a `QuotaExceededError` as its message,
//! quota and amount requested, each read back as a new one of the realm
//! where it is read (Blink's). Any other platform object is
//! a `DataCloneError`.

use std::any::Any;

use dom::{EventTargetImpl, SerializedMessage};
use v8::{ValueDeserializerHelper, ValueSerializerHelper};

use crate::state::{self, Scope};

/// The tags host objects are written after: a transferred port, a
/// `DOMException` and a `QuotaExceededError`.
const PORT_TAG: u32 = b'M' as u32;
const DOM_EXCEPTION_TAG: u32 = b'x' as u32;
const QUOTA_EXCEEDED_ERROR_TAG: u32 = b'q' as u32;

/// Writes `object` if it is a `DOMException` of either kind, and says
/// whether it was.
fn write_dom_exception(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<'_, v8::Object>,
    out: &dyn ValueSerializerHelper,
) -> bool {
    use crate::exception::{self, Brand};
    let Some(brand) = exception::brand(scope, object) else {
        return false;
    };
    let (name, message) = exception::name_and_message(scope, object);
    match brand {
        Brand::DomException => {
            out.write_uint32(DOM_EXCEPTION_TAG);
            write_string(out, &name);
            write_string(out, &message);
        }
        Brand::QuotaExceededError => {
            let (quota, requested) = exception::quota_and_requested(scope, object);
            out.write_uint32(QUOTA_EXCEEDED_ERROR_TAG);
            write_string(out, &message);
            write_number(out, quota);
            write_number(out, requested);
        }
    }
    true
}

fn write_string(out: &dyn ValueSerializerHelper, string: &str) {
    out.write_uint32(u32::try_from(string.len()).unwrap_or(u32::MAX));
    out.write_raw_bytes(string.as_bytes());
}

/// A nullable `double`: whether there is one, then it.
fn write_number(out: &dyn ValueSerializerHelper, number: Option<f64>) {
    out.write_uint32(u32::from(number.is_some()));
    if let Some(number) = number {
        out.write_double(number);
    }
}

fn read_string(input: &dyn ValueDeserializerHelper) -> Option<String> {
    let mut length = 0;
    if !input.read_uint32(&mut length) {
        return None;
    }
    let bytes = input.read_raw_bytes(usize::try_from(length).ok()?)?;
    String::from_utf8(bytes.to_vec()).ok()
}

fn read_number(input: &dyn ValueDeserializerHelper) -> Option<Option<f64>> {
    let mut present = 0;
    if !input.read_uint32(&mut present) {
        return None;
    }
    if present == 0 {
        return Some(None);
    }
    let mut number = 0.0;
    input.read_double(&mut number).then_some(Some(number))
}

/// A `DOMException` of either kind (`tag` says which), read back as a new
/// one of the current realm. `None` if the bytes are cut short.
fn read_dom_exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    tag: u32,
    input: &dyn ValueDeserializerHelper,
) -> Option<v8::Local<'s, v8::Object>> {
    let mut scope = state::scope(scope)?;
    if tag == DOM_EXCEPTION_TAG {
        let name = read_string(input)?;
        let message = read_string(input)?;
        crate::exception::new_dom_exception(&mut scope, &name, &message)
    } else {
        let message = read_string(input)?;
        let quota = read_number(input)?;
        let requested = read_number(input)?;
        crate::exception::new_quota_exceeded_error(&mut scope, &message, quota, requested)
    }
}

/// The backing stores of the `ArrayBuffer`s a message transfers, in order
/// (`SerializedMessage::transferred`, which only this engine reads).
struct Transferred(Vec<v8::SharedRef<v8::BackingStore>>);

/// Throws a `DataCloneError` with `message`.
fn throw_data_clone_error(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    if let Some(mut scope) = state::scope(scope) {
        crate::exception::throw_dom_exception(&mut scope, "DataCloneError", message);
    }
}

/// The serializer's delegate: the ports transferred (their wrappers, in
/// order) are written as their places; any other platform object is a
/// `DataCloneError`, as V8's own refusals (a function) are.
struct ForMessage {
    ports: Vec<v8::Global<v8::Object>>,
}

impl v8::ValueSerializerImpl for ForMessage {
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        message: v8::Local<'s, v8::String>,
    ) {
        let message = message.to_rust_string_lossy(scope);
        throw_data_clone_error(scope, &message);
    }

    // A DOM object's wrapper is an API wrapper, which V8 would otherwise
    // clone as a plain object.
    fn has_custom_host_object(&self, _isolate: &v8::Isolate) -> bool {
        true
    }

    fn is_host_object<'s>(
        &self,
        _scope: &mut v8::PinScope<'s, '_>,
        object: v8::Local<'s, v8::Object>,
    ) -> Option<bool> {
        Some(object.is_api_wrapper())
    }

    fn write_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        object: v8::Local<'s, v8::Object>,
        value_serializer: &dyn ValueSerializerHelper,
    ) -> Option<bool> {
        if write_dom_exception(scope, object, value_serializer) {
            return Some(true);
        }
        let place = self
            .ports
            .iter()
            .position(|port| v8::Local::new(scope, port).strict_equals(object.into()));
        if let Some(place) = place {
            value_serializer.write_uint32(PORT_TAG);
            value_serializer.write_uint32(u32::try_from(place).unwrap_or(u32::MAX));
            return Some(true);
        }
        let name = object.get_constructor_name().to_rust_string_lossy(scope);
        let message = if name == "MessagePort" {
            // Blink's message.
            "A MessagePort could not be cloned because it was not transferred.".to_owned()
        } else {
            format!("{name} object could not be cloned.")
        };
        throw_data_clone_error(scope, &message);
        None
    }

    fn get_shared_array_buffer_id<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        _shared_array_buffer: v8::Local<'s, v8::SharedArrayBuffer>,
    ) -> Option<u32> {
        // Only a cross-origin isolated agent shares memory; machopo's aren't.
        throw_data_clone_error(scope, "SharedArrayBuffer can't be cloned here.");
        None
    }

    fn get_wasm_module_transfer_id(
        &self,
        scope: &mut v8::PinScope<'_, '_>,
        _module: v8::Local<v8::WasmModuleObject>,
    ) -> Option<u32> {
        throw_data_clone_error(scope, "WebAssembly.Module can't be cloned here.");
        None
    }
}

/// The deserializer's delegate: a host object is a transferred port, read
/// as its place among `ports` (the message's, wrapped where it is read),
/// or a `DOMException`.
struct FromMessage {
    ports: Vec<v8::Global<v8::Object>>,
}

impl v8::ValueDeserializerImpl for FromMessage {
    fn read_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        value_deserializer: &dyn ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        let (mut tag, mut place) = (0, 0);
        if !value_deserializer.read_uint32(&mut tag) {
            return None;
        }
        if tag == DOM_EXCEPTION_TAG || tag == QUOTA_EXCEEDED_ERROR_TAG {
            return read_dom_exception(scope, tag, value_deserializer);
        }
        if tag != PORT_TAG || !value_deserializer.read_uint32(&mut place) {
            return None;
        }
        let port = self.ports.get(usize::try_from(place).ok()?)?;
        Some(v8::Local::new(scope, port))
    }
}

/// What a transfer list argument is (`dom_api::TransferArg`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferForm {
    Sequence,
    Options,
    SequenceOrOptions,
}

/// The items of a transfer list, `value` as `form` says: a sequence
/// (undefined: none; anything else that isn't one a `TypeError`), or an
/// options dictionary whose `transfer` is one (undefined or null, or no
/// `transfer`: none), or either (an iterable object is the sequence, as
/// WebIDL's overload resolution has it). `None` if it threw.
fn transfer_items<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    form: TransferForm,
) -> Option<Vec<v8::Local<'s, v8::Value>>> {
    if value.is_undefined() {
        return Some(Vec::new());
    }
    let options = match form {
        TransferForm::Sequence => false,
        TransferForm::Options => true,
        TransferForm::SequenceOrOptions => match v8::Local::<v8::Object>::try_from(value) {
            Ok(object) => {
                let iterator = v8::Symbol::get_iterator(scope);
                object.get(scope, iterator.into())?.is_undefined()
            }
            Err(_) => value.is_null(),
        },
    };
    let mut list = value;
    if options {
        let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
            // Null is the dictionary's defaults; another primitive a
            // `TypeError` as for any dictionary.
            if value.is_null() {
                return Some(Vec::new());
            }
            crate::throw_type_error(
                scope,
                "The provided value is not of type 'StructuredSerializeOptions'.",
            );
            return None;
        };
        let key = v8::String::new(scope, "transfer")?;
        list = object.get(scope, key.into())?;
        if list.is_undefined() {
            return Some(Vec::new());
        }
    }
    let mut items = Vec::new();
    crate::observable_array::for_each_item(scope, list, |_, item| {
        items.push(item);
        Some(())
    })?;
    Some(items)
}

/// `value` serialized for a message with `transfer`'s ports and
/// `ArrayBuffer`s transferred (HTML's `StructuredSerializeWithTransfer`), in
/// the spec's order: the transfer list first, anything but a port or a
/// buffer, a duplicate or `source` (the posting port) a `DataCloneError`;
/// then the value; then, as serializing may have run script, a buffer
/// detached or not detachable, a port shipped or closed, a
/// `DataCloneError` too; then the buffers are detached, their backing
/// stores going with the message. The serialization and the ports' wrappers
/// (the operation ships them); `None` with the exception pending.
pub(crate) fn serialize_message<'s>(
    scope: &mut Scope<'_, 's, '_>,
    value: v8::Local<'s, v8::Value>,
    transfer: v8::Local<'s, v8::Value>,
    form: TransferForm,
    source: Option<v8::Local<'s, v8::Object>>,
) -> Option<(SerializedMessage, Vec<v8::Local<'s, v8::Object>>)> {
    let items = transfer_items(scope.scope(), transfer, form)?;
    let mut ports: Vec<(usize, v8::Local<'s, v8::Object>)> = Vec::new();
    let mut buffers: Vec<(usize, v8::Local<'s, v8::ArrayBuffer>)> = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(item) {
            if buffers.iter().any(|(_, other)| other.strict_equals(item)) {
                throw_data_clone_error(
                    scope.scope(),
                    &format!(
                        "ArrayBuffer at index {index} is a duplicate of an earlier ArrayBuffer."
                    ),
                );
                return None;
            }
            buffers.push((index, buffer));
            continue;
        }
        let port = is_port(scope, item);
        let Some(port) = port else {
            throw_data_clone_error(
                scope.scope(),
                &format!("Value at index {index} does not have a transferable type."),
            );
            return None;
        };
        let problem = if source.is_some_and(|source| source.strict_equals(item)) {
            Some("contains the source port")
        } else if ports.iter().any(|(_, other)| other.strict_equals(item)) {
            Some("is a duplicate of an earlier port")
        } else {
            None
        };
        if let Some(problem) = problem {
            throw_data_clone_error(scope.scope(), &format!("Port at index {index} {problem}."));
            return None;
        }
        ports.push((index, port));
    }
    let bytes = {
        let scope = scope.scope();
        let context = scope.get_current_context();
        let wrappers = ports
            .iter()
            .map(|(_, port)| v8::Global::new(scope, *port))
            .collect();
        let serializer = v8::ValueSerializer::new(scope, Box::new(ForMessage { ports: wrappers }));
        for (place, (_, buffer)) in buffers.iter().enumerate() {
            serializer.transfer_array_buffer(u32::try_from(place).unwrap_or(u32::MAX), *buffer);
        }
        serializer.write_header();
        match serializer.write_value(context, value) {
            Some(true) => serializer.release(),
            _ => return None,
        }
    };
    let mut problems: Vec<(usize, String)> = buffers
        .iter()
        .filter_map(|(index, buffer)| {
            let problem = if buffer.was_detached() {
                "is already detached"
            } else if !buffer.is_detachable() {
                "is not detachable"
            } else {
                return None;
            };
            Some((*index, format!("ArrayBuffer at index {index} {problem}.")))
        })
        .collect();
    for (index, port) in &ports {
        let unusable = {
            let (state, mut no_gc) = scope.state();
            state.dom.read(|cx| {
                no_gc
                    .unwrap::<EventTargetImpl>(cx.gc, *port)
                    .is_none_or(|port| cx.message_port_unusable(port))
            })
        };
        if unusable {
            problems.push((
                *index,
                format!("Port at index {index} is already detached."),
            ));
        }
    }
    problems.sort_by_key(|(index, _)| *index);
    if let Some((_, message)) = problems.first() {
        throw_data_clone_error(scope.scope(), message);
        return None;
    }
    let stores: Vec<_> = buffers
        .iter()
        .map(|(_, buffer)| {
            let store = buffer.get_backing_store();
            buffer.detach(None);
            store
        })
        .collect();
    let transferred =
        (!stores.is_empty()).then(|| Box::new(Transferred(stores)) as Box<dyn Any + Send>);
    Some((
        SerializedMessage { bytes, transferred },
        ports.into_iter().map(|(_, port)| port).collect(),
    ))
}

/// `value` serialized for storage (HTML's `StructuredSerializeForStorage`:
/// a history's state): nothing transferred, any platform object a
/// `DataCloneError`. `None` with the exception pending.
pub(crate) fn serialize_for_storage<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<Vec<u8>> {
    let context = scope.get_current_context();
    let serializer = v8::ValueSerializer::new(scope, Box::new(ForMessage { ports: Vec::new() }));
    serializer.write_header();
    match serializer.write_value(context, value) {
        Some(true) => Some(serializer.release()),
        _ => None,
    }
}

/// `value` as a message port's wrapper, if it is one.
fn is_port<'s>(
    scope: &mut Scope<'_, 's, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Object>> {
    let object = v8::Local::<v8::Object>::try_from(value).ok()?;
    if !bindings::is_wrapper::<EventTargetImpl>(scope.scope(), value) {
        return None;
    }
    let (state, mut no_gc) = scope.state();
    state
        .dom
        .read(|cx| {
            no_gc
                .unwrap::<EventTargetImpl>(cx.gc, object)
                .is_some_and(|target| cx.is_message_port(target))
        })
        .then_some(object)
}

/// `payload` deserialized in the current context, its host objects
/// `ports`' wrappers and its transferred buffers new `ArrayBuffer`s of
/// their backing stores: none if it doesn't deserialize.
pub(crate) fn deserialize_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    payload: SerializedMessage,
    ports: Vec<v8::Global<v8::Object>>,
) -> Option<v8::Local<'s, v8::Value>> {
    let context = scope.get_current_context();
    v8::tc_scope!(let tc, scope);
    let deserializer =
        v8::ValueDeserializer::new(tc, Box::new(FromMessage { ports }), &payload.bytes);
    let stores = payload
        .transferred
        .and_then(|transferred| transferred.downcast::<Transferred>().ok())
        .map_or_else(Vec::new, |transferred| transferred.0);
    for (place, store) in stores.into_iter().enumerate() {
        let buffer = v8::ArrayBuffer::with_backing_store(tc, &store);
        deserializer.transfer_array_buffer(u32::try_from(place).unwrap_or(u32::MAX), buffer);
    }
    let value = match deserializer.read_header(context) {
        Some(true) => deserializer.read_value(context),
        _ => None,
    };
    value.map(|value| v8::Local::new(tc, value))
}
