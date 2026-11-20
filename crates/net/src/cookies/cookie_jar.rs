//! The network context's cookies as its loads use them (Chromium's
//! `URLRequestHttpJob`'s cookie steps, `AddCookieHeaderAndStart` and
//! `SaveCookiesAndNotifyHeadersComplete`, with its `CookieSettings`): which
//! of the jar's cookies a hop of a request sends, and which `Set-Cookie`s of
//! its response are kept, by the request's trusted params (the browser's),
//! the `SameSite` context and the partition they make, and the cookie
//! policy. The jar's cookies come from the profile's database first: a
//! hop waits for them.

use std::sync::{Arc, Mutex, PoisonError};

use protocol::network::TrustedParams;
use site::{Site, SiteForCookies};
use url::Url;

use super::canonical_cookie::CanonicalCookie;
use super::cookie_constants::CookieSourceType;
use super::cookie_monster::CookieMonster;
use super::cookie_monster::PersistentCookieStore;
use super::cookie_options::{CookieOptions, SameSiteCookieContext};
use super::cookie_partition_key::CookiePartitionKey;
use super::cookie_util::parse_cookie_expiration_time;
use super::sqlite_persistent_cookie_store::Loaded;
use super::time::Time;

/// The network context's cookie jar, and its policy. Clones share the jar:
/// the loads are tasks of the network service's one thread, which a lock
/// lets share it (tokio's tasks are `Send`; it is never held across an
/// `await`, so never contended).
#[derive(Clone)]
pub(crate) struct CookieJar {
    monster: Arc<Mutex<CookieMonster>>,
    /// Set once the database's cookies are in the jar (the first wait,
    /// without one); `loading` has them until then.
    loaded: Arc<tokio::sync::OnceCell<()>>,
    loading: Arc<Mutex<Option<Loaded>>>,
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

/// The `SameSite` context script reads and writes the cookies of `url` in
/// (`ComputeSameSiteContextForScriptGet`, without an initiator, and
/// `ForScriptSet`): strict if `url` is first party to the site for cookies
/// (setting needs lax, which strict is too), else cross-site.
fn context_for_script(url: &Url, site_for_cookies: &SiteForCookies) -> SameSiteCookieContext {
    if site_for_cookies.is_first_party(url) {
        SameSiteCookieContext::SameSiteStrict
    } else {
        SameSiteCookieContext::CrossSite
    }
}

/// A call of script's on the jar (`RestrictedCookieManager`'s, with the
/// script role), from where `trusted` (a document's loader's) says.
pub(crate) enum ScriptCookies {
    /// `document.cookie`'s getter: the cookie string, to `done`.
    Get {
        url: Url,
        trusted: TrustedParams,
        done: Box<dyn FnOnce(String) + Send>,
    },
    /// `document.cookie`'s setter.
    Set {
        url: Url,
        trusted: TrustedParams,
        line: String,
    },
    /// `navigator.cookieEnabled`.
    Enabled {
        url: Url,
        trusted: TrustedParams,
        done: Box<dyn FnOnce(bool) + Send>,
    },
}

/// Which partitions a deletion takes cookies of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartitionFilter {
    /// Unpartitioned and partitioned ones alike.
    Any,
    /// Unpartitioned ones alone.
    Unpartitioned,
    /// Those of this partition alone.
    Of(CookiePartitionKey),
}

/// Which cookies a deletion takes (`network::mojom::CookieDeletionFilter`,
/// as DevTools fills it): those of every given part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieDeletionFilter {
    pub name: Option<String>,
    /// The cookie's domain exactly (`.` and the domain for a domain
    /// cookie).
    pub domain: Option<String>,
    pub path: Option<String>,
    pub partition: PartitionFilter,
}

impl CookieDeletionFilter {
    /// Every cookie (`ClearBrowserCookies`'s empty filter).
    pub fn all() -> Self {
        Self {
            name: None,
            domain: None,
            path: None,
            partition: PartitionFilter::Any,
        }
    }

