//! Fetch's header rules `protocol::network` holds, which the renderer
//! (`Headers`, `fetch()`) and the network service share: the
//! CORS-safelisted request-headers, those a `no-cors` request may carry,
//! and what a CORS-filtered response keeps.

use protocol::network::{
    CredentialsMode, cors_filtered_headers, cors_unsafe_request_header_names,
    is_cors_safelisted_method, is_cors_safelisted_request_header,
    is_no_cors_safelisted_request_header, is_privileged_no_cors_request_header_name,
};

/// Each name's rule, its value at most 128 bytes long.
#[test]
fn cors_safelisted_request_headers() {
    let safelisted = [
        ("Accept", "text/html, */*"),
        ("Accept-Language", "en-US;q=0.5, *"),
        ("content-language", "de"),
        ("Content-Type", "text/plain;charset=UTF-8"),
        ("Content-Type", "Multipart/Form-Data; boundary=x"),
        ("Content-Type", "application/x-www-form-urlencoded"),
        ("Range", "bytes=0-"),
        ("Range", "bytes=10-20"),
    ];
    for (name, value) in safelisted {
        assert!(
            is_cors_safelisted_request_header(name, value),
            "{name}: {value}"
        );
    }
    let unsafe_ones = [
        ("Accept", "\""),
        ("Accept", "text/html\u{1}"),
        ("Accept-Language", "en@"),
        ("Content-Language", "de/x"),
        ("Content-Type", "application/json"),
        ("Content-Type", "text/plain(x)"),
        ("Content-Type", "not a type"),
        ("Range", "bytes=-500"),
        ("Range", "bytes=20-10"),
        ("Range", "bytes=-"),
        ("Range", "items=0-1"),
        ("Range", "bytes=0-1,3-4"),
        ("X-Machopo", "1"),
    ];
    for (name, value) in unsafe_ones {
        assert!(
            !is_cors_safelisted_request_header(name, value),
            "{name}: {value}"
        );
    }
    assert!(is_cors_safelisted_request_header(
        "Accept",
        &"a".repeat(128)
    ));
    assert!(!is_cors_safelisted_request_header(
        "Accept",
        &"a".repeat(129)
    ));
}

/// A `no-cors` request's headers: the four names, with safelisted values;
/// `Range` is the browser's own.
#[test]
fn no_cors_safelisted_request_headers() {
    assert!(is_no_cors_safelisted_request_header(
        "Content-Type",
        "text/plain"
    ));
    assert!(!is_no_cors_safelisted_request_header(
        "Content-Type",
        "text/html"
    ));
    assert!(!is_no_cors_safelisted_request_header("Range", "bytes=0-1"));
    assert!(is_privileged_no_cors_request_header_name("RANGE"));
    assert!(!is_privileged_no_cors_request_header_name("Accept"));
    assert!(is_cors_safelisted_method("POST"));
    assert!(!is_cors_safelisted_method("PUT"));
    assert!(!is_cors_safelisted_method("post"));
}

/// The names a preflight asks for: the unsafe ones, lowercase, sorted,
/// once each; all of them once the safelisted values pass 1024 bytes.
#[test]
fn cors_unsafe_request_header_names_of_a_list() {
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(
        cors_unsafe_request_header_names(&[
            pair("X-B", "1"),
            pair("Accept", "*/*"),
            pair("Content-Type", "application/json"),
            pair("x-b", "2"),
        ]),
        ["content-type", "x-b"]
    );
    let long = "a".repeat(100);
    let many: Vec<(String, String)> = (0..11).map(|_| pair("Accept-Language", &long)).collect();
    assert_eq!(cors_unsafe_request_header_names(&many), ["accept-language"]);
    assert!(cors_unsafe_request_header_names(&many[..10]).is_empty());
}

/// A CORS-filtered response keeps the safelisted headers and those the
/// server exposes, by name in any case; all of them for `*`, unless the
/// request's credentials are included.
#[test]
fn a_cors_filtered_responses_headers() {
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    let headers = |expose: &str| {
        vec![
            pair("content-type", "text/plain"),
            pair("cache-control", "no-store"),
            pair("x-secret", "1"),
            pair("x-other", "2"),
            pair("access-control-expose-headers", expose),
        ]
    };
    let names = |headers: Vec<(String, String)>| -> Vec<String> {
        headers.into_iter().map(|(name, _)| name).collect()
    };
    let same_origin = CredentialsMode::SameOrigin;
    assert_eq!(
        names(cors_filtered_headers(headers(""), same_origin)),
        ["content-type", "cache-control"]
    );
    assert_eq!(
        names(cors_filtered_headers(headers(" X-Secret , "), same_origin)),
        ["content-type", "cache-control", "x-secret"]
    );
    assert_eq!(
        names(cors_filtered_headers(headers("*"), same_origin)).len(),
        5
    );
    assert_eq!(
        names(cors_filtered_headers(
            headers("*"),
            CredentialsMode::Include
        )),
        ["content-type", "cache-control"]
    );
    assert_eq!(
        names(cors_filtered_headers(
            headers("*, x-other"),
            CredentialsMode::Include
        )),
        ["content-type", "cache-control", "x-other"]
    );
}
