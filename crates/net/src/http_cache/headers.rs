//! What a response's headers say to the cache (the parts of Chromium's
//! `HttpResponseHeaders` it uses): how a header's values are listed, how
//! long the response stays fresh, how old it is, whether it has to be
//! validated before it is used again, what to validate it with, and how a
//! `304`'s headers update it.
//!
//! Headers are the transport's: names lowercase, one entry per line, in
//! order.

use crate::cookies::{Time, parse_cookie_expiration_time};

/// Microseconds in a second.
const MICROS_PER_SECOND: i64 = 1_000_000;

/// Headers whose value is one value even with commas in it: the others are
/// a list (`HttpUtil::IsNonCoalescingHeader`).
const NON_COALESCING: [&str; 9] = [
    "date",
    "expires",
    "last-modified",
    "location",
    "retry-after",
    "set-cookie",
    "www-authenticate",
    "proxy-authenticate",
    "strict-transport-security",
];

/// Headers a `304` doesn't update in the stored response
/// (`kNonUpdatedHeaders`): they describe the stored body, the connection or
/// a challenge, not the response's freshness.
const NON_UPDATED: [&str; 19] = [
    "connection",
    "proxy-connection",
    "keep-alive",
    "www-authenticate",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-location",
    "content-md5",
    "etag",
    "content-encoding",
    "content-range",
    "content-type",
    "content-length",
    "x-frame-options",
    "x-xss-protection",
];

/// Prefixes of the headers a `304` doesn't update either
/// (`kNonUpdatedHeaderPrefixes`).
const NON_UPDATED_PREFIXES: [&str; 2] = ["x-content-", "x-webkit-"];

/// The values of every `name` header, in order (`EnumerateHeader`): a
/// non-coalescing header's whole value each, a list's each comma-separated
/// value (commas in a quoted string don't separate), trimmed of spaces and
/// tabs, empty ones skipped.
pub(crate) fn values<'a>(
    headers: &'a [(String, String)],
    name: &'a str,
) -> impl Iterator<Item = &'a str> + 'a {
    let whole = NON_COALESCING
        .iter()
        .any(|header| header.eq_ignore_ascii_case(name));
    headers
        .iter()
        .filter(move |(header, _)| header.eq_ignore_ascii_case(name))
        .flat_map(move |(_, value)| {
            let pieces: Box<dyn Iterator<Item = &str>> = if whole {
                Box::new(std::iter::once(value.as_str()))
            } else {
                Box::new(split_list(value))
            };
            pieces.map(trim_lws).filter(|value| !value.is_empty())
        })
}

/// The first of [`values`].
pub(crate) fn first_value<'a>(headers: &'a [(String, String)], name: &'a str) -> Option<&'a str> {
    values(headers, name).next()
}

/// Whether one of `name`'s values is `value`, ignoring ASCII case
/// (`HasHeaderValue`).
pub(crate) fn has_value(headers: &[(String, String)], name: &str, value: &str) -> bool {
    values(headers, name).any(|v| v.eq_ignore_ascii_case(value))
}

/// `value` split at the commas outside quoted strings (`HttpUtil::
/// ValuesIterator`, a tokenizer with `"` as its quote: a backslash in a
/// quoted string escapes the next character).
fn split_list(value: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(value);
    std::iter::from_fn(move || {
        let text = rest?;
        let mut quoted = false;
        let mut escaped = false;
        for (at, c) in text.char_indices() {
            if escaped {
                escaped = false;
            } else if quoted {
                match c {
                    '\\' => escaped = true,
                    '"' => quoted = false,
                    _ => {}
                }
            } else if c == '"' {
                quoted = true;
            } else if c == ',' {
                rest = text.get(at.saturating_add(1)..);
                return text.get(..at);
            }
        }
        rest = None;
        Some(text)
    })
}

