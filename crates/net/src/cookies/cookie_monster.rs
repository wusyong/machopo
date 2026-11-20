//! The cookie jar (Chromium's `net::CookieMonster`), in memory: setting a
//! cookie replaces the one of its name, domain and path; reading gives the
//! cookies a URL gets, in the order a `Cookie` header sends them.

use std::collections::BTreeMap;

use base::SequenceNumber;
use url::Url;

use super::canonical_cookie::CanonicalCookie;
use super::cookie_inclusion_status::{CookieInclusionStatus, ExclusionReason};
use super::cookie_options::CookieOptions;
use super::cookie_util::{self, CookieAccessScheme};
use super::time::Time;

/// A cookie in the jar.
#[derive(Debug, Clone)]
struct Entry {
    cookie: CanonicalCookie,
    /// When it came into the jar, among the others: what orders two
    /// cookies of the same path length and creation time, which Chromium
    /// leaves to its sort.
    order: u64,
}

/// The cookie jar (`net::CookieMonster`), in memory, synchronous: its
/// caller is the one thread cookies are read and written on (the
/// `NetworkService` thread). No limits on how many cookies yet, so nothing
/// is evicted but expired ones; no persistence; no partitions.
///
/// "Now" is given to the calls that need it (Chromium's take
/// `Time::Now()`), so a test can choose it.
#[derive(Debug, Default)]
pub struct CookieMonster {
    /// The cookies by their key: the registrable domain of their domain, or
    /// the domain itself if it has none (`CookieMonster::GetKey`). A URL's
    /// cookies are all under its host's key.
    cookies: BTreeMap<String, Vec<Entry>>,
    next_order: SequenceNumber<u64>,
}

/// The key `domain` (a cookie's, or a URL's host) is kept under
/// (`CookieMonster::GetKey`).
fn key(domain: &str) -> String {
    cookie_util::registrable_domain_of(domain)
        .unwrap_or_else(|| domain.strip_prefix('.').unwrap_or(domain))
        .to_owned()
}

/// Sorts `indices` of `entries` in the order a `Cookie` header sends the
/// cookies in (`CookieMonster::CookieSorter`, RFC 6265bis's): longer paths
/// first, then the earlier made.
fn sort_for_sending(entries: &[Entry], indices: &mut [usize]) {
    indices.sort_by_key(|&index| {
        entries.get(index).map(|entry| {
            (
                std::cmp::Reverse(entry.cookie.path().len()),
                entry.cookie.creation_date(),
                entry.order,
            )
        })
    });
}

