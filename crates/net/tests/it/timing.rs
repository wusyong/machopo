//! When the parts of a load happened (`ResourceResponse::timing`), where
//! it came from and how: against a local test server that keeps
//! connections alive, so a second request can reuse the first's.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use crossbeam_channel::unbounded;
use ipc::TimeTicks;
use net::test_server::{Route, TestServer};
use net::{NetworkContext, NetworkContextParams};
use protocol::network::{LoadTiming, RequestDestination, ResourceRequest, ResourceResponse};

/// A network service that resolves `timing.test` to this machine by its
/// rules.
fn network() -> NetworkContext {
    let params = NetworkContextParams {
        host_resolver_rules: net::HostMappingRules::parse("MAP timing.test 127.0.0.1").unwrap(),
        ..Default::default()
    };
    NetworkContext::start(params).unwrap()
}

fn get(network: &NetworkContext, url: &str) -> ResourceResponse {
    let (tx, rx) = unbounded();
    network.fetch(
        ResourceRequest::new(url, RequestDestination::Document),
        move |result| {
            let _ = tx.send(result);
        },
    );
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the network service answers")
        .expect("the load succeeds")
}

fn keep_alive_server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start_keep_alive(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

/// The server's address, from one of its URLs.
fn address(server: &TestServer) -> SocketAddr {
    let url = url::Url::parse(&server.url("/")).unwrap();
    SocketAddr::new(
        url.host_str().unwrap().parse().unwrap(),
        url.port().unwrap(),
    )
}

fn timing_of(response: &ResourceResponse) -> LoadTiming {
    response.timing.expect("a load is timed")
}

/// Each part comes after the one before it, all within `[from, to]`.
fn assert_in_order(timing: &LoadTiming, from: TimeTicks, to: TimeTicks) {
    let mut points = vec![timing.request_start];
    if let Some(dns) = timing.dns {
        points.extend([dns.start, dns.end]);
    }
    if let Some(connect) = timing.connect {
        points.extend([connect.start, connect.end]);
    }
    points.extend([timing.receive_headers_end, timing.response_end]);
    assert!(
        points.windows(2).all(|pair| pair[0] <= pair[1]),
        "in order: {timing:?}"
    );
    assert!(from <= timing.request_start, "{from:?} <= {timing:?}");
    assert!(timing.response_end <= to, "{timing:?} <= {to:?}");
}

#[test]
fn a_request_that_opens_a_connection_times_it_and_one_that_reuses_it_doesnt() {
    let server = keep_alive_server(&[("/a", Route::ok("text/plain", "a"))]);
    let network = network();
    let url = server.url_for_host("timing.test", "/a");

    let before = TimeTicks::now();
    let first = get(&network, &url);
    let second = get(&network, &url);
    let after = TimeTicks::now();

    let (first_timing, second_timing) = (first.timing.unwrap(), second.timing.unwrap());
    let requests = server.requests();
    assert_eq!(
        requests.iter().map(|r| r.connection).collect::<Vec<_>>(),
        [1, 1],
        "the second request reused the first's connection"
    );
    assert!(first_timing.dns.is_some(), "{:?}", first_timing);
    assert!(first_timing.connect.is_some(), "{:?}", first_timing);
    assert_eq!(
        first_timing.connect.unwrap().start,
        first_timing.dns.unwrap().end,
        "connecting starts where resolving ended"
    );
    assert_in_order(&first_timing, before, after);
    assert_eq!((second_timing.dns, second_timing.connect), (None, None));
    assert_in_order(&second_timing, first_timing.response_end, after);
    for response in [&first, &second] {
        assert_eq!(response.remote_address, Some(address(&server)));
        assert_eq!(response.protocol, "http/1.1");
    }
}

#[test]
fn an_ip_address_is_connected_to_without_resolving() {
    let server = keep_alive_server(&[("/a", Route::ok("text/plain", "a"))]);
    let network = network();
    let response = get(&network, &server.url("/a"));
    assert_eq!(timing_of(&response).dns, None);
    assert!(
        timing_of(&response).connect.is_some(),
        "{:?}",
        timing_of(&response)
    );
}

#[test]
fn waiting_for_the_server_comes_between_the_connection_and_the_head() {
    let wait = Duration::from_millis(150);
    let server = keep_alive_server(&[("/slow", Route::ok("text/plain", "a").with_delay(wait))]);
    let network = network();
    let before = TimeTicks::now();
    let response = get(&network, &server.url_for_host("timing.test", "/slow"));
    let timing = timing_of(&response);
    assert_in_order(&timing, before, TimeTicks::now());
    assert!(
        timing
            .receive_headers_end
            .saturating_since(timing.connect.unwrap().end)
            >= wait,
        "{timing:?}"
    );
}

#[test]
fn the_response_ends_once_its_body_has_come() {
    let wait = Duration::from_millis(150);
    let server = keep_alive_server(&[(
        "/slow-body",
        Route::ok("text/plain", "body").with_body_delay(wait),
    )]);
    let network = network();
    let before = TimeTicks::now();
    let response = get(&network, &server.url("/slow-body"));
    let timing = timing_of(&response);
    assert_eq!(&response.body[..], b"body");
    assert_in_order(&timing, before, TimeTicks::now());
    assert!(
        timing
            .response_end
            .saturating_since(timing.receive_headers_end)
            >= wait,
        "{timing:?}"
    );
}

#[test]
fn a_local_scheme_is_timed_as_a_whole_and_named() {
    let network = network();
    let before = TimeTicks::now();
    let response = get(&network, "data:text/plain,hi");
    assert_eq!(response.protocol, "data");
    assert_eq!(response.remote_address, None);
    assert_eq!(
        (timing_of(&response).dns, timing_of(&response).connect),
        (None, None)
    );
    assert_eq!(
        timing_of(&response).receive_headers_end,
        timing_of(&response).response_end
    );
    assert_in_order(&timing_of(&response), before, TimeTicks::now());

    let path = std::env::temp_dir().join(format!("machopo-timing-{}.txt", std::process::id()));
    std::fs::write(&path, "file").unwrap();
    let before = TimeTicks::now();
    let response = get(&network, &net::file_url(&path).unwrap());
    let _ = std::fs::remove_file(&path);
    assert_eq!(response.protocol, "file");
    assert_in_order(&timing_of(&response), before, TimeTicks::now());

    let url = "blob:http://timing.test/1";
    network.register_blob_url(1, url, "text/plain".into(), b"blob"[..].into());
    let before = TimeTicks::now();
    let response = network
        .load_blob_url(
            &ResourceRequest::new(url, RequestDestination::Empty),
            Some(1),
        )
        .unwrap();
    assert_eq!(response.protocol, "blob");
    assert_in_order(&timing_of(&response), before, TimeTicks::now());
}