/// `value` without leading and trailing spaces and tabs (`HttpUtil::
/// TrimLWS`).
fn trim_lws(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

/// A date header's time (`GetTimeValuedHeader`): `Expires: 0` is the
/// earliest time there is (already expired); one that doesn't parse is as
/// if the header weren't there.
fn time_value(headers: &[(String, String)], name: &str) -> Option<Time> {
    let value = first_value(headers, name)?;
    if name == "expires" && value == "0" {
        return Some(Time::MIN);
    }
    parse_cookie_expiration_time(value)
}

pub(crate) fn date(headers: &[(String, String)]) -> Option<Time> {
    time_value(headers, "date")
}

pub(crate) fn last_modified(headers: &[(String, String)]) -> Option<Time> {
    time_value(headers, "last-modified")
}

fn expires(headers: &[(String, String)]) -> Option<Time> {
    time_value(headers, "expires")
}

/// `Age`, in microseconds (`GetAgeValue`): digits only; one too big for 32
/// bits is the most 32 bits hold, as an `Age` a cache can't pass on
/// exactly.
fn age(headers: &[(String, String)]) -> Option<i64> {
    let value = first_value(headers, "age")?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds = value.parse::<u32>().unwrap_or(u32::MAX);
    Some(i64::from(seconds).saturating_mul(MICROS_PER_SECOND))
}

/// A directive's delta-seconds, in microseconds (`ParseSeconds`): digits
/// between spaces, saturated.
fn parse_seconds(value: &str) -> Option<i64> {
    let value = value.trim_matches(' ');
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds = value.parse::<i64>().unwrap_or(i64::MAX);
    Some(seconds.saturating_mul(MICROS_PER_SECOND))
}

/// `value` without `prefix`, matched ignoring ASCII case.
fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| value.get(prefix.len()..))
        .flatten()
}

/// `Cache-Control: no-cache` or `no-store` (`HasCacheRestriction`): never
/// fresh. Only the bare directives: `no-cache="field"` isn't one.
fn has_cache_restriction(headers: &[(String, String)]) -> bool {
    values(headers, "cache-control")
        .any(|v| v.eq_ignore_ascii_case("no-cache") || v.eq_ignore_ascii_case("no-store"))
}

/// The `Cache-Control` directives freshness depends on
/// (`ParseCacheControlDirectivesForFreshness`): the first `max-age` and
/// `stale-while-revalidate` that parse.
#[derive(Default)]
struct FreshnessDirectives {
    immutable: bool,
    must_revalidate: bool,
    max_age: Option<i64>,
    stale_while_revalidate: Option<i64>,
}

fn freshness_directives(headers: &[(String, String)]) -> FreshnessDirectives {
    let mut directives = FreshnessDirectives::default();
    for value in values(headers, "cache-control") {
        if value.eq_ignore_ascii_case("immutable") {
            directives.immutable = true;
        } else if value.eq_ignore_ascii_case("must-revalidate") {
            directives.must_revalidate = true;
        } else if let Some(seconds) = directives
            .max_age
            .is_none()
            .then(|| strip_prefix_ignore_case(value, "max-age="))
            .flatten()
        {
            directives.max_age = parse_seconds(seconds);
        } else if let Some(seconds) = directives
            .stale_while_revalidate
            .is_none()
            .then(|| strip_prefix_ignore_case(value, "stale-while-revalidate="))
            .flatten()
        {
            directives.stale_while_revalidate = parse_seconds(seconds);
        }
    }
    directives
}

/// How long a response is fresh, and how long after that it may still be
/// used while it is validated in the background (`FreshnessLifetimes`), in
/// microseconds. `i64::MAX` is forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Lifetimes {
    pub(crate) freshness: i64,
    pub(crate) staleness: i64,
}

/// The response's lifetimes, received at `response_time`
/// (`GetFreshnessLifetimes`): none with `no-cache`, `no-store` or `Pragma:
/// no-cache`; else `max-age`, else `Expires` after `Date` (or the time it
/// came), else for `200`, `203` and `206` a tenth of the time since
/// `Last-Modified`; `300`, `301`, `308` and `410` are fresh forever without
/// any of these. `stale-while-revalidate` gives the staleness, unless
/// `must-revalidate`.
pub(crate) fn freshness_lifetimes(
    status: u16,
    headers: &[(String, String)],
    response_time: Time,
) -> Lifetimes {
    let mut lifetimes = Lifetimes::default();
    if has_cache_restriction(headers) {
        return lifetimes;
    }
    let directives = freshness_directives(headers);
    // `Cache-Control: immutable` overrides it only behind a feature of
    // Chromium's (`kCacheControlImmutable`) that isn't on by default.
    if has_value(headers, "pragma", "no-cache") {
        return lifetimes;
    }
    lifetimes.staleness = if directives.must_revalidate {
        0
    } else {
        directives.stale_while_revalidate.unwrap_or(0)
    };
    if let Some(max_age) = directives.max_age {
        lifetimes.freshness = max_age;
        return lifetimes;
    }
    let date = date(headers).unwrap_or(response_time);
    if let Some(expires) = expires(headers) {
        // A date in the past is not fresh at all.
        if expires > date {
            lifetimes.freshness = expires.saturating_micros_since(date);
        }
        return lifetimes;
    }
    if matches!(status, 200 | 203 | 206)
        && !directives.must_revalidate
        && let Some(last_modified) = last_modified(headers)
        // One in the future gives nothing.
        && last_modified <= date
    {
        lifetimes.freshness = date.saturating_micros_since(last_modified).saturating_div(10);
        return lifetimes;
    }
    if matches!(status, 300 | 301 | 308 | 410) {
        return Lifetimes {
            freshness: i64::MAX,
            staleness: 0,
        };
    }
    lifetimes
}

