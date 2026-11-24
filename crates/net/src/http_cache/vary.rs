//! Which requests a stored response answers, by its `Vary` (Chromium's
//! `HttpVaryData`): the values the request that got it had for the headers
//! `Vary` names, kept with it; a later request matches when it has the same
//! ones.
//!
//! Chromium keeps a SHA-256 of the values, not the values: its entries go to
//! disk, and a `Vary: Cookie` would put the cookie there. These are kept in
//! memory only, beside the cookies themselves, so the values are kept as
//! they are; one written to disk has to be hashed as Chromium's is.

use super::headers::{has_value, values};

/// What a stored response's `Vary` asks of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaryData {
    /// `Vary: *`, or a `Vary` naming `Authorization`, which the cache can't
    /// see: no request matches, so it is always validated
    /// (`HandleAsVaryStar`).
    Star,
    /// Each named header's value in the request it was stored for (none is
    /// ""), each followed by `\n` (`AddField`).
    Values(String),
}

impl VaryData {
    /// The `Vary` of a response to a request of `request` headers; `None`
    /// if it has none (any request matches it).
    pub(crate) fn new(request: &[(String, String)], response: &[(String, String)]) -> Option<Self> {
        if is_star(response) {
            return Some(Self::Star);
        }
        let mut named = values(response, "vary").peekable();
        named.peek()?;
        let mut joined = String::new();
        for name in named {
            joined.push_str(request_header(request, name).unwrap_or(""));
            joined.push('\n');
        }
        Some(Self::Values(joined))
    }

    /// Whether a request of `request` headers gets the response stored with
    /// this, whose headers are `response` (`MatchesRequest`).
    pub(crate) fn matches(
        &self,
        request: &[(String, String)],
        response: &[(String, String)],
    ) -> bool {
        match self {
            Self::Star => false,
            Self::Values(_) => Self::new(request, response).as_ref() == Some(self),
        }
    }
}

fn is_star(response: &[(String, String)]) -> bool {
    has_value(response, "vary", "*") || has_value(response, "vary", "authorization")
}

/// A request header's value, its name matched ignoring ASCII case
/// (`HttpRequestHeaders::GetHeader`).
pub(crate) fn request_header<'a>(request: &'a [(String, String)], name: &str) -> Option<&'a str> {
    request
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(lines: &[(&str, &str)]) -> Vec<(String, String)> {
        lines
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn no_vary_matches_any_request() {
        assert_eq!(VaryData::new(&headers(&[("accept", "a")]), &[]), None);
    }

    #[test]
    fn the_named_headers_values_have_to_be_the_same() {
        let response = headers(&[("vary", "Accept-Language, X-Mode"), ("vary", "accept")]);
        let request = headers(&[("Accept-Language", "en"), ("accept", "*/*")]);
        let vary = VaryData::new(&request, &response).expect("a vary");
        assert_eq!(vary, VaryData::Values("en\n\n*/*\n".to_owned()));
        assert!(vary.matches(
            &headers(&[("accept", "*/*"), ("ACCEPT-LANGUAGE", "en")]),
            &response
        ));
        // Values as they are: not trimmed, not lowercased.
        assert!(!vary.matches(
            &headers(&[("accept-language", "EN"), ("accept", "*/*")]),
            &response
        ));
        // A header there now that wasn't then.
        assert!(!vary.matches(
            &headers(&[
                ("accept-language", "en"),
                ("x-mode", "1"),
                ("accept", "*/*")
            ]),
            &response
        ));
    }

    #[test]
    fn star_and_authorization_match_nothing() {
        for vary in ["*", "Accept, *", "Authorization"] {
            let response = headers(&[("vary", vary)]);
            let data = VaryData::new(&[], &response).expect("a vary");
            assert_eq!(data, VaryData::Star, "{vary}");
            assert!(!data.matches(&[], &response), "{vary}");
        }
    }
}
