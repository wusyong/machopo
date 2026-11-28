//! `Cross-Origin-Resource-Policy` (Fetch's "cross-origin resource policy
//! check", Chromium's `CrossOriginResourcePolicy::IsBlocked`): a server's
//! say that its response is for its own origin or site alone, which a
//! `no-cors` request of another's doesn't get. Each hop's response is
//! checked, a redirect's too, as Fetch's "HTTP fetch" checks each.
//!
//! The embedder policy is an input of the check (Cross-Origin-Embedder-
//! Policy's `require-corp` takes a missing header for `same-origin`);
//! machopo has no COEP, so every document's is `unsafe-none`, and a
//! response without the header is allowed, as Chrome's for a page that
//! sends none.

use url::{Host, Origin, Url};

/// The header's name (lowercase, as the response's headers are kept).
const HEADER: &str = "cross-origin-resource-policy";

/// What a response's header says (Fetch's ABNF: those three, case
/// sensitive; any other value is as none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    SameOrigin,
    SameSite,
    CrossOrigin,
}

/// A document's embedder policy (Cross-Origin-Embedder-Policy's value):
/// machopo's documents' is always `unsafe-none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EmbedderPolicy {
    #[default]
    UnsafeNone,
}

fn policy(headers: &[(String, String)]) -> Option<Policy> {
    // More than one is as none (a list isn't a value of the ABNF);
    // Chromium joins them with ", ", which parses as none too.
    let mut values = headers
        .iter()
        .filter(|(name, _)| name == HEADER)
        .map(|(_, value)| value.trim_matches([' ', '\t']));
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    match value {
        "same-origin" => Some(Policy::SameOrigin),
        "same-site" => Some(Policy::SameSite),
        "cross-origin" => Some(Policy::CrossOrigin),
        _ => None,
    }
}

/// HTML's "schemelessly same site", as Chromium's `ShouldAllowSameSite`
/// has it: of the same registrable domain (or host, where there is none),
/// whatever the scheme; never for an IP address, which serves many sites
/// ("Different sites might be served from the same IP address - they
/// should still be considered to be different sites"), nor an opaque
/// origin.
fn schemelessly_same_site(initiator: &Origin, url: &Url) -> bool {
    let Origin::Tuple(_, initiator_host, _) = initiator else {
        return false;
    };
    let (Host::Domain(initiator_name), Some(Host::Domain(name))) = (initiator_host, url.host())
    else {
        return false;
    };
    let domain = |name: &str| {
        site::Site::of(&Origin::Tuple(
            "https".to_owned(),
            Host::Domain(name.to_owned()),
            443,
        ))
        .registrable_domain_or_host()
        .map(str::to_owned)
    };
    domain(initiator_name) == domain(name)
}

/// Whether the response at `url` with `headers` is blocked for a
/// `no-cors` request from `initiator` (serialized; `null` for an opaque
/// origin) of a document of `embedder` policy (Fetch's "cross-origin
/// resource policy internal check", not for a navigation).
pub(crate) fn blocks(
    initiator: &str,
    url: &Url,
    headers: &[(String, String)],
    embedder: EmbedderPolicy,
) -> bool {
    let policy = match (policy(headers), embedder) {
        (policy, EmbedderPolicy::UnsafeNone) => policy,
    };
    let initiator = match Url::parse(initiator) {
        Ok(initiator) if initiator.origin().is_tuple() => initiator.origin(),
        _ => Origin::new_opaque(),
    };
    match policy {
        None | Some(Policy::CrossOrigin) => false,
        Some(_) if initiator == url.origin() => false,
        Some(Policy::SameOrigin) => true,
        Some(Policy::SameSite) => {
            let secure = |scheme: &str| scheme == "https";
            let initiator_secure =
                matches!(&initiator, Origin::Tuple(scheme, ..) if secure(scheme));
            !(schemelessly_same_site(&initiator, url)
                && (initiator_secure || !secure(url.scheme())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(initiator: &str, url: &str, value: Option<&str>) -> bool {
        let headers: Vec<(String, String)> = value
            .map(|value| (HEADER.to_owned(), value.to_owned()))
            .into_iter()
            .collect();
        blocks(
            initiator,
            &Url::parse(url).unwrap(),
            &headers,
            EmbedderPolicy::UnsafeNone,
        )
    }

    /// No header, `cross-origin`, and any value that isn't one of the
    /// three (case and all): allowed, from anywhere.
    #[test]
    fn what_says_nothing_allows_anyone() {
        for value in [
            None,
            Some("cross-origin"),
            Some("Same-Origin"),
            Some("same-origin, same-site"),
            Some(""),
        ] {
            assert!(
                !blocked("https://b.example", "https://a.example/x", value),
                "{value:?}"
            );
        }
    }

    #[test]
    fn same_origin_is_for_the_origin_alone() {
        let url = "https://a.example/x";
        assert!(!blocked("https://a.example", url, Some("same-origin")));
        assert!(!blocked("https://a.example", url, Some(" same-origin\t")));
        for initiator in [
            "https://www.a.example",
            "http://a.example",
            "https://a.example:444",
            "null",
        ] {
            assert!(blocked(initiator, url, Some("same-origin")), "{initiator}");
        }
    }

    /// The same site whatever the scheme, but an `https:` response not for
    /// an `http:` initiator; never an IP address's other origins, nor an
    /// opaque initiator.
    #[test]
    fn same_site_is_for_the_site_alone() {
        let value = Some("same-site");
        assert!(!blocked(
            "https://www.a.example",
            "https://a.example/x",
            value
        ));
        assert!(!blocked(
            "https://a.example",
            "http://shop.a.example/x",
            value
        ));
        assert!(blocked(
            "http://a.example",
            "https://shop.a.example/x",
            value
        ));
        assert!(!blocked(
            "http://a.example",
            "http://shop.a.example/x",
            value
        ));
        assert!(blocked("https://b.example", "https://a.example/x", value));
        assert!(blocked(
            "http://127.0.0.1:8000",
            "http://127.0.0.1:8001/x",
            value
        ));
        assert!(!blocked(
            "http://127.0.0.1:8000",
            "http://127.0.0.1:8000/x",
            value
        ));
        assert!(blocked("null", "https://a.example/x", value));
        // `localhost` has no registrable domain: its host is its site.
        assert!(!blocked(
            "http://localhost:1",
            "http://localhost:2/x",
            value
        ));
        assert!(blocked(
            "http://a.localhost:1",
            "http://localhost:2/x",
            value
        ));
    }

    /// Two of the header are as none: allowed.
    #[test]
    fn more_than_one_is_none() {
        let headers = vec![
            (HEADER.to_owned(), "same-origin".to_owned()),
            (HEADER.to_owned(), "same-origin".to_owned()),
        ];
        assert!(!blocks(
            "https://b.example",
            &Url::parse("https://a.example/").unwrap(),
            &headers,
            EmbedderPolicy::UnsafeNone
        ));
    }
}
