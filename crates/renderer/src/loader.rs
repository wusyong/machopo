//! The renderer's requests to the browser's network (Blink's
//! `ResourceFetcher` handing loads to the frame's `URLLoaderFactory`): their
//! ids, unique in the renderer, so a response for a document that has gone
//! is recognised and dropped. And the blob URLs the renderer's realms
//! registered (`BlobUrls`).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use base::SequenceNumber;
use crossbeam_channel::Sender;
use dom_api::FrameId;
use protocol::network::ResourceRequest;
use protocol::renderer::RendererHostMsg;

/// The next request id, shared by everything in the renderer that sends
/// one; and the URL of each request until it is answered, which the page's
/// log names a failed load by.
#[derive(Debug, Clone)]
pub(crate) struct RequestIds {
    next: Rc<RefCell<SequenceNumber<u64>>>,
    in_flight: Rc<RefCell<HashMap<u64, String>>>,
}

impl RequestIds {
    pub(crate) fn new() -> Self {
        Self {
            next: Rc::new(RefCell::new(SequenceNumber::starting_at(1))),
            in_flight: Rc::default(),
        }
    }

    pub(crate) fn next(&self) -> u64 {
        self.next.borrow_mut().get_next()
    }

    /// Request `request_id`, `request`, is being sent.
    pub(crate) fn sent(&self, request_id: u64, request: &ResourceRequest) {
        self.in_flight
            .borrow_mut()
            .insert(request_id, request.url.clone());
    }

    /// Request `request_id` was cancelled.
    pub(crate) fn cancelled(&self, request_id: u64) {
        self.in_flight.borrow_mut().remove(&request_id);
    }

    /// Request `request_id` was answered: its URL, if it was sent.
    pub(crate) fn answered(&self, request_id: u64) -> Option<String> {
        self.in_flight.borrow_mut().remove(&request_id)
    }
}

/// The blob URLs this renderer's realms registered (Blink's
/// `PublicURLManager`), which the browser serves (`RegisterBlobUrl`). A
/// revoked one is revoked here at once, for the `fetch()`es after it; the
/// browser hears of it once the loads asked for before it went
/// (`flush_revoked`), so they get the blob (File API's URL parser resolves
/// a blob URL as it parses it).
#[derive(Debug, Clone)]
pub(crate) struct BlobUrls {
    to_browser: Sender<RendererHostMsg>,
    state: Rc<RefCell<BlobUrlState>>,
}

#[derive(Debug, Default)]
struct BlobUrlState {
    /// The URLs registered and not revoked, by the frame whose realm
    /// registered each (its document at the time: the URL goes with the
    /// next).
    live: HashMap<String, FrameId>,
    /// Revoked since the browser last heard, in order.
    revoked: Vec<String>,
    /// Every one revoked.
    gone: HashSet<String>,
}

impl BlobUrls {
    pub(crate) fn new(to_browser: Sender<RendererHostMsg>) -> Self {
        Self {
            to_browser,
            state: Rc::default(),
        }
    }

    /// Registers `url` for a blob of `mime_type` and `bytes`, which frame
    /// `owner`'s realm made: the browser hears now, before any load of it.
    pub(crate) fn register(
        &self,
        url: &str,
        owner: FrameId,
        mime_type: &str,
        bytes: std::sync::Arc<[u8]>,
    ) {
        self.state.borrow_mut().live.insert(url.to_owned(), owner);
        let _ = self.to_browser.send(RendererHostMsg::RegisterBlobUrl {
            url: url.to_owned(),
            mime_type: mime_type.to_owned(),
            bytes,
        });
    }

    /// Revokes `url`, if one of this renderer's realms registered it.
    pub(crate) fn revoke(&self, url: &str) {
        let mut state = self.state.borrow_mut();
        if state.live.remove(url).is_some() {
            state.revoked.push(url.to_owned());
            state.gone.insert(url.to_owned());
        }
    }

    /// Whether `url` (without its fragment) was one of this renderer's,
    /// revoked.
    pub(crate) fn is_revoked(&self, url: &url::Url) -> bool {
        let mut url = url.clone();
        url.set_fragment(None);
        self.state.borrow().gone.contains(url.as_str())
    }

    /// Revokes the URLs the DOM's `frame`'s realm made: its document
    /// went (another took its place, or the frame went).
    pub(crate) fn revoke_frame(&self, frame: FrameId) {
        let urls: Vec<String> = self
            .state
            .borrow()
            .live
            .iter()
            .filter(|(_, owner)| **owner == frame)
            .map(|(url, _)| url.clone())
            .collect();
        for url in urls {
            self.revoke(&url);
        }
    }

    /// Tells the browser of the URLs revoked since it last heard: after the
    /// loads the tasks that revoked them asked for.
    pub(crate) fn flush_revoked(&self) {
        let revoked = std::mem::take(&mut self.state.borrow_mut().revoked);
        for url in revoked {
            let _ = self.to_browser.send(RendererHostMsg::RevokeBlobUrl { url });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blob URL is registered with the browser at once, with the blob's
    /// type and bytes; a revoke is the renderer's at once (`is_revoked`,
    /// its fragment aside) and the browser's only once flushed; a URL not
    /// registered, or revoked already, isn't revoked again; a frame's URLs
    /// are revoked when its document goes, the other frames' kept.
    #[test]
    fn blob_urls_are_registered_and_revoked() {
        let (to_browser, from_renderer) = crossbeam_channel::unbounded();
        let urls = BlobUrls::new(to_browser);
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"hi"[..]);
        urls.register("blob:null/1", FrameId(1), "a/b", bytes.clone());
        urls.register("blob:null/2", FrameId(2), "a/b", bytes.clone());
        let registered: Vec<(String, String, Vec<u8>)> = from_renderer
            .try_iter()
            .map(|msg| match msg {
                RendererHostMsg::RegisterBlobUrl {
                    url,
                    mime_type,
                    bytes,
                } => (url, mime_type, bytes.to_vec()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            registered,
            [
                ("blob:null/1".into(), "a/b".into(), b"hi".to_vec()),
                ("blob:null/2".into(), "a/b".into(), b"hi".to_vec()),
            ]
        );
        let first: url::Url = "blob:null/1#x".parse().unwrap();
        assert!(!urls.is_revoked(&first));
        urls.revoke("blob:null/1");
        urls.revoke("blob:null/1");
        urls.revoke("blob:null/unknown");
        assert!(urls.is_revoked(&first));
        assert!(from_renderer.try_recv().is_err(), "not before the flush");
        urls.flush_revoked();
        let revoked = |rx: &crossbeam_channel::Receiver<RendererHostMsg>| -> Vec<String> {
            rx.try_iter()
                .map(|msg| match msg {
                    RendererHostMsg::RevokeBlobUrl { url } => url,
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        assert_eq!(revoked(&from_renderer), ["blob:null/1"]);
        urls.register("blob:null/3", FrameId(1), "a/b", bytes.clone());
        urls.register("blob:null/4", FrameId(3), "a/b", bytes);
        let _ = from_renderer.try_iter().count();
        urls.revoke_frame(FrameId(3));
        urls.flush_revoked();
        assert_eq!(
            revoked(&from_renderer),
            ["blob:null/4"],
            "that frame's only"
        );
        assert!(urls.is_revoked(&"blob:null/4".parse().unwrap()));
        assert!(!urls.is_revoked(&"blob:null/2".parse().unwrap()));
    }
}
