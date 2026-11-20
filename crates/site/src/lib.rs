//! What a document's data is kept by, for the browser, the network service
//! and the renderer alike (the part of Chromium's `//net/base` both sides
//! link): a host's registrable domain, by the public suffix list, and an
//! origin's [`Site`] (HTML's "obtain a site", `net::SchemefulSite`).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

use url::{Host, Origin, Url};

/// `host`'s registrable domain (its public suffix and one label more, its
/// "eTLD+1"), if it has one: none for an IP address, a public suffix itself
/// (`co.uk`, `github.io`), a single label (`localhost`) or a name with an
/// empty label in it. The list's private part counts (`alice.github.io` is
/// its own), and so does a top-level domain the list doesn't know, as a
/// suffix (`b.test` of `a.b.test`): Chromium's `GetDomainAndRegistry` with
/// `INCLUDE_PRIVATE_REGISTRIES`. A name's last dot stays.
///
/// `host` is as a URL has it: lowercase ASCII, an internationalized name
/// in punycode.
pub fn registrable_domain<'a>(host: &Host<&'a str>) -> Option<&'a str> {
    let Host::Domain(name) = *host else {
        return None;
    };
    // The list's lookup takes labels as they come: an empty one (but after
    // the last dot) isn't a name's.
    let labels = name.strip_suffix('.').unwrap_or(name);
    if labels.split('.').any(str::is_empty) {
        return None;
    }
    let length = psl::domain(name.as_bytes())?.as_bytes().len();
    // The domain is the end of the name.
    name.get(name.len().checked_sub(length)?..)
}

/// A site (HTML's "site", Chromium's `net::SchemefulSite`): what "same
/// site" compares. An opaque origin's is that origin's alone; a tuple
/// origin's is its scheme and its host's registrable domain, or its host
/// if it has none (an IP address, `localhost`), whatever its port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Site(Repr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Repr {
    /// The opaque origin it is of: the same site as itself only.
    Opaque(url::OpaqueOrigin),
    /// The host as a URL writes it (an IPv6 address in brackets).
    Tuple { scheme: Box<str>, host: Box<str> },
}

impl Site {
    /// `origin`'s site (HTML's "obtain a site").
    pub fn of(origin: &Origin) -> Self {
        match origin {
            Origin::Opaque(opaque) => Self(Repr::Opaque(opaque.clone())),
            Origin::Tuple(scheme, host, _) => {
                let host = match host {
                    Host::Domain(name) => registrable_domain(&Host::Domain(name.as_str()))
                        .unwrap_or(name)
                        .into(),
                    address => address.to_string().into_boxed_str(),
                };
                Self(Repr::Tuple {
                    scheme: scheme.as_str().into(),
                    host,
                })
            }
        }
    }

    /// The site of `url`'s origin.
    pub fn of_url(url: &Url) -> Self {
        Self::of(&url.origin())
    }

    /// Whether it is an opaque origin's: the same site as nothing else.
    pub fn is_opaque(&self) -> bool {
        matches!(self.0, Repr::Opaque(_))
    }

    /// Its scheme, if it isn't opaque.
    pub fn scheme(&self) -> Option<&str> {
        match &self.0 {
            Repr::Opaque(_) => None,
            Repr::Tuple { scheme, .. } => Some(scheme),
        }
    }

    /// Its registrable domain, or its host if that has none; none for an
    /// opaque one (`SchemefulSite::registrable_domain_or_host`).
    pub fn registrable_domain_or_host(&self) -> Option<&str> {
        match &self.0 {
            Repr::Opaque(_) => None,
            Repr::Tuple { host, .. } => Some(host),
        }
    }

    /// As it is written: `https://example.com`, or `null` for an opaque
    /// one (`SchemefulSite::Serialize`), which tells no two apart.
    pub fn serialize(&self) -> String {
        match &self.0 {
            Repr::Opaque(_) => "null".to_owned(),
            Repr::Tuple { scheme, host } => format!("{scheme}://{host}"),
        }
    }
}
