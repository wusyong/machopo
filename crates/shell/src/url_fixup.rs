//! What the address bar navigates to for what was typed
//! (`url_formatter::FixupURL`, without search: there are no suggestions).

use std::path::Path;

use url::{Host, Url};

/// The URL for `input`, or `None` for nothing (only white space).
///
/// - A URL with a scheme the browser loads (`http`, `https`, `file`,
///   `about`) is taken as it is.
/// - A local path that exists becomes a `file:` URL; a relative one is
///   resolved against `cwd`.
/// - Anything else is taken as a host and path: `http://` for `localhost`
///   and IP addresses (local servers rarely have TLS), `https://` for the
///   rest. With no such host it ends as `ERR_NAME_NOT_RESOLVED`.
pub fn fixup_url(input: &str, cwd: &Path) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(input)
        && matches!(url.scheme(), "http" | "https" | "file" | "about")
    {
        return Some(url.into());
    }
    let path = cwd.join(input);
    if (input.starts_with('/')
        || input.starts_with('.')
        || input.contains(std::path::MAIN_SEPARATOR))
        && path.exists()
        && let Some(url) = path
            .canonicalize()
            .ok()
            .and_then(|path| browser::loader::file_url(&path))
    {
        return Some(url);
    }
    let scheme = match Url::parse(&format!("http://{input}")) {
        Ok(url) if is_local(&url) => "http",
        Ok(_) => "https",
        // Not even a host (`a b`): let the load fail with the host it got.
        Err(_) => "https",
    };
    Some(format!("{scheme}://{input}"))
}

fn is_local(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(_) | Host::Ipv6(_)) => true,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixup(input: &str) -> Option<String> {
        fixup_url(input, Path::new(env!("CARGO_MANIFEST_DIR")))
    }

    #[test]
    fn urls_with_a_scheme_stay() {
        assert_eq!(
            fixup("  https://a.test/x?y#z "),
            Some("https://a.test/x?y#z".into())
        );
        assert_eq!(fixup("HTTP://A.test"), Some("http://a.test/".into()));
        assert_eq!(fixup("about:blank"), Some("about:blank".into()));
        assert_eq!(
            fixup("file:///tmp/x.html"),
            Some("file:///tmp/x.html".into())
        );
        assert_eq!(fixup("   "), None);
    }

    #[test]
    fn hosts_get_https_and_local_ones_http() {
        assert_eq!(fixup("example.com"), Some("https://example.com".into()));
        assert_eq!(
            fixup("text.npr.org/x"),
            Some("https://text.npr.org/x".into())
        );
        // `localhost:8000` parses as a URL with the scheme `localhost`.
        assert_eq!(
            fixup("localhost:8000/a"),
            Some("http://localhost:8000/a".into())
        );
        assert_eq!(
            fixup("127.0.0.1:8000"),
            Some("http://127.0.0.1:8000".into())
        );
        assert_eq!(fixup("[::1]:8000"), Some("http://[::1]:8000".into()));
        // No search: words are a host too, which won't resolve.
        assert_eq!(
            fixup("machopo browser"),
            Some("https://machopo browser".into())
        );
    }

    #[test]
    fn existing_paths_become_file_urls() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let url = browser::loader::file_url(&manifest.canonicalize().unwrap()).unwrap();
        assert_eq!(fixup(manifest.to_str().unwrap()), Some(url.clone()));
        assert_eq!(fixup("./Cargo.toml"), Some(url));
        // A path that isn't there is a host.
        assert_eq!(
            fixup("./missing.html"),
            Some("https://./missing.html".into())
        );
    }
}