impl CookieMonster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `cookie`, as `source_url` sets it, in `options`
    /// (`CookieMonster::SetCanonicalCookie`): if `source_url` may
    /// ([`CanonicalCookie::is_set_permitted_in_context`]), it replaces the
    /// cookie of its name, domain and path, keeping that one's creation
    /// time if the value is the same. Refused if it would replace an
    /// `HttpOnly` one in `options` without them (script), or if
    /// `source_url` is insecure and a `Secure` cookie of its name is on a
    /// domain one is in the other and on its path ("leave secure cookies
    /// alone"). One whose expiry has passed (by its creation time) deletes
    /// the cookie it replaces and isn't kept. Cookies of its key expired by
    /// then are deleted first.
    pub fn set_canonical_cookie(
        &mut self,
        mut cookie: CanonicalCookie,
        source_url: &Url,
        options: &CookieOptions,
    ) -> Result<(), CookieInclusionStatus> {
        let mut status = cookie.is_set_permitted_in_context(source_url, options);
        let allowed_to_set_secure_cookie = cookie_util::provisional_access_scheme(source_url)
            != CookieAccessScheme::NonCryptographic;
        let already_expired = cookie.is_expired(cookie.creation_date());
        let key = key(cookie.domain());

        // Expired cookies go first, so one can't keep a new one out: RFC
        // 6265bis evicts them "at any time"; Chromium leaves them until
        // they are read or the domain has too many.
        if let Some(entries) = self.cookies.get_mut(&key) {
            entries.retain(|entry| !entry.cookie.is_expired(cookie.creation_date()));
            if entries.is_empty() {
                self.cookies.remove(&key);
            }
        }

        // `MaybeDeleteEquivalentCookieAndUpdateStatus`.
        let mut replaced = None;
        for (index, entry) in self.cookies.get(&key).into_iter().flatten().enumerate() {
            let existing = &entry.cookie;
            if existing.is_secure()
                && !allowed_to_set_secure_cookie
                && cookie.is_equivalent_for_secure_cookie_matching(existing)
            {
                status.add_exclusion_reason(ExclusionReason::OverwriteSecure);
            }
            if cookie.is_equivalent(existing) {
                if existing.is_http_only() && !options.include_httponly {
                    status.add_exclusion_reason(ExclusionReason::OverwriteHttpOnly);
                } else {
                    replaced = Some(index);
                }
            }
        }
        status.into_result()?;

        let entries = self.cookies.entry(key.clone()).or_default();
        let mut order = None;
        if let Some(index) = replaced {
            let old = entries.remove(index);
            // The same cookie, set again: it keeps its age (and its place
            // among those of its age). RFC 6265bis keeps the old creation
            // time whatever the value; Chromium only for the same value.
            if old.cookie.value() == cookie.value() {
                cookie.set_creation_date(old.cookie.creation_date());
                order = Some(old.order);
            }
        }
        if !already_expired {
            let order = order.unwrap_or_else(|| self.next_order.get_next());
            entries.push(Entry { cookie, order });
        } else if entries.is_empty() {
            self.cookies.remove(&key);
        }
        Ok(())
    }

    /// The cookies `url` gets in `options` at `now`, in the order a `Cookie`
    /// header sends them (`CookieMonster::GetCookieListWithOptions`):
    /// longer paths first, then the earlier made. Those it gets are marked
    /// as used at `now`; expired ones it finds are deleted. None for a URL
    /// of a scheme cookies aren't for.
    pub fn get_cookie_list_with_options(
        &mut self,
        url: &Url,
        options: &CookieOptions,
        now: Time,
    ) -> Vec<CanonicalCookie> {
        if !cookie_util::is_cookieable_scheme(url) {
            return Vec::new();
        }
        let Some(host) = url.host_str() else {
            return Vec::new();
        };
        let key = key(host);
        let Some(entries) = self.cookies.get_mut(&key) else {
            return Vec::new();
        };
        entries.retain(|entry| !entry.cookie.is_expired(now));

        let mut sorted: Vec<usize> = (0..entries.len()).collect();
        sort_for_sending(entries, &mut sorted);
        let mut cookies = Vec::new();
        for index in sorted {
            let Some(entry) = entries.get_mut(index) else {
                continue;
            };
            if entry
                .cookie
                .include_for_request_url(url, options, now)
                .is_include()
            {
                entry.cookie.set_last_access_date(now);
                cookies.push(entry.cookie.clone());
            }
        }
        if entries.is_empty() {
            self.cookies.remove(&key);
        }
        cookies
    }

    /// The value of the `Cookie` header a request to `url` sends in
    /// `options` at `now`, `None` if it sends none: the cookies of
    /// [`get_cookie_list_with_options`](Self::get_cookie_list_with_options),
    /// as [`CanonicalCookie::build_cookie_line`] writes them.
    pub fn cookie_line_for(
        &mut self,
        url: &Url,
        options: &CookieOptions,
        now: Time,
    ) -> Option<String> {
        let cookies = self.get_cookie_list_with_options(url, options, now);
        (!cookies.is_empty()).then(|| CanonicalCookie::build_cookie_line(&cookies))
    }

    /// Every cookie not expired at `now` (`CookieMonster::GetAllCookies`),
    /// those of a key together, by key, each key's in the order they came
    /// in; the expired ones are deleted.
    pub fn all_cookies(&mut self, now: Time) -> Vec<CanonicalCookie> {
        for entries in self.cookies.values_mut() {
            entries.retain(|entry| !entry.cookie.is_expired(now));
        }
        self.cookies.retain(|_, entries| !entries.is_empty());
        self.cookies
            .values()
            .flat_map(|entries| {
                let mut entries: Vec<&Entry> = entries.iter().collect();
                entries.sort_by_key(|entry| entry.order);
                entries.into_iter().map(|entry| entry.cookie.clone())
            })
            .collect()
    }

    /// Deletes the cookie of `name`, `domain` (with its dot, for a domain
    /// cookie) and `path`, unpartitioned or partitioned; whether there was
    /// one.
    pub fn delete_cookie(&mut self, name: &str, domain: &str, path: &str) -> bool {
        self.delete_matching(|cookie| {
            cookie.name() == name && cookie.domain() == domain && cookie.path() == path
        }) > 0
    }

    /// Deletes every cookie `predicate` picks
    /// (`CookieMonster::DeleteMatchingCookies`); how many.
    pub fn delete_matching(
        &mut self,
        mut predicate: impl FnMut(&CanonicalCookie) -> bool,
    ) -> usize {
        let mut deleted: usize = 0;
        for entries in self.cookies.values_mut() {
            let before = entries.len();
            entries.retain(|entry| !predicate(&entry.cookie));
            // `retain` only removes.
            deleted = deleted.saturating_add(before.saturating_sub(entries.len()));
        }
        self.cookies.retain(|_, entries| !entries.is_empty());
        deleted
    }

    /// Deletes every cookie (`DeleteAllAsync`); how many.
    pub fn delete_all(&mut self) -> usize {
        self.delete_matching(|_| true)
    }
}
