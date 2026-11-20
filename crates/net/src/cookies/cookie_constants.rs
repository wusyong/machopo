//! The kinds a cookie's attributes come in (Chromium's
//! `net/cookies/cookie_constants.h`).

/// A cookie's `SameSite` attribute (`net::CookieSameSite`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookieSameSite {
    /// None, or one with a value it doesn't know (`Extended`, empty, ...):
    /// treated as `Lax`, but a cookie less than two minutes old goes with
    /// a top-level navigation of an unsafe method too ("Lax-allowing-unsafe").
    Unspecified,
    /// `SameSite=None`: sent cross-site as well; it has to be `Secure`.
    NoRestriction,
    /// `SameSite=Lax`.
    LaxMode,
    /// `SameSite=Strict`.
    StrictMode,
}

impl CookieSameSite {
    /// The mode `value` (a `SameSite` attribute's value) names, ASCII
    /// case-insensitively (`StringToCookieSameSite`).
    pub fn from_attribute_value(value: &str) -> Self {
        if value.eq_ignore_ascii_case("none") {
            Self::NoRestriction
        } else if value.eq_ignore_ascii_case("lax") {
            Self::LaxMode
        } else if value.eq_ignore_ascii_case("strict") {
            Self::StrictMode
        } else {
            Self::Unspecified
        }
    }
}

/// Where a cookie was set from (`net::CookieSourceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookieSourceType {
    /// A response's `Set-Cookie` header.
    Http,
    /// `document.cookie` (or the Cookie Store API).
    Script,
    /// Anything else: the browser itself, automation, DevTools.
    Other,
}

/// Whether the URL that set a cookie was a secure one
/// (`net::CookieSourceScheme`). Only kept, for now: Chromium's scheme-bound
/// cookies (`kEnableSchemeBoundCookies`), which would go by it, are off by
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookieSourceScheme {
    NonSecure,
    /// `https:` or `wss:`, or a cookie with `Secure` (set from a trustworthy
    /// URL, like `http://localhost`).
    Secure,
}

/// A cookie name's prefix, which asks for some attributes
/// (`net::CookiePrefix`): compared ASCII case-insensitively, as Chromium
/// does (`__SECURE-` is `__Secure-`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookiePrefix {
    None,
    /// `__Secure-`: has to be `Secure`, from a secure URL.
    Secure,
    /// `__Host-`: as `__Secure-`, and host-only with `Path=/`.
    Host,
    /// `__Http-`: as `__Secure-`, and `HttpOnly`.
    Http,
    /// `__Host-Http-`: both of `__Host-`'s and `__Http-`'s.
    HostHttp,
}

/// The prefixes, a longer one before one it starts with (`__Host-Http-`
/// before `__Host-`).
const PREFIXES: [(&str, CookiePrefix); 4] = [
    ("__Secure-", CookiePrefix::Secure),
    ("__Host-Http-", CookiePrefix::HostHttp),
    ("__Http-", CookiePrefix::Http),
    ("__Host-", CookiePrefix::Host),
];

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
}

impl CookiePrefix {
    /// The prefix `name` starts with (`cookie_util::GetCookiePrefix`).
    pub(crate) fn of(name: &str) -> Self {
        PREFIXES
            .iter()
            .find(|(prefix, _)| starts_with_ignore_ascii_case(name, prefix))
            .map_or(Self::None, |&(_, prefix)| prefix)
    }

    /// Whether a cookie without a name whose value is `value` would read as
    /// a prefixed one in a `Cookie` header (`HasHiddenPrefixName`): such a
    /// cookie is refused, so it can't pass for one that had to be secure.
    pub(crate) fn is_hidden_in(value: &str) -> bool {
        let value = value.trim_start_matches([' ', '\t']);
        PREFIXES
            .iter()
            .any(|(prefix, _)| starts_with_ignore_ascii_case(value, prefix))
    }
}
