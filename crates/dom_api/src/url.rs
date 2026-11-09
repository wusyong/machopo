//! `URL` and `URLSearchParams` (https://url.spec.whatwg.org/#api; Blink's
//! `DOMURL` and `URLSearchParams`), over `dom`'s: the parser is the
//! `url` crate's, the one navigation and the network use, its `quirks`
//! module URL Standard's getters and setters. A `URL`'s `searchParams`
//! mirror its query: setting `href` or `search` parses the query into the
//! params' list again, and the params write their list back after each
//! change (URL Standard's "update steps"). And `createObjectURL`,
//! `revokeObjectURL` (File API's): the host keeps the blob URLs.

use dom::{Blob, BlobUrlRequest, DomString, Url as UrlObject, UrlSearchParams};
use elise::Gc;
use url::{Url, form_urlencoded, quirks};

use crate::typed::{OpFront, StringPairsInit};
use crate::{ApiCx, ApiError, DomStr};

/// URL Standard's "API URL parser": `input` against `base`, if given; a
/// `TypeError` for either failing.
fn parse(input: &str, base: Option<&str>) -> Result<Url, ApiError> {
    let base = base
        .map(|base| {
            Url::parse(base).map_err(|_| ApiError::type_error(format!("Invalid base URL: {base}")))
        })
        .transpose()?;
    Url::options()
        .base_url(base.as_ref())
        .parse(input)
        .map_err(|_| ApiError::type_error(format!("Invalid URL: {input}")))
}

/// `input` parsed as `application/x-www-form-urlencoded`: names and values.
fn parse_query(input: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(input.as_bytes())
        .into_owned()
        .collect()
}

/// `url`'s query parsed; none without one.
fn query_list(url: &Url) -> Vec<(String, String)> {
    url.query().map(parse_query).unwrap_or_default()
}

/// `list` serialized as `application/x-www-form-urlencoded`.
pub(crate) fn serialize(list: &[(String, String)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(list)
        .finish()
}

/// `new URL(url, base)`: a `TypeError` if it doesn't parse.
pub(crate) fn construct<'gc>(
    cx: &ApiCx<'_, 'gc>,
    url: &str,
    base: Option<&str>,
) -> Result<Gc<'gc, UrlObject>, ApiError> {
    let url = parse(url, base)?;
    let list = query_list(&url);
    Ok(cx.create_url(url, list))
}

/// `URL.parse(url, base)`: `new URL`'s, or `null` where it throws.
pub(crate) fn parse_static<'gc>(
    cx: &ApiCx<'_, 'gc>,
    url: &str,
    base: Option<&str>,
) -> Result<Option<Gc<'gc, UrlObject>>, ApiError> {
    Ok(construct(cx, url, base).ok())
}

/// `URL.canParse(url, base)`.
pub(crate) fn can_parse<'gc>(
    _: &ApiCx<'_, 'gc>,
    url: &str,
    base: Option<&str>,
) -> Result<bool, ApiError> {
    Ok(parse(url, base).is_ok())
}

/// `URL.createObjectURL(obj)`: a new blob URL for the blob (File API's
/// "generate a new blob URL": `blob:`, the current realm's origin, `/`, a
/// new UUID), which the host registers for the realm's document.
pub(crate) fn create_object_url<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    obj: Gc<'gc, Blob>,
) -> Result<DomStr<'gc>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("createObjectURL: no window"))?;
    let document = cx.window_document(window);
    let origin = crate::frame::origin(cx, document)
        .map_or_else(|| "null".to_owned(), |origin| origin.ascii_serialization());
    let url = format!("blob:{origin}/{}", crate::crypto::new_uuid());
    cx.request_blob_url(BlobUrlRequest::Register {
        document: cx.node_id(document),
        url: url.clone(),
        data: cx.blob_data(obj).clone(),
    });
    Ok(DomStr::Owned(DomString::new(&url)))
}

