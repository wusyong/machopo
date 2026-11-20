//! The cookie commands of CDP the browser answers (the cookie parts of
//! Chromium's `NetworkHandler` and `StorageHandler`), through the profile's
//! network context (its `CookieManager`): `Network.getCookies`,
//! `getAllCookies`, `setCookie`, `setCookies`, `deleteCookies`,
//! `clearBrowserCookies`, and `Storage.getCookies`, `setCookies`,
//! `clearCookies`. Each is answered once the network service has done it.

use browser::WebContents;
use net::cookies::{
    CanonicalCookie, CookieDeletionFilter, CookiePartitionKey, CookieSameSite, CookieSourceScheme,
    PartitionFilter, Time,
};
use protocol::automation::{Error, INVALID_PARAMS};
use serde_json::{Value, json};
use url::Url;

use crate::automation_session::Reply;

/// Whether `name` is one of the cookie commands.
pub(crate) fn is_cookie_command(name: &str) -> bool {
    matches!(
        name,
        "Network.getCookies"
            | "Network.getAllCookies"
            | "Network.setCookie"
            | "Network.setCookies"
            | "Network.deleteCookies"
            | "Network.clearBrowserCookies"
            | "Storage.getCookies"
            | "Storage.setCookies"
            | "Storage.clearCookies"
    )
}

/// Runs the cookie command `name` with `params` on the profile of
/// `contents` (the session's tab, or the active one), answering `reply`:
/// at once if its params are wrong, else once the network service has done
/// it.
pub(crate) fn run(name: &str, params: &Value, contents: &WebContents, reply: Reply) {
    let network = contents.network();
    match name {
        "Network.getCookies" => {
            // The tab's frames' URLs unless it names some
            // (`ComputeCookieURLs`), of the tab's partition.
            let urls: Vec<Url> = match params.get("urls").and_then(Value::as_array) {
                Some(urls) => urls
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|url| Url::parse(url).ok())
                    .collect(),
                None => contents
                    .frame_urls()
                    .iter()
                    .filter_map(|url| Url::parse(url).ok())
                    .collect(),
            };
            let top_frame = contents.url().and_then(|url| Url::parse(url).ok());
            network.get_cookie_list(urls, top_frame, move |cookies| {
                reply.answer(Ok(json!({ "cookies": cookies_json(&cookies) })));
            });
        }
        "Network.getAllCookies" | "Storage.getCookies" => {
            network.get_all_cookies(move |cookies| {
                reply.answer(Ok(json!({ "cookies": cookies_json(&cookies) })));
            });
        }
        "Network.setCookie" => match cookie_from_params(params) {
            Ok(cookie) => network.set_canonical_cookies(vec![cookie], move |set| {
                reply.answer(Ok(json!({ "success": set.first() == Some(&true) })));
            }),
            Err(error) => reply.answer(Err(error)),
        },
        "Network.setCookies" | "Storage.setCookies" => {
            // All of them, or none (`SetCookies`).
            let cookies = params
                .get("cookies")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("cookies: array expected"))
                .and_then(|cookies| cookies.iter().map(cookie_from_params).collect());
            match cookies {
                Ok(cookies) => network.set_canonical_cookies(cookies, move |_| reply.done()),
                Err(_) => reply.answer(Err(invalid("Invalid cookie fields"))),
            }
        }
        "Network.deleteCookies" => match deletion_filter(params) {
            Ok(filter) => network.delete_cookies(filter, move |_| reply.done()),
            Err(error) => reply.answer(Err(error)),
        },
        "Network.clearBrowserCookies" | "Storage.clearCookies" => {
            network.delete_cookies(CookieDeletionFilter::all(), move |_| reply.done());
        }
        _ => reply.answer(Err(Error::new(
            protocol::automation::METHOD_NOT_FOUND,
            format!("'{name}' wasn't found"),
        ))),
    }
}

fn invalid(message: &str) -> Error {
    Error::new(INVALID_PARAMS, message.to_owned())
}

/// The cookies as CDP's `Network.Cookie`s.
fn cookies_json(cookies: &[CanonicalCookie]) -> Vec<Value> {
    cookies.iter().map(cookie_json).collect()
}

/// `cookie` as CDP's `Network.Cookie` (`BuildCookie`): `expires` in
/// seconds since the epoch, -1 for a session cookie; `sameSite` only if it
/// has one; every cookie of medium priority.
pub(crate) fn cookie_json(cookie: &CanonicalCookie) -> Value {
    let mut json = json!({
        "name": cookie.name(),
        "value": cookie.value(),
        "domain": cookie.domain(),
        "path": cookie.path(),
        "expires": cookie.expiry_date().map_or(-1.0, seconds_since_epoch),
        "size": cookie.name_value_size_bytes(),
        "httpOnly": cookie.is_http_only(),
        "secure": cookie.is_secure(),
        "session": !cookie.is_persistent(),
        "priority": "Medium",
        "sourceScheme": match cookie.source_scheme() {
            CookieSourceScheme::NonSecure => "NonSecure",
            CookieSourceScheme::Secure => "Secure",
        },
        "sourcePort": cookie.source_port().map_or(-1, i32::from),
    });
    let same_site = match cookie.same_site() {
        CookieSameSite::Unspecified => None,
        CookieSameSite::NoRestriction => Some("None"),
        CookieSameSite::LaxMode => Some("Lax"),
        CookieSameSite::StrictMode => Some("Strict"),
    };
    if let Some(same_site) = same_site {
        json["sameSite"] = json!(same_site);
    }
    if let Some((site, has_cross_site_ancestor)) = cookie
        .partition_key()
        .and_then(CookiePartitionKey::serialize)
    {
        json["partitionKey"] = json!({
            "topLevelSite": site,
            "hasCrossSiteAncestor": has_cross_site_ancestor,
        });
    }
    json
}

