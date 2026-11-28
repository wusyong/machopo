//! Whether a response may be shown in a child frame of its ancestors
//! (Chromium's `AncestorThrottle`, which the browser runs for a child
//! frame's navigation's final response): CSP's `frame-ancestors`, then
//! `X-Frame-Options`, which an enforced `frame-ancestors` overrides.

use url::{Origin, Url};

use crate::csp::{self, Disposition};

/// Why a response is refused its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// An enforced `frame-ancestors` doesn't allow an ancestor's origin.
    FrameAncestors,
    /// `X-Frame-Options: DENY`.
    Deny,
    /// `X-Frame-Options: SAMEORIGIN`, and an ancestor is of another
    /// origin.
    SameOrigin,
    /// `X-Frame-Options` with values that conflict (two of them, at least
    /// one a keyword), taken as `DENY`.
    Conflict,
}

/// HTML's "check a navigation response's adherence to
/// `X-Frame-Options`", its values got, decoded and split: none or `ALLOWALL`
/// or what isn't a keyword allows any frame; `DENY` none; `SAMEORIGIN`
/// one whose every ancestor is of the response's origin; more than one
/// value, one of them a keyword, none (Chromium's `ParseXFrameOptions`
/// takes two values that differ as a conflict too). `ALLOW-FROM` isn't
/// one (RFC 7034's, which Chromium doesn't take either).
fn x_frame_options(
    headers: &[(String, String)],
    origin: &Origin,
    ancestors: &[Origin],
) -> Result<(), Refused> {
    let mut values: Vec<String> = Vec::new();
    for (_, value) in headers.iter().filter(|(name, _)| name == "x-frame-options") {
        for value in value.split(',') {
            let value = value.trim_matches([' ', '\t']).to_ascii_lowercase();
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    let keyword = |value: &String| matches!(value.as_str(), "deny" | "allowall" | "sameorigin");
    match values.as_slice() {
        [] => Ok(()),
        [_, _, ..] if values.iter().any(keyword) => Err(Refused::Conflict),
        [_, _, ..] => Ok(()),
        [only] => match only.as_str() {
            "deny" => Err(Refused::Deny),
            "sameorigin" if ancestors.iter().any(|ancestor| ancestor != origin) => {
                Err(Refused::SameOrigin)
            }
            _ => Ok(()),
        },
    }
}

/// Whether the response at `url` with `headers` (names lowercase) may be
/// shown in a child frame whose ancestors, from its parent up to the top,
/// are of `ancestors`: every enforced policy's `frame-ancestors` is to
/// allow each of them (CSP3's "frame-ancestors Navigation Response
/// Check"); with none, `X-Frame-Options` decides. A report-only policy
/// doesn't count (machopo reports nothing). A main frame's response, of
/// no ancestors, is never refused.
pub fn check(url: &Url, headers: &[(String, String)], ancestors: &[Origin]) -> Result<(), Refused> {
    if ancestors.is_empty() {
        return Ok(());
    }
    let origin = url.origin();
    let mut enforced = false;
    for policy in csp::policies(headers) {
        if policy.disposition != Disposition::Enforce {
            continue;
        }
        let Some(list) = policy.frame_ancestors() else {
            continue;
        };
        enforced = true;
        if !ancestors
            .iter()
            .all(|ancestor| list.allows(ancestor, &origin))
        {
            return Err(Refused::FrameAncestors);
        }
    }
    if enforced {
        return Ok(());
    }
    x_frame_options(headers, &origin, ancestors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(url: &str) -> Origin {
        Url::parse(url).unwrap().origin()
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// The response at `https://a.example/x` with `X-Frame-Options`'
    /// `values` (one header each), in frames of `ancestors`.
    fn xfo(values: &[&str], ancestors: &[&str]) -> Result<(), Refused> {
        let pairs: Vec<(&str, &str)> = values
            .iter()
            .map(|value| ("x-frame-options", *value))
            .collect();
        let ancestors: Vec<Origin> = ancestors.iter().map(|url| origin(url)).collect();
        check(
            &Url::parse("https://a.example/x").unwrap(),
            &headers(&pairs),
            &ancestors,
        )
    }

    #[test]
    fn deny_refuses_every_frame() {
        assert_eq!(xfo(&["DENY"], &["https://a.example"]), Err(Refused::Deny));
        assert_eq!(
            xfo(&[" deny\t"], &["https://b.example"]),
            Err(Refused::Deny)
        );
        assert_eq!(
            xfo(&["deny", "Deny"], &["https://a.example"]),
            Err(Refused::Deny)
        );
    }

    /// `SAMEORIGIN` wants every ancestor of the response's origin, the
    /// top's too, not the parent's alone.
    #[test]
    fn sameorigin_wants_every_ancestor_same_origin() {
        assert_eq!(xfo(&["SAMEORIGIN"], &["https://a.example"]), Ok(()));
        assert_eq!(
            xfo(&["sameorigin"], &["https://a.example", "https://b.example"]),
            Err(Refused::SameOrigin)
        );
        assert_eq!(
            xfo(&["sameorigin"], &["https://www.a.example"]),
            Err(Refused::SameOrigin)
        );
        assert_eq!(
            xfo(&["sameorigin"], &["http://a.example"]),
            Err(Refused::SameOrigin)
        );
    }

    /// HTML's table: a keyword with any other value, in one header or
    /// two, is a conflict; values that are no keyword, or `ALLOWALL`
    /// alone, allow.
    #[test]
    fn values_are_split_and_a_keyword_with_another_conflicts() {
        let other = &["https://b.example"][..];
        assert_eq!(xfo(&["DENY, SAMEORIGIN"], other), Err(Refused::Conflict));
        assert_eq!(xfo(&["SAMEORIGIN", "DENY"], other), Err(Refused::Conflict));
        assert_eq!(xfo(&["SAMEORIGIN,"], other), Err(Refused::Conflict));
        assert_eq!(xfo(&["ALLOWALL, INVALID"], other), Err(Refused::Conflict));
        assert_eq!(
            xfo(&["SAMEORIGIN, SAMEORIGIN"], &["https://a.example"]),
            Ok(())
        );
        assert_eq!(xfo(&["INVALID, OTHER"], other), Ok(()));
        assert_eq!(xfo(&["ALLOWALL"], other), Ok(()));
        assert_eq!(xfo(&["ALLOW-FROM https://b.example"], other), Ok(()));
        assert_eq!(xfo(&[], other), Ok(()));
    }

    #[test]
    fn a_main_frame_is_never_refused() {
        assert_eq!(xfo(&["DENY"], &[]), Ok(()));
    }

    /// An enforced `frame-ancestors` decides alone, every ancestor up to
    /// the top, each enforced policy's; `X-Frame-Options` then counts for
    /// nothing; a report-only one counts for nothing itself.
    #[test]
    fn frame_ancestors_overrides_x_frame_options() {
        let url = Url::parse("https://a.example/x").unwrap();
        let check = |pairs: &[(&str, &str)], ancestors: &[&str]| {
            let ancestors: Vec<Origin> = ancestors.iter().map(|url| origin(url)).collect();
            check(&url, &headers(pairs), &ancestors)
        };
        let csp = "content-security-policy";
        assert_eq!(
            check(
                &[(csp, "frame-ancestors 'self'"), ("x-frame-options", "DENY")],
                &["https://a.example"]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                &[(csp, "frame-ancestors https://b.example")],
                &["https://b.example", "https://c.example"]
            ),
            Err(Refused::FrameAncestors)
        );
        assert_eq!(
            check(
                &[(csp, "frame-ancestors *"), (csp, "frame-ancestors 'self'")],
                &["https://b.example"]
            ),
            Err(Refused::FrameAncestors)
        );
        assert_eq!(
            check(
                &[(csp, "script-src 'none'"), ("x-frame-options", "DENY")],
                &["https://a.example"]
            ),
            Err(Refused::Deny)
        );
        assert_eq!(
            check(
                &[
                    (
                        "content-security-policy-report-only",
                        "frame-ancestors 'none'"
                    ),
                    ("x-frame-options", "DENY")
                ],
                &["https://a.example"]
            ),
            Err(Refused::Deny)
        );
        assert_eq!(
            check(
                &[(
                    "content-security-policy-report-only",
                    "frame-ancestors 'none'"
                )],
                &["https://a.example"]
            ),
            Ok(())
        );
    }
}
