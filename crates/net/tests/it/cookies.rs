//! Cookies: a `Set-Cookie` line parsed, the cookie it makes for a URL, and
//! the jar that keeps them, against what Chromium does (its features as
//! they are on by default) and, where it agrees, RFC 6265bis and WPT's
//! `cookies/` tests.

use net::cookies::{
    CanonicalCookie, CookieInclusionStatus, CookieMonster, CookieOptions, CookiePartitionKey,
    CookieSameSite, CookieSourceScheme, CookieSourceType, ExclusionReason,
    MAX_COOKIE_ATTRIBUTE_VALUE_SIZE, MAX_COOKIE_NAME_PLUS_VALUE_SIZE, ParsedCookie,
    SameSiteCookieContext, Time, parse_cookie_expiration_time,
};
use site::{AncestorChainBit, Site};
use url::Url;

use ExclusionReason::*;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

/// A fixed "now" for the tests, and `seconds` after it.
fn at(seconds: i64) -> Time {
    Time::from_unix_seconds(1_700_000_000 + seconds)
}

fn utc(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Time {
    let days = days_since_epoch(year, month, day);
    Time::from_unix_seconds(
        days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second),
    )
}

/// Days from 1970-01-01, by counting: independent of the crate's formula.
fn days_since_epoch(year: u32, month: u32, day: u32) -> i64 {
    let leap = |y: u32| (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let mut days: i64 = 0;
    if year >= 1970 {
        for y in 1970..year {
            days += if leap(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if leap(y) { 366 } else { 365 };
        }
    }
    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    days += lengths.iter().take((month - 1) as usize).sum::<i64>();
    days + i64::from(day) - 1
}

fn create(at_url: &str, line: &str) -> Result<CanonicalCookie, CookieInclusionStatus> {
    CanonicalCookie::create(
        &url(at_url),
        line,
        at(0),
        None,
        None,
        CookieSourceType::Http,
    )
}

/// The partition of a frame of another site in a top-level frame of
/// `site`.
fn partition(site: &str) -> CookiePartitionKey {
    CookiePartitionKey::new(Site::of_url(&url(site)), AncestorChainBit::CrossSite)
}

fn created(at_url: &str, line: &str) -> CanonicalCookie {
    create(at_url, line).unwrap_or_else(|status| panic!("{line:?} at {at_url}: {status}"))
}

fn refused(at_url: &str, line: &str) -> Vec<ExclusionReason> {
    match create(at_url, line) {
        Ok(cookie) => panic!("{line:?} at {at_url} made {cookie:?}"),
        Err(status) => status.exclusion_reasons().to_vec(),
    }
}

fn parsed(line: &str) -> ParsedCookie {
    ParsedCookie::parse(line).unwrap_or_else(|reason| panic!("{line:?}: {reason:?}"))
}

/// An HTTP response's options, in a same-site context.
fn http() -> CookieOptions {
    CookieOptions {
        include_httponly: true,
        same_site_context: SameSiteCookieContext::SameSiteStrict,
        lax_allow_unsafe: true,
    }
}

/// Script's (`document.cookie`), in a same-site document.
fn script() -> CookieOptions {
    CookieOptions {
        include_httponly: false,
        ..http()
    }
}

fn in_context(context: SameSiteCookieContext) -> CookieOptions {
    CookieOptions {
        same_site_context: context,
        ..http()
    }
}

/// A jar and a clock that ticks a second at each cookie set.
struct Jar {
    monster: CookieMonster,
    now: i64,
}

impl Jar {
    fn new() -> Self {
        Self {
            monster: CookieMonster::new(),
            now: 0,
        }
    }

    fn set_with(
        &mut self,
        at_url: &str,
        line: &str,
        options: &CookieOptions,
    ) -> Result<(), CookieInclusionStatus> {
        self.now += 1;
        let source = if options.include_httponly {
            CookieSourceType::Http
        } else {
            CookieSourceType::Script
        };
        let cookie = CanonicalCookie::create(&url(at_url), line, at(self.now), None, None, source)?;
        self.monster
            .set_canonical_cookie(cookie, &url(at_url), options)
    }

    fn set(&mut self, at_url: &str, line: &str) {
        self.set_with(at_url, line, &http())
            .unwrap_or_else(|status| panic!("{line:?} at {at_url}: {status}"));
    }

    fn refused(
        &mut self,
        at_url: &str,
        line: &str,
        options: &CookieOptions,
    ) -> Vec<ExclusionReason> {
        match self.set_with(at_url, line, options) {
            Ok(()) => panic!("{line:?} at {at_url} was set"),
            Err(status) => status.exclusion_reasons().to_vec(),
        }
    }

    /// The `Cookie` header a request to `to` sends now in `options`; empty
    /// for none.
    fn line_with(&mut self, to: &str, options: &CookieOptions) -> String {
        self.monster
            .cookie_line_for(&url(to), options, None, at(self.now))
            .unwrap_or_default()
    }

    fn line(&mut self, to: &str) -> String {
        self.line_with(to, &http())
    }
}

// --- ParsedCookie -------------------------------------------------------

#[test]
fn parses_a_name_and_value() {
    let cookie = parsed("a=b");
    assert_eq!((cookie.name(), cookie.value()), ("a", "b"));
    assert_eq!(cookie.attributes().count(), 0);
    assert_eq!(cookie.path(), None);
    assert!(!cookie.is_secure());
}

#[test]
fn trims_spaces_and_tabs_around_names_and_values() {
    let cookie = parsed("  test =  4 \t; path = /x ");
    assert_eq!((cookie.name(), cookie.value()), ("test", "4"));
    assert_eq!(cookie.path(), Some("/x"));
    let cookie = parsed("\ta=b\t");
    assert_eq!((cookie.name(), cookie.value()), ("a", "b"));
    // Inside, whitespace stays.
    let cookie = parsed("test test=8 8");
    assert_eq!((cookie.name(), cookie.value()), ("test test", "8 8"));
}

#[test]
fn a_first_pair_without_equals_is_a_nameless_value() {
    let cookie = parsed("abc");
    assert_eq!((cookie.name(), cookie.value()), ("", "abc"));
    let cookie = parsed("  abc def ; secure");
    assert_eq!((cookie.name(), cookie.value()), ("", "abc def"));
    assert!(cookie.is_secure());
    let cookie = parsed("=abc");
    assert_eq!((cookie.name(), cookie.value()), ("", "abc"));
    let cookie = parsed("test1=");
    assert_eq!((cookie.name(), cookie.value()), ("test1", ""));
    // A `;` ends the value, quoted or not.
    let cookie = parsed("\"test9;test\"=9");
    assert_eq!((cookie.name(), cookie.value()), ("", "\"test9"));
}

#[test]
fn the_value_runs_to_the_first_semicolon() {
    let cookie = parsed("a=b=c; d");
    assert_eq!((cookie.name(), cookie.value()), ("a", "b=c"));
    let cookie = parsed("$Version=1; test=7");
    assert_eq!((cookie.name(), cookie.value()), ("$Version", "1"));
    assert_eq!(cookie.attributes().collect::<Vec<_>>(), [("test", "7")]);
}

#[test]
fn no_name_and_no_value_is_no_cookie() {
    for line in ["", "=", "   ", ";", " = ; path=/"] {
        assert_eq!(ParsedCookie::parse(line), Err(NoCookieContent), "{line:?}");
    }
}

#[test]
fn terminators_anywhere_refuse_the_line() {
    // `;\n` would be no cookie at all, but for the terminator.
    for line in [
        "a=b\nc",
        "a=b\r",
        "a=\0b",
        "a=b; path=/\n",
        "\na=b",
        ";\n",
        " \r",
    ] {
        assert_eq!(
            ParsedCookie::parse(line),
            Err(DisallowedCharacter),
            "{line:?}"
        );
    }
}

#[test]
fn control_characters_refuse_the_line_a_tab_too() {
    // RFC 6265bis lets a tab through; Chromium (and so WPT's expectations
    // for it) doesn't.
    for line in [
        "a=b\tc",
        "a\tb=c",
        "a\u{1}=b",
        "a=b\u{7f}",
        "a=b; pa\u{1}th=/",
        "a=b; path=/\u{7f}",
        "a=b; path=/x\ty",
    ] {
        assert_eq!(
            ParsedCookie::parse(line),
            Err(DisallowedCharacter),
            "{line:?}"
        );
    }
}

#[test]
fn name_and_value_size_limit() {
    let name = "n".repeat(96);
    let fits = format!(
        "{name}={}",
        "v".repeat(MAX_COOKIE_NAME_PLUS_VALUE_SIZE - 96)
    );
    assert!(ParsedCookie::parse(&fits).is_ok());
    let over = format!(
        "{name}={}",
        "v".repeat(MAX_COOKIE_NAME_PLUS_VALUE_SIZE - 95)
    );
    assert_eq!(ParsedCookie::parse(&over), Err(NameValuePairExceedsMaxSize));
    let nameless = "v".repeat(MAX_COOKIE_NAME_PLUS_VALUE_SIZE + 1);
    assert_eq!(
        ParsedCookie::parse(&nameless),
        Err(NameValuePairExceedsMaxSize)
    );
}

#[test]
fn an_attribute_value_too_long_is_ignored() {
    let long = format!("/{}", "p".repeat(MAX_COOKIE_ATTRIBUTE_VALUE_SIZE));
    let fits = &long[..MAX_COOKIE_ATTRIBUTE_VALUE_SIZE];
    let cookie = parsed(&format!("a=b; path={fits}"));
    assert_eq!(cookie.path(), Some(fits));
    // An earlier one of the name counts instead.
    let cookie = parsed(&format!("a=b; path=/x; path={long}"));
    assert_eq!(cookie.path(), Some("/x"));
    assert_eq!(cookie.attributes().count(), 1);
}

#[test]
fn attribute_names_are_case_insensitive_and_the_last_wins() {
    let cookie = parsed("a=b; PATH=/x; Path=/y; DoMaIn=one.example; domain=two.example");
    assert_eq!(cookie.path(), Some("/y"));
    assert_eq!(cookie.domain(), Some("two.example"));
    let cookie = parsed("a=b; Expires=Wed, 01 Jan 2031 00:00:00 GMT; MAX-AGE=10; max-age=20");
    assert_eq!(cookie.expires(), Some("Wed, 01 Jan 2031 00:00:00 GMT"));
    assert_eq!(cookie.max_age(), Some("20"));
    assert_eq!(
        cookie
            .attributes()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["expires", "max-age", "max-age"]
    );
}

#[test]
fn flags_whatever_their_value() {
    let cookie = parsed("a=b; SECURE=no; httponly; Partitioned=");
    assert!(cookie.is_secure());
    assert!(cookie.is_http_only());
    assert!(cookie.is_partitioned());
    // `Domain=` is there, empty.
    assert_eq!(parsed("a=b; domain=").domain(), Some(""));
}

#[test]
fn same_site_values() {
    for (line, same_site) in [
        ("a=b", CookieSameSite::Unspecified),
        ("a=b; SameSite=Strict", CookieSameSite::StrictMode),
        ("a=b; samesite=LAX", CookieSameSite::LaxMode),
        ("a=b; SameSite=none", CookieSameSite::NoRestriction),
        ("a=b; SameSite=Extended", CookieSameSite::Unspecified),
        ("a=b; SameSite=", CookieSameSite::Unspecified),
        ("a=b; SameSite", CookieSameSite::Unspecified),
        (
            "a=b; SameSite=Lax; SameSite=bogus",
            CookieSameSite::Unspecified,
        ),
        (
            "a=b; SameSite=Lax; SameSite=Strict",
            CookieSameSite::StrictMode,
        ),
    ] {
        assert_eq!(parsed(line).same_site(), same_site, "{line}");
    }
}

#[test]
fn non_ascii_names_and_values() {
    let cookie = parsed("é=ü; path=/ö");
    assert_eq!((cookie.name(), cookie.value()), ("é", "ü"));
    assert_eq!(cookie.path(), Some("/ö"));
}

// --- Expires dates ------------------------------------------------------

#[test]
fn expires_dates() {
    for (date, time) in [
        ("Fri, 01 Jan 2038 00:00:00 GMT", utc(2038, 1, 1, 0, 0, 0)),
        ("Sat, 15-Apr-17 21:01:22 GMT", utc(2017, 4, 15, 21, 1, 22)),
        (
            "Thu, 10 Apr 1980 16:33:12 GMT",
            utc(1980, 4, 10, 16, 33, 12),
        ),
        ("Fri Jan 01 03:14:07 2038", utc(2038, 1, 1, 3, 14, 7)),
        (
            "Thu Apr 10 1980 16:33:12 GMT-0700 (PDT)",
            utc(1980, 4, 10, 16, 33, 12),
        ),
        ("1 JANUARY 2021 1:2:3", utc(2021, 1, 1, 1, 2, 3)),
        ("29 feb 2024 23:59:59", utc(2024, 2, 29, 23, 59, 59)),
        // The first time-like token counts.
        ("1 Jan 2021 01:02:03 04:05:06", utc(2021, 1, 1, 1, 2, 3)),
        ("2021 1 Jan 00:00:00", utc(2021, 1, 1, 0, 0, 0)),
        // Two-digit years.
        ("1 Jan 70 00:00:00", utc(1970, 1, 1, 0, 0, 0)),
        ("1 Jan 99 00:00:00", utc(1999, 1, 1, 0, 0, 0)),
        ("1 Jan 69 00:00:00", utc(2069, 1, 1, 0, 0, 0)),
        ("1 Jan 0 00:00:00", utc(2000, 1, 1, 0, 0, 0)),
        // Leading zeros, a five-digit year, many digits in a time field.
        ("01 Jan 02024 0000001:02:03", utc(2024, 1, 1, 1, 2, 3)),
        ("1 Jan 10000 00:00:00", utc(10000, 1, 1, 0, 0, 0)),
        // Chromium takes a year before 1601 (a date long gone); the draft
        // fails it.
        ("1 Jan 1600 00:00:00", utc(1600, 1, 1, 0, 0, 0)),
    ] {
        assert_eq!(parse_cookie_expiration_time(date), Some(time), "{date}");
    }
}

#[test]
fn not_expires_dates() {
    for date in [
        "",
        "Jan 2038 00:00:00",
        "1 2038 00:00:00",
        "1 Jan 00:00:00",
        "1 Jan 2038",
        "31 Feb 2021 00:00:00",
        "29 Feb 2023 00:00:00",
        "0 Jan 2021 00:00:00",
        "32 Jan 2021 00:00:00",
        "1 Jan 2021 24:00:00",
        "1 Jan 2021 00:60:00",
        "1 Jan 2021 00:00:60",
        "1 Jan 2021 1:2",
        "1 Jan 2021 1:2:3:4",
        "1 Jan 2021 1:2:x",
        "1 Jan 100000 00:00:00",
        // Chromium wants a day all digits; the draft would take this one.
        "15th Jan 2021 00:00:00",
        "1 Ja 2021 00:00:00",
    ] {
        assert_eq!(parse_cookie_expiration_time(date), None, "{date:?}");
    }
}

// --- CanonicalCookie::create --------------------------------------------

#[test]
fn a_host_only_cookie_with_the_default_path() {
    let cookie = created("https://www.example.com/a/b?q", "a=b");
    assert_eq!(cookie.domain(), "www.example.com");
    assert!(cookie.is_host_cookie());
    assert_eq!(cookie.path(), "/a");
    assert_eq!(cookie.creation_date(), at(0));
    assert_eq!(cookie.expiry_date(), None);
    assert!(!cookie.is_persistent());
    assert_eq!(cookie.source_scheme(), CookieSourceScheme::Secure);
    assert_eq!(cookie.source_port(), Some(443));
    assert_eq!(cookie.source_type(), CookieSourceType::Http);
}

#[test]
fn default_paths() {
    for (at_url, path) in [
        ("https://example.com", "/"),
        ("https://example.com/", "/"),
        ("https://example.com/a", "/"),
        ("https://example.com/a/", "/a"),
        ("https://example.com/a/b/c", "/a/b"),
        ("https://example.com/a/b/", "/a/b"),
    ] {
        assert_eq!(created(at_url, "a=b").path(), path, "{at_url}");
    }
    // A `Path` that doesn't start with `/` is the default.
    assert_eq!(
        created("https://example.com/a/b", "a=b; path=x").path(),
        "/a"
    );
    assert_eq!(
        created("https://example.com/a/b", "a=b; path=").path(),
        "/a"
    );
    assert_eq!(
        created("https://example.com/a/b", "a=b; path=/z/").path(),
        "/z/"
    );
}

#[test]
fn domain_cookies() {
    for domain in [
        "example.com",
        ".example.com",
        "EXAMPLE.com",
        "www.example.com",
    ] {
        let cookie = created("https://www.example.com/", &format!("a=b; Domain={domain}"));
        let expected = if domain == "www.example.com" {
            ".www.example.com"
        } else {
            ".example.com"
        };
        assert_eq!(cookie.domain(), expected, "{domain}");
        assert!(cookie.is_domain_cookie());
    }
    // `Domain=` is no domain.
    assert_eq!(
        created("https://www.example.com/", "a=b; Domain=").domain(),
        "www.example.com"
    );
    // A non-ASCII domain is the punycode one.
    assert_eq!(
        created(
            "https://www.xn--bcher-kva.example/",
            "a=b; Domain=bücher.example"
        )
        .domain(),
        ".xn--bcher-kva.example"
    );
}

#[test]
fn domains_a_host_cant_set() {
    assert_eq!(
        refused("https://www.example.com/", "a=b; Domain=other.com"),
        [DomainMismatch, InvalidDomain]
    );
    // A subdomain of the host.
    assert_eq!(
        refused(
            "https://www.example.com/",
            "a=b; Domain=sub.www.example.com"
        ),
        [InvalidDomain]
    );
    // Of the same registrable domain, but not over the host.
    assert_eq!(
        refused("https://www.example.com/", "a=b; Domain=other.example.com"),
        [InvalidDomain]
    );
    assert_eq!(
        refused("https://www.example.com/", "a=b; Domain=ample.com"),
        [DomainMismatch, InvalidDomain]
    );
    // A public suffix.
    for (at_url, domain) in [
        ("https://www.example.com/", "com"),
        ("https://www.example.co.uk/", "co.uk"),
        ("https://foo.github.io/", "github.io"),
    ] {
        assert!(
            refused(at_url, &format!("a=b; Domain={domain}")).contains(&InvalidDomain),
            "{domain} at {at_url}"
        );
    }
    for domain in ["%65xample.com", "exa mple.com", ".", "..example.com"] {
        assert!(
            refused("https://www.example.com/", &format!("a=b; Domain={domain}"))
                .contains(&InvalidDomain),
            "{domain}"
        );
    }
}

#[test]
fn a_public_suffix_or_intranet_host_sets_host_cookies_only() {
    // A public suffix that is the host makes a host cookie.
    for domain in ["github.io", ".GitHub.io"] {
        assert_eq!(
            created("https://github.io/", &format!("a=b; Domain={domain}")).domain(),
            "github.io"
        );
    }
    assert_eq!(
        created("http://localhost/", "a=b; Domain=.localhost").domain(),
        "localhost"
    );
    assert_eq!(
        refused("http://intranet/", "a=b; Domain=other"),
        [InvalidDomain]
    );
}

#[test]
fn ip_address_hosts() {
    for domain in ["", "127.0.0.1", ".127.0.0.1"] {
        let cookie = created("http://127.0.0.1/", &format!("a=b; Domain={domain}"));
        assert_eq!(cookie.domain(), "127.0.0.1", "{domain}");
        assert!(cookie.is_host_cookie());
    }
    for domain in ["0.0.1", "0.1", "1"] {
        assert_eq!(
            refused("http://127.0.0.1/", &format!("a=b; Domain={domain}")),
            [InvalidDomain],
            "{domain}"
        );
    }
    assert_eq!(
        created("http://[::1]/", "a=b; Domain=[::1]").domain(),
        "[::1]"
    );
    assert_eq!(
        created("http://[::1]:8000/", "a=b").source_port(),
        Some(8000)
    );
}

#[test]
fn max_age_and_expires() {
    let expiry = |line: &str| created("https://example.com/", line).expiry_date();
    assert_eq!(expiry("a=b; Max-Age=100"), Some(at(100)));
    assert_eq!(expiry("a=b; Max-Age=+100"), Some(at(100)));
    // Zero or less: the earliest time.
    for max_age in ["0", "-1", "-20", "-99999999999999999999"] {
        assert_eq!(expiry(&format!("a=b; max-age={max_age}")), Some(Time::MIN));
    }
    // Max-Age before Expires, wherever they are.
    assert_eq!(
        expiry("a=b; Max-Age=100; Expires=Thu, 10 Apr 1980 16:33:12 GMT"),
        Some(at(100))
    );
    assert_eq!(
        expiry("a=b; Expires=Thu, 10 Apr 1980 16:33:12 GMT; Max-Age=100"),
        Some(at(100))
    );
    // A Max-Age that isn't a number is ignored: Expires counts.
    for max_age in ["abc", "50,399", "2.5", "", "1 0"] {
        assert_eq!(
            expiry(&format!(
                "a=b; Max-Age={max_age}; Expires=Thu, 10 Apr 1980 16:33:12 GMT"
            )),
            Some(utc(1980, 4, 10, 16, 33, 12)),
            "{max_age}"
        );
        assert_eq!(expiry(&format!("a=b; Max-Age={max_age}")), None);
    }
    // An Expires that isn't a date is none.
    assert_eq!(expiry("a=b; Expires=tomorrow"), None);
    assert_eq!(expiry("a=b; Expires="), None);
}

#[test]
fn expiry_is_at_most_400_days_away() {
    let latest = at(400 * 86_400);
    let expiry = |line: &str| created("https://example.com/", line).expiry_date();
    assert_eq!(expiry("a=b; Max-Age=99999999999999999999"), Some(latest));
    assert_eq!(expiry("a=b; Max-Age=9223372036854775807"), Some(latest));
    assert_eq!(
        expiry(&format!("a=b; Max-Age={}", 400 * 86_400 + 1)),
        Some(latest)
    );
    assert_eq!(
        expiry(&format!("a=b; Max-Age={}", 400 * 86_400 - 1)),
        Some(at(400 * 86_400 - 1))
    );
    assert_eq!(
        expiry("a=b; Expires=Fri, 01 Jan 2100 00:00:00 GMT"),
        Some(latest)
    );
}

#[test]
fn expires_moves_by_the_servers_clock_skew() {
    // The server's clock is an hour behind ours.
    let cookie = CanonicalCookie::create(
        &url("https://example.com/"),
        "a=b; Expires=Fri, 01 Jan 2038 00:00:00 GMT",
        utc(2030, 1, 1, 1, 0, 0),
        Some(utc(2030, 1, 1, 0, 0, 0)),
        None,
        CookieSourceType::Http,
    )
    .unwrap();
    assert_eq!(
        cookie.expiry_date(),
        Some(utc(2030, 1, 1, 1, 0, 0).saturating_add_days(400))
    );
    let cookie = CanonicalCookie::create(
        &url("https://example.com/"),
        "a=b; Expires=Wed, 02 Jan 2030 00:00:00 GMT",
        utc(2030, 1, 1, 1, 0, 0),
        Some(utc(2030, 1, 1, 0, 0, 0)),
        None,
        CookieSourceType::Http,
    )
    .unwrap();
    assert_eq!(cookie.expiry_date(), Some(utc(2030, 1, 2, 1, 0, 0)));
}

#[test]
fn secure_from_a_trustworthy_url_looks_set_from_a_secure_one() {
    let cookie = created("http://localhost/", "a=b; Secure");
    assert_eq!(cookie.source_scheme(), CookieSourceScheme::Secure);
    assert_eq!(cookie.source_port(), Some(443));
    let cookie = created("http://localhost:8000/", "a=b; Secure");
    assert_eq!(cookie.source_port(), Some(8000));
    let cookie = created("http://example.com/", "a=b");
    assert_eq!(cookie.source_scheme(), CookieSourceScheme::NonSecure);
    assert_eq!(cookie.source_port(), Some(80));
}

#[test]
fn secure_prefix() {
    for line in [
        "__Secure-a=b; Secure",
        "__SECURE-a=b; secure",
        "__secure-a=b; Secure",
    ] {
        created("https://example.com/", line);
        created("http://localhost/", line);
    }
    assert_eq!(
        refused("https://example.com/", "__Secure-a=b"),
        [InvalidPrefix]
    );
    assert_eq!(
        refused("https://example.com/", "__sEcUrE-a=b"),
        [InvalidPrefix]
    );
    assert_eq!(
        refused("http://example.com/", "__Secure-a=b; Secure"),
        [InvalidPrefix]
    );
    // Not quite the prefix: no rule.
    created("http://example.com/", "__Secure=b");
    created("http://example.com/", "_Secure-a=b");
}

#[test]
fn host_prefix() {
    created("https://example.com/a/b", "__Host-a=b; Secure; Path=/");
    created(
        "https://example.com/",
        "__HOST-a=b; Secure; Path=/; Domain=",
    );
    created("http://localhost/", "__Host-a=b; Secure; Path=/");
    created(
        "http://127.0.0.1/",
        "__Host-a=b; Secure; Path=/; Domain=127.0.0.1",
    );
    for line in [
        "__Host-a=b; Path=/",
        // `Path=/` has to be said, even where it is the default.
        "__Host-a=b; Secure",
        "__Host-a=b; Secure; Path=/a",
        "__Host-a=b; Secure; Path=/; Domain=example.com",
        "__host-a=b; Secure; Path=/; Domain=.example.com",
    ] {
        assert_eq!(
            refused("https://example.com/", line),
            [InvalidPrefix],
            "{line}"
        );
    }
    assert_eq!(
        refused("http://example.com/", "__Host-a=b; Secure; Path=/"),
        [InvalidPrefix]
    );
}

#[test]
fn http_prefixes() {
    created("https://example.com/", "__Http-a=b; Secure; HttpOnly");
    created(
        "https://example.com/",
        "__Host-Http-a=b; Secure; HttpOnly; Path=/",
    );
    for line in [
        "__Http-a=b; Secure",
        "__Http-a=b; HttpOnly",
        "__Host-Http-a=b; Secure; Path=/",
        "__Host-Http-a=b; Secure; HttpOnly",
        "__HOST-HTTP-a=b; Secure; HttpOnly; Path=/; Domain=example.com",
    ] {
        assert_eq!(
            refused("https://example.com/", line),
            [InvalidPrefix],
            "{line}"
        );
    }
}

#[test]
fn nameless_cookies() {
    let cookie = created("https://example.com/", "abc");
    assert_eq!((cookie.name(), cookie.value()), ("", "abc"));
    created("https://example.com/", "=abc");
    // One that would read as a prefixed name, `Secure` or not.
    for line in [
        "__Secure-abc",
        "=__Host-abc=123",
        "=__SeCuRe-abc=123",
        "__HoSt-abc; Secure; Path=/",
        "__Http-abc",
        "__Host-Http-abc",
    ] {
        assert!(
            refused("https://example.com/", line).contains(&InvalidPrefix),
            "{line}"
        );
    }
    // One that would read as a name and a value (Chromium's
    // `kCookieParseRejectEmptyNameAmbiguous`; WPT and the draft take it).
    assert_eq!(
        refused("https://example.com/", "=test=2"),
        [AmbiguousSerialization]
    );
    assert_eq!(
        refused("https://example.com/", "===test=2b"),
        [AmbiguousSerialization]
    );
    // Every reason, together.
    assert_eq!(
        refused("https://example.com/", "=__Secure-a=b; Domain=other.com"),
        [
            AmbiguousSerialization,
            DomainMismatch,
            InvalidDomain,
            InvalidPrefix
        ]
    );
}

/// A `Partitioned` cookie set by a request of a partition is kept in it,
/// and has to be `Secure`, from a URL that may set one; set by a request
/// without one, it is an unpartitioned cookie, `Secure` or not.
#[test]
fn partitioned_needs_secure_from_a_secure_url() {
    let in_partition = |at_url: &str, line: &str| {
        CanonicalCookie::create(
            &url(at_url),
            line,
            at(0),
            None,
            Some(&partition("https://top.example")),
            CookieSourceType::Http,
        )
    };
    let cookie = in_partition("https://example.com/", "a=b; Secure; Partitioned").unwrap();
    assert_eq!(
        cookie.partition_key(),
        Some(&partition("https://top.example"))
    );
    assert!(cookie.is_partitioned());
    assert!(in_partition("http://localhost/", "a=b; Secure; Partitioned").is_ok());
    let refused = |at_url: &str, line: &str| {
        in_partition(at_url, line)
            .unwrap_err()
            .exclusion_reasons()
            .to_vec()
    };
    assert_eq!(
        refused("https://example.com/", "a=b; Partitioned"),
        [InvalidPartitioned]
    );
    assert_eq!(
        refused("http://example.com/", "a=b; Partitioned; Secure"),
        [InvalidPartitioned]
    );
    // Not `Partitioned`: not in the partition.
    let cookie = in_partition("https://example.com/", "a=b").unwrap();
    assert_eq!(cookie.partition_key(), None);
    // No partition: unpartitioned, and needn't be `Secure`.
    let cookie = created("https://example.com/", "a=b; Partitioned");
    assert!(!cookie.is_partitioned());
    created("http://example.com/", "a=b; Partitioned");
}

#[test]
fn what_parsing_refuses_refuses_creating() {
    assert_eq!(refused("https://example.com/", ""), [NoCookieContent]);
    assert_eq!(
        refused("https://example.com/", "a=b\tc"),
        [DisallowedCharacter]
    );
    assert_eq!(refused("data:text/plain,x", "a=b"), [FailureToStore]);
}

#[test]
fn non_ascii_cookies() {
    let cookie = created("https://example.com/", "名前=値");
    assert_eq!((cookie.name(), cookie.value()), ("名前", "値"));
}

#[test]
fn cookie_lines() {
    let cookies = [
        created("https://example.com/", "a=b"),
        created("https://example.com/", "nameless"),
        created("https://example.com/", "empty="),
    ];
    assert_eq!(
        CanonicalCookie::build_cookie_line(&cookies),
        "a=b; nameless; empty="
    );
    assert_eq!(CanonicalCookie::build_cookie_line(&[]), "");
}

// --- CookieMonster ------------------------------------------------------

#[test]
fn sets_and_sends() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=b");
    jar.set("https://example.com/", "c=d");
    assert_eq!(jar.line("https://example.com/"), "a=b; c=d");
    assert_eq!(jar.line("https://example.com/x/y"), "a=b; c=d");
    assert_eq!(jar.line("https://other.example/"), "");
}

#[test]
fn the_same_name_domain_and_path_replace() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "test12=11");
    jar.set("https://example.com/", "test12=12");
    assert_eq!(jar.line("https://example.com/"), "test12=12");
    // Another path, or a domain cookie of the host, is another cookie.
    jar.set("https://example.com/", "test12=13; Path=/a");
    jar.set("https://example.com/", "test12=14; Domain=example.com");
    assert_eq!(
        jar.line("https://example.com/a"),
        "test12=13; test12=12; test12=14"
    );
    // WPT's `name.html`: a nameless cookie replaces a nameless one.
    let mut jar = Jar::new();
    jar.set("https://example.com/", "test11");
    jar.set("https://example.com/", "test11a");
    assert_eq!(jar.line("https://example.com/"), "test11a");
    let mut jar = Jar::new();
    jar.set("https://example.com/", "\"test=5\"=test");
    jar.set("https://example.com/", "\"test=5");
    assert_eq!(jar.line("https://example.com/"), "\"test=5");
    let mut jar = Jar::new();
    jar.set("https://example.com/", "testA=16");
    jar.set("https://example.com/", "=test16");
    jar.set("https://example.com/", "testB=16");
    assert_eq!(
        jar.line("https://example.com/"),
        "testA=16; test16; testB=16"
    );
}