/// `URL.revokeObjectURL(url)`: a blob URL of the current realm's origin
/// stands for nothing now; anything else does nothing.
pub(crate) fn revoke_object_url<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    url: &str,
) -> Result<(), ApiError> {
    let Ok(parsed) = Url::parse(url) else {
        return Ok(());
    };
    let Some(window) = op.current_window(cx) else {
        return Ok(());
    };
    let origin = crate::frame::origin(cx, cx.window_document(window));
    let blob_origin = parsed.origin();
    if parsed.scheme() != "blob" || !blob_origin.is_tuple() || origin.as_ref() != Some(&blob_origin)
    {
        return Ok(());
    }
    cx.request_blob_url(BlobUrlRequest::Revoke {
        url: parsed.as_str().to_owned(),
    });
    Ok(())
}

fn get<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    part: fn(&Url) -> &str,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.with_url(this, |url| DomString::new(part(url))),
    ))
}

/// The URL changed by `set` (a URL Standard setter, which ignores what it
/// can't take).
fn set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    set: impl FnOnce(&mut Url),
) -> Result<(), ApiError> {
    cx.with_url(this, set);
    Ok(())
}

/// The `searchParams`' list is `list` now, not written back.
fn set_list<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, UrlObject>, list: Vec<(String, String)>) {
    let params = cx.url_search_params(this);
    cx.with_search_params(params, |old| *old = list);
}

/// `href` (and `toString()`, `toJSON()`): the serialization.
pub(crate) fn href<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::href)
}

/// Setting `href`: a URL of its own, a `TypeError` if it doesn't parse;
/// the `searchParams` take its query.
pub(crate) fn set_href<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    let url = parse(value, None)?;
    let list = query_list(&url);
    cx.with_url(this, |old| *old = url);
    set_list(cx, this, list);
    Ok(())
}

/// `origin`: its serialization (`null` for an opaque one).
pub(crate) fn origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.with_url(this, |url| {
        DomString::new(&quirks::origin(url))
    })))
}

pub(crate) fn protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::protocol)
}

pub(crate) fn set_protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_protocol(url, value);
    })
}

pub(crate) fn username<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::username)
}

pub(crate) fn set_username<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_username(url, value);
    })
}

pub(crate) fn password<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::password)
}

pub(crate) fn set_password<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_password(url, value);
    })
}

pub(crate) fn host<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::host)
}

pub(crate) fn set_host<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_host(url, value);
    })
}

pub(crate) fn hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::hostname)
}

pub(crate) fn set_hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_hostname(url, value);
    })
}

pub(crate) fn port<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::port)
}

pub(crate) fn set_port<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_port(url, value);
    })
}

pub(crate) fn pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::pathname)
}

pub(crate) fn set_pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| quirks::set_pathname(url, value))
}

pub(crate) fn search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::search)
}

/// Setting `search`: the query (none for ""), and the `searchParams` take
/// the value, its leading `?` removed, parsed.
pub(crate) fn set_search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    cx.with_url(this, |url| quirks::set_search(url, value));
    let list = if value.is_empty() {
        Vec::new()
    } else {
        parse_query(value.strip_prefix('?').unwrap_or(value))
    };
    set_list(cx, this, list);
    Ok(())
}

/// `searchParams`: the same object each time.
pub(crate) fn search_params<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<Gc<'gc, UrlSearchParams>, ApiError> {
    Ok(cx.url_search_params(this))
}

pub(crate) fn hash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
) -> Result<DomStr<'gc>, ApiError> {
    get(cx, this, quirks::hash)
}

pub(crate) fn set_hash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlObject>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| quirks::set_hash(url, value))
}

/// `new URLSearchParams(init)`: pairs (each exactly two strings), a record,
/// or a query string (a leading `?` removed).
pub(crate) fn construct_search_params<'gc>(
    cx: &ApiCx<'_, 'gc>,
    init: StringPairsInit<'_>,
) -> Result<Gc<'gc, UrlSearchParams>, ApiError> {
    let list = match init {
        StringPairsInit::Sequences(items) => items
            .iter()
            .map(|pair| match pair.as_slice() {
                [name, value] => Ok((name.clone(), value.clone())),
                _ => Err(ApiError::type_error(
                    "Failed to construct 'URLSearchParams': The provided sequence's items must each be a pair.",
                )),
            })
            .collect::<Result<_, _>>()?,
        StringPairsInit::Record(entries) => entries.to_vec(),
        StringPairsInit::String(query) => parse_query(query.strip_prefix('?').unwrap_or(query)),
    };
    Ok(cx.create_url_search_params(list))
}

