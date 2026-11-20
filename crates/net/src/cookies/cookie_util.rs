//! The rules cookies are set and sent by that aren't one cookie's (Chromium's
//! `net::cookie_util`): an `Expires` date, a cookie's domain and default
//! path, domain- and path-matching, and how secure a URL is for cookies.

use url::{Host, Url};

use super::cookie_constants::CookiePrefix;
use super::cookie_inclusion_status::{CookieInclusionStatus, ExclusionReason};
use super::parsed_cookie::ParsedCookie;
use super::time::Time;

/// What separates the parts of a cookie's date (RFC 6265bis's `delimiter`,
/// which is Chromium's set too): everything ASCII but letters, digits and
/// `:`, and the tab; not the other control characters or non-ASCII.
fn is_date_delimiter(c: char) -> bool {
    c == '\t' || (c.is_ascii_punctuation() && c != ':') || c == ' '
}

/// `base::StringToUint` and `base::StringToInt` of a token that starts with
/// a digit: all of it digits, and in range.
fn parse_digits(token: &str) -> Option<u32> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

/// The time an `Expires` attribute's value names, if it names one
/// (`cookie_util::ParseCookieExpirationTime`): RFC 6265bis's date algorithm,
/// as Chromium has it. The value is split at delimiters into tokens; the
/// first that starts with a letter and begins with a month's first three
/// letters (`jan`, `June`) is the month; the first with a `:` is the time,
/// if it is exactly three fields of digits (`hh:mm:ss`); the first other
/// one of one or two digits is the day of the month, and the first after
/// that of up to five digits the year. A year of 70 to 99 is 19xx, one of
/// 0 to 69 20xx. Anything else is ignored (a time zone, a weekday).
///
/// Where Chromium and the draft differ, Chromium's: a day or a year has to
/// be all digits (the draft's `15th` is a day there), a time field may have
/// any number of digits, a year may have one or five, and a year before
/// 1601 is a date (in the past) where the draft fails it.
pub fn parse_cookie_expiration_time(time_string: &str) -> Option<Time> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];

    let mut day_of_month = None;
    let mut month = None;
    let mut time = None;
    let mut year = None;
    for token in time_string
        .split(is_date_delimiter)
        .filter(|token| !token.is_empty())
    {
        let numerical = token.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if !numerical {
            if month.is_none() {
                let lower = token.get(..3).map(str::to_ascii_lowercase);
                month = MONTHS
                    .iter()
                    .zip(1..)
                    .find(|(name, _)| lower.as_deref() == Some(**name))
                    .map(|(_, number)| number);
            }
            // A second word is the time zone (or the weekday): ignored, as
            // Chromium has no reason to go by one.
        } else if token.contains(':') {
            if time.is_none() {
                let mut fields = token.split(':');
                time = match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(hour), Some(minute), Some(second), None) => {
                        match (
                            parse_digits(hour),
                            parse_digits(minute),
                            parse_digits(second),
                        ) {
                            (Some(hour), Some(minute), Some(second)) => {
                                Some((hour, minute, second))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
            }
        } else if day_of_month.is_none() && token.len() <= 2 {
            day_of_month = parse_digits(token);
        } else if year.is_none() && token.len() <= 5 {
            year = parse_digits(token);
        }
    }

    let (Some(day_of_month), Some(month), Some((hour, minute, second)), Some(year)) =
        (day_of_month, month, time, year)
    else {
        return None;
    };
    // Two-digit years: a year has at most five digits, so these can't
    // overflow.
    let year = match year {
        70..=99 => year.saturating_add(1900),
        0..=69 => year.saturating_add(2000),
        _ => year,
    };
    Time::from_utc(year, month, day_of_month, hour, minute, second)
}

/// How secure a URL is for cookies (`net::CookieAccessScheme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookieAccessScheme {
    /// `http:` and `ws:` but to localhost.
    NonCryptographic,
    /// `http:` or `ws:` to localhost: potentially trustworthy, so it may set
    /// and get `Secure` cookies.
    Trustworthy,
    /// `https:` and `wss:`.
    Cryptographic,
}

/// `GURL::SchemeIsCryptographic`.
pub(crate) fn is_cryptographic(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "wss")
}

/// Whether `url` is to this machine (`net::IsLocalhost`): a loopback
/// address, `localhost`, or a name under `localhost.` (a last dot or not).
pub(crate) fn is_localhost(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(name)) => {
            let name = name.strip_suffix('.').unwrap_or(name);
            name.eq_ignore_ascii_case("localhost")
                || name
                    .len()
                    .checked_sub(".localhost".len())
                    .and_then(|start| name.get(start..))
                    .is_some_and(|end| end.eq_ignore_ascii_case(".localhost"))
        }
        None => false,
    }
}

/// `cookie_util::ProvisionalAccessScheme`: without a `CookieAccessDelegate`,
/// which in Chromium can make more URLs trustworthy (enterprise policy).
pub(crate) fn provisional_access_scheme(url: &Url) -> CookieAccessScheme {
    if is_cryptographic(url) {
        CookieAccessScheme::Cryptographic
    } else if is_localhost(url) {
        CookieAccessScheme::Trustworthy
    } else {
        CookieAccessScheme::NonCryptographic
    }
}

