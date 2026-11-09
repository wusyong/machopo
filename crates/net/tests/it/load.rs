//! Loads end to end through the `NetworkService` thread, against a local
//! test server: nothing here leaves the machine.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer, unused_port};
use net::{NetworkContext, NetworkContextParams};
use protocol::network::{
    CredentialsMode, LoadResult, NetErrorCode, RedirectMode, RequestDestination, RequestMode,
    ResourceRequest, ResponseTainting,
};

/// The user agent the network service is started with here.
const AGENT: &str = "machopo-test/1.0";

/// The service's parameters here: a user agent, nothing else.
fn params() -> NetworkContextParams {
    NetworkContextParams {
        user_agent: AGENT.into(),
        ..Default::default()
    }
}

fn fetch(network: &NetworkContext, url: &str, destination: RequestDestination) -> LoadResult {
    load(network, ResourceRequest::new(url, destination))
}

/// What the service answers `request`.
fn load(network: &NetworkContext, request: ResourceRequest) -> LoadResult {
    let (tx, rx) = unbounded();
    network.fetch(request, move |result| {
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the network service answers")
}

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

#[test]
fn http_response_with_charset_and_headers() {
    let server = server(&[(
        "/page",
        Route::ok("text/HTML; charset=\"Shift_JIS\"", "<p>hi").with_header("X-Test", "1"),
    )]);
    let network = NetworkContext::start(params()).unwrap();
    let response = fetch(&network, &server.url("/page"), RequestDestination::Document).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.url, server.url("/page"));
    assert_eq!(response.mime_type, "text/html");
    assert_eq!(response.charset.as_deref(), Some("Shift_JIS"));
    assert_eq!(response.body, &b"<p>hi"[..]);
    assert!(response.headers.contains(&("x-test".into(), "1".into())));

    // What we sent: an Accept for the destination, and the User-Agent the
    // network service was started with (the browser's).
    let request = &server.requests()[0];
    assert!(request.header("accept").unwrap().starts_with("text/html"));
    assert_eq!(request.header("user-agent"), Some(AGENT));
}

#[test]
fn error_statuses_still_have_a_body() {
    let server = server(&[]);
    let network = NetworkContext::start(params()).unwrap();
    let response = fetch(
        &network,
        &server.url("/missing"),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!(response.status, 404);
    assert_eq!(response.body, &b"not found"[..]);
}

#[test]
fn missing_content_type_is_sniffed() {
    let server = server(&[
        ("/markup", Route::new(200, &b"\n <html>"[..])),
        ("/text", Route::new(200, &b"plain"[..])),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let markup = fetch(
        &network,
        &server.url("/markup"),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!(markup.mime_type, "text/html");
    let text = fetch(&network, &server.url("/text"), RequestDestination::Document).unwrap();
    assert_eq!(text.mime_type, "text/plain");
}

#[test]
fn redirects_are_followed_and_keep_the_fragment() {
    let server = server(&[
        ("/a", Route::redirect(301, "/b")),
        ("/b", Route::redirect(302, "c?x=1")),
        ("/c", Route::ok("text/plain", "end")),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let response = fetch(
        &network,
        &server.url("/a#top"),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!(response.url, server.url("/c?x=1#top"));
    assert_eq!(response.body, &b"end"[..]);
    let targets: Vec<_> = server.requests().into_iter().map(|r| r.target).collect();
    // The fragment never goes on the wire.
    assert_eq!(targets, ["/a", "/b", "/c?x=1"]);
}

#[test]
fn redirect_limits() {
    let server = server(&[
        ("/loop", Route::redirect(302, "/loop")),
        ("/to-file", Route::redirect(302, "file:///etc/passwd")),
        ("/no-location", Route::new(302, &b"stay"[..])),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let looped = fetch(&network, &server.url("/loop"), RequestDestination::Document).unwrap_err();
    assert_eq!(looped.code, NetErrorCode::TooManyRedirects);
    // The first request plus 20 redirects, as Chromium allows.
    assert_eq!(server.requests().len(), 21);

    let to_file = fetch(
        &network,
        &server.url("/to-file"),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(to_file.code, NetErrorCode::UnsafeRedirect);

    let stay = fetch(
        &network,
        &server.url("/no-location"),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!((stay.status, &stay.body[..]), (302, &b"stay"[..]));
}

#[test]
fn connection_failures() {
    let network = NetworkContext::start(params()).unwrap();
    let refused = fetch(
        &network,
        &format!("http://127.0.0.1:{}/", unused_port()),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(refused.code, NetErrorCode::ConnectionRefused, "{refused:?}");

    // `.invalid` never resolves (RFC 6761).
    let unresolved = fetch(
        &network,
        "http://machopo.invalid/",
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(
        unresolved.code,
        NetErrorCode::NameNotResolved,
        "{unresolved:?}"
    );
}

#[test]
fn files_and_other_schemes() {
    let dir = std::env::temp_dir().join(format!("machopo-net-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let css = dir.join("a b.css");
    std::fs::write(&css, "p {}").unwrap();
    let network = NetworkContext::start(params()).unwrap();

    let url = net::file_url(&css).unwrap();
    let response = fetch(&network, &url, RequestDestination::Style).unwrap();
    assert_eq!(response.mime_type, "text/css");
    assert_eq!(response.body, &b"p {}"[..]);

    let missing = fetch(
        &network,
        &net::file_url(&dir.join("missing.html")).unwrap(),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(missing.code, NetErrorCode::FileNotFound);

    // A directory: a page listing it, directories first, by name.
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let listing = fetch(
        &network,
        &net::file_url(&dir).unwrap(),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!(listing.mime_type, "text/html");
    let page = String::from_utf8(listing.body.to_vec()).unwrap();
    let sub = page.find(">sub/</a>").expect("the directory");
    let file = page.find(">a b.css</a>").expect("the file");
    assert!(sub < file, "{page}");
    assert!(page.contains(&format!("href=\"{}\"", net::file_url(&css).unwrap())));

    let unknown = fetch(
        &network,
        "gopher://example.com/",
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(unknown.code, NetErrorCode::UnknownUrlScheme);
    let invalid = fetch(&network, "not a url", RequestDestination::Document).unwrap_err();
    assert_eq!(invalid.code, NetErrorCode::InvalidUrl);
    let _ = std::fs::remove_dir_all(&dir);
}

/// WPT's test certificates (`tools/certs` of the checkout next to the
/// repo): `web-platform.test`'s, and the root that signs it, which web
/// tests trust.
fn wpt_cert(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../third_party/wpt/tools/certs")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// What web tests set: WPT's domains to this machine (the rules wptrunner
/// gives Chrome), and WPT's root trusted.
fn web_test_params() -> NetworkContextParams {
    let mut params = NetworkContextParams {
        host_resolver_rules: net::HostMappingRules::parse(
            "MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1",
        )
        .unwrap(),
        ..Default::default()
    };
    params
        .add_trust_anchors_pem(&wpt_cert("cacert.pem"))
        .unwrap();
    params
}

fn https_server() -> TestServer {
    TestServer::start_https(
        [
            ("/page".to_string(), Route::ok("text/plain", "secure")),
            (
                "/until-close".to_string(),
                Route::ok("text/plain", "until the end").close_delimited(),
            ),
            (
                "/corrupt".to_string(),
                Route::ok("text/plain", "then garbage")
                    .close_delimited()
                    .corrupt_after_body(),
            ),
            (
                "/cut-short".to_string(),
                Route::ok("text/plain", "cut")
                    .with_header("Content-Length", "100")
                    .close_delimited(),
            ),
        ]
        .into_iter()
        .collect(),
        &wpt_cert("web-platform.test.pem"),
        &wpt_cert("web-platform.test.key"),
    )
    .expect("test server starts")
}

#[test]
fn host_mapping_rules() {
    let server = server(&[("/page", Route::ok("text/plain", "mapped"))]);
    let network = NetworkContext::start(web_test_params()).unwrap();
    let get = |host: &str| {
        fetch(
            &network,
            &server.url_for_host(host, "/page"),
            RequestDestination::Document,
        )
    };
    for host in ["web-platform.test", "www.www1.not-web-platform.test"] {
        assert_eq!(get(host).unwrap().body, &b"mapped"[..], "{host}");
    }
    // The first matching rule decides: `nonexistent` doesn't resolve,
    // though `*.test` would map it.
    let missing = get("nonexistent.web-platform.test").unwrap_err();
    assert_eq!(missing.code, NetErrorCode::NameNotResolved, "{missing:?}");
    assert_eq!(server.requests().len(), 2);
    // Hosts no rule matches still go to the system.
    assert_eq!(get("localhost").unwrap().body, &b"mapped"[..]);
    let unresolved = fetch(
        &network,
        "http://machopo.invalid/",
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(unresolved.code, NetErrorCode::NameNotResolved);

    // Without rules, `.test` is the system's, which never resolves it
    // (RFC 6761).
    let plain = NetworkContext::start(NetworkContextParams::default()).unwrap();
    let unmapped = fetch(
        &plain,
        &server.url_for_host("web-platform.test", "/page"),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(unmapped.code, NetErrorCode::NameNotResolved, "{unmapped:?}");
}

#[test]
fn additional_trust_anchors() {
    let server = https_server();
    let network = NetworkContext::start(web_test_params()).unwrap();
    let url = server.url_for_host("www.web-platform.test", "/page");
    let response = fetch(&network, &url, RequestDestination::Document).unwrap();
    assert_eq!((response.status, &response.body[..]), (200, &b"secure"[..]));

    // The certificate is still checked: its names don't include the
    // address.
    let by_address =
        fetch(&network, &server.url("/page"), RequestDestination::Document).unwrap_err();
    assert_eq!(
        by_address.code,
        NetErrorCode::CertCommonNameInvalid,
        "{by_address:?}"
    );

    // Without the extra root, WPT's certificate has an unknown issuer.
    let untrusted = NetworkContext::start(NetworkContextParams {
        additional_trust_anchors: Vec::new(),
        ..web_test_params()
    })
    .unwrap();
    let error = fetch(&untrusted, &url, RequestDestination::Document).unwrap_err();
    assert_eq!(error.code, NetErrorCode::CertAuthorityInvalid, "{error:?}");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn trust_anchors_come_from_pem() {
    let mut params = NetworkContextParams::default();
    params
        .add_trust_anchors_pem(&wpt_cert("cacert.pem"))
        .unwrap();
    assert_eq!(params.additional_trust_anchors.len(), 1);
    // A key is no certificate, and neither is nothing.
    assert!(
        params
            .add_trust_anchors_pem(&wpt_cert("cacert.key"))
            .is_err()
    );
    assert!(params.add_trust_anchors_pem(b"").is_err());
    assert_eq!(params.additional_trust_anchors.len(), 1);
}

/// A body that ends where the connection does, closed without TLS's
/// close_notify (as wptserve does), is complete, as in Chromium; over
/// plain HTTP too.
#[test]
fn bodies_that_end_with_the_connection() {
    let secure = https_server();
    let network = NetworkContext::start(web_test_params()).unwrap();
    let url = secure.url_for_host("web-platform.test", "/until-close");
    let response = fetch(&network, &url, RequestDestination::Document).unwrap();
    assert_eq!(response.body, &b"until the end"[..]);
    // With a length, the connection ending first is still an error.
    let short = fetch(
        &network,
        &secure.url_for_host("web-platform.test", "/cut-short"),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(short.code, NetErrorCode::Failed, "{short:?}");
    // Only the connection ending ends such a body: a TLS error is one.
    let corrupt = fetch(
        &network,
        &secure.url_for_host("web-platform.test", "/corrupt"),
        RequestDestination::Document,
    )
    .unwrap_err();
    assert_eq!(corrupt.code, NetErrorCode::SslProtocolError, "{corrupt:?}");

    let plain = server(&[(
        "/until-close",
        Route::ok("text/plain", "plain end").close_delimited(),
    )]);
    let response = fetch(
        &network,
        &plain.url("/until-close"),
        RequestDestination::Document,
    )
    .unwrap();
    assert_eq!(response.body, &b"plain end"[..]);
}

/// `fetch()`'s request: script's headers go out as given (an `Accept`
/// among them taking the place of the destination's), each character of a
/// value one byte; the response says whether it was redirected, and its
/// status's reason phrase.
#[test]
fn a_fetch_sends_its_headers() {
    let server = server(&[
        ("/plain", Route::ok("text/plain", "here")),
        ("/moved", Route::redirect(307, "/plain")),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let mut request = ResourceRequest::new(server.url("/moved"), RequestDestination::Empty);
    request.headers = vec![
        ("X-One".into(), "caf\u{e9}".into()),
        ("Accept".into(), "text/x-machopo".into()),
    ];
    let response = load(&network, request).unwrap();
    assert_eq!(response.url, server.url("/plain"));
    assert!(response.redirected);
    assert_eq!(response.status_text, "OK");
    let plain = server
        .requests()
        .into_iter()
        .find(|r| r.target == "/plain")
        .unwrap();
    assert_eq!(plain.header("accept"), Some("text/x-machopo"));
    // Latin-1 on the wire, which the server reads back as such.
    assert_eq!(plain.header("x-one"), Some("caf\u{e9}"));
    let response = fetch(&network, &server.url("/plain"), RequestDestination::Empty).unwrap();
    assert!(!response.redirected);
    let plain = server.requests().pop().unwrap();
    assert_eq!(plain.header("accept"), Some("*/*"));
}

/// A request of script's (with an initiator, `origin`) of `url`, in
/// `mode`.
fn script(url: String, origin: &str, mode: RequestMode) -> ResourceRequest {
    let mut request = ResourceRequest::new(url, RequestDestination::Empty);
    request.initiator = Some(origin.to_owned());
    request.mode = mode;
    request.credentials = CredentialsMode::SameOrigin;
    request
}

/// The origin of `server`, serialized.
fn origin_of(server: &TestServer) -> String {
    server.url("").trim_end_matches('/').to_owned()
}

/// A CORS request to another origin: sent with `Origin`, an error unless
/// that origin allows it (`*`, or this origin, not another, and with credentials
/// `Access-Control-Allow-Credentials: true`, never `*`); its response
/// comes with all its headers but `Set-Cookie` (the renderer filters what
/// script sees, as Blink does). One whose preflight the
/// other origin doesn't answer for goes no further than the `OPTIONS`; one
/// with a forbidden header goes nowhere, whatever the renderer said.
#[test]
fn cors_requests() {
    let page = server(&[]);
    let origin = origin_of(&page);
    let headers = |route: Route| {
        route
            .with_header("Cache-Control", "no-store")
            .with_header("X-Secret", "1")
            .with_header("X-Other", "2")
            .with_header("Set-Cookie", "a=b")
    };
    let other = server(&[
        ("/closed", headers(Route::ok("text/plain", "secret"))),
        (
            "/open",
            headers(Route::ok("text/plain", "open"))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/exposed",
            headers(Route::ok("text/plain", "exposed"))
                .with_header("Access-Control-Allow-Origin", &origin)
                .with_header("Access-Control-Expose-Headers", "x-secret, Set-Cookie"),
        ),
        (
            "/star",
            headers(Route::ok("text/plain", "star"))
                .with_header("Access-Control-Allow-Origin", "*")
                .with_header("Access-Control-Expose-Headers", "*"),
        ),
        (
            "/elsewhere",
            headers(Route::ok("text/plain", "elsewhere"))
                .with_header("Access-Control-Allow-Origin", "http://example.com"),
        ),
        (
            "/credentialed",
            headers(Route::ok("text/plain", "credentialed"))
                .with_header("Access-Control-Allow-Origin", &origin)
                .with_header("Access-Control-Allow-Credentials", "true")
                .with_header("Access-Control-Expose-Headers", "*"),
        ),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let cors = |path: &str| script(other.url(path), &origin, RequestMode::Cors);
    let names = |response: &protocol::network::ResourceResponse| -> Vec<String> {
        response
            .headers
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    };
    assert_eq!(
        load(&network, cors("/closed")).unwrap_err().code,
        NetErrorCode::Failed
    );
    assert!(load(&network, cors("/elsewhere")).is_err());
    let open = load(&network, cors("/open")).unwrap();
    assert_eq!(open.tainting, ResponseTainting::Cors);
    assert_eq!(open.body, &b"open"[..]);
    assert_eq!(
        names(&open),
        [
            "connection",
            "content-length",
            "content-type",
            "cache-control",
            "x-secret",
            "x-other",
            "access-control-allow-origin"
        ]
    );
    let exposed = load(&network, cors("/exposed")).unwrap();
    assert!(names(&exposed).contains(&"x-other".to_owned()));
    assert!(!names(&exposed).contains(&"set-cookie".to_owned()));
    let with_credentials = |path: &str| {
        let mut request = cors(path);
        request.credentials = CredentialsMode::Include;
        load(&network, request)
    };
    assert!(with_credentials("/star").is_err());
    assert!(with_credentials("/exposed").is_err());
    // Allowed with credentials; what script sees of it is the renderer's
    // to filter (`protocol::network::cors_filtered_headers`).
    with_credentials("/credentialed").unwrap();
    let origins: Vec<Option<String>> = other
        .requests()
        .iter()
        .map(|r| r.header("origin").map(str::to_owned))
        .collect();
    assert!(
        origins
            .iter()
            .all(|o| o.as_deref() == Some(origin.as_str())),
        "{origins:?}"
    );
    let sent = other.requests().len();
    let mut preflighted = cors("/open");
    preflighted.headers = vec![("X-Machopo".into(), "1".into())];
    assert!(load(&network, preflighted).is_err());
    let mut put = cors("/open");
    put.method = "PUT".into();
    assert!(load(&network, put).is_err());
    let mut forbidden = cors("/open");
    forbidden.headers = vec![("Cookie".into(), "a=b".into())];
    assert!(load(&network, forbidden).is_err());
    let methods: Vec<String> = other.requests()[sent..]
        .iter()
        .map(|r| r.method.clone())
        .collect();
    assert_eq!(methods, ["OPTIONS", "OPTIONS"]);
}

/// A CORS request whose method isn't safelisted, or with unsafe headers,
/// goes after a preflight: an `OPTIONS` with `Origin`, `Accept: */*`,
/// `Access-Control-Request-Method` and `-Headers` (lowercase, sorted), no
/// body or script's headers. The preflight passes the CORS check, with an
/// ok status (not a redirect, even one that allows the request), and
/// allows the method and each unsafe header (`*` without credentials;
/// `Authorization` by name only); then the request goes, the preflight not
/// cached. A request to its own origin has none.
#[test]
fn preflights() {
    let page = server(&[("/mine", Route::ok("text/plain", "mine"))]);
    let origin = origin_of(&page);
    let allow = |methods: &str, headers: &str| {
        Route::new(204, "")
            .with_header("Access-Control-Allow-Origin", &origin)
            .with_header("Access-Control-Allow-Credentials", "true")
            .with_header("Access-Control-Allow-Methods", methods)
            .with_header("Access-Control-Allow-Headers", headers)
    };
    let answer =
        Route::ok("text/plain", "done").with_header("Access-Control-Allow-Origin", &origin);
    let other = server(&[
        ("OPTIONS /put", allow("PUT", "")),
        ("/put", answer.clone()),
        ("OPTIONS /headers", allow("", "X-Machopo, content-type")),
        ("/headers", answer.clone()),
        ("OPTIONS /star", allow("*", "*")),
        (
            "/star",
            answer
                .clone()
                .with_header("Access-Control-Allow-Credentials", "true"),
        ),
        ("OPTIONS /moved", Route::redirect(307, "/put")),
        ("OPTIONS /allowed-redirect", {
            // Allowing all it should, but a redirect.
            let mut redirect = allow("PUT", "").with_header("Location", "/put");
            redirect.status = 302;
            redirect
        }),
        ("/allowed-redirect", Route::ok("text/plain", "done")),
        ("/moved", answer.clone()),
        (
            "OPTIONS /closed",
            Route::new(204, "").with_header("Access-Control-Allow-Methods", "PUT"),
        ),
        ("/closed", answer.clone()),
        ("OPTIONS /bad", allow("PUT, (bad)", "")),
        ("/bad", answer.clone()),
        ("OPTIONS /lowercase", allow("put", "")),
        ("/lowercase", answer),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let request = |path: &str, method: &str, headers: &[(&str, &str)]| {
        let mut request = script(other.url(path), &origin, RequestMode::Cors);
        request.method = method.into();
        request.headers = headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        request
    };
    let last_two = || {
        let requests = other.requests();
        requests[requests.len() - 2..].to_vec()
    };
    assert_eq!(
        load(&network, request("/put", "PUT", &[])).unwrap().body,
        &b"done"[..]
    );
    let [preflight, put] = <[_; 2]>::try_from(last_two()).unwrap();
    assert_eq!(
        (preflight.method.as_str(), put.method.as_str()),
        ("OPTIONS", "PUT")
    );
    assert_eq!(
        preflight.header("access-control-request-method"),
        Some("PUT")
    );
    assert_eq!(preflight.header("access-control-request-headers"), None);
    assert_eq!(preflight.header("origin"), Some(origin.as_str()));
    assert_eq!(preflight.header("accept"), Some("*/*"));
    // Not cached: another preflight each time.
    load(&network, request("/put", "PUT", &[])).unwrap();
    assert_eq!(last_two()[0].method, "OPTIONS");
    let headers = [
        ("X-Machopo", "1"),
        ("Content-Type", "application/json"),
        ("X-Other", "2"),
    ];
    assert!(load(&network, request("/headers", "POST", &headers)).is_err());
    assert_eq!(
        other
            .requests()
            .last()
            .unwrap()
            .header("access-control-request-headers"),
        Some("content-type,x-machopo,x-other")
    );
    let headers = [("X-Machopo", "1"), ("Content-Type", "application/json")];
    assert_eq!(
        load(&network, request("/headers", "POST", &headers))
            .unwrap()
            .body,
        &b"done"[..]
    );
    // A safelisted method needs no allowing.
    let preflight = &last_two()[0];
    assert_eq!(
        preflight.header("access-control-request-method"),
        Some("POST")
    );
    assert!(preflight.body.is_empty());
    assert_eq!(preflight.header("x-machopo"), None);
    // `*` allows any method and header but `Authorization`, and only
    // without credentials.
    assert!(load(&network, request("/star", "DELETE", &[("X-Machopo", "1")])).is_ok());
    assert!(load(&network, request("/star", "PUT", &[("Authorization", "a")])).is_err());
    let mut credentialed = request("/star", "PUT", &[]);
    credentialed.credentials = CredentialsMode::Include;
    assert!(load(&network, credentialed).is_err());
    let mut credentialed = request("/star", "POST", &[("X-Machopo", "1")]);
    credentialed.credentials = CredentialsMode::Include;
    assert!(load(&network, credentialed).is_err());
    // Not allowed: another method, a redirected or uncheckable preflight,
    // a list that doesn't parse, a method's case.
    assert!(load(&network, request("/put", "DELETE", &[])).is_err());
    for path in [
        "/moved",
        "/allowed-redirect",
        "/closed",
        "/bad",
        "/lowercase",
    ] {
        assert!(load(&network, request(path, "PUT", &[])).is_err(), "{path}");
    }
    // A request to its own origin needs no preflight.
    let mut mine = script(page.url("/mine"), &origin, RequestMode::Cors);
    mine.method = "PUT".into();
    mine.headers = vec![("X-Machopo".into(), "1".into())];
    assert_eq!(load(&network, mine).unwrap().body, &b"mine"[..]);
    let methods: Vec<String> = page.requests().iter().map(|r| r.method.clone()).collect();
    assert_eq!(methods, ["PUT"]);
    let done: Vec<String> = other
        .requests()
        .iter()
        .filter(|r| r.method != "OPTIONS")
        .map(|r| r.target.clone())
        .collect();
    assert_eq!(done, ["/put", "/put", "/headers", "/star"]);
}

/// The other modes: a `no-cors` request to another origin gets an opaque
/// response (a `fetch()`'s without its body, its status and headers for
/// the renderer to hide from script; a subresource's with its body), and
/// must follow its redirects; a `same-origin` one is an
/// error there, sent nowhere. A `cors` request of a `file:` URL is an
/// error. A request to its own origin, or of a `data:`
/// URL (in any mode), is basic, a `GET` without `Origin`.
#[test]
fn no_cors_and_same_origin_modes() {
    let page = server(&[("/mine", Route::ok("text/plain", "mine"))]);
    let origin = origin_of(&page);
    let other = server(&[(
        "/data",
        Route::ok("text/plain", "secret").with_header("X-Secret", "1"),
    )]);
    let network = NetworkContext::start(params()).unwrap();
    let opaque = load(
        &network,
        script(other.url("/data"), &origin, RequestMode::NoCors),
    )
    .unwrap();
    assert_eq!(opaque.tainting, ResponseTainting::Opaque);
    assert_eq!((opaque.status, opaque.body.len()), (200, 0));
    assert!(opaque.headers.contains(&("x-secret".into(), "1".into())));
    let mut image = script(other.url("/data"), &origin, RequestMode::NoCors);
    image.destination = RequestDestination::Image;
    let image = load(&network, image).unwrap();
    assert_eq!(
        (image.tainting, &image.body[..]),
        (ResponseTainting::Opaque, b"secret".as_slice())
    );
    assert!(
        other
            .requests()
            .iter()
            .all(|r| r.header("origin").is_none())
    );
    let mut manual = script(other.url("/data"), &origin, RequestMode::NoCors);
    manual.redirect = RedirectMode::Manual;
    assert!(load(&network, manual).is_err());
    let sent = other.requests().len();
    assert!(
        load(
            &network,
            script(other.url("/data"), &origin, RequestMode::SameOrigin)
        )
        .is_err()
    );
    let data = load(
        &network,
        script("data:,x".into(), &origin, RequestMode::SameOrigin),
    )
    .unwrap();
    assert_eq!(data.tainting, ResponseTainting::Basic);
    assert_eq!(other.requests().len(), sent);
    let mine = load(
        &network,
        script(page.url("/mine"), &origin, RequestMode::SameOrigin),
    )
    .unwrap();
    assert_eq!(
        (mine.tainting, &mine.body[..]),
        (ResponseTainting::Basic, b"mine".as_slice())
    );
    // CORS is only for HTTP(S): a `file:` URL is an error in `cors` mode,
    // from any origin, an opaque one too (whatever a renderer asks).
    let dir = std::env::temp_dir().join(format!("machopo-net-cors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "local").unwrap();
    let file = net::file_url(&file).unwrap();
    for from in [origin.as_str(), "null"] {
        assert!(
            load(&network, script(file.clone(), from, RequestMode::Cors)).is_err(),
            "{from}"
        );
    }
    let data = load(
        &network,
        script("data:,x".into(), &origin, RequestMode::Cors),
    )
    .unwrap();
    assert_eq!(
        (data.tainting, &data.body[..]),
        (ResponseTainting::Basic, b"x".as_slice())
    );
    assert_eq!(page.requests().pop().unwrap().header("origin"), None);
}

/// A CORS request's redirects: one from its own origin to another goes on
/// as CORS (its `Origin` this origin); the redirect of another origin has
/// to allow this one too; from another origin to a third, or back home,
/// the origin is tainted (`Origin: null`, which only `*` or `null`
/// allows), and coming home is still CORS; a location with credentials is
/// an error.
#[test]
fn cors_redirects() {
    let third = server(&[
        (
            "/open",
            Route::ok("text/plain", "third").with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/null",
            Route::ok("text/plain", "null").with_header("Access-Control-Allow-Origin", "null"),
        ),
    ]);
    let page = server(&[]);
    let origin = origin_of(&page);
    let home = server(&[
        ("/plain", Route::ok("text/plain", "home")),
        (
            "/allowed",
            Route::ok("text/plain", "home").with_header("Access-Control-Allow-Origin", "null"),
        ),
    ]);
    let home_origin = origin_of(&home);
    let other = server(&[
        (
            "/open",
            Route::ok("text/plain", "other").with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/hop",
            Route::redirect(302, &third.url("/open"))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/hop-null",
            Route::redirect(302, &third.url("/null"))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
        ("/closed-hop", Route::redirect(302, "/open")),
        (
            "/back/plain",
            Route::redirect(302, &home.url("/plain"))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/back/allowed",
            Route::redirect(302, &home.url("/allowed"))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
        (
            "/credentials",
            Route::redirect(302, &other_with_credentials(&third))
                .with_header("Access-Control-Allow-Origin", "*"),
        ),
    ]);
    let mine = server(&[("/away", Route::redirect(302, &other.url("/open")))]);
    let mine_origin = origin_of(&mine);
    let network = NetworkContext::start(params()).unwrap();
    let away = load(
        &network,
        script(mine.url("/away"), &mine_origin, RequestMode::Cors),
    )
    .unwrap();
    assert_eq!(
        (away.tainting, &away.body[..]),
        (ResponseTainting::Cors, b"other".as_slice())
    );
    assert!(away.redirected);
    assert_eq!(
        other.requests().pop().unwrap().header("origin"),
        Some(mine_origin.as_str())
    );
    // Away and back: still CORS at home, the origin tainted (`null`).
    let back = |path: &str| {
        script(
            other.url(&format!("/back{path}")),
            &home_origin,
            RequestMode::Cors,
        )
    };
    assert!(load(&network, back("/plain")).is_err());
    assert_eq!(load(&network, back("/allowed")).unwrap().body, &b"home"[..]);
    assert_eq!(
        home.requests().pop().unwrap().header("origin"),
        Some("null")
    );
    let hop = load(
        &network,
        script(other.url("/hop"), &origin, RequestMode::Cors),
    )
    .unwrap();
    assert_eq!(hop.body, &b"third"[..]);
    assert_eq!(
        third.requests().pop().unwrap().header("origin"),
        Some("null")
    );
    let hop_null = load(
        &network,
        script(other.url("/hop-null"), &origin, RequestMode::Cors),
    )
    .unwrap();
    assert_eq!(hop_null.body, &b"null"[..]);
    assert!(
        load(
            &network,
            script(other.url("/closed-hop"), &origin, RequestMode::Cors)
        )
        .is_err()
    );
    assert!(
        load(
            &network,
            script(other.url("/credentials"), &origin, RequestMode::Cors)
        )
        .is_err()
    );
}

/// `server`'s `/open`, with credentials in its URL.
fn other_with_credentials(server: &TestServer) -> String {
    server
        .url("/open")
        .replacen("http://", "http://user:pass@", 1)
}

/// A `data:` URL's response: Fetch's "OK", and its MIME type as the
/// `Content-Type`.
#[test]
fn a_data_url_has_a_content_type() {
    let network = NetworkContext::start(params()).unwrap();
    let response = fetch(
        &network,
        "data:text/plain;charset=utf-8,hi",
        RequestDestination::Empty,
    )
    .unwrap();
    assert_eq!(response.status_text, "OK");
    assert_eq!(
        response.headers,
        [(
            "content-type".to_owned(),
            "text/plain;charset=utf-8".to_owned()
        )]
    );
}

/// A `Location` is bytes (each a character in the response's headers),
/// read as UTF-8 as a URL is.
#[test]
fn a_location_is_utf_8() {
    let server = server(&[
        ("/moved", Route::redirect(302, "/caf\u{e9}")),
        ("/caf%C3%A9", Route::ok("text/plain", "here")),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let response = fetch(&network, &server.url("/moved"), RequestDestination::Empty).unwrap();
    assert_eq!(response.url, server.url("/caf%C3%A9"));
    assert_eq!(response.body, &b"here"[..]);
}

/// A request of `method` with `body` (and a `Content-Type` for it).
fn send(url: String, method: &str, body: Option<&str>) -> ResourceRequest {
    let mut request = ResourceRequest::new(url, RequestDestination::Empty);
    request.method = method.into();
    if let Some(body) = body {
        request.body = Some(body.as_bytes().to_vec());
        request.headers = vec![("Content-Type".into(), "text/plain".into())];
    }
    request
}

/// Any method, with its body; a `POST` or `PUT` without one says
/// `Content-Length: 0`, and a `HEAD`'s response has none. A forbidden
/// method, or one that isn't a token, is refused whatever the renderer
/// said.
#[test]
fn a_request_has_a_method_and_a_body() {
    let server = server(&[("/echo", Route::ok("text/plain", "answer"))]);
    let network = NetworkContext::start(params()).unwrap();
    let url = server.url("/echo");
    let response = load(&network, send(url.clone(), "POST", Some("caf\u{e9}"))).unwrap();
    assert_eq!(response.body, &b"answer"[..]);
    assert_eq!(
        load(&network, send(url.clone(), "PUT", None))
            .unwrap()
            .status,
        200
    );
    assert_eq!(
        load(&network, send(url.clone(), "HEAD", None))
            .unwrap()
            .body,
        &b""[..]
    );
    load(&network, send(url.clone(), "DELETE", None)).unwrap();
    let requests = server.requests();
    let seen: Vec<(&str, &[u8], Option<&str>)> = requests
        .iter()
        .map(|r| (r.method.as_str(), &r.body[..], r.header("content-length")))
        .collect();
    assert_eq!(
        seen,
        [
            ("POST", "caf\u{e9}".as_bytes(), Some("5")),
            ("PUT", b"".as_slice(), Some("0")),
            ("HEAD", b"".as_slice(), None),
            ("DELETE", b"".as_slice(), None),
        ]
    );
    for method in ["TRACE", "connect", "a b", ""] {
        let error = load(&network, send(url.clone(), method, None)).unwrap_err();
        assert_eq!(error.code, NetErrorCode::Failed, "{method:?}");
    }
    assert_eq!(server.requests().len(), 4);
}

/// Fetch's "HTTP-redirect fetch": a 301 or 302 of a `POST`, and a 303 of
/// anything but `GET` or `HEAD`, go on as a `GET` without the body and its
/// headers; a 307 or 308 keeps them; `Authorization` stays with its origin.
#[test]
fn a_redirect_changes_the_method() {
    let other = server(&[("/plain", Route::ok("text/plain", "elsewhere"))]);
    let server = server(&[
        ("/plain", Route::ok("text/plain", "here")),
        ("/301", Route::redirect(301, "/plain")),
        ("/302", Route::redirect(302, "/plain")),
        ("/303", Route::redirect(303, "/plain")),
        ("/307", Route::redirect(307, "/plain")),
        ("/308", Route::redirect(308, "/plain")),
        ("/away", Route::redirect(307, &other.url("/plain"))),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    // A `HEAD` has no body.
    let tries = [
        ("/301", "POST", Some("body")),
        ("/302", "POST", Some("body")),
        ("/303", "PUT", Some("body")),
        ("/303", "HEAD", None),
        ("/307", "POST", Some("body")),
        ("/308", "PUT", Some("body")),
        ("/301", "PUT", Some("body")),
    ];
    let mut seen = Vec::new();
    for (path, method, body) in tries {
        load(&network, send(server.url(path), method, body)).unwrap();
        let plain = server.requests().pop().unwrap();
        seen.push((
            plain.method.clone(),
            String::from_utf8(plain.body.clone()).unwrap(),
            plain.header("content-type").map(str::to_owned),
        ));
    }
    let gone = ("GET".to_owned(), String::new(), None);
    let kept = |method: &str| {
        (
            method.to_owned(),
            "body".to_owned(),
            Some("text/plain".to_owned()),
        )
    };
    assert_eq!(
        seen,
        [
            gone.clone(),
            gone.clone(),
            gone,
            ("HEAD".to_owned(), String::new(), None),
            kept("POST"),
            kept("PUT"),
            kept("PUT"),
        ]
    );
    let mut request = send(server.url("/away"), "GET", None);
    request.headers = vec![
        ("Authorization".into(), "Basic a".into()),
        ("X-Kept".into(), "1".into()),
    ];
    load(&network, request).unwrap();
    let away = other.requests().pop().unwrap();
    assert_eq!(away.header("authorization"), None);
    assert_eq!(away.header("x-kept"), Some("1"));
    let mut request = send(server.url("/307"), "GET", None);
    request.headers = vec![("Authorization".into(), "Basic a".into())];
    load(&network, request).unwrap();
    assert_eq!(
        server.requests().pop().unwrap().header("authorization"),
        Some("Basic a")
    );
}

/// A redirect of an `error` request is a network error; of a `manual` one,
/// the response, not followed (the renderer filters it).
#[test]
fn redirect_modes() {
    let server = server(&[
        ("/plain", Route::ok("text/plain", "here")),
        ("/moved", Route::redirect(302, "/plain")),
        ("/nowhere", Route::new(302, "")),
    ]);
    let network = NetworkContext::start(params()).unwrap();
    let mut error = ResourceRequest::new(server.url("/moved"), RequestDestination::Empty);
    error.redirect = RedirectMode::Error;
    assert_eq!(
        load(&network, error).unwrap_err().code,
        NetErrorCode::Failed
    );
    let mut manual = ResourceRequest::new(server.url("/moved"), RequestDestination::Empty);
    manual.redirect = RedirectMode::Manual;
    let response = load(&network, manual).unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.url, server.url("/moved"));
    assert!(!response.redirected);
    // A redirect status without a `Location` is one too.
    let mut error = ResourceRequest::new(server.url("/nowhere"), RequestDestination::Empty);
    error.redirect = RedirectMode::Error;
    assert!(load(&network, error).is_err());
    let targets: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(targets, ["/moved", "/moved", "/nowhere"]);
}

/// Script's request other than a `GET` or `HEAD` says where it comes from
/// (`Origin`); a `HEAD`'s response has no body, a `data:` URL's either.
#[test]
fn origin_and_head() {
    let server = server(&[("/echo", Route::ok("text/plain", "answer"))]);
    let origin = server.url("").trim_end_matches('/').to_owned();
    let network = NetworkContext::start(params()).unwrap();
    for method in ["POST", "GET", "HEAD"] {
        let mut request = send(server.url("/echo"), method, None);
        request.initiator = Some(origin.clone());
        load(&network, request).unwrap();
    }
    load(&network, send(server.url("/echo"), "POST", None)).unwrap();
    let origins: Vec<Option<String>> = server
        .requests()
        .iter()
        .map(|r| r.header("origin").map(str::to_owned))
        .collect();
    assert_eq!(origins, [Some(origin), None, None, None]);
    let data = load(&network, send("data:,body".into(), "HEAD", None)).unwrap();
    assert_eq!(data.status, 200);
    assert_eq!(data.body, &b""[..]);
    assert_eq!(
        load(&network, send("data:,body".into(), "POST", None))
            .unwrap()
            .body,
        &b"body"[..]
    );
}

/// A cancelled load never calls back, whether the server answered yet or
/// not, and the server sees it given up; a load whose handle is dropped
/// without cancelling goes on.
#[test]
fn a_load_is_cancelled() {
    let server = server(&[(
        "/slow",
        Route::ok("text/plain", "late").with_delay(Duration::from_millis(300)),
    )]);
    let network = NetworkContext::start(params()).unwrap();
    let (tx, rx) = unbounded();
    let start = |tx: crossbeam_channel::Sender<&'static str>, name: &'static str| {
        network.fetch(
            ResourceRequest::new(server.url("/slow"), RequestDestination::Empty),
            move |_| {
                let _ = tx.send(name);
            },
        )
    };
    // At once, before the service may have started it; and once it waits
    // for the server.
    start(tx.clone(), "cancelled at once").cancel();
    let waiting = start(tx.clone(), "cancelled waiting");
    std::thread::sleep(Duration::from_millis(100));
    waiting.cancel();
    drop(start(tx, "dropped"));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok("dropped"));
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
    // The one cancelled at once may never have reached the server.
    assert!(server.wait_for_abandoned("/slow", Duration::from_secs(2)));
    let answered = server.requests().iter().filter(|r| !r.abandoned).count();
    assert_eq!(answered, 1);
}