/// `f` with the list, then, if it changed it, URL Standard's "update
/// steps": the `URL`, if there is one, takes the list as its query (none if
/// it serializes to nothing).
fn update<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    f: impl FnOnce(&mut Vec<(String, String)>) -> R,
) -> R {
    let (result, query) = cx.with_search_params(this, |list| {
        let result = f(list);
        (result, serialize(list))
    });
    if let Some(url) = cx.search_params_url(this) {
        cx.with_url(url, |url| {
            url.set_query((!query.is_empty()).then_some(query.as_str()))
        });
    }
    result
}

/// `size`: the number of pairs.
pub(crate) fn size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
) -> Result<u32, ApiError> {
    Ok(cx.with_search_params(this, |list| u32::try_from(list.len()).unwrap_or(u32::MAX)))
}

/// `append(name, value)`: a pair at the end.
pub(crate) fn append<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    update(cx, this, |list| {
        list.push((name.to_owned(), value.to_owned()))
    });
    Ok(())
}

/// Whether `pair` is named `name` (and, with `value`, has it).
fn matches(pair: &(String, String), name: &str, value: Option<&str>) -> bool {
    pair.0 == name && value.is_none_or(|value| pair.1 == value)
}

/// `delete(name, value)`: every pair named `name` (with `value`, if
/// given).
pub(crate) fn delete<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
    value: Option<&str>,
) -> Result<(), ApiError> {
    update(cx, this, |list| {
        list.retain(|pair| !matches(pair, name, value))
    });
    Ok(())
}

/// `get(name)`: the first pair's value, or `null`.
pub(crate) fn get_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
) -> Result<Option<DomString>, ApiError> {
    Ok(cx.with_search_params(this, |list| {
        list.iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| DomString::new(value))
    }))
}

/// `getAll(name)`: every pair's value, in order.
pub(crate) fn get_all<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
) -> Result<Vec<DomString>, ApiError> {
    Ok(cx.with_search_params(this, |list| {
        list.iter()
            .filter(|(n, _)| n == name)
            .map(|(_, value)| DomString::new(value))
            .collect()
    }))
}

/// `has(name, value)`: whether a pair is named `name` (with `value`, if
/// given).
pub(crate) fn has<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
    value: Option<&str>,
) -> Result<bool, ApiError> {
    Ok(cx.with_search_params(this, |list| {
        list.iter().any(|pair| matches(pair, name, value))
    }))
}

/// `set(name, value)`: the first pair named `name` takes `value` and the
/// others go; without one, a pair at the end.
pub(crate) fn set_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    update(cx, this, |list| {
        match list.iter().position(|(n, _)| n == name) {
            Some(first) => {
                list[first].1 = value.to_owned();
                let mut index = 0usize;
                list.retain(|(n, _)| {
                    let keep = index <= first || n != name;
                    index = index.saturating_add(1);
                    keep
                });
            }
            None => list.push((name.to_owned(), value.to_owned())),
        }
    });
    Ok(())
}

/// `sort()`: by name, comparing UTF-16 code units, pairs of the same name
/// keeping their order.
pub(crate) fn sort<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
) -> Result<(), ApiError> {
    update(cx, this, |list| {
        list.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()))
    });
    Ok(())
}

/// `toString()` (the stringifier): the list serialized.
pub(crate) fn to_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(
        &cx.with_search_params(this, |list| serialize(list)),
    )))
}

/// The pair at `index` for the pair iterator (`#[pairs]`): `[name,
/// value]`, none past the last.
pub(crate) fn pair<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UrlSearchParams>,
    index: u32,
) -> Result<Vec<DomString>, ApiError> {
    Ok(cx.with_search_params(this, |list| {
        usize::try_from(index)
            .ok()
            .and_then(|index| list.get(index))
            .map(|(name, value)| vec![DomString::new(name), DomString::new(value)])
            .unwrap_or_default()
    }))
}
