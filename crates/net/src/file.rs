//! `file:` URLs (`FileURLLoader`), and converting between them and paths.

use std::path::{Path, PathBuf};

use protocol::network::{LoadResult, NetError, NetErrorCode, ResourceResponse, ResponseTainting};
use url::Url;

use crate::mime;

pub(crate) async fn load(url: Url) -> LoadResult {
    let Some(path) = file_path(url.as_str()) else {
        return Err(NetError::new(NetErrorCode::InvalidUrl, url.as_str()));
    };
    let read = {
        let path = path.clone();
        tokio::task::spawn_blocking(move || match path.is_dir() {
            true => directory_listing(&path).map(|page| (page, true)),
            false => std::fs::read(path).map(|body| (body, false)),
        })
        .await
    };
    let (body, is_dir) = match read {
        Ok(Ok(read)) => read,
        Ok(Err(err)) => {
            let code = match err.kind() {
                std::io::ErrorKind::PermissionDenied => NetErrorCode::AccessDenied,
                _ => NetErrorCode::FileNotFound,
            };
            return Err(NetError::new(code, format!("{}: {err}", path.display())));
        }
        Err(join) => return Err(NetError::new(NetErrorCode::Failed, join.to_string())),
    };
    Ok(ResourceResponse {
        url: url.into(),
        status: 200,
        status_text: String::new(),
        redirected: false,
        tainting: ResponseTainting::Basic,
        mime_type: match is_dir {
            true => "text/html",
            false => mime::for_extension(path.extension().and_then(|e| e.to_str())),
        }
        .to_owned(),
        charset: None,
        headers: Vec::new(),
        // Timed by whoever loaded it.
        timing: None,
        remote_address: None,
        protocol: "file".to_owned(),
        encoded_headers_length: 0,
        encoded_body_length: 0,
        body: body.into(),
    })
}

/// A page listing the directory at `path` (Chrome's `FileURLDirectoryLoader`,
/// without its script and sorting): its parent and its entries, each a
/// link by its absolute URL (the directory's URL may lack its trailing
/// slash), directories first, by name.
fn directory_listing(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut entries: Vec<(bool, String, PathBuf)> = std::fs::read_dir(path)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((!is_dir, name, entry.path()))
        })
        .collect();
    entries.sort();
    let title = escape(&path.to_string_lossy());
    let mut page = format!(
        "<!DOCTYPE html>\n<meta charset=\"utf-8\">\n<title>Index of {title}</title>\n\
         <h1>Index of {title}</h1>\n<ul>\n"
    );
    let mut item = |href: Option<String>, text: &str| {
        if let Some(href) = href {
            page.push_str(&format!(
                "<li><a href=\"{}\">{}</a>\n",
                escape(&href),
                escape(text)
            ));
        }
    };
    item(path.parent().and_then(file_url), "[parent directory]");
    for (is_file, name, entry) in &entries {
        let text = if *is_file {
            name.clone()
        } else {
            format!("{name}/")
        };
        item(file_url(entry), &text);
    }
    page.push_str("</ul>\n");
    Ok(page.into_bytes())
}

/// `text` as HTML text or an attribute's value.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `file:///C:/a%20b/c.html` -> `C:\a b\c.html` (or `/a b/c.html`). The
/// query and fragment are not part of the path.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    // Only local files: `file:///...` (empty host) or `file://localhost/...`.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(path)?;
    // `/C:/...` on Windows.
    let bytes = decoded.as_bytes();
    let path = if bytes.len() >= 3 && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() {
        &decoded[1..]
    } else {
        &decoded[..]
    };
    Some(PathBuf::from(path))
}

/// An absolute path as a `file://` URL. `None` for relative paths.
pub fn file_url(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let text = path.to_str()?;
    // `canonicalize` on Windows returns verbatim paths (`\\?\C:\...`).
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(text)
        .replace('\\', "/");
    let text = if text.starts_with('/') {
        text
    } else {
        format!("/{text}")
    };
    let mut url = String::from("file://");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(url)
}

fn percent_decode(text: &str) -> Option<String> {
    let mut rest = text.as_bytes();
    let mut out = Vec::with_capacity(rest.len());
    while let Some((&first, after)) = rest.split_first() {
        if first == b'%' {
            // Two hex digits, or the whole text doesn't decode.
            let (hex, after) = after.split_first_chunk::<2>()?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            rest = after;
        } else {
            out.push(first);
            rest = after;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_round_trip() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\a b\ü.html")
        } else {
            PathBuf::from("/a b/ü.html")
        };
        let url = file_url(&path).unwrap();
        if cfg!(windows) {
            assert_eq!(url, "file:///C:/a%20b/%C3%BC.html");
        } else {
            assert_eq!(url, "file:///a%20b/%C3%BC.html");
        }
        assert_eq!(file_path(&url).unwrap(), path);
        assert_eq!(file_path(&format!("{url}#frag")).unwrap(), path);
        assert_eq!(file_path(&format!("{url}?q=1")).unwrap(), path);
        assert_eq!(file_url(Path::new("relative.html")), None);
    }
}
