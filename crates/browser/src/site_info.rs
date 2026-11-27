//! Which site a renderer holds, and whether a main frame's navigation
//! commits in the renderer the tab has or in a new one (Chromium's
//! `SiteInfo` and the part of `RenderFrameHostManager::
//! ShouldSwapBrowsingInstancesForNavigation` a tab without openers or
//! out-of-process frames needs).

use protocol::renderer::FrameLoadType;
use site::Site;

/// The site a document is of, for which renderer it goes in (Chromium's
/// `SiteInfo`): a renderer holds one site's documents alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SiteInfo {
    /// A web page's: the scheme and registrable domain of its URL's
    /// origin (`site::Site`). One of an opaque origin (a `data:` URL, a
    /// `blob:null/…` one, one that doesn't parse) is the same as no other,
    /// itself not either once it is made again.
    Web(Site),
    /// Every `file:` URL's, as one (Chromium's `SiteInfo` gives them the
    /// one site `file:`): a renderer that may read local files may ask the
    /// browser for any, so two in two renderers keep nothing apart.
    File,
    /// A page of the browser's own (`about:` but `about:blank`): each by
    /// its name, as Chrome's WebUI is by its `chrome://` host.
    BuiltIn(String),
}

impl SiteInfo {
    /// The site of a document at `url`, an error page's too (its URL is
    /// where it failed to go): none for `about:blank`, which takes no site
    /// and goes in whatever renderer the tab has.
    pub(crate) fn of_url(url: &str) -> Option<Self> {
        if let Some(path) = crate::loader::about_path(url) {
            return (path != "blank").then(|| Self::BuiltIn(path.to_owned()));
        }
        let Ok(parsed) = url::Url::parse(url) else {
            return Some(Self::Web(Site::of(&url::Origin::new_opaque())));
        };
        if parsed.scheme() == "file" {
            return Some(Self::File);
        }
        Some(Self::Web(Site::of_url(&parsed)))
    }
}

/// Whether a main frame navigation of `navigation_type` to a document of
/// `site` (`SiteInfo::of_url`) commits in a new renderer, the tab's
/// holding `current` (none yet: the renderer it was made with, which
/// takes the first site it is given). A reload stays, whatever its URL
/// (one of a `data:` URL is the same document again); `about:blank` stays,
/// and leaves the renderer's site as it was; any other goes where its site
/// is other than the renderer's. A traversal goes by its entry's URL, as a
/// navigation to it would.
pub(crate) fn swaps(
    current: Option<&SiteInfo>,
    navigation_type: FrameLoadType,
    site: Option<&SiteInfo>,
) -> bool {
    if matches!(
        navigation_type,
        FrameLoadType::Reload | FrameLoadType::ReloadBypassingCache
    ) {
        return false;
    }
    match (current, site) {
        (Some(current), Some(site)) => current != site,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(url: &str) -> Option<SiteInfo> {
        SiteInfo::of_url(url)
    }

    /// Whether a new entry's navigation from a renderer holding `from`'s
    /// site to `to` swaps.
    fn swaps_from(from: &str, to: &str) -> bool {
        swaps(
            site(from).as_ref(),
            FrameLoadType::Standard,
            site(to).as_ref(),
        )
    }

    #[test]
    fn another_site_swaps_and_the_same_site_stays() {
        assert!(swaps_from("https://a.example/", "https://b.example/"));
        assert!(!swaps_from("https://a.example/x", "https://a.example/y"));
        assert!(!swaps_from(
            "https://www.a.example/",
            "https://shop.a.example:8443/"
        ));
        // The scheme is the site's: http to https is another.
        assert!(swaps_from("http://a.example/", "https://a.example/"));
        assert!(swaps_from("https://127.0.0.1/", "https://127.0.0.2/"));
    }

    #[test]
    fn a_renderer_with_no_site_yet_takes_the_first() {
        assert!(!swaps(
            None,
            FrameLoadType::Standard,
            site("https://a.example/").as_ref()
        ));
    }

    #[test]
    fn about_blank_stays_whatever_the_renderer_holds() {
        assert_eq!(site("about:blank"), None);
        assert_eq!(site("about:blank#x"), None);
        assert!(!swaps_from("https://a.example/", "about:blank"));
        assert!(!swaps_from("about:demo", "about:blank"));
    }

    #[test]
    fn every_page_of_the_browsers_is_a_site_of_its_own() {
        assert!(swaps_from("https://a.example/", "about:demo"));
        assert!(swaps_from("about:demo", "https://a.example/"));
        assert!(swaps_from("about:demo", "about:other"));
        assert!(!swaps_from("about:demo", "about:demo?x"));
    }

    #[test]
    fn every_local_file_is_one_site() {
        assert!(!swaps_from("file:///a/x.html", "file:///b/y.html"));
        assert!(swaps_from("file:///a/x.html", "https://a.example/"));
        assert!(swaps_from("https://a.example/", "file:///a/x.html"));
    }

    #[test]
    fn an_opaque_origins_url_is_the_same_site_as_none() {
        assert!(swaps_from("https://a.example/", "data:text/html,x"));
        assert!(swaps_from("data:text/html,x", "data:text/html,x"));
        assert!(swaps_from("https://a.example/", "blob:null/0"));
        // A blob URL is its origin's.
        assert!(!swaps_from(
            "https://a.example/",
            "blob:https://a.example/0"
        ));
        assert!(swaps_from("https://a.example/", "blob:https://b.example/0"));
    }

    #[test]
    fn a_reload_stays() {
        for reload in [FrameLoadType::Reload, FrameLoadType::ReloadBypassingCache] {
            let data = site("data:text/html,x");
            assert!(!swaps(
                data.as_ref(),
                reload,
                site("data:text/html,x").as_ref()
            ));
        }
    }

    #[test]
    fn a_traversal_goes_by_its_entrys_url() {
        let a = site("https://a.example/");
        assert!(swaps(
            a.as_ref(),
            FrameLoadType::BackForward,
            site("https://b.example/").as_ref()
        ));
        assert!(!swaps(
            a.as_ref(),
            FrameLoadType::BackForward,
            site("https://a.example/z").as_ref()
        ));
    }
}