    fn matches(&self, cookie: &CanonicalCookie) -> bool {
        self.name
            .as_deref()
            .is_none_or(|name| cookie.name() == name)
            && self
                .domain
                .as_deref()
                .is_none_or(|domain| cookie.domain() == domain)
            && self
                .path
                .as_deref()
                .is_none_or(|path| cookie.path() == path)
            && match &self.partition {
                PartitionFilter::Any => true,
                PartitionFilter::Unpartitioned => !cookie.is_partitioned(),
                PartitionFilter::Of(key) => cookie.partition_key() == Some(key),
            }
    }
}

/// A call of the browser's on the jar (`network::mojom::CookieManager`'s,
/// for DevTools and automation): every cookie of any context, `HttpOnly`
/// ones too.
pub(crate) enum CookieManagerCall {
    /// The cookies each of `urls` could get, in the partition of a frame of
    /// it in a top-level frame at `top_frame` (if any) too
    /// (`GetCookieList`, as DevTools' `CookieRetrieverNetworkService`):
    /// each once.
    GetList {
        urls: Vec<Url>,
        top_frame: Option<Url>,
        done: Box<dyn FnOnce(Vec<CanonicalCookie>) + Send>,
    },
    /// Every cookie (`GetAllCookies`).
    GetAll {
        done: Box<dyn FnOnce(Vec<CanonicalCookie>) + Send>,
    },
    /// Sets each, as from its own `https:` URL (`SetCanonicalCookie` from
    /// `cookie_util::SimulatedCookieSource`), in any context: whether each
    /// was.
    Set {
        cookies: Vec<CanonicalCookie>,
        done: Box<dyn FnOnce(Vec<bool>) + Send>,
    },
    /// Deletes those `filter` takes (`DeleteCookies`): how many.
    Delete {
        filter: CookieDeletionFilter,
        done: Box<dyn FnOnce(usize) + Send>,
    },
}

/// Whether the cookie policy lets a hop to `url`, first party to
/// `site_for_cookies`, have `cookie` (`CookieSettings::IsCookieAccessible`
/// with third-party cookies blocked): a first party's (schemeless, as
/// Chromium's third-party check is) always; a third party's only if it is
/// partitioned (it goes only with requests of the top-level site it was
/// set in). Chromium lets a third party's through by default; machopo
/// doesn't, as Safari doesn't.
fn accessible(cookie: &CanonicalCookie, url: &Url, site_for_cookies: &SiteForCookies) -> bool {
    cookie.is_partitioned() || site_for_cookies.is_first_party_schemeless(url)
}

/// The options cookies of a hop are read or written in.
fn options(context: SameSiteCookieContext) -> CookieOptions {
    CookieOptions {
        include_httponly: true,
        same_site_context: context,
    }
}

