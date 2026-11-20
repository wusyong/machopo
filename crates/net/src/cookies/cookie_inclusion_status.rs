//! Why a cookie wasn't set, or wasn't sent (Chromium's
//! `CookieInclusionStatus`, its exclusion reasons; no warnings).

use std::fmt;

/// One reason a cookie was refused (`CookieInclusionStatus::ExclusionReason`,
/// the ones machopo has).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExclusionReason {
    /// The URL can't have cookies: no host, say (`EXCLUDE_FAILURE_TO_STORE`).
    FailureToStore,
    /// Neither a name nor a value (`EXCLUDE_NO_COOKIE_CONTENT`).
    NoCookieContent,
    /// A control character (a tab too), or a `;` or `=` where it can't be
    /// (`EXCLUDE_DISALLOWED_CHARACTER`).
    DisallowedCharacter,
    /// The name and value are longer than 4096 bytes together
    /// (`EXCLUDE_NAME_VALUE_PAIR_EXCEEDS_MAX_SIZE`).
    NameValuePairExceedsMaxSize,
    /// No name, and a value with a `=`, which a `Cookie` header would read
    /// as a name and value (`EXCLUDE_AMBIGUOUS_SERIALIZATION`).
    AmbiguousSerialization,
    /// The `Domain` attribute isn't one the URL can set a cookie on
    /// (`EXCLUDE_INVALID_DOMAIN`).
    InvalidDomain,
    /// The `Domain` attribute isn't of the URL's site, or the cookie's
    /// domain doesn't domain-match the URL's host
    /// (`EXCLUDE_DOMAIN_MISMATCH`).
    DomainMismatch,
    /// The URL's path isn't on the cookie's (`EXCLUDE_NOT_ON_PATH`).
    NotOnPath,
    /// A `__Secure-`, `__Host-`, `__Http-` or `__Host-Http-` name without
    /// what the prefix asks for, or a nameless cookie whose value starts
    /// with one (`EXCLUDE_INVALID_PREFIX`).
    InvalidPrefix,
    /// `Partitioned` without `Secure`, or from an insecure URL
    /// (`EXCLUDE_INVALID_PARTITIONED`).
    InvalidPartitioned,
    /// Not an `http:`, `https:`, `ws:` or `wss:` URL
    /// (`EXCLUDE_NONCOOKIEABLE_SCHEME`).
    NoncookieableScheme,
    /// A `Secure` cookie, and an insecure URL (`EXCLUDE_SECURE_ONLY`).
    SecureOnly,
    /// An `HttpOnly` cookie, and script (`EXCLUDE_HTTP_ONLY`).
    HttpOnly,
    /// A `SameSite=Strict` cookie, and a context that isn't strictly
    /// same-site (laxly, to set one) (`EXCLUDE_SAMESITE_STRICT`).
    SameSiteStrict,
    /// A `SameSite=Lax` cookie, and a cross-site context
    /// (`EXCLUDE_SAMESITE_LAX`).
    SameSiteLax,
    /// A cookie without `SameSite`, treated as `Lax`, and a cross-site
    /// context (`EXCLUDE_SAMESITE_UNSPECIFIED_TREATED_AS_LAX`).
    SameSiteUnspecifiedTreatedAsLax,
    /// `SameSite=None` without `Secure` (`EXCLUDE_SAMESITE_NONE_INSECURE`).
    SameSiteNoneInsecure,
    /// An insecure URL setting a cookie that would overwrite or shadow a
    /// `Secure` one ("leave secure cookies alone",
    /// `EXCLUDE_OVERWRITE_SECURE`).
    OverwriteSecure,
    /// Script setting a cookie that would overwrite an `HttpOnly` one
    /// (`EXCLUDE_OVERWRITE_HTTP_ONLY`).
    OverwriteHttpOnly,
}

/// The reasons a cookie was refused, each once, in the order they were
/// found (`CookieInclusionStatus`); none: it was let through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieInclusionStatus {
    exclusions: Vec<ExclusionReason>,
}

impl CookieInclusionStatus {
    /// A status of one reason.
    pub fn excluded(reason: ExclusionReason) -> Self {
        Self {
            exclusions: vec![reason],
        }
    }

    /// Whether the cookie was let through (`IsInclude`): no reason.
    pub fn is_include(&self) -> bool {
        self.exclusions.is_empty()
    }

    pub fn has_exclusion_reason(&self, reason: ExclusionReason) -> bool {
        self.exclusions.contains(&reason)
    }

    /// The reasons, as they were found.
    pub fn exclusion_reasons(&self) -> &[ExclusionReason] {
        &self.exclusions
    }

    pub(crate) fn add_exclusion_reason(&mut self, reason: ExclusionReason) {
        if !self.has_exclusion_reason(reason) {
            self.exclusions.push(reason);
        }
    }

    /// `Ok` if it lets the cookie through, else itself.
    pub(crate) fn into_result(self) -> Result<(), Self> {
        if self.is_include() { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for CookieInclusionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_include() {
            return f.write_str("included");
        }
        write!(f, "excluded: {:?}", self.exclusions)
    }
}
