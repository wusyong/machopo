//! Host mapping rules (Chromium's `net::HostMappingRules`, as
//! `--host-resolver-rules` gives them to `MappedHostResolver`): which host
//! names resolve to a fixed address, or not at all, instead of asking the
//! system. Web tests use them to send WPT's domains to the local wptserve
//! without touching the machine's hosts file.

use std::net::IpAddr;

/// What a rule does with the host names it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMapping {
    /// Resolves to this address.
    Address(IpAddr),
    /// Fails to resolve (Chromium's `^NOTFOUND`).
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    /// Lowercase; `*` matches any run of characters, dots included, and
    /// `?` any one character (`base::MatchPattern`).
    pattern: String,
    mapping: HostMapping,
}

/// Rules in order; the first whose pattern matches a host decides it.
/// Empty by default: every host goes to the system's resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostMappingRules {
    rules: Vec<Rule>,
}

impl HostMappingRules {
    /// Parses Chromium's syntax: rules separated by commas, each
    /// `MAP <pattern> <replacement>`, where the replacement is an IP
    /// address (IPv6 with or without brackets) or `^NOTFOUND`. Chromium's
    /// other forms (`EXCLUDE`, a host name or a port as the replacement)
    /// are errors: nothing needs them yet.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rules = Vec::new();
        for rule in text.split(',') {
            let parts: Vec<&str> = rule.split_whitespace().collect();
            let [verb, pattern, replacement] = parts[..] else {
                if parts.is_empty() {
                    continue;
                }
                return Err(format!("expected `MAP <pattern> <replacement>`: {rule:?}"));
            };
            if !verb.eq_ignore_ascii_case("MAP") {
                return Err(format!("only MAP rules are supported: {rule:?}"));
            }
            let mapping = if replacement == "^NOTFOUND" {
                HostMapping::NotFound
            } else {
                let address = replacement
                    .strip_prefix('[')
                    .and_then(|a| a.strip_suffix(']'))
                    .unwrap_or(replacement);
                HostMapping::Address(address.parse().map_err(|_| {
                    format!("the replacement must be an IP address or ^NOTFOUND: {rule:?}")
                })?)
            };
            rules.push(Rule {
                pattern: pattern.to_ascii_lowercase(),
                mapping,
            });
        }
        Ok(Self { rules })
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// What the first matching rule does with `host`; `None` if no rule
    /// matches.
    pub fn map(&self, host: &str) -> Option<HostMapping> {
        let host = host.to_ascii_lowercase();
        self.rules
            .iter()
            .find(|rule| matches(rule.pattern.as_bytes(), host.as_bytes()))
            .map(|rule| rule.mapping)
    }
}

/// `base::MatchPattern`'s glob: `*` any run, `?` any one character.
fn matches(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| matches(rest, &text[skip..])),
        Some((&c, rest)) => match text.split_first() {
            Some((&t, text)) if c == b'?' || c == t => matches(rest, text),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: HostMapping = HostMapping::Address(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    #[test]
    fn the_first_matching_rule_decides() {
        let rules =
            HostMappingRules::parse("MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1")
                .unwrap();
        assert_eq!(rules.map("web-platform.test"), Some(LOCAL));
        // `*` crosses dots.
        assert_eq!(rules.map("www.www1.web-platform.test"), Some(LOCAL));
        assert_eq!(rules.map("xn--lve-6lad.web-platform.test"), Some(LOCAL));
        assert_eq!(
            rules.map("nonexistent.web-platform.test"),
            Some(HostMapping::NotFound)
        );
        assert_eq!(
            rules.map("nonexistent.nonexistent.not-web-platform.test"),
            Some(HostMapping::NotFound)
        );
        // Case doesn't matter, on either side.
        assert_eq!(rules.map("WWW.Web-Platform.TEST"), Some(LOCAL));
        // The whole name has to match.
        assert_eq!(rules.map("web-platform.test.example"), None);
        assert_eq!(rules.map("example.com"), None);
        assert_eq!(rules.map("test"), None);
    }

    #[test]
    fn patterns_and_replacements() {
        let rules = HostMappingRules::parse(
            " map A?C.example [::1] ,, MAP *.example.org 10.0.0.1, MAP * ^NOTFOUND ",
        )
        .unwrap();
        assert_eq!(
            rules.map("abc.example"),
            Some(HostMapping::Address("::1".parse().unwrap()))
        );
        assert_eq!(
            rules.map("x.y.example.org"),
            Some(HostMapping::Address("10.0.0.1".parse().unwrap()))
        );
        // `?` is one character; the catch-all gets the rest.
        assert_eq!(rules.map("abbc.example"), Some(HostMapping::NotFound));
        assert_eq!(rules.map("example.org"), Some(HostMapping::NotFound));
        assert!(HostMappingRules::parse("").unwrap().is_empty());
        assert_eq!(HostMappingRules::default().map("anything"), None);
    }

    #[test]
    fn what_is_not_supported_is_an_error() {
        for bad in [
            "MAP *.test",
            "MAP *.test 127.0.0.1 extra",
            "EXCLUDE localhost",
            "MAPS *.test 127.0.0.1",
            "MAP *.test localhost",
            "MAP *.test 127.0.0.1:8000",
        ] {
            assert!(HostMappingRules::parse(bad).is_err(), "{bad}");
        }
    }
}
