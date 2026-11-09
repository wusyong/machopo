//! HTML's reflection of content attributes
//! (https://html.spec.whatwg.org/#reflecting-content-attributes-in-idl-attributes):
//! a getter and a setter for each rule, by the attribute's fixed name
//! number (a const generic: the table writes `reflect::string::<{
//! local::NAME }>`, as an event handler's type is one), so that the
//! interfaces' many reflected attributes are one line each. Set in no
//! namespace (HTML's "set an attribute value"); `[CEReactions]` is the
//! table's.

use dom::{NameId, NodeRef, QualName, names};

use crate::exception::{DomException, ExceptionName};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// The largest value an `unsigned long` reflection keeps: HTML limits them
/// to what a `long` holds.
const MAX_REFLECTED: u32 = 2_147_483_647;

fn set<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, name: NameId, value: &str) {
    cx.set_attribute_ns(this, QualName::local(&names::get(name)), value);
}

/// A `DOMString`: the attribute's value, else the empty string.
pub(crate) fn string<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::element::reflected(cx, this, N))
}

pub(crate) fn set_string<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, N, value);
    Ok(())
}

/// A `USVString` reflecting a URL (`src`): none for no attribute; else
/// its value parsed against the document's base URL, or as it is if that
/// fails.
pub(crate) fn url<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let Some(value) = cx.attribute_value_by_id(this, N) else {
        return Ok(DomStr::Name(names::NONE));
    };
    let value = value.borrow().to_str().into_owned();
    let resolved = cx
        .resolve_url(cx.document_of(this), &value)
        .map(String::from);
    Ok(DomStr::Owned(dom::DomString::new(
        &resolved.unwrap_or(value),
    )))
}

/// A form's `action` and a submitter's `formAction` (HTML's): the URL the
/// attribute reflects, the document's URL for none or an empty one.
pub(crate) fn action_url<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    if cx
        .attribute_value_by_id(this, N)
        .is_none_or(|value| value.borrow().to_str().is_empty())
    {
        let url = cx
            .document_url(cx.document_of(this))
            .unwrap_or_else(|| dom::DomString::new(""));
        return Ok(DomStr::Owned(url));
    }
    url::<N>(cx, this)
}

/// An enumerated attribute "limited to only known values": the keyword
/// the value matches ASCII case-insensitively, in its canonical form;
/// `missing` without the attribute (the empty string for none), `invalid`
/// with a value that matches none.
pub(crate) fn enumerated<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    local: NameId,
    keywords: &[&'static str],
    missing: Option<&'static str>,
    invalid: &'static str,
) -> DomStr<'gc> {
    let keyword = match cx.attribute_value_by_id(this, local) {
        None => missing.unwrap_or(""),
        Some(value) => {
            let value = value.borrow().to_str().into_owned();
            keywords
                .iter()
                .find(|keyword| keyword.eq_ignore_ascii_case(&value))
                .copied()
                .unwrap_or(invalid)
        }
    };
    DomStr::Owned(dom::DomString::new(keyword))
}

/// A `boolean`: whether the attribute is there; set, it added (empty) or
/// removed.
pub(crate) fn boolean<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx.attribute_value_by_id(this, N).is_some())
}

pub(crate) fn set_boolean<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    if value {
        set(cx, this, N, "");
    } else {
        cx.remove_attribute(this, &names::get(N));
    }
    Ok(())
}

fn integer<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, name: NameId) -> Option<i32> {
    cx.attribute_value_by_id(this, name)
        .and_then(|value| parse_integer(&value.borrow().to_str()))
}

/// A `long` (`start`, `<li>`'s `value`): the attribute's integer, else
/// `D`.
pub(crate) fn long<'gc, const N: NameId, const D: i32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<i32, ApiError> {
    Ok(integer(cx, this, N).unwrap_or(D))
}

