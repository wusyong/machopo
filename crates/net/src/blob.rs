//! `blob:` URLs (Chromium's `storage/browser/blob`: the `BlobUrlRegistry`
//! and `BlobURLLoader`): what each URL a renderer registered stands for,
//! and the response to a load of one (Fetch's "scheme fetch" of a `blob:`
//! URL). Chromium serves them in the browser process, not the network
//! service; machopo's network runs in the browser process, so here.
//!
//! A load from a renderer gets only the URLs that renderer registered
//! (Chromium partitions them too); a navigation, any. A load is answered at
//! once, as it is asked for: a URL revoked after a load was asked for still
//! gave it the blob. Revoking takes two steps, as the renderer's loads and
//! its navigations reach the browser on different threads: the IO thread
//! marks the URL revoked for loads in order with them, and the UI thread
//! forgets it in order with the navigations asked for before.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;

use protocol::network::{
    LoadResult, NetError, NetErrorCode, ResourceRequest, ResourceResponse, ResponseTainting,
    parse_single_range,
};
use url::Url;

use crate::mime;

/// A registered URL's blob, and the renderer that registered it.
struct Entry {
    owner: u64,
    mime_type: String,
    bytes: Arc<[u8]>,
    /// Revoked for its renderer's loads; still there for a navigation it
    /// asked for before.
    revoked: bool,
}

/// The blob URLs registered, by URL without its fragment. Every renderer
/// and navigation shares one.
#[derive(Clone, Default)]
pub(crate) struct BlobUrlRegistry {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

/// `url` without its fragment, as the registry keys it; `None` if it
/// doesn't parse.
fn key(url: &str) -> Option<String> {
    let mut url = Url::parse(url).ok()?;
    url.set_fragment(None);
    Some(url.into())
}

impl BlobUrlRegistry {
    pub(crate) fn register(&self, owner: u64, url: &str, mime_type: String, bytes: Arc<[u8]>) {
        let Some(key) = key(url) else {
            return;
        };
        let entry = Entry {
            owner,
            mime_type,
            bytes,
            revoked: false,
        };
        self.entries
            .lock()
            .expect("the blob URLs")
            .insert(key, entry);
    }

    /// Revokes `url` for loads, if renderer `owner` registered it.
    pub(crate) fn revoke(&self, owner: u64, url: &str) {
        let Some(key) = key(url) else {
            return;
        };
        let mut entries = self.entries.lock().expect("the blob URLs");
        if let Some(entry) = entries.get_mut(&key).filter(|entry| entry.owner == owner) {
            entry.revoked = true;
        }
    }

    /// Takes out `url`, if renderer `owner` registered it: for navigations
    /// too.
    pub(crate) fn forget(&self, owner: u64, url: &str) {
        let Some(key) = key(url) else {
            return;
        };
        let mut entries = self.entries.lock().expect("the blob URLs");
        if entries.get(&key).is_some_and(|entry| entry.owner == owner) {
            entries.remove(&key);
        }
    }

    /// Takes out every URL renderer `owner` registered: it has gone.
    pub(crate) fn revoke_all_of(&self, owner: u64) {
        self.entries
            .lock()
            .expect("the blob URLs")
            .retain(|_, entry| entry.owner != owner);
    }

