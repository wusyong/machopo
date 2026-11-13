//! Fetch's `Headers` (https://fetch.spec.whatwg.org/#headers-class,
//! Blink's `core/fetch/headers.cc`): its guard's checks and the
//! interface's members, over `dom`'s list; the list's algorithms are
//! `protocol::header_list`'s. Names and values
//! are `ByteString`s (`ByteString`: each character a byte).

use dom::{DomString, Headers, HeadersGuard};
use elise::Gc;
use protocol::header_list::{self as list, HeaderList};
use protocol::network;

use crate::typed::{DomStr, StringPairsArg, StringPairsInit};
use crate::{ApiCx, ApiError};

/// Fetch's "validate" a header for a `Headers`: an invalid name or value
/// is a `TypeError`, as is any change to immutable headers; one the guard
/// forbids is ignored (`false`).
fn validate(guard: HeadersGuard, name: &str, value: &str) -> Result<bool, ApiError> {
    if !list::is_header_name(name) {
        return Err(ApiError::type_error(format!("Invalid name: {name:?}")));
    }
    if !list::is_header_value(value) {
        return Err(ApiError::type_error(format!("Invalid value: {value:?}")));
    }
    Ok(match guard {
        HeadersGuard::Immutable => return Err(ApiError::type_error("Headers are immutable")),
        HeadersGuard::Request => !network::is_forbidden_request_header(name, value),
        HeadersGuard::Response => !list::is_forbidden_response_header_name(name),
        // Checked by each change, against what the list would hold.
        HeadersGuard::RequestNoCors | HeadersGuard::None => true,
    })
}

fn check_name(name: &str) -> Result<(), ApiError> {
    if list::is_header_name(name) {
        Ok(())
    } else {
        Err(ApiError::type_error(format!("Invalid name: {name:?}")))
    }
}

/// Fetch's "append" to a `Headers`.
pub(crate) fn append_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    headers: Gc<'gc, Headers>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    let value = list::normalize(value);
    let guard = cx.headers_guard(headers);
    if !validate(guard, name, value)? {
        return Ok(());
    }
    cx.with_header_list(headers, |headers| {
        if guard == HeadersGuard::RequestNoCors {
            // The value the name would have once appended.
            let combined = match list::get(headers, name) {
                Some(existing) => format!("{existing}, {value}"),
                None => value.to_owned(),
            };
            if !network::is_no_cors_safelisted_request_header(name, &combined) {
                return;
            }
        }
        list::append(headers, name, value);
        if guard == HeadersGuard::RequestNoCors {
            list::remove_privileged_no_cors_request_headers(headers);
        }
    });
    Ok(())
}

/// Fetch's "fill" a `Headers` from `init`: a sequence of name-value pairs
/// (each exactly two), or a record.
pub(crate) fn fill<'gc>(
    cx: &ApiCx<'_, 'gc>,
    headers: Gc<'gc, Headers>,
    init: &StringPairsInit<'_>,
) -> Result<(), ApiError> {
    match init {
        StringPairsInit::Sequences(pairs) => {
            for pair in *pairs {
                let [name, value] = pair.as_slice() else {
                    return Err(ApiError::type_error(format!(
                        "Invalid value: a header is a name and a value, not {} items",
                        pair.len()
                    )));
                };
                append_to(cx, headers, name, value)?;
            }
        }
        StringPairsInit::Record(entries) => {
            for (name, value) in *entries {
                append_to(cx, headers, name, value)?;
            }
        }
        // `HeaderPairs` takes no string.
        StringPairsInit::String(_) => {}
    }
    Ok(())
}

/// A dictionary's `HeadersInit` (`RequestInit`'s, `ResponseInit`'s) as
/// `fill` takes it.
pub(crate) fn pairs_init(pairs: &StringPairsArg) -> StringPairsInit<'_> {
    match pairs {
        StringPairsArg::Sequences(items) => StringPairsInit::Sequences(items),
        StringPairsArg::Record(entries) => StringPairsInit::Record(entries),
        StringPairsArg::String(string) => StringPairsInit::String(string),
    }
}

/// `new Headers(init)`.
pub(crate) fn construct<'gc>(
    cx: &ApiCx<'_, 'gc>,
    init: StringPairsInit<'_>,
) -> Result<Gc<'gc, Headers>, ApiError> {
    let headers = cx.create_headers(HeadersGuard::None);
    fill(cx, headers, &init)?;
    Ok(headers)
}

/// `append(name, value)`.
pub(crate) fn append<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    append_to(cx, this, name, value)
}

/// `delete(name)`.
pub(crate) fn delete<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    name: &str,
) -> Result<(), ApiError> {
    let guard = cx.headers_guard(this);
    if !validate(guard, name, "")? {
        return Ok(());
    }
    if guard == HeadersGuard::RequestNoCors
        && !network::is_no_cors_safelisted_request_header_name(name)
        && !network::is_privileged_no_cors_request_header_name(name)
    {
        return Ok(());
    }
    cx.with_header_list(this, |headers| {
        if !list::contains(headers, name) {
            return;
        }
        list::delete(headers, name);
        if guard == HeadersGuard::RequestNoCors {
            list::remove_privileged_no_cors_request_headers(headers);
        }
    });
    Ok(())
}

/// `get(name)`: its values joined, or null.
pub(crate) fn get<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    name: &str,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    check_name(name)?;
    Ok(cx
        .with_header_list(this, |headers| list::get(headers, name))
        .map(DomStr::fresh))
}

/// `getSetCookie()`: the `Set-Cookie` values, each on its own.
pub(crate) fn get_set_cookie<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
) -> Result<Vec<DomString>, ApiError> {
    Ok(cx.with_header_list(this, |headers| {
        list::values(headers, "set-cookie")
            .iter()
            .map(|value| DomString::new(value))
            .collect()
    }))
}

/// `has(name)`.
pub(crate) fn has<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    name: &str,
) -> Result<bool, ApiError> {
    check_name(name)?;
    Ok(cx.with_header_list(this, |headers| list::contains(headers, name)))
}

/// `set(name, value)`.
pub(crate) fn set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    let value = list::normalize(value);
    let guard = cx.headers_guard(this);
    if !validate(guard, name, value)? {
        return Ok(());
    }
    if guard == HeadersGuard::RequestNoCors
        && !network::is_no_cors_safelisted_request_header(name, value)
    {
        return Ok(());
    }
    cx.with_header_list(this, |headers| {
        list::set(headers, name, value);
        if guard == HeadersGuard::RequestNoCors {
            list::remove_privileged_no_cors_request_headers(headers);
        }
    });
    Ok(())
}

/// The pair iterator's pair at `index` (WebIDL's "value pairs to iterate
/// over": the list sorted and combined, as it is now); empty past the
/// last.
pub(crate) fn pair<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Headers>,
    index: u32,
) -> Result<Vec<DomString>, ApiError> {
    let pairs: HeaderList = cx.with_header_list(this, |headers| list::sort_and_combine(headers));
    Ok(usize::try_from(index)
        .ok()
        .and_then(|index| pairs.get(index))
        .map(|(name, value)| vec![DomString::new(name), DomString::new(value)])
        .unwrap_or_default())
}
