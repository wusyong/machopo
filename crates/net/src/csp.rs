//! Content Security Policy as a response's headers deliver it (CSP3's
//! "parse a response's Content Security Policies", Chromium's
//! `network::ParseContentSecurityPolicies`): its policies and their
//! directives, names and values. Only `frame-ancestors`' value is read
//! here (`SourceList`), checked by the browser as a child frame's
//! navigation commits; the other directives are kept as they came, for
//! what comes to read them.

use url::{Host, Origin};

/// Whether a policy is enforced (`Content-Security-Policy`) or only
/// reported (`Content-Security-Policy-Report-Only`): a report-only one
/// blocks nothing, and machopo reports nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Enforce,
    Report,
}

/// One policy: a header value's part between commas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub disposition: Disposition,
    /// Each directive's name (lowercase) and value (trimmed), in order,
    /// the first of a name kept ("Ignoring duplicate Content-Security-Policy
    /// directive").
    pub directives: Vec<(String, String)>,
}

impl Policy {
    /// Its `frame-ancestors` directive's source list, if it has one.
    pub fn frame_ancestors(&self) -> Option<SourceList> {
        self.directives
            .iter()
            .find(|(name, _)| name == "frame-ancestors")
            .map(|(_, value)| SourceList::parse(value))
    }
}

/// The policies of `headers` (names lowercase): each
/// `content-security-policy` and `content-security-policy-report-only`
/// header's value split at its commas (headers sent more than once may be
/// joined with them, RFC 7230), each part's directives at its semicolons.
/// A part with no directive is no policy.
pub fn policies(headers: &[(String, String)]) -> Vec<Policy> {
    let mut policies = Vec::new();
    for (name, value) in headers {
        let disposition = match name.as_str() {
            "content-security-policy" => Disposition::Enforce,
            "content-security-policy-report-only" => Disposition::Report,
            _ => continue,
        };
        for part in value.split(',') {
            let mut directives: Vec<(String, String)> = Vec::new();
            for directive in part.split(';') {
                let directive = directive.trim_matches(is_whitespace);
                let (name, value) = directive
                    .split_once(is_whitespace)
                    .unwrap_or((directive, ""));
                if name.is_empty() {
                    continue;
                }
                let name = name.to_ascii_lowercase();
                if directives.iter().any(|(known, _)| *known == name) {
                    continue;
                }
                directives.push((name, value.trim_matches(is_whitespace).to_owned()));
            }
            if !directives.is_empty() {
                policies.push(Policy {
                    disposition,
                    directives,
                });
            }
        }
    }
    policies
}

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r')
}

/// A port of a host source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Port {
    Any,
    Number(u16),
}

/// A source expression that names a place (CSP's host-source and
/// scheme-source; Chromium's `CSPSource`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    /// Lowercase; empty for a host source without one (the protected
    /// resource's then).
    scheme: String,
    /// Lowercase, without the `*.` of a wildcard; empty for a scheme
    /// source, or `*` alone.
    host: String,
    /// `*.` before the host: its subdomains, not itself.
    host_wildcard: bool,
    /// None for none given: the scheme's default.
    port: Option<Port>,
    /// None for none given.
    path: Option<String>,
    /// A scheme source (`https:`): the scheme alone counts.
    scheme_only: bool,
}

/// A directive's source list, as `frame-ancestors` takes it: `'self'`,
/// `*`, and sources of places; `'none'` is none of them. Keywords
/// `frame-ancestors` doesn't take (`'unsafe-inline'`, nonces, hashes)
/// match nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceList {
    allow_self: bool,
    allow_star: bool,
    sources: Vec<Source>,
}

impl SourceList {
    fn parse(value: &str) -> Self {
        // `'none'` is no source: alone it leaves the list empty, with
        // others it counts for nothing (as Chromium ignores it then).
        let mut list = Self::default();
        for token in value.split(is_whitespace).filter(|token| !token.is_empty()) {
            if token.eq_ignore_ascii_case("'self'") {
                list.allow_self = true;
            } else if token == "*" {
                list.allow_star = true;
            } else if let Some(source) = parse_source(token) {
                list.sources.push(source);
            }
        }
        list
    }