#[test]
fn longer_paths_first_then_older() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "z=1");
    jar.set("https://example.com/", "a=2");
    jar.set("https://example.com/", "deep=3; Path=/a/b");
    jar.set("https://example.com/", "mid=4; Path=/a");
    jar.set("https://example.com/", "first=5; Path=/a/b/");
    assert_eq!(
        jar.line("https://example.com/a/b/c"),
        "first=5; deep=3; mid=4; z=1; a=2"
    );
}

#[test]
fn the_same_value_set_again_keeps_its_age() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=1");
    jar.set("https://example.com/", "b=1");
    jar.set("https://example.com/", "a=1; Max-Age=1000");
    assert_eq!(jar.line("https://example.com/"), "a=1; b=1");
    let all = jar.monster.all_cookies(at(jar.now));
    assert_eq!(all[0].creation_date(), at(1));
    assert_eq!(all[0].expiry_date(), Some(at(3 + 1000)));
    // Another value is a new cookie (RFC 6265bis would keep the age).
    jar.set("https://example.com/", "a=2");
    assert_eq!(jar.line("https://example.com/"), "b=1; a=2");
}

#[test]
fn an_expired_cookie_deletes_and_isnt_kept() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "testA=7; max-age=60");
    jar.set("https://example.com/", "testB=7; max-age=60");
    jar.set("https://example.com/", "testA=differentvalue; max-age=0");
    jar.set("https://example.com/", "testC=7; max-age=0");
    assert_eq!(jar.line("https://example.com/"), "testB=7");
    jar.set(
        "https://example.com/",
        "testB=7; Expires=Thu, 10 Apr 1980 16:33:12 GMT",
    );
    assert_eq!(jar.line("https://example.com/"), "");
    assert!(jar.monster.all_cookies(at(jar.now)).is_empty());
}

