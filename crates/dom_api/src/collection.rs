//! `HTMLCollection` (https://dom.spec.whatwg.org/#interface-htmlcollection)
//! and `getElementsByTagName`, which makes one: live, its elements found
//! at each ask (`dom::HtmlCollection`), a new one each call; and
//! `document.all` (HTML's `HTMLAllCollection`).

use dom::{HtmlCollection, NodeRef};
use elise::Gc;

use crate::typed::CollectionOrElement;
use crate::{ApiCx, ApiError};

/// `getElementsByTagName` of a document or an element.
pub(crate) fn get_elements_by_tag_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified: &str,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.elements_by_tag_name(this, qualified))
}

/// `getElementsByClassName` of a document or an element.
pub(crate) fn get_elements_by_class_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    class_names: &str,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.elements_by_class_name(this, class_names))
}

/// What `getElementsByTagName` finds now, in tree order (WebDriver's
/// Find Elements).
pub(crate) fn elements_by_tag_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    qualified: &str,
) -> Vec<NodeRef<'gc>> {
    let collection = cx.elements_by_tag_name(root, qualified);
    (0..cx.collection_length(collection))
        .filter_map(|index| cx.collection_item(collection, index))
        .collect()
}

pub(crate) fn length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
) -> Result<u32, ApiError> {
    Ok(cx.collection_length(this))
}

/// `item(index)`, and the indexed getter: null past the last.
pub(crate) fn item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    index: u32,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.collection_item(this, index))
}

/// `document.all`: every element of the document, the same each time.
pub(crate) fn document_all<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, dom::KeptCollection::All))
}

/// `HTMLAllCollection`'s `item(nameOrIndex)`, which calling it calls too:
/// the element at an index (a string that is an array index is one), else
/// what `namedItem` gives; `null` without an argument.
pub(crate) fn all_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    name_or_index: Option<&str>,
) -> Result<Option<CollectionOrElement<'gc>>, ApiError> {
    let Some(key) = name_or_index else {
        return Ok(None);
    };
    match array_index(key) {
        Some(index) => Ok(cx
            .collection_item(this, index)
            .map(CollectionOrElement::Element)),
        None => all_named_item(cx, this, key),
    }
}

/// `HTMLAllCollection`'s `namedItem` (HTML's "get the 'all'-named
/// element(s)"): the one element of the document's whose `id` is `name`,
/// or whose `name` is for the elements that may have one there; a live
/// `HTMLCollection` of them, the same while the document lives, if there
/// are several; `null` for none.
pub(crate) fn all_named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    name: &str,
) -> Result<Option<CollectionOrElement<'gc>>, ApiError> {
    let named = cx.all_named_collection(cx.collection_root(this), name);
    Ok(match cx.collection_length(named) {
        0 => None,
        1 => cx
            .collection_item(named, 0)
            .map(CollectionOrElement::Element),
        _ => Some(CollectionOrElement::Collection(named)),
    })
}

/// `key` as an array index (ECMAScript's: the canonical numeric string of
/// an integer below 2^32 - 1), if it is one.
fn array_index(key: &str) -> Option<u32> {
    let index: u32 = key.parse().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

pub(crate) fn named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    key: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.collection_named_item(this, key))
}