/// How old the response is at `now`, in microseconds (`GetCurrentAge`,
/// RFC 9111's 4.2.3): asked for at `request_time`, received at
/// `response_time`. Without `Date`, it was made as it came; without `Age`,
/// it was 0 then.
pub(crate) fn current_age(
    headers: &[(String, String)],
    request_time: Time,
    response_time: Time,
    now: Time,
) -> i64 {
    let date = date(headers).unwrap_or(response_time);
    let age = age(headers).unwrap_or(0);
    let apparent_age = response_time.saturating_micros_since(date).max(0);
    let response_delay = response_time.saturating_micros_since(request_time);
    let corrected_age = age.saturating_add(response_delay);
    let corrected_initial_age = apparent_age.max(corrected_age);
    let resident_time = now.saturating_micros_since(response_time);
    corrected_initial_age.saturating_add(resident_time)
}

/// Whether a stored response can be used as it is (`ValidationType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Validation {
    /// Fresh: use it.
    None,
    /// Stale, but within `stale-while-revalidate`: it could be used while
    /// validated in the background.
    Asynchronous,
    /// Validate it first.
    Synchronous,
}

/// Whether the response has to be validated before it is used at `now`
/// (`RequiresValidation`).
pub(crate) fn requires_validation(
    status: u16,
    headers: &[(String, String)],
    request_time: Time,
    response_time: Time,
    now: Time,
) -> Validation {
    let lifetimes = freshness_lifetimes(status, headers, response_time);
    if lifetimes.freshness == 0 && lifetimes.staleness == 0 {
        return Validation::Synchronous;
    }
    let age = current_age(headers, request_time, response_time, now);
    if lifetimes.freshness > age {
        return Validation::None;
    }
    if lifetimes.freshness.saturating_add(lifetimes.staleness) > age {
        return Validation::Asynchronous;
    }
    Validation::Synchronous
}

/// Whether the response may be stored at all
/// (`UpdateAndReportCacheability`): anything but `Cache-Control:
/// no-store`. Whether it can be used again is then up to its freshness.
pub(crate) fn is_storable(headers: &[(String, String)]) -> bool {
    !has_value(headers, "cache-control", "no-store")
}

/// The headers a conditional request for the stored response sends
/// (`ConditionalizeRequest`): `If-None-Match` its `ETag` (over HTTP/1.1 and
/// later), `If-Modified-Since` its `Last-Modified`; only for a `200` or a
/// `206`. When the stored response's `Vary` didn't match the request, only
/// the `ETag` (a date says nothing about which variant). `None`: it can't
/// be validated, and is fetched again whole.
pub(crate) fn validation_headers(
    status: u16,
    protocol: &str,
    headers: &[(String, String)],
    vary_mismatch: bool,
) -> Option<Vec<(String, String)>> {
    if !matches!(status, 200 | 206) {
        return None;
    }
    let etag = at_least_http_1_1(protocol)
        .then(|| first_value(headers, "etag"))
        .flatten();
    let last_modified = if vary_mismatch {
        None
    } else {
        first_value(headers, "last-modified")
    };
    if etag.is_none() && last_modified.is_none() {
        return None;
    }
    let mut sent = Vec::new();
    if let Some(etag) = etag {
        sent.push(("if-none-match".to_owned(), etag.to_owned()));
    }
    if let Some(last_modified) = last_modified {
        sent.push(("if-modified-since".to_owned(), last_modified.to_owned()));
    }
    Some(sent)
}

