//! WebCrypto's `Crypto` (https://w3c.github.io/webcrypto/#crypto-interface;
//! Blink's `modules/crypto/crypto.cc`): a window's `crypto`, its
//! `getRandomValues()` and `randomUUID()`, from the operating system's
//! random numbers. Not `subtle`. And `structuredClone()` (HTML's), which
//! the window has beside it.

use dom::{Crypto, SerializedMessage, Window};
use elise::Gc;

use crate::typed::{ArrayViewArg, CloneParam};
use crate::{ApiCx, ApiError, DomException, DomStr, ExceptionName};

/// How many bytes `getRandomValues()` fills at most (WebCrypto's limit).
const MAX_BYTES: usize = 65536;

/// `n` random bytes from the operating system (Blink's `crypto::RandBytes`).
fn random_bytes(n: usize) -> Vec<u8> {
    let mut bytes = vec![0; n];
    // Chromium can't go on without them either (BoringSSL aborts).
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    bytes
}

/// `crypto`: the same object each time, kept by the window.
pub(crate) fn window_crypto<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, Crypto>, ApiError> {
    Ok(cx.window_crypto(this))
}

/// `getRandomValues(array)`: fills an integer typed array with random
/// numbers, in place, and gives it back. A float array or a `DataView` is
/// a `TypeMismatchError`, more than 65536 bytes a `QuotaExceededError` (the
/// messages are Chrome's).
pub(crate) fn get_random_values<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Crypto>,
    array: ArrayViewArg,
) -> Result<Vec<u8>, ApiError> {
    if !array.integer {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::TypeMismatch,
            message: format!(
                "Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView is of type '{}', which is not an integer array type.",
                array.name
            ),
        }));
    }
    if array.byte_length > MAX_BYTES {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::QuotaExceeded,
            message: format!(
                "Failed to execute 'getRandomValues' on 'Crypto': The ArrayBufferView's byte length ({}) exceeds the number of bytes of entropy available via this API ({MAX_BYTES}).",
                array.byte_length
            ),
        }));
    }
    Ok(random_bytes(array.byte_length))
}

/// `randomUUID()`: a version 4 UUID (RFC 9562), its hex digits lowercase.
pub(crate) fn random_uuid<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Crypto>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh(new_uuid()))
}

/// A new version 4 UUID (WebCrypto's "generate a random UUID"), which a
/// blob URL's path is too.
pub(crate) fn new_uuid() -> String {
    uuid(&random_bytes(16))
}

/// The version 4 UUID of 16 random bytes: the version (4) and the variant
/// (`10`) set in them.
fn uuid(random: &[u8]) -> String {
    let mut bytes = random.to_vec();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// `structuredClone(value, options)`: the value serialized (with its
/// transfer list) by the front end, its ports shipped (new ones take their
/// places, as a message's), deserialized in the current realm.
pub(crate) fn structured_clone<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Window>,
    value: CloneParam<'_, 'gc>,
) -> Result<(SerializedMessage, Vec<Gc<'gc, dom::EventTargetImpl>>), ApiError> {
    let ports = value
        .ports
        .iter()
        .map(|&port| cx.ship_message_port(port))
        .collect();
    let payload = value
        .payload
        .take()
        .ok_or_else(|| ApiError::type_error("structuredClone: its value is taken"))?;
    Ok((payload, ports))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids() {
        assert_eq!(uuid(&[0; 16]), "00000000-0000-4000-8000-000000000000");
        assert_eq!(uuid(&[0xff; 16]), "ffffffff-ffff-4fff-bfff-ffffffffffff");
        let bytes: Vec<u8> = (0..16).collect();
        assert_eq!(uuid(&bytes), "00010203-0405-4607-8809-0a0b0c0d0e0f");
    }
}