#[test]
fn expired_cookies_go_when_read() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=b; Max-Age=10");
    jar.set("https://example.com/", "c=d");
    let options = http();
    let target = url("https://example.com/");
    let cookies = jar
        .monster
        .get_cookie_list_with_options(&target, &options, None, at(10));
    assert_eq!(CanonicalCookie::build_cookie_line(&cookies), "a=b; c=d");
    let cookies = jar
        .monster
        .get_cookie_list_with_options(&target, &options, None, at(11));
    assert_eq!(CanonicalCookie::build_cookie_line(&cookies), "c=d");
    assert_eq!(jar.monster.all_cookies(at(11)).len(), 1);
}

/// A cookie read is marked as used then, but not again within a minute of
/// its last use (Chromium's `kDefaultAccessUpdateThresholdSeconds`).
#[test]
fn a_read_marks_its_use_a_minute_apart() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=b");
    let target = url("https://example.com/");
    let last_use = |jar: &mut Jar, now| {
        jar.monster
            .get_cookie_list_with_options(&target, &http(), None, at(now))[0]
            .last_access_date()
    };
    assert_eq!(last_use(&mut jar, 60), at(1));
    assert_eq!(last_use(&mut jar, 61), at(61));
    assert_eq!(last_use(&mut jar, 120), at(61));
    assert_eq!(last_use(&mut jar, 121), at(121));
}

