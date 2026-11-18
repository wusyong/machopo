//! A frame's token, as DevTools and WebDriver write it.

use protocol::FrameToken;

/// Its 32 upper-case hex digits, as Chrome's `frameId`; read back, of
/// either case.
#[test]
fn a_token_is_written_as_32_hex_digits() {
    let token = FrameToken([
        0x00, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba,
        0xfe,
    ]);
    let text = token.to_string();
    assert_eq!(text, "000123456789ABCDEF1032547698BAFE");
    assert_eq!(FrameToken::parse(&text), Some(token));
    assert_eq!(FrameToken::parse(&text.to_lowercase()), Some(token));
    assert_eq!(format!("{token:?}"), format!("FrameToken({text})"));
}

/// Anything but 32 hex digits isn't one.
#[test]
fn what_isnt_a_token() {
    for text in [
        "",
        "000123456789ABCDEF1032547698BAF",
        "000123456789ABCDEF1032547698BAFE0",
        // Not a digit, and a sign `from_str_radix` would take.
        "000123456789ABCDEF1032547698BAFG",
        "+00123456789ABCDEF1032547698BAFE",
        // A child frame's id as the renderer wrote it before.
        "0123456789abcdef.f1",
        // 32 bytes, not 32 characters.
        "000123456789ABCDEF1032547698BAé",
    ] {
        assert_eq!(FrameToken::parse(text), None, "{text:?}");
    }
}
