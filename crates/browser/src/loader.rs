//! What a navigation commits besides a network response
//! (`//content/browser/loader`): the built-in `about:` pages, and the page
//! shown when a load fails. Everything else is loaded by `net`.
//!
//! The renderer never touches the file system or the network: the browser
//! fetches the body and hands it over with `CommitNavigation`, and
//! subresources go through the browser's IO thread to `net` too. A
//! sandboxed renderer process then needs no change.

use protocol::network::NetError;

pub use net::{file_path, file_url};

/// The built-in demo page, shown by `machopo` without arguments.
const DEMO_HTML: &str = include_str!("../../../resources/demo.html");

/// The markup of a built-in page, or `None` if `url` isn't one.
pub fn about_page(url: &str) -> Option<&'static str> {
    match url {
        "about:blank" => Some(""),
        "about:demo" => Some(DEMO_HTML),
        _ => None,
    }
}

/// A page for a failed load, like Chromium's net error page, titled with
/// the error code.
pub fn error_page(error: &NetError) -> String {
    let detail = error
        .detail
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let code = error.code.name();
    format!("<!DOCTYPE html><title>{code}</title><h1>{code}</h1><p>{detail}</p>")
}

#[cfg(test)]
mod tests {
    use protocol::network::NetErrorCode;

    use super::*;

    #[test]
    fn built_in_pages() {
        assert_eq!(about_page("about:blank"), Some(""));
        assert!(about_page("about:demo").unwrap().contains("<title>"));
        assert_eq!(about_page("about:nothing"), None);
        assert_eq!(about_page("https://example.com/"), None);
    }

    #[test]
    fn error_pages_are_titled_with_the_code_and_escaped() {
        let page = error_page(&NetError::new(NetErrorCode::FileNotFound, "<a> & b"));
        assert!(page.contains("<title>ERR_FILE_NOT_FOUND</title>"));
        assert!(page.contains("&lt;a&gt; &amp; b"));
    }
}
