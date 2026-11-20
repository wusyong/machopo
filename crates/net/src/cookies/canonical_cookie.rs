//! A cookie as the store keeps it (Chromium's `net::CanonicalCookie`, with
//! `CookieBase`): its domain and path decided, its expiry a time, and the
//! checks of whether it may be set from a URL and sent to one.

use url::Url;

use super::cookie_constants::{CookiePrefix, CookieSameSite, CookieSourceScheme, CookieSourceType};
use super::cookie_inclusion_status::{CookieInclusionStatus, ExclusionReason};
use super::cookie_options::{
    CookieOptions, LAX_ALLOW_UNSAFE_MAX_AGE_SECONDS, SameSiteCookieContext,
};
use super::cookie_partition_key::CookiePartitionKey;
use super::cookie_util::{self, CookieAccessScheme};
use super::parsed_cookie::ParsedCookie;
use super::time::Time;

/// The longest a cookie is kept for, from when it was made, in days: a
/// later expiry is cut to it (RFC 6265bis's recommended upper limit;
/// Chromium's `ValidateAndAdjustExpiryDate`, without its three hours for an
/// insecure one, `kTimeLimitedInsecureCookies`, off by default).
pub const MAX_EXPIRY_DAYS: i64 = 400;

/// A cookie (`net::CanonicalCookie`): made from a `Set-Cookie` line and the
/// URL that set it with [`create`](Self::create), which checks what the line
/// can be on its own; whether that URL may set it, and which URLs it is
/// sent to, are [`is_set_permitted_in_context`](Self::is_set_permitted_in_context)
/// and [`include_for_request_url`](Self::include_for_request_url), which the
/// store asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalCookie {
    name: String,
    value: String,
    /// The host for a host-only cookie (`example.com`), `.` and the domain
    /// for a domain cookie (`.example.com`).
    domain: String,
    /// Always starts with `/`.
    path: String,
    creation: Time,
    /// `None`: a session cookie.
    expiry: Option<Time>,
    last_access: Time,
    secure: bool,
    http_only: bool,
    same_site: CookieSameSite,
    /// Its partition, if it is `Partitioned` (CHIPS) and was set by a
    /// request that had one: it goes only with requests of that partition,
    /// and is never the same cookie as one of its name, domain and path in
    /// another (or in none).
    partition_key: Option<CookiePartitionKey>,
    source_scheme: CookieSourceScheme,
    /// The port of the URL that set it (see `source_port`).
    source_port: Option<u16>,
    source_type: CookieSourceType,
}

/// A cookie's SameSite as it is applied (`net::CookieEffectiveSameSite`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveSameSite {
    NoRestriction,
    LaxMode,
    StrictMode,
    /// A cookie without `SameSite` less than two minutes old: `Lax`, but
    /// it also goes with a top-level navigation of an unsafe method.
    LaxModeAllowUnsafe,
}

/// Chromium's `base::StringToInt64`: the number, if all of `text` is one in
/// range (an optional `+` or `-`, then digits); else as much of it as was
/// read before the first byte that isn't a digit, or the end of the range
/// it went past.
fn string_to_i64(text: &str) -> (i64, bool) {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() {
        return (0, false);
    }
    let mut number: i64 = 0;
    for c in digits.chars() {
        let Some(digit) = c.to_digit(10) else {
            return (number, false);
        };
        let digit = i64::from(digit);
        let next = number.checked_mul(10).and_then(|n| {
            if negative {
                n.checked_sub(digit)
            } else {
                n.checked_add(digit)
            }
        });
        match next {
            Some(next) => number = next,
            None => return (if negative { i64::MIN } else { i64::MAX }, false),
        }
    }
    (number, true)
}

