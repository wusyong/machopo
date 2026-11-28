//! A document's and a frame's sandbox flags (HTML's sandboxing flag set,
//! Chromium's `network::mojom::WebSandboxFlags`), and the `sandbox`
//! attribute's parsing (Chromium's `network::ParseWebSandboxPolicy`). A
//! flag set is a restriction: flags are added, as policies combine, never
//! taken away.

use serde::{Deserialize, Serialize};

/// A set of sandbox flags: each restricts what a document may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxFlags(u32);

impl SandboxFlags {
    /// None: nothing is restricted.
    pub const NONE: Self = Self(0);
    /// Only its own frame and those below it may be navigated (the
    /// sandboxed navigation browsing context flag).
    pub const NAVIGATION: Self = Self(1 << 0);
    pub const PLUGINS: Self = Self(1 << 1);
    /// Its origin is a new opaque one (the sandboxed origin browsing
    /// context flag).
    pub const ORIGIN: Self = Self(1 << 2);
    pub const FORMS: Self = Self(1 << 3);
    pub const SCRIPTS: Self = Self(1 << 4);
    pub const TOP_NAVIGATION: Self = Self(1 << 5);
    pub const POPUPS: Self = Self(1 << 6);
    /// Autofocus, autoplay and the like.
    pub const AUTOMATIC_FEATURES: Self = Self(1 << 7);
    pub const POINTER_LOCK: Self = Self(1 << 8);
    pub const DOCUMENT_DOMAIN: Self = Self(1 << 9);
    pub const ORIENTATION_LOCK: Self = Self(1 << 10);
    /// The flags go to the windows it opens.
    pub const PROPAGATES_TO_AUXILIARY_BROWSING_CONTEXTS: Self = Self(1 << 11);
    pub const MODALS: Self = Self(1 << 12);
    pub const PRESENTATION_CONTROLLER: Self = Self(1 << 13);
    pub const TOP_NAVIGATION_BY_USER_ACTIVATION: Self = Self(1 << 14);
    pub const DOWNLOADS: Self = Self(1 << 15);
    pub const STORAGE_ACCESS_BY_USER_ACTIVATION: Self = Self(1 << 16);
    pub const TOP_NAVIGATION_TO_CUSTOM_PROTOCOLS: Self = Self(1 << 17);
    pub const ALLOW_SAME_SITE_NONE_COOKIES: Self = Self(1 << 18);
    /// Every flag: an iframe's `sandbox` with no token.
    pub const ALL: Self = Self((1 << 19) - 1);

