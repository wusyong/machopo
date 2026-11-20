//! The network context's cookies as its loads use them (Chromium's
//! `URLRequestHttpJob`'s cookie steps, `AddCookieHeaderAndStart` and
//! `SaveCookiesAndNotifyHeadersComplete`, with its `CookieSettings`): which
//! of the jar's cookies a hop of a request sends, and which `Set-Cookie`s of
//! its response are kept, by the request's trusted params (the browser's),
//! the `SameSite` context they make, and the cookie policy.

use std::sync::{Arc, Mutex, PoisonError};

use protocol::network::TrustedParams;
use site::{Site, SiteForCookies};
use url::Url;

use super::canonical_cookie::CanonicalCookie;
use super::cookie_constants::CookieSourceType;
use super::cookie_monster::CookieMonster;
use super::cookie_options::{CookieOptions, SameSiteCookieContext};
use super::cookie_util::parse_cookie_expiration_time;
use super::time::Time;

/// The network context's cookie jar, and its policy. Clones share the jar:
/// the loads are tasks of the network service's one thread, which a lock
/// lets share it (tokio's tasks are `Send`; it is never held across an
/// `await`, so never contended).
#[derive(Clone)]
pub(crate) struct CookieJar {
    monster: Arc<Mutex<CookieMonster>>,
    /// Chromium's cookie policy (`--allow-third-party-cookies`): a third
    /// party's cookies are sent and kept, and a cookie without `SameSite`
    /// is Lax-allowing-unsafe for two minutes. Without it, machopo's own: a
    /// third party's are neither, and `Lax` is `Lax` (the strict end RFC
    /// 6265bis permits, and recommends for third parties).
    allow_third_party_cookies: bool,
}

/// The `SameSite` context of a hop of a request to `url`, of `method`
/// (`cookie_util::ComputeSameSiteContextForRequest`, schemeful, without the
/// redirect chain: `kCookieSameSiteConsidersRedirectChain` is off): strict
/// if `url` is first party to the site for cookies and the initiator is of
/// its site too (or there is none: the browser's own navigation); a main
/// frame's navigation from another site, lax (laxly unsafe for an unsafe
/// method); anything else cross-site.
fn context_for_request(
    url: &Url,
    site_for_cookies: &SiteForCookies,
    trusted: &TrustedParams,
    method: &str,
) -> SameSiteCookieContext {
    if !site_for_cookies.is_first_party(url) {
        return SameSiteCookieContext::CrossSite;
    }
    if same_site_initiator(url, trusted) {
        return SameSiteCookieContext::SameSiteStrict;
    }
    if !trusted.is_main_frame_navigation {
        return SameSiteCookieContext::CrossSite;
    }
    if matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE") {
        SameSiteCookieContext::SameSiteLax
    } else {
        SameSiteCookieContext::SameSiteLaxMethodUnsafe
    }
}

/// The `SameSite` context the response of a hop to `url` sets cookies in
/// (`ComputeSameSiteContextForResponse`): a main frame's navigation sets
/// any; else the request's, strict counted as lax (setting any cookie needs
/// lax at most), whatever the method.
fn context_for_response(
    url: &Url,
    site_for_cookies: &SiteForCookies,
    trusted: &TrustedParams,
) -> SameSiteCookieContext {
    if trusted.is_main_frame_navigation && !site_for_cookies.is_null() {
        return SameSiteCookieContext::SameSiteLax;
    }
    match context_for_request(url, site_for_cookies, trusted, "GET") {
        SameSiteCookieContext::SameSiteStrict => SameSiteCookieContext::SameSiteLax,
        context => context,
    }
}

/// Whether the request's initiator is of `url`'s site, scheme too; true
/// without one (`SiteForCookies::FromOrigin(initiator)
/// .IsFirstPartyWithSchemefulMode(url)`). An opaque one is of no site.
fn same_site_initiator(url: &Url, trusted: &TrustedParams) -> bool {
    let Some(initiator) = &trusted.initiator else {
        return true;
    };
    Url::parse(initiator).is_ok_and(|initiator| {
        SiteForCookies::of_site(Site::of(&initiator.origin())).is_first_party(url)
    })
}

