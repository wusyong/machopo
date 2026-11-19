//! A load started with `NetworkContext::load`: its `UrlLoaderClient` hears
//! of the head with the body's data pipe, then of the end; or only of the
//! end, for a load with no response. Against a local test server.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ipc::data_pipe::{Consumer, Read};
use net::test_server::{Route, TestServer, unused_port};
use net::{AtRedirects, LoadHandle, NetworkContext, NetworkContextParams, UrlLoaderClient};
use protocol::network::{
    CompletionStatus, NetErrorCode, RedirectInfo, RequestDestination, ResourceRequest, ResponseHead,
};

/// How long to watch for something that mustn't happen.
const QUIET: Duration = Duration::from_millis(200);
/// How long something that should happen may take.
const SOON: Duration = Duration::from_secs(30);

enum Event {
    Redirect(RedirectInfo),
    Response(ResponseHead, Consumer),
    Complete(CompletionStatus),
}

/// Tells the test of each call, from the `NetworkService` thread.
struct Client {
    events: Sender<Event>,
    /// Lets the body go as soon as it comes.
    drop_body: bool,
}

impl UrlLoaderClient for Client {
    fn on_receive_redirect(&mut self, redirect: RedirectInfo) {
        let _ = self.events.send(Event::Redirect(redirect));
    }

    fn on_receive_response(&mut self, head: ResponseHead, body: Consumer) {
        if self.drop_body {
            drop(body);
            return;
        }
        let _ = self.events.send(Event::Response(head, body));
    }