    /// The flags of `bits`, as `bits` gives them back; bits no flag has are
    /// dropped.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits & Self::ALL.0)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether every flag of `flags` is set: whether what they restrict is.
    pub const fn contains(self, flags: Self) -> bool {
        self.0 & flags.0 == flags.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Both sets' flags: the stricter of two policies, combined.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl std::ops::BitOr for SandboxFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

/// The flags a `sandbox` attribute's token lifts (Chromium's
/// `ParseWebSandboxToken`), any case; none for a token that isn't one.
fn lifted(token: &str) -> Option<SandboxFlags> {
    use SandboxFlags as F;
    let flags = match token.to_ascii_lowercase().as_str() {
        "allow-downloads" => F::DOWNLOADS,
        "allow-forms" => F::FORMS,
        "allow-modals" => F::MODALS,
        "allow-orientation-lock" => F::ORIENTATION_LOCK,
        "allow-pointer-lock" => F::POINTER_LOCK,
        "allow-popups" => F::POPUPS | F::TOP_NAVIGATION_TO_CUSTOM_PROTOCOLS,
        "allow-popups-to-escape-sandbox" => F::PROPAGATES_TO_AUXILIARY_BROWSING_CONTEXTS,
        "allow-presentation" => F::PRESENTATION_CONTROLLER,
        "allow-same-origin" => F::ORIGIN,
        "allow-scripts" => F::AUTOMATIC_FEATURES | F::SCRIPTS,
        "allow-storage-access-by-user-activation" => F::STORAGE_ACCESS_BY_USER_ACTIVATION,
        "allow-top-navigation" => F::TOP_NAVIGATION | F::TOP_NAVIGATION_TO_CUSTOM_PROTOCOLS,
        "allow-top-navigation-by-user-activation" => F::TOP_NAVIGATION_BY_USER_ACTIVATION,
        "allow-top-navigation-to-custom-protocols" => F::TOP_NAVIGATION_TO_CUSTOM_PROTOCOLS,
        "allow-same-site-none-cookies" => F::ALLOW_SAME_SITE_NONE_COOKIES,
        _ => return None,
    };
    Some(flags)
}

/// Whether `token` is one a `sandbox` attribute takes (its
/// `DOMTokenList`'s `supports()`).
pub fn is_token(token: &str) -> bool {
    lifted(token).is_some()
}

/// A `sandbox` attribute's value, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSandbox {
    pub flags: SandboxFlags,
    /// Chromium's console error for the tokens that are none, if there
    /// are any: "'x' is an invalid sandbox flag.", "'x', 'y' are invalid
    /// sandbox flags." (each once, in order).
    pub error: Option<String>,
}

/// HTML's "parse a sandboxing directive" of an iframe's `sandbox`
/// attribute `value`, as Chromium's `ParseWebSandboxPolicy` does: every
/// flag, less those its tokens (split at ASCII whitespace, any case) lift.
pub fn parse(value: &str) -> ParsedSandbox {
    let mut flags = SandboxFlags::ALL;
    let mut invalid: Vec<&str> = Vec::new();
    for token in value
        .split([' ', '\n', '\t', '\r', '\x0C'])
        .filter(|token| !token.is_empty())
    {
        match lifted(token) {
            Some(lifted) => flags = flags.without(lifted),
            None if !invalid.contains(&token) => invalid.push(token),
            None => {}
        }
    }
    let error = match invalid.as_slice() {
        [] => None,
        [one] => Some(format!("'{one}' is an invalid sandbox flag.")),
        many => Some(format!(
            "'{}' are invalid sandbox flags.",
            many.join("', '")
        )),
    };
    ParsedSandbox { flags, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SandboxFlags as F;

    /// No token restricts everything; each token lifts its flags, any
    /// case, whitespace between them; `allow-scripts` and `allow-popups`
    /// lift two each.
    #[test]
    fn tokens_lift_their_flags() {
        assert_eq!(parse("").flags, F::ALL);
        assert_eq!(parse(" \t").flags, F::ALL);
        let flags = parse("Allow-Scripts\nallow-same-origin").flags;
        assert!(!flags.contains(F::SCRIPTS) && !flags.contains(F::AUTOMATIC_FEATURES));
        assert!(!flags.contains(F::ORIGIN));
        assert!(flags.contains(F::POPUPS) && flags.contains(F::NAVIGATION));
        let flags = parse("allow-popups").flags;
        assert!(!flags.contains(F::POPUPS));
        assert!(!flags.contains(F::TOP_NAVIGATION_TO_CUSTOM_PROTOCOLS));
        assert!(flags.contains(F::TOP_NAVIGATION));
        assert!(
            !parse("allow-top-navigation")
                .flags
                .contains(F::TOP_NAVIGATION)
        );
    }

    /// Every token of the table lifts something, none lifts the
    /// navigation flag (only no attribute does).
    #[test]
    fn every_token_is_one() {
        for token in [
            "allow-downloads",
            "allow-forms",
            "allow-modals",
            "allow-orientation-lock",
            "allow-pointer-lock",
            "allow-popups",
            "allow-popups-to-escape-sandbox",
            "allow-presentation",
            "allow-same-origin",
            "allow-scripts",
            "allow-storage-access-by-user-activation",
            "allow-top-navigation",
            "allow-top-navigation-by-user-activation",
            "allow-top-navigation-to-custom-protocols",
            "allow-same-site-none-cookies",
        ] {
            assert!(is_token(token), "{token}");
            let flags = parse(token).flags;
            assert_ne!(flags, F::ALL, "{token}");
            assert!(flags.contains(F::NAVIGATION), "{token}");
        }
        assert!(!is_token("allow-plugins"));
    }

    /// The tokens that are none are told once each, in order, as
    /// Chromium's message has them; they lift nothing.
    #[test]
    fn invalid_tokens_are_told() {
        assert_eq!(parse("allow-scripts").error, None);
        assert_eq!(
            parse("bogus allow-scripts").error.as_deref(),
            Some("'bogus' is an invalid sandbox flag.")
        );
        let parsed = parse("b a b allow-forms");
        assert_eq!(
            parsed.error.as_deref(),
            Some("'b', 'a' are invalid sandbox flags.")
        );
        assert_eq!(parsed.flags, F::ALL.without(F::FORMS));
    }

    #[test]
    fn flags_combine_as_the_stricter() {
        let a = parse("allow-scripts allow-same-origin").flags;
        let b = parse("allow-scripts").flags;
        let both = a | b;
        assert!(both.contains(F::ORIGIN) && !both.contains(F::SCRIPTS));
        assert_eq!(F::from_bits(both.bits()), both);
        assert_eq!(F::from_bits(u32::MAX), F::ALL);
        assert!(F::NONE.is_empty() && !F::ALL.is_empty());
    }
}
