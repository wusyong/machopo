//! `Content-Encoding`: a body decoded as its header says, a piece at a time
//! as it comes (Chromium's `SourceStream`s: `GzipSourceStream`,
//! `BrotliSourceStream`). The network service decodes it itself, not
//! reqwest, so it knows how many bytes came and the response keeps
//! `Content-Encoding` and `Content-Length`.

use std::io::Write;

use bytes::Bytes;
use flate2::{Crc, Decompress, FlushDecompress, Status};

/// What `Accept-Encoding` offers: the codings a [`Decoder`] undoes (Chrome
/// also offers `zstd`).
pub(crate) const ACCEPT_ENCODING: &str = "gzip, deflate, br";

/// How much more room inflating makes for its output at a time.
const OUTPUT_STEP: usize = 32 * 1024;

/// Undoes the codings `Content-Encoding` lists, last first (they were
/// applied in order), on each piece of the body as it comes. A body with a
/// coding it doesn't know is passed on as it came, all of it
/// (`URLRequestHttpJob::SetUpSourceStream` passes it on raw); an empty one
/// too. An error if a coding's data doesn't decode, or ends before its
/// stream does; whatever comes after a stream's end is left out.
pub(crate) struct Decoder {
    /// In the order they were applied.
    codings: Vec<Coding>,
    /// Whether any of the body came: an empty body isn't decoded.
    started: bool,
}

impl Decoder {
    pub(crate) fn new(content_encoding: Option<&str>) -> Self {
        let mut codings = Vec::new();
        for coding in content_encoding.unwrap_or_default().split(',') {
            match coding.trim().to_ascii_lowercase().as_str() {
                "" | "identity" => {}
                "gzip" | "x-gzip" => codings.push(Coding::Gzip(Gzip::default())),
                "deflate" => codings.push(Coding::Deflate(Deflate::default())),
                "br" => codings.push(Coding::Brotli(Box::new(
                    brotli_decompressor::DecompressorWriter::new(Vec::new(), 4096),
                ))),
                _ => {
                    codings.clear();
                    break;
                }
            }
        }
        Self {
            codings,
            started: false,
        }
    }

    /// What `piece`, the next of the body, decodes to: perhaps nothing yet.
    pub(crate) fn push(&mut self, piece: Bytes) -> Result<Bytes, String> {
        if self.codings.is_empty() || piece.is_empty() {
            return Ok(piece);
        }
        self.started = true;
        let mut data: Option<Vec<u8>> = None;
        for coding in self.codings.iter_mut().rev() {
            let decoded = coding.push(data.as_deref().unwrap_or(&piece))?;
            data = Some(decoded);
        }
        Ok(data.unwrap_or_default().into())
    }

    /// The rest, now the body is over.
    pub(crate) fn finish(&mut self) -> Result<Bytes, String> {
        if !self.started {
            return Ok(Bytes::new());
        }
        let mut data = Vec::new();
        for coding in self.codings.iter_mut().rev() {
            let mut decoded = coding.push(&data)?;
            decoded.extend(coding.finish()?);
            data = decoded;
        }
        Ok(data.into())
    }
}

enum Coding {
    Gzip(Gzip),
    Deflate(Deflate),
    Brotli(Box<brotli_decompressor::DecompressorWriter<Vec<u8>>>),
}