#[test]
fn domain_matching() {
    let mut jar = Jar::new();
    jar.set("https://www.example.com/", "host=1");
    jar.set("https://www.example.com/", "domain=2; Domain=example.com");
    assert_eq!(jar.line("https://www.example.com/"), "host=1; domain=2");
    assert_eq!(jar.line("https://example.com/"), "domain=2");
    assert_eq!(jar.line("https://a.b.example.com/"), "domain=2");
    assert_eq!(jar.line("https://sub.www.example.com/"), "domain=2");
    assert_eq!(jar.line("https://notexample.com/"), "");
    // At a label: `.b.example.com` isn't `ab.example.com`'s.
    jar.set("https://x.b.example.com/", "b=3; Domain=b.example.com");
    assert_eq!(jar.line("https://x.b.example.com/"), "domain=2; b=3");
    assert_eq!(jar.line("https://ab.example.com/"), "domain=2");
    assert_eq!(jar.line("https://example.org/"), "");
}

#[test]
fn path_matching() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=b; Path=/a");
    assert_eq!(jar.line("https://example.com/a"), "a=b");
    assert_eq!(jar.line("https://example.com/a/"), "a=b");
    assert_eq!(jar.line("https://example.com/a/b?x"), "a=b");
    assert_eq!(jar.line("https://example.com/ab"), "");
    assert_eq!(jar.line("https://example.com/"), "");
    assert_eq!(jar.line("https://example.com/A"), "");
}

