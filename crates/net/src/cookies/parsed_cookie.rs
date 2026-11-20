//! A `Set-Cookie` line taken apart (Chromium's `net::ParsedCookie`): its
//! name and value, and its attributes, without judging them against a URL.

use super::cookie_constants::CookieSameSite;
use super::cookie_inclusion_status::ExclusionReason;

/// The most a cookie's name and value can be together, in bytes
/// (`ParsedCookie::kMaxCookieNamePlusValueSize`).
pub const MAX_COOKIE_NAME_PLUS_VALUE_SIZE: usize = 4096;

/// The most an attribute's value can be, in bytes: a longer one is ignored,
/// as if it weren't there (`ParsedCookie::kMaxCookieAttributeValueSize`).
pub const MAX_COOKIE_ATTRIBUTE_VALUE_SIZE: usize = 1024;

/// What is trimmed around names and values.
const WHITESPACE: [char; 2] = [' ', '\t'];

/// A `Set-Cookie` line, parsed as Chromium does (`ParsedCookie`, which is
/// RFC 6265bis's "parse a cookie" with Chromium's character checks): the
/// first `name=value` pair, and each `;`-separated attribute after it, its
/// name lowercased. A first pair without a `=` is a value without a name
/// (`Set-Cookie: abc` is the nameless cookie `abc`). Of each attribute
/// machopo knows (`Domain`, `Path`, `Expires`, `Max-Age`, `Secure`,
/// `HttpOnly`, `SameSite`, `Partitioned`) the last one counts; others are
/// kept, unused. `Priority`, Chromium's own, is one of those: it only
/// matters for eviction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCookie {
    name: String,
    value: String,
    /// The attributes after the first pair, in order, names lowercased.
    attributes: Vec<(String, String)>,
    // Indices into `attributes` of the last of each machopo knows.
    path: Option<usize>,
    domain: Option<usize>,
    expires: Option<usize>,
    max_age: Option<usize>,
    secure: bool,
    http_only: bool,
    same_site: Option<usize>,
    partitioned: bool,
}

/// HTTP's CTL: `%x00-1F` and `%x7F` (`HttpUtil::IsControlChar`), the tab
/// among them.
fn is_control(byte: u8) -> bool {
    byte <= 0x1f || byte == 0x7f
}

/// Whether `name` can be a cookie's name or an attribute's
/// (`ParsedCookie::IsValidCookieName`): no CTL, `;` or `=`. Wider than RFC
/// 6265bis's `cookie-name` grammar (any other byte, non-ASCII too), as
/// Chromium is.
fn is_valid_cookie_name(name: &str) -> bool {
    !name
        .bytes()
        .any(|b| is_control(b) || b == b';' || b == b'=')
}

/// Whether `value` can be a cookie's value or an attribute's
/// (`ParsedCookie::IsValidCookieValue`): no CTL or `;`. RFC 6265bis lets a
/// tab through; Chromium doesn't.
fn is_valid_cookie_value(value: &str) -> bool {
    !value.bytes().any(|b| is_control(b) || b == b';')
}

/// `ParsedCookie::IsValidCookieNameValuePair`: not both empty, not too
/// long together, and of characters a name and a value can have.
pub(super) fn check_name_value_pair(name: &str, value: &str) -> Result<(), ExclusionReason> {
    if name.is_empty() && value.is_empty() {
        return Err(ExclusionReason::NoCookieContent);
    }
    if name
        .len()
        .checked_add(value.len())
        .is_none_or(|size| size > MAX_COOKIE_NAME_PLUS_VALUE_SIZE)
    {
        return Err(ExclusionReason::NameValuePairExceedsMaxSize);
    }
    if !is_valid_cookie_name(name) || !is_valid_cookie_value(value) {
        return Err(ExclusionReason::DisallowedCharacter);
    }
    Ok(())
}

