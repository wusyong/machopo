//! Web Storage (https://html.spec.whatwg.org/#storage): a window's
//! `localStorage` and `sessionStorage`, each a `Storage`, its area's items
//! in the DOM (`dom`'s `storage`). The window gets each from the host the
//! first time it is asked for, or a `storage` listener is added to it, the
//! host waiting for the browser (Blink's `CachedStorageArea` loads with a
//! `[Sync]` `GetAll`); the members read and change the DOM's copy, and the
//! engine hands the host what changed before the call returns. `storage`
//! events are `StorageEvent`s, fired in tasks of their own.

use dom::names;
use dom::{
    DomString, Event, EventFlags, Storage, StorageEventValues, StorageKind, TargetRoot, Window,
};
use elise::{Gc, Persistent};
use protocol::FrameToken;
pub use protocol::renderer::OpenedStorageArea;

use crate::exception::{DomException, ExceptionName};
use crate::interfaces::StorageEventInit;
use crate::typed::{OpFront, ScriptHost, StorageAreaObject};
use crate::{ApiCx, ApiError, dispatch};

/// The host's side of Web Storage (Blink's `StorageController` and the
/// areas' `mojom::StorageArea`s), the engine's to call.
pub trait StorageHost {
    /// Frame `frame`'s document opens its `localStorage` or
    /// `sessionStorage` (`kind`), waiting for the answer: its area, with
    /// its items unless the DOM has the area already; none if its document
    /// may have no storage.
    fn open_storage_area(&self, frame: FrameToken, kind: StorageKind) -> Option<OpenedStorageArea>;

    /// Script changed an area, which the DOM has already.
    fn storage_changed(&self, change: dom::StorageChange);
}

/// `localStorage`: the window's, the same each time (`storage`).
pub(crate) fn local_storage<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Window>,
) -> Result<Option<Persistent<Storage>>, ApiError> {
    storage(host, &this, StorageKind::Local)
}

/// `sessionStorage`: the window's, the same each time (`storage`).
pub(crate) fn session_storage<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Window>,
) -> Result<Option<Persistent<Storage>>, ApiError> {
    storage(host, &this, StorageKind::Session)
}

/// The window's `Storage` of `kind`, the same each time once it has one;
/// the host opens its area the first time (HTML's "obtain a storage
/// bottle map"). A `SecurityError` if its document's origin is opaque or
/// the host gives it none; null for a window shown in no frame any more
/// (Blink's, for a window with no frame).
fn storage<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    kind: StorageKind,
) -> Result<Option<Persistent<Storage>>, ApiError> {
    let (storage, document, active, opaque) = host.read(|cx| {
        let window = window.get(cx.gc);
        let storage = cx
            .window_storage(window, kind)
            .map(|storage| Persistent::new(cx.gc, storage));
        let document = cx.window_document(window);
        let active = crate::history::is_fully_active(cx, document);
        let opaque = !cx
            .document_origin(document)
            .is_some_and(|origin| origin.is_tuple());
        (storage, cx.root(document), active, opaque)
    });
    if storage.is_some() {
        return Ok(storage);
    }
    if !active {
        return Ok(None);
    }
    if opaque {
        return Err(denied());
    }
    let Some(opened) = host.open_storage_area(&document, kind) else {
        return Err(denied());
    };
    Ok(Some(host.mutate(|cx| {
        let storage = cx.set_window_storage(
            window.get(cx.gc),
            kind,
            opened.area,
            opened.items,
            protocol::renderer::PER_STORAGE_AREA_QUOTA,
        );
        Persistent::new(cx.gc, storage)
    })))
}

/// The windows given `storage` listeners in the operation (`dom`'s
/// `take_storage_listened`): their `localStorage` and `sessionStorage`
/// opened, to hear of their changes (Blink's
/// `DOMWindowStorageController::DidAddEventListener`); one that may have
/// none goes without.
pub fn open_for_listeners<H: ScriptHost>(host: &mut H) {
    for window in host.mutate(|cx| cx.take_storage_listened()) {
        for kind in [StorageKind::Local, StorageKind::Session] {
            let _ = storage(host, &window, kind);
        }
    }
}

/// A `storage` event of `info` at `document`'s window (its task runs),
/// its `storageArea` the window's `Storage` of the kind.
pub(crate) fn run_storage_event<H: ScriptHost>(
    host: &mut H,
    document: &dom::NodeRoot,
    info: dom::StorageEventInfo,
) {
    let Some((window, event)) = host.mutate(|cx| {
        let window = cx.window_of(document.get(cx.gc))?;
        let event = cx.create_event(
            names::event::STORAGE,
            EventFlags {
                trusted: true,
                ..EventFlags::default()
            },
            Some(cx.window_context(window)),
        );
        cx.set_storage_event_data(
            event,
            StorageEventValues {
                key: info.key,
                old_value: info.old_value,
                new_value: info.new_value,
                url: info.url,
            },
            cx.window_storage(window, info.kind),
        );
        Some((
            Persistent::new(cx.gc, window),
            Persistent::new(cx.gc, event),
        ))
    }) else {
        return;
    };
    dispatch(host, &TargetRoot::Window(window), &event);
}