impl Coding {
    fn name(&self) -> &'static str {
        match self {
            Self::Gzip(_) => "gzip",
            Self::Deflate(_) => "deflate",
            Self::Brotli(_) => "br",
        }
    }

    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, String> {
        let name = self.name();
        match self {
            Self::Gzip(gzip) => gzip.push(input),
            Self::Deflate(deflate) => deflate.push(input),
            Self::Brotli(brotli) => {
                let mut input = input;
                while !input.is_empty() {
                    match brotli.write(input) {
                        // The stream is over: the rest is left out.
                        Ok(0) => break,
                        // At most what it was given (`Write`'s contract).
                        Ok(taken) => input = input.get(taken..).unwrap_or_default(),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                Ok(std::mem::take(brotli.get_mut()))
            }
        }
        .map_err(|e| format!("{name} doesn't decode: {e}"))
    }

    /// What is left once the input is over; an error if the stream hadn't
    /// ended.
    fn finish(&mut self) -> Result<Vec<u8>, String> {
        let name = self.name();
        let ended = match self {
            Self::Gzip(gzip) => matches!(gzip.stage, GzipStage::Done),
            Self::Deflate(deflate) => deflate.inflate.as_ref().is_some_and(|i| i.ended),
            Self::Brotli(brotli) => {
                let closed = brotli.close().is_ok();
                if closed {
                    return Ok(std::mem::take(brotli.get_mut()));
                }
                false
            }
        };
        match ended {
            true => Ok(Vec::new()),
            false => Err(format!("{name} doesn't decode: it ends before its stream")),
        }
    }
}

/// Raw deflate or zlib data (RFC 1951, 1950), inflated as it comes.
struct Inflate {
    state: Decompress,
    /// The stream is over: what comes after isn't its.
    ended: bool,
}

impl Inflate {
    fn new(zlib_header: bool) -> Self {
        Self {
            state: Decompress::new(zlib_header),
            ended: false,
        }
    }

    /// Inflates `input` onto `out`; how much of it the stream took (all of
    /// it, unless the stream ended within).
    fn push(&mut self, mut input: &[u8], out: &mut Vec<u8>) -> Result<usize, String> {
        let mut taken = 0usize;
        while !self.ended {
            out.reserve(OUTPUT_STEP);
            let (in_before, out_before) = (self.state.total_in(), self.state.total_out());
            let status = self
                .state
                .decompress_vec(input, out, FlushDecompress::None)
                .map_err(|e| e.to_string())?;
            // At most `input`'s length: it can't take more than it was given.
            let consumed = usize::try_from(self.state.total_in().saturating_sub(in_before))
                .unwrap_or(usize::MAX)
                .min(input.len());
            let produced = self.state.total_out().saturating_sub(out_before);
            input = input.get(consumed..).unwrap_or_default();
            taken = taken.saturating_add(consumed);
            match status {
                Status::StreamEnd => self.ended = true,
                Status::Ok | Status::BufError => {
                    // It wants more input: it took all there was and had
                    // room left for more output, or could do nothing.
                    let room_left = out.len() < out.capacity();
                    if (input.is_empty() && room_left) || (consumed == 0 && produced == 0) {
                        break;
                    }
                }
            }
        }
        Ok(taken)
    }
}

/// gzip (RFC 1952): a header, deflate data, and a trailer with its CRC and
/// length. Only the first member, as before; what follows is left out.
#[derive(Default)]
struct Gzip {
    stage: GzipStage,
    /// The header or the trailer, until all of it came.
    pending: Vec<u8>,
    /// Of what was inflated.
    crc: Crc,
}

#[derive(Default)]
enum GzipStage {
    #[default]
    Header,
    Body(Inflate),
    Trailer,
    Done,
}

/// The trailer: CRC-32 and the length modulo 2^32, little-endian.
const GZIP_TRAILER_LENGTH: usize = 8;

impl Gzip {
    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        if matches!(self.stage, GzipStage::Header) {
            self.pending.extend_from_slice(input);
            let Some(length) = gzip_header_length(&self.pending)? else {
                return Ok(out);
            };
            let rest = self.pending.split_off(length);
            self.pending.clear();
            self.stage = GzipStage::Body(Inflate::new(false));
            self.after_header(&rest, &mut out)?;
        } else {
            self.after_header(input, &mut out)?;
        }
        Ok(out)
    }

    fn after_header(&mut self, mut input: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        if let GzipStage::Body(inflate) = &mut self.stage {
            let start = out.len();
            let taken = inflate.push(input, out)?;
            // What it added to `out`, and the rest of `input` (it takes
            // no more than it is given).
            self.crc.update(out.get(start..).unwrap_or_default());
            input = input.get(taken..).unwrap_or_default();
            if inflate.ended {
                self.stage = GzipStage::Trailer;
            }
        }
        if matches!(self.stage, GzipStage::Trailer) {
            let wanted = GZIP_TRAILER_LENGTH.saturating_sub(self.pending.len());
            self.pending
                .extend_from_slice(input.get(..wanted).unwrap_or(input));
            if let Ok(trailer) = <[u8; GZIP_TRAILER_LENGTH]>::try_from(self.pending.as_slice()) {
                let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
                let length = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
                if crc != self.crc.sum() || length != self.crc.amount() {
                    return Err("its CRC or length doesn't match".into());
                }
                self.stage = GzipStage::Done;
            }
        }
        Ok(())
    }
}

