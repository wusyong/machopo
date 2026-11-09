//! Finding elements: `getElementById` (NonElementParentNode's,
//! https://dom.spec.whatwg.org/#interface-nonelementparentnode), and by
//! selectors (https://dom.spec.whatwg.org/#scope-match-a-selectors-string):
//! `querySelector`, `querySelectorAll` (ParentNode's), `matches` and
//! `closest` (Element's), each parsing its selectors once a thread
//! (`style::SelectorQuery`, Blink's `SelectorQuery`).

use std::rc::Rc;

use dom::{NodeList, NodeRef};
use elise::Gc;
use style::SelectorQuery;

use crate::exception::{DomException, ExceptionName};
use crate::{ApiCx, ApiError};

pub(crate) fn get_element_by_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    element_id: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.element_by_id(this, element_id))
}

/// `selectors` parsed: an invalid list is a `SyntaxError`.
fn parse(selectors: &str) -> Result<Rc<SelectorQuery>, ApiError> {
    SelectorQuery::parse(selectors).ok_or_else(|| {
        ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message: format!("'{selectors}' is not a valid selector"),
        })
    })
}

pub(crate) fn query_selector<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    selectors: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(parse(selectors)?.query_first(cx, this))
}

/// What `querySelectorAll` finds, in tree order, with no list made
/// (WebDriver's Find Elements).
pub(crate) fn select_all<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    selectors: &str,
) -> Result<Vec<NodeRef<'gc>>, ApiError> {
    Ok(parse(selectors)?.query_all(cx, root))
}

/// `querySelectorAll`: a static list, a new one each call.
pub(crate) fn query_selector_all<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    selectors: &str,
) -> Result<Gc<'gc, NodeList>, ApiError> {
    let found = parse(selectors)?.query_all(cx, this);
    Ok(cx.static_node_list(&found))
}

/// `matches` and `webkitMatchesSelector`.
pub(crate) fn matches<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    selectors: &str,
) -> Result<bool, ApiError> {
    Ok(parse(selectors)?.matches(cx, this))
}

pub(crate) fn closest<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    selectors: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(parse(selectors)?.closest(cx, this))
}

pub(crate) fn node_list_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeList>,
) -> Result<u32, ApiError> {
    Ok(cx.node_list_length(this))
}

/// `item(index)`, and the indexed getter: null past the last.
pub(crate) fn node_list_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeList>,
    index: u32,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.node_list_item(this, index))
}