/// The site of the top-level frame of a hop to `url`: a main frame
/// navigation's, its URL's at each hop (its `IsolationInfo` is redone at a
/// redirect); another request's, the browser's. None if it is opaque.
fn top_frame_site_at(url: &Url, trusted: &TrustedParams) -> Option<Site> {
    let site = if trusted.is_main_frame_navigation {
        Site::of_url(url)
    } else {
        Site::of_url(&Url::parse(trusted.top_frame_site.as_deref()?).ok()?)
    };
    (!site.is_opaque()).then_some(site)
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

/// What a hop's cookies are read and written by, but its `SameSite`
/// context: its site for cookies and its partition.
struct Hop {
    site_for_cookies: SiteForCookies,
    partition_key: Option<CookiePartitionKey>,
}

impl Hop {
    fn new(url: &Url, trusted: &TrustedParams) -> Self {
        let site_for_cookies = site_for_cookies_at(url, trusted);
        let partition_key = CookiePartitionKey::for_request(
            top_frame_site_at(url, trusted).as_ref(),
            &site_for_cookies,
            url,
        );
        Self {
            site_for_cookies,
            partition_key,
        }
    }
}

impl CookieJar {
    /// A jar: empty, or, given a store, its persistent cookies written to
    /// it and the ones it had loaded in once they come.
    pub(crate) fn new(store: Option<(Box<dyn PersistentCookieStore>, Loaded)>) -> Self {
        let (monster, loading) = match store {
            Some((store, loaded)) => (CookieMonster::with_store(store), Some(loaded)),
            None => (CookieMonster::new(), None),
        };
        Self {
            monster: Arc::new(Mutex::new(monster)),
            loaded: Arc::default(),
            loading: Arc::new(Mutex::new(loading)),
        }
    }

    /// Waits for the database's cookies to be in the jar (Chromium's
    /// `CookieMonster` queues a call until its key is loaded; here, until
    /// all are): what a hop does before it reads or writes cookies.
    pub(crate) async fn wait_for_load(&self) {
        self.loaded
            .get_or_init(|| async {
                let loading = self
                    .loading
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                // Taken once: the first to wait loads them, the others
                // wait for it.
                if let Some(loading) = loading {
                    // None if the database thread is gone: nothing loaded.
                    let cookies = loading.await.unwrap_or_default();
                    self.monster().load(cookies);
                }
            })
            .await;
    }

    fn monster(&self) -> std::sync::MutexGuard<'_, CookieMonster> {
        self.monster.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The `Cookie` header's value for a hop of a request to `url`, of
    /// `method`, from where `trusted` says (none for a request without
    /// them: the browser's own, which carry no cookies), if it sends any:
    /// the jar's cookies the context and the partition give it, but those
    /// the policy keeps from it (`AnnotateAndMoveUserBlockedCookies`).
    /// After [`wait_for_load`](Self::wait_for_load).
    pub(crate) fn cookie_header(
        &self,
        url: &Url,
        trusted: Option<&TrustedParams>,
        method: &str,
    ) -> Option<String> {
        let trusted = trusted?;
        let hop = Hop::new(url, trusted);
        let context = context_for_request(url, &hop.site_for_cookies, trusted, method);
        let cookies = self.monster().get_cookie_list_with_options(
            url,
            &options(context),
            hop.partition_key.as_ref(),
            Time::now(),
        );
        let cookies: Vec<CanonicalCookie> = cookies
            .into_iter()
            .filter(|cookie| accessible(cookie, url, &hop.site_for_cookies))
            .collect();
        (!cookies.is_empty()).then(|| CanonicalCookie::build_cookie_line(&cookies))
    }

    /// Answers `call` (after [`wait_for_load`](Self::wait_for_load)).
    pub(crate) fn answer_manager(&self, call: CookieManagerCall) {
        let now = Time::now();
        match call {
            CookieManagerCall::GetList {
                urls,
                top_frame,
                done,
            } => {
                let top_site = top_frame
                    .as_ref()
                    .map(Site::of_url)
                    .filter(|site| !site.is_opaque());
                let site_for_cookies = top_site
                    .clone()
                    .map_or_else(SiteForCookies::null, SiteForCookies::of_site);
                let mut cookies: Vec<CanonicalCookie> = Vec::new();
                let mut monster = self.monster();
                for url in &urls {
                    let partition_key =
                        CookiePartitionKey::for_request(top_site.as_ref(), &site_for_cookies, url);
                    for cookie in
                        monster.get_all_inclusive_cookie_list(url, partition_key.as_ref(), now)
                    {
                        if !cookies.iter().any(|kept| kept.is_equivalent(&cookie)) {
                            cookies.push(cookie);
                        }
                    }
                }
                drop(monster);
                done(cookies);
            }
            CookieManagerCall::GetAll { done } => {
                let cookies = self.monster().all_cookies(now);
                done(cookies);
            }
            CookieManagerCall::Set { cookies, done } => {
                let mut monster = self.monster();
                let set = cookies
                    .into_iter()
                    .map(|cookie| {
                        let source = Url::parse(&format!(
                            "https://{}{}",
                            cookie.domain_without_dot(),
                            cookie.path()
                        ));
                        source.is_ok_and(|source| {
                            monster
                                .set_canonical_cookie(
                                    cookie,
                                    &source,
                                    &CookieOptions::make_all_inclusive(),
                                )
                                .is_ok()
                        })
                    })
                    .collect();
                drop(monster);
                done(set);
            }
            CookieManagerCall::Delete { filter, done } => {
                let deleted = self
                    .monster()
                    .delete_matching(|cookie| filter.matches(cookie));
                done(deleted);
            }
        }
    }

    /// Answers `call` (after [`wait_for_load`](Self::wait_for_load)).
    pub(crate) fn answer_script(&self, call: ScriptCookies) {
        match call {
            ScriptCookies::Get { url, trusted, done } => {
                done(self.cookies_for_script(&url, &trusted));
            }
            ScriptCookies::Set { url, trusted, line } => {
                self.set_from_script(&url, &trusted, &line);
            }
            ScriptCookies::Enabled { url, trusted, done } => {
                let site_for_cookies = site_for_cookies_at(&url, &trusted);
                done(site_for_cookies.is_first_party_schemeless(&url));
            }
        }
    }

    /// The cookie string script sees at `url`, from where `trusted` says
    /// (`GetCookiesString`): the jar's cookies of the context and the
    /// partition, but `HttpOnly` ones and those the policy keeps from it.
    fn cookies_for_script(&self, url: &Url, trusted: &TrustedParams) -> String {
        let hop = Hop::new(url, trusted);
        let options = CookieOptions {
            include_httponly: false,
            ..options(context_for_script(url, &hop.site_for_cookies))
        };
        let cookies = self.monster().get_cookie_list_with_options(
            url,
            &options,
            hop.partition_key.as_ref(),
            Time::now(),
        );
        let cookies: Vec<CanonicalCookie> = cookies
            .into_iter()
            .filter(|cookie| accessible(cookie, url, &hop.site_for_cookies))
            .collect();
        CanonicalCookie::build_cookie_line(&cookies)
    }

    /// Sets `line` as script sets it at `url`, from where `trusted` says
    /// (`SetCookieFromString`): not `HttpOnly`, nor over an `HttpOnly`
    /// one; in the partition, if `Partitioned`; not if the policy keeps it
    /// from script there.
    fn set_from_script(&self, url: &Url, trusted: &TrustedParams, line: &str) {
        let hop = Hop::new(url, trusted);
        let options = CookieOptions {
            include_httponly: false,
            ..options(context_for_script(url, &hop.site_for_cookies))
        };
        let cookie = CanonicalCookie::create(
            url,
            line,
            Time::now(),
            None,
            hop.partition_key.as_ref(),
            CookieSourceType::Script,
        );
        if let Ok(cookie) = cookie
            && accessible(&cookie, url, &hop.site_for_cookies)
        {
            let _ = self.monster().set_canonical_cookie(cookie, url, &options);
        }
    }

    /// Keeps the cookies the `Set-Cookie` headers of the response of a hop
    /// to `url` set (`headers`: the response's, names lowercase), from
    /// where `trusted` says: none for a request without them, nor those
    /// the policy keeps from it; a `Partitioned` one in the hop's
    /// partition. An `Expires` is moved by how far the response's `Date`
    /// is from now. After [`wait_for_load`](Self::wait_for_load).
    pub(crate) fn set_from_response(
        &self,
        url: &Url,
        trusted: Option<&TrustedParams>,
        headers: &[(String, String)],
    ) {
        let Some(trusted) = trusted else {
            return;
        };
        let hop = Hop::new(url, trusted);
        let options = options(context_for_response(url, &hop.site_for_cookies, trusted));
        let server_time = headers
            .iter()
            .find(|(name, _)| name == "date")
            .and_then(|(_, date)| parse_cookie_expiration_time(date));
        let now = Time::now();
        let mut monster = self.monster();
        for (_, line) in headers.iter().filter(|(name, _)| name == "set-cookie") {
            let cookie = CanonicalCookie::create(
                url,
                line,
                now,
                server_time,
                hop.partition_key.as_ref(),
                CookieSourceType::Http,
            );
            if let Ok(cookie) = cookie
                && accessible(&cookie, url, &hop.site_for_cookies)
            {
                // A cookie the context or the jar refuses is dropped, as
                // Chromium's (it tells DevTools why; machopo doesn't yet).
                let _ = monster.set_canonical_cookie(cookie, url, &options);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Waker};

    use tokio::sync::oneshot;

    use super::*;

    struct NoStore;

    impl PersistentCookieStore for NoStore {
        fn add_cookie(&mut self, _: &CanonicalCookie) {}
        fn update_cookie_access_time(&mut self, _: &CanonicalCookie) {}
        fn delete_cookie(&mut self, _: &CanonicalCookie) {}
    }

    fn first_party() -> TrustedParams {
        TrustedParams {
            site_for_cookies: Some("https://a.example".to_owned()),
            top_frame_site: Some("https://a.example".to_owned()),
            initiator: Some("https://a.example".to_owned()),
            is_main_frame_navigation: false,
        }
    }

    /// Until the database's cookies are in, a hop waits; then it has them.
    /// The first to wait loads them; later waits are over at once.
    #[test]
    fn a_hop_waits_for_the_databases_cookies() {
        let (send, loaded) = oneshot::channel();
        let jar = CookieJar::new(Some((Box::new(NoStore), loaded)));
        let url = Url::parse("https://a.example/").unwrap();
        let mut context = Context::from_waker(Waker::noop());
        let mut waiting = pin!(jar.wait_for_load());
        assert!(waiting.as_mut().poll(&mut context).is_pending());
        let cookie = CanonicalCookie::create(
            &url,
            "a=1; Max-Age=60",
            Time::now(),
            None,
            None,
            CookieSourceType::Http,
        )
        .unwrap();
        send.send(vec![cookie]).unwrap();
        assert!(waiting.as_mut().poll(&mut context).is_ready());
        assert_eq!(
            jar.cookie_header(&url, Some(&first_party()), "GET")
                .as_deref(),
            Some("a=1")
        );
        assert!(pin!(jar.wait_for_load()).poll(&mut context).is_ready());
    }

    /// A main frame's navigation is in the partition of its URL's site at
    /// each hop, whatever site it started at (a redirect's next hop is
    /// another top-level site's): a `Partitioned` cookie it sets goes with
    /// that site's requests.
    #[test]
    fn a_navigations_partition_is_its_hops() {
        let jar = CookieJar::new(None);
        let url = Url::parse("https://a.example/").unwrap();
        let from_b = TrustedParams {
            site_for_cookies: Some("https://b.example".to_owned()),
            top_frame_site: Some("https://b.example".to_owned()),
            initiator: None,
            is_main_frame_navigation: true,
        };
        let headers = [(
            "set-cookie".to_owned(),
            "chip=1; Secure; Partitioned".to_owned(),
        )];
        jar.set_from_response(&url, Some(&from_b), &headers);
        assert_eq!(
            jar.cookie_header(&url, Some(&first_party()), "GET")
                .as_deref(),
            Some("chip=1")
        );
    }

    /// Without a database, or once its thread is gone, there is nothing to
    /// wait for.
    #[test]
    fn nothing_to_wait_for_without_a_database() {
        let mut context = Context::from_waker(Waker::noop());
        let jar = CookieJar::new(None);
        assert!(pin!(jar.wait_for_load()).poll(&mut context).is_ready());
        let (send, loaded) = oneshot::channel::<Vec<CanonicalCookie>>();
        drop(send);
        let jar = CookieJar::new(Some((Box::new(NoStore), loaded)));
        assert!(pin!(jar.wait_for_load()).poll(&mut context).is_ready());
    }
}