    /// Whether `ancestor`'s origin matches, for a protected resource of
    /// `self_origin` (Chromium's `CheckCSPSourceList`): `*` any HTTP(S)
    /// one; `'self'` the resource's origin, or its `https:` upgrade; else
    /// one of the sources. An opaque one matches nothing: it has no scheme,
    /// host or port to match. (Chromium's `*` takes the resource's own
    /// scheme too, for schemes like `chrome-extension:`: `url` gives a
    /// scheme that isn't special an opaque origin, and an ancestor, a
    /// document, is of no other special one but `file:`, opaque too.)
    pub fn allows(&self, ancestor: &Origin, self_origin: &Origin) -> bool {
        let Origin::Tuple(scheme, host, port) = ancestor else {
            return false;
        };
        let self_scheme = match self_origin {
            Origin::Tuple(scheme, ..) => scheme.as_str(),
            Origin::Opaque(_) => "",
        };
        if self.allow_star && matches!(scheme.as_str(), "http" | "https") {
            return true;
        }
        let place = Place {
            scheme,
            host,
            port: *port,
        };
        if self.allow_self
            && let Origin::Tuple(self_scheme, self_host, self_port) = self_origin
        {
            let source = Source {
                scheme: self_scheme.clone(),
                host: host_text(self_host),
                host_wildcard: false,
                port: Some(Port::Number(*self_port)),
                path: None,
                scheme_only: false,
            };
            if source_allows(&source, &place, self_scheme) {
                return true;
            }
        }
        self.sources
            .iter()
            .any(|source| source_allows(source, &place, self_scheme))
    }
}

/// An ancestor's origin, as a URL of it is matched.
struct Place<'a> {
    scheme: &'a str,
    host: &'a Host,
    port: u16,
}

fn host_text(host: &Host) -> String {
    match host {
        Host::Domain(name) => name.to_ascii_lowercase(),
        other => other.to_string(),
    }
}

/// `scheme:`, `[scheme://]host[:port][path]` (CSP's grammar, as far as
/// matching an origin needs); none for a scheme or port that isn't one. A
/// host that isn't one (`'unsafe-inline'`) is kept: it is no ancestor's,
/// whose host is a valid domain or address, so it matches none.
fn parse_source(token: &str) -> Option<Source> {
    let is_scheme = |text: &str| {
        let mut chars = text.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    };
    if let Some(scheme) = token.strip_suffix(':')
        && is_scheme(scheme)
    {
        return Some(Source {
            scheme: scheme.to_ascii_lowercase(),
            host: String::new(),
            host_wildcard: false,
            port: None,
            path: None,
            scheme_only: true,
        });
    }
    let (scheme, rest) = match token.split_once("://") {
        Some((scheme, rest)) if is_scheme(scheme) => (scheme.to_ascii_lowercase(), rest),
        Some(_) => return None,
        None => (String::new(), token),
    };
    let (authority, path) = match rest.find('/') {
        Some(slash) => (rest.get(..slash)?, rest.get(slash..).map(str::to_owned)),
        None => (rest, None),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, "*")) => (host, Some(Port::Any)),
        Some((host, port)) => (host, Some(Port::Number(port.parse().ok()?))),
        None => (authority, None),
    };
    let (host, host_wildcard) = match host.strip_prefix("*.") {
        Some(host) => (host, true),
        None if host == "*" => ("", true),
        None => (host, false),
    };
    Some(Source {
        scheme,
        host: host.to_ascii_lowercase(),
        host_wildcard,
        port,
        path,
        scheme_only: false,
    })
}

/// How a source's scheme or port matches a place's (Chromium's
/// `SchemeMatchingResult`, `PortMatchingResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Matching {
    No,
    Exactly,
    /// `http` to `https`, port 80 to 443: allowed together.
    Upgrade,
    /// `:*`, or no port and the place's scheme's default: an upgrade's
    /// too.
    Wildcard,
}