/// The site a hop to `url` is first party to: a main frame navigation's,
/// its URL's at each hop (Chromium's `RedirectInfo::new_site_for_cookies`);
/// another request's, the browser's, as it was.
fn site_for_cookies_at(url: &Url, trusted: &TrustedParams) -> SiteForCookies {
    if trusted.is_main_frame_navigation {
        return SiteForCookies::of_site(Site::of_url(url));
    }
    trusted
        .site_for_cookies
        .as_deref()
        .and_then(|site| Url::parse(site).ok())
        .map_or_else(SiteForCookies::null, |site| {
            SiteForCookies::of_site(Site::of_url(&site))
        })
}

impl CookieJar {
    /// An empty jar, of Chromium's cookie policy if
    /// `allow_third_party_cookies`, else of machopo's.
    pub(crate) fn new(allow_third_party_cookies: bool) -> Self {
        Self {
            monster: Arc::default(),
            allow_third_party_cookies,
        }
    }

    fn monster(&self) -> std::sync::MutexGuard<'_, CookieMonster> {
        self.monster.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the policy lets a hop to `url`, first party to
    /// `site_for_cookies`, have cookies (`CookieSettings::IsCookieAccessible`):
    /// a first party's (schemeless, as Chromium's third-party check is)
    /// always; a third party's only with Chromium's policy. `Partitioned`
    /// cookies are a third party's too until they are kept by their
    /// top-level site: sent across sites, they would be the unpartitioned
    /// cookies the policy keeps from third parties.
    fn accessible(&self, url: &Url, site_for_cookies: &SiteForCookies) -> bool {
        self.allow_third_party_cookies || site_for_cookies.is_first_party_schemeless(url)
    }

    /// The options cookies of a hop to `url` are read or written in.
    fn options(&self, context: SameSiteCookieContext) -> CookieOptions {
        CookieOptions {
            include_httponly: true,
            same_site_context: context,
            lax_allow_unsafe: self.allow_third_party_cookies,
        }
    }

    /// The `Cookie` header's value for a hop of a request to `url`, of
    /// `method`, from where `trusted` says (none for a request without
    /// them: the browser's own, which carry no cookies), if it sends any.
    pub(crate) fn cookie_header(
        &self,
        url: &Url,
        trusted: Option<&TrustedParams>,
        method: &str,
    ) -> Option<String> {
        let trusted = trusted?;
        let site_for_cookies = site_for_cookies_at(url, trusted);
        if !self.accessible(url, &site_for_cookies) {
            return None;
        }
        let context = context_for_request(url, &site_for_cookies, trusted, method);
        self.monster()
            .cookie_line_for(url, &self.options(context), Time::now())
    }

    /// Keeps the cookies the `Set-Cookie` headers of the response of a hop
    /// to `url` set (`headers`: the response's, names lowercase), from
    /// where `trusted` says: none for a request without them, or that the
    /// policy keeps from cookies. An `Expires` is moved by how far the
    /// response's `Date` is from now.
    pub(crate) fn set_from_response(
        &self,
        url: &Url,
        trusted: Option<&TrustedParams>,
        headers: &[(String, String)],
    ) {
        let Some(trusted) = trusted else {
            return;
        };
        let site_for_cookies = site_for_cookies_at(url, trusted);
        if !self.accessible(url, &site_for_cookies) {
            return;
        }
        let options = self.options(context_for_response(url, &site_for_cookies, trusted));
        let server_time = headers
            .iter()
            .find(|(name, _)| name == "date")
            .and_then(|(_, date)| parse_cookie_expiration_time(date));
        let now = Time::now();
        let mut monster = self.monster();
        for (_, line) in headers.iter().filter(|(name, _)| name == "set-cookie") {
            let cookie =
                CanonicalCookie::create(url, line, now, server_time, CookieSourceType::Http);
            if let Ok(cookie) = cookie {
                // A cookie the context or the jar refuses is dropped, as
                // Chromium's (it tells DevTools why; machopo doesn't yet).
                let _ = monster.set_canonical_cookie(cookie, url, &options);
            }
        }
    }
}
