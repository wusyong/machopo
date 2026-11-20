//! The cookie jar's partitions, its limits and what it tells its store,
//! against what Chromium's `CookieMonster` does (with every cookie of
//! medium priority: `Priority` isn't parsed).

use std::sync::{Arc, Mutex};

use net::cookies::{
    CanonicalCookie, CookieInclusionStatus, CookieMonster, CookieOptions, CookiePartitionKey,
    CookieSourceType, DOMAIN_MAX_COOKIES, MAX_COOKIES, PER_PARTITION_DOMAIN_MAX_COOKIES,
    PersistentCookieStore, SameSiteCookieContext, Time,
};
use site::{AncestorChainBit, Site};
use url::Url;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

/// A fixed "now" for the tests, and `seconds` after it.
fn at(seconds: i64) -> Time {
    Time::from_unix_seconds(1_700_000_000 + seconds)
}

const DAY: i64 = 86_400;

fn http() -> CookieOptions {
    CookieOptions {
        include_httponly: true,
        same_site_context: SameSiteCookieContext::SameSiteStrict,
        lax_allow_unsafe: false,
    }
}

/// The partition of a frame of another site in a top-level frame of
/// `site`.
fn partition(site: &str) -> CookiePartitionKey {
    CookiePartitionKey::new(Site::of_url(&url(site)), AncestorChainBit::CrossSite)
}

/// A jar whose clock is set by each call.
struct Jar {
    monster: CookieMonster,
}

impl Jar {
    fn new() -> Self {
        Self {
            monster: CookieMonster::new(),
        }
    }

    fn try_set(
        &mut self,
        now: i64,
        at_url: &str,
        line: &str,
        partition: Option<&CookiePartitionKey>,
    ) -> Result<(), CookieInclusionStatus> {
        let cookie = CanonicalCookie::create(
            &url(at_url),
            line,
            at(now),
            None,
            partition,
            CookieSourceType::Http,
        )?;
        self.monster
            .set_canonical_cookie(cookie, &url(at_url), &http())
    }

    fn set(&mut self, now: i64, at_url: &str, line: &str, partition: Option<&CookiePartitionKey>) {
        self.try_set(now, at_url, line, partition)
            .unwrap_or_else(|status| panic!("{line:?} at {at_url}: {status}"));
    }

    /// What a request to `to` of `partition` sends at `now`.
    fn line(&mut self, now: i64, to: &str, partition: Option<&CookiePartitionKey>) -> String {
        self.monster
            .cookie_line_for(&url(to), &http(), partition, at(now))
            .unwrap_or_default()
    }

    /// The names of every cookie, sorted.
    fn names(&mut self, now: i64) -> Vec<String> {
        let mut names: Vec<String> = self
            .monster
            .all_cookies(at(now))
            .iter()
            .map(|cookie| cookie.name().to_owned())
            .collect();
        names.sort_unstable();
        names
    }
}

fn names(prefix: &str, range: std::ops::Range<usize>) -> Vec<String> {
    range.map(|i| format!("{prefix}{i}")).collect()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort_unstable();
    names
}

/// A partitioned cookie goes only with requests of its partition (the
/// top-level site and the ancestor chain bit), unpartitioned ones with
/// every request; one of each name, domain, path and partition, so the
/// same name in another partition is another cookie, and a partitioned
/// `Secure` one keeps no unpartitioned cookie of its name out.
#[test]
fn a_partitioned_cookie_goes_with_its_partition_alone() {
    let mut jar = Jar::new();
    let (one, two) = (
        partition("https://one.example"),
        partition("https://two.example"),
    );
    let same_site_one = CookiePartitionKey::new(
        Site::of_url(&url("https://one.example")),
        AncestorChainBit::SameSite,
    );
    jar.set(1, "https://a.example/", "a=0", None);
    jar.set(
        2,
        "https://a.example/",
        "a=1; Secure; Partitioned",
        Some(&one),
    );
    jar.set(
        3,
        "https://a.example/",
        "a=2; Secure; Partitioned",
        Some(&two),
    );
    assert_eq!(jar.line(4, "https://a.example/", None), "a=0");
    assert_eq!(jar.line(4, "https://a.example/", Some(&one)), "a=0; a=1");
    assert_eq!(jar.line(4, "https://a.example/", Some(&two)), "a=0; a=2");
    assert_eq!(
        jar.line(4, "https://a.example/", Some(&same_site_one)),
        "a=0"
    );
    jar.set(
        5,
        "https://a.example/",
        "a=3; Secure; Partitioned",
        Some(&one),
    );
    assert_eq!(jar.line(6, "https://a.example/", Some(&one)), "a=0; a=3");
    assert_eq!(jar.line(6, "https://a.example/", Some(&two)), "a=0; a=2");

    // Leave secure cookies alone, in their partition.
    jar.set(
        7,
        "https://a.example/",
        "s=1; Secure; Partitioned",
        Some(&one),
    );
    jar.set(8, "http://a.example/", "s=2", None);
    assert_eq!(jar.line(9, "http://a.example/", Some(&one)), "a=0; s=2");
    assert_eq!(
        jar.line(9, "https://a.example/", Some(&one)),
        "a=0; a=3; s=1; s=2"
    );
}

