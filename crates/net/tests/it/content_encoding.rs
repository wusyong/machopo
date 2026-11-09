//! Bodies sent with a `Content-Encoding`: the network service offers the
//! codings it undoes, undoes them itself, and keeps the headers as sent and
//! how many bytes came, against a local test server.

use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

use crossbeam_channel::unbounded;
use net::test_server::{Route, TestServer};
use net::{NetworkContext, NetworkContextParams};
use protocol::network::{LoadResult, NetErrorCode, RequestDestination, ResourceRequest};

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn load(request: ResourceRequest) -> LoadResult {
    let network = NetworkContext::start(NetworkContextParams::default()).unwrap();
    let (tx, rx) = unbounded();
    network.fetch(request, move |result| {
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the network service answers")
}

fn get(url: &str) -> LoadResult {
    load(ResourceRequest::new(url, RequestDestination::Empty))
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn a_gzipped_body_is_undone_and_its_headers_and_size_kept() {
    let text = "hello, ".repeat(100);
    let compressed = gzip(text.as_bytes());
    let server = server(&[(
        "/text",
        Route::ok("text/plain", compressed.clone()).with_header("Content-Encoding", "gzip"),
    )]);
    let response = get(&server.url("/text")).unwrap();
    assert_eq!(&response.body[..], text.as_bytes());
    assert!(
        response
            .headers
            .contains(&("content-encoding".into(), "gzip".into()))
    );
    assert!(
        response
            .headers
            .contains(&("content-length".into(), compressed.len().to_string()))
    );
    assert_eq!(response.encoded_body_length, compressed.len() as u64);
    // "HTTP/1.1 200 OK", and each header the server sent, as lines.
    let head: usize = "HTTP/1.1 200 OK\r\n\r\n".len()
        + response
            .headers
            .iter()
            .map(|(name, value)| name.len() + value.len() + 4)
            .sum::<usize>();
    assert_eq!(response.encoded_headers_length, head as u64);
    assert_eq!(
        server.requests()[0].header("accept-encoding"),
        Some("gzip, deflate, br")
    );
}

#[test]
fn a_range_asks_for_no_coding() {
    let server = server(&[("/text", Route::ok("text/plain", "hello"))]);
    let mut request = ResourceRequest::new(server.url("/text"), RequestDestination::Empty);
    request.headers.push(("Range".into(), "bytes=0-1".into()));
    load(request).unwrap();
    assert_eq!(
        server.requests()[0].header("accept-encoding"),
        Some("identity")
    );
}

#[test]
fn a_body_that_doesnt_decode_is_a_network_error() {
    let server = server(&[(
        "/bad",
        Route::ok("text/plain", "not gzip").with_header("Content-Encoding", "gzip"),
    )]);
    let error = get(&server.url("/bad")).unwrap_err();
    assert_eq!(error.code, NetErrorCode::ContentDecodingFailed);
}

#[test]
fn a_coding_not_offered_is_left_as_it_came() {
    let server = server(&[(
        "/odd",
        Route::ok("text/plain", "raw bytes").with_header("Content-Encoding", "compress"),
    )]);
    let response = get(&server.url("/odd")).unwrap();
    assert_eq!(&response.body[..], b"raw bytes");
    assert_eq!(response.encoded_body_length, 9);
}

#[test]
fn a_local_scheme_takes_nothing_on_the_wire() {
    let response = get("data:text/plain,hi").unwrap();
    assert_eq!(
        (
            response.encoded_headers_length,
            response.encoded_body_length
        ),
        (0, 0)
    );
}