/// The schemes cookies are for (`CookieMonster::kDefaultCookieableSchemes`;
/// `file:` is one only if the embedder asks).
pub(crate) fn is_cookieable_scheme(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "ws" | "wss")
}

/// `name`'s registrable domain, for a name a cookie's domain or a URL's
/// host is: none for an IP address (which isn't one's), a public suffix or
/// a single label. A leading dot is skipped, as Chromium's
/// `GetDomainAndRegistry` does.
pub(crate) fn registrable_domain_of(name: &str) -> Option<&str> {
    let name = name.strip_prefix('.').unwrap_or(name);
    if name.starts_with('[') || name.parse::<std::net::Ipv4Addr>().is_ok() {
        return None;
    }
    site::registrable_domain(&Host::Domain(name))
}

/// `cookie_util::GetEffectiveDomain`: the registrable domain for a web
/// scheme, else the host itself.
fn effective_domain<'a>(scheme: &str, host: &'a str) -> Option<&'a str> {
    if matches!(scheme, "http" | "https" | "ws" | "wss") {
        registrable_domain_of(host)
    } else {
        Some(host.strip_prefix('.').unwrap_or(host))
    }
}

/// The domain a cookie set from `url` with a `Domain` attribute of
/// `domain_string` (empty without one) is for
/// (`cookie_util::GetCookieDomainWithString`): `url`'s host for a
/// host-only cookie, or `.` and the attribute's domain, canonicalized, for
/// a domain cookie. None if it can't be set there: a domain the host isn't
/// in, a public suffix (unless it is the host: that is a host-only
/// cookie), another site, or any domain for an IP address but itself.
///
/// A non-ASCII `Domain` is canonicalized to punycode: Chromium only warns
/// of one (`kCookieDomainRejectNonASCII` is off by default).
pub(crate) fn cookie_domain_with_string(
    url: &Url,
    domain_string: &str,
    status: &mut CookieInclusionStatus,
) -> Option<String> {
    let host = url.host()?;
    let url_host = url.host_str()?;
    // A name can't end in an empty label but its last.
    if url_host.ends_with("..") {
        return None;
    }
    let is_host_ip = !matches!(host, Host::Domain(_));
    let without_dot = domain_string.strip_prefix('.').unwrap_or(domain_string);
    // Domain=IPADDR (a leading dot or not) is a host cookie, as in other
    // browsers.
    if domain_string.is_empty() || (is_host_ip && url_host.eq_ignore_ascii_case(without_dot)) {
        return Some(url_host.to_owned());
    }
    if is_host_ip {
        return None;
    }
    // No %-escapes: the URL parser would undo them.
    if domain_string.contains('%') {
        return None;
    }
    // `CanonicalizeHost` keeps a leading dot; the URL parser doesn't take
    // one, so it goes and comes back.
    let canonical = match Host::parse(without_dot).ok()? {
        Host::Domain(name) => name,
        address => address.to_string(),
    };
    if canonical.is_empty() {
        return None;
    }
    let cookie_domain = format!(".{canonical}");

    let Some(url_domain_and_registry) = effective_domain(url.scheme(), url_host) else {
        // An intranet host, a public suffix: no domain cookie, but a
        // `Domain` that is the host itself is a host cookie, as in other
        // browsers.
        let normalized = without_dot.to_ascii_lowercase();
        return (url_host == normalized).then_some(normalized);
    };
    if effective_domain(url.scheme(), &cookie_domain) != Some(url_domain_and_registry) {
        status.add_exclusion_reason(ExclusionReason::DomainMismatch);
        return None;
    }
    // The host is the domain or under it: of the same registrable domain,
    // that is a suffix check.
    let under = if url_host.len() < cookie_domain.len() {
        cookie_domain.strip_prefix('.') == Some(url_host)
    } else {
        url_host.ends_with(&cookie_domain)
    };
    under.then_some(cookie_domain)
}

/// The path a cookie with a `Path` attribute of `path_string` (empty
/// without one) set from `url` is for (`cookie_util::CanonPathWithString`):
/// the attribute if it starts with `/`, else RFC 6265bis's default-path,
/// the URL's path up to (not with) its last `/`, or `/` if that is its
/// first.
pub(crate) fn canon_path_with_string(url: &Url, path_string: &str) -> String {
    if path_string.starts_with('/') {
        return path_string.to_owned();
    }
    let url_path = url.path();
    match url_path.rfind('/') {
        None | Some(0) => "/".to_owned(),
        Some(last) => url_path.get(..last).unwrap_or("/").to_owned(),
    }
}

