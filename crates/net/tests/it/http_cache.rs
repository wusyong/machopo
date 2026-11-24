//! The HTTP cache, end to end through the `NetworkService` thread against a
//! local test server: which loads reach the server, what they send, and
//! what the page gets.

use std::collections::HashMap;
use std::io::Write as _;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer};
use net::{NetworkContext, NetworkContextParams};
use protocol::network::{LoadResult, RequestDestination, ResourceRequest, TrustedParams};

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn network() -> NetworkContext {
    NetworkContext::start(NetworkContextParams::default()).expect("the network service starts")
}

/// A subresource's trusted params: in a frame of `frame` in a top-level
/// frame of `top`.
fn in_frame(top: &str, frame: &str) -> Option<TrustedParams> {
    Some(TrustedParams {
        top_frame_site: Some(top.to_owned()),
        frame_site: Some(frame.to_owned()),
        initiator: Some(frame.to_owned()),
        ..TrustedParams::default()
    })
}

/// An image request of `url` in a page of `http://a.test`.
fn image(url: &str) -> ResourceRequest {
    let mut request = ResourceRequest::new(url, RequestDestination::Image);
    request.trusted = in_frame("http://a.test", "http://a.test");
    request
}

fn with_header(mut request: ResourceRequest, name: &str, value: &str) -> ResourceRequest {
    request.headers.push((name.to_owned(), value.to_owned()));
    request
}

fn load(network: &NetworkContext, request: ResourceRequest) -> LoadResult {
    let (tx, rx) = unbounded();
    network.fetch(request, move |result| {
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the network service answers")
}

fn body(network: &NetworkContext, request: ResourceRequest) -> Vec<u8> {
    load(network, request).expect("it loads").body.to_vec()
}

/// How many requests of `target` the server got.
fn hits(server: &TestServer, target: &str) -> usize {
    server
        .requests()
        .iter()
        .filter(|request| request.target == target)
        .count()
}

/// The `name` header of the server's `n`th request of `target`.
fn sent(server: &TestServer, target: &str, n: usize, name: &str) -> Option<String> {
    server
        .requests()
        .iter()
        .filter(|request| request.target == target)
        .nth(n)
        .and_then(|request| request.header(name).map(str::to_owned))
}

#[test]
fn a_fresh_response_answers_without_the_network() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    assert_eq!(body(&network, image(&server.url("/i"))), b"pixels");
    let again = load(&network, image(&server.url("/i"))).expect("it loads");
    assert_eq!(again.body.to_vec(), b"pixels");
    assert_eq!(again.head.status, 200);
    // Nothing came over the network.
    assert_eq!(again.encoded_body_length, 0);
    assert_eq!(hits(&server, "/i"), 1);
}

#[test]
fn heuristic_freshness_from_last_modified() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Last-Modified", "Mon, 01 Jan 2024 00:00:00 GMT"),
    )]);
    let network = network();
    body(&network, image(&server.url("/i")));
    body(&network, image(&server.url("/i")));
    assert_eq!(hits(&server, "/i"), 1);
}

#[test]
fn no_store_and_nothing_to_go_by_go_to_the_network() {
    let server = server(&[
        (
            "/no-store",
            Route::ok("image/png", "a")
                .with_header("Cache-Control", "max-age=60, no-store")
                .with_header("ETag", "\"a\""),
        ),
        ("/plain", Route::ok("image/png", "b")),
    ]);
    let network = network();
    for _ in 0..2 {
        body(&network, image(&server.url("/no-store")));
        body(&network, image(&server.url("/plain")));
    }
    assert_eq!(hits(&server, "/no-store"), 2);
    assert_eq!(hits(&server, "/plain"), 2);
    // Not kept, so not validated either; without validators, the second
    // goes out whole.
    assert_eq!(sent(&server, "/no-store", 1, "if-none-match"), None);
    assert_eq!(sent(&server, "/plain", 1, "if-none-match"), None);
    assert_eq!(sent(&server, "/plain", 1, "if-modified-since"), None);
}

#[test]
fn a_stale_one_is_validated_and_a_304_answers_with_it() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", "\"v1\"")
            .with_header("Last-Modified", "Mon, 01 Jan 2024 00:00:00 GMT")
            .revalidated(),
    )]);
    let network = network();
    body(&network, image(&server.url("/i")));
    let again = load(&network, image(&server.url("/i"))).expect("it loads");
    assert_eq!(again.head.status, 200);
    assert_eq!(again.body.to_vec(), b"pixels");
    assert_eq!(hits(&server, "/i"), 2);
    assert_eq!(sent(&server, "/i", 1, "if-none-match").as_deref(), Some("\"v1\""));
    assert_eq!(
        sent(&server, "/i", 1, "if-modified-since").as_deref(),
        Some("Mon, 01 Jan 2024 00:00:00 GMT")
    );
}

#[test]
fn a_304s_headers_update_the_entry() {
    // Stale as it came; the 304 says it is fresh for a minute.
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", "\"v1\"")
            .with_304_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    for _ in 0..3 {
        assert_eq!(body(&network, image(&server.url("/i"))), b"pixels");
    }
    assert_eq!(hits(&server, "/i"), 2);
}

