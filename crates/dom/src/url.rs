//! `URL` and `URLSearchParams` (URL Standard; Blink's `DOMURL` and
//! `URLSearchParams`): a parsed URL, and a list of names and values; of no
//! realm. A `URL` keeps its `searchParams`, made with it, which points
//! back at it (URL Standard's "URL object"): strongly, where Blink's is a
//! `WeakMember`. Only the params alive keep a URL nobody sees, which the
//! tracing collector takes with them. The DOM only stores them; URL
//! Standard's steps, which keep the two in step, are `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, JsEdge, LocalExternal, Member};

use crate::{Cx, sites};

#[derive(GcObject)]
pub struct Url {
    search_params: Member<UrlSearchParams>,
    /// Taken and put back (`with_url`): never none out of it.
    url: LocalExternal<Cell<Option<url::Url>>>,
    wrapper: JsEdge,
}

#[derive(GcObject)]
pub struct UrlSearchParams {
    /// The `URL` whose `searchParams` it is, if any.
    url: Member<Url>,
    /// URL Standard's "list": names and values, in order; taken and put
    /// back.
    list: LocalExternal<Cell<PairList>>,
    wrapper: JsEdge,
}

/// URL Standard's list of names and values.
type PairList = Vec<(String, String)>;

impl<'gc> Cx<'_, 'gc> {
    /// A new `URL` of `url`, with its `searchParams` of `list` (its
    /// query's).
    pub fn create_url(&self, url: url::Url, list: Vec<(String, String)>) -> Gc<'gc, Url> {
        let made = self.gc.alloc_in(
            &sites::OTHER,
            UrlInit {
                search_params: None,
                url: LocalExternal::new(Cell::new(Some(url))),
                wrapper: (),
            },
        );
        let params = self.create_url_search_params(list);
        params.url.set(self.gc, Some(made));
        made.search_params.set(self.gc, Some(params));
        made
    }

    /// `f` with `url`'s URL, which it may change.
    pub fn with_url<R>(&self, url: Gc<'gc, Url>, f: impl FnOnce(&mut url::Url) -> R) -> R {
        let cell = &*url.into_ref().url;
        let mut value = cell.take().expect("a URL's value, put back after each use");
        let result = f(&mut value);
        cell.set(Some(value));
        result
    }

    pub fn url_search_params(&self, url: Gc<'gc, Url>) -> Gc<'gc, UrlSearchParams> {
        url.search_params
            .get(self.gc)
            .expect("a URL has its searchParams")
    }

    /// A new `URLSearchParams` of `list`, of no `URL`.
    pub fn create_url_search_params(
        &self,
        list: Vec<(String, String)>,
    ) -> Gc<'gc, UrlSearchParams> {
        self.gc.alloc_in(
            &sites::OTHER,
            UrlSearchParamsInit {
                url: None,
                list: LocalExternal::new(Cell::new(list)),
                wrapper: (),
            },
        )
    }

    /// The `URL` whose `searchParams` `params` is, if any.
    pub fn search_params_url(&self, params: Gc<'gc, UrlSearchParams>) -> Option<Gc<'gc, Url>> {
        params.url.get(self.gc)
    }

    /// `f` with `params`' list, which it may change.
    pub fn with_search_params<R>(
        &self,
        params: Gc<'gc, UrlSearchParams>,
        f: impl FnOnce(&mut Vec<(String, String)>) -> R,
    ) -> R {
        let cell = &*params.into_ref().list;
        let mut list = cell.take();
        let result = f(&mut list);
        cell.set(list);
        result
    }

    pub(crate) fn url_wrapper_edge(&self, url: Gc<'gc, Url>) -> &'gc JsEdge {
        &url.into_ref().wrapper
    }

    pub(crate) fn search_params_wrapper_edge(
        &self,
        params: Gc<'gc, UrlSearchParams>,
    ) -> &'gc JsEdge {
        &params.into_ref().wrapper
    }
}
