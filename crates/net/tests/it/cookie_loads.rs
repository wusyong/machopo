//! Cookies on loads: each hop sends the jar's cookies its URL gets, and
//! keeps what its response sets, by the request's trusted params (the
//! browser's), its credentials mode, the `SameSite` context they make and
//! the cookie policy (no third party's but partitioned ones).
//! `a.localhost` and `b.localhost` are two
//! sites, both trustworthy, so `Secure` cookies are set over HTTP.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer};
use net::{HostMappingRules, NetworkContext, NetworkContextParams};
use protocol::network::{CredentialsMode, RequestDestination, ResourceRequest, TrustedParams};

fn network() -> NetworkContext {
    network_in(None)
}

/// A network context whose cookies are kept in the folder `dir`, if any.
fn network_in(dir: Option<&std::path::Path>) -> NetworkContext {
    NetworkContext::start(NetworkContextParams {
        host_resolver_rules: HostMappingRules::parse("MAP *.localhost 127.0.0.1").unwrap(),
        data_directory: dir.map(std::path::Path::to_path_buf),
        ..Default::default()
    })
    .unwrap()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("net-cookie-loads")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn server() -> TestServer {
    let routes = [
        (
            "/set",
            Route::ok("text/plain", "set")
                .with_header("Set-Cookie", "strict=1; SameSite=Strict")
                .with_header("Set-Cookie", "lax=1; SameSite=Lax")
                .with_header("Set-Cookie", "unspecified=1")
                .with_header("Set-Cookie", "none=1; SameSite=None; Secure"),
        ),
        (
            "/redirect",
            Route::redirect(302, "/echo").with_header("Set-Cookie", "hop=1"),
        ),
        ("/echo", Route::ok("text/plain", "echo")),
        (
            "/third",
            Route::ok("text/plain", "third")
                .with_header("Set-Cookie", "third=1; SameSite=None; Secure"),
        ),
        (
            "/http-only",
            Route::ok("text/plain", "http-only").with_header("Set-Cookie", "only=1; HttpOnly"),
        ),
        (
            "/persistent",
            Route::ok("text/plain", "persistent")
                .with_header("Set-Cookie", "kept=1; Max-Age=3600")
                .with_header("Set-Cookie", "session=1"),
        ),
        (
            "/partitioned",
            Route::ok("text/plain", "partitioned")
                .with_header(
                    "Set-Cookie",
                    "chip=1; SameSite=None; Secure; Partitioned; Max-Age=3600",
                )
                .with_header("Set-Cookie", "plain=1; SameSite=None; Secure; Max-Age=3600"),
        ),
    ];
    TestServer::start(
        routes
            .into_iter()
            .map(|(path, route)| (path.to_owned(), route))
            .collect::<HashMap<_, _>>(),
    )
    .unwrap()
}

/// A first party's subresource request of `site`'s document.
fn first_party(site: &str) -> TrustedParams {
    TrustedParams {
        site_for_cookies: Some(format!("http://{site}")),
        top_frame_site: Some(format!("http://{site}")),
        initiator: Some(format!("http://{site}")),
        is_main_frame_navigation: false,
    }
}

/// Loads `request` and waits for it.
fn load(network: &NetworkContext, request: ResourceRequest) {
    let (done, answered) = unbounded();
    network.fetch(request, move |result| {
        let _ = done.send(result.is_ok());
    });
    assert!(
        answered
            .recv_timeout(Duration::from_secs(30))
            .expect("the network service answers")
    );
}

fn request(url: String, trusted: Option<TrustedParams>) -> ResourceRequest {
    let mut request = ResourceRequest::new(url, RequestDestination::Empty);
    request.trusted = trusted;
    request
}

/// The `Cookie` header of the last request `server` had for `target`,
/// its cookies' names sorted ("" for none).
fn sent(server: &TestServer, target: &str) -> String {
    let cookie = server
        .requests()
        .into_iter()
        .rfind(|request| request.target == target)
        .and_then(|request| request.header("cookie").map(str::to_owned))
        .unwrap_or_default();
    let mut names: Vec<&str> = cookie
        .split("; ")
        .filter(|pair| !pair.is_empty())
        .map(|pair| pair.split('=').next().unwrap_or_default())
        .collect();
    names.sort_unstable();
    names.join(" ")
}

/// What a first party's response sets, its next requests send; a
/// redirect's too, at the hop after it. A request without trusted params
/// (the browser's own) sends none and keeps none; nor does one that omits
/// credentials.
#[test]
fn a_first_partys_cookies_are_kept_and_sent() {
    let (network, server) = (network(), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    load(
        &network,
        request(a("/redirect"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "hop");
    load(
        &network,
        request(a("/set"), Some(first_party("a.localhost"))),
    );
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "hop lax none strict unspecified");
    load(&network, request(a("/echo"), None));
    assert_eq!(sent(&server, "/echo"), "");
    let mut omitting = request(a("/echo"), Some(first_party("a.localhost")));
    omitting.credentials = CredentialsMode::Omit;
    load(&network, omitting);
    assert_eq!(sent(&server, "/echo"), "");
    // Nor kept by one without trusted params.
    let other = server.url_for_host("b.localhost", "/set");
    load(&network, request(other, None));
    load(
        &network,
        request(
            server.url_for_host("b.localhost", "/echo"),
            Some(first_party("b.localhost")),
        ),
    );
    assert_eq!(sent(&server, "/echo"), "");
}

/// A `same-origin` request sends cookies to its initiator's origin, not
/// to another (its response tainted).
#[test]
fn same_origin_credentials_go_to_the_initiators_origin_alone() {
    let (network, server) = (network(), server());
    load(
        &network,
        request(
            server.url_for_host("a.localhost", "/set"),
            Some(first_party("a.localhost")),
        ),
    );
    let mut same_origin = request(
        server.url_for_host("a.localhost", "/echo"),
        Some(first_party("a.localhost")),
    );
    same_origin.credentials = CredentialsMode::SameOrigin;
    let port = url::Url::parse(&server.url("/")).unwrap().port().unwrap();
    same_origin.initiator = Some(format!("http://a.localhost:{port}"));
    load(&network, same_origin.clone());
    assert_eq!(sent(&server, "/echo"), "lax none strict unspecified");
    // From another origin of the site: CORS, and not its origin.
    same_origin.initiator = Some(format!("http://www.a.localhost:{port}"));
    load(&network, same_origin);
    assert_eq!(sent(&server, "/echo"), "");
}

/// `SameSite` by the context: a main frame's navigation from another site
/// gets `Lax` and unspecified ones with a safe method, not `Strict`; with
/// an unsafe one, `None` ones alone (no Lax-allowing-unsafe). The
/// browser's own navigation (no initiator) gets them all.
#[test]
fn same_site_by_the_requests_context() {
    let (network, server) = (network(), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    load(
        &network,
        request(a("/set"), Some(first_party("a.localhost"))),
    );
    let navigation = |initiator: Option<&str>| TrustedParams {
        site_for_cookies: None,
        top_frame_site: None,
        initiator: initiator.map(|site| format!("http://{site}")),
        is_main_frame_navigation: true,
    };
    load(
        &network,
        request(a("/echo"), Some(navigation(Some("b.localhost")))),
    );
    assert_eq!(sent(&server, "/echo"), "lax none unspecified");
    let mut posted = request(a("/echo"), Some(navigation(Some("b.localhost"))));
    posted.method = "POST".to_owned();
    load(&network, posted);
    assert_eq!(sent(&server, "/echo"), "none");
    load(&network, request(a("/echo"), Some(navigation(None))));
    assert_eq!(sent(&server, "/echo"), "lax none strict unspecified");
    // A subresource of a same-site page from another site's initiator:
    // cross-site, `None` ones alone.
    let mut from_b = first_party("a.localhost");
    from_b.initiator = Some("http://b.localhost".to_owned());
    load(&network, request(a("/echo"), Some(from_b)));
    assert_eq!(sent(&server, "/echo"), "none");
}

/// A third party's request (its site for cookies another site, or null)
/// sends no cookies and keeps none, `SameSite=None` ones neither. One
/// without trusted params has none either.
#[test]
fn a_third_partys_cookies_by_the_policy() {
    let (network, server) = (network(), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    load(
        &network,
        request(a("/set"), Some(first_party("a.localhost"))),
    );
    let third_party = |site_for_cookies: Option<&str>| TrustedParams {
        site_for_cookies: site_for_cookies.map(str::to_owned),
        top_frame_site: Some("http://b.localhost".to_owned()),
        initiator: Some("http://b.localhost".to_owned()),
        is_main_frame_navigation: false,
    };
    for site_for_cookies in [Some("http://b.localhost"), None] {
        load(
            &network,
            request(a("/echo"), Some(third_party(site_for_cookies))),
        );
        assert_eq!(sent(&server, "/echo"), "", "{site_for_cookies:?}");
    }
    // A request without trusted params (the browser's own) has none,
    // as any.
    load(&network, request(a("/echo"), None));
    assert_eq!(sent(&server, "/echo"), "");
    load(
        &network,
        request(a("/third"), Some(third_party(Some("http://b.localhost")))),
    );
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "lax none strict unspecified");
}

/// A third party's request in `top`'s top-level frame (its site for
/// cookies null).
fn embedded_in(top: &str) -> TrustedParams {
    TrustedParams {
        site_for_cookies: None,
        top_frame_site: Some(format!("http://{top}")),
        initiator: Some(format!("http://{top}")),
        is_main_frame_navigation: false,
    }
}

/// The profile's folder keeps the persistent cookies: a network context
/// started on it once the one before has gone sends them; not the session
/// ones.
#[test]
fn a_profiles_persistent_cookies_outlast_its_network_context() {
    let (dir, server) = (scratch("outlast"), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    {
        let network = network_in(Some(&dir));
        load(
            &network,
            request(a("/persistent"), Some(first_party("a.localhost"))),
        );
        load(
            &network,
            request(a("/echo"), Some(first_party("a.localhost"))),
        );
        assert_eq!(sent(&server, "/echo"), "kept session");
    }
    // Script's first read waits for them too.
    let network = network_in(Some(&dir));
    assert_eq!(
        script_cookies(&network, &a("/"), first_party("a.localhost")),
        "kept=1"
    );
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "kept");
}

/// A third party's `Partitioned` cookie is kept in the partition of its
/// top-level site: it goes with that site's third-party requests alone,
/// not another's nor the first party's own, and is kept in it in the
/// profile's folder too. An unpartitioned one isn't kept.
#[test]
fn a_third_partys_partitioned_cookies_are_its_top_level_sites() {
    let (dir, server) = (scratch("partitioned"), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    {
        let network = network_in(Some(&dir));
        load(
            &network,
            request(a("/partitioned"), Some(embedded_in("b.localhost"))),
        );
        load(
            &network,
            request(a("/echo"), Some(embedded_in("b.localhost"))),
        );
        assert_eq!(sent(&server, "/echo"), "chip");
        load(
            &network,
            request(a("/echo"), Some(embedded_in("c.localhost"))),
        );
        assert_eq!(sent(&server, "/echo"), "");
        load(
            &network,
            request(a("/echo"), Some(first_party("a.localhost"))),
        );
        assert_eq!(sent(&server, "/echo"), "");
    }
    let network = network_in(Some(&dir));
    load(
        &network,
        request(a("/echo"), Some(embedded_in("b.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "chip");
}

/// A partition minds whether a frame of another site is between: what a
/// site's own page keeps in its partition doesn't go with its requests
/// from a frame of another site in that page.
#[test]
fn a_partition_minds_a_cross_site_ancestor() {
    let (network, server) = (network(), server());
    let b = |path: &str| server.url_for_host("b.localhost", path);
    load(
        &network,
        request(b("/partitioned"), Some(first_party("b.localhost"))),
    );
    load(
        &network,
        request(b("/echo"), Some(first_party("b.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "chip plain");
    load(
        &network,
        request(b("/echo"), Some(embedded_in("b.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "");
}

/// What script reads at `url` from where `trusted` says, waiting for it.
fn script_cookies(network: &NetworkContext, url: &str, trusted: TrustedParams) -> String {
    let (done, answered) = unbounded();
    network.get_cookies_for_script(url::Url::parse(url).unwrap(), trusted, move |cookies| {
        let _ = done.send(cookies);
    });
    answered.recv_timeout(Duration::from_secs(30)).unwrap()
}

fn script_enabled(network: &NetworkContext, url: &str, trusted: TrustedParams) -> bool {
    let (done, answered) = unbounded();
    network.cookies_enabled_for(url::Url::parse(url).unwrap(), trusted, move |enabled| {
        let _ = done.send(enabled);
    });
    answered.recv_timeout(Duration::from_secs(30)).unwrap()
}

/// Script's cookies (`document.cookie`): a first party's reads every one
/// of its URL but `HttpOnly` ones (of every `SameSite`), and what it sets
/// it reads at once and its requests send; it sets no `HttpOnly` one, nor
/// one over an `HttpOnly` one.
#[test]
fn script_reads_and_sets_a_first_partys_cookies() {
    let (network, server) = (network(), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    load(
        &network,
        request(a("/set"), Some(first_party("a.localhost"))),
    );
    let set = |line: &str| {
        network.set_cookie_from_script(
            url::Url::parse(&a("/")).unwrap(),
            first_party("a.localhost"),
            line.to_owned(),
        );
    };
    set("script=1");
    set("strict=2; HttpOnly");
    set("hidden=1; HttpOnly");
    let names = |line: String| {
        let mut names: Vec<String> = line
            .split("; ")
            .filter(|pair| !pair.is_empty())
            .map(str::to_owned)
            .collect();
        names.sort_unstable();
        names.join(" ")
    };
    assert_eq!(
        names(script_cookies(
            &network,
            &a("/"),
            first_party("a.localhost")
        )),
        "lax=1 none=1 script=1 strict=1 unspecified=1"
    );
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "lax none script strict unspecified");

    // Over an `HttpOnly` one: no.
    load(
        &network,
        request(a("/http-only"), Some(first_party("a.localhost"))),
    );
    set("only=2");
    assert!(!script_cookies(&network, &a("/"), first_party("a.localhost")).contains("only"));
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert!(sent(&server, "/echo").contains("only"));
}

/// A third party's script reads and sets none but `Partitioned` ones, and
/// cookies aren't enabled for it; a first party's are.
#[test]
fn a_third_partys_script_by_the_policy() {
    let (network, server) = (network(), server());
    let a = server.url_for_host("a.localhost", "/");
    let in_b = || embedded_in("b.localhost");
    // A first party's `SameSite=None` cookie: a third party's script
    // doesn't read it.
    network.set_cookie_from_script(
        url::Url::parse(&a).unwrap(),
        first_party("a.localhost"),
        "first=1; SameSite=None; Secure".to_owned(),
    );
    for line in [
        "none=1; SameSite=None; Secure",
        "chip=1; SameSite=None; Secure; Partitioned",
        "lax=1",
    ] {
        network.set_cookie_from_script(url::Url::parse(&a).unwrap(), in_b(), line.to_owned());
    }
    let read = script_cookies(&network, &a, in_b());
    assert_eq!(read, "chip=1");
    // The partitioned one is of b's top-level site alone.
    assert_eq!(
        script_cookies(&network, &a, first_party("a.localhost")),
        "first=1"
    );
    assert!(!script_enabled(&network, &a, in_b()));
    assert!(script_enabled(&network, &a, first_party("a.localhost")));
}

fn waited<T: Send + 'static>(call: impl FnOnce(Box<dyn FnOnce(T) + Send>)) -> T {
    let (done, answered) = unbounded();
    call(Box::new(move |value| {
        let _ = done.send(value);
    }));
    answered.recv_timeout(Duration::from_secs(30)).unwrap()
}

fn names_of(cookies: &[net::cookies::CanonicalCookie]) -> Vec<String> {
    let mut names: Vec<String> = cookies
        .iter()
        .map(|cookie| cookie.name().to_owned())
        .collect();
    names.sort_unstable();
    names
}

/// The browser's cookie calls (DevTools', automation's): a URL's cookies of
/// any context (`HttpOnly` and `Strict` too), and of the partition of the
/// top-level frame given, each once, not marked as used; every cookie;
/// cookies set as from their own `https:` URL, each said set or not; and
/// those a filter takes deleted (by name, exact domain, path, partition).
#[test]
fn the_browsers_cookie_calls() {
    use net::cookies::{
        CanonicalCookie, CookieDeletionFilter, CookieSameSite, PartitionFilter, Time,
    };
    let (network, server) = (network(), server());
    let a = |path: &str| server.url_for_host("a.localhost", path);
    load(
        &network,
        request(a("/set"), Some(first_party("a.localhost"))),
    );
    load(
        &network,
        request(a("/http-only"), Some(first_party("a.localhost"))),
    );
    network.set_cookie_from_script(
        url::Url::parse(&a("/")).unwrap(),
        embedded_in("b.localhost"),
        "chip=1; SameSite=None; Secure; Partitioned".to_owned(),
    );
    let url_a = url::Url::parse(&a("/")).unwrap();
    let list = |top: Option<&str>| {
        let urls = vec![url_a.clone(), url_a.clone()];
        let top = top.map(|top| url::Url::parse(top).unwrap());
        waited(|done| network.get_cookie_list(urls, top, done))
    };
    assert_eq!(
        names_of(&list(None)),
        ["lax", "none", "only", "strict", "unspecified"]
    );
    assert_eq!(
        names_of(&list(Some("http://b.localhost/"))),
        ["chip", "lax", "none", "only", "strict", "unspecified"]
    );
    let all: Vec<CanonicalCookie> = waited(|done| network.get_all_cookies(done));
    assert_eq!(all.len(), 6);
    // Read, not used: the times are as they were.
    let again: Vec<CanonicalCookie> = waited(|done| network.get_all_cookies(done));
    assert_eq!(all, again);

    let set = |cookie| waited(|done| network.set_canonical_cookies(vec![cookie], done));
    let made = |name: &str, http_only| {
        CanonicalCookie::create_sanitized(
            &url::Url::parse("http://a.localhost").unwrap(),
            name,
            "1",
            "",
            "",
            Time::now(),
            None,
            false,
            http_only,
            CookieSameSite::StrictMode,
            None,
        )
        .unwrap()
    };
    assert_eq!(set(made("devtools", true)), [true]);
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert!(sent(&server, "/echo").contains("devtools"));
    // Over an `HttpOnly` one: they may (HTTP's context).
    assert_eq!(set(made("only", false)), [true]);

    let delete = |filter| waited(|done| network.delete_cookies(filter, done));
    let filter = |name: Option<&str>, domain: Option<&str>, partition| CookieDeletionFilter {
        name: name.map(str::to_owned),
        domain: domain.map(str::to_owned),
        path: None,
        partition,
    };
    assert_eq!(
        delete(filter(
            Some("lax"),
            Some(".a.localhost"),
            PartitionFilter::Any
        )),
        0
    );
    assert_eq!(
        delete(filter(
            Some("lax"),
            Some("a.localhost"),
            PartitionFilter::Any
        )),
        1
    );
    assert_eq!(
        delete(filter(Some("chip"), None, PartitionFilter::Unpartitioned)),
        0
    );
    let partition = list(Some("http://b.localhost/"))
        .into_iter()
        .find(|cookie| cookie.name() == "chip")
        .and_then(|cookie| cookie.partition_key().cloned())
        .unwrap();
    let other = net::cookies::CookiePartitionKey::new(
        site::Site::of_url(&url::Url::parse("http://c.localhost").unwrap()),
        site::AncestorChainBit::CrossSite,
    );
    assert_eq!(
        delete(filter(Some("chip"), None, PartitionFilter::Of(other))),
        0
    );
    assert_eq!(
        delete(filter(Some("chip"), None, PartitionFilter::Of(partition))),
        1
    );
    assert_eq!(
        delete(CookieDeletionFilter {
            path: Some("/p".to_owned()),
            ..filter(None, None, PartitionFilter::Any)
        }),
        0
    );
    assert_eq!(delete(CookieDeletionFilter::all()), 5);
    let all: Vec<CanonicalCookie> = waited(|done| network.get_all_cookies(done));
    assert!(all.is_empty());
}