impl ParsedCookie {
    /// Parses `cookie_line`, a `Set-Cookie` header's value or what script
    /// gives `document.cookie` (`ParsedCookie::ParsedCookie`), or says why
    /// it is no cookie at all.
    ///
    /// Chromium's rules, where they differ from RFC 6265bis's: a `\n`,
    /// `\r` or NUL anywhere refuses the line, as does a control character
    /// (a tab too) in the name, the value or an attribute, or a `;` or `=`
    /// in an attribute's name. An attribute's value longer than
    /// [`MAX_COOKIE_ATTRIBUTE_VALUE_SIZE`] is dropped, as if it weren't
    /// there; a name and value longer than
    /// [`MAX_COOKIE_NAME_PLUS_VALUE_SIZE`] together refuse the line.
    pub fn parse(cookie_line: &str) -> Result<Self, ExclusionReason> {
        // Chromium once cut the line at the first of these; it now refuses
        // it, as a cut line would be another cookie than the one sent.
        if cookie_line.bytes().any(|b| matches!(b, b'\n' | b'\r' | 0)) {
            return Err(ExclusionReason::DisallowedCharacter);
        }
        if cookie_line.is_empty() {
            return Err(ExclusionReason::NoCookieContent);
        }

        let mut first: Option<(String, String)> = None;
        let mut attributes = Vec::new();
        // What is left to parse, at the start of a pair.
        let mut rest = cookie_line;
        while !rest.is_empty() {
            let is_first = first.is_none();
            // `ParseToken`: the pair's name, up to a `=` or a `;`.
            let token_start = rest.trim_start_matches(WHITESPACE);
            if token_start.is_empty() && !is_first {
                break;
            }
            let separator = token_start.find([';', '=']).unwrap_or(token_start.len());
            let (token, after_token) = token_start.split_at(separator);
            let token = token.trim_end_matches(WHITESPACE);
            let (name, value_start) = match after_token.strip_prefix('=') {
                Some(after_equals) => (token, after_equals),
                // A first pair without a `=` is all value, with no name
                // (Mozilla's bug 169091; other browsers agree).
                None if is_first => ("", token_start),
                // A later one is all name: `Secure`, `HttpOnly`.
                None => (token, after_token),
            };
            // `ParseValue`: up to the next `;` (a `=` is part of it).
            let value_start = value_start.trim_start_matches(WHITESPACE);
            let value_end = value_start.find(';').unwrap_or(value_start.len());
            let (value, after_value) = value_start.split_at(value_end);
            let value = value.trim_end_matches(WHITESPACE);
            rest = after_value.strip_prefix(';').unwrap_or(after_value);

            if is_first {
                check_name_value_pair(name, value)?;
                first = Some((name.to_owned(), value.to_owned()));
                continue;
            }
            // Attribute names are case-insensitive (RFC 2109 on).
            let name = name.to_ascii_lowercase();
            if !is_valid_cookie_name(&name) || !is_valid_cookie_value(value) {
                return Err(ExclusionReason::DisallowedCharacter);
            }
            if value.len() > MAX_COOKIE_ATTRIBUTE_VALUE_SIZE {
                continue;
            }
            attributes.push((name, value.to_owned()));
        }
        // A line of only whitespace has a first pair, empty: refused above.
        let (name, value) = first.ok_or(ExclusionReason::NoCookieContent)?;

        let mut cookie = Self {
            name,
            value,
            attributes,
            path: None,
            domain: None,
            expires: None,
            max_age: None,
            secure: false,
            http_only: false,
            same_site: None,
            partitioned: false,
        };
        // `SetupAttributes`: the last of each wins.
        for (index, (name, _)) in cookie.attributes.iter().enumerate() {
            match name.as_str() {
                "path" => cookie.path = Some(index),
                "domain" => cookie.domain = Some(index),
                "expires" => cookie.expires = Some(index),
                "max-age" => cookie.max_age = Some(index),
                "secure" => cookie.secure = true,
                "httponly" => cookie.http_only = true,
                "samesite" => cookie.same_site = Some(index),
                "partitioned" => cookie.partitioned = true,
                _ => {}
            }
        }
        Ok(cookie)
    }

    /// The name: empty for a nameless cookie.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn attribute(&self, index: Option<usize>) -> Option<&str> {
        self.attributes.get(index?).map(|(_, value)| value.as_str())
    }

    /// The last `Path` attribute's value (maybe empty), if there is one.
    pub fn path(&self) -> Option<&str> {
        self.attribute(self.path)
    }

    /// The last `Domain` attribute's value, if there is one: `Domain=`
    /// gives the empty string, which makes a host-only cookie.
    pub fn domain(&self) -> Option<&str> {
        self.attribute(self.domain)
    }

    /// The last `Expires` attribute's value, unparsed.
    pub fn expires(&self) -> Option<&str> {
        self.attribute(self.expires)
    }

    /// The last `Max-Age` attribute's value, unparsed.
    pub fn max_age(&self) -> Option<&str> {
        self.attribute(self.max_age)
    }

    /// Whether it has `Secure` (whatever its value: `Secure=no` too).
    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn is_http_only(&self) -> bool {
        self.http_only
    }

    /// The last `SameSite` attribute's mode: unspecified without one, or
    /// with a value other than `Strict`, `Lax` or `None`.
    pub fn same_site(&self) -> CookieSameSite {
        self.attribute(self.same_site).map_or(
            CookieSameSite::Unspecified,
            CookieSameSite::from_attribute_value,
        )
    }

    pub fn is_partitioned(&self) -> bool {
        self.partitioned
    }

    /// The attributes after the name and value, in order, their names
    /// lowercased (`ForEachAttribute`): unknown ones too, and the ones a
    /// later one of the same name overrides.
    pub fn attributes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.attributes
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}
