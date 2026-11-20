//! The cookie jar (Chromium's `net::CookieMonster`): setting a cookie
//! replaces the one of its name, domain, path and partition; reading gives
//! the cookies a URL gets, in the order a `Cookie` header sends them; a
//! domain, a partition's domain and the jar have at most so many, the
//! least recently used going first. Persistent cookies are written through
//! a [`PersistentCookieStore`], if it has one.

use std::collections::{BTreeMap, HashSet};

use base::SequenceNumber;
use url::Url;

use super::canonical_cookie::CanonicalCookie;
use super::cookie_inclusion_status::{CookieInclusionStatus, ExclusionReason};
use super::cookie_options::CookieOptions;
use super::cookie_partition_key::CookiePartitionKey;
use super::cookie_util::{self, CookieAccessScheme};
use super::time::Time;

/// The most unpartitioned cookies a key (a registrable domain) has
/// (`kDomainMaxCookies`); past it, it is cut by
/// [`DOMAIN_PURGE_COOKIES`] more (`kDomainPurgeCookies`).
pub const DOMAIN_MAX_COOKIES: usize = 180;
pub const DOMAIN_PURGE_COOKIES: usize = 30;
/// What a key's purge leaves.
const DOMAIN_PURGED_TO: usize = DOMAIN_MAX_COOKIES - DOMAIN_PURGE_COOKIES;
/// The most unpartitioned cookies the jar has (`kMaxCookies`); past it,
/// it is cut by [`PURGE_COOKIES`] more (`kPurgeCookies`), but for the
/// cookies used in the last [`SAFE_FROM_GLOBAL_PURGE_DAYS`].
pub const MAX_COOKIES: usize = 3300;
pub const PURGE_COOKIES: usize = 300;
/// What the jar's purge leaves.
const PURGED_TO: usize = MAX_COOKIES - PURGE_COOKIES;
pub const SAFE_FROM_GLOBAL_PURGE_DAYS: i64 = 30;
/// The most cookies, and bytes of their names and values, a partition has
/// of a key (`kPerPartitionDomainMaxCookies`,
/// `kPerPartitionDomainMaxCookieBytes`). Partitioned cookies have no limit
/// across keys yet, as in Chromium.
pub const PER_PARTITION_DOMAIN_MAX_COOKIES: usize = 180;
pub const PER_PARTITION_DOMAIN_MAX_COOKIE_BYTES: usize = 10240;
/// How long after a cookie was last used reading it again marks it used
/// (`kDefaultAccessUpdateThresholdSeconds`): fewer writes to the store.
pub const ACCESS_UPDATE_THRESHOLD_SECONDS: i64 = 60;

/// Where the jar's persistent cookies are written
/// (`CookieMonster::PersistentCookieStore`): told of each change to one,
/// as it happens. Session cookies are never written.
pub trait PersistentCookieStore: Send {
    fn add_cookie(&mut self, cookie: &CanonicalCookie);
    fn update_cookie_access_time(&mut self, cookie: &CanonicalCookie);
    fn delete_cookie(&mut self, cookie: &CanonicalCookie);
}

/// A cookie in the jar.
#[derive(Debug, Clone)]
struct Entry {
    cookie: CanonicalCookie,
    /// When it came into the jar, among the others: what orders two
    /// cookies of the same path length and creation time, which Chromium
    /// leaves to its sort, and which it is to a purge.
    order: u64,
}

/// The cookie jar (`net::CookieMonster`), synchronous: its caller is the
/// one thread cookies are read and written on (the `NetworkService`
/// thread), which has it loaded before it reads or writes cookies.
///
/// "Now" is given to the calls that need it (Chromium's take
/// `Time::Now()`), so a test can choose it.
#[derive(Default)]
pub struct CookieMonster {
    /// The cookies by their key: the registrable domain of their domain, or
    /// the domain itself if it has none (`CookieMonster::GetKey`). A URL's
    /// cookies are all under its host's key, those of every partition
    /// together (Chromium keeps each partition's in a map of its own).
    cookies: BTreeMap<String, Vec<Entry>>,
    next_order: SequenceNumber<u64>,
    /// How many unpartitioned cookies there are: what [`MAX_COOKIES`]
    /// counts.
    unpartitioned: usize,
    /// No later than the earliest an unpartitioned cookie was last used,
    /// if known (`earliest_access_time_`): the jar's purge waits until it
    /// is older than [`SAFE_FROM_GLOBAL_PURGE_DAYS`], when there is one to
    /// purge.
    earliest_access: Option<Time>,
    store: Option<Box<dyn PersistentCookieStore>>,
}