#[test]
fn secure_cookies_only_from_and_to_secure_urls() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "s=1; Secure");
    jar.set("http://example.com/", "i=2");
    assert_eq!(jar.line("https://example.com/"), "s=1; i=2");
    assert_eq!(jar.line("wss://example.com/"), "s=1; i=2");
    assert_eq!(jar.line("http://example.com/"), "i=2");
    assert_eq!(jar.line("ws://example.com/"), "i=2");
    // Every reason: it is `Secure`, and would overwrite a `Secure` one.
    assert_eq!(
        jar.refused("http://example.com/", "s=3; Secure", &http()),
        [SecureOnly, OverwriteSecure]
    );
    assert_eq!(
        jar.refused("http://example.com/", "t=3; Secure", &http()),
        [SecureOnly]
    );

    // localhost is trustworthy.
    let mut jar = Jar::new();
    jar.set("http://localhost:8000/", "s=1; Secure");
    jar.set("http://sub.localhost/", "t=2; Secure");
    assert_eq!(jar.line("http://localhost:8000/"), "s=1");
    assert_eq!(jar.line("http://sub.localhost/"), "t=2");
    jar.set("http://127.0.0.1/", "u=3; Secure");
    assert_eq!(jar.line("http://127.0.0.1/"), "u=3");
    jar.set("http://[::1]/", "v=4; Secure");
    assert_eq!(jar.line("http://[::1]/"), "v=4");
}

