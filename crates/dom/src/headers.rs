//! Fetch's `Headers` (Blink's `core/fetch/headers.h`, with its
//! `FetchHeaderList`): a header list (names and values, in order, each
//! character a byte) and its guard. The list's algorithms are
//! `protocol::network::header_list`'s; the
//! interface's are `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, JsEdge, LocalExternal};

use crate::{Cx, sites};

/// Fetch's headers guard: which changes script may make to a `Headers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadersGuard {
    Immutable,
    Request,
    /// A `no-cors` request's: only the no-CORS-safelisted headers.
    RequestNoCors,
    Response,
    None,
}

impl HeadersGuard {
    const ALL: [Self; 5] = [
        Self::Immutable,
        Self::Request,
        Self::RequestNoCors,
        Self::Response,
        Self::None,
    ];

    fn raw(self) -> u8 {
        match self {
            Self::Immutable => 0,
            Self::Request => 1,
            Self::RequestNoCors => 2,
            Self::Response => 3,
            Self::None => 4,
        }
    }

    fn from_raw(raw: u8) -> Self {
        Self::ALL[usize::from(raw)]
    }
}

/// A header list: names and values, in order.
type HeaderList = Vec<(String, String)>;

#[derive(GcObject)]
pub struct Headers {
    /// Its header list; taken out while it is read or changed, and put
    /// back.
    list: LocalExternal<Cell<HeaderList>>,
    /// Its guard (`HeadersGuard::raw`).
    guard: Cell<u8>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new `Headers` of an empty list, guarded by `guard`.
    pub fn create_headers(&self, guard: HeadersGuard) -> Gc<'gc, Headers> {
        self.gc.alloc_in(
            &sites::OTHER,
            HeadersInit {
                list: LocalExternal::new(Cell::new(Vec::new())),
                guard: Cell::new(guard.raw()),
                wrapper: (),
            },
        )
    }

    pub fn headers_guard(&self, headers: Gc<'gc, Headers>) -> HeadersGuard {
        HeadersGuard::from_raw(headers.guard.get())
    }

    pub fn set_headers_guard(&self, headers: Gc<'gc, Headers>, guard: HeadersGuard) {
        headers.guard.set(guard.raw());
    }

    /// `f` with `headers`' list, which it may change.
    pub fn with_header_list<R>(
        &self,
        headers: Gc<'gc, Headers>,
        f: impl FnOnce(&mut Vec<(String, String)>) -> R,
    ) -> R {
        let cell = &*headers.into_ref().list;
        let mut list = cell.take();
        let result = f(&mut list);
        cell.set(list);
        result
    }

    pub(crate) fn headers_wrapper_edge(&self, headers: Gc<'gc, Headers>) -> &'gc JsEdge {
        &headers.into_ref().wrapper
    }
}
