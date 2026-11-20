//! The site a request is first party to, for `SameSite`.

use site::{Site, SiteForCookies};
use url::{Origin, Url};

fn origin(url: &str) -> Origin {
    Url::parse(url).unwrap().origin()
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

/// A top-level frame's is its own site; a frame's of the top-level site,
/// every frame between of it too, is the top-level site; one of another
/// site, or under one, or an opaque one's, is null.
#[test]
fn a_frames_is_the_top_level_site_if_every_frame_is_of_it() {
    let top = origin("https://www.a.example/");
    let first = SiteForCookies::of_frame(&top, []);
    assert_eq!(first.site(), Some(&Site::of(&top)));
    assert_eq!(
        SiteForCookies::of_frame(&origin("https://sub.a.example:8443/"), [&top]),
        first
    );
    let other = origin("https://b.example/");
    assert!(SiteForCookies::of_frame(&other, [&top]).is_null());
    assert!(SiteForCookies::of_frame(&top, [&other, &top]).is_null());
    let opaque = Url::parse("data:text/html,").unwrap().origin();
    assert!(SiteForCookies::of_frame(&opaque, [&top]).is_null());
    assert!(SiteForCookies::of_frame(&top, [&opaque]).is_null());
    assert!(SiteForCookies::of_frame(&opaque, []).is_null());
    assert!(SiteForCookies::of_site(Site::of(&opaque)).is_null());
    assert!(SiteForCookies::null().is_null());
}

/// A URL is first party to it if it is of its site, its scheme too:
/// `ws:` as `http:`, `wss:` as `https:`; nothing is first party to null.
#[test]
fn a_url_is_first_party_to_its_own_site_scheme_and_all() {
    let site = SiteForCookies::of_site(Site::of_url(&url("https://a.example/")));
    assert!(site.is_first_party(&url("https://x.a.example:444/path")));
    assert!(site.is_first_party(&url("wss://a.example/socket")));
    assert!(!site.is_first_party(&url("http://a.example/")));
    assert!(!site.is_first_party(&url("ws://a.example/")));
    assert!(!site.is_first_party(&url("https://b.example/")));
    assert!(!SiteForCookies::null().is_first_party(&url("https://a.example/")));
    // Schemeless, for the cookie policy: of the registrable domain.
    assert!(site.is_first_party_schemeless(&url("http://x.a.example/")));
    assert!(!site.is_first_party_schemeless(&url("https://b.example/")));
    assert!(!SiteForCookies::null().is_first_party_schemeless(&url("https://a.example/")));
    let http = SiteForCookies::of_site(Site::of_url(&url("http://a.example/")));
    assert!(http.is_first_party(&url("ws://a.example/socket")));
    assert!(!http.is_first_party(&url("wss://a.example/socket")));
}