#[test]
fn http_only_cookies_are_not_scripts() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "h=1; HttpOnly");
    jar.set("https://example.com/", "s=2");
    assert_eq!(jar.line("https://example.com/"), "h=1; s=2");
    assert_eq!(jar.line_with("https://example.com/", &script()), "s=2");
    // Script can't set one, or overwrite one.
    assert_eq!(
        jar.refused("https://example.com/", "x=1; HttpOnly", &script()),
        [HttpOnly]
    );
    assert_eq!(
        jar.refused("https://example.com/", "h=evil", &script()),
        [OverwriteHttpOnly]
    );
    assert_eq!(jar.line("https://example.com/"), "h=1; s=2");
    // It can overwrite its own; HTTP can overwrite either.
    jar.set_with("https://example.com/", "s=3", &script())
        .unwrap();
    jar.set("https://example.com/", "h=4");
    assert_eq!(jar.line_with("https://example.com/", &script()), "s=3; h=4");
}

#[test]
fn leave_secure_cookies_alone() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=secure; Secure");
    assert_eq!(
        jar.refused("http://example.com/", "a=evil", &http()),
        [OverwriteSecure]
    );
    // Its domain is in the new one's, or the other way.
    assert_eq!(
        jar.refused(
            "http://www.example.com/",
            "a=evil; Domain=example.com",
            &http()
        ),
        [OverwriteSecure]
    );
    // Another name is fine; a path the secure one isn't on is too.
    jar.set("http://example.com/", "b=fine");
    assert_eq!(jar.line("https://example.com/"), "a=secure; b=fine");

    let mut jar = Jar::new();
    jar.set("https://example.com/login", "a=secure; Secure; Path=/login");
    jar.set("http://example.com/", "a=insecure; Path=/");
    jar.set("http://example.com/", "a=insecure; Path=/foo");
    for path in ["/login", "/login/en"] {
        assert_eq!(
            jar.refused(
                "http://example.com/",
                &format!("a=evil; Path={path}"),
                &http()
            ),
            [OverwriteSecure],
            "{path}"
        );
    }
    assert_eq!(
        jar.line("https://example.com/login"),
        "a=secure; a=insecure"
    );

    // A secure domain cookie shadows a host one under it.
    let mut jar = Jar::new();
    jar.set(
        "https://www.example.com/",
        "a=secure; Secure; Domain=example.com",
    );
    assert_eq!(
        jar.refused("http://sub.example.com/", "a=evil", &http()),
        [OverwriteSecure]
    );
    // A trustworthy URL may.
    let mut jar = Jar::new();
    jar.set("https://localhost/", "a=secure; Secure");
    jar.set("http://localhost/", "a=replaced");
    assert_eq!(jar.line("http://localhost/"), "a=replaced");
}