/// Past 180 unpartitioned cookies, a key's are cut to 150: the least
/// recently used first (a cookie read is used then). Its partitioned
/// cookies aren't counted, nor cut.
#[test]
fn a_keys_cookies_are_cut_to_150() {
    let mut jar = Jar::new();
    let site = partition("https://top.example");
    for i in 0..10 {
        jar.set(
            1,
            "https://a.example/",
            &format!("p{i}=1; Secure; Partitioned"),
            Some(&site),
        );
    }
    for i in 0..DOMAIN_MAX_COOKIES {
        let path = if i < 10 { "/p" } else { "/q" };
        // `www.a.example`'s cookies are under `a.example`'s key too.
        let host = if i % 2 == 0 {
            "a.example"
        } else {
            "www.a.example"
        };
        let now = i64::try_from(i).unwrap() + 10;
        jar.set(
            now,
            &format!("https://{host}{path}"),
            &format!("c{i}=1; Path={path}; Domain=a.example"),
            None,
        );
    }
    assert_eq!(jar.names(1000).len(), DOMAIN_MAX_COOKIES + 10);
    // The first ten are used again.
    assert!(!jar.line(1000, "https://a.example/p", None).is_empty());
    jar.set(1001, "https://a.example/q", "c180=1; Path=/q", None);
    let kept = [names("c", 0..10), names("c", 41..181), names("p", 0..10)].concat();
    assert_eq!(jar.names(1001), sorted(kept));
}

/// A key's purge takes those not `Secure` first, then the least recently
/// used of any.
#[test]
fn a_keys_secure_cookies_go_last() {
    let mut jar = Jar::new();
    for i in 0..140 {
        jar.set(i, "https://a.example/", &format!("s{i}=1; Secure"), None);
    }
    for i in 0..41 {
        jar.set(200 + i, "https://a.example/", &format!("n{i}=1"), None);
    }
    let kept = [names("n", 31..41), names("s", 0..140)].concat();
    assert_eq!(jar.names(300), sorted(kept));

    // The partitioned ones, older still, aren't the key's to purge.
    let mut jar = Jar::new();
    let site = partition("https://top.example");
    for i in 0..5 {
        jar.set(
            -10 + i,
            "https://a.example/",
            &format!("p{i}=1; Secure; Partitioned"),
            Some(&site),
        );
    }
    for i in 0..170 {
        jar.set(i, "https://a.example/", &format!("s{i}=1; Secure"), None);
    }
    for i in 0..11 {
        jar.set(200 + i, "https://a.example/", &format!("n{i}=1"), None);
    }
    let kept = [names("p", 0..5), names("s", 20..170)].concat();
    assert_eq!(jar.names(300), sorted(kept));
}

/// Past 3300 unpartitioned cookies, the jar's are cut to 3000: the least
/// recently used, those not `Secure` first, but none used in the last 30
/// days.
#[test]
fn the_jars_cookies_are_cut_to_3000() {
    // 3299 cookies, the first ten `Secure`, all made in the first hour.
    let fill = |jar: &mut Jar| {
        for key in 0..33 {
            for i in 0..100 {
                let now = key * 100 + i;
                let line = if key == 0 && i < 10 {
                    format!("s{i}=1; Secure")
                } else {
                    format!("c{now}=1")
                };
                if now < 3299 {
                    jar.set(now, &format!("https://k{key}.example/"), &line, None);
                }
            }
        }
    };
    let mut jar = Jar::new();
    fill(&mut jar);
    // 3300, a month later: none cut.
    jar.set(31 * DAY, "https://edge.example/", "edge=1", None);
    assert_eq!(jar.names(31 * DAY).len(), MAX_COOKIES);
    jar.set(31 * DAY, "https://new.example/", "new=1", None);
    let kept = jar.names(31 * DAY);
    assert_eq!(kept.len(), 3000);
    let mut expected = [names("s", 0..10), vec!["edge".to_owned(), "new".to_owned()]].concat();
    // The oldest not `Secure`: c10 to c310.
    expected.extend((311..3299).map(|i| format!("c{i}")));
    assert_eq!(kept, sorted(expected));

    // Used in the last 30 days: none cut.
    let mut jar = Jar::new();
    fill(&mut jar);
    jar.set(29 * DAY, "https://edge.example/", "edge=1", None);
    jar.set(29 * DAY, "https://new.example/", "new=1", None);
    assert_eq!(jar.names(29 * DAY).len(), MAX_COOKIES + 1);
}