#[test]
fn a_304_that_says_no_store_drops_the_entry() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", "\"v1\"")
            .with_304_header("Cache-Control", "no-store"),
    )]);
    let network = network();
    for _ in 0..3 {
        assert_eq!(body(&network, image(&server.url("/i"))), b"pixels");
    }
    // The second was validated and answered; the third had nothing to
    // validate.
    assert_eq!(hits(&server, "/i"), 3);
    assert_eq!(sent(&server, "/i", 1, "if-none-match").as_deref(), Some("\"v1\""));
    assert_eq!(sent(&server, "/i", 2, "if-none-match"), None);
}

#[test]
fn a_vary_that_differs_goes_to_the_network() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "max-age=60")
            .with_header("Vary", "X-Mode"),
    )]);
    let network = network();
    let mode = |value| with_header(image(&server.url("/i")), "x-mode", value);
    body(&network, mode("a"));
    body(&network, mode("a"));
    assert_eq!(hits(&server, "/i"), 1);
    body(&network, mode("b"));
    assert_eq!(hits(&server, "/i"), 2);
}

#[test]
fn vary_accept_goes_by_the_accept_the_transport_sends() {
    let server = server(&[(
        "/r",
        Route::ok("text/plain", "r")
            .with_header("Cache-Control", "max-age=60")
            .with_header("Vary", "Accept"),
    )]);
    let network = network();
    let as_fetch = || {
        let mut request = image(&server.url("/r"));
        request.destination = RequestDestination::Empty;
        request
    };
    body(&network, image(&server.url("/r")));
    // An image's `Accept` isn't `fetch()`'s.
    body(&network, as_fetch());
    assert_eq!(hits(&server, "/r"), 2);
    body(&network, as_fetch());
    assert_eq!(hits(&server, "/r"), 2);
}

#[test]
fn entries_are_kept_by_top_level_and_frame_site() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    let framed = |top: &str, frame: &str| {
        let mut request = image(&server.url("/i"));
        request.trusted = in_frame(top, frame);
        request
    };
    body(&network, framed("http://a.test", "http://a.test"));
    body(&network, framed("http://a.test", "http://a.test"));
    assert_eq!(hits(&server, "/i"), 1);
    body(&network, framed("http://b.test", "http://a.test"));
    assert_eq!(hits(&server, "/i"), 2);
    body(&network, framed("http://a.test", "http://x.test"));
    assert_eq!(hits(&server, "/i"), 3);
}

#[test]
fn no_site_no_cache() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    let untrusted = || ResourceRequest::new(server.url("/i"), RequestDestination::Image);
    body(&network, untrusted());
    body(&network, untrusted());
    let opaque = || {
        let mut request = image(&server.url("/i"));
        request.trusted.as_mut().expect("trusted").frame_site = None;
        request
    };
    body(&network, opaque());
    body(&network, opaque());
    assert_eq!(hits(&server, "/i"), 4);
}

#[test]
fn a_post_that_works_drops_the_entry() {
    let server = server(&[
        (
            "/r",
            Route::ok("text/plain", "resource").with_header("Cache-Control", "max-age=60"),
        ),
        ("POST /r", Route::ok("text/plain", "done")),
        (
            "/kept",
            Route::ok("text/plain", "resource").with_header("Cache-Control", "max-age=60"),
        ),
        ("POST /kept", Route::new(500, "failed")),
    ]);
    let network = network();
    let get = || image(&server.url("/r"));
    body(&network, get());
    body(&network, get());
    assert_eq!(hits(&server, "/r"), 1);
    let mut post = image(&server.url("/r"));
    post.method = "POST".to_owned();
    post.destination = RequestDestination::Empty;
    body(&network, post);
    body(&network, get());
    // The POST and the GET after it.
    assert_eq!(hits(&server, "/r"), 3);
    // One that fails changed nothing.
    body(&network, image(&server.url("/kept")));
    let mut post = image(&server.url("/kept"));
    post.method = "POST".to_owned();
    post.destination = RequestDestination::Empty;
    body(&network, post);
    body(&network, image(&server.url("/kept")));
    assert_eq!(hits(&server, "/kept"), 2);
}

#[test]
fn request_headers_say_how_the_cache_is_used() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "max-age=60")
            .with_header("ETag", "\"v1\"")
            .revalidated(),
    )]);
    let network = network();
    let url = server.url("/i");
    body(&network, image(&url));
    // `no-cache`: fetched again, whole, and kept.
    body(&network, with_header(image(&url), "pragma", "no-cache"));
    assert_eq!(hits(&server, "/i"), 2);
    assert_eq!(sent(&server, "/i", 1, "if-none-match"), None);
    body(&network, image(&url));
    assert_eq!(hits(&server, "/i"), 2);
    // `max-age=0`: validated.
    body(&network, with_header(image(&url), "cache-control", "max-age=0"));
    assert_eq!(hits(&server, "/i"), 3);
    assert_eq!(sent(&server, "/i", 2, "if-none-match").as_deref(), Some("\"v1\""));
    // `If-Match`: past the cache.
    body(&network, with_header(image(&url), "if-match", "\"v1\""));
    assert_eq!(hits(&server, "/i"), 4);
}

