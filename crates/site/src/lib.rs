//! What a document's data is kept by, for the browser, the network service
//! and the renderer alike (the part of Chromium's `//net/base` and
//! `blink/public/common` both sides link): a host's registrable domain, by
//! the public suffix list, an origin's [`Site`] (HTML's "obtain a site",
//! `net::SchemefulSite`), and the [`StorageKey`] a frame's storage is kept
//! by (`blink::StorageKey`).

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

/// Whether a frame and every frame it is in are of its top-level site
/// (Chromium's `blink::mojom::AncestorChainBit`): `a.example` in
/// `b.example` in `a.example` is of the top-level site, but `b.example`
/// can control it, so it is a third party's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AncestorChainBit {
    SameSite,
    CrossSite,
}

/// What a frame's storage is kept by (the Storage Standard's storage key,
/// Chromium's `blink::StorageKey` with third-party storage partitioning,
/// its default): its document's origin, the site of the top-level frame
/// it is in, and whether a frame of another site is between them. A
/// frame of another site than the top-level one's has storage of its own
/// for each top-level site it is in. No nonce: machopo has no
/// credentialless iframes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey {
    origin: Origin,
    top_level_site: Site,
    ancestor_chain_bit: AncestorChainBit,
}

impl StorageKey {
    /// The key of a frame whose document is of `origin`, in frames of
    /// `ancestors`' origins, its parent's first, the top-level frame's
    /// last (none: it is the top-level frame), as Chromium's
    /// `RenderFrameHostImpl::CalculateStorageKey`. The ancestor chain bit
    /// is `SameSite` if the frame and each one it is in are of the
    /// top-level site; if its origin or the top-level site is opaque, it
    /// is `CrossSite`.
    pub fn of_frame<'a>(origin: &Origin, ancestors: impl IntoIterator<Item = &'a Origin>) -> Self {
        let ancestors: Vec<&Origin> = ancestors.into_iter().collect();
        let top = ancestors.last().copied().unwrap_or(origin);
        let top_level_site = Site::of(top);
        // The frame and each one it is in. An opaque site is no other
        // origin's: with an opaque origin or top-level site, only an opaque
        // top-level frame itself is of it, which is cross-site all the same.
        let all_of_the_top_level_site = std::iter::once(origin)
            .chain(ancestors.iter().copied())
            .all(|frame| Site::of(frame) == top_level_site);
        let ancestor_chain_bit = if origin.is_tuple() && all_of_the_top_level_site {
            AncestorChainBit::SameSite
        } else {
            AncestorChainBit::CrossSite
        };
        Self {
            origin: origin.clone(),
            top_level_site,
            ancestor_chain_bit,
        }
    }

    /// The key of a top-level frame whose document is of `origin`
    /// (`StorageKey::CreateFirstParty`).
    pub fn first_party(origin: &Origin) -> Self {
        Self::of_frame(origin, [])
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn top_level_site(&self) -> &Site {
        &self.top_level_site
    }

    pub fn ancestor_chain_bit(&self) -> AncestorChainBit {
        self.ancestor_chain_bit
    }

    /// Whether it is a third party's (`StorageKey::IsThirdPartyContext`):
    /// its origin isn't of the top-level site, or a frame of another site
    /// is between them, or either is opaque.
    pub fn is_third_party(&self) -> bool {
        self.ancestor_chain_bit == AncestorChainBit::CrossSite
    }

    /// As storage on disk keeps it, in Chromium's `StorageKey::Serialize`
    /// form: the origin's URL (`https://a.example/`); a third party's with
    /// `^0` and the top-level site if that is another site
    /// (`https://chat.example/^0https://a.example`), else with `^31` (a
    /// frame of another site between them). None if the origin or the
    /// top-level site is opaque: no other document is of it, and its
    /// storage goes with it.
    pub fn serialize(&self) -> Option<String> {
        if !self.origin.is_tuple() || self.top_level_site.is_opaque() {
            return None;
        }
        let origin = format!("{}/", self.origin.ascii_serialization());
        Some(if self.ancestor_chain_bit == AncestorChainBit::SameSite {
            origin
        } else if Site::of(&self.origin) == self.top_level_site {
            format!("{origin}^31")
        } else {
            format!("{origin}^0{}", self.top_level_site.serialize())
        })
    }
}

/// The site a request is first party to, for its cookies' `SameSite`
/// (Chromium's `net::SiteForCookies`): a frame's is the top-level site if
/// the frame and every frame it is in are of it, else null (a third
/// party's, to no site first party).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SiteForCookies(Option<Site>);

impl SiteForCookies {
    /// A frame's whose document is of `origin`, in frames of `ancestors`'
    /// origins, its parent's first, the top-level frame's last (Chromium's
    /// `RenderFrameHostImpl::ComputeSiteForCookies`): the top-level site,
    /// null if it is opaque or the frame or one between isn't of it.
    pub fn of_frame<'a>(origin: &Origin, ancestors: impl IntoIterator<Item = &'a Origin>) -> Self {
        let ancestors: Vec<&Origin> = ancestors.into_iter().collect();
        let top = ancestors.last().copied().unwrap_or(origin);
        let site = Site::of(top);
        let first_party = !site.is_opaque()
            && std::iter::once(origin)
                .chain(ancestors.iter().copied())
                .all(|frame| Site::of(frame) == site);
        Self(first_party.then_some(site))
    }

    /// Of `site`: a top-level navigation's to it, say
    /// (`SiteForCookies::FromUrl`).
    pub fn of_site(site: Site) -> Self {
        Self((!site.is_opaque()).then_some(site))
    }

    /// The null one: first party to nothing.
    pub fn null() -> Self {
        Self(None)
    }

    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }

    pub fn site(&self) -> Option<&Site> {
        self.0.as_ref()
    }

    /// Whether `url` is first party to it, its scheme as well
    /// (`IsSchemefullyFirstParty`): of its site, `ws:` as `http:` and
    /// `wss:` as `https:`.
    pub fn is_first_party(&self, url: &Url) -> bool {
        let Some(site) = &self.0 else {
            return false;
        };
        let mut url = url.clone();
        let http = match url.scheme() {
            "ws" => Some("http"),
            "wss" => Some("https"),
            _ => None,
        };
        if let Some(http) = http {
            // Always a scheme a URL with a host takes.
            let _ = url.set_scheme(http);
        }
        Site::of_url(&url) == *site
    }
}
