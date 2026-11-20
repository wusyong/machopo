//! What a cookie is read or written in (Chromium's `net::CookieOptions`).

/// How same-site the request or document a cookie is read or written for
/// is (`CookieOptions::SameSiteCookieContext::ContextType`), from the least
/// trusted to the most: the order compares them. Computing it (from the
/// request's URL chain, initiator, site for cookies and method) is the
/// caller's (`cookie_util::ComputeSameSiteContextFor*`). Chromium keeps a
/// schemeless and a schemeful one and goes by the schemeful one (Schemeful
/// Same-Site is on by default): this is that one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SameSiteCookieContext {
    /// `CROSS_SITE`: only `SameSite=None` cookies.
    CrossSite,
    /// `SAME_SITE_LAX_METHOD_UNSAFE`: a top-level navigation to the site
    /// with an unsafe method (`POST`): only cookies without `SameSite` less
    /// than two minutes old, besides `None` ones ("Lax-allowing-unsafe").
    SameSiteLaxMethodUnsafe,
    /// `SAME_SITE_LAX`: a top-level navigation to the site from another one,
    /// with a safe method: `Lax` cookies too. Enough to set any cookie.
    SameSiteLax,
    /// `SAME_SITE_STRICT`: of the site, from the site: every cookie.
    SameSiteStrict,
}

/// What a cookie is read or written in (`net::CookieOptions`, the part
/// machopo has). The default is script's, cross-site, without
/// Lax-allowing-unsafe: machopo's default cookie policy, the strict end
/// RFC 6265bis permits (Chromium's has it; machopo's has it only when
/// third-party cookies are let through, as all of Chromium's defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CookieOptions {
    /// Whether `HttpOnly` cookies are read and written: HTTP's, not script's
    /// (the opposite of `exclude_httponly`).
    pub include_httponly: bool,
    pub same_site_context: SameSiteCookieContext,
    /// Whether a cookie without `SameSite` goes with a top-level navigation
    /// of an unsafe method while it is less than
    /// [`LAX_ALLOW_UNSAFE_MAX_AGE_SECONDS`] old ("Lax-allowing-unsafe", RFC
    /// 6265bis's optional compatibility mode): yes in Chromium, unless
    /// `kSameSiteDefaultChecksMethodRigorously` is on (off by default); in
    /// machopo, only with Chromium's cookie policy.
    pub lax_allow_unsafe: bool,
}

/// How long a cookie without `SameSite` is "Lax-allowing-unsafe" for, in
/// seconds (`kLaxAllowUnsafeMaxAge`, two minutes).
pub const LAX_ALLOW_UNSAFE_MAX_AGE_SECONDS: i64 = 120;

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            include_httponly: false,
            same_site_context: SameSiteCookieContext::CrossSite,
            lax_allow_unsafe: false,
        }
    }
}

impl CookieOptions {
    /// Options that let every cookie through (`MakeAllInclusive`): the
    /// browser's own reads and writes, automation's, DevTools'.
    pub fn make_all_inclusive() -> Self {
        Self {
            include_httponly: true,
            same_site_context: SameSiteCookieContext::SameSiteStrict,
            lax_allow_unsafe: true,
        }
    }
}