/// How long the gzip header at the start of `data` is, once all of it is
/// there; an error as soon as it can't be one.
fn gzip_header_length(data: &[u8]) -> Result<Option<usize>, String> {
    const MAGIC: [u8; 3] = [0x1f, 0x8b, 8];
    const FHCRC: u8 = 0x02;
    const FEXTRA: u8 = 0x04;
    const FNAME: u8 = 0x08;
    const FCOMMENT: u8 = 0x10;
    const RESERVED: u8 = 0xe0;
    if data.iter().zip(MAGIC).any(|(&byte, magic)| byte != magic) {
        return Err("not a gzip header".into());
    }
    // ID1 ID2 CM FLG MTIME(4) XFL OS.
    let Some(&flags) = data.get(3) else {
        return Ok(None);
    };
    if flags & RESERVED != 0 {
        return Err("reserved header flags".into());
    }
    let mut at = 10usize;
    if flags & FEXTRA != 0 {
        let Some(&[low, high]) = data.get(at..at.saturating_add(2)) else {
            return Ok(None);
        };
        at = at
            .saturating_add(2)
            .saturating_add(usize::from(u16::from_le_bytes([low, high])));
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let Some(end) = data
                .get(at..)
                .and_then(|rest| rest.iter().position(|&b| b == 0))
            else {
                return Ok(None);
            };
            at = at.saturating_add(end).saturating_add(1);
        }
    }
    if flags & FHCRC != 0 {
        at = at.saturating_add(2);
    }
    Ok((data.len() >= at).then_some(at))
}

/// `deflate` is zlib's format, but servers send raw deflate too: zlib's if
/// it starts with a zlib header, raw if not (Chromium's `GzipSourceStream`
/// looks at the first two bytes the same way).
#[derive(Default)]
struct Deflate {
    /// Until it is known which.
    pending: Vec<u8>,
    inflate: Option<Inflate>,
}

impl Deflate {
    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        match &mut self.inflate {
            Some(inflate) => {
                inflate.push(input, &mut out)?;
            }
            None => {
                self.pending.extend_from_slice(input);
                let Some(&[cmf, flg]) = self.pending.get(..2) else {
                    return Ok(out);
                };
                let pending = std::mem::take(&mut self.pending);
                self.inflate
                    .insert(Inflate::new(is_zlib_header(cmf, flg)))
                    .push(&pending, &mut out)?;
            }
        }
        Ok(out)
    }
}

/// RFC 1950's CMF and FLG: deflate with a window of at most 32 KiB, and a
/// check that makes them a multiple of 31.
fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && (u16::from(cmf) << 8 | u16::from(flg)) % 31 == 0
}

#[cfg(test)]
mod tests {
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

    /// `body` decoded in pieces of `piece` bytes.
    fn decode_in(
        content_encoding: Option<&str>,
        body: &[u8],
        piece: usize,
    ) -> Result<Vec<u8>, String> {
        let mut decoder = Decoder::new(content_encoding);
        let mut out = Vec::new();
        for piece in body.chunks(piece) {
            out.extend_from_slice(&decoder.push(Bytes::copy_from_slice(piece))?);
        }
        out.extend_from_slice(&decoder.finish()?);
        Ok(out)
    }

