//! What a request's entry is kept under (Chromium's
//! `HttpCache::GenerateCacheKey`, with the cache split by
//! `NetworkIsolationKey` as Chrome has it): the top-level frame's site, the
//! frame's site and the URL, so that a page can't tell from the cache what
//! another site's pages loaded.
//!
//! The key is Chromium's string: `1/0/_dk_`, then `s_` for a child frame's
//! document and `cn_` for a main frame's navigation another site started,
//! then the two sites and the URL, each after a space. `1/` is "credentials
//! may be sent" (Chromium splits on it only behind a feature that isn't on
//! by default), `0/` no upload (only `GET`s and `HEAD`s are read from it).
//! A navigation's sites are its URL's at each hop: a main frame's both, a
//! child frame's its own.

use protocol::network::{RequestDestination, ResourceRequest};
use site::Site;
use url::Url;

/// The key of `request`'s hop to `url`; `None` when it has no site to be
/// kept under (an opaque top-level frame or frame, or a request the
/// browser gave no trusted params): such a load goes past the cache, as
/// Chromium's transient keys do.
pub(crate) fn cache_key(request: &ResourceRequest, url: &Url) -> Option<String> {
    let trusted = request.trusted.as_ref()?;
    let url_site = Site::of_url(url);
    if url_site.is_opaque() {
        return None;
    }
    let (top, frame, subframe_document) = match request.destination {
        RequestDestination::Document => (url_site.serialize(), url_site.serialize(), false),
        RequestDestination::Iframe => (trusted.top_frame_site.clone()?, url_site.serialize(), true),
        _ => (
            trusted.top_frame_site.clone()?,
            trusted.frame_site.clone()?,
            false,
        ),
    };
    // A main frame's navigation another site asked for (its initiator: none
    // for the browser's own, the address typed).
    let cross_site_navigation = request.destination == RequestDestination::Document
        && trusted
            .initiator
            .as_deref()
            .is_some_and(|initiator| !same_site(initiator, &url_site));
    let mut spec = url.clone();
    spec.set_fragment(None);
    // A URL with a host always takes these.
    let _ = spec.set_username("");
    let _ = spec.set_password(None);
    Some(format!(
        "1/0/_dk_{}{}{top} {frame} {spec}",
        if subframe_document { "s_" } else { "" },
        if cross_site_navigation { "cn_" } else { "" },
    ))
}

/// Whether the serialized origin `initiator` is of `site`, which isn't
/// opaque (`SchemefulSite::IsSameSite`): an opaque one, `null`, doesn't
/// parse, so is of no site.
fn same_site(initiator: &str, site: &Site) -> bool {
    Url::parse(initiator).is_ok_and(|initiator| Site::of(&initiator.origin()) == *site)
}

#[cfg(test)]
mod tests {
    use protocol::network::TrustedParams;

    use super::*;

    fn request(destination: RequestDestination, trusted: Option<TrustedParams>) -> ResourceRequest {
        let mut request = ResourceRequest::new("https://c.example/x", destination);
        request.trusted = trusted;
        request
    }

    fn subresource(top: &str, frame: &str) -> Option<TrustedParams> {
        Some(TrustedParams {
            top_frame_site: Some(top.to_owned()),
            frame_site: Some(frame.to_owned()),
            ..TrustedParams::default()
        })
    }

    fn url(spec: &str) -> Url {
        Url::parse(spec).expect("a URL")
    }

    #[test]
    fn a_subresource_is_kept_under_its_top_level_and_frame_sites() {
        let image = request(
            RequestDestination::Image,
            subresource("https://a.example", "https://x.example"),
        );
        assert_eq!(
            cache_key(&image, &url("https://img.c.example/i.png")).as_deref(),
            Some("1/0/_dk_https://a.example https://x.example https://img.c.example/i.png")
        );
        // Another frame site, another entry.
        let other = request(
            RequestDestination::Image,
            subresource("https://a.example", "https://y.example"),
        );
        assert_ne!(
            cache_key(&image, &url("https://c.example/i.png")),
            cache_key(&other, &url("https://c.example/i.png"))
        );
    }

    #[test]
    fn the_fragment_and_credentials_dont_count() {
        let script = request(
            RequestDestination::Script,
            subresource("https://a.example", "https://a.example"),
        );
        assert_eq!(
            cache_key(&script, &url("https://u:p@c.example/s.js#top")),
            cache_key(&script, &url("https://c.example/s.js"))
        );
    }

    #[test]
    fn no_site_no_key() {
        let url = url("https://c.example/i.png");
        assert_eq!(
            cache_key(&request(RequestDestination::Image, None), &url),
            None
        );
        let opaque_top = TrustedParams {
            frame_site: Some("https://a.example".to_owned()),
            ..TrustedParams::default()
        };
        assert_eq!(
            cache_key(&request(RequestDestination::Image, Some(opaque_top)), &url),
            None
        );
        let opaque_frame = TrustedParams {
            top_frame_site: Some("https://a.example".to_owned()),
            ..TrustedParams::default()
        };
        assert_eq!(
            cache_key(
                &request(RequestDestination::Image, Some(opaque_frame)),
                &url
            ),
            None
        );
    }

    #[test]
    fn a_main_frames_navigation_is_its_urls_both_and_cn_from_another_site() {
        let navigation = |initiator: Option<&str>| {
            request(
                RequestDestination::Document,
                Some(TrustedParams {
                    top_frame_site: Some("https://elsewhere.example".to_owned()),
                    initiator: initiator.map(str::to_owned),
                    is_main_frame_navigation: true,
                    ..TrustedParams::default()
                }),
            )
        };
        let to = url("https://www.c.example/page");
        let expected = "https://c.example https://c.example https://www.c.example/page";
        assert_eq!(
            cache_key(&navigation(None), &to),
            Some(format!("1/0/_dk_{expected}"))
        );
        assert_eq!(
            cache_key(&navigation(Some("https://c.example")), &to),
            Some(format!("1/0/_dk_{expected}"))
        );
        for other in ["https://a.example", "http://c.example", "null"] {
            assert_eq!(
                cache_key(&navigation(Some(other)), &to),
                Some(format!("1/0/_dk_cn_{expected}")),
                "{other}"
            );
        }
    }

    #[test]
    fn a_child_frames_document_is_its_own_with_s() {
        let iframe = request(
            RequestDestination::Iframe,
            Some(TrustedParams {
                top_frame_site: Some("https://a.example".to_owned()),
                initiator: Some("https://b.example".to_owned()),
                ..TrustedParams::default()
            }),
        );
        assert_eq!(
            cache_key(&iframe, &url("https://x.example/f.html")).as_deref(),
            Some("1/0/_dk_s_https://a.example https://x.example https://x.example/f.html")
        );
    }
}