#[test]
fn the_callers_own_validation_goes_out_and_updates_the_entry() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", "\"v1\"")
            .with_304_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    let url = server.url("/i");
    body(&network, image(&url));
    // Its own validators: out as it is, and the caller gets the 304.
    let response = load(&network, with_header(image(&url), "if-none-match", "\"v1\""))
        .expect("it loads");
    assert_eq!(response.head.status, 304);
    assert_eq!(hits(&server, "/i"), 2);
    // The 304 freshened the entry.
    body(&network, image(&url));
    assert_eq!(hits(&server, "/i"), 2);
}

#[test]
fn the_callers_own_validation_of_nothing_stored_isnt_kept() {
    // Fresh for a minute, but asked for with validators the cache has no
    // response of: past the cache, and not kept.
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    let url = server.url("/i");
    let response = load(&network, with_header(image(&url), "if-none-match", "\"other\""))
        .expect("it loads");
    assert_eq!(response.head.status, 200);
    body(&network, image(&url));
    assert_eq!(hits(&server, "/i"), 2);
}

#[test]
fn a_head_uses_a_gets_entry_and_makes_none() {
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    let head = || {
        let mut request = image(&server.url("/i"));
        request.method = "HEAD".to_owned();
        request
    };
    // No entry: out, and none made.
    let response = load(&network, head()).expect("it loads");
    assert_eq!(response.head.status, 200);
    body(&network, image(&server.url("/i")));
    assert_eq!(hits(&server, "/i"), 2);
    // Now the GET's answers it, without its body.
    let response = load(&network, head()).expect("it loads");
    assert_eq!(response.head.status, 200);
    assert!(response.body.is_empty());
    assert_eq!(hits(&server, "/i"), 2);
}

#[test]
fn a_head_answered_whole_drops_the_entry() {
    // Never a 304: a validation gets the whole response.
    let server = server(&[(
        "/i",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", "\"v1\""),
    )]);
    let network = network();
    body(&network, image(&server.url("/i")));
    let mut head = image(&server.url("/i"));
    head.method = "HEAD".to_owned();
    load(&network, head).expect("it loads");
    assert_eq!(sent(&server, "/i", 1, "if-none-match").as_deref(), Some("\"v1\""));
    // Nothing left to validate.
    body(&network, image(&server.url("/i")));
    assert_eq!(sent(&server, "/i", 2, "if-none-match"), None);
}

#[test]
fn a_permanent_redirect_is_kept() {
    let server = server(&[
        ("/old", Route::redirect(301, "/new")),
        (
            "/new",
            Route::ok("image/png", "pixels").with_header("Cache-Control", "max-age=60"),
        ),
    ]);
    let network = network();
    assert_eq!(body(&network, image(&server.url("/old"))), b"pixels");
    assert_eq!(body(&network, image(&server.url("/old"))), b"pixels");
    assert_eq!(hits(&server, "/old"), 1);
    assert_eq!(hits(&server, "/new"), 1);
}

#[test]
fn a_body_in_its_coding_is_kept_so_and_undone_again() {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(b"pixels, compressed").expect("compresses");
    let server = server(&[(
        "/i",
        Route::new(200, gzip.finish().expect("compresses"))
            .with_header("Content-Type", "image/png")
            .with_header("Content-Encoding", "gzip")
            .with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    assert_eq!(body(&network, image(&server.url("/i"))), b"pixels, compressed");
    assert_eq!(body(&network, image(&server.url("/i"))), b"pixels, compressed");
    assert_eq!(hits(&server, "/i"), 1);
}

#[test]
fn one_bigger_than_an_eighth_isnt_kept() {
    // An eighth of 50 MiB, and a little more.
    let big = vec![b'x'; 50 * 1024 * 1024 / 8 + 1];
    let server = server(&[(
        "/big",
        Route::ok("image/png", big).with_header("Cache-Control", "max-age=60"),
    )]);
    let network = network();
    body(&network, image(&server.url("/big")));
    body(&network, image(&server.url("/big")));
    assert_eq!(hits(&server, "/big"), 2);
}

#[test]
fn loads_at_once_for_one_key_reach_the_server_once() {
    let server = server(&[(
        "/slow",
        Route::ok("image/png", "pixels")
            .with_header("Cache-Control", "max-age=60")
            .with_delay(Duration::from_millis(300)),
    )]);
    let network = network();
    let (tx, rx) = unbounded();
    for _ in 0..3 {
        let tx = tx.clone();
        network.fetch(image(&server.url("/slow")), move |result| {
            let _ = tx.send(result);
        });
    }
    for _ in 0..3 {
        let result = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the network service answers");
        assert_eq!(result.expect("it loads").body.to_vec(), b"pixels");
    }
    assert_eq!(hits(&server, "/slow"), 1);
}
