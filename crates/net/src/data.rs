//! `data:` URLs (Chromium's `DataURL::Parse`): the body is in the URL, as
//! the Fetch Standard's "data: URL processor" reads it (percent-decoding,
//! forgiving base64, the MIME type before the comma, `text/plain;
//! charset=US-ASCII` when there is none).

use protocol::network::{LoadResult, NetError, NetErrorCode, ResourceResponse, ResponseTainting};
use url::Url;

pub(crate) fn load(url: &Url) -> LoadResult {
    let invalid = |detail: String| NetError::new(NetErrorCode::InvalidUrl, detail);
    let data =
        data_url::DataUrl::process(url.as_str()).map_err(|e| invalid(format!("{url}: {e:?}")))?;
    let (body, _fragment) = data
        .decode_to_vec()
        .map_err(|e| invalid(format!("{url}: {e:?}")))?;
    let mime = data.mime_type();
    Ok(ResourceResponse {
        url: url.as_str().to_owned(),
        status: 200,
        // Fetch's "scheme fetch" of a `data:` URL: "OK", and the MIME type
        // as its `Content-Type`.
        status_text: "OK".to_owned(),
        redirected: false,
        tainting: ResponseTainting::Basic,
        mime_type: format!("{}/{}", mime.type_, mime.subtype),
        charset: mime.get_parameter("charset").map(str::to_owned),
        headers: vec![("content-type".to_owned(), mime.to_string())],
        // Timed by whoever loaded it.
        timing: None,
        remote_address: None,
        protocol: "data".to_owned(),
        encoded_headers_length: 0,
        encoded_body_length: 0,
        body: body.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(url: &str) -> LoadResult {
        super::load(&Url::parse(url).unwrap())
    }

    #[test]
    fn base64_and_percent_encoded() {
        let response = load("data:application/wasm;base64,AGFzbQEAAAA=").unwrap();
        assert_eq!(response.mime_type, "application/wasm");
        assert_eq!(response.body, &b"\0asm\x01\0\0\0"[..]);
        assert_eq!(response.status, 200);
        let response = load("data:text/html;charset=UTF-8,%3Cp%3Eh%C3%A9").unwrap();
        assert_eq!(response.mime_type, "text/html");
        assert_eq!(response.charset.as_deref(), Some("UTF-8"));
        assert_eq!(response.body, "<p>hé".as_bytes());
    }

    #[test]
    fn defaults_and_errors() {
        let response = load("data:,hello#fragment").unwrap();
        assert_eq!(response.mime_type, "text/plain");
        assert_eq!(response.charset.as_deref(), Some("US-ASCII"));
        assert_eq!(response.body, &b"hello"[..]);
        // No comma; base64 that doesn't decode.
        assert_eq!(
            load("data:text/plain").unwrap_err().code,
            NetErrorCode::InvalidUrl
        );
        assert_eq!(
            load("data:;base64,a").unwrap_err().code,
            NetErrorCode::InvalidUrl
        );
    }
}