/// An `unsigned long` "clamped to the range" [`MIN`, `MAX`] (`colSpan`):
/// the attribute's non-negative integer brought into it (one too large
/// for any integer type is above it, as Blink's
/// `ParseHTMLClampedNonNegativeInteger` saturates), else `D`; set as
/// `set_unsigned`.
pub(crate) fn clamped<'gc, const N: NameId, const D: u32, const MIN: u32, const MAX: u32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<u32, ApiError> {
    Ok(cx
        .attribute_value_by_id(this, N)
        .and_then(|value| parse_non_negative_saturating(&value.borrow().to_str()))
        .map_or(D, |value| {
            u32::try_from(value).unwrap_or(u32::MAX).clamp(MIN, MAX)
        }))
}

/// HTML's rules for parsing non-negative integers, without an upper
/// limit: one too large for a `u64` is `u64::MAX`.
fn parse_non_negative_saturating(text: &str) -> Option<u64> {
    let text = text.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, text.get(1..)?),
        b'+' => (false, text.get(1..)?),
        _ => (false, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let digits = digits.get(..end)?;
    if digits.is_empty() || (negative && digits.bytes().any(|digit| digit != b'0')) {
        return None;
    }
    Some(digits.parse().unwrap_or(u64::MAX))
}

/// A `long` set: its decimal digits.
pub(crate) fn set_long<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: i32,
) -> Result<(), ApiError> {
    set(cx, this, N, &value.to_string());
    Ok(())
}

/// A `long` "limited to only non-negative numbers" (`maxLength`): the
/// attribute's non-negative integer, else `D`; setting a negative one is
/// an `IndexSizeError`.
pub(crate) fn non_negative<'gc, const N: NameId, const D: i32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<i32, ApiError> {
    Ok(integer(cx, this, N)
        .filter(|value| *value >= 0)
        .unwrap_or(D))
}

pub(crate) fn set_non_negative<'gc, const N: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: i32,
) -> Result<(), ApiError> {
    if value < 0 {
        return Err(index_size_error(&format!(
            "{} can't be negative",
            names::get(N)
        )));
    }
    set_long::<N>(cx, this, value)
}

/// An `unsigned long`: the attribute's non-negative integer up to
/// 2147483647, else `D`; setting one above that sets `D`.
pub(crate) fn unsigned<'gc, const N: NameId, const D: u32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<u32, ApiError> {
    Ok(integer(cx, this, N)
        .filter(|value| *value >= 0)
        .map_or(D, i32::unsigned_abs))
}

pub(crate) fn set_unsigned<'gc, const N: NameId, const D: u32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: u32,
) -> Result<(), ApiError> {
    let value = if value <= MAX_REFLECTED { value } else { D };
    set(cx, this, N, &value.to_string());
    Ok(())
}

/// An `unsigned long` "limited to only positive numbers" (`size`): as
/// `unsigned`, 0 counting as missing; setting 0 is an `IndexSizeError`, or
/// with `F` ("with fallback": `cols`, `rows`) sets `D`.
pub(crate) fn positive<'gc, const N: NameId, const D: u32>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<u32, ApiError> {
    Ok(match unsigned::<N, D>(cx, this)? {
        0 => D,
        value => value,
    })
}

pub(crate) fn set_positive<'gc, const N: NameId, const D: u32, const F: bool>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: u32,
) -> Result<(), ApiError> {
    if value == 0 && !F {
        return Err(index_size_error(&format!(
            "{} must be positive",
            names::get(N)
        )));
    }
    let value = if (1..=MAX_REFLECTED).contains(&value) {
        value
    } else {
        D
    };
    set(cx, this, N, &value.to_string());
    Ok(())
}

fn index_size_error(message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::IndexSize,
        message: message.to_owned(),
    })
}

/// HTML's "rules for parsing integers": leading ASCII whitespace, a sign,
/// digits; anything after them is ignored; none past an `i32`.
pub(crate) fn parse_integer(text: &str) -> Option<i32> {
    let text = text.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, text.get(1..)?),
        b'+' => (false, text.get(1..)?),
        _ => (false, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let value: i64 = digits.get(..end)?.parse().ok()?;
    i32::try_from(if negative {
        value.checked_neg()?
    } else {
        value
    })
    .ok()
}
