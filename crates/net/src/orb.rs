//! Opaque response blocking, as Chromium's ORB v0.1 has it
//! (`services/network/public/cpp/orb/`): a renderer's `no-cors` request's
//! opaque response that is plainly HTML, XML or JSON (by its MIME type and
//! `nosniff`, or by its first 1024 bytes) doesn't reach the renderer,
//! which may not read it but would hold it in its memory (a renderer taken
//! over, or a page reading its own process's memory by a side channel,
//! would). What isn't plainly one of those is allowed ("fail open"): the
//! full ORB's last step, parsing the body as JavaScript, Chromium skips
//! too.
//!
//! A `fetch()`'s opaque response (the `Empty` destination) loses its body
//! with its tainting (`Cors::filter`), as Chromium gives it an empty one
//! for a blocked response; the other destinations' blocked responses are a
//! network error, `ERR_BLOCKED_BY_ORB`.

use protocol::network::{
    NetError, NetErrorCode, RequestDestination, RequestMode, ResourceRequest, ResponseTainting,
};

use crate::url_loader::Response;

/// How many of the body's first bytes are sniffed (`net::kMaxBytesToSniff`).
const SNIFF_LENGTH: usize = 1024;

/// Why a response was blocked (Chromium's `BlockingDecisionReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blocked {
    NeverSniffedMimeType,
    NoSniffHeader,
    UnexpectedRangeResponse,
    SniffedAsHtml,
    SniffedAsXml,
    SniffedAsJson,
}

/// What the headers alone decide (Chromium's `Decision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Allow,
    Block(Blocked),
    SniffMore,
}

/// A MIME type's kind, as ORB sorts them (`GetCanonicalMimeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Html,
    Xml,
    Json,
    Plain,
    NeverSniffed,
    Others,
}

/// Types the body is never sniffed for: blocked as they come (Chromium's
/// `IsNeverSniffedMimeType`).
const NEVER_SNIFFED: &[&str] = &[
    "application/gzip",
    "application/x-gzip",
    "application/zip",
    "text/event-stream",
    "application/msexcel",
    "application/mspowerpoint",
    "application/msword",
    "application/msword-template",
    "application/pdf",
    "application/vnd.ces-quickpoint",
    "application/vnd.ces-quicksheet",
    "application/vnd.ces-quickword",
    "application/vnd.ms-excel",
    "application/vnd.ms-excel.sheet.macroenabled.12",
    "application/vnd.ms-powerpoint",
    "application/vnd.ms-powerpoint.presentation.macroenabled.12",
    "application/vnd.ms-word",
    "application/vnd.ms-word.document.12",
    "application/vnd.ms-word.document.macroenabled.12",
    "application/vnd.msword",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.openxmlformats-officedocument.presentationml.template",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    "application/vnd.presentation-openxml",
    "application/vnd.presentation-openxmlm",
    "application/vnd.spreadsheet-openxml",
    "application/vnd.wordprocessing-openxml",
    "text/csv",
    "text/pdf",
    "application/proto",
    "application/rtf",
    "application/x-7z-compressed",
    "text/calendar",
    "text/rtf",
    "text/tab-separated-values",
    "text/vcard",
    "text/x-vcard",
    "multipart/signed",
    "multipart/byteranges",
];

/// `mime_type` (an essence, lowercase) as ORB sorts it: SVG and DASH
/// manifests before the `+xml` suffix makes them XML.
fn kind(mime_type: &str) -> Kind {
    match mime_type {
        "image/svg+xml" | "application/dash+xml" => Kind::Others,
        "text/html" => Kind::Html,
        "application/json" | "text/json" | "application/json+protobuf" => Kind::Json,
        _ if mime_type.ends_with("+json") => Kind::Json,
        "application/xml" | "text/xml" => Kind::Xml,
        _ if mime_type.ends_with("+xml") => Kind::Xml,
        "text/plain" => Kind::Plain,
        _ if NEVER_SNIFFED.contains(&mime_type)
            || ["opendocument", "protobuf", "yaml"]
                .iter()
                .any(|part| mime_type.contains(part))
            || mime_type.ends_with("+proto") =>
        {
            Kind::NeverSniffed
        }
        _ => Kind::Others,
    }
}

/// Types an opaque response of is allowed whatever its body (ORB's
/// "opaque-safelisted MIME type"): what `no-cors` is for, a stylesheet, an
/// SVG image, media and its manifests and captions.
fn safelisted(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "text/css"
            | "image/svg+xml"
            | "application/ogg"
            | "application/dash+xml"
            | "application/vnd.apple.mpegurl"
            | "text/vtt"
    ) || mime_type.starts_with("audio/")
        || mime_type.starts_with("video/")
}