#[test]
fn an_expired_secure_cookie_keeps_nothing_out() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "a=secure; Secure; Max-Age=5");
    jar.set("https://example.com/", "h=1; HttpOnly; Max-Age=5");
    // Expired by the time of the next set, but not read since.
    jar.now = 10;
    jar.set("http://example.com/", "a=insecure");
    jar.set_with("http://example.com/", "h=script", &script())
        .unwrap();
    assert_eq!(jar.line("http://example.com/"), "a=insecure; h=script");
}

#[test]
fn same_site_when_setting() {
    let cross_site = in_context(SameSiteCookieContext::CrossSite);
    let mut jar = Jar::new();
    assert_eq!(
        jar.refused("https://example.com/", "a=b; SameSite=Strict", &cross_site),
        [SameSiteStrict]
    );
    assert_eq!(
        jar.refused("https://example.com/", "a=b; SameSite=Lax", &cross_site),
        [SameSiteLax]
    );
    assert_eq!(
        jar.refused("https://example.com/", "a=b", &cross_site),
        [SameSiteUnspecifiedTreatedAsLax]
    );
    assert_eq!(
        jar.refused(
            "https://example.com/",
            "a=b; SameSite=Lax",
            &in_context(SameSiteCookieContext::SameSiteLaxMethodUnsafe)
        ),
        [SameSiteLax]
    );
    // `SameSite=None` has to be `Secure`, in any context.
    assert_eq!(
        jar.refused("https://example.com/", "a=b; SameSite=None", &http()),
        [SameSiteNoneInsecure]
    );
    jar.set_with(
        "https://example.com/",
        "n=1; SameSite=None; Secure",
        &cross_site,
    )
    .unwrap();
    // A laxly same-site context (a top-level navigation) sets `Strict` ones.
    jar.set_with(
        "https://example.com/",
        "s=2; SameSite=Strict",
        &in_context(SameSiteCookieContext::SameSiteLax),
    )
    .unwrap();
    assert_eq!(jar.line("https://example.com/"), "n=1; s=2");
}

#[test]
fn same_site_when_sending() {
    let mut jar = Jar::new();
    jar.set("https://example.com/", "strict=1; SameSite=Strict");
    jar.set("https://example.com/", "lax=2; SameSite=Lax");
    jar.set("https://example.com/", "none=3; SameSite=None; Secure");
    jar.set("https://example.com/", "default=4");
    // Two minutes on: no longer Lax-allowing-unsafe.
    jar.now = 4 + 121;
    let line = |jar: &mut Jar, context| jar.line_with("https://example.com/", &in_context(context));
    assert_eq!(
        line(&mut jar, SameSiteCookieContext::SameSiteStrict),
        "strict=1; lax=2; none=3; default=4"
    );
    assert_eq!(
        line(&mut jar, SameSiteCookieContext::SameSiteLax),
        "lax=2; none=3; default=4"
    );
    assert_eq!(
        line(&mut jar, SameSiteCookieContext::SameSiteLaxMethodUnsafe),
        "none=3"
    );
    assert_eq!(line(&mut jar, SameSiteCookieContext::CrossSite), "none=3");
}

#[test]
fn lax_allowing_unsafe_for_two_minutes() {
    let unsafe_navigation = in_context(SameSiteCookieContext::SameSiteLaxMethodUnsafe);
    let mut jar = Jar::new();
    jar.set("https://example.com/", "default=1");
    jar.set("https://example.com/", "lax=2; SameSite=Lax");
    // Made at 1; two minutes on, still.
    jar.now = 1 + 120;
    assert_eq!(
        jar.line_with("https://example.com/", &unsafe_navigation),
        "default=1"
    );
    // Not without the feature.
    let rigorous = CookieOptions {
        lax_allow_unsafe: false,
        ..unsafe_navigation
    };
    assert_eq!(jar.line_with("https://example.com/", &rigorous), "");
    // Not cross-site.
    assert_eq!(
        jar.line_with(
            "https://example.com/",
            &in_context(SameSiteCookieContext::CrossSite)
        ),
        ""
    );
    jar.now = 1 + 121;
    assert_eq!(
        jar.line_with("https://example.com/", &unsafe_navigation),
        ""
    );
}

#[test]
fn only_web_schemes_have_cookies() {
    let mut jar = Jar::new();
    assert_eq!(
        jar.refused("ftp://example.com/", "a=b", &http()),
        [NoncookieableScheme]
    );
    jar.set("https://example.com/", "a=b");
    assert_eq!(jar.line("ftp://example.com/"), "");
    assert_eq!(jar.line("file:///etc/hosts"), "");
}

