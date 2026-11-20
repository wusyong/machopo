//! The partition a `Partitioned` cookie (CHIPS) is kept in (Chromium's
//! `net::CookiePartitionKey`): the site of the top-level frame of the
//! request that set it, and whether a frame of another site was between
//! (its ancestor chain bit). A partitioned cookie goes only with requests
//! of its partition, so a third party's is a cookie of that top-level site
//! alone, not one that follows the user across sites.

use site::{AncestorChainBit, Site, SiteForCookies};
use url::Url;

/// A cookie partition (`net::CookiePartitionKey`, without a nonce: machopo
/// has no credentialless iframes or fenced frames).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CookiePartitionKey {
    site: Site,
    ancestor_chain_bit: AncestorChainBit,
}

impl CookiePartitionKey {
    pub fn new(site: Site, ancestor_chain_bit: AncestorChainBit) -> Self {
        Self {
            site,
            ancestor_chain_bit,
        }
    }

    /// The partition of a request to `url` whose top-level frame is of
    /// `top_frame_site`, first party to `site_for_cookies`
    /// (`CookiePartitionKey::FromNetworkIsolationKey`): none without a
    /// top-level site; cross-site unless `url` is first party to the site
    /// for cookies, whatever its scheme (`SiteForCookies::IsFirstParty`).
    /// A main frame's navigation is first party to its URL's site, so of
    /// the same site, as Chromium makes it.
    pub fn for_request(
        top_frame_site: Option<&Site>,
        site_for_cookies: &SiteForCookies,
        url: &Url,
    ) -> Option<Self> {
        let site = top_frame_site?.clone();
        let ancestor_chain_bit = if site_for_cookies.is_first_party_schemeless(url) {
            AncestorChainBit::SameSite
        } else {
            AncestorChainBit::CrossSite
        };
        Some(Self::new(site, ancestor_chain_bit))
    }

    pub fn site(&self) -> &Site {
        &self.site
    }

    /// Whether a frame of another site than the top-level one's was
    /// between it and the request (`IsThirdParty`).
    pub fn has_cross_site_ancestor(&self) -> bool {
        self.ancestor_chain_bit == AncestorChainBit::CrossSite
    }

    /// As the cookie database keeps it (`CookiePartitionKey::Serialize`):
    /// its site as written and its ancestor chain bit; none for an opaque
    /// site's, which no later machopo can be in again.
    pub fn serialize(&self) -> Option<(String, bool)> {
        (!self.site.is_opaque()).then(|| (self.site.serialize(), self.has_cross_site_ancestor()))
    }

    /// The one [`serialize`](Self::serialize) wrote as `site` and
    /// `has_cross_site_ancestor` (`CookiePartitionKey::FromStorage`); none
    /// if `site` isn't a site as it writes one.
    pub fn from_storage(site: &str, has_cross_site_ancestor: bool) -> Option<Self> {
        let parsed = Site::of_url(&Url::parse(site).ok()?);
        if parsed.is_opaque() || parsed.serialize() != site {
            return None;
        }
        Some(Self::new(
            parsed,
            if has_cross_site_ancestor {
                AncestorChainBit::CrossSite
            } else {
                AncestorChainBit::SameSite
            },
        ))
    }
}
