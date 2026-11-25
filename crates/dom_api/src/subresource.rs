//! A subresource's response as its host hands it to the engine's side
//! (the network service's, filtered for its request's origin; the host's
//! own type is its transport's), and what HTML takes of it for each kind:
//! a classic script's text, a module's, a wasm component's bytes, a
//! sheet's text. Each is none for a failed load (a network error, a CORS
//! failure) or a response of no use, which the DOM notes as failed by
//! what `failed_url` names it.

/// A response that came.
pub struct SubresourceResponse<'a> {
    pub status: u16,
    /// Its MIME type's essence.
    pub mime_type: &'a str,
    /// Its MIME type's `charset`, if it says one.
    pub charset: Option<&'a str>,
    /// Where it came from, after redirects.
    pub url: &'a str,
    /// Another origin's, fetched `no-cors`: script may read nothing of it
    /// (a script's errors muted, a sheet's rules unreadable).
    pub opaque: bool,
    pub body: &'a [u8],
}

/// A response, or a load that failed, named (`Err`) by the URL asked for,
/// else its error's detail.
pub type SubresourceResult<'a> = Result<SubresourceResponse<'a>, &'a str>;

/// What a subresource that failed (or was of no use) is named by, in its
/// document's failed loads (`Cx::note_load_failed`): where its response
/// came from, or what the failure is named by.
#[must_use]
pub fn failed_url(result: &SubresourceResult<'_>) -> String {
    match result {
        Ok(response) => response.url.to_owned(),
        Err(name) => (*name).to_owned(),
    }
}

/// Whether a response of MIME type `essence` is blocked as a script
/// (Blink's `AllowedByNosniff::MimeTypeAsScript`: `image/`, `audio/`,
/// `video/`, `text/csv`).
#[must_use]
pub fn is_blocked_as_script(essence: &str) -> bool {
    ["image/", "audio/", "video/"]
        .iter()
        .any(|prefix| essence.starts_with(prefix))
        || essence == "text/csv"
}

/// A classic script's response as HTML's "fetch a classic script" takes
/// it: none for a failed load, a status that isn't ok, or a MIME type no
/// script is (`is_blocked_as_script`); else its text decoded (a BOM, the
/// response's charset, the `charset` attribute's label, the document's
/// encoding, in that order), its URL after redirects, and whether its
/// errors are muted (another origin's, `no-cors`).
#[must_use]
pub fn script_loaded(
    result: &SubresourceResult<'_>,
    charset: Option<&str>,
    document_encoding: &'static encoding_rs::Encoding,
) -> Option<dom::LoadedScript> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) || is_blocked_as_script(response.mime_type) {
        return None;
    }
    let fallback = charset
        .and_then(|label| encoding_rs::Encoding::for_label(label.trim().as_bytes()))
        .unwrap_or(document_encoding);
    Some(dom::LoadedScript {
        source: crate::encoding::decode_script(response.charset, response.body, fallback),
        url: response.url.to_owned(),
        muted: response.opaque,
    })
}

/// A wasm script's response: none for a failed load, a status that isn't
/// ok, or a MIME type that isn't `application/wasm` (as
/// `WebAssembly.compileStreaming` asks); else its bytes and its URL after
/// redirects.
#[must_use]
pub fn wasm_loaded(result: &SubresourceResult<'_>) -> Option<(Vec<u8>, String)> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) || response.mime_type != "application/wasm" {
        return None;
    }
    Some((response.body.to_vec(), response.url.to_owned()))
}

/// A module's response as HTML's "fetch a single module script" takes it:
/// none for a failed load, a status that isn't ok, or a MIME type that
/// isn't JavaScript's; else its text, UTF-8, and its URL after redirects
/// (its base URL).
#[must_use]
pub fn module_loaded(result: &SubresourceResult<'_>) -> Option<crate::module::FetchedModule> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) || !crate::is_javascript_mime_type(response.mime_type)
    {
        return None;
    }
    let (source, _) = encoding_rs::UTF_8.decode_with_bom_removal(response.body);
    Some(crate::module::FetchedModule {
        url: url::Url::parse(response.url).ok()?,
        source: source.into_owned(),
    })
}

/// A sheet's text, decoded (a BOM, the response's charset, the document's
/// encoding): none for a failed load, an error status, or (but in quirks
/// mode) a type that isn't `text/css`.
#[must_use]
pub fn stylesheet_text(
    result: &SubresourceResult<'_>,
    quirks: bool,
    document_encoding: &'static encoding_rs::Encoding,
) -> Option<String> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) || (response.mime_type != "text/css" && !quirks) {
        return None;
    }
    Some(crate::encoding::decode_stylesheet(
        response.charset,
        response.body,
        document_encoding,
    ))
}
