//! Cookies on loads: each hop sends the jar's cookies its URL gets, and
//! keeps what its response sets, by the request's trusted params (the
//! browser's), its credentials mode, the `SameSite` context they make and
//! the cookie policy (machopo's: no third party's; or Chromium's, with
//! `allow_third_party_cookies`). `a.localhost` and `b.localhost` are two
//! sites, both trustworthy, so `Secure` cookies are set over HTTP.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer};
use net::{HostMappingRules, NetworkContext, NetworkContextParams};
use protocol::network::{CredentialsMode, RequestDestination, ResourceRequest, TrustedParams};

fn network(allow_third_party_cookies: bool) -> NetworkContext {
    network_in(allow_third_party_cookies, None)
}

/// A network context whose cookies are kept in the folder `dir`, if any.
fn network_in(allow_third_party_cookies: bool, dir: Option<&std::path::Path>) -> NetworkContext {
    NetworkContext::start(NetworkContextParams {
        host_resolver_rules: HostMappingRules::parse("MAP *.localhost 127.0.0.1").unwrap(),
        allow_third_party_cookies,
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
    let (network, server) = (network(false), server());
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
    let (network, server) = (network(false), server());
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
/// an unsafe one, `None` ones alone (no Lax-allowing-unsafe in machopo's
/// policy, but with Chromium's for a cookie two minutes old). The
/// browser's own navigation (no initiator) gets them all.
#[test]
fn same_site_by_the_requests_context() {
    for allow in [false, true] {
        let (network, server) = (network(allow), server());
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
        assert_eq!(
            sent(&server, "/echo"),
            "lax none unspecified",
            "allow {allow}"
        );
        let mut posted = request(a("/echo"), Some(navigation(Some("b.localhost"))));
        posted.method = "POST".to_owned();
        load(&network, posted);
        assert_eq!(
            sent(&server, "/echo"),
            if allow { "none unspecified" } else { "none" },
            "allow {allow}"
        );
        load(&network, request(a("/echo"), Some(navigation(None))));
        assert_eq!(
            sent(&server, "/echo"),
            "lax none strict unspecified",
            "allow {allow}"
        );
        // A subresource of a same-site page from another site's initiator:
        // cross-site, `None` ones alone.
        let mut from_b = first_party("a.localhost");
        from_b.initiator = Some("http://b.localhost".to_owned());
        load(&network, request(a("/echo"), Some(from_b)));
        assert_eq!(sent(&server, "/echo"), "none", "allow {allow}");
    }
}

/// A third party's request (its site for cookies another site, or null)
/// sends no cookies and keeps none in machopo's policy; in Chromium's,
/// `SameSite=None` ones go and are kept. One without trusted params has
/// none in either.
#[test]
fn a_third_partys_cookies_by_the_policy() {
    for allow in [false, true] {
        let (network, server) = (network(allow), server());
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
            assert_eq!(
                sent(&server, "/echo"),
                if allow { "none" } else { "" },
                "allow {allow}, {site_for_cookies:?}"
            );
        }
        // A request without trusted params (the browser's own) has none,
        // whatever the policy.
        load(&network, request(a("/echo"), None));
        assert_eq!(sent(&server, "/echo"), "", "allow {allow}");
        load(
            &network,
            request(a("/third"), Some(third_party(Some("http://b.localhost")))),
        );
        load(
            &network,
            request(a("/echo"), Some(first_party("a.localhost"))),
        );
        assert_eq!(
            sent(&server, "/echo"),
            if allow {
                "lax none strict third unspecified"
            } else {
                "lax none strict unspecified"
            },
            "allow {allow}"
        );
    }
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
        let network = network_in(false, Some(&dir));
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
    let network = network_in(false, Some(&dir));
    load(
        &network,
        request(a("/echo"), Some(first_party("a.localhost"))),
    );
    assert_eq!(sent(&server, "/echo"), "kept");
}

/// A third party's `Partitioned` cookie is kept, in either policy, in the
/// partition of its top-level site: it goes with that site's third-party
/// requests alone, not another's nor the first party's own, and is kept in
/// it in the profile's folder too. An unpartitioned one is the policy's.
#[test]
fn a_third_partys_partitioned_cookies_are_its_top_level_sites() {
    for allow in [false, true] {
        let (dir, server) = (scratch(&format!("partitioned-{allow}")), server());
        let a = |path: &str| server.url_for_host("a.localhost", path);
        {
            let network = network_in(allow, Some(&dir));
            load(
                &network,
                request(a("/partitioned"), Some(embedded_in("b.localhost"))),
            );
            load(
                &network,
                request(a("/echo"), Some(embedded_in("b.localhost"))),
            );
            assert_eq!(
                sent(&server, "/echo"),
                if allow { "chip plain" } else { "chip" },
                "allow {allow}"
            );
            load(
                &network,
                request(a("/echo"), Some(embedded_in("c.localhost"))),
            );
            assert_eq!(
                sent(&server, "/echo"),
                if allow { "plain" } else { "" },
                "allow {allow}"
            );
            load(
                &network,
                request(a("/echo"), Some(first_party("a.localhost"))),
            );
            assert_eq!(
                sent(&server, "/echo"),
                if allow { "plain" } else { "" },
                "allow {allow}"
            );
        }
        let network = network_in(allow, Some(&dir));
        load(
            &network,
            request(a("/echo"), Some(embedded_in("b.localhost"))),
        );
        assert_eq!(
            sent(&server, "/echo"),
            if allow { "chip plain" } else { "chip" },
            "allow {allow}"
        );
    }
}

/// A partition minds whether a frame of another site is between: what a
/// site's own page keeps in its partition doesn't go with its requests
/// from a frame of another site in that page.
#[test]
fn a_partition_minds_a_cross_site_ancestor() {
    for allow in [false, true] {
        let (network, server) = (network(allow), server());
        let b = |path: &str| server.url_for_host("b.localhost", path);
        load(
            &network,
            request(b("/partitioned"), Some(first_party("b.localhost"))),
        );
        load(
            &network,
            request(b("/echo"), Some(first_party("b.localhost"))),
        );
        assert_eq!(sent(&server, "/echo"), "chip plain", "allow {allow}");
        load(
            &network,
            request(b("/echo"), Some(embedded_in("b.localhost"))),
        );
        assert_eq!(
            sent(&server, "/echo"),
            if allow { "plain" } else { "" },
            "allow {allow}"
        );
    }
}
