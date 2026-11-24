//! A stored response (what Chromium's `HttpResponseInfo` and an entry's
//! body stream keep): its head as the transport gave it, when it was asked
//! for and came, its `Vary`, and its body, still in its `Content-Encoding`.

use bytes::Bytes;

use super::vary::VaryData;
use crate::cookies::Time;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredResponse {
    pub(crate) status: u16,
    pub(crate) status_text: String,
    /// `http/1.1`, `h2`...
    pub(crate) protocol: &'static str,
    /// Names lowercase, in order; no `Set-Cookie` (the cache never sets a
    /// cookie again, Chromium's `PERSIST_SANS_COOKIES`).
    pub(crate) headers: Vec<(String, String)>,
    /// When the request it answers was sent, and when its head came: what
    /// its age counts from.
    pub(crate) request_time: Time,
    pub(crate) response_time: Time,
    pub(crate) vary: Option<VaryData>,
    /// The body as the wire had it, in the pieces it came in.
    pub(crate) body: Vec<Bytes>,
}

impl StoredResponse {
    /// What it takes of the cache's size, in bytes: its headers, its
    /// `Vary`'s values and its body (Chromium counts an entry's key and its
    /// data streams).
    pub(crate) fn size(&self) -> u64 {
        let len = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        let headers = self.headers.iter().fold(0u64, |sum, (name, value)| {
            sum.saturating_add(len(name.len())).saturating_add(len(value.len()))
        });
        let vary = match &self.vary {
            Some(VaryData::Values(values)) => len(values.len()),
            Some(VaryData::Star) | None => 0,
        };
        let body = self
            .body
            .iter()
            .fold(0u64, |sum, piece| sum.saturating_add(len(piece.len())));
        headers
            .saturating_add(len(self.status_text.len()))
            .saturating_add(vary)
            .saturating_add(body)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_size_counts_headers_vary_status_text_and_body() {
        let response = StoredResponse {
            status: 200,
            status_text: "OK".to_owned(),
            protocol: "h2",
            headers: vec![("etag".to_owned(), "\"v1\"".to_owned())],
            request_time: Time::UNIX_EPOCH,
            response_time: Time::UNIX_EPOCH,
            vary: Some(VaryData::Values("en\n".to_owned())),
            body: vec![Bytes::from_static(b"abc"), Bytes::from_static(b"de")],
        };
        // 4 + 4, 2, 3, 3 + 2.
        assert_eq!(response.size(), 18);
        let star = StoredResponse {
            vary: Some(VaryData::Star),
            ..response
        };
        assert_eq!(star.size(), 15);
    }
}