    /// Fetch's "scheme fetch" of the `blob:` URL `request` is for, from
    /// renderer `requester` (a subresource), or for a navigation (`None`):
    /// a network error unless it is a `GET` of a URL registered (by the
    /// requester, and not revoked); the blob, or the range of it a `Range`
    /// header asks for.
    pub(crate) fn load(&self, request: &ResourceRequest, requester: Option<u64>) -> LoadResult {
        let not_found = || NetError::new(NetErrorCode::FileNotFound, request.url.clone());
        let key = key(&request.url).ok_or_else(not_found)?;
        let (mime_type, bytes) = {
            let entries = self.entries.lock().expect("the blob URLs");
            let entry = entries
                .get(&key)
                .filter(|entry| {
                    requester.is_none_or(|requester| entry.owner == requester && !entry.revoked)
                })
                .ok_or_else(not_found)?;
            (entry.mime_type.clone(), entry.bytes.clone())
        };
        // The response shares the blob's bytes, whole or a range of them.
        let bytes = Bytes::from_owner(bytes);
        if request.method != "GET" {
            return Err(NetError::new(
                NetErrorCode::Failed,
                format!("{}: a blob URL takes only GET", request.url),
            ));
        }
        let full_length = bytes.len() as u64;
        let range = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("range"))
            .map(|(_, value)| value.as_str());
        let (status, status_text, body, content_range) = match range {
            None => (200, "OK", bytes.clone(), None),
            Some(value) => {
                let invalid = || {
                    NetError::new(
                        NetErrorCode::Failed,
                        format!("{}: unsatisfiable range {value}", request.url),
                    )
                };
                let value: String = value.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
                let (start, end) = match parse_single_range(&value).ok_or_else(invalid)? {
                    // The last `end` bytes.
                    (None, Some(suffix)) => (
                        full_length.saturating_sub(suffix),
                        full_length.saturating_sub(1),
                    ),
                    (Some(start), end) => {
                        if start >= full_length {
                            return Err(invalid());
                        }
                        // Not 0: `start` is below it (exact).
                        let last = full_length.saturating_sub(1);
                        (start, end.map_or(last, |end| end.min(last)))
                    }
                    (None, None) => return Err(invalid()),
                };
                if full_length == 0 {
                    return Err(invalid());
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "start <= end < full_length, the length of `bytes`: both fit a usize"
                )]
                let body = bytes.slice(start as usize..=end as usize);
                (
                    206,
                    "Partial Content",
                    body,
                    Some(format!("bytes {start}-{end}/{full_length}")),
                )
            }
        };
        let mut headers = vec![
            ("content-length".to_owned(), body.len().to_string()),
            ("content-type".to_owned(), mime_type.clone()),
        ];
        if let Some(content_range) = content_range {
            headers.push(("content-range".to_owned(), content_range));
        }
        // A blob of no type is sniffed, as the network sniffs a response
        // without a `Content-Type`.
        let (essence, charset) = mime::parse_content_type(&mime_type)
            .unwrap_or_else(|| (mime::sniff(&body).to_owned(), None));
        Ok(ResourceResponse {
            url: request.url.clone(),
            status,
            status_text: status_text.to_owned(),
            redirected: false,
            tainting: ResponseTainting::Basic,
            mime_type: essence,
            charset,
            headers,
            // Timed by whoever loaded it.
            timing: None,
            remote_address: None,
            protocol: "blob".to_owned(),
            encoded_headers_length: 0,
            encoded_body_length: 0,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use protocol::network::RequestDestination;

    use super::*;

    const URL: &str = "blob:https://a.test/0f5c7f0e-5a1e-4c4b-9f6b-3b8a3b9c0d1e";

    fn registry() -> BlobUrlRegistry {
        let registry = BlobUrlRegistry::default();
        registry.register(
            1,
            URL,
            "text/plain;charset=utf-8".into(),
            Arc::from(&b"hello"[..]),
        );
        registry
    }

    fn get(url: &str) -> ResourceRequest {
        ResourceRequest::new(url, RequestDestination::Empty)
    }

    fn ranged(range: &str) -> ResourceRequest {
        let mut request = get(URL);
        request.headers.push(("Range".into(), range.into()));
        request
    }

    #[test]
    fn a_blob_url_gives_its_blob() {
        let registry = registry();
        let response = registry.load(&get(URL), Some(1)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.status_text, "OK");
        assert_eq!(response.body, &b"hello"[..]);
        assert_eq!(response.mime_type, "text/plain");
        assert_eq!(response.charset.as_deref(), Some("utf-8"));
        assert_eq!(
            response.headers,
            [
                ("content-length".to_owned(), "5".to_owned()),
                (
                    "content-type".to_owned(),
                    "text/plain;charset=utf-8".to_owned()
                ),
            ]
        );
        // Its fragment isn't the URL's; a navigation gets it whoever asks.
        let with_fragment = format!("{URL}#x");
        assert_eq!(
            registry.load(&get(&with_fragment), None).unwrap().body,
            &b"hello"[..]
        );
    }

    #[test]
    fn only_its_renderer_and_get() {
        let registry = registry();
        assert!(
            registry.load(&get(URL), Some(2)).is_err(),
            "another renderer's"
        );
        let mut post = get(URL);
        post.method = "POST".into();
        assert!(registry.load(&post, Some(1)).is_err());
        assert!(
            registry
                .load(&get("blob:https://a.test/other"), Some(1))
                .is_err()
        );
    }

    #[test]
    fn revoked_by_its_renderer() {
        let registry = registry();
        registry.revoke(2, URL);
        registry.forget(2, URL);
        assert!(
            registry.load(&get(URL), Some(1)).is_ok(),
            "not another's to revoke"
        );
        registry.revoke(1, &format!("{URL}#fragment"));
        assert!(registry.load(&get(URL), Some(1)).is_err());
        // A navigation asked for before gets it, until it is forgotten.
        assert!(registry.load(&get(URL), None).is_ok());
        registry.forget(1, URL);
        assert!(registry.load(&get(URL), None).is_err());
        let registry = self::registry();
        registry.revoke_all_of(2);
        assert!(registry.load(&get(URL), Some(1)).is_ok());
        registry.revoke_all_of(1);
        assert!(registry.load(&get(URL), None).is_err());
    }

    #[test]
    fn ranges() {
        let registry = registry();
        let body = |range: &str| {
            registry.load(&ranged(range), Some(1)).map(|response| {
                (
                    response.status,
                    String::from_utf8(response.body.to_vec()).unwrap(),
                )
            })
        };
        assert_eq!(body("bytes=1-3").unwrap(), (206, "ell".into()));
        assert_eq!(body("bytes=2-").unwrap(), (206, "llo".into()));
        assert_eq!(body("bytes=-2").unwrap(), (206, "lo".into()));
        assert_eq!(body("bytes=-9").unwrap(), (206, "hello".into()));
        assert_eq!(body("bytes=3-99").unwrap(), (206, "lo".into()));
        assert_eq!(body("bytes = 0 - 0").unwrap(), (206, "h".into()));
        assert!(body("bytes=5-").is_err());
        assert!(body("bytes=3-1").is_err());
        assert!(body("lines=1-2").is_err());
        let response = registry.load(&ranged("bytes=1-3"), Some(1)).unwrap();
        assert_eq!(response.status_text, "Partial Content");
        assert!(
            response
                .headers
                .contains(&("content-range".to_owned(), "bytes 1-3/5".to_owned()))
        );
    }
}
