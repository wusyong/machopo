//! Turning response bytes into text: HTML's encoding sniffing algorithm for
//! documents, and css-syntax's for stylesheets (Blink's
//! `TextResourceDecoder`). Decoding itself is encoding_rs, which follows
//! the WHATWG Encoding standard.

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252, X_USER_DEFINED};

/// How far `<meta charset>` (and `<meta name="script">`) is looked for.
pub(crate) const PRESCAN_BYTES: usize = 1024;

/// A document's text and the encoding it was read in (which its
/// stylesheets fall back to).
pub(crate) fn decode_document(
    mime_type: &str,
    charset: Option<&str>,
    body: &[u8],
) -> (String, &'static Encoding) {
    let (encoding, bom_len) = document_encoding(mime_type, charset, body);
    let (text, _) = encoding.decode_without_bom_handling(&body[bom_len..]);
    (text.into_owned(), encoding)
}

/// HTML's "encoding sniffing algorithm", in order: a BOM, the transport's
/// `charset`, `<meta charset>` in the first 1024 bytes, then a default.
///
/// The default: valid UTF-8 is read as UTF-8, anything else as
/// windows-1252 (the standard's default for most locales, and what keeps
/// the result independent of the machine). Chrome runs an encoding
/// detector here, which says UTF-8 for such pages, and Firefox reads valid
/// UTF-8 `file:`s as UTF-8; without this, an undeclared UTF-8 page would
/// come out garbled. XHTML is XML, whose default is UTF-8.
fn document_encoding(
    mime_type: &str,
    charset: Option<&str>,
    body: &[u8],
) -> (&'static Encoding, usize) {
    if let Some((encoding, len)) = Encoding::for_bom(body) {
        return (encoding, len);
    }
    if let Some(encoding) = charset.and_then(|label| Encoding::for_label(label.as_bytes())) {
        return (encoding, 0);
    }
    if mime_type == "text/html"
        && let Some(encoding) = prescan(&body[..body.len().min(PRESCAN_BYTES)])
    {
        return (encoding, 0);
    }
    if mime_type == "application/xhtml+xml" || std::str::from_utf8(body).is_ok() {
        return (UTF_8, 0);
    }
    (WINDOWS_1252, 0)
}

/// An XML file's text (an SVG image's; Blink's `TextResourceDecoder` for
/// XML): a BOM, the transport's `charset`, the XML declaration's
/// `encoding`, then UTF-8, XML's default.
pub(crate) fn decode_xml(charset: Option<&str>, body: &[u8]) -> String {
    let (encoding, bom_len) = Encoding::for_bom(body)
        .or_else(|| {
            charset
                .and_then(|label| Encoding::for_label(label.as_bytes()))
                .map(|e| (e, 0))
        })
        .or_else(|| xml_declaration_encoding(body).map(|e| (e, 0)))
        .unwrap_or((UTF_8, 0));
    let (text, _) = encoding.decode_without_bom_handling(&body[bom_len..]);
    text.into_owned()
}

/// `<?xml ... encoding="label"?>` at the very start, its label quoted
/// either way.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`at` is where `encoding` starts in the declaration: its end is in it too"
)]
fn xml_declaration_encoding(body: &[u8]) -> Option<&'static Encoding> {
    let rest = body.strip_prefix(b"<?xml")?;
    let declaration = &rest[..find(rest, b"?>")?];
    let at = find(declaration, b"encoding")?;
    let value = declaration[at + b"encoding".len()..]
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let (&quote, value) = value.split_first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let end = value.iter().position(|&b| b == quote)?;
    Encoding::for_label(&value[..end])
}

