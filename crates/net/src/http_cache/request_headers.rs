//! What a request's own headers say to the cache (Chromium's
//! `http_cache_util`): some keep it from the cache, or have it fetched or
//! validated again; `If-None-Match` and `If-Modified-Since` make it a
//! validation of the caller's own.

use super::headers::has_value;
use super::vary::request_header;

/// What a request's headers make of its load, as the load flag Chromium
/// adds for them (`GetLoadFlagsForExtraHeaders`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderFlag {
    /// `If-Unmodified-Since`, `If-Match` or `If-Range`: past the cache, in
    /// and out (`LOAD_DISABLE_CACHE`), as a stored answer could be an
    /// unexpected `412`. And `Range`: the cache keeps no part of a resource
    /// (Chromium's `PartialData` does; a range it can't handle disables the
    /// cache too).
    Disable,
    /// `Cache-Control: no-cache` or `Pragma: no-cache`: from the network,
    /// and stored (`LOAD_BYPASS_CACHE`).
    Bypass,
    /// `Cache-Control: max-age=0`: validated even if fresh
    /// (`LOAD_VALIDATE_CACHE`).
    Validate,
}

/// The flag `request`'s headers give its load, if any: the first of
/// [`HeaderFlag`]'s, in its order (`kSpecialHeaders`).
pub(crate) fn header_flag(request: &[(String, String)]) -> Option<HeaderFlag> {
    if ["if-unmodified-since", "if-match", "if-range", "range"]
        .iter()
        .any(|name| request_header(request, name).is_some())
    {
        return Some(HeaderFlag::Disable);
    }
    if has_request_value(request, "cache-control", "no-cache")
        || has_request_value(request, "pragma", "no-cache")
    {
        return Some(HeaderFlag::Bypass);
    }
    if has_request_value(request, "cache-control", "max-age=0") {
        return Some(HeaderFlag::Validate);
    }
    None
}

/// Whether the request's `name` header (the first) lists `value`, ignoring
/// ASCII case (`HeaderMatches`).
fn has_request_value(request: &[(String, String)], name: &str, value: &str) -> bool {
    request_header(request, name)
        .is_some_and(|header| has_value(&[(name.to_owned(), header.to_owned())], name, value))
}

/// The caller's own validation (`ValidationHeaders`): the `If-Modified-Since`
/// and `If-None-Match` it sends. The cache answers it from a stored response
/// whose `Last-Modified` and `ETag` are those, if it is fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalValidation {
    if_modified_since: Option<String>,
    if_none_match: Option<String>,
}

/// Why a request's validation headers can't be used: one is empty
/// (`MaybeCreate`'s error), and the load goes past the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmptyValidationHeader;

impl ExternalValidation {
    /// The request's validation headers, if it has any.
    pub(crate) fn of(request: &[(String, String)]) -> Result<Option<Self>, EmptyValidationHeader> {
        let get = |name| match request_header(request, name) {
            Some("") => Err(EmptyValidationHeader),
            value => Ok(value.map(str::to_owned)),
        };
        let validation = Self {
            if_modified_since: get("if-modified-since")?,
            if_none_match: get("if-none-match")?,
        };
        Ok(
            (validation.if_modified_since.is_some() || validation.if_none_match.is_some())
                .then_some(validation),
        )
    }

    /// Whether the stored response's validators are the request's
    /// (`Match`): each the request sends is the response's, where the
    /// response has it (one it doesn't have doesn't count against it).
    pub(crate) fn matches(&self, response: &[(String, String)]) -> bool {
        let same =
            |sent: &Option<String>, name| match (sent, super::headers::first_value(response, name))
            {
                (Some(sent), Some(stored)) => sent == stored,
                _ => true,
            };
        same(&self.if_modified_since, "last-modified") && same(&self.if_none_match, "etag")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(lines: &[(&str, &str)]) -> Vec<(String, String)> {
        lines
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn the_flags_in_their_order() {
        assert_eq!(header_flag(&[]), None);
        assert_eq!(
            header_flag(&headers(&[("If-Match", "\"a\""), ("pragma", "no-cache")])),
            Some(HeaderFlag::Disable)
        );
        assert_eq!(
            header_flag(&headers(&[("cache-control", "max-age=0, No-Cache")])),
            Some(HeaderFlag::Bypass)
        );
        assert_eq!(
            header_flag(&headers(&[("pragma", "no-cache")])),
            Some(HeaderFlag::Bypass)
        );
        assert_eq!(
            header_flag(&headers(&[("cache-control", "MAX-AGE=0")])),
            Some(HeaderFlag::Validate)
        );
        // Not another `max-age`.
        assert_eq!(
            header_flag(&headers(&[("cache-control", "max-age=00")])),
            None
        );
    }

    #[test]
    fn external_validation() {
        assert_eq!(ExternalValidation::of(&[]), Ok(None));
        assert_eq!(
            ExternalValidation::of(&headers(&[("if-none-match", "")])),
            Err(EmptyValidationHeader)
        );
        let validation = ExternalValidation::of(&headers(&[("If-None-Match", "\"v1\"")]))
            .expect("valid")
            .expect("some");
        assert!(validation.matches(&headers(&[("etag", "\"v1\"")])));
        assert!(!validation.matches(&headers(&[("etag", "\"v2\"")])));
        // A validator the response doesn't have doesn't count against it.
        assert!(validation.matches(&headers(&[("last-modified", "x")])));
    }
}