/// Whether a cookie for `domain` goes to `host` (`cookie_util::IsDomainMatch`):
/// a host cookie to its host only, a domain cookie (`.example.com`) to that
/// domain and the ones under it.
pub(crate) fn is_domain_match(domain: &str, host: &str) -> bool {
    if host == domain {
        return true;
    }
    let Some(without_dot) = domain.strip_prefix('.') else {
        return false;
    };
    host == without_dot || (host.len() > domain.len() && host.ends_with(domain))
}

/// Whether a cookie for `cookie_path` goes to a URL whose path is
/// `url_path` (`cookie_util::IsOnPath`, RFC 6265bis's path-match): the
/// same, or under it at a `/` (`/a` is on `/a/b`, not on `/ab`).
pub(crate) fn is_on_path(cookie_path: &str, url_path: &str) -> bool {
    if cookie_path.is_empty() {
        return false;
    }
    let Some(after) = url_path.strip_prefix(cookie_path) else {
        return false;
    };
    after.is_empty() || cookie_path.ends_with('/') || after.starts_with('/')
}

/// Whether `subdomain` is `superdomain` or under it at a label (Chromium's
/// `IsSubdomainOf`), neither with a leading dot.
pub(crate) fn is_subdomain_of(subdomain: &str, superdomain: &str) -> bool {
    if subdomain.len() <= superdomain.len() {
        return subdomain == superdomain;
    }
    subdomain
        .strip_suffix(superdomain)
        .is_some_and(|start| start.ends_with('.'))
}

/// Whether a cookie set from `url` has what its name's prefix asks for
/// (`cookie_util::IsCookiePrefixValid`, given the URL): `Secure` from a
/// secure (or trustworthy) URL for each; for `__Host-`, `Path=/` and no
/// `Domain` attribute (one equal to an IP address host is no domain); for
/// `__Http-`, `HttpOnly`.
pub(crate) fn is_cookie_prefix_valid(
    prefix: CookiePrefix,
    url: &Url,
    cookie: &ParsedCookie,
) -> bool {
    is_prefix_valid_for(
        prefix,
        url,
        cookie.is_secure(),
        cookie.is_http_only(),
        cookie.domain().unwrap_or(""),
        cookie.path(),
    )
}

/// [`is_cookie_prefix_valid`] of a cookie's attributes as they are given
/// (`IsCookiePrefixValid` with them, `CreateSanitizedCookie`'s): `domain`
/// empty without one, `path` none without one.
pub(crate) fn is_prefix_valid_for(
    prefix: CookiePrefix,
    url: &Url,
    secure: bool,
    http_only: bool,
    domain: &str,
    path: Option<&str>,
) -> bool {
    let secure =
        || secure && provisional_access_scheme(url) != CookieAccessScheme::NonCryptographic;
    let host = || {
        secure()
            && path == Some("/")
            && (domain.is_empty()
                || (!matches!(url.host(), Some(Host::Domain(_)) | None)
                    && url.host_str() == Some(domain)))
    };
    let http = || secure() && http_only;
    match prefix {
        CookiePrefix::None => true,
        CookiePrefix::Secure => secure(),
        CookiePrefix::Host => host(),
        CookiePrefix::Http => http(),
        CookiePrefix::HostHttp => http() && host(),
    }
}

/// `CanonicalCookie::GetAndAdjustPortForTrustworthyUrls`: `url`'s port, or
/// its scheme's default; but 443 for a `Secure` cookie from `http:` or `ws:`
/// on 80 (a trustworthy URL), so it looks like one from a secure URL.
pub(crate) fn port_adjusted_for_trustworthy_urls(url: &Url, secure: bool) -> Option<u16> {
    let port = url.port_or_known_default();
    if secure && !is_cryptographic(url) && matches!(url.scheme(), "http" | "ws") && port == Some(80)
    {
        return Some(443);
    }
    port
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiters_are_the_drafts() {
        let delimiters: String = (0u8..=0x7f)
            .map(char::from)
            .filter(|&c| is_date_delimiter(c))
            .collect();
        assert_eq!(delimiters, "\t !\"#$%&'()*+,-./;<=>?@[\\]^_`{|}~");
    }

    #[test]
    fn paths() {
        assert!(is_on_path("/", "/anything"));
        assert!(is_on_path("/a", "/a"));
        assert!(is_on_path("/a", "/a/b"));
        assert!(is_on_path("/a/", "/a/b"));
        assert!(!is_on_path("/a", "/ab"));
        assert!(!is_on_path("/a/b", "/a"));
        assert!(!is_on_path("", "/"));
    }

    #[test]
    fn domains() {
        assert!(is_domain_match("example.com", "example.com"));
        assert!(!is_domain_match("example.com", "www.example.com"));
        assert!(is_domain_match(".example.com", "example.com"));
        assert!(is_domain_match(".example.com", "a.b.example.com"));
        assert!(!is_domain_match(".example.com", "badexample.com"));
        assert!(is_subdomain_of("a.example.com", "example.com"));
        assert!(is_subdomain_of("example.com", "example.com"));
        assert!(!is_subdomain_of("aexample.com", "example.com"));
        assert!(!is_subdomain_of("example.com", "a.example.com"));
    }
}
