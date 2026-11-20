//! A host's registrable domain and an origin's site.

use site::{Site, registrable_domain};
use url::{Host, Url};

fn domain(host: &str) -> Option<&str> {
    registrable_domain(&Host::Domain(host))
}

fn site(url: &str) -> Site {
    Site::of_url(&Url::parse(url).unwrap())
}

/// The public suffix and one label more: the list's ICANN part, its
/// wildcards and their exceptions, and names in punycode.
#[test]
fn a_registrable_domain_is_the_public_suffix_and_a_label() {
    for (host, expected) in [
        ("example.com", Some("example.com")),
        ("www.example.com", Some("example.com")),
        ("a.b.example.co.uk", Some("example.co.uk")),
        // `*.ck`, but `!www.ck`.
        ("b.c.ck", Some("b.c.ck")),
        ("a.b.c.ck", Some("b.c.ck")),
        ("a.www.ck", Some("www.ck")),
        // `*.kawasaki.jp`, but `!city.kawasaki.jp`.
        ("a.b.kawasaki.jp", Some("a.b.kawasaki.jp")),
        ("a.city.kawasaki.jp", Some("city.kawasaki.jp")),
        // 公司.cn, a suffix.
        ("a.xn--55qx5d.cn", Some("a.xn--55qx5d.cn")),
        ("b.a.xn--55qx5d.cn", Some("a.xn--55qx5d.cn")),
    ] {
        assert_eq!(domain(host), expected, "{host}");
    }
}

/// A public suffix itself has none, nor has a single label.
#[test]
fn a_public_suffix_has_no_registrable_domain() {
    for host in [
        "com",
        "co.uk",
        "c.ck",
        "xn--55qx5d.cn",
        "localhost",
        "github.io",
    ] {
        assert_eq!(domain(host), None, "{host}");
    }
}

/// The list's private part counts: each of a host's customers is a site of
/// its own.
#[test]
fn private_suffixes_count() {
    assert_eq!(domain("alice.github.io"), Some("alice.github.io"));
    assert_eq!(domain("x.alice.github.io"), Some("alice.github.io"));
    assert_eq!(domain("x.s3.amazonaws.com"), Some("x.s3.amazonaws.com"));
    assert_eq!(domain("s3.amazonaws.com"), None);
}

/// A top-level domain the list doesn't know is a suffix all the same.
#[test]
fn an_unknown_top_level_domain_is_a_suffix() {
    assert_eq!(domain("web-platform.test"), Some("web-platform.test"));
    assert_eq!(domain("www1.web-platform.test"), Some("web-platform.test"));
    assert_eq!(domain("a.b.internal"), Some("b.internal"));
}

/// An address has none (the list would take its last numbers for labels),
/// nor has a name with an empty label; a name's last dot stays.
#[test]
fn what_is_no_name_has_no_registrable_domain() {
    assert_eq!(
        registrable_domain(&Host::Ipv4("127.0.0.1".parse().unwrap())),
        None
    );
    assert_eq!(
        registrable_domain(&Host::Ipv6("::1".parse().unwrap())),
        None
    );
    for host in ["", ".", "a..com", ".example.com", "example.com.."] {
        assert_eq!(domain(host), None, "{host:?}");
    }
    assert_eq!(domain("www.example.com."), Some("example.com."));
}

/// A site is a scheme and a registrable domain: hosts of one are the same
/// site whatever their ports, but not across schemes.
#[test]
fn a_site_is_a_scheme_and_a_registrable_domain() {
    let a = site("https://www.example.com/path");
    assert_eq!(a, site("https://example.com:8443/"));
    assert_eq!(a, site("https://a.b.example.com/"));
    assert_ne!(a, site("http://www.example.com/"));
    assert_ne!(a, site("https://example.org/"));
    assert_eq!(a.serialize(), "https://example.com");
    assert_eq!(
        (a.scheme(), a.registrable_domain_or_host(), a.is_opaque()),
        (Some("https"), Some("example.com"), false)
    );
    // The customers of a private suffix are sites apart.
    assert_ne!(
        site("https://alice.github.io/"),
        site("https://bob.github.io/")
    );
}

/// A host with no registrable domain is its site's: an address, a single
/// label, a public suffix itself.
#[test]
fn a_host_with_no_registrable_domain_is_its_site() {
    for (url, written) in [
        ("http://127.0.0.1:8000/", "http://127.0.0.1"),
        ("http://[::1]:8000/", "http://[::1]"),
        ("http://localhost:3000/", "http://localhost"),
        ("https://github.io/", "https://github.io"),
    ] {
        assert_eq!(site(url).serialize(), written, "{url}");
    }
    assert_ne!(site("http://127.0.0.1/"), site("http://127.0.0.2/"));
    assert_eq!(site("http://127.0.0.1:1/"), site("http://127.0.0.1:2/"));
}

/// An opaque origin's site is its own: the same as itself, and as no
/// other's.
#[test]
fn an_opaque_origins_site_is_its_own() {
    let origin = Url::parse("data:text/plain,a").unwrap().origin();
    let a = Site::of(&origin);
    assert!(a.is_opaque());
    assert_eq!(a, Site::of(&origin));
    assert_eq!(a, Site::of(&origin.clone()));
    assert_ne!(a, site("data:text/plain,a"));
    assert_ne!(a, site("file:///tmp/a.html"));
    assert_eq!(
        (
            a.scheme(),
            a.registrable_domain_or_host(),
            a.serialize().as_str()
        ),
        (None, None, "null")
    );
}
