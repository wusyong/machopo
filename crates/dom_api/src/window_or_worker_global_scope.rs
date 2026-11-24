//! HTML's `btoa()` and `atob()` (https://html.spec.whatwg.org/#atob; Blink's
//! `WindowOrWorkerGlobalScope`): a window's and a worker's global's, the
//! same for both, of nothing but their argument.

use base64::Engine;
use dom::Window;
use elise::Gc;

use crate::{ApiCx, ApiError, DomStr, ExceptionName};

/// `btoa(data)`: `data`'s code points as bytes, base64 encoded; an
/// `InvalidCharacterError` for one past U+00FF (a lone surrogate, which
/// comes as U+FFFD, too).
pub(crate) fn btoa<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Window>,
    data: &str,
) -> Result<DomStr<'gc>, ApiError> {
    encode(data)
}

/// A worker's global's `btoa`, as a window's.
pub(crate) fn worker_btoa<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, dom::WorkerGlobalScope>,
    data: &str,
) -> Result<DomStr<'gc>, ApiError> {
    encode(data)
}

/// `atob(data)`: `data` forgiving-base64 decoded (Infra's: ASCII
/// whitespace dropped, the padding optional), each byte a code point; an
/// `InvalidCharacterError` for what doesn't decode.
pub(crate) fn atob<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Window>,
    data: &str,
) -> Result<DomStr<'gc>, ApiError> {
    decode(data)
}

/// A worker's global's `atob`, as a window's.
pub(crate) fn worker_atob<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, dom::WorkerGlobalScope>,
    data: &str,
) -> Result<DomStr<'gc>, ApiError> {
    decode(data)
}

fn encode(data: &str) -> Result<DomStr<'static>, ApiError> {
    let bytes = data
        .chars()
        .map(|c| u8::try_from(u32::from(c)))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| ApiError::dom(ExceptionName::InvalidCharacter))?;
    Ok(DomStr::fresh(
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

fn decode(data: &str) -> Result<DomStr<'static>, ApiError> {
    // Not ASCII is no base64 symbol: none of its UTF-8 bytes is one either.
    let bytes = data_url::forgiving_base64::decode_to_vec(data.as_bytes())
        .map_err(|_| ApiError::dom(ExceptionName::InvalidCharacter))?;
    Ok(DomStr::fresh(
        bytes.into_iter().map(char::from).collect::<String>(),
    ))
}
