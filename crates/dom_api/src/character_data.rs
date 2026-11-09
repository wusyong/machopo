//! `CharacterData` (https://dom.spec.whatwg.org/#interface-characterdata): a
//! text's data, its length in UTF-16 code units, its edits (DOM's "replace
//! data", by UTF-16 offsets on the UTF-8 the DOM keeps); and `Text`'s
//! (`splitText`, `wholeText`) and the constructors of `Text`, `Comment`.

use dom::{NodeRef, StrRef};

use crate::typed::{DomStr, OpFront};
use crate::{ApiCx, ApiError};

/// The data of a text where it is, the table's types promising one.
fn data_of<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> StrRef<'gc> {
    cx.text_data(this).expect("a character data's data")
}

pub(crate) fn data<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<StrRef<'gc>, ApiError> {
    Ok(data_of(cx, this))
}

pub(crate) fn set_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_data(this, value);
    Ok(())
}

/// `length`: in UTF-16 code units, at most `u32::MAX` (WebIDL's
/// `unsigned long`; no string the DOM holds is that long).
pub(crate) fn length<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(u32::try_from(data_of(cx, this).borrow().utf16_len()).unwrap_or(u32::MAX))
}

/// An `IndexSizeError` for an offset past the data's length.
fn index_size(offset: u32, length: usize) -> ApiError {
    ApiError::Dom(crate::DomException {
        name: crate::ExceptionName::IndexSize,
        message: format!("The offset {offset} is larger than the data's length ({length})."),
    })
}

/// The data's length in UTF-16 code units, and `offset` within it (else an
/// `IndexSizeError`).
fn checked_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
) -> Result<usize, ApiError> {
    let length = data_of(cx, this).borrow().utf16_len();
    let at = usize::try_from(offset).unwrap_or(usize::MAX);
    if at > length {
        return Err(index_size(offset, length));
    }
    Ok(at)
}

/// DOM's "replace data" with its checks: `count` UTF-16 code units from
/// `offset` (fewer at the end) replaced by `data`.
fn replace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
    count: u32,
    data: &str,
) -> Result<(), ApiError> {
    let at = checked_offset(cx, this, offset)?;
    cx.replace_data(this, at, usize::try_from(count).unwrap_or(usize::MAX), data);
    Ok(())
}

/// `substringData(offset, count)`: `count` units from `offset`, fewer at
/// the end; half a surrogate pair cut is U+FFFD (the DOM's strings keep
/// none alone).
pub(crate) fn substring_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
    count: u32,
) -> Result<DomStr<'gc>, ApiError> {
    let at = checked_offset(cx, this, offset)?;
    let data = data_of(cx, this)
        .borrow()
        .utf16_substring(at, usize::try_from(count).unwrap_or(usize::MAX));
    Ok(DomStr::Owned(data.into()))
}

pub(crate) fn append_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    data: &str,
) -> Result<(), ApiError> {
    let end = data_of(cx, this).borrow().utf16_len();
    cx.replace_data(this, end, 0, data);
    Ok(())
}

pub(crate) fn insert_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
    data: &str,
) -> Result<(), ApiError> {
    replace(cx, this, offset, 0, data)
}

pub(crate) fn delete_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
    count: u32,
) -> Result<(), ApiError> {
    replace(cx, this, offset, count, "")
}

pub(crate) fn replace_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
    count: u32,
    data: &str,
) -> Result<(), ApiError> {
    replace(cx, this, offset, count, data)
}

/// `splitText(offset)`: the data from `offset` moved into a new text of
/// the same type and document, put after it in its parent (if it has one),
/// the live ranges with it (`Cx::split_text`); an `IndexSizeError` past
/// the length.
pub(crate) fn split_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    offset: u32,
) -> Result<NodeRef<'gc>, ApiError> {
    checked_offset(cx, this, offset)?;
    Ok(cx.split_text(this, offset))
}

/// `wholeText`: the data of the contiguous texts around it (CDATA sections
/// too: DOM's "contiguous Text nodes"), it among them, joined in order.
pub(crate) fn whole_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let is_text = |node: &NodeRef<'gc>| cx.kind(*node) == dom::NodeKind::Text;
    let mut first = this;
    while let Some(before) = cx.previous_sibling(first).filter(is_text) {
        first = before;
    }
    let mut joined = String::new();
    let mut at = Some(first);
    while let Some(node) = at.filter(is_text) {
        joined.push_str(&data_of(cx, node).borrow().to_str());
        at = cx.next_sibling(node);
    }
    Ok(DomStr::Owned(joined.into()))
}

/// `new Text(data)`, `new Comment(data)`: a node in no tree, of the
/// current realm's document.
pub(crate) fn construct_text<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    data: Option<&str>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.create_text(realm_document(op, cx)?, data.unwrap_or("")))
}

pub(crate) fn construct_comment<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    data: Option<&str>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.create_comment(realm_document(op, cx)?, data.unwrap_or("")))
}

/// The document of the realm a constructor runs in (its window's).
pub(crate) fn realm_document<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("Illegal constructor"))?;
    Ok(cx.window_document(window))
}
