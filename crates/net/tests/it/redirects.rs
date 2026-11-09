//! A load that waits at each redirect (`fetch_with_redirects`) until its
//! handle follows it, against a local test server.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use net::test_server::{Route, TestServer};
use net::{LoadHandle, NetworkContext, NetworkContextParams};
use protocol::network::{LoadResult, RedirectInfo, RequestDestination, ResourceRequest};

/// How long to watch for something that mustn't happen.
const QUIET: Duration = Duration::from_millis(200);

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

/// The paths the server has been asked for, in order.
fn paths(server: &TestServer) -> Vec<String> {
    server.requests().into_iter().map(|r| r.target).collect()
}

struct Load {
    handle: LoadHandle,
    redirects: Receiver<RedirectInfo>,
    done: Receiver<LoadResult>,
}

fn start(network: &NetworkContext, request: ResourceRequest) -> Load {
    let (redirect_tx, redirects) = unbounded();
    let (done_tx, done) = unbounded();
    let handle = network.fetch_with_redirects(
        request,
        move |redirect| {
            let _ = redirect_tx.send(redirect);
        },
        move |result| {
            let _ = done_tx.send(result);
        },
    );
    Load {
        handle,
        redirects,
        done,
    }
}

fn get(url: &str) -> ResourceRequest {
    ResourceRequest::new(url, RequestDestination::Document)
}

fn network() -> NetworkContext {
    NetworkContext::start(NetworkContextParams::default()).unwrap()
}

impl Load {
    fn next_redirect(&self) -> RedirectInfo {
        self.redirects
            .recv_timeout(Duration::from_secs(30))
            .expect("the load reaches a redirect")
    }

    fn result(&self) -> LoadResult {
        self.done
            .recv_timeout(Duration::from_secs(30))
            .expect("the load ends")
    }
}

#[test]
fn a_load_waits_at_a_redirect_until_it_is_followed() {
    let server = server(&[
        (
            "/a",
            Route {
                body: b"moved".to_vec(),
                ..Route::redirect(302, "/b").with_header("X-Hop", "a")
            },
        ),
        ("/b", Route::ok("text/plain", "b")),
    ]);
    let network = network();
    let load = start(&network, get(&server.url("/a")));

    let redirect = load.next_redirect();
    assert_eq!(redirect.new_url, server.url("/b"));
    assert_eq!(redirect.new_method, "GET");
    let response = &redirect.response;
    assert_eq!(response.url, server.url("/a"));
    assert_eq!(response.status, 302);
    assert!(response.headers.contains(&("x-hop".into(), "a".into())));
    assert!(response.body.is_empty());
    assert!(
        response.timing.unwrap().connect.is_some(),
        "{:?}",
        response.timing
    );
    assert_eq!(response.protocol, "http/1.1");

    std::thread::sleep(QUIET);
    assert_eq!(paths(&server), ["/a"], "not yet followed");
    assert!(load.done.try_recv().is_err());

    load.handle.follow_redirect();
    let response = load.result().unwrap();
    assert_eq!(&response.body[..], b"b");
    assert!(response.redirected);
    assert_eq!(paths(&server), ["/a", "/b"]);
}

#[test]
fn each_redirect_of_a_chain_waits_in_turn() {
    let server = server(&[
        ("/a", Route::redirect(301, "/b")),
        ("/b", Route::redirect(307, "/c")),
        ("/c", Route::ok("text/plain", "c")),
    ]);
    let network = network();
    let load = start(&network, get(&server.url("/a")));
    assert_eq!(load.next_redirect().response.status, 301);
    load.handle.follow_redirect();
    let second = load.next_redirect();
    assert_eq!(
        (second.response.url.as_str(), second.response.status),
        (server.url("/b").as_str(), 307)
    );
    assert_eq!(paths(&server), ["/a", "/b"]);
    load.handle.follow_redirect();
    assert_eq!(&load.result().unwrap().body[..], b"c");
    assert!(load.redirects.try_recv().is_err(), "two redirects only");
}

#[test]
fn a_redirect_says_what_the_next_request_is() {
    let server = server(&[
        ("/form", Route::redirect(303, "/done")),
        ("/done", Route::ok("text/plain", "done")),
    ]);
    let network = network();
    let mut request = get(&server.url("/form"));
    request.method = "POST".into();
    request.body = Some(b"x=1".to_vec());
    let load = start(&network, request);
    let redirect = load.next_redirect();
    assert_eq!(redirect.new_method, "GET", "a 303 goes on as a GET");
    load.handle.follow_redirect();
    load.result().unwrap();
    let requests = server.requests();
    assert_eq!(requests[1].method, "GET");
}

#[test]
fn a_load_cancelled_at_a_redirect_ends_there() {
    let server = server(&[
        ("/a", Route::redirect(302, "/b")),
        ("/b", Route::ok("text/plain", "b")),
    ]);
    let network = network();
    let load = start(&network, get(&server.url("/a")));
    load.next_redirect();
    load.handle.cancel();
    std::thread::sleep(QUIET);
    assert_eq!(paths(&server), ["/a"]);
    assert!(
        load.done.try_recv().is_err(),
        "a cancelled load never answers"
    );
}

#[test]
fn following_before_a_redirect_lets_none_through() {
    let server = server(&[
        (
            "/a",
            Route::redirect(302, "/b").with_delay(Duration::from_millis(100)),
        ),
        ("/b", Route::ok("text/plain", "b")),
    ]);
    let network = network();
    let load = start(&network, get(&server.url("/a")));
    load.handle.follow_redirect();
    load.next_redirect();
    std::thread::sleep(QUIET);
    assert_eq!(paths(&server), ["/a"], "the early follow didn't count");
    load.handle.follow_redirect();
    assert_eq!(&load.result().unwrap().body[..], b"b");
}

#[test]
fn a_load_that_follows_by_itself_tells_of_no_redirect() {
    let server = server(&[
        ("/a", Route::redirect(302, "/b")),
        ("/b", Route::ok("text/plain", "b")),
    ]);
    let network = network();
    let (tx, rx) = unbounded();
    network.fetch(get(&server.url("/a")), move |result| {
        let _ = tx.send(result);
    });
    let response = rx.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    assert_eq!(&response.body[..], b"b");
    assert_eq!(paths(&server), ["/a", "/b"]);
}