/// `time` in seconds since the epoch, as a double
/// (`InSecondsFSinceUnixEpoch`).
fn seconds_since_epoch(time: Time) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "microseconds as a double's seconds, as Chromium's InSecondsF"
    )]
    let micros = time.unix_micros() as f64;
    micros / 1_000_000.0
}

/// A `Network.CookieParam` (`setCookie`'s params too) as a cookie
/// (`MakeCookieFromProtocolValues`): a URL (`http:` or `https:`) or a
/// domain at least. Without `domain`, a host-only cookie of the URL's
/// host; a `domain` with a leading dot is a domain cookie of it, one
/// without a host-only cookie of it. Set as from `https:` if `secure`
/// (or an `https:` URL), else `http:`, of that host, at `path` (or `/`);
/// `expires` in seconds since the epoch, none or negative for a session
/// cookie.
fn cookie_from_params(params: &Value) -> Result<CanonicalCookie, Error> {
    let string = |name: &str| params.get(name).and_then(Value::as_str).unwrap_or("");
    let flag = |name: &str| params.get(name).and_then(Value::as_bool).unwrap_or(false);
    let (Some(name), Some(value)) = (
        params.get("name").and_then(Value::as_str),
        params.get("value").and_then(Value::as_str),
    ) else {
        return Err(invalid("name and value: string expected"));
    };
    let (url_spec, domain) = (string("url"), string("domain"));
    if url_spec.is_empty() && domain.is_empty() {
        return Err(invalid(
            "At least one of the url or domain needs to be specified",
        ));
    }
    let mut secure = flag("secure");
    let mut normalized_domain = domain.to_owned();
    if !url_spec.is_empty() {
        let url = Url::parse(url_spec)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .ok_or_else(|| invalid("URL must have scheme http or https"))?;
        secure = secure || url.scheme() == "https";
        if normalized_domain.is_empty() {
            normalized_domain = url.host_str().unwrap_or("").to_owned();
        }
    }
    // A domain without a leading dot is the URL's host alone: a host-only
    // cookie.
    let url_host = match normalized_domain.strip_prefix('.') {
        Some(host) => host.to_owned(),
        None => std::mem::take(&mut normalized_domain),
    };
    let url = Url::parse(&format!(
        "{}://{url_host}",
        if secure { "https" } else { "http" }
    ))
    .map_err(|_| invalid("Sanitizing cookie failed"))?;
    let expires = params
        .get("expires")
        .and_then(Value::as_f64)
        .unwrap_or(-1.0);
    let expiry = (expires >= 0.0)
        .then(|| Time::from_unix_micros(base::numerics::saturated_cast(expires * 1_000_000.0)));
    let same_site = match string("sameSite") {
        "Lax" => CookieSameSite::LaxMode,
        "Strict" => CookieSameSite::StrictMode,
        "None" => CookieSameSite::NoRestriction,
        _ => CookieSameSite::Unspecified,
    };
    let partition_key = match params.get("partitionKey") {
        Some(key)
            if !key
                .get("topLevelSite")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty() =>
        {
            let site = key
                .get("topLevelSite")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cross = key
                .get("hasCrossSiteAncestor")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(
                CookiePartitionKey::from_storage(site, cross)
                    .ok_or_else(|| invalid("Deserializing cookie partition key failed"))?,
            )
        }
        _ => None,
    };
    CanonicalCookie::create_sanitized(
        &url,
        name,
        value,
        &normalized_domain,
        string("path"),
        Time::now(),
        expiry,
        secure,
        flag("httpOnly"),
        same_site,
        partition_key.as_ref(),
    )
    .map_err(|_| invalid("Sanitizing cookie failed"))
}

/// `deleteCookies`' params as a filter (`DeleteCookies`): the cookies of
/// `name`, of exactly `domain` (or the URL's host: host-only ones), of
/// `path` if given, and of `partitionKey`'s partition if given, else
/// unpartitioned ones.
fn deletion_filter(params: &Value) -> Result<CookieDeletionFilter, Error> {
    let string = |name: &str| params.get(name).and_then(Value::as_str);
    let name = string("name").ok_or_else(|| invalid("name: string expected"))?;
    let domain = match (
        string("domain").filter(|domain| !domain.is_empty()),
        string("url"),
    ) {
        (Some(domain), _) => domain.to_owned(),
        (None, Some(url)) => Url::parse(url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .and_then(|url| url.host_str().map(str::to_owned))
            .ok_or_else(|| invalid("An http or https url URL must be specified"))?,
        (None, None) => {
            return Err(invalid(
                "At least one of the url and domain needs to be specified",
            ));
        }
    };
    let partition = match params.get("partitionKey") {
        Some(key) => {
            let site = key
                .get("topLevelSite")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cross = key
                .get("hasCrossSiteAncestor")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            PartitionFilter::Of(
                CookiePartitionKey::from_storage(site, cross)
                    .ok_or_else(|| invalid("Deserializing cookie partition key failed"))?,
            )
        }
        None => PartitionFilter::Unpartitioned,
    };
    Ok(CookieDeletionFilter {
        name: Some(name.to_owned()),
        domain: Some(domain),
        path: string("path")
            .filter(|path| !path.is_empty())
            .map(str::to_owned),
        partition,
    })
}