impl CanonicalCookie {
    /// The cookie `cookie_line` (a `Set-Cookie` header's value, or what
    /// script gave `document.cookie`) sets from `url`, made at
    /// `creation_time` (`CanonicalCookie::Create`); or why it is none.
    /// `server_time` is the response's `Date`, if it had one: an `Expires`
    /// is moved by how far the server's clock is from ours. A `Partitioned`
    /// one is kept in `partition_key`, the request's partition; without
    /// one, it is an unpartitioned cookie (and needn't be `Secure`).
    ///
    /// This checks what the line can be on its own and for `url`: its
    /// domain (host-only, or a domain `url`'s host is in, of its site, not
    /// a public suffix; an IP address host takes no other), its path, its
    /// expiry (`Max-Age` before `Expires`, at most [`MAX_EXPIRY_DAYS`]
    /// ahead), its name's prefix, a partitioned one's `Secure`, and a nameless
    /// cookie whose value would read as a prefixed name or as a name and a
    /// value (`=a=b`: Chromium's `kCookieParseRejectEmptyNameAmbiguous`, on
    /// by default; RFC 6265bis takes it). Whether `url` may set it (`Secure`
    /// from an insecure URL, `HttpOnly` from script, `SameSite`) is
    /// [`is_set_permitted_in_context`](Self::is_set_permitted_in_context)'s.
    /// A cookie whose expiry has passed is made: setting it deletes the one
    /// it replaces.
    ///
    /// Non-ASCII names and values are taken (`kDisallowNonAsciiCookies` is
    /// off by default).
    pub fn create(
        url: &Url,
        cookie_line: &str,
        creation_time: Time,
        server_time: Option<Time>,
        partition_key: Option<&CookiePartitionKey>,
        source_type: CookieSourceType,
    ) -> Result<Self, CookieInclusionStatus> {
        if url.host().is_none() {
            return Err(CookieInclusionStatus::excluded(
                ExclusionReason::FailureToStore,
            ));
        }
        let parsed = ParsedCookie::parse(cookie_line).map_err(CookieInclusionStatus::excluded)?;
        let mut status = CookieInclusionStatus::default();

        if parsed.name().is_empty() && parsed.value().contains('=') {
            status.add_exclusion_reason(ExclusionReason::AmbiguousSerialization);
        }
        let domain =
            cookie_util::cookie_domain_with_string(url, parsed.domain().unwrap_or(""), &mut status);
        if domain.is_none() {
            status.add_exclusion_reason(ExclusionReason::InvalidDomain);
        }
        let path = cookie_util::canon_path_with_string(url, parsed.path().unwrap_or(""));

        let prefix_valid = if parsed.name().is_empty() {
            !CookiePrefix::is_hidden_in(parsed.value())
        } else {
            cookie_util::is_cookie_prefix_valid(CookiePrefix::of(parsed.name()), url, &parsed)
        };
        if !prefix_valid {
            status.add_exclusion_reason(ExclusionReason::InvalidPrefix);
        }
        // `IsCookiePartitionedValid`: a partitioned cookie has to be
        // `Secure`, from a URL that may set a `Secure` cookie.
        let partition_key = partition_key.filter(|_| parsed.is_partitioned());
        if partition_key.is_some()
            && (!parsed.is_secure()
                || cookie_util::provisional_access_scheme(url)
                    == CookieAccessScheme::NonCryptographic)
        {
            status.add_exclusion_reason(ExclusionReason::InvalidPartitioned);
        }
        let (Some(domain), true) = (domain, status.is_include()) else {
            return Err(status);
        };

        // A `Secure` cookie from a trustworthy `http:` URL looks like one
        // from a secure URL, so it isn't stranded if the URL stops being
        // trustworthy.
        let source_scheme = if parsed.is_secure() || cookie_util::is_cryptographic(url) {
            CookieSourceScheme::Secure
        } else {
            CookieSourceScheme::NonSecure
        };
        let source_port = cookie_util::port_adjusted_for_trustworthy_urls(url, parsed.is_secure());
        let expiry =
            Self::parse_expiration(&parsed, creation_time, server_time.unwrap_or(creation_time));
        let expiry = Self::validate_and_adjust_expiry_date(expiry, creation_time);

        Ok(Self {
            name: parsed.name().to_owned(),
            value: parsed.value().to_owned(),
            domain,
            path,
            creation: creation_time,
            expiry,
            last_access: creation_time,
            secure: parsed.is_secure(),
            http_only: parsed.is_http_only(),
            same_site: parsed.same_site(),
            partition_key: partition_key.cloned(),
            source_scheme,
            source_port,
            source_type,
        })
    }

    /// When a cookie parsed as `cookie` made at `current` expires, `None` for
    /// a session cookie (`CanonicalCookie::ParseExpiration`). `Max-Age`
    /// first: zero or less is the earliest time; a number too big is the
    /// end of the range. One that isn't a number is ignored, and so is an
    /// `Expires` that isn't a date. Chromium takes a `+` before the number,
    /// where RFC 6265bis ignores the attribute.
    fn parse_expiration(cookie: &ParsedCookie, current: Time, server_time: Time) -> Option<Time> {
        if let Some(max_age) = cookie.max_age() {
            match string_to_i64(max_age) {
                (seconds, true) if seconds <= 0 => return Some(Time::MIN),
                (seconds, true) => return Some(current.saturating_add_seconds(seconds)),
                // The user agent may clip a date it can't represent (RFC
                // 6265bis says so of `Expires`; Chromium does it here too).
                (i64::MIN, false) => return Some(Time::MIN),
                (i64::MAX, false) => return Some(Time::MAX),
                _ => {}
            }
        }
        let expires = cookie.expires().filter(|expires| !expires.is_empty())?;
        let parsed = cookie_util::parse_cookie_expiration_time(expires)?;
        Some(parsed.saturating_add_micros(current.saturating_micros_since(server_time)))
    }

