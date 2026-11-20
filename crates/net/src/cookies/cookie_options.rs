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
    /// with an unsafe method (`POST`): `None` cookies alone, as without
    /// Lax-allowing-unsafe (Chromium's lets cookies without `SameSite` less
    /// than two minutes old through too).
    SameSiteLaxMethodUnsafe,
    /// `SAME_SITE_LAX`: a top-level navigation to the site from another one,
    /// with a safe method: `Lax` cookies too. Enough to set any cookie.
    SameSiteLax,
    /// `SAME_SITE_STRICT`: of the site, from the site: every cookie.
    SameSiteStrict,
}

/// What a cookie is read or written in (`net::CookieOptions`, the part
/// machopo has). The default is script's, cross-site. There is no
/// Lax-allowing-unsafe (Chromium's two minutes in which a cookie without
/// `SameSite` goes with a top-level navigation of any method): `Lax` is
/// `Lax`, the strict end RFC 6265bis permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CookieOptions {
    /// Whether `HttpOnly` cookies are read and written: HTTP's, not script's
    /// (the opposite of `exclude_httponly`).
    pub include_httponly: bool,
    pub same_site_context: SameSiteCookieContext,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            include_httponly: false,
            same_site_context: SameSiteCookieContext::CrossSite,
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
        }
    }
}