/// A partition's cookies of a key are cut to 180, and to 10 KiB of names
/// and values, the least recently used first, `Secure` or not (they all
/// are); another partition's are its own.
#[test]
fn a_partitions_cookies_are_cut_to_180_and_10_kib() {
    let mut jar = Jar::new();
    let (one, two) = (
        partition("https://one.example"),
        partition("https://two.example"),
    );
    jar.set(
        0,
        "https://a.example/",
        "other=1; Secure; Partitioned",
        Some(&two),
    );
    for i in 0..=PER_PARTITION_DOMAIN_MAX_COOKIES {
        let now = i64::try_from(i).unwrap() + 1;
        jar.set(
            now,
            "https://a.example/",
            &format!("c{i}=1; Secure; Partitioned"),
            Some(&one),
        );
    }
    let kept = [names("c", 1..181), vec!["other".to_owned()]].concat();
    assert_eq!(jar.names(1000), sorted(kept));

    let mut jar = Jar::new();
    let value = "v".repeat(1000);
    for i in 0..11 {
        jar.set(
            i,
            "https://a.example/",
            &format!("b{i}={value}; Secure; Partitioned"),
            Some(&one),
        );
    }
    // 11 of 1002 bytes are past 10240; 10 aren't.
    assert_eq!(jar.names(100), sorted(names("b", 1..11)));
}

/// What the store is told: each persistent cookie added, its use (a minute
/// apart), its deletion (replaced, expired, purged); never a session
/// cookie. Loading writes nothing back.
#[test]
fn the_store_is_told_of_persistent_cookies() {
    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<String>>>);
    impl Log {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }
    struct Recording(Log);
    impl PersistentCookieStore for Recording {
        fn add_cookie(&mut self, cookie: &CanonicalCookie) {
            self.0
                .0
                .lock()
                .unwrap()
                .push(format!("add {}={}", cookie.name(), cookie.value()));
        }
        fn update_cookie_access_time(&mut self, cookie: &CanonicalCookie) {
            self.0
                .0
                .lock()
                .unwrap()
                .push(format!("use {}", cookie.name()));
        }
        fn delete_cookie(&mut self, cookie: &CanonicalCookie) {
            self.0
                .0
                .lock()
                .unwrap()
                .push(format!("delete {}={}", cookie.name(), cookie.value()));
        }
    }
    let log = Log::default();
    let mut jar = Jar {
        monster: CookieMonster::with_store(Box::new(Recording(log.clone()))),
    };
    let set = |jar: &mut Jar, now, line: &str| jar.set(now, "https://a.example/", line, None);
    set(&mut jar, 1, "a=1; Max-Age=3600");
    set(&mut jar, 2, "s=1");
    assert_eq!(log.take(), ["add a=1"]);
    set(&mut jar, 3, "a=2; Max-Age=3600");
    assert_eq!(log.take(), ["delete a=1", "add a=2"]);
    jar.line(30, "https://a.example/", None);
    assert!(log.take().is_empty());
    jar.line(70, "https://a.example/", None);
    assert_eq!(log.take(), ["use a"]);
    set(&mut jar, 71, "s=2; Max-Age=3600");
    set(&mut jar, 72, "a=3");
    assert_eq!(log.take(), ["add s=2", "delete a=2"]);
    set(&mut jar, 73, "e=1; Max-Age=10");
    jar.line(100, "https://a.example/", None);
    assert_eq!(log.take(), ["add e=1", "delete e=1"]);
    // Gone in a purge: the least recently used, the session cookie `a`
    // among them.
    for i in 0..DOMAIN_MAX_COOKIES - 1 {
        set(
            &mut jar,
            200 + i64::try_from(i).unwrap(),
            &format!("c{i}=1; Max-Age=3600"),
        );
    }
    let log = log.take();
    assert_eq!(log.len(), DOMAIN_MAX_COOKIES - 1 + 30);
    assert_eq!(log.last().unwrap(), "delete c28=1");
    assert!(log.contains(&"delete s=2".to_owned()));

    let log = Log::default();
    let mut monster = CookieMonster::with_store(Box::new(Recording(log.clone())));
    let cookie = CanonicalCookie::create(
        &url("https://a.example/"),
        "a=1; Max-Age=3600",
        at(0),
        None,
        None,
        CookieSourceType::Http,
    )
    .unwrap();
    monster.load(vec![cookie]);
    assert!(log.take().is_empty());
    assert_eq!(
        monster.cookie_line_for(&url("https://a.example/"), &http(), None, at(1)),
        Some("a=1".to_owned())
    );
}