/// `new StorageEvent(type, eventInitDict)`.
pub(crate) fn construct_storage_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &StorageEventInit<O::Node>,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_context(cx),
    );
    let storage_area = init
        .storageArea
        .as_ref()
        .map(|StorageAreaObject(storage)| op.object::<Storage>(cx, storage));
    cx.set_storage_event_data(
        event,
        StorageEventValues {
            key: init.key.clone(),
            old_value: init.oldValue.clone(),
            new_value: init.newValue.clone(),
            url: init.url.clone(),
        },
        storage_area,
    );
    Ok(event)
}

/// `initStorageEvent(...)`: nothing while it is being dispatched.
#[expect(clippy::too_many_arguments, reason = "HTML's arguments")]
pub(crate) fn init_storage_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    key: Option<&str>,
    old_value: Option<&str>,
    new_value: Option<&str>,
    url: &str,
    storage_area: Option<Gc<'gc, Storage>>,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    cx.set_storage_event_data(
        this,
        StorageEventValues {
            key: key.map(str::to_owned),
            old_value: old_value.map(str::to_owned),
            new_value: new_value.map(str::to_owned),
            url: url.to_owned(),
        },
        storage_area,
    );
    Ok(())
}

fn event_values<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> &'gc StorageEventValues {
    cx.storage_event_data(event)
        .expect("the front end's checks: a StorageEvent")
        .0
}

pub(crate) fn event_key<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<DomString>, ApiError> {
    Ok(event_values(cx, this).key.as_deref().map(DomString::new))
}

pub(crate) fn event_old_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<DomString>, ApiError> {
    Ok(event_values(cx, this)
        .old_value
        .as_deref()
        .map(DomString::new))
}

pub(crate) fn event_new_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<DomString>, ApiError> {
    Ok(event_values(cx, this)
        .new_value
        .as_deref()
        .map(DomString::new))
}

pub(crate) fn event_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomString, ApiError> {
    Ok(DomString::new(&event_values(cx, this).url))
}

pub(crate) fn event_storage_area<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<Gc<'gc, Storage>>, ApiError> {
    Ok(cx
        .storage_event_data(this)
        .expect("the front end's checks: a StorageEvent")
        .1)
}

fn denied() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::Security,
        message: "Access is denied for this document.".into(),
    })
}

/// `length`: how many items the area has.
pub(crate) fn length<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Storage>) -> Result<u32, ApiError> {
    // An area holds 10 MiB at most: far fewer items than `u32::MAX`.
    Ok(u32::try_from(cx.storage_length(this)).unwrap_or(u32::MAX))
}

/// `key(index)`: the key at `index` in the area's order, or null.
pub(crate) fn key<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Storage>,
    index: u32,
) -> Result<Option<DomString>, ApiError> {
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    Ok(cx.storage_key(this, index).map(DomString::from))
}

/// `getItem(key)`, and the named getter: `key`'s value, or null.
pub(crate) fn get_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Storage>,
    key: &str,
) -> Result<Option<DomString>, ApiError> {
    Ok(cx.storage_get(this, key).map(DomString::from))
}

/// `setItem(key, value)`, and the named setter.
pub(crate) fn set_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Storage>,
    key: &str,
    value: &str,
) -> Result<(), ApiError> {
    if !cx.storage_set(this, key, value) {
        // Chrome's message.
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::QuotaExceeded,
            message: format!(
                "Failed to execute 'setItem' on 'Storage': Setting the value of '{key}' exceeded the quota."
            ),
        }));
    }
    Ok(())
}

/// `removeItem(key)`, and the named deleter.
pub(crate) fn remove_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Storage>,
    key: &str,
) -> Result<(), ApiError> {
    cx.storage_remove(this, key);
    Ok(())
}

/// `clear()`.
pub(crate) fn clear<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Storage>) -> Result<(), ApiError> {
    cx.storage_clear(this);
    Ok(())
}

/// The named properties' names: the keys, in order.
pub(crate) fn names<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Storage>,
) -> Result<Vec<DomString>, ApiError> {
    Ok(cx
        .storage_keys(this)
        .into_iter()
        .map(DomString::from)
        .collect())
}