    /// `body` decoded whole, and a byte at a time: the same either way.
    fn decode(content_encoding: Option<&str>, body: Vec<u8>) -> Result<Vec<u8>, String> {
        let whole = decode_in(content_encoding, &body, body.len().max(1));
        let bytewise = decode_in(content_encoding, &body, 1);
        assert_eq!(whole, bytewise, "{content_encoding:?}");
        whole
    }

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

    /// More than one round of making room for the output, also once all
    /// of the input was taken: a little of it inflates to a lot.
    #[test]
    fn a_body_bigger_than_a_step_of_output_is_all_undone() {
        let big: Vec<u8> = (0..(5 * OUTPUT_STEP)).map(|i| (i % 7) as u8).collect();
        assert_eq!(decode_in(Some("gzip"), &gzip(&big), 1000).unwrap(), big);
        assert_eq!(decode_in(Some("deflate"), &zlib(&big), 1000).unwrap(), big);
        let zeros = vec![0; 100 * OUTPUT_STEP];
        assert_eq!(decode(Some("gzip"), gzip(&zeros)).unwrap(), zeros);
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
        assert!(decode(Some("deflate"), b"not deflate".to_vec()).is_err());
        assert!(decode(Some("br"), b"not brotli at all".to_vec()).is_err());
        let mut bad_crc = gzip(b"hello");
        let crc = bad_crc.len() - 8;
        bad_crc[crc] ^= 1;
        assert!(decode(Some("gzip"), bad_crc).is_err());
        let mut bad_length = gzip(b"hello");
        let length = bad_length.len() - 4;
        bad_length[length] ^= 1;
        assert!(decode(Some("gzip"), bad_length).is_err());
    }

    #[test]
    fn a_stream_cut_short_is_an_error() {
        let gzipped = gzip(b"hello world");
        for end in [5, 12, gzipped.len() - 1] {
            assert!(
                decode(Some("gzip"), gzipped[..end].to_vec()).is_err(),
                "{end}"
            );
        }
        let zlibbed = zlib(b"hello world");
        assert!(decode(Some("deflate"), zlibbed[..zlibbed.len() - 2].to_vec()).is_err());
        assert!(decode(Some("deflate"), zlibbed[..1].to_vec()).is_err());
        assert!(decode(Some("br"), BROTLI_HELLO[..6].to_vec()).is_err());
    }

    #[test]
    fn what_follows_a_stream_is_left_out() {
        let after = |mut body: Vec<u8>| {
            body.extend_from_slice(b"junk");
            body
        };
        assert_eq!(decode(Some("gzip"), after(gzip(b"hi"))).unwrap(), b"hi");
        assert_eq!(decode(Some("deflate"), after(zlib(b"hi"))).unwrap(), b"hi");
        assert_eq!(
            decode(Some("br"), after(BROTLI_HELLO.to_vec())).unwrap(),
            b"hello"
        );
    }

    /// A header with every optional field: extra data, a name, a comment
    /// and the header's CRC.
    #[test]
    fn a_gzip_header_with_its_optional_fields_is_skipped() {
        let plain = gzip(b"hello");
        let mut header = vec![0x1f, 0x8b, 8, 0x02 | 0x04 | 0x08 | 0x10, 0, 0, 0, 0, 0, 3];
        header.extend_from_slice(&[3, 0, b'a', 0, b'c']);
        header.extend_from_slice(b"name\0comment\0");
        header.extend_from_slice(&[0xaa, 0xbb]);
        let body = [header, plain[10..].to_vec()].concat();
        assert_eq!(decode(Some("gzip"), body).unwrap(), b"hello");
        let mut reserved = plain.clone();
        reserved[3] = 0x20;
        assert!(decode(Some("gzip"), reserved).is_err());
    }

    #[test]
    fn a_zlib_header_is_told_from_raw_deflate() {
        let zlibbed = zlib(b"x");
        assert!(is_zlib_header(zlibbed[0], zlibbed[1]));
        let raw = raw_deflate(b"x");
        assert!(!is_zlib_header(raw[0], raw[1]));
        // CM 8 and a multiple of 31, but a window over 32 KiB.
        assert!(!is_zlib_header(0x88, 0x1c));
    }
}
