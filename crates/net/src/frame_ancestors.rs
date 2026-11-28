//! Whether a response may be shown in a child frame of its ancestors
//! (Chromium's `AncestorThrottle`, which the browser runs for a child
//! frame's navigation's final response): CSP's `frame-ancestors`, then
//! `X-Frame-Options`, which an enforced `frame-ancestors` overrides; and
//! what the throttle tells the consoles of it, word for word.

use url::{Origin, Url};

use crate::csp::{self, Disposition, SourceList};

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
    /// `X-Frame-Options` with values that conflict, taken as `DENY`.
    Conflict,
}

/// What the check found (Chromium's `AncestorThrottle::ProcessResponseImpl`
/// and the console messages it leaves).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// Why the response is refused, if it is.
    pub refused: Option<Refused>,
    /// For the console of the frame, once it commits (its error page, or
    /// its document): `X-Frame-Options`' (Chromium's
    /// `AddDeferredConsoleMessage`).
    pub deferred: Vec<String>,
    /// `frame-ancestors`' violations: each one's message, for the console
    /// of the ancestor (its index in the ancestors) that isn't allowed
    /// (Chromium reports them through that ancestor's CSP context).
    pub violations: Vec<(usize, String)>,
}

/// What `X-Frame-Options` says (Chromium's `XFrameOptionsValue`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameOptions {
    None,
    Deny,
    SameOrigin,
    AllowAll,
    /// No keyword.
    Invalid,
    /// Values that differ: `DENY`.
    Conflict,
}

/// HTML's "get, decode, and split" of the response's `X-Frame-Options`,
/// each value as Chromium's `ParseXFrameOptions` takes it: `DENY`,
/// `SAMEORIGIN`, `ALLOWALL` (any case), or else invalid; two values that
/// differ so are a conflict (HTML's "more than one value, one a keyword":
/// two invalid ones are no conflict in either). `ALLOW-FROM` is invalid
/// (RFC 7034's, which Chromium doesn't take either).
fn frame_options(headers: &[(String, String)]) -> FrameOptions {
    let mut options = FrameOptions::None;
    for (_, value) in headers.iter().filter(|(name, _)| name == "x-frame-options") {
        for value in value.split(',') {
            let this = match value
                .trim_matches([' ', '\t'])
                .to_ascii_lowercase()
                .as_str()
            {
                "deny" => FrameOptions::Deny,
                "sameorigin" => FrameOptions::SameOrigin,
                "allowall" => FrameOptions::AllowAll,
                _ => FrameOptions::Invalid,
            };
            if options == FrameOptions::None {
                options = this;
            } else if options != this {
                return FrameOptions::Conflict;
            }
        }
    }
    options
}

/// The URL of `origin`, as Chromium's messages write a response's
/// (`url::Origin::GetURL`): its serialization and `/`.
fn origin_url(origin: &Origin) -> String {
    match origin {
        Origin::Tuple(..) => format!("{}/", origin.ascii_serialization()),
        Origin::Opaque(_) => String::new(),
    }
}

/// The message of a `frame-ancestors` violation of the response of
/// `origin` (Chromium's `ReportViolation`): the response's origin, the
/// directive as parsed, what came of it, a note for a `*` that didn't
/// match, and a newline.
fn violation(origin: &Origin, disposition: Disposition, list: &SourceList) -> String {
    let (report_only, outcome) = match disposition {
        Disposition::Enforce => ("", "The request has been blocked."),
        Disposition::Report => (
            "report-only ",
            "The violation has been logged, but no further action has been taken.",
        ),
    };
    let mut text = format!(
        "Framing '{}' violates the following {report_only}Content Security Policy directive: \"frame-ancestors {list}\". {outcome}",
        origin_url(origin)
    );
    if list.allows_star() {
        let scheme = match origin {
            Origin::Tuple(scheme, ..) => scheme.as_str(),
            Origin::Opaque(_) => "",
        };
        text.push_str(&format!(
            " Note that '*' matches only URLs with network schemes ('http', 'https', 'ws', 'wss'), or URLs whose scheme matches `self`'s scheme. The scheme '{scheme}:' must be added explicitly."
        ));
    }
    text.push('\n');
    text
}