    fn on_complete(&mut self, status: CompletionStatus) {
        let _ = self.events.send(Event::Complete(status));
    }
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

fn network() -> NetworkContext {
    NetworkContext::start(NetworkContextParams::default()).unwrap()
}

fn start(
    network: &NetworkContext,
    request: ResourceRequest,
    at_redirects: AtRedirects,
    drop_body: bool,
) -> (LoadHandle, Receiver<Event>) {
    let (events, received) = unbounded();
    let handle = network.load(request, at_redirects, Client { events, drop_body });
    (handle, received)
}

fn get(url: &str) -> ResourceRequest {
    ResourceRequest::new(url, RequestDestination::Document)
}

fn next(events: &Receiver<Event>) -> Event {
    events.recv_timeout(SOON).expect("the load goes on")
}

fn response(events: &Receiver<Event>) -> (ResponseHead, Consumer) {
    match next(events) {
        Event::Response(head, body) => (head, body),
        Event::Redirect(redirect) => panic!("a redirect to {}", redirect.new_url),
        Event::Complete(status) => panic!("the end, before a response: {status:?}"),
    }
}

fn complete(events: &Receiver<Event>) -> CompletionStatus {
    match next(events) {
        Event::Complete(status) => status,
        Event::Redirect(redirect) => panic!("a redirect to {}", redirect.new_url),
        Event::Response(head, _) => panic!("a response from {}", head.url),
    }
}

/// All of a body, read as the watcher says there is more.
fn read_all(body: &mut Consumer) -> Vec<u8> {
    let (readable, wait) = unbounded();
    body.watch(move || {
        let _ = readable.send(());
    });
    let mut all = Vec::new();
    loop {
        match body.read(64 * 1024) {
            Read::Data(bytes) => all.extend_from_slice(&bytes),
            Read::Empty => wait.recv_timeout(SOON).expect("told of more"),
            Read::Done => return all,
        }
    }
}

#[test]
fn the_head_then_the_body_then_the_end() {
    let server = server(&[("/page", Route::ok("text/html; charset=utf-8", "<p>hi"))]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (head, mut body) = response(&events);
    assert_eq!(head.status, 200);
    assert_eq!(head.url, server.url("/page"));
    assert_eq!(head.mime_type, "text/html");
    assert_eq!(head.charset.as_deref(), Some("utf-8"));
    assert_eq!(read_all(&mut body), b"<p>hi");
    let status = complete(&events);
    assert_eq!(status.error, None);
    assert_eq!(status.encoded_body_length, 5);
    let timing = head.timing.expect("timed");
    assert!(status.completion_time >= timing.receive_headers_end);
}

#[test]
fn a_load_without_a_response_tells_only_of_its_end() {
    let network = network();
    let url = format!("http://127.0.0.1:{}/", unused_port());
    let (_handle, events) = start(&network, get(&url), AtRedirects::Follow, false);
    let status = complete(&events);
    assert_eq!(
        status.error.map(|error| error.code),
        Some(NetErrorCode::ConnectionRefused)
    );
    assert_eq!(status.encoded_body_length, 0);
    assert!(events.recv_timeout(QUIET).is_err());
}

/// The body waits in the network service while no one reads it: the end
/// comes only once all of it was read.
#[test]
fn a_body_bigger_than_its_pipe_waits_to_be_read() {
    let big: Vec<u8> = (0..(3 * net::BODY_PIPE_CAPACITY))
        .map(|i| (i % 251) as u8)
        .collect();
    let server = server(&[("/big", Route::ok("application/octet-stream", big.clone()))]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/big")),
        AtRedirects::Follow,
        false,
    );
    let (_head, mut body) = response(&events);
    assert!(
        events.recv_timeout(QUIET).is_err(),
        "the end came with the body unread"
    );
    assert_eq!(read_all(&mut body), big);
    assert_eq!(complete(&events).error, None);
}

#[test]
fn a_body_no_one_reads_ends_the_load() {
    let server = server(&[("/page", Route::ok("text/plain", "unread"))]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        true,
    );
    let status = complete(&events);
    assert_eq!(
        status.error.map(|error| error.code),
        Some(NetErrorCode::Failed)
    );
}

#[test]
fn a_head_request_has_an_empty_body() {
    let server = server(&[("/page", Route::ok("text/plain", "not sent"))]);
    let network = network();
    let mut request = get(&server.url("/page"));
    request.method = "HEAD".into();
    let (_handle, events) = start(&network, request, AtRedirects::Follow, false);
    let (_head, mut body) = response(&events);
    assert_eq!(read_all(&mut body), b"");
    assert_eq!(complete(&events).error, None);
}

#[test]
fn a_load_that_waits_at_a_redirect_goes_on_when_told() {
    let server = server(&[
        ("/a", Route::redirect(302, "/b")),
        ("/b", Route::ok("text/plain", "here")),
    ]);
    let network = network();
    let (handle, events) = start(&network, get(&server.url("/a")), AtRedirects::Wait, false);
    let Event::Redirect(redirect) = next(&events) else {
        panic!("expected the redirect first");
    };
    assert_eq!(redirect.new_url, server.url("/b"));
    assert_eq!(redirect.response.status, 302);
    assert!(events.recv_timeout(QUIET).is_err(), "it went on by itself");
    handle.follow_redirect();
    let (head, mut body) = response(&events);
    assert_eq!(head.url, server.url("/b"));
    assert!(head.redirected);
    assert_eq!(read_all(&mut body), b"here");
    assert_eq!(complete(&events).error, None);
}

#[test]
fn a_load_that_follows_redirects_tells_of_none() {
    let server = server(&[
        ("/a", Route::redirect(302, "/b")),
        ("/b", Route::ok("text/plain", "here")),
    ]);
    let network = network();
    let (_handle, events) = start(&network, get(&server.url("/a")), AtRedirects::Follow, false);
    let (head, mut body) = response(&events);
    assert_eq!(head.url, server.url("/b"));
    assert_eq!(read_all(&mut body), b"here");
    assert_eq!(complete(&events).error, None);
}

#[test]
fn a_cancelled_load_tells_nothing_more() {
    let server = server(&[(
        "/slow",
        Route::ok("text/plain", "late").with_delay(Duration::from_millis(300)),
    )]);
    let network = network();
    let (handle, events) = start(
        &network,
        get(&server.url("/slow")),
        AtRedirects::Follow,
        false,
    );
    handle.cancel();
    assert!(events.recv_timeout(Duration::from_millis(600)).is_err());
}