/// The MIME type ORB goes by: `Content-Type`'s as Chromium parses it
/// (`HttpUtil::ParseContentType`), up to the first whitespace, `;` or `(`
/// of a value, so `image/svg xml` (a `+` decoded as a space) is
/// `image/svg`, where Fetch's parser takes it for none; of every value,
/// the last that is a `type/subtype` but `*/*`. None if there is none.
fn mime_type(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .filter(|(name, _)| name == "content-type")
        .flat_map(|(_, value)| crate::mime::split_header_values(value))
        .filter_map(|value| {
            let value = value.trim_start_matches([' ', '\t']);
            let end = value.find([' ', '\t', ';', '(']).unwrap_or(value.len());
            let essence = value.get(..end)?.to_ascii_lowercase();
            let (kind, subtype) = essence.split_once('/')?;
            (!kind.is_empty() && !subtype.is_empty() && essence != "*/*").then_some(essence)
        })
        .next_back()
}

/// The first value of `X-Content-Type-Options` is `nosniff`, any case.
fn nosniff(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name == "x-content-type-options")
        .and_then(|(_, value)| value.split(',').next())
        .is_some_and(|first| first.trim().eq_ignore_ascii_case("nosniff"))
}

/// A `206` of the middle of a resource: its `Content-Range` starts after
/// its first byte. Chromium lets one through for a URL it sniffed as media
/// before (a media element's later requests); machopo has no media
/// elements, so it lets none.
fn range_from_the_middle(status: u16, headers: &[(String, String)]) -> bool {
    status == 206
        && headers
            .iter()
            .find(|(name, _)| name == "content-range")
            .and_then(|(_, value)| value.trim().strip_prefix("bytes"))
            .and_then(|rest| rest.trim_start().split('-').next())
            .and_then(|first| first.trim().parse::<u64>().ok())
            .is_some_and(|first| first > 0)
}

/// What the response's head decides (Chromium's `Init`'s steps): its MIME
/// type is `Content-Type`'s, none if it sent no usable one.
fn decide(mime_type: Option<&str>, status: u16, headers: &[(String, String)]) -> Decision {
    if let Some(mime_type) = mime_type {
        if safelisted(mime_type) {
            return Decision::Allow;
        }
        match kind(mime_type) {
            Kind::NeverSniffed => return Decision::Block(Blocked::NeverSniffedMimeType),
            Kind::Html | Kind::Json | Kind::Plain | Kind::Xml if nosniff(headers) => {
                return Decision::Block(Blocked::NoSniffHeader);
            }
            _ => {}
        }
    }
    if range_from_the_middle(status, headers) {
        return Decision::Block(Blocked::UnexpectedRangeResponse);
    }
    Decision::SniffMore
}

/// A sniffer's answer: the body starts with it, doesn't, or is too short
/// to tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sniffed {
    Yes,
    No,
    Maybe,
}

fn strip_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data)
}

fn trim_leading_whitespace(data: &[u8]) -> &[u8] {
    let start = data
        .iter()
        .position(|byte| !b"\t\n\x0C\r ".contains(byte))
        .unwrap_or(data.len());
    data.get(start..).unwrap_or_default()
}

