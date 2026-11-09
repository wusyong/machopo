//! The DOM's checks of names (https://dom.spec.whatwg.org/#namespaces): which
//! local names and prefixes are valid, and "validate and extract" for the
//! namespaced members (Blink's `Document::ParseQualifiedName` and
//! `QualifiedName` checks).

use dom::QualName;

use crate::exception::{ApiError, ExceptionName};

const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// ASCII whitespace, NULL, `/` or `>`: what no name may contain.
fn is_delimiter(c: char) -> bool {
    c.is_ascii_whitespace() || matches!(c, '\0' | '/' | '>')
}

/// "Valid namespace prefix": not empty, no delimiter.
/// DOM's "valid doctype name": no ASCII whitespace, NULL or `>`.
pub(crate) fn is_valid_doctype_name(name: &str) -> bool {
    !name
        .chars()
        .any(|c| c.is_ascii_whitespace() || matches!(c, '\0' | '>'))
}

/// XML's `Name` production: a name start character, then name characters
/// (a processing instruction's target; element and attribute names have
/// DOM's own, looser rules).
pub(crate) fn is_xml_name(name: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c,
            ':' | 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}')
    }
    fn rest(c: char) -> bool {
        start(c)
            || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
    }
    let mut chars = name.chars();
    chars.next().is_some_and(start) && chars.all(rest)
}

pub(crate) fn is_valid_namespace_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && !prefix.chars().any(is_delimiter)
}

/// "Valid attribute local name": not empty, no delimiter and no `=`.
pub(crate) fn is_valid_attribute_local_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| is_delimiter(c) || c == '=')
}

/// "Valid element local name": an ASCII letter then anything but a
/// delimiter (what HTML's parser can make), or a `:`, `_` or non-ASCII
/// character then ASCII letters, digits, `-`, `.`, `:`, `_` or non-ASCII
/// characters.
pub(crate) fn is_valid_element_local_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_ascii_alphabetic() {
        return !chars.any(is_delimiter);
    }
    let name_char = |c: char| matches!(c, ':' | '_') || !c.is_ascii();
    name_char(first)
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.') || name_char(c))
}

/// Whose names "validate and extract" checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameContext {
    Element,
    Attribute,
}

/// DOM's "validate and extract": `namespace` (empty is null) and
/// `qualified` split at its first colon into a prefix and a local name,
/// each checked, and the namespace checked against the prefix.
pub(crate) fn validate_and_extract<'a>(
    namespace: Option<&'a str>,
    qualified: &'a str,
    context: NameContext,
) -> Result<QualName<'a>, ApiError> {
    let invalid = || ApiError::dom(ExceptionName::InvalidCharacter);
    let namespace_error = || ApiError::dom(ExceptionName::Namespace);
    let namespace = namespace.filter(|uri| !uri.is_empty());
    let (prefix, local) = match qualified.split_once(':') {
        Some((prefix, local)) => {
            if !is_valid_namespace_prefix(prefix) {
                return Err(invalid());
            }
            (Some(prefix), local)
        }
        None => (None, qualified),
    };
    let valid = match context {
        NameContext::Attribute => is_valid_attribute_local_name(local),
        NameContext::Element => is_valid_element_local_name(local),
    };
    if !valid {
        return Err(invalid());
    }
    if prefix.is_some() && namespace.is_none() {
        return Err(namespace_error());
    }
    if prefix == Some("xml") && namespace != Some(XML_NAMESPACE) {
        return Err(namespace_error());
    }
    let xmlns = qualified == "xmlns" || prefix == Some("xmlns");
    if xmlns != (namespace == Some(XMLNS_NAMESPACE)) {
        return Err(namespace_error());
    }
    Ok(QualName {
        namespace,
        prefix,
        local,
    })
}
