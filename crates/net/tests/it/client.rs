//! A load started with `NetworkContext::load`: its `UrlLoaderClient` hears
//! of the head with the body's data pipe, then of the end; or only of the
//! end, for a load with no response. Against a local test server.

use std::collections::HashMap;
use std::io::Write;
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
/// How long the server stops in a body: long enough that the test reads
/// what came before it, before the rest comes.
const PAUSE: Duration = Duration::from_secs(1);

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

/// The next piece of the body, once it comes.
fn next_piece(body: &mut Consumer) -> Option<Vec<u8>> {
    let (readable, wait) = unbounded();
    body.watch(move || {
        let _ = readable.send(());
    });
    loop {
        match body.read(usize::MAX) {
            Read::Data(bytes) => return Some(bytes.to_vec()),
            Read::Empty => wait.recv_timeout(SOON).expect("told of more"),
            Read::Done => return None,
        }
    }
}

/// Bytes that don't compress much, as a page's text would.
fn varied(length: usize) -> Vec<u8> {
    let mut state = 1u32;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            b'a' + (state >> 16) as u8 % 26
        })
        .collect()
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

#[test]
fn the_head_comes_before_the_body() {
    let server = server(&[(
        "/page",
        Route::ok("text/plain", "late").with_body_delay(PAUSE),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (head, mut body) = response(&events);
    assert_eq!(head.mime_type, "text/plain");
    assert!(
        matches!(body.read(usize::MAX), Read::Empty),
        "the body came first"
    );
    assert_eq!(read_all(&mut body), b"late");
    assert_eq!(complete(&events).error, None);
}

#[test]
fn a_body_comes_as_it_arrives() {
    let server = server(&[(
        "/page",
        Route::ok("text/plain", "first|second").with_pause_in_body(6, PAUSE),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (_head, mut body) = response(&events);
    assert_eq!(next_piece(&mut body).unwrap(), b"first|");
    assert!(matches!(body.read(usize::MAX), Read::Empty));
    assert!(events.try_recv().is_err(), "the end came before the rest");
    assert_eq!(read_all(&mut body), b"second");
    let status = complete(&events);
    assert_eq!(status.error, None);
    assert_eq!(status.encoded_body_length, 12);
}

#[test]
fn a_compressed_body_comes_decoded_as_it_arrives() {
    let text = varied(64 * 1024);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&text).unwrap();
    let gzipped = encoder.finish().unwrap();
    let server = server(&[(
        "/page",
        Route::ok("text/plain", gzipped.clone())
            .with_header("Content-Encoding", "gzip")
            .with_pause_in_body(gzipped.len() / 2, PAUSE),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (_head, mut body) = response(&events);
    let mut decoded = next_piece(&mut body).unwrap();
    assert!(decoded.len() < text.len(), "all of it before the pause");
    decoded.extend(read_all(&mut body));
    assert_eq!(decoded, text);
    let status = complete(&events);
    assert_eq!(status.error, None);
    assert_eq!(status.encoded_body_length, gzipped.len() as u64);
}

#[test]
fn a_body_that_doesnt_decode_ends_in_an_error_after_its_head() {
    let server = server(&[(
        "/page",
        Route::ok("text/plain", "not gzip").with_header("Content-Encoding", "gzip"),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (_head, mut body) = response(&events);
    assert_eq!(read_all(&mut body), b"");
    assert_eq!(
        complete(&events).error.map(|error| error.code),
        Some(NetErrorCode::ContentDecodingFailed)
    );
}

/// Without a `Content-Type`, the head waits for the start of the body, to
/// sniff it: all of a short body.
#[test]
fn a_head_without_a_type_waits_for_a_short_body() {
    let server = server(&[(
        "/page",
        Route::new(200, "  <p>hi").with_body_delay(Duration::from_millis(300)),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (head, mut body) = response(&events);
    assert_eq!(head.mime_type, "text/html");
    assert_eq!(next_piece(&mut body).unwrap(), b"  <p>hi");
    assert_eq!(read_all(&mut body), b"");
    assert_eq!(complete(&events).error, None);
}

/// ... but only its first 1024 bytes of a long one.
#[test]
fn a_head_without_a_type_waits_for_1024_bytes_only() {
    let mut page = b"<p>".to_vec();
    page.extend(varied(3000));
    let server = server(&[(
        "/page",
        Route::new(200, page.clone()).with_pause_in_body(1500, PAUSE),
    )]);
    let network = network();
    let (_handle, events) = start(
        &network,
        get(&server.url("/page")),
        AtRedirects::Follow,
        false,
    );
    let (head, mut body) = response(&events);
    assert_eq!(head.mime_type, "text/html");
    let mut read = next_piece(&mut body).unwrap();
    assert!(read.len() >= 1024, "{}", read.len());
    assert!(read.len() <= 1500, "{}", read.len());
    read.extend(read_all(&mut body));
    assert_eq!(read, page);
}

/// What a redirect says of the page it leaves isn't waited for.
#[test]
fn a_redirects_body_is_not_read() {
    let server = server(&[
        (
            "/a",
            Route {
                body: b"moved".to_vec(),
                ..Route::redirect(302, "/b").with_body_delay(Duration::from_secs(10))
            },
        ),
        ("/b", Route::ok("text/plain", "here")),
    ]);
    let network = network();
    let (_handle, events) = start(&network, get(&server.url("/a")), AtRedirects::Follow, false);
    let Event::Response(head, mut body) = events
        .recv_timeout(Duration::from_secs(5))
        .expect("the redirect's body wasn't waited for")
    else {
        panic!("expected the response");
    };
    assert_eq!(head.url, server.url("/b"));
    assert_eq!(read_all(&mut body), b"here");
}

/// A file is read a piece at a time, not all of it before the head.
#[test]
fn a_file_comes_in_pieces() {
    let contents = varied(3 * net::BODY_PIPE_CAPACITY);
    let path = std::env::temp_dir().join(format!("machopo-client-{}.txt", std::process::id()));
    std::fs::write(&path, &contents).unwrap();
    let network = network();
    let url = net::file_url(&path).unwrap();
    let (_handle, events) = start(&network, get(&url), AtRedirects::Follow, false);
    let (head, mut body) = response(&events);
    assert_eq!(head.mime_type, "text/plain");
    let mut read = next_piece(&mut body).unwrap();
    assert!(read.len() < net::BODY_PIPE_CAPACITY, "{}", read.len());
    read.extend(read_all(&mut body));
    let _ = std::fs::remove_file(&path);
    assert!(read == contents, "the file as it is");
    let status = complete(&events);
    assert_eq!(status.error, None);
    assert_eq!(status.encoded_body_length, 0);
}