/// Whether the response at `url` with `headers` (names lowercase) may be
/// shown in a child frame whose ancestors, from its parent up to the top,
/// are of `ancestors`, and what the consoles are told of it. Each
/// ancestor, from the parent up, is checked against every policy's
/// `frame-ancestors` (CSP3's "frame-ancestors Navigation Response Check"),
/// each violation told to it: the first an enforced one doesn't allow
/// refuses the response (those above aren't checked); a report-only one
/// refuses nothing. With an enforced `frame-ancestors`, `X-Frame-Options`
/// counts for nothing, its messages too; else it decides. A main frame's
/// response, of no ancestors, is never refused.
pub fn check(url: &Url, headers: &[(String, String)], ancestors: &[Origin]) -> Checked {
    let mut checked = Checked::default();
    if ancestors.is_empty() {
        return checked;
    }
    let origin = url.origin();
    let policies: Vec<(Disposition, SourceList)> = csp::policies(headers)
        .iter()
        .filter_map(|policy| Some((policy.disposition, policy.frame_ancestors()?)))
        .collect();
    for (at, ancestor) in ancestors.iter().enumerate() {
        let mut blocked = false;
        for (disposition, list) in &policies {
            if !list.allows(ancestor, &origin) {
                checked
                    .violations
                    .push((at, violation(&origin, *disposition, list)));
                blocked |= *disposition == Disposition::Enforce;
            }
        }
        if blocked {
            checked.refused = Some(Refused::FrameAncestors);
            return checked;
        }
    }
    // Chromium skips it only when it would say something (its metrics
    // count the rest): the same.
    if policies
        .iter()
        .any(|(disposition, _)| *disposition == Disposition::Enforce)
    {
        return checked;
    }
    let options = frame_options(headers);
    let target = origin_url(&origin);
    // The header's values as Chromium's `GetNormalizedHeader` joins them.
    let value = || {
        headers
            .iter()
            .filter(|(name, _)| name == "x-frame-options")
            .map(|(_, value)| value.trim_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let refused = |keyword: &str| {
        format!(
            "Refused to display '{target}' in a frame because it set 'X-Frame-Options' to '{keyword}'."
        )
    };
    match options {
        FrameOptions::Conflict => {
            checked.refused = Some(Refused::Conflict);
            checked.deferred.push(format!(
                "Refused to display '{target}' in a frame because it set multiple 'X-Frame-Options' headers with conflicting values ('{}'). Falling back to 'deny'.",
                value()
            ));
        }
        FrameOptions::Invalid => checked.deferred.push(format!(
            "Invalid 'X-Frame-Options' header encountered when loading '{target}': '{}' is not a recognized directive. The header will be ignored.",
            value()
        )),
        FrameOptions::Deny => {
            checked.refused = Some(Refused::Deny);
            checked.deferred.push(refused("deny"));
        }
        FrameOptions::SameOrigin if ancestors.iter().any(|ancestor| *ancestor != origin) => {
            checked.refused = Some(Refused::SameOrigin);
            checked.deferred.push(refused("sameorigin"));
        }
        FrameOptions::SameOrigin | FrameOptions::None | FrameOptions::AllowAll => {}
    }
    checked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `check`'s refusal, as a result.
    fn refusal(
        url: &Url,
        headers: &[(String, String)],
        ancestors: &[Origin],
    ) -> Result<(), Refused> {
        check(url, headers, ancestors).refused.map_or(Ok(()), Err)
    }

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
        refusal(
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
            refusal(&url, &headers(pairs), &ancestors)
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

    /// The response at `https://a.example/x` with `pairs` in frames of
    /// `ancestors`, checked.
    fn checked(pairs: &[(&str, &str)], ancestors: &[&str]) -> Checked {
        let ancestors: Vec<Origin> = ancestors.iter().map(|url| origin(url)).collect();
        check(
            &Url::parse("https://a.example/x").unwrap(),
            &headers(pairs),
            &ancestors,
        )
    }

    /// `X-Frame-Options`' messages, for the frame once it commits, word
    /// for word as Chrome's: the response's origin; a conflict's and an
    /// invalid one's header as it came, headers joined with `, `; an
    /// invalid one refuses nothing; none when it allows.
    #[test]
    fn x_frame_options_leaves_chromes_messages() {
        let xfo = "x-frame-options";
        let other = &["https://b.example"][..];
        let deferred = |pairs: &[(&str, &str)], ancestors: &[&str]| {
            let checked = checked(pairs, ancestors);
            assert!(checked.violations.is_empty());
            (checked.refused, checked.deferred)
        };
        assert_eq!(
            deferred(&[(xfo, "DENY")], other),
            (
                Some(Refused::Deny),
                vec!["Refused to display 'https://a.example/' in a frame because it set 'X-Frame-Options' to 'deny'.".to_owned()]
            )
        );
        assert_eq!(
            deferred(&[(xfo, "SameOrigin")], other),
            (
                Some(Refused::SameOrigin),
                vec!["Refused to display 'https://a.example/' in a frame because it set 'X-Frame-Options' to 'sameorigin'.".to_owned()]
            )
        );
        assert_eq!(
            deferred(&[(xfo, "SAMEORIGIN")], &["https://a.example"]),
            (None, vec![])
        );
        assert_eq!(
            deferred(&[(xfo, "SAMEORIGIN"), (xfo, " DENY")], other),
            (
                Some(Refused::Conflict),
                vec!["Refused to display 'https://a.example/' in a frame because it set multiple 'X-Frame-Options' headers with conflicting values ('SAMEORIGIN, DENY'). Falling back to 'deny'.".to_owned()]
            )
        );
        assert_eq!(
            deferred(&[(xfo, "INVALID, OTHER")], other),
            (
                None,
                vec!["Invalid 'X-Frame-Options' header encountered when loading 'https://a.example/': 'INVALID, OTHER' is not a recognized directive. The header will be ignored.".to_owned()]
            )
        );
        assert_eq!(deferred(&[(xfo, "ALLOWALL")], other), (None, vec![]));
        assert_eq!(deferred(&[], other), (None, vec![]));
    }

    /// A `frame-ancestors` violation's message is the ancestor's that
    /// isn't allowed, one per policy, the directive as parsed: the first
    /// an enforced policy doesn't allow ends it; a report-only one's are
    /// every ancestor's it doesn't allow, and refuse nothing.
    #[test]
    fn frame_ancestors_violations_are_the_ancestors_not_allowed() {
        let csp = "content-security-policy";
        let blocked = |list: &str| {
            format!(
                "Framing 'https://a.example/' violates the following Content Security Policy directive: \"frame-ancestors {list}\". The request has been blocked.\n"
            )
        };
        let checked_ = checked(
            &[(csp, "frame-ancestors 'none'")],
            &["https://a.example", "https://b.example"],
        );
        assert_eq!(checked_.refused, Some(Refused::FrameAncestors));
        assert_eq!(checked_.violations, [(0, blocked("'none'"))]);
        assert!(checked_.deferred.is_empty());
        let checked_ = checked(
            &[(csp, "frame-ancestors 'self' HTTPS://C.example")],
            &["https://a.example", "https://b.example"],
        );
        assert_eq!(
            checked_.violations,
            [(1, blocked("'self' https://c.example"))]
        );
        let checked_ = checked(
            &[
                (csp, "frame-ancestors 'none'"),
                (csp, "frame-ancestors https://c.example"),
            ],
            &["https://b.example"],
        );
        assert_eq!(
            checked_.violations,
            [(0, blocked("'none'")), (0, blocked("https://c.example"))]
        );
        let report = "Framing 'https://a.example/' violates the following report-only Content Security Policy directive: \"frame-ancestors 'self'\". The violation has been logged, but no further action has been taken.\n";
        let checked_ = checked(
            &[(
                "content-security-policy-report-only",
                "frame-ancestors 'self'",
            )],
            &[
                "https://b.example",
                "https://a.example",
                "https://c.example",
            ],
        );
        assert_eq!(checked_.refused, None);
        assert_eq!(
            checked_.violations,
            [(0, report.to_owned()), (2, report.to_owned())]
        );
    }

    /// A `*` that didn't match has Chromium's note, of the response's
    /// scheme.
    #[test]
    fn a_star_that_didnt_match_has_a_note() {
        let checked = check(
            &Url::parse("https://a.example/x").unwrap(),
            &headers(&[("content-security-policy", "frame-ancestors * 'self'")]),
            &[Origin::new_opaque()],
        );
        assert_eq!(
            checked.violations,
            [(0, "Framing 'https://a.example/' violates the following Content Security Policy directive: \"frame-ancestors *\". The request has been blocked. Note that '*' matches only URLs with network schemes ('http', 'https', 'ws', 'wss'), or URLs whose scheme matches `self`'s scheme. The scheme 'https:' must be added explicitly.\n".to_owned())]
        );
    }

    /// An enforced `frame-ancestors` that allows leaves `X-Frame-Options`
    /// no message; a report-only one leaves it its own.
    #[test]
    fn x_frame_options_overridden_leaves_no_message() {
        let checked_ = checked(
            &[
                ("content-security-policy", "frame-ancestors *"),
                ("x-frame-options", "bogus"),
            ],
            &["https://b.example"],
        );
        assert_eq!(checked_, Checked::default());
        let checked_ = checked(
            &[
                (
                    "content-security-policy-report-only",
                    "frame-ancestors 'none'",
                ),
                ("x-frame-options", "DENY"),
            ],
            &["https://a.example"],
        );
        assert_eq!(checked_.refused, Some(Refused::Deny));
        assert_eq!(checked_.violations.len(), 1);
        assert_eq!(checked_.deferred.len(), 1);
    }
}