/// Whether a response of `protocol` (`http/1.1`, `h2`...) is HTTP/1.1 or
/// later (`GetHttpVersion() >= HttpVersion(1, 1)`).
fn at_least_http_1_1(protocol: &str) -> bool {
    !matches!(protocol, "http/0.9" | "http/1.0")
}

/// The stored response's headers updated by a `304`'s `new` ones
/// (`HttpResponseHeaders::Update`): each header `new` has replaces the
/// stored one of its name (every line of it), unless it describes the body,
/// the connection or a challenge (`NON_UPDATED`); the stored ones it
/// doesn't have stay.
pub(crate) fn update(stored: &[(String, String)], new: &[(String, String)]) -> Vec<(String, String)> {
    let updated: Vec<&(String, String)> = new.iter().filter(|(name, _)| should_update(name)).collect();
    stored
        .iter()
        .filter(|(name, _)| {
            !updated
                .iter()
                .any(|(new_name, _)| new_name.eq_ignore_ascii_case(name))
        })
        .chain(updated.iter().copied())
        .cloned()
        .collect()
}

fn should_update(name: &str) -> bool {
    !NON_UPDATED
        .iter()
        .any(|header| header.eq_ignore_ascii_case(name))
        && !NON_UPDATED_PREFIXES
            .iter()
            .any(|prefix| strip_prefix_ignore_case(name, prefix).is_some())
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

    const SECOND: i64 = MICROS_PER_SECOND;

    fn at(date: &str) -> Time {
        parse_cookie_expiration_time(date).expect("a date")
    }

    #[test]
    fn a_list_is_split_at_commas_outside_quotes() {
        let h = headers(&[
            ("cache-control", r#"max-age=10, no-cache="a, b", , private"#),
            ("cache-control", "must-revalidate"),
            ("etag", r#""a\"b, c", W/"d""#),
        ]);
        assert_eq!(
            values(&h, "cache-control").collect::<Vec<_>>(),
            ["max-age=10", r#"no-cache="a, b""#, "private", "must-revalidate"]
        );
        assert_eq!(values(&h, "etag").collect::<Vec<_>>(), [r#""a\"b, c""#, r#"W/"d""#]);
    }

    #[test]
    fn a_date_is_one_value_commas_and_all() {
        let h = headers(&[("date", "Tue, 24 Nov 2026 02:51:00 GMT")]);
        assert_eq!(
            values(&h, "date").collect::<Vec<_>>(),
            ["Tue, 24 Nov 2026 02:51:00 GMT"]
        );
        assert_eq!(date(&h), Some(at("Tue, 24 Nov 2026 02:51:00 GMT")));
    }

    #[test]
    fn names_match_ignoring_case() {
        let h = headers(&[("pragma", "No-Cache")]);
        assert!(has_value(&h, "Pragma", "no-cache"));
    }

    #[test]
    fn max_age_comes_first() {
        let now = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let h = headers(&[
            ("cache-control", "max-age=60"),
            ("expires", "Mon, 23 Nov 2026 00:00:00 GMT"),
        ]);
        assert_eq!(freshness_lifetimes(200, &h, now).freshness, 60 * SECOND);
    }

    #[test]
    fn the_first_max_age_that_parses_counts() {
        let now = Time::UNIX_EPOCH;
        let h = headers(&[("cache-control", r#"max-age="5", MAX-AGE= 7 , max-age=9"#)]);
        assert_eq!(freshness_lifetimes(200, &h, now).freshness, 7 * SECOND);
    }

    #[test]
    fn a_huge_max_age_saturates() {
        let h = headers(&[("cache-control", "max-age=99999999999999999999999")]);
        assert_eq!(
            freshness_lifetimes(200, &h, Time::UNIX_EPOCH).freshness,
            i64::MAX
        );
    }

    #[test]
    fn no_cache_no_store_and_pragma_are_never_fresh() {
        for line in [
            ("cache-control", "max-age=60, no-cache"),
            ("cache-control", "max-age=60, NO-STORE"),
            ("pragma", "no-cache"),
        ] {
            let mut h = headers(&[("cache-control", "max-age=60, stale-while-revalidate=60")]);
            h.push((line.0.to_owned(), line.1.to_owned()));
            assert_eq!(
                freshness_lifetimes(200, &h, Time::UNIX_EPOCH),
                Lifetimes::default(),
                "{line:?}"
            );
        }
        // A field's `no-cache` isn't the directive.
        let h = headers(&[("cache-control", r#"max-age=60, no-cache="set-cookie""#)]);
        assert_eq!(
            freshness_lifetimes(200, &h, Time::UNIX_EPOCH).freshness,
            60 * SECOND
        );
    }

    #[test]
    fn expires_counts_from_date_or_the_response_time() {
        let date = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let h = headers(&[
            ("date", "Tue, 24 Nov 2026 00:00:00 GMT"),
            ("expires", "Tue, 24 Nov 2026 01:00:00 GMT"),
        ]);
        let later = date.saturating_add_seconds(600);
        assert_eq!(freshness_lifetimes(200, &h, later).freshness, 3600 * SECOND);
        let h = headers(&[("expires", "Tue, 24 Nov 2026 01:00:00 GMT")]);
        assert_eq!(freshness_lifetimes(200, &h, later).freshness, 3000 * SECOND);
    }

    #[test]
    fn expires_in_the_past_or_zero_is_not_fresh() {
        let date = at("Tue, 24 Nov 2026 00:00:00 GMT");
        for expires in ["Mon, 23 Nov 2026 00:00:00 GMT", "0"] {
            let h = headers(&[
                ("expires", expires),
                ("last-modified", "Mon, 02 Nov 2026 00:00:00 GMT"),
            ]);
            assert_eq!(freshness_lifetimes(200, &h, date).freshness, 0, "{expires}");
        }
    }

    #[test]
    fn an_expires_that_doesnt_parse_is_as_if_it_werent_there() {
        let date = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let h = headers(&[
            ("expires", "soon"),
            ("last-modified", "Thu, 15 Oct 2026 00:00:00 GMT"),
        ]);
        // Forty days since `Last-Modified`: four days.
        assert_eq!(
            freshness_lifetimes(200, &h, date).freshness,
            4 * 86_400 * SECOND
        );
    }

    #[test]
    fn the_heuristic_is_a_tenth_since_last_modified_for_some_statuses() {
        let date = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let h = headers(&[
            ("date", "Tue, 24 Nov 2026 00:00:00 GMT"),
            ("last-modified", "Mon, 23 Nov 2026 08:00:00 GMT"),
        ]);
        for status in [200, 203, 206] {
            assert_eq!(
                freshness_lifetimes(status, &h, date).freshness,
                5760 * SECOND,
                "{status}"
            );
        }
        for status in [204, 302, 404] {
            assert_eq!(freshness_lifetimes(status, &h, date).freshness, 0, "{status}");
        }
        // Not with `must-revalidate`, nor from the future.
        let mut must = h.clone();
        must.push(("cache-control".to_owned(), "must-revalidate".to_owned()));
        assert_eq!(freshness_lifetimes(200, &must, date).freshness, 0);
        let future = headers(&[
            ("date", "Tue, 24 Nov 2026 00:00:00 GMT"),
            ("last-modified", "Wed, 25 Nov 2026 00:00:00 GMT"),
        ]);
        assert_eq!(freshness_lifetimes(200, &future, date).freshness, 0);
    }

    #[test]
    fn permanent_statuses_are_fresh_forever() {
        for status in [300, 301, 308, 410] {
            assert_eq!(
                freshness_lifetimes(status, &[], Time::UNIX_EPOCH),
                Lifetimes {
                    freshness: i64::MAX,
                    staleness: 0
                },
                "{status}"
            );
        }
        // Unless something else says.
        let h = headers(&[("cache-control", "max-age=5")]);
        assert_eq!(
            freshness_lifetimes(301, &h, Time::UNIX_EPOCH).freshness,
            5 * SECOND
        );
        assert_eq!(freshness_lifetimes(302, &[], Time::UNIX_EPOCH).freshness, 0);
    }

    #[test]
    fn stale_while_revalidate_unless_must_revalidate() {
        let h = headers(&[("cache-control", "max-age=10, stale-while-revalidate=20")]);
        assert_eq!(
            freshness_lifetimes(200, &h, Time::UNIX_EPOCH),
            Lifetimes {
                freshness: 10 * SECOND,
                staleness: 20 * SECOND
            }
        );
        let h = headers(&[(
            "cache-control",
            "max-age=10, stale-while-revalidate=20, must-revalidate",
        )]);
        assert_eq!(freshness_lifetimes(200, &h, Time::UNIX_EPOCH).staleness, 0);
    }

    #[test]
    fn the_age_is_rfc_9111s() {
        let request = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let response = request.saturating_add_seconds(2);
        let now = response.saturating_add_seconds(100);
        // `Date` 10 s before it came: apparent age 10.
        let h = headers(&[("date", "Mon, 23 Nov 2026 23:59:52 GMT")]);
        assert_eq!(current_age(&h, request, response, now), 110 * SECOND);
        // `Age: 30` plus the 2 s it took beats it.
        let mut aged = h.clone();
        aged.push(("age".to_owned(), "30".to_owned()));
        assert_eq!(current_age(&aged, request, response, now), 132 * SECOND);
        // No `Date`, no `Age`: just the delay and the time since.
        assert_eq!(current_age(&[], request, response, now), 102 * SECOND);
        // An `Age` too big for 32 bits is the most they hold; one that isn't
        // digits isn't there.
        let big = headers(&[("age", "99999999999")]);
        assert_eq!(
            current_age(&big, response, response, response),
            i64::from(u32::MAX) * SECOND
        );
        let bad = headers(&[("age", "-5")]);
        assert_eq!(current_age(&bad, response, response, response), 0);
    }

    #[test]
    fn validation_by_age() {
        let t = at("Tue, 24 Nov 2026 00:00:00 GMT");
        let h = headers(&[("cache-control", "max-age=10, stale-while-revalidate=20")]);
        let at_seconds = |s| t.saturating_add_seconds(s);
        assert_eq!(requires_validation(200, &h, t, t, at_seconds(9)), Validation::None);
        assert_eq!(
            requires_validation(200, &h, t, t, at_seconds(10)),
            Validation::Asynchronous
        );
        assert_eq!(
            requires_validation(200, &h, t, t, at_seconds(30)),
            Validation::Synchronous
        );
        assert_eq!(requires_validation(200, &[], t, t, t), Validation::Synchronous);
        // A clock gone back makes the age negative: nothing to go by is
        // still validated, not taken as fresh.
        let earlier = t.saturating_add_seconds(-60);
        assert_eq!(
            requires_validation(200, &[], t, t, earlier),
            Validation::Synchronous
        );
    }

    #[test]
    fn only_no_store_isnt_stored() {
        assert!(is_storable(&headers(&[("cache-control", "no-cache, private")])));
        assert!(!is_storable(&headers(&[("cache-control", "max-age=5, no-store")])));
    }

    #[test]
    fn validators_go_by_status_version_and_vary() {
        let h = headers(&[
            ("etag", r#""v1""#),
            ("last-modified", "Mon, 23 Nov 2026 08:01:08 GMT"),
        ]);
        assert_eq!(
            validation_headers(200, "h2", &h, false),
            Some(headers(&[
                ("if-none-match", r#""v1""#),
                ("if-modified-since", "Mon, 23 Nov 2026 08:01:08 GMT"),
            ]))
        );
        // No `ETag` before HTTP/1.1.
        assert_eq!(
            validation_headers(200, "http/1.0", &h, false),
            Some(headers(&[(
                "if-modified-since",
                "Mon, 23 Nov 2026 08:01:08 GMT"
            )]))
        );
        // A `Vary` that didn't match: the `ETag` alone.
        assert_eq!(
            validation_headers(200, "http/1.1", &h, true),
            Some(headers(&[("if-none-match", r#""v1""#)]))
        );
        assert_eq!(validation_headers(200, "http/1.0", &h, true), None);
        assert_eq!(validation_headers(404, "http/1.1", &h, false), None);
        assert_eq!(validation_headers(200, "http/1.1", &[], false), None);
    }

    #[test]
    fn a_304_updates_what_it_has_but_not_the_bodys_headers() {
        let stored = headers(&[
            ("content-type", "text/html"),
            ("cache-control", "max-age=10"),
            ("cache-control", "public"),
            ("etag", r#""v1""#),
            ("x-kept", "a"),
        ]);
        let new = headers(&[
            ("cache-control", "max-age=60"),
            ("content-type", "text/plain"),
            ("etag", r#""v2""#),
            ("X-Content-Type-Options", "nosniff"),
            ("date", "Tue, 24 Nov 2026 00:00:00 GMT"),
        ]);
        assert_eq!(
            update(&stored, &new),
            headers(&[
                ("content-type", "text/html"),
                ("etag", r#""v1""#),
                ("x-kept", "a"),
                ("cache-control", "max-age=60"),
                ("date", "Tue, 24 Nov 2026 00:00:00 GMT"),
            ])
        );
    }
}