#[test]
fn ip_address_hosts_keep_their_cookies() {
    let mut jar = Jar::new();
    jar.set("http://127.0.0.1/", "a=b");
    jar.set("http://[::1]/", "c=d");
    jar.set("http://0.0.1/", "e=f");
    assert_eq!(jar.line("http://127.0.0.1/"), "a=b");
    assert_eq!(jar.line("http://[::1]/"), "c=d");
    assert_eq!(jar.line("http://127.0.0.2/"), "");
}

#[test]
fn listing_and_deleting() {
    let mut jar = Jar::new();
    jar.set("https://b.example/", "one=1");
    jar.set("https://a.example/", "two=2; Path=/p");
    jar.set("https://www.b.example/", "three=3; Domain=b.example");
    jar.set("https://b.example/", "four=4; Max-Age=1");
    let names = |jar: &mut Jar| {
        let now = at(jar.now);
        jar.monster
            .all_cookies(now)
            .iter()
            .map(|cookie| cookie.name().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&mut jar), ["two", "one", "three", "four"]);
    jar.now += 1;
    assert_eq!(names(&mut jar), ["two", "one", "three"]);

    assert!(!jar.monster.delete_cookie("two", "a.example", "/"));
    assert!(jar.monster.delete_cookie("two", "a.example", "/p"));
    assert!(!jar.monster.delete_cookie("three", "b.example", "/"));
    assert!(jar.monster.delete_cookie("three", ".b.example", "/"));
    assert_eq!(names(&mut jar), ["one"]);

    jar.set("https://c.example/", "five=5");
    assert_eq!(
        jar.monster
            .delete_matching(|cookie| cookie.domain().ends_with("c.example")),
        1
    );
    jar.set("https://c.example/", "six=6");
    assert_eq!(jar.monster.delete_all(), 2);
    assert_eq!(names(&mut jar), Vec::<String>::new());
}

/// The default options are script's, cross-site, without
/// Lax-allowing-unsafe: machopo's default cookie policy, RFC 6265bis's
/// strict end (Chromium's has it on).
#[test]
fn the_default_options_are_strict() {
    let options = CookieOptions::default();
    assert!(!options.include_httponly);
    assert_eq!(options.same_site_context, SameSiteCookieContext::CrossSite);
    assert!(!options.lax_allow_unsafe);
}

// --- CanonicalCookie::create_sanitized ------------------------------------

fn sanitized(
    at_url: &str,
    name: &str,
    value: &str,
    domain: &str,
    path: &str,
    secure: bool,
    partition: Option<&CookiePartitionKey>,
) -> Result<CanonicalCookie, Vec<ExclusionReason>> {
    CanonicalCookie::create_sanitized(
        &url(at_url),
        name,
        value,
        domain,
        path,
        at(0),
        Some(at(60)),
        secure,
        false,
        CookieSameSite::LaxMode,
        partition,
    )
    .map_err(|status| status.exclusion_reasons().to_vec())
}

/// A cookie from its parts (DevTools', automation's): the URL's host for a
/// host-only one, a domain cookie's of a `Domain` it is in; the path
/// given, or the URL's default; set as from a secure URL if `Secure`. What
/// a `Set-Cookie` line couldn't say is refused: a `;`, a terminator or
/// surrounding space in the name, value, path or domain, a `=` in the
/// name, a domain the host isn't in, a relative path, a prefix without
/// what it asks for, a nameless value that reads as a pair, a partitioned
/// one not `Secure`.
#[test]
fn a_cookie_from_its_parts() {
    let cookie = sanitized("http://www.a.example", "n", "v", "", "", false, None).unwrap();
    assert_eq!(
        (cookie.domain(), cookie.path(), cookie.source_scheme()),
        ("www.a.example", "/", CookieSourceScheme::NonSecure)
    );
    assert_eq!(cookie.expiry_date(), Some(at(60)));
    assert_eq!(cookie.source_type(), CookieSourceType::Other);
    let cookie = sanitized(
        "http://www.a.example",
        "n",
        "v",
        ".a.example",
        "/p",
        true,
        None,
    )
    .unwrap();
    assert_eq!(
        (cookie.domain(), cookie.path(), cookie.source_scheme()),
        (".a.example", "/p", CookieSourceScheme::Secure)
    );
    assert_eq!(cookie.source_port(), Some(443));
    for (name, value, domain, path) in [
        ("a;b", "v", "", ""),
        (" n", "v", "", ""),
        ("n=", "v", "", ""),
        ("n", "v\n", "", ""),
        ("n", "v ", "", ""),
        ("n", "v", "", "/p;"),
        ("n", "\u{1}", "", ""),
    ] {
        assert!(
            sanitized("http://a.example", name, value, domain, path, false, None)
                .unwrap_err()
                .contains(&DisallowedCharacter),
            "{name:?} {value:?} {path:?}"
        );
    }
    assert!(
        sanitized("http://a.example", "n", "v", "a.example\n", "", false, None)
            .unwrap_err()
            .contains(&InvalidDomain)
    );
    assert!(
        sanitized("http://a.example", "n", "v", ".b.example", "", false, None)
            .unwrap_err()
            .contains(&InvalidDomain)
    );
    assert_eq!(
        sanitized("http://a.example", "n", "v", "", "p", false, None).unwrap_err(),
        [InvalidPath]
    );
    assert_eq!(
        sanitized(
            "http://a.example",
            "n",
            "v",
            "",
            &format!("/{}", "p".repeat(1024)),
            false,
            None
        )
        .unwrap_err(),
        [AttributeValueExceedsMaxSize]
    );
    assert_eq!(
        sanitized("http://a.example", "__Secure-n", "v", "", "", false, None).unwrap_err(),
        [InvalidPrefix]
    );
    assert!(sanitized("https://a.example", "__Host-n", "v", "", "", true, None).is_ok());
    assert_eq!(
        sanitized("https://a.example", "__Host-n", "v", "", "/p", true, None).unwrap_err(),
        [InvalidPrefix]
    );
    assert_eq!(
        sanitized("http://a.example", "", "__Secure-n", "", "", false, None).unwrap_err(),
        [InvalidPrefix]
    );
    assert_eq!(
        sanitized("http://a.example", "", "a=b", "", "", false, None).unwrap_err(),
        [AmbiguousSerialization]
    );
    let partition = partition("https://top.example");
    assert_eq!(
        sanitized(
            "https://a.example",
            "n",
            "v",
            "",
            "",
            false,
            Some(&partition)
        )
        .unwrap_err(),
        [InvalidPartitioned]
    );
    let cookie = sanitized(
        "https://a.example",
        "n",
        "v",
        "",
        "",
        true,
        Some(&partition),
    )
    .unwrap();
    assert_eq!(cookie.partition_key(), Some(&partition));
}