impl std::fmt::Debug for CookieMonster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieMonster")
            .field("cookies", &self.cookies)
            .field("store", &self.store.is_some())
            .finish_non_exhaustive()
    }
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

/// The order a purge takes cookies in (`LRACookieSorter`): the least
/// recently used first, then the earlier made.
fn least_recently_used(entry: &Entry) -> (Time, Time, u64) {
    (
        entry.cookie.last_access_date(),
        entry.cookie.creation_date(),
        entry.order,
    )
}

impl CookieMonster {
    /// An empty jar, in memory alone.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty jar whose persistent cookies are written to `store`; the
    /// cookies `store` has are [`load`](Self::load)ed into it.
    pub fn with_store(store: Box<dyn PersistentCookieStore>) -> Self {
        Self {
            store: Some(store),
            ..Self::default()
        }
    }

    /// Takes in the cookies its store had (`StoreLoadedCookies`), the
    /// earlier made first, without writing them back.
    pub fn load(&mut self, mut cookies: Vec<CanonicalCookie>) {
        cookies.sort_by_key(CanonicalCookie::creation_date);
        for cookie in cookies {
            if !cookie.is_partitioned() {
                let access = cookie.last_access_date();
                self.earliest_access = Some(self.earliest_access.map_or(access, |e| e.min(access)));
            }
            self.insert(key(cookie.domain()), cookie, None, false);
        }
    }

    /// Puts `cookie` under `key`, its place among those of its age `order`
    /// if it takes one's (a new one's if none); written to the store if
    /// `sync_to_store` and it is persistent (`InternalInsertCookie`).
    fn insert(
        &mut self,
        key: String,
        cookie: CanonicalCookie,
        order: Option<u64>,
        sync_to_store: bool,
    ) {
        if sync_to_store
            && cookie.is_persistent()
            && let Some(store) = &mut self.store
        {
            store.add_cookie(&cookie);
        }
        if !cookie.is_partitioned() {
            self.unpartitioned = self.unpartitioned.saturating_add(1);
        }
        let order = order.unwrap_or_else(|| self.next_order.get_next());
        self.cookies
            .entry(key)
            .or_default()
            .push(Entry { cookie, order });
    }

    /// Deletes the cookies under `key` that `doomed` picks, from the store
    /// too (`InternalDeleteCookie`); how many.
    fn delete_where(&mut self, key: &str, mut doomed: impl FnMut(&Entry) -> bool) -> usize {
        let Some(entries) = self.cookies.get_mut(key) else {
            return 0;
        };
        let (store, unpartitioned) = (&mut self.store, &mut self.unpartitioned);
        let before = entries.len();
        entries.retain(|entry| {
            if !doomed(entry) {
                return true;
            }
            if entry.cookie.is_persistent()
                && let Some(store) = store
            {
                store.delete_cookie(&entry.cookie);
            }
            if !entry.cookie.is_partitioned() {
                *unpartitioned = unpartitioned.saturating_sub(1);
            }
            false
        });
        // `retain` only removes.
        let deleted = before.saturating_sub(entries.len());
        if entries.is_empty() {
            self.cookies.remove(key);
        }
        deleted
    }

    /// Deletes the cookies under `key` expired by `now`.
    fn delete_expired(&mut self, key: &str, now: Time) -> usize {
        self.delete_where(key, |entry| entry.cookie.is_expired(now))
    }