/// Whether `data` starts with one of `signatures` (Yes), might once more
/// of it comes (Maybe: it is a prefix of one), or doesn't (No).
fn matches_signature(data: &[u8], signatures: &[&[u8]], ignore_case: bool) -> Sniffed {
    let same = |a: &[u8], b: &[u8]| {
        if ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let mut maybe = false;
    for signature in signatures {
        match data.get(..signature.len()) {
            Some(start) if same(start, signature) => return Sniffed::Yes,
            None if signature
                .get(..data.len())
                .is_some_and(|start| same(start, data)) =>
            {
                maybe = true;
            }
            _ => {}
        }
    }
    if maybe { Sniffed::Maybe } else { Sniffed::No }
}

/// HTML: one of the tags HTML documents start with, any case, after
/// whitespace and HTML comments (`<!--` alone isn't a signature: it is
/// valid JavaScript too). A comment skipped, scanning goes on at the end
/// of its line, as JavaScript would.
fn sniff_html(data: &[u8]) -> Sniffed {
    const SIGNATURES: &[&[u8]] = &[
        b"<!doctype html",
        b"<script",
        b"<html",
        b"<head",
        b"<iframe",
        b"<h1",
        b"<div",
        b"<font",
        b"<table",
        b"<a",
        b"<style",
        b"<title",
        b"<b",
        b"<p",
    ];
    let mut data = strip_bom(data);
    while !data.is_empty() {
        data = trim_leading_whitespace(data);
        let found = matches_signature(data, SIGNATURES, true);
        if found != Sniffed::No {
            return found;
        }
        match skip_html_comment(data) {
            Some(rest) => data = rest,
            None if b"<!--".starts_with(data) || data.starts_with(b"<!--") => {
                return Sniffed::Maybe;
            }
            None => return Sniffed::No,
        }
    }
    Sniffed::Maybe
}

/// `data` past the HTML comment it starts with, at the line terminator
/// after it (`\n`, `\r`, U+2028, U+2029); none if it doesn't start with
/// one, or the comment or its line doesn't end in `data`.
fn skip_html_comment(data: &[u8]) -> Option<&[u8]> {
    let inside = data.strip_prefix(b"<!--")?;
    let end = inside.windows(3).position(|three| three == b"-->")?;
    let after = inside.get(end.checked_add(3)?..)?;
    let line_end = (0..after.len()).find(|&i| {
        let rest = after.get(i..).unwrap_or_default();
        rest.starts_with(b"\n")
            || rest.starts_with(b"\r")
            || rest.starts_with("\u{2028}".as_bytes())
            || rest.starts_with("\u{2029}".as_bytes())
    })?;
    after.get(line_end..)
}

/// XML: `<?xml`, after whitespace, case sensitive.
fn sniff_xml(data: &[u8]) -> Sniffed {
    matches_signature(trim_leading_whitespace(strip_bom(data)), &[b"<?xml"], false)
}

/// JSON: an object's first key and its colon (`{ "key":`), whitespace
/// outside the string aside.
fn sniff_json(data: &[u8]) -> Sniffed {
    #[derive(Clone, Copy)]
    enum State {
        Start,
        LeftBrace,
        LeftQuote,
        Escape,
        RightQuote,
    }
    let mut state = State::Start;
    for &byte in data {
        let in_string = matches!(state, State::LeftQuote | State::Escape);
        if !in_string && b"\t\n\x0C\r ".contains(&byte) {
            continue;
        }
        state = match (state, byte) {
            (State::Start, b'{') => State::LeftBrace,
            (State::LeftBrace, b'"') => State::LeftQuote,
            (State::LeftQuote, b'"') => State::RightQuote,
            (State::LeftQuote, b'\\') => State::Escape,
            (State::LeftQuote, _) | (State::Escape, _) => State::LeftQuote,
            (State::RightQuote, b':') => return Sniffed::Yes,
            _ => return Sniffed::No,
        };
    }
    Sniffed::Maybe
}

/// What only `fetch()` can read: a prefix that keeps a script from running
/// (what JSON APIs put before their data), or JSON.
fn sniff_fetch_only(data: &[u8]) -> Sniffed {
    const PREFIXES: &[&[u8]] = &[
        b")]}'",
        b"{}&&",
        b"{} &&",
        b"for(;;);",
        b"while(1);",
        b"for (;;);",
        b"while (1);",
    ];
    let data = strip_bom(data);
    let found = matches_signature(data, PREFIXES, false);
    if found != Sniffed::No {
        return found;
    }
    sniff_json(data)
}

/// What the body's first bytes decide (Chromium's `Sniff`, then its end of
/// the body: allowed, failing open). Chromium first lets through what its
/// magic numbers make an image, audio or video, keeping a media one's URL
/// for its later range requests: machopo has no media elements to make
/// those, and none of those magic numbers starts as the HTML, XML and JSON
/// below do, so that step would let through nothing these block, and isn't
/// here.
fn sniff(mime_type: Option<&str>, data: &[u8]) -> Result<(), Blocked> {
    if mime_type.is_none() {
        return Ok(());
    }
    if sniff_html(data) == Sniffed::Yes {
        return Err(Blocked::SniffedAsHtml);
    }
    if sniff_xml(data) == Sniffed::Yes {
        return Err(Blocked::SniffedAsXml);
    }
    if sniff_fetch_only(data) == Sniffed::Yes {
        return Err(Blocked::SniffedAsJson);
    }
    Ok(())
}

/// Whether ORB looks at `request`'s response of `tainting`: a renderer's
/// (it has an initiator) `no-cors` one of another origin (opaque), but a
/// `fetch()`'s, whose opaque response has lost its body already.
fn applies(request: &ResourceRequest, tainting: ResponseTainting) -> bool {
    request.mode == RequestMode::NoCors
        && request.initiator.is_some()
        && tainting == ResponseTainting::Opaque
        && request.destination != RequestDestination::Empty
}

/// `response` to `request`, or the network error it is blocked as: its
/// first bytes read ahead (they still come from its body) if its head
/// doesn't decide.
pub(crate) async fn check(
    request: &ResourceRequest,
    mut response: Response,
) -> Result<Response, NetError> {
    if !applies(request, response.head.tainting) {
        return Ok(response);
    }
    let mime_type = mime_type(&response.head.headers);
    let decided = match decide(
        mime_type.as_deref(),
        response.head.status,
        &response.head.headers,
    ) {
        Decision::Allow => Ok(()),
        Decision::Block(why) => Err(why),
        Decision::SniffMore => {
            let start = response.body.read_ahead(SNIFF_LENGTH).await?;
            sniff(
                mime_type.as_deref(),
                start.get(..SNIFF_LENGTH).unwrap_or(&start),
            )
        }
    };
    match decided {
        Ok(()) => Ok(response),
        Err(why) => {
            tracing::debug!(url = %request.url, ?why, "a renderer's opaque response blocked");
            Err(NetError::new(
                NetErrorCode::BlockedByOrb,
                format!("{}: blocked by ORB ({why:?})", request.url),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// What `mime_type` (none: no `Content-Type`) and `body` come to, a
    /// `200` with no other headers.
    fn verdict(mime_type: Option<&str>, body: &[u8]) -> Result<(), Blocked> {
        match decide(mime_type, 200, &[]) {
            Decision::Allow => Ok(()),
            Decision::Block(why) => Err(why),
            Decision::SniffMore => sniff(mime_type, body),
        }
    }

    /// `Content-Type` as Chromium parses it: up to whitespace, `;` or
    /// `(`, the last usable value; `*/*` and what has no subtype are none.
    #[test]
    fn the_mime_type_is_chromiums() {
        let of = |values: &[&str]| {
            let headers: Vec<(String, String)> = values
                .iter()
                .map(|value| ("content-type".to_owned(), (*value).to_owned()))
                .collect();
            mime_type(&headers)
        };
        assert_eq!(of(&["image/svg xml"]).as_deref(), Some("image/svg"));
        assert_eq!(
            of(&[" Text/HTML; charset=utf-8"]).as_deref(),
            Some("text/html")
        );
        assert_eq!(
            of(&["text/html, application/json"]).as_deref(),
            Some("application/json")
        );
        assert_eq!(of(&["text/html", "*/*"]).as_deref(), Some("text/html"));
        assert_eq!(of(&["text/"]), None);
        assert_eq!(of(&["nonsense"]), None);
        assert_eq!(of(&[]), None);
    }

    #[test]
    fn safelisted_types_are_allowed_whatever_they_hold() {
        for mime_type in [
            "text/css",
            "image/svg+xml",
            "audio/mpeg",
            "video/mp4",
            "application/ogg",
            "application/dash+xml",
            "application/vnd.apple.mpegurl",
            "text/vtt",
        ] {
            assert_eq!(verdict(Some(mime_type), b"<html>"), Ok(()), "{mime_type}");
        }
    }

    #[test]
    fn never_sniffed_types_are_blocked() {
        for mime_type in [
            "application/pdf",
            "application/zip",
            "text/csv",
            "text/event-stream",
            "multipart/byteranges",
            "application/vnd.oasis.opendocument.text",
            "application/x-protobuf",
            "application/yaml",
            "application/x+proto",
        ] {
            assert_eq!(
                verdict(Some(mime_type), b"GIF89a"),
                Err(Blocked::NeverSniffedMimeType),
                "{mime_type}"
            );
        }
    }

    /// `nosniff` blocks HTML, JSON, XML and plain text as they come; not
    /// SVG (an image), nor a type ORB doesn't sort; only the first value
    /// counts.
    #[test]
    fn nosniff_blocks_what_would_be_sniffed() {
        let nosniff = headers(&[("x-content-type-options", "NoSniff, other")]);
        for mime_type in [
            "text/html",
            "application/json",
            "application/ld+json",
            "text/json",
            "application/xml",
            "application/atom+xml",
            "text/plain",
        ] {
            assert_eq!(
                decide(Some(mime_type), 200, &nosniff),
                Decision::Block(Blocked::NoSniffHeader),
                "{mime_type}"
            );
        }
        assert_eq!(
            decide(Some("application/javascript"), 200, &nosniff),
            Decision::SniffMore
        );
        let second = headers(&[("x-content-type-options", "other, nosniff")]);
        assert_eq!(decide(Some("text/html"), 200, &second), Decision::SniffMore);
    }

    #[test]
    fn a_range_from_the_middle_is_blocked() {
        let middle = headers(&[("content-range", "bytes 10-19/100")]);
        assert_eq!(
            decide(Some("image/png"), 206, &middle),
            Decision::Block(Blocked::UnexpectedRangeResponse)
        );
        let start = headers(&[("content-range", "bytes 0-9/100")]);
        assert_eq!(decide(Some("image/png"), 206, &start), Decision::SniffMore);
        assert_eq!(decide(Some("image/png"), 200, &middle), Decision::SniffMore);
    }

    /// HTML by its first tag, any case, after a BOM, whitespace and
    /// comments (a comment's line ends first); not `<!--` alone, which is
    /// JavaScript too.
    #[test]
    fn html_is_sniffed_by_its_first_tag() {
        for body in [
            &b"<!DOCTYPE html><p>"[..],
            b"\xEF\xBB\xBF  <HTML>",
            b"\n\t<script>x</script>",
            b"<!-- a comment -->\n<div>",
            b"<pre>",
        ] {
            assert_eq!(
                verdict(Some("application/javascript"), body),
                Err(Blocked::SniffedAsHtml),
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
        for body in [&b"<!-- a comment --> x = 1;"[..], b"x = '<html>'", b"<!--"] {
            assert_eq!(
                verdict(Some("text/html"), body),
                Ok(()),
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn xml_is_sniffed_by_its_declaration_case_and_all() {
        assert_eq!(
            verdict(Some("text/plain"), b" <?xml version"),
            Err(Blocked::SniffedAsXml)
        );
        assert_eq!(verdict(Some("text/plain"), b"<?XML version"), Ok(()));
    }

    /// JSON by an object's first key and colon; the prefixes that keep a
    /// script from running, case and spacing as they are.
    #[test]
    fn json_and_script_breaking_prefixes_are_sniffed() {
        for body in [
            &br#"{"a": 1}"#[..],
            b"{ \"a\\\"b\" :",
            b")]}'\n[1]",
            b"{}&&",
            b"for(;;);",
            b"while (1);",
        ] {
            assert_eq!(
                verdict(Some("application/javascript"), body),
                Err(Blocked::SniffedAsJson),
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
        for body in [
            &b"{a: 1}"[..],
            b"[1, 2]",
            b"{\"a\"",
            b"{\"a\", 1}",
            b"WHILE(1);",
        ] {
            assert_eq!(
                verdict(Some("application/json"), body),
                Ok(()),
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    /// Images and media are allowed, whatever their label: no magic number
    /// starts as HTML, XML or JSON do. With no `Content-Type`, nothing is
    /// sniffed: allowed.
    #[test]
    fn images_and_media_are_allowed_and_an_unlabeled_body_too() {
        for body in [
            &b"\x89PNG\x0D\x0A\x1A\x0A"[..],
            b"GIF89a",
            b"\xFF\xD8\xFF\xE0",
            b"RIFF\0\0\0\0WEBPVP8 ",
            b"\x1A\x45\xDF\xA3",
            b"\0\0\0\x18ftypmp42",
            b"\xFF\xFB\x90",
        ] {
            assert_eq!(verdict(Some("text/html"), body), Ok(()), "{body:?}");
        }
        assert_eq!(verdict(None, b"<html>"), Ok(()));
        assert_eq!(verdict(None, br#"{"a":1}"#), Ok(()));
    }

    /// What can't be told, a script, or an empty body: allowed.
    #[test]
    fn what_cant_be_told_is_allowed() {
        for body in [&b"var x = 1;"[..], b"", b"   ", b"<!-- unterminated"] {
            assert_eq!(verdict(Some("text/html"), body), Ok(()), "{body:?}");
        }
    }

    #[test]
    fn only_a_renderers_cross_origin_no_cors_subresource_is_looked_at() {
        let mut request = ResourceRequest::new("https://b.example/x", RequestDestination::Image);
        request.initiator = Some("https://a.example".to_owned());
        assert!(applies(&request, ResponseTainting::Opaque));
        assert!(!applies(&request, ResponseTainting::Basic));
        let mut fetch = request.clone();
        fetch.destination = RequestDestination::Empty;
        assert!(!applies(&fetch, ResponseTainting::Opaque));
        let mut cors = request.clone();
        cors.mode = RequestMode::Cors;
        assert!(!applies(&cors, ResponseTainting::Cors));
        let mut browsers = request;
        browsers.initiator = None;
        assert!(!applies(&browsers, ResponseTainting::Opaque));
    }
}