/// A stylesheet's text (css-syntax "decode bytes"): a BOM, the transport's
/// `charset`, `@charset`, then the document's encoding.
pub(crate) fn decode_stylesheet(
    charset: Option<&str>,
    body: &[u8],
    document_encoding: &'static Encoding,
) -> String {
    let (encoding, bom_len) = Encoding::for_bom(body)
        .or_else(|| {
            charset
                .and_then(|label| Encoding::for_label(label.as_bytes()))
                .map(|e| (e, 0))
        })
        .or_else(|| at_charset(body).map(|e| (e, 0)))
        .unwrap_or((document_encoding, 0));
    let (text, _) = encoding.decode_without_bom_handling(&body[bom_len..]);
    text.into_owned()
}

/// A classic script's text (HTML "fetch a classic script"): a BOM, the
/// transport's `charset`, then the document's encoding.
pub(crate) fn decode_script(
    charset: Option<&str>,
    body: &[u8],
    document_encoding: &'static Encoding,
) -> String {
    let (encoding, bom_len) = Encoding::for_bom(body)
        .or_else(|| {
            charset
                .and_then(|label| Encoding::for_label(label.as_bytes()))
                .map(|e| (e, 0))
        })
        .unwrap_or((document_encoding, 0));
    let (text, _) = encoding.decode_without_bom_handling(&body[bom_len..]);
    text.into_owned()
}

/// `@charset "label";` at the very start, as css-syntax reads it: exactly
/// those bytes, with UTF-16 meaning UTF-8 (the bytes were ASCII after all).
fn at_charset(body: &[u8]) -> Option<&'static Encoding> {
    let rest = body.strip_prefix(b"@charset \"")?;
    let end = rest[..rest.len().min(PRESCAN_BYTES)]
        .windows(2)
        .position(|w| w == b"\";")?;
    let encoding = Encoding::for_label(&rest[..end])?;
    Some(if encoding == UTF_16BE || encoding == UTF_16LE {
        UTF_8
    } else {
        encoding
    })
}

/// HTML's "prescan a byte stream to determine its encoding": looks for a
/// `<meta charset>` or `<meta http-equiv=content-type content=...>`.
fn prescan(bytes: &[u8]) -> Option<&'static Encoding> {
    prescan_metas(bytes, meta_charset)
}

/// Whether a document declares its scripts wasm: a
/// `<meta name="script" content="wasm">` in its first 1024 bytes, found as
/// the encoding's `<meta>` is (the document's engine is chosen before it
/// is parsed). The values ASCII case-insensitively.
pub(crate) fn declares_wasm_scripts(body: &[u8]) -> bool {
    prescan_metas(&body[..body.len().min(PRESCAN_BYTES)], |attributes| {
        let value = |name: &[u8]| {
            attributes
                .iter()
                .find(|(attribute, _)| attribute == name)
                .map(|(_, value)| value.as_slice())
        };
        (value(b"name") == Some(b"script") && value(b"content") == Some(b"wasm")).then_some(())
    })
    .is_some()
}

/// The walk of HTML's prescan, for any `<meta>` looked for before parsing:
/// skips comments and the attributes of other tags, and gives `found` each
/// `<meta>`'s attributes in order (lowercased name and value, as HTML's
/// "get an attribute" reads them), until it returns something.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`i` is a position in `bytes`, moved past what was found in it: at most its length (a slice's, <= isize::MAX) and a few bytes"
)]
pub(crate) fn prescan_metas<T>(
    bytes: &[u8],
    mut found: impl FnMut(&[(Vec<u8>, Vec<u8>)]) -> Option<T>,
) -> Option<T> {
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if rest.starts_with(b"<!--") {
            // The comment ends at the first `-->`, which may share its
            // dashes with the `<!--` (`<!-->`).
            i += 2 + find(&rest[2..], b"-->")? + 3;
        } else if starts_with_ignore_case(rest, b"<meta")
            && rest.get(5).is_some_and(|&b| is_space(b) || b == b'/')
        {
            i += 5;
            let mut attributes = Vec::new();
            while let Some(attribute) = attribute(bytes, &mut i) {
                attributes.push(attribute);
            }
            if let Some(result) = found(&attributes) {
                return Some(result);
            }
        } else if rest.len() >= 2
            && rest[0] == b'<'
            && (rest[1].is_ascii_alphabetic()
                || (rest[1] == b'/' && rest.get(2).is_some_and(u8::is_ascii_alphabetic)))
        {
            // Another tag: skip its name and attributes, so a `<meta` in
            // an attribute value doesn't count.
            i += 1;
            while i < bytes.len() && !is_space(bytes[i]) && bytes[i] != b'>' {
                i += 1;
            }
            while attribute(bytes, &mut i).is_some() {}
        } else if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            i += find(rest, b">")? + 1;
        } else {
            i += 1;
        }
    }
    None
}