    /// Sets `cookie`, as `source_url` sets it, in `options`
    /// (`CookieMonster::SetCanonicalCookie`): if `source_url` may
    /// ([`CanonicalCookie::is_set_permitted_in_context`]), it replaces the
    /// cookie of its name, domain, path and partition, keeping that one's
    /// creation time if the value is the same. Refused if it would replace
    /// an `HttpOnly` one in `options` without them (script), or if
    /// `source_url` is insecure and a `Secure` cookie of its name and
    /// partition is on a domain one is in the other and on its path
    /// ("leave secure cookies alone"). One whose expiry has passed (by its
    /// creation time) deletes the cookie it replaces and isn't kept.
    /// Cookies of its key expired by then are deleted first; once it is
    /// in, its key, its partition's key and the jar are cut to their
    /// limits (`GarbageCollect`).
    pub fn set_canonical_cookie(
        &mut self,
        mut cookie: CanonicalCookie,
        source_url: &Url,
        options: &CookieOptions,
    ) -> Result<(), CookieInclusionStatus> {
        let mut status = cookie.is_set_permitted_in_context(source_url, options);
        let allowed_to_set_secure_cookie = cookie_util::provisional_access_scheme(source_url)
            != CookieAccessScheme::NonCryptographic;
        let now = cookie.creation_date();
        let already_expired = cookie.is_expired(now);
        let key = key(cookie.domain());

        // Expired cookies go first, so one can't keep a new one out: RFC
        // 6265bis evicts them "at any time"; Chromium leaves them until
        // they are read or the domain has too many.
        self.delete_expired(&key, now);

        // `MaybeDeleteEquivalentCookieAndUpdateStatus`.
        let mut replaced = None;
        for entry in self.cookies.get(&key).into_iter().flatten() {
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
                    replaced = Some((
                        entry.order,
                        existing.value() == cookie.value(),
                        existing.creation_date(),
                    ));
                }
            }
        }
        status.into_result()?;

        let mut order = None;
        if let Some((old_order, same_value, old_creation)) = replaced {
            self.delete_where(&key, |entry| entry.order == old_order);
            // The same cookie, set again: it keeps its age (and its place
            // among those of its age). RFC 6265bis keeps the old creation
            // time whatever the value; Chromium only for the same value.
            if same_value {
                cookie.set_creation_date(old_creation);
                order = Some(old_order);
            }
        }
        if !already_expired {
            let partition_key = cookie.partition_key().cloned();
            self.insert(key.clone(), cookie, order, true);
            match partition_key {
                Some(partition_key) => self.garbage_collect_partitioned(now, &partition_key, &key),
                None => self.garbage_collect(now, &key),
            }
        }
        Ok(())
    }

    /// Cuts `key`'s unpartitioned cookies to [`DOMAIN_MAX_COOKIES`] less
    /// [`DOMAIN_PURGE_COOKIES`] once they are past it, then the jar's to
    /// [`MAX_COOKIES`] less [`PURGE_COOKIES`] (`GarbageCollect`): the
    /// expired first, then the least recently used, those not `Secure`
    /// before those that are. The jar keeps those used in the last
    /// [`SAFE_FROM_GLOBAL_PURGE_DAYS`].
    ///
    /// `Priority` isn't parsed: every cookie is of medium priority, so of
    /// Chromium's six rounds of a key's purge only the two of medium ones
    /// (not `Secure`, then any) have cookies, and the quota of medium ones
    /// they leave (`kDomainCookiesQuotaMedium`, 50) never binds: the purge
    /// stops at 150.
    fn garbage_collect(&mut self, now: Time, key: &str) {
        let count = |monster: &Self| {
            monster.cookies.get(key).map_or(0, |entries| {
                entries
                    .iter()
                    .filter(|e| !e.cookie.is_partitioned())
                    .count()
            })
        };
        if count(self) > DOMAIN_MAX_COOKIES {
            self.delete_where(key, |e| {
                !e.cookie.is_partitioned() && e.cookie.is_expired(now)
            });
            let count = count(self);
            if count > DOMAIN_MAX_COOKIES {
                let purge_goal = count.saturating_sub(DOMAIN_PURGED_TO);
                let deleted = self.purge_least_recent(key, purge_goal, true);
                self.purge_least_recent(key, purge_goal.saturating_sub(deleted), false);
            }
        }

        let safe_date = now.saturating_add_days(SAFE_FROM_GLOBAL_PURGE_DAYS.saturating_neg());
        if self.unpartitioned > MAX_COOKIES && self.earliest_access.is_none_or(|e| e < safe_date) {
            let keys: Vec<String> = self.cookies.keys().cloned().collect();
            for key in &keys {
                self.delete_where(key, |e| {
                    !e.cookie.is_partitioned() && e.cookie.is_expired(now)
                });
            }
            if self.unpartitioned > MAX_COOKIES {
                self.purge_jar(safe_date);
            }
        }
    }

    /// One of a key's purge's rounds (`PurgeLeastRecentMatches`): deletes
    /// `key`'s least recently used unpartitioned cookies, at most
    /// `purge_goal`, none `Secure` if `protect_secure_cookies`; how many.
    fn purge_least_recent(
        &mut self,
        key: &str,
        purge_goal: usize,
        protect_secure_cookies: bool,
    ) -> usize {
        let Some(entries) = self.cookies.get(key) else {
            return 0;
        };
        let mut candidates: Vec<&Entry> = entries
            .iter()
            .filter(|e| !e.cookie.is_partitioned())
            .filter(|e| !protect_secure_cookies || !e.cookie.is_secure())
            .collect();
        candidates.sort_by_key(|e| least_recently_used(e));
        let doomed: HashSet<u64> = candidates
            .into_iter()
            .take(purge_goal)
            .map(|e| e.order)
            .collect();
        self.delete_where(key, |e| doomed.contains(&e.order))
    }

    /// The jar's purge, past [`MAX_COOKIES`] unpartitioned cookies
    /// (`GarbageCollectLeastRecentlyAccessed`): the least recently used
    /// down to [`MAX_COOKIES`] less [`PURGE_COOKIES`], those not `Secure`
    /// first, but none used since `safe_date`.
    fn purge_jar(&mut self, safe_date: Time) {
        let purge_goal = self.unpartitioned.saturating_sub(PURGED_TO);
        let mut all: Vec<(String, &Entry)> = self
            .cookies
            .iter()
            .flat_map(|(key, entries)| entries.iter().map(move |e| (key.clone(), e)))
            .filter(|(_, e)| !e.cookie.is_partitioned())
            .collect();
        all.sort_by_key(|(_, e)| least_recently_used(e));
        let (secure, non_secure): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|(_, e)| e.cookie.is_secure());
        let mut doomed: Vec<(String, u64)> = Vec::new();
        // Of a round's cookies, least recently used first, the first
        // `goal` but those used since `safe_date`; and when the first left
        // was last used.
        let mut round = |cookies: &[(String, &Entry)], goal: usize| {
            let old = cookies
                .iter()
                .take(goal)
                .take_while(|(_, e)| e.cookie.last_access_date() < safe_date)
                .count();
            doomed.extend(
                cookies
                    .iter()
                    .take(old)
                    .map(|(key, e)| (key.clone(), e.order)),
            );
            (
                old,
                cookies.get(old).map(|(_, e)| e.cookie.last_access_date()),
            )
        };
        let (deleted, earliest_non_secure) = round(&non_secure, purge_goal.min(non_secure.len()));
        if secure.is_empty() {
            self.earliest_access = earliest_non_secure;
        } else if deleted < purge_goal {
            let goal = purge_goal.saturating_sub(deleted).min(secure.len());
            let (_, earliest_secure) = round(&secure, goal);
            self.earliest_access = match (earliest_non_secure, earliest_secure) {
                (Some(non_secure), Some(secure)) if non_secure < secure => Some(non_secure),
                _ => earliest_secure,
            };
        }
        // Else the secure cookies weren't looked at: the earliest stays no
        // later than it is, as Chromium leaves it.
        for (key, order) in doomed {
            self.delete_where(&key, |e| e.order == order);
        }
    }

    /// Cuts `partition_key`'s cookies of `key` to
    /// [`PER_PARTITION_DOMAIN_MAX_COOKIES`] and
    /// [`PER_PARTITION_DOMAIN_MAX_COOKIE_BYTES`] once they are past either
    /// (`GarbageCollectPartitionedCookies`): the expired, then the least
    /// recently used, `Secure` or not.
    fn garbage_collect_partitioned(
        &mut self,
        now: Time,
        partition_key: &CookiePartitionKey,
        key: &str,
    ) {
        let of_partition = |e: &Entry| e.cookie.partition_key() == Some(partition_key);
        let usage = |monster: &Self| {
            monster.cookies.get(key).map_or((0, 0), |entries| {
                entries
                    .iter()
                    .filter(|e| of_partition(e))
                    .fold((0, 0), |(count, bytes), e| {
                        (
                            usize::saturating_add(count, 1),
                            usize::saturating_add(bytes, e.cookie.name_value_size_bytes()),
                        )
                    })
            })
        };
        let over = |(count, bytes): (usize, usize)| {
            count > PER_PARTITION_DOMAIN_MAX_COOKIES
                || bytes > PER_PARTITION_DOMAIN_MAX_COOKIE_BYTES
        };
        if !over(usage(self)) {
            return;
        }
        self.delete_where(key, |e| of_partition(e) && e.cookie.is_expired(now));
        let (mut count, mut bytes) = usage(self);
        let Some(entries) = self.cookies.get(key) else {
            return;
        };
        let mut candidates: Vec<&Entry> = entries.iter().filter(|e| of_partition(e)).collect();
        candidates.sort_by_key(|e| least_recently_used(e));
        let mut doomed = HashSet::new();
        for entry in candidates {
            if !over((count, bytes)) {
                break;
            }
            count = count.saturating_sub(1);
            bytes = bytes.saturating_sub(entry.cookie.name_value_size_bytes());
            doomed.insert(entry.order);
        }
        self.delete_where(key, |e| doomed.contains(&e.order));
    }

    /// The cookies `url` gets in `options` at `now`, in the order a `Cookie`
    /// header sends them (`CookieMonster::GetCookieListWithOptions`):
    /// unpartitioned ones, and those of `partition_key` (the request's
    /// partition, if it has one); longer paths first, then the earlier
    /// made. Those it gets are marked as used at `now`, if they weren't in
    /// the last [`ACCESS_UPDATE_THRESHOLD_SECONDS`]; expired ones it finds
    /// are deleted. None for a URL of a scheme cookies aren't for.
    pub fn get_cookie_list_with_options(
        &mut self,
        url: &Url,
        options: &CookieOptions,
        partition_key: Option<&CookiePartitionKey>,
        now: Time,
    ) -> Vec<CanonicalCookie> {
        self.cookie_list(url, options, partition_key, now, true)
    }

    /// The cookies of [`get_cookie_list_with_options`] for DevTools and
    /// automation: every one `url` could get in any context (`HttpOnly`
    /// too, of any `SameSite`), not marked as used (Chromium's
    /// `CookieOptions::MakeAllInclusive`, `do_not_update_access_time`).
    ///
    /// [`get_cookie_list_with_options`]: Self::get_cookie_list_with_options
    pub fn get_all_inclusive_cookie_list(
        &mut self,
        url: &Url,
        partition_key: Option<&CookiePartitionKey>,
        now: Time,
    ) -> Vec<CanonicalCookie> {
        self.cookie_list(
            url,
            &CookieOptions::make_all_inclusive(),
            partition_key,
            now,
            false,
        )
    }

    fn cookie_list(
        &mut self,
        url: &Url,
        options: &CookieOptions,
        partition_key: Option<&CookiePartitionKey>,
        now: Time,
        update_access_time: bool,
    ) -> Vec<CanonicalCookie> {
        if !cookie_util::is_cookieable_scheme(url) {
            return Vec::new();
        }
        let Some(host) = url.host_str() else {
            return Vec::new();
        };
        let key = key(host);
        self.delete_expired(&key, now);
        let Some(entries) = self.cookies.get_mut(&key) else {
            return Vec::new();
        };

        let mut sorted: Vec<usize> = (0..entries.len()).collect();
        sort_for_sending(entries, &mut sorted);
        let mut cookies = Vec::new();
        for index in sorted {
            let Some(entry) = entries.get_mut(index) else {
                continue;
            };
            let cookie = &mut entry.cookie;
            if cookie
                .partition_key()
                .is_some_and(|key| Some(key) != partition_key)
                || !cookie.include_for_request_url(url, options).is_include()
            {
                continue;
            }
            // `InternalUpdateCookieAccessTime`.
            if update_access_time
                && now.saturating_micros_since(cookie.last_access_date())
                    >= ACCESS_UPDATE_THRESHOLD_SECONDS.saturating_mul(1_000_000)
            {
                cookie.set_last_access_date(now);
                if cookie.is_persistent()
                    && let Some(store) = &mut self.store
                {
                    store.update_cookie_access_time(cookie);
                }
            }
            cookies.push(cookie.clone());
        }
        cookies
    }

    /// The value of the `Cookie` header a request to `url` of
    /// `partition_key` sends in `options` at `now`, `None` if it sends
    /// none: the cookies of
    /// [`get_cookie_list_with_options`](Self::get_cookie_list_with_options),
    /// as [`CanonicalCookie::build_cookie_line`] writes them.
    pub fn cookie_line_for(
        &mut self,
        url: &Url,
        options: &CookieOptions,
        partition_key: Option<&CookiePartitionKey>,
        now: Time,
    ) -> Option<String> {
        let cookies = self.get_cookie_list_with_options(url, options, partition_key, now);
        (!cookies.is_empty()).then(|| CanonicalCookie::build_cookie_line(&cookies))
    }

    /// Every cookie not expired at `now` (`CookieMonster::GetAllCookies`),
    /// those of a key together, by key, each key's in the order they came
    /// in; the expired ones are deleted.
    pub fn all_cookies(&mut self, now: Time) -> Vec<CanonicalCookie> {
        let keys: Vec<String> = self.cookies.keys().cloned().collect();
        for key in &keys {
            self.delete_expired(key, now);
        }
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
        let keys: Vec<String> = self.cookies.keys().cloned().collect();
        keys.iter().fold(0, |deleted: usize, key| {
            deleted.saturating_add(self.delete_where(key, |entry| predicate(&entry.cookie)))
        })
    }

    /// Deletes every cookie (`DeleteAllAsync`); how many.
    pub fn delete_all(&mut self) -> usize {
        self.delete_matching(|_| true)
    }
}
