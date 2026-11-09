//! `Content-Encoding`: a body decoded as its header says (Chromium's
//! `SourceStream`s: `GzipSourceStream`, `BrotliSourceStream`). The network
//! service decodes it itself, not reqwest, so it knows how many bytes came
//! and the response keeps `Content-Encoding` and `Content-Length`.

use std::io::Read;

/// What `Accept-Encoding` offers: the codings `decode` undoes (Chrome also
/// offers `zstd`).
pub(crate) const ACCEPT_ENCODING: &str = "gzip, deflate, br";

/// `body` with the codings `content_encoding` lists undone, last first (they
/// were applied in order). A body with a coding it doesn't know is left as
/// it came, all of it (`URLRequestHttpJob::SetUpSourceStream` passes it on
/// raw); an empty one too. An error if a coding's data doesn't decode.
pub(crate) fn decode(content_encoding: Option<&str>, body: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(content_encoding) = content_encoding else {
        return Ok(body);
    };
    let mut codings = Vec::new();
    for coding in content_encoding.split(',') {
        match coding.trim().to_ascii_lowercase().as_str() {
            "" | "identity" => {}
            "gzip" | "x-gzip" => codings.push(Coding::Gzip),
            "deflate" => codings.push(Coding::Deflate),
            "br" => codings.push(Coding::Brotli),
            _ => return Ok(body),
        }
    }
    if body.is_empty() {
        return Ok(body);
    }
    codings
        .iter()
        .rev()
        .try_fold(body, |body, coding| coding.decode(&body))
}

#[derive(Debug, Clone, Copy)]
enum Coding {
    Gzip,
    Deflate,
    Brotli,
}

impl Coding {
    fn decode(self, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut decoded = Vec::new();
        let read = match self {
            Self::Gzip => flate2::read::GzDecoder::new(data).read_to_end(&mut decoded),
            // `deflate` is zlib's format, but servers send raw deflate too
            // (Chromium's `GzipSourceStream` takes either).
            Self::Deflate => match flate2::read::ZlibDecoder::new(data).read_to_end(&mut decoded) {
                Ok(n) => Ok(n),
                Err(_) => {
                    decoded.clear();
                    flate2::read::DeflateDecoder::new(data).read_to_end(&mut decoded)
                }
            },
            Self::Brotli => {
                brotli_decompressor::Decompressor::new(data, 4096).read_to_end(&mut decoded)
            }
        };
        read.map(|_| decoded)
            .map_err(|e| format!("{self:?} doesn't decode: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn raw_deflate(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// "hello", brotli-compressed (`printf hello | brotli`).
    const BROTLI_HELLO: &[u8] = &[0x0b, 0x02, 0x80, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x03];

    #[test]
    fn each_coding_is_undone() {
        assert_eq!(decode(Some("gzip"), gzip(b"hello")).unwrap(), b"hello");
        assert_eq!(decode(Some("x-gzip"), gzip(b"hello")).unwrap(), b"hello");
        assert_eq!(decode(Some("deflate"), zlib(b"hello")).unwrap(), b"hello");
        assert_eq!(
            decode(Some("deflate"), raw_deflate(b"hello")).unwrap(),
            b"hello"
        );
        assert_eq!(decode(Some("br"), BROTLI_HELLO.to_vec()).unwrap(), b"hello");
        assert_eq!(
            decode(Some("identity"), b"hello".to_vec()).unwrap(),
            b"hello"
        );
        assert_eq!(decode(None, b"hello".to_vec()).unwrap(), b"hello");
    }

    #[test]
    fn codings_are_undone_last_first() {
        let twice = zlib(&gzip(b"hello"));
        assert_eq!(decode(Some("gzip, deflate"), twice).unwrap(), b"hello");
        assert_eq!(
            decode(Some(" GZIP ,identity"), gzip(b"hello")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn an_unknown_coding_leaves_the_body_as_it_came() {
        let body = gzip(b"hello");
        assert_eq!(decode(Some("gzip, zstd"), body.clone()).unwrap(), body);
    }

    #[test]
    fn an_empty_body_needs_no_decoding() {
        assert_eq!(decode(Some("gzip"), Vec::new()).unwrap(), b"");
    }

    #[test]
    fn data_that_doesnt_decode_is_an_error() {
        assert!(decode(Some("gzip"), b"not gzip".to_vec()).is_err());
        assert!(decode(Some("br"), b"not brotli at all".to_vec()).is_err());
    }
}