/// The encoding one `<meta>` declares, from its attributes.
fn meta_charset(attributes: &[(Vec<u8>, Vec<u8>)]) -> Option<&'static Encoding> {
    let mut seen: Vec<&[u8]> = Vec::new();
    let mut got_pragma = false;
    let mut need_pragma = None;
    let mut charset = None;
    for (name, value) in attributes {
        // Only the first of each name counts.
        if seen.contains(&name.as_slice()) {
            continue;
        }
        match name.as_slice() {
            b"http-equiv" if value.eq_ignore_ascii_case(b"content-type") => got_pragma = true,
            b"content" if charset.is_none() => {
                if let Some(encoding) = charset_from_content(value) {
                    charset = Some(encoding);
                    need_pragma = Some(true);
                }
            }
            b"charset" => {
                charset = Encoding::for_label(value);
                need_pragma = Some(false);
            }
            _ => {}
        }
        seen.push(name);
    }
    match need_pragma {
        None => return None,
        Some(true) if !got_pragma => return None,
        _ => {}
    }
    let encoding = charset?;
    Some(if encoding == UTF_16BE || encoding == UTF_16LE {
        UTF_8
    } else if encoding == X_USER_DEFINED {
        WINDOWS_1252
    } else {
        encoding
    })
}

/// HTML's "get an attribute": `(lowercased name, value)`, or `None` at the
/// end of the tag. Leaves `i` after the attribute.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`i` steps over bytes it has read from `bytes`: at most its length (a slice's, <= isize::MAX) and one"
)]
fn attribute(bytes: &[u8], i: &mut usize) -> Option<(Vec<u8>, Vec<u8>)> {
    while *i < bytes.len() && (is_space(bytes[*i]) || bytes[*i] == b'/') {
        *i += 1;
    }
    if *i >= bytes.len() || bytes[*i] == b'>' {
        return None;
    }
    let mut name = Vec::new();
    loop {
        let &b = bytes.get(*i)?;
        if b == b'=' && !name.is_empty() {
            break;
        }
        if is_space(b) {
            // Spaces may come before the `=`.
            while bytes.get(*i).is_some_and(|&b| is_space(b)) {
                *i += 1;
            }
            if bytes.get(*i) != Some(&b'=') {
                return Some((name, Vec::new()));
            }
            break;
        }
        if b == b'/' || b == b'>' {
            return Some((name, Vec::new()));
        }
        name.push(b.to_ascii_lowercase());
        *i += 1;
    }
    // At the `=`.
    *i += 1;
    while bytes.get(*i).is_some_and(|&b| is_space(b)) {
        *i += 1;
    }
    let mut value = Vec::new();
    match bytes.get(*i) {
        Some(&quote @ (b'"' | b'\'')) => {
            *i += 1;
            loop {
                let &b = bytes.get(*i)?;
                *i += 1;
                if b == quote {
                    return Some((name, value));
                }
                value.push(b.to_ascii_lowercase());
            }
        }
        Some(b'>') => return Some((name, value)),
        None => return None,
        _ => {}
    }
    while let Some(&b) = bytes.get(*i) {
        if is_space(b) || b == b'>' {
            break;
        }
        value.push(b.to_ascii_lowercase());
        *i += 1;
    }
    Some((name, value))
}

