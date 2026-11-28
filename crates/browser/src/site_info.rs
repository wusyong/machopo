//! Which site a renderer holds, and whether a main frame's navigation
//! commits in the renderer the tab has or in a new one (Chromium's
//! `SiteInfo` and the part of `RenderFrameHostManager::
//! ShouldSwapBrowsingInstancesForNavigation` a tab without openers or
//! out-of-process frames needs).

use site::Site;

use crate::frame_tree::CommittedOrigin;

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
    /// The site of a document at `url`, committed with `origin`: whose its
    /// content is (Chromium's `SiteInfo`, an `about:blank`'s by its origin,
    /// as `GetPossiblyOverriddenOriginFromUrl` has it). An `about:blank` of
    /// its initiator's origin is that origin's site (a local file's, the
    /// local files'); one of an opaque origin is empty, no one's, and has
    /// none: it goes in whatever renderer the tab has. An error page is its
    /// URL's, where it failed to go: its content is the browser's.
    pub(crate) fn of_document(url: &str, origin: &CommittedOrigin) -> Option<Self> {
        if crate::loader::about_path(url) == Some("blank") {
            return match &origin.origin {
                tuple @ url::Origin::Tuple(..) => Some(Self::Web(Site::of(tuple))),
                url::Origin::Opaque(_) => origin.local.then_some(Self::File),
            };
        }
        Self::of_url(url)
    }

    /// The site of a document at `url` (`about:blank` aside, which goes by
    /// its origin).
    fn of_url(url: &str) -> Option<Self> {
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

/// Whether a renderer locked to `lock` (none yet: the one a tab was made
/// with, which takes the first site it is given) takes a main frame's
/// document of `site` (`SiteInfo::of_document`): one of no site, or of its
/// own. One it doesn't goes in a new renderer, and is a bad message where
/// it comes anyway: whether to swap and the lock's check are the one
/// comparison, so the browser never sends a document where it would
/// refuse it.
pub(crate) fn admits(lock: Option<&SiteInfo>, site: Option<&SiteInfo>) -> bool {
    match (lock, site) {
        (Some(lock), Some(site)) => lock == site,
        _ => true,
    }
}

/// Whether a renderer locked to `lock` may be given what goes by the
/// top-level site `top` (serialized; none for an opaque one): a loader's,
/// a storage area's (Chromium's process lock's jail check, by the
/// top-level site while out-of-process frames aren't). An opaque one's
/// any renderer may: under it there are no cookies (no site for cookies)
/// and its frames' storage is under that opaque site alone, no other
/// renderer's. Else the renderer's lock is to be that site: not a local
/// file's or a page of the browser's own (whose documents are opaque), nor
/// none yet (Chromium's citadel check: every renderer is locked by the
/// first document with a site it commits, before it loads it).
pub(crate) fn allows(lock: Option<&SiteInfo>, top: Option<&str>) -> bool {
    let Some(top) = top else {
        return true;
    };
    matches!(lock, Some(SiteInfo::Web(site)) if !site.is_opaque() && site.serialize() == top)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(url: &str) -> CommittedOrigin {
        CommittedOrigin {
            origin: url::Url::parse(url).unwrap().origin(),
            local: url.starts_with("file:"),
        }
    }

    /// The site of a document at `url`, of its URL's origin.
    fn site(url: &str) -> Option<SiteInfo> {
        SiteInfo::of_document(url, &origin(url))
    }

    /// Whether a renderer locked to `from`'s site takes a document at `to`.
    fn takes(from: &str, to: &str) -> bool {
        admits(site(from).as_ref(), site(to).as_ref())
    }

    #[test]
    fn another_site_is_another_renderers_and_the_same_site_stays() {
        assert!(!takes("https://a.example/", "https://b.example/"));
        assert!(takes("https://a.example/x", "https://a.example/y"));
        assert!(takes(
            "https://www.a.example/",
            "https://shop.a.example:8443/"
        ));
        // The scheme is the site's: http to https is another.
        assert!(!takes("http://a.example/", "https://a.example/"));
        assert!(!takes("https://127.0.0.1/", "https://127.0.0.2/"));
    }

    #[test]
    fn a_renderer_with_no_site_yet_takes_the_first() {
        assert!(admits(None, site("https://a.example/").as_ref()));
        assert!(admits(None, site("data:text/html,x").as_ref()));
    }

    /// An `about:blank` is of the origin it commits with: its initiator's
    /// site's renderer takes it, another's doesn't; an opaque one, no
    /// one's, any takes, and it leaves the renderer as it was.
    #[test]
    fn about_blank_is_its_origins() {
        let blank = |of: &str| SiteInfo::of_document("about:blank#x", &origin(of));
        assert_eq!(blank("https://c.example/"), site("https://c.example/"));
        assert_eq!(blank("file:///a/x.html"), Some(SiteInfo::File));
        assert_eq!(
            SiteInfo::of_document("about:blank", &CommittedOrigin::opaque()),
            None
        );
        let a = site("https://a.example/");
        assert!(admits(a.as_ref(), blank("https://www.a.example/").as_ref()));
        assert!(!admits(a.as_ref(), blank("https://c.example/").as_ref()));
        assert!(admits(a.as_ref(), None));
        assert!(admits(site("about:demo").as_ref(), None));
    }

    #[test]
    fn every_page_of_the_browsers_is_a_site_of_its_own() {
        assert!(!takes("https://a.example/", "about:demo"));
        assert!(!takes("about:demo", "https://a.example/"));
        assert!(!takes("about:demo", "about:other"));
        assert!(takes("about:demo", "about:demo?x"));
    }

    #[test]
    fn every_local_file_is_one_site() {
        assert!(takes("file:///a/x.html", "file:///b/y.html"));
        assert!(!takes("file:///a/x.html", "https://a.example/"));
        assert!(!takes("https://a.example/", "file:///a/x.html"));
    }

    /// A URL of an opaque origin is a site of its own each time it is
    /// worked out (a history entry keeps the one it committed as); a blob
    /// URL is its origin's.
    #[test]
    fn an_opaque_origins_url_is_the_same_site_as_none() {
        assert!(!takes("https://a.example/", "data:text/html,x"));
        assert!(!takes("data:text/html,x", "data:text/html,x"));
        let data = site("data:text/html,x");
        assert!(admits(data.as_ref(), data.as_ref()), "the same one");
        assert!(!takes("https://a.example/", "blob:null/0"));
        assert!(takes("https://a.example/", "blob:https://a.example/0"));
        assert!(!takes("https://a.example/", "blob:https://b.example/0"));
    }

    /// An error page is of where it failed to go.
    #[test]
    fn an_error_page_is_its_urls() {
        let error = SiteInfo::of_document("https://b.example/x", &CommittedOrigin::opaque());
        assert_eq!(error, site("https://b.example/"));
    }

    /// What goes by an opaque top-level site any renderer is given; by a
    /// web site's, only one locked to that site, not one locked to none,
    /// to another, to the local files or to a page of the browser's.
    #[test]
    fn a_renderer_is_given_only_its_top_level_sites() {
        let a = site("https://a.example/");
        for lock in [
            None,
            a.as_ref(),
            Some(&SiteInfo::File),
            site("data:,").as_ref(),
        ] {
            assert!(allows(lock, None), "{lock:?}");
        }
        assert!(allows(a.as_ref(), Some("https://a.example")));
        for lock in [
            None,
            site("https://b.example/").as_ref(),
            site("http://a.example/").as_ref(),
            Some(&SiteInfo::File),
            site("about:demo").as_ref(),
            site("data:,").as_ref(),
        ] {
            assert!(!allows(lock, Some("https://a.example")), "{lock:?}");
        }
        // An opaque lock's serialization is no site's.
        assert!(!allows(site("data:,").as_ref(), Some("null")));
    }
}
