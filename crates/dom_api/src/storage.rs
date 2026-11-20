//! `localStorage` (https://html.spec.whatwg.org/#storage): a window's
//! `Storage`, its area's items in the DOM (`dom`'s `storage`). The window
//! gets it from the host the first time it is asked for, the host waiting
//! for the browser (Blink's `CachedStorageArea` loads with a `[Sync]`
//! `GetAll`); the members read and change the DOM's copy, and the engine
//! hands the host what changed before the call returns.

use dom::{DomString, Storage, Window};
use elise::{Gc, Persistent};
use protocol::FrameToken;
pub use protocol::renderer::OpenedStorageArea;

use crate::exception::{DomException, ExceptionName};
use crate::typed::ScriptHost;
use crate::{ApiCx, ApiError};

/// The host's side of `localStorage` (Blink's `StorageController` and the
/// areas' `mojom::StorageArea`s), the engine's to call.
pub trait StorageHost {
    /// Frame `frame`'s document opens its `localStorage`, waiting for the
    /// answer: its area, with its items unless the DOM has the area
    /// already; none if its document may have no storage.
    fn open_local_storage(&self, frame: FrameToken) -> Option<OpenedStorageArea>;

    /// Script changed an area, which the DOM has already.
    fn storage_changed(&self, change: dom::StorageChange);
}

/// `localStorage`: the window's `Storage`, the same each time once it has
/// one; the host opens its area the first time (HTML's "obtain a local
/// storage bottle map"). A `SecurityError` if its document's origin is
/// opaque or the host gives it none; null for a window shown in no frame
/// any more (Blink's, for a window with no frame).
pub(crate) fn local_storage<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Window>,
) -> Result<Option<Persistent<Storage>>, ApiError> {
    let (storage, document, active, opaque) = host.read(|cx| {
        let window = this.get(cx.gc);
        let storage = cx
            .window_local_storage(window)
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
    let Some(opened) = host.open_local_storage(&document) else {
        return Err(denied());
    };
    Ok(Some(host.mutate(|cx| {
        let storage = cx.set_window_local_storage(this.get(cx.gc), opened.area, opened.items);
        Persistent::new(cx.gc, storage)
    })))
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
    cx.storage_set(this, key, value);
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