    /// `expiry`, cut to [`MAX_EXPIRY_DAYS`] after `creation`
    /// (`ValidateAndAdjustExpiryDate`).
    fn validate_and_adjust_expiry_date(expiry: Option<Time>, creation: Time) -> Option<Time> {
        let latest = creation.saturating_add_days(MAX_EXPIRY_DAYS);
        expiry.map(|expiry| expiry.min(latest))
    }

    /// A cookie as the cookie database kept it (`CanonicalCookie::FromStorage`),
    /// if it is one [`create`](Self::create) could have made: a name and
    /// value of a `Set-Cookie` line (not both empty, not too long, no
    /// control character or `;`, a nameless one's value no name), a domain (`.` and a
    /// host, or a host) and a path starting with `/`, an expiry no earlier
    /// than its creation, a `SameSite=None` one `Secure`, a partitioned one
    /// `Secure`.
    #[expect(
        clippy::too_many_arguments,
        reason = "a cookie's fields, as Chromium's FromStorage takes them"
    )]
    pub fn from_storage(
        name: String,
        value: String,
        domain: String,
        path: String,
        creation: Time,
        expiry: Option<Time>,
        last_access: Time,
        secure: bool,
        http_only: bool,
        same_site: CookieSameSite,
        partition_key: Option<CookiePartitionKey>,
        source_scheme: CookieSourceScheme,
        source_port: Option<u16>,
        source_type: CookieSourceType,
    ) -> Option<Self> {
        let canonical = super::parsed_cookie::check_name_value_pair(&name, &value).is_ok()
            && (!name.is_empty() || !value.contains('='))
            && !domain.is_empty()
            && domain != "."
            && path.starts_with('/')
            && expiry.is_none_or(|expiry| expiry >= creation)
            && (same_site != CookieSameSite::NoRestriction || secure)
            && (partition_key.is_none() || secure);
        canonical.then_some(Self {
            name,
            value,
            domain,
            path,
            creation,
            expiry,
            last_access,
            secure,
            http_only,
            same_site,
            partition_key,
            source_scheme,
            source_port,
            source_type,
        })
    }

    /// The name; empty for a nameless cookie.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// The host for a host-only cookie, `.` and the domain for a domain
    /// cookie.
    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// When it was made; for one that replaced a cookie of the same value,
    /// when that one was.
    pub fn creation_date(&self) -> Time {
        self.creation
    }

    /// When it expires; `None` for a session cookie.
    pub fn expiry_date(&self) -> Option<Time> {
        self.expiry
    }

    /// When it was last sent (or made).
    pub fn last_access_date(&self) -> Time {
        self.last_access
    }

    /// Whether it has an expiry (it outlives the session).
    pub fn is_persistent(&self) -> bool {
        self.expiry.is_some()
    }

    /// Whether it had expired by `now`.
    pub fn is_expired(&self, now: Time) -> bool {
        self.expiry.is_some_and(|expiry| now >= expiry)
    }

    /// Whether it has `Secure` (`SecureAttribute`).
    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn is_http_only(&self) -> bool {
        self.http_only
    }

    pub fn same_site(&self) -> CookieSameSite {
        self.same_site
    }

    /// Whether it is kept in a partition (`IsPartitioned`).
    pub fn is_partitioned(&self) -> bool {
        self.partition_key.is_some()
    }

    pub fn partition_key(&self) -> Option<&CookiePartitionKey> {
        self.partition_key.as_ref()
    }

    /// The bytes of its name and value (`NameValueSizeBytes`): what a
    /// partition's domain may have 10 KiB of.
    pub fn name_value_size_bytes(&self) -> usize {
        self.name.len().saturating_add(self.value.len())
    }

    /// Whether it is a domain cookie (its domain starts with `.`), sent to
    /// the domain's subdomains too.
    pub fn is_domain_cookie(&self) -> bool {
        self.domain.starts_with('.')
    }

    /// Whether it is sent only to the host that set it.
    pub fn is_host_cookie(&self) -> bool {
        !self.is_domain_cookie()
    }

    pub fn source_scheme(&self) -> CookieSourceScheme {
        self.source_scheme
    }

    /// The port of the URL that set it (its scheme's default if it had
    /// none; 443 for a `Secure` one from `http:` on 80), if it had one.
    pub fn source_port(&self) -> Option<u16> {
        self.source_port
    }

    pub fn source_type(&self) -> CookieSourceType {
        self.source_type
    }

    /// The domain without its leading dot (`DomainWithoutDot`).
    pub fn domain_without_dot(&self) -> &str {
        self.domain.strip_prefix('.').unwrap_or(&self.domain)
    }

    pub(crate) fn set_creation_date(&mut self, creation: Time) {
        self.creation = creation;
    }

    pub(crate) fn set_last_access_date(&mut self, last_access: Time) {
        self.last_access = last_access;
    }

    /// Whether `other` is the same cookie, one replacing the other
    /// (`IsEquivalent`): the same name, domain and path (and partition).
    pub fn is_equivalent(&self, other: &Self) -> bool {
        self.name == other.name
            && self.domain == other.domain
            && self.path == other.path
            && self.partition_key == other.partition_key
    }

    /// Whether this cookie, set from an insecure URL, would overwrite or
    /// shadow `secure_cookie` (`IsEquivalentForSecureCookieMatching`, RFC
    /// 6265bis's "leave secure cookies alone"): the same name, a domain one
    /// is in the other, and a path `secure_cookie`'s is on. Not symmetric:
    /// a secure `a` on `/login` keeps an insecure `a` off `/login/en` but
    /// not off `/`.
    pub fn is_equivalent_for_secure_cookie_matching(&self, secure_cookie: &Self) -> bool {
        // Chromium doesn't special-case IP addresses here either.
        self.partition_key == secure_cookie.partition_key
            && self.name == secure_cookie.name
            && (cookie_util::is_subdomain_of(
                self.domain_without_dot(),
                secure_cookie.domain_without_dot(),
            ) || cookie_util::is_subdomain_of(
                secure_cookie.domain_without_dot(),
                self.domain_without_dot(),
            ))
            && cookie_util::is_on_path(&secure_cookie.path, &self.path)
    }

    /// Its SameSite as applied at `now` (`GetEffectiveSameSite`, with
    /// Chromium's default, non-legacy, access semantics).
    fn effective_same_site(&self, options: &CookieOptions, now: Time) -> EffectiveSameSite {
        match self.same_site {
            CookieSameSite::Unspecified => {
                let recently_created = options.lax_allow_unsafe
                    && now.saturating_micros_since(self.creation)
                        <= LAX_ALLOW_UNSAFE_MAX_AGE_SECONDS.saturating_mul(1_000_000);
                if recently_created {
                    EffectiveSameSite::LaxModeAllowUnsafe
                } else {
                    EffectiveSameSite::LaxMode
                }
            }
            CookieSameSite::NoRestriction => EffectiveSameSite::NoRestriction,
            CookieSameSite::LaxMode => EffectiveSameSite::LaxMode,
            CookieSameSite::StrictMode => EffectiveSameSite::StrictMode,
        }
    }

    /// Whether `url` may set it, in `options` (`IsSetPermittedInContext`):
    /// a scheme for cookies, a domain that matches `url`'s host, `Secure`
    /// only from a secure or trustworthy URL (`https:`, `wss:`, localhost),
    /// `HttpOnly` only where `options` include them (not script's), a
    /// `Strict` or `Lax` one (or one without `SameSite`) only in a context
    /// at least laxly same-site, and `SameSite=None` only with `Secure`.
    /// Its creation time stands for now: setting is when it was made.
    pub fn is_set_permitted_in_context(
        &self,
        url: &Url,
        options: &CookieOptions,
    ) -> CookieInclusionStatus {
        let mut status = CookieInclusionStatus::default();
        if !cookie_util::is_cookieable_scheme(url) {
            status.add_exclusion_reason(ExclusionReason::NoncookieableScheme);
        }
        if !url
            .host_str()
            .is_some_and(|host| cookie_util::is_domain_match(&self.domain, host))
        {
            status.add_exclusion_reason(ExclusionReason::DomainMismatch);
        }
        if self.secure
            && cookie_util::provisional_access_scheme(url) == CookieAccessScheme::NonCryptographic
        {
            status.add_exclusion_reason(ExclusionReason::SecureOnly);
        }
        if self.http_only && !options.include_httponly {
            status.add_exclusion_reason(ExclusionReason::HttpOnly);
        }
        if self.same_site == CookieSameSite::NoRestriction && !self.secure {
            status.add_exclusion_reason(ExclusionReason::SameSiteNoneInsecure);
        }
        // A `Strict` cookie can be set by a top-level navigation that would
        // get `Lax` ones, intentionally.
        let context = options.same_site_context;
        if context < SameSiteCookieContext::SameSiteLax {
            match self.effective_same_site(options, self.creation) {
                EffectiveSameSite::StrictMode => {
                    status.add_exclusion_reason(ExclusionReason::SameSiteStrict);
                }
                EffectiveSameSite::LaxMode | EffectiveSameSite::LaxModeAllowUnsafe => {
                    status.add_exclusion_reason(if self.same_site == CookieSameSite::Unspecified {
                        ExclusionReason::SameSiteUnspecifiedTreatedAsLax
                    } else {
                        ExclusionReason::SameSiteLax
                    });
                }
                EffectiveSameSite::NoRestriction => {}
            }
        }
        status
    }

    /// Whether it is sent to `url`, in `options`, at `now`
    /// (`IncludeForRequestURL`): `HttpOnly` only where `options` include
    /// them, `Secure` only to a secure or trustworthy URL, to a host its
    /// domain matches and a path on its path, by its SameSite and the
    /// context's, and a `SameSite=None` one only if `Secure`. Expiry is the
    /// store's to check.
    pub fn include_for_request_url(
        &self,
        url: &Url,
        options: &CookieOptions,
        now: Time,
    ) -> CookieInclusionStatus {
        let mut status = CookieInclusionStatus::default();
        if self.http_only && !options.include_httponly {
            status.add_exclusion_reason(ExclusionReason::HttpOnly);
        }
        if self.secure
            && cookie_util::provisional_access_scheme(url) == CookieAccessScheme::NonCryptographic
        {
            status.add_exclusion_reason(ExclusionReason::SecureOnly);
        }
        if !url
            .host_str()
            .is_some_and(|host| cookie_util::is_domain_match(&self.domain, host))
        {
            status.add_exclusion_reason(ExclusionReason::DomainMismatch);
        }
        if !cookie_util::is_on_path(&self.path, url.path()) {
            status.add_exclusion_reason(ExclusionReason::NotOnPath);
        }
        let context = options.same_site_context;
        match self.effective_same_site(options, now) {
            EffectiveSameSite::StrictMode if context < SameSiteCookieContext::SameSiteStrict => {
                status.add_exclusion_reason(ExclusionReason::SameSiteStrict);
            }
            EffectiveSameSite::LaxMode if context < SameSiteCookieContext::SameSiteLax => {
                status.add_exclusion_reason(if self.same_site == CookieSameSite::Unspecified {
                    ExclusionReason::SameSiteUnspecifiedTreatedAsLax
                } else {
                    ExclusionReason::SameSiteLax
                });
            }
            EffectiveSameSite::LaxModeAllowUnsafe
                if context < SameSiteCookieContext::SameSiteLaxMethodUnsafe =>
            {
                status.add_exclusion_reason(ExclusionReason::SameSiteUnspecifiedTreatedAsLax);
            }
            _ => {}
        }
        // One set before SameSite=None had to be Secure (none here, but
        // DevTools and automation can make one).
        if self.same_site == CookieSameSite::NoRestriction && !self.secure {
            status.add_exclusion_reason(ExclusionReason::SameSiteNoneInsecure);
        }
        status
    }

    /// The `Cookie` header's value for `cookies`, in their order
    /// (`CanonicalCookie::BuildCookieLine`): `name=value` pairs joined by
    /// `; `, a nameless cookie as its value alone (as it was set).
    pub fn build_cookie_line<'a>(cookies: impl IntoIterator<Item = &'a CanonicalCookie>) -> String {
        let mut line = String::new();
        for cookie in cookies {
            if !line.is_empty() {
                line.push_str("; ");
            }
            if !cookie.name.is_empty() {
                line.push_str(&cookie.name);
                line.push('=');
            }
            line.push_str(&cookie.value);
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_to_i64_as_chromium() {
        assert_eq!(string_to_i64("42"), (42, true));
        assert_eq!(string_to_i64("+42"), (42, true));
        assert_eq!(string_to_i64("-42"), (-42, true));
        assert_eq!(string_to_i64(""), (0, false));
        assert_eq!(string_to_i64("-"), (0, false));
        assert_eq!(string_to_i64("4a2"), (4, false));
        assert_eq!(string_to_i64("9223372036854775807"), (i64::MAX, true));
        assert_eq!(string_to_i64("-9223372036854775808"), (i64::MIN, true));
        assert_eq!(string_to_i64("9223372036854775808"), (i64::MAX, false));
        assert_eq!(string_to_i64("-9223372036854775809"), (i64::MIN, false));
        // What was read before a non-digit stays: exactly the end of the
        // range reads as past it.
        assert_eq!(string_to_i64("9223372036854775807x"), (i64::MAX, false));
    }
}
