//! MIME types: parsing `Content-Type`, and guessing when there is none.

/// `text/HTML; Charset="UTF-8"` -> `("text/html", Some("UTF-8"))`. `None`
/// if there is no `type/subtype` essence.
pub(crate) fn parse_content_type(value: &str) -> Option<(String, Option<String>)> {
    let mut parts = value.split(';');
    let essence = parts.next()?.trim().to_ascii_lowercase();
    let (kind, subtype) = essence.split_once('/')?;
    if kind.is_empty() || subtype.is_empty() || essence.contains(char::is_whitespace) {
        return None;
    }
    let charset = parts.find_map(|param| {
        let (name, value) = param.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("charset") {
            return None;
        }
        let value = value.trim().trim_matches('"').trim();
        (!value.is_empty()).then(|| value.to_owned())
    });
    Some((essence, charset))
}

/// A server sent no usable `Content-Type`. Chromium sniffs the body
/// (`net::SniffMimeType`); we only tell markup from text.
pub(crate) fn sniff(body: &[u8]) -> &'static str {
    let start = body.trim_ascii_start();
    if start.starts_with(b"<") {
        "text/html"
    } else {
        "text/plain"
    }
}

/// By file extension, as `net::GetMimeTypeFromFile` does.
pub(crate) fn for_extension(extension: Option<&str>) -> &'static str {
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html",
        Some("xht" | "xhtml") => "application/xhtml+xml",
        Some("css") => "text/css",
        Some("js" | "mjs") => "text/javascript",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        _ => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type() {
        assert_eq!(
            parse_content_type("text/HTML; Charset=\"UTF-8\""),
            Some(("text/html".into(), Some("UTF-8".into())))
        );
        assert_eq!(
            parse_content_type("text/css;foo=bar; charset=windows-1252"),
            Some(("text/css".into(), Some("windows-1252".into())))
        );
        assert_eq!(
            parse_content_type("image/png"),
            Some(("image/png".into(), None))
        );
        assert_eq!(parse_content_type("html"), None);
        assert_eq!(parse_content_type(""), None);
        assert_eq!(parse_content_type("text /html"), None);
    }

    #[test]
    fn sniffing_and_extensions() {
        assert_eq!(sniff(b"  \n<!DOCTYPE html>"), "text/html");
        assert_eq!(sniff(b"hello"), "text/plain");
        assert_eq!(for_extension(Some("HTM")), "text/html");
        assert_eq!(for_extension(Some("css")), "text/css");
        assert_eq!(for_extension(Some("wasm")), "application/wasm");
        assert_eq!(for_extension(None), "text/plain");
    }
}