fn scheme_matching(source: &str, place: &str) -> Matching {
    match (source, place) {
        (a, b) if a == b => Matching::Exactly,
        ("http", "https") | ("ws", "wss") => Matching::Upgrade,
        _ => Matching::No,
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

/// A source without a port takes the place's scheme's default (so
/// `http://b.example` matches `https://b.example`, upgraded).
fn port_matching(source: &Source, place: &Place<'_>) -> Matching {
    match source.port {
        Some(Port::Any) => Matching::Wildcard,
        None if default_port(place.scheme) == Some(place.port) => Matching::Wildcard,
        None => Matching::No,
        Some(Port::Number(port)) if port == place.port => Matching::Exactly,
        Some(Port::Number(80)) if place.port == 443 => Matching::Upgrade,
        Some(Port::Number(_)) => Matching::No,
    }
}

/// Whether `source` matches `place` for a protected resource of scheme
/// `self_scheme` (Chromium's `CheckCSPSource`): a scheme source by its
/// scheme alone; a host source by its scheme (the resource's if it has
/// none: an opaque one's, empty, matches none), host, port and path, an upgrade (`http` to `https`) of the
/// scheme only with one of the port, and the other way round.
fn source_allows(source: &Source, place: &Place<'_>, self_scheme: &str) -> bool {
    let scheme = if source.scheme.is_empty() {
        self_scheme
    } else {
        source.scheme.as_str()
    };
    let schemes = scheme_matching(scheme, place.scheme);
    if source.scheme_only || schemes == Matching::No {
        return source.scheme_only && schemes != Matching::No;
    }
    let host = host_text(place.host);
    let hosts = if source.host_wildcard {
        source.host.is_empty() || host.ends_with(&format!(".{}", source.host))
    } else {
        host == source.host
    };
    if !hosts {
        return false;
    }
    let ports = port_matching(source, place);
    // An upgraded scheme wants a port that may be upgraded, and an upgraded
    // port an upgraded scheme.
    if ports == Matching::No
        || (schemes == Matching::Upgrade
            && !matches!(ports, Matching::Upgrade | Matching::Wildcard))
        || (ports == Matching::Upgrade && schemes != Matching::Upgrade)
    {
        return false;
    }
    // An origin's path is `/`: a source with another names no origin.
    source
        .path
        .as_deref()
        .is_none_or(|path| path.is_empty() || path == "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(url: &str) -> Origin {
        url::Url::parse(url).unwrap().origin()
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// Each header's value split at commas into policies, at semicolons
    /// into directives; names any case, the first of a name kept; empty
    /// parts no policy; report-only ones apart.
    #[test]
    fn policies_and_directives_are_split() {
        let policies = policies(&headers(&[
            (
                "content-security-policy",
                "Frame-Ancestors 'self'; frame-ancestors 'none' , script-src x;;",
            ),
            (
                "content-security-policy-report-only",
                "frame-ancestors 'none'",
            ),
            ("content-security-policy", " ; , "),
            ("x-other", "frame-ancestors 'none'"),
        ]));
        assert_eq!(
            policies,
            [
                Policy {
                    disposition: Disposition::Enforce,
                    directives: vec![("frame-ancestors".into(), "'self'".into())],
                },
                Policy {
                    disposition: Disposition::Enforce,
                    directives: vec![("script-src".into(), "x".into())],
                },
                Policy {
                    disposition: Disposition::Report,
                    directives: vec![("frame-ancestors".into(), "'none'".into())],
                },
            ]
        );
    }

    fn allows(list: &str, ancestor: &str, protected: &str) -> bool {
        SourceList::parse(list).allows(&origin(ancestor), &origin(protected))
    }

    #[test]
    fn none_allows_nothing() {
        assert!(!allows("'none'", "https://a.example", "https://a.example"));
        assert!(!allows("", "https://a.example", "https://a.example"));
        assert!(!allows(
            " 'NONE' ",
            "https://a.example",
            "https://a.example"
        ));
        assert!(allows(
            "'none' 'self'",
            "https://a.example",
            "https://a.example"
        ));
    }

    /// `'self'` is the protected resource's origin, or its upgrade to
    /// `https` on the default ports; not another port, host or scheme.
    #[test]
    fn self_is_the_resources_origin() {
        let resource = "http://a.example/x";
        assert!(allows("'self'", "http://a.example", resource));
        assert!(allows("'SELF'", "https://a.example", resource));
        assert!(!allows("'self'", "http://a.example:8080", resource));
        assert!(!allows("'self'", "http://www.a.example", resource));
        assert!(!allows(
            "'self'",
            "https://a.example",
            "http://a.example:8080/x"
        ));
        assert!(!allows("'self'", "http://a.example", "https://a.example/x"));
    }

    /// `*` is any HTTP(S) origin; not an opaque one (a `file:` URL's, a
    /// scheme that isn't special's: `url` gives them opaque origins).
    #[test]
    fn star_is_any_network_origin() {
        let resource = "https://a.example/x";
        assert!(allows("*", "https://b.example", resource));
        assert!(allows("*", "http://b.example:81", resource));
        assert!(!allows("*", "file:///", resource));
        assert!(!allows("*", "ftp://b.example", resource));
        assert!(!SourceList::parse("*").allows(&Origin::new_opaque(), &origin(resource)));
    }

    /// Hosts exactly or by a wildcard of their subdomains (not the domain
    /// itself); ports the default without one, `:*` any, 80 upgraded to
    /// 443; a scheme, or the resource's with none; a path names no origin.
    #[test]
    fn host_sources_match_by_scheme_host_and_port() {
        let resource = "https://a.example/x";
        assert!(allows("https://b.example", "https://b.example", resource));
        assert!(allows("b.example", "https://b.example", resource));
        assert!(!allows("b.example", "http://b.example", resource));
        assert!(allows("http://b.example", "https://b.example", resource));
        assert!(!allows("https://b.example", "http://b.example", resource));
        assert!(allows("*.b.example", "https://www.b.example", resource));
        assert!(!allows("*.b.example", "https://b.example", resource));
        assert!(allows("https://*", "https://anything.example", resource));
        assert!(!allows(
            "https://b.example",
            "https://b.example:8443",
            resource
        ));
        assert!(allows(
            "https://b.example:8443",
            "https://b.example:8443",
            resource
        ));
        assert!(allows(
            "https://b.example:*",
            "https://b.example:8443",
            resource
        ));
        assert!(allows("http://b.example:80", "https://b.example", resource));
        assert!(!allows(
            "https://b.example:80",
            "https://b.example",
            resource
        ));
        assert!(!allows(
            "http://b.example",
            "http://b.example:443",
            resource
        ));
        assert!(!allows(
            "http://b.example:8080",
            "https://b.example:8080",
            resource
        ));
        assert!(allows("https://b.example/", "https://b.example", resource));
        assert!(!allows(
            "https://b.example/path",
            "https://b.example",
            resource
        ));
        assert!(allows("HTTPS://B.Example", "https://b.example", resource));
    }

    #[test]
    fn scheme_sources_match_by_scheme() {
        let resource = "https://a.example/x";
        assert!(allows("https:", "https://b.example", resource));
        assert!(allows("http:", "https://b.example", resource));
        assert!(!allows("https:", "http://b.example", resource));
    }

    /// What `frame-ancestors` doesn't take matches nothing, the rest as
    /// ever.
    #[test]
    fn keywords_it_doesnt_take_are_left_out() {
        assert!(!allows(
            "'unsafe-inline'",
            "https://a.example",
            "https://a.example"
        ));
        assert!(allows(
            "'unsafe-inline' https://b.example",
            "https://b.example",
            "https://a.example"
        ));
        assert!(!allows(
            "b.example:port",
            "https://b.example",
            "https://a.example"
        ));
    }
}