/// HTML's "extracting a character encoding from a meta element", for
/// `content="text/html; charset=..."`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`i` steps over bytes of `content` found or read: at most its length (a slice's, <= isize::MAX) and one"
)]
fn charset_from_content(content: &[u8]) -> Option<&'static Encoding> {
    let mut i = 0;
    loop {
        i += find(&content[i..], b"charset")? + b"charset".len();
        while content.get(i).is_some_and(|&b| is_space(b)) {
            i += 1;
        }
        if content.get(i) == Some(&b'=') {
            break;
        }
    }
    i += 1;
    while content.get(i).is_some_and(|&b| is_space(b)) {
        i += 1;
    }
    let value = match content.get(i)? {
        &quote @ (b'"' | b'\'') => {
            let rest = &content[i + 1..];
            &rest[..rest.iter().position(|&b| b == quote)?]
        }
        _ => {
            let rest = &content[i..];
            let end = rest
                .iter()
                .position(|&b| is_space(b) || b == b';')
                .unwrap_or(rest.len());
            &rest[..end]
        }
    };
    Encoding::for_label(value)
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn starts_with_ignore_case(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use encoding_rs::{BIG5, SHIFT_JIS, UTF_16LE, WINDOWS_1252};

    use super::*;

    fn html(body: &[u8]) -> &'static Encoding {
        document_encoding("text/html", None, body).0
    }

    #[test]
    fn bom_beats_the_header_which_beats_meta() {
        let meta = b"<meta charset=big5>";
        assert_eq!(
            document_encoding("text/html", Some("shift_jis"), meta).0,
            SHIFT_JIS
        );
        let with_bom = [&b"\xFF\xFE"[..], meta].concat();
        assert_eq!(
            document_encoding("text/html", Some("shift_jis"), &with_bom),
            (UTF_16LE, 2)
        );
        assert_eq!(html(meta), BIG5);
        // An unknown label in the header is skipped.
        assert_eq!(document_encoding("text/html", Some("bogus"), meta).0, BIG5);
    }

    #[test]
    fn meta_forms() {
        assert_eq!(html(b"<META CHARSET='Big5'>"), BIG5);
        assert_eq!(
            html(b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=shift_jis\">"),
            SHIFT_JIS
        );
        // `content` needs the pragma.
        assert_eq!(
            html(b"<meta content=\"text/html; charset=big5\">\xE9"),
            WINDOWS_1252
        );
        // Only the first `charset` attribute counts.
        assert_eq!(html(b"<meta charset=big5 charset=shift_jis>"), BIG5);
        // UTF-16 in a meta is UTF-8; x-user-defined is windows-1252.
        assert_eq!(html(b"<meta charset=utf-16le>\xE9"), UTF_8);
        assert_eq!(html(b"<meta charset=x-user-defined>"), WINDOWS_1252);
    }

    /// XML: a BOM, the header, the declaration, then UTF-8 (not
    /// windows-1252, even for bytes that aren't UTF-8).
    #[test]
    fn xml_encodings() {
        let declared = "<?xml version=\"1.0\" encoding='Shift_JIS'?><a>\u{8868}</a>";
        let (bytes, _, _) = SHIFT_JIS.encode(declared);
        assert!(decode_xml(None, &bytes).contains('\u{8868}'));
        assert!(!decode_xml(Some("windows-1252"), &bytes).contains('\u{8868}'));
        let with_bom = [&b"\xEF\xBB\xBF"[..], "<a>\u{e9}</a>".as_bytes()].concat();
        assert_eq!(decode_xml(Some("shift_jis"), &with_bom), "<a>\u{e9}</a>");
        assert_eq!(
            decode_xml(None, "<a>\u{e9}</a>".as_bytes()),
            "<a>\u{e9}</a>"
        );
        assert_eq!(decode_xml(None, b"<a>\xE9</a>"), "<a>\u{FFFD}</a>");
    }

    #[test]
    fn a_meta_declares_wasm_scripts() {
        assert!(declares_wasm_scripts(
            b"<!DOCTYPE html><meta name=script content=wasm>"
        ));
        // The values in any case; the attributes in any order, among others.
        assert!(declares_wasm_scripts(
            b"<meta charset=utf-8><META id=x CONTENT='WASM' NAME=\"Script\">"
        ));
        // Another name, another content, one of the two.
        assert!(!declares_wasm_scripts(b"<meta name=scripts content=wasm>"));
        assert!(!declares_wasm_scripts(b"<meta name=script content=js>"));
        assert!(!declares_wasm_scripts(b"<meta name=script>"));
        assert!(!declares_wasm_scripts(b"<meta content=wasm>"));
        assert!(!declares_wasm_scripts(b""));
        // Not in a comment, nor in another tag's attribute.
        assert!(!declares_wasm_scripts(
            b"<!-- <meta name=script content=wasm> -->"
        ));
        assert!(!declares_wasm_scripts(
            b"<p title='<meta name=script content=wasm>'>"
        ));
        // Past the first 1024 bytes it doesn't count.
        let late = [&[b' '; 1024][..], b"<meta name=script content=wasm>"].concat();
        assert!(!declares_wasm_scripts(&late));
        let just = [&[b' '; 993][..], b"<meta name=script content=wasm>"].concat();
        assert_eq!(just.len(), 1024);
        assert!(declares_wasm_scripts(&just));
    }

    #[test]
    fn prescan_skips_comments_and_other_tags() {
        assert_eq!(
            html(b"<!-- <meta charset=big5> --><meta charset=shift_jis>"),
            SHIFT_JIS
        );
        assert_eq!(html(b"<!--><meta charset=big5>"), BIG5);
        assert_eq!(html(b"<p title='<meta charset=big5>'>\xE9"), WINDOWS_1252);
        assert_eq!(
            html(b"<!DOCTYPE html><html><head><meta charset=big5>"),
            BIG5
        );
        // Past the first 1024 bytes it doesn't count.
        let late = [&[b' '; 1024][..], b"<meta charset=big5>\xE9"].concat();
        assert_eq!(html(&late), WINDOWS_1252);
    }

    #[test]
    fn default_is_utf8_when_valid_else_windows_1252() {
        assert_eq!(html("<p>中文".as_bytes()), UTF_8);
        assert_eq!(html(b"<p>caf\xE9"), WINDOWS_1252);
        assert_eq!(
            document_encoding("application/xhtml+xml", None, b"\xE9").0,
            UTF_8
        );
        let (text, encoding) = decode_document("text/plain", None, b"caf\xE9");
        assert_eq!((text.as_str(), encoding), ("café", WINDOWS_1252));
    }

    #[test]
    fn stylesheets() {
        let big5 = "p{content:'中'}";
        let (bytes, _, _) = BIG5.encode(big5);
        assert_eq!(decode_stylesheet(Some("big5"), &bytes, UTF_8), big5);
        let with_rule = [&b"@charset \"big5\";"[..], &bytes].concat();
        assert_eq!(
            decode_stylesheet(None, &with_rule, UTF_8),
            format!("@charset \"big5\";{big5}")
        );
        // Falls back to the document's encoding.
        assert_eq!(decode_stylesheet(None, &bytes, BIG5), big5);
        // `@charset "utf-16"` is read as UTF-8.
        assert_eq!(
            decode_stylesheet(None, "@charset \"utf-16\";é".as_bytes(), WINDOWS_1252),
            "@charset \"utf-16\";é"
        );
        // The BOM wins over everything.
        assert_eq!(
            decode_stylesheet(Some("big5"), b"\xEF\xBB\xBFp{}", BIG5),
            "p{}"
        );
    }
}
