//! A request that went out on a kept-alive connection the server had
//! closed (a server may close one at any time, without saying so) goes
//! out again on another, as Chromium's `HttpNetworkTransaction` sends it
//! again; one that opened its connection doesn't. Against a local test
//! server that keeps connections alive, so a second request reuses the
//! first's.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer, Unanswered};
use net::{NetworkContext, NetworkContextParams};
use protocol::network::{NetError, RequestDestination, ResourceRequest, ResourceResponse};

fn keep_alive_server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start_keep_alive(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn load(network: &NetworkContext, request: ResourceRequest) -> Result<ResourceResponse, NetError> {
    let (tx, rx) = unbounded();
    network.fetch(request, move |result| {
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the network service answers")
}

fn get(network: &NetworkContext, url: &str) -> Result<ResourceResponse, NetError> {
    load(
        network,
        ResourceRequest::new(url, RequestDestination::Document),
    )
}

/// Each request the server read: its path, and which connection it came
/// on.
fn seen(server: &TestServer) -> Vec<(String, u64)> {
    server
        .requests()
        .into_iter()
        .map(|r| (r.target, r.connection))
        .collect()
}

#[test]
fn a_request_on_a_connection_the_server_closed_goes_out_again_on_another() {
    let server = keep_alive_server(&[
        ("/a", Route::ok("text/plain", "a")),
        (
            "/b",
            Route::ok("text/plain", "b").unanswered(Unanswered::OnReusedConnection),
        ),
    ]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &server.url("/a")).expect("the first load succeeds");
    let second = get(&network, &server.url("/b")).expect("the second load succeeds");

    assert_eq!(second.body, "b");
    assert_eq!(
        seen(&server),
        [("/a".into(), 1), ("/b".into(), 1), ("/b".into(), 2)],
        "it went out on the first's connection, then on a new one"
    );
    let timing = second.head.timing.expect("a load is timed");
    assert!(
        timing.connect.is_some(),
        "the new connection is the request's: {timing:?}"
    );
}

#[test]
fn a_request_on_a_connection_reset_goes_out_again_on_another() {
    let server = keep_alive_server(&[
        ("/a", Route::ok("text/plain", "a")),
        (
            "/b",
            Route::ok("text/plain", "b").unanswered(Unanswered::ResetOnReusedConnection),
        ),
    ]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &server.url("/a")).expect("the first load succeeds");
    let second = get(&network, &server.url("/b")).expect("the second load succeeds");

    assert_eq!(second.body, "b");
    assert_eq!(
        seen(&server),
        [("/a".into(), 1), ("/b".into(), 2)],
        "the first's connection reset as it went out (unrecorded), then a new one"
    );
}

#[test]
fn a_post_sent_again_sends_its_body_again() {
    let server = keep_alive_server(&[
        ("/a", Route::ok("text/plain", "a")),
        (
            "/b",
            Route::ok("text/plain", "b").unanswered(Unanswered::OnReusedConnection),
        ),
    ]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &server.url("/a")).expect("the first load succeeds");
    let mut post = ResourceRequest::new(server.url("/b"), RequestDestination::Empty);
    post.method = "POST".into();
    post.body = Some(b"x=1".into());
    load(&network, post).expect("the POST succeeds");

    let requests = server.requests();
    let posts: Vec<_> = requests.iter().filter(|r| r.target == "/b").collect();
    assert_eq!(posts.len(), 2, "{requests:?}");
    for request in posts {
        assert_eq!(request.method, "POST");
        assert_eq!(request.body, b"x=1");
    }
}

#[test]
fn a_request_on_a_connection_it_opened_doesnt_go_out_again() {
    let server = keep_alive_server(&[
        ("/a", Route::ok("text/plain", "a")),
        (
            "/gone",
            Route::ok("text/plain", "never").unanswered(Unanswered::Always),
        ),
    ]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &server.url("/a")).expect("the first load succeeds");
    get(&network, &server.url("/gone")).expect_err("the server never answers");

    assert_eq!(
        seen(&server),
        [("/a".into(), 1), ("/gone".into(), 1), ("/gone".into(), 2)],
        "again once the reused connection closed, not once the new one did"
    );
}

#[test]
fn a_first_request_the_server_doesnt_answer_fails_at_once() {
    let server = keep_alive_server(&[(
        "/gone",
        Route::ok("text/plain", "never").unanswered(Unanswered::Always),
    )]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &server.url("/gone")).expect_err("the server never answers");
    assert_eq!(seen(&server), [("/gone".into(), 1)]);
}

/// Each connection of the pool the server closed is tried in turn, then a
/// new one.
#[test]
fn a_request_goes_out_again_until_it_opens_a_connection() {
    let server = keep_alive_server(&[
        (
            "/a",
            Route::ok("text/plain", "a").with_delay(Duration::from_millis(300)),
        ),
        (
            "/b",
            Route::ok("text/plain", "b").unanswered(Unanswered::OnReusedConnection),
        ),
    ]);
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    // Three at once: three connections, each kept alive once answered.
    let (tx, rx) = unbounded();
    for _ in 0..3 {
        let tx = tx.clone();
        network.fetch(
            ResourceRequest::new(server.url("/a"), RequestDestination::Document),
            move |result| {
                let _ = tx.send(result);
            },
        );
    }
    for _ in 0..3 {
        rx.recv_timeout(Duration::from_secs(30))
            .expect("the network service answers")
            .expect("each of the first loads succeeds");
    }
    let b = get(&network, &server.url("/b")).expect("the load succeeds on a new connection");
    assert_eq!(b.body, "b");

    let requests = server.requests();
    let mut b: Vec<u64> = requests
        .iter()
        .filter(|r| r.target == "/b")
        .map(|r| r.connection)
        .collect();
    b.sort_unstable();
    assert_eq!(
        b,
        [1, 2, 3, 4],
        "each of the pool's, then a new one: {requests:?}"
    );
}

/// A connection reset as it opens (here in TLS's handshake, its hello
/// unread) is the request's own: it isn't sent again.
#[test]
fn a_connection_reset_as_it_opens_isnt_tried_again() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted, accepts) = unbounded();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = accepted.send(());
            // Dropped once the hello came, unread: reset.
            let _ = stream.peek(&mut [0]);
        }
    });
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    get(&network, &format!("https://127.0.0.1:{port}/")).expect_err("the handshake fails");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(accepts.try_iter().count(), 1);
}
