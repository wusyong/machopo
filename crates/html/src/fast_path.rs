//! Blink's `HTMLDocumentParserFastPath`: the markup `innerHTML` and the like
//! are mostly given (a few common elements, attributes, text) parsed
//! without the tokenizer's state machine and the tree builder. The input is
//! scanned whole first; anything the fast path doesn't know, or whose tree
//! the tree builder would make otherwise than by plain nesting (an end tag
//! that isn't the current element's, a start tag that closes an open
//! element, a character reference other than a few), makes it give up
//! before any node is made, and the fragment parsing algorithm parses the
//! input instead. So what it builds is what that algorithm would. This is
//! the scan; `fast_path_build` makes the nodes from what it found.

use std::borrow::Cow;
use std::ops::Range;

/// What the scan found, in order: what building makes of it. Names are
/// where they are in the input; text and attribute values too unless a
/// character reference was replaced (`decode`, run once, as the scan
/// checks them): building copies each once, the only time it is copied.
pub(crate) enum Event<'a> {
    /// An element's start tag, with its attributes (in the scan's `attrs`)
    /// and the line its `>` is on (where its inline event handlers' sources
    /// are, as the tokenizer's line when it emits the tag); a void element
    /// has no end.
    Start {
        name: &'a str,
        attrs: Range<usize>,
        line: u32,
    },
    End,
    Text(Cow<'a, str>),
}

/// What [`scan`] found: the events, and every start tag's attributes, each
/// a name and its value decoded (empty for one without a value).
pub(crate) struct Scan<'a> {
    pub(crate) events: Vec<Event<'a>>,
    pub(crate) attrs: Vec<(&'a str, Cow<'a, str>)>,
}

/// The context elements whose children the tree builder parses in "in
/// body" with nothing open: what the fast path's elements nest in as they
/// would at the top of `<body>`.
pub(crate) fn is_context(name: &str) -> bool {
    matches!(
        name,
        "body"
            | "div"
            | "span"
            | "p"
            | "li"
            | "ul"
            | "ol"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "main"
            | "nav"
            | "label"
            | "button"
    )
}

/// The elements the fast path makes: ones "in body" makes by plain nesting
/// (given the checks in `scan`), and that need nothing else of the parser.
fn is_known(name: &str) -> bool {
    is_void(name)
        || matches!(
            name,
            "a" | "abbr"
                | "article"
                | "aside"
                | "b"
                | "bdi"
                | "bdo"
                | "button"
                | "cite"
                | "code"
                | "dfn"
                | "div"
                | "em"
                | "footer"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "header"
                | "i"
                | "kbd"
                | "label"
                | "li"
                | "main"
                | "mark"
                | "nav"
                | "ol"
                | "p"
                | "q"
                | "s"
                | "samp"
                | "section"
                | "small"
                | "span"
                | "strong"
                | "sub"
                | "sup"
                | "u"
                | "ul"
                | "var"
        )
}

pub(crate) fn is_void(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img" | "input" | "wbr")
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// The start tags "in body" begins by closing a `p` element in button
/// scope.
fn closes_p(name: &str) -> bool {
    is_heading(name)
        || matches!(
            name,
            "article"
                | "aside"
                | "div"
                | "footer"
                | "header"
                | "hr"
                | "li"
                | "main"
                | "nav"
                | "ol"
                | "p"
                | "section"
                | "ul"
        )
}

/// HTML's special elements among the known ones (what stops `li`'s walk
/// down the stack).
fn is_special(name: &str) -> bool {
    is_heading(name)
        || matches!(
            name,
            "article"
                | "aside"
                | "br"
                | "button"
                | "div"
                | "footer"
                | "header"
                | "hr"
                | "img"
                | "input"
                | "li"
                | "main"
                | "nav"
                | "ol"
                | "p"
                | "section"
                | "ul"
                | "wbr"
        )
}

/// The events of `input`, if the fast path knows all of it and the tree
/// builder would nest it as written.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "i is an index into `bytes`, or at most two past one: far from usize::MAX"
)]
pub(crate) fn scan(input: &str) -> Option<Scan<'_>> {
    let bytes = input.as_bytes();
    let mut events = Vec::new();
    let mut attrs: Vec<(&str, Cow<'_, str>)> = Vec::new();
    // The open elements' names, innermost last.
    let mut open: Vec<&str> = Vec::new();
    let mut line: u32 = 1;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            let text = &input[start..i];
            line = line.saturating_add(count_lines(text));
            events.push(Event::Text(decode(text)?));
            continue;
        }
        // A tag.
        i += 1;
        let end_tag = bytes.get(i) == Some(&b'/');
        if end_tag {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
        if name_start == i || !bytes[name_start].is_ascii_alphabetic() {
            return None;
        }
        let name = &input[name_start..i];
        // Lowercase names only (the known ones are): the tokenizer's
        // lowercasing needs no copy.
        if !is_known(name) {
            return None;
        }
        if end_tag {
            i = skip_space(bytes, i, &mut line)?;
            if bytes.get(i) != Some(&b'>') {
                return None;
            }
            i += 1;
            // Only the current element's: anything else needs the tree
            // builder's end tag steps (implied end tags, the adoption
            // agency).
            if open.last() != Some(&name) {
                return None;
            }
            open.pop();
            events.push(Event::End);
            continue;
        }
        let first_attr = attrs.len();
        loop {
            let after = skip_space(bytes, i, &mut line)?;
            match bytes.get(after) {
                Some(b'>') => {
                    i = after + 1;
                    break;
                }
                Some(b'/') if bytes.get(after + 1) == Some(&b'>') && is_void(name) => {
                    i = after + 2;
                    break;
                }
                // Attributes are separated from the name and each other.
                Some(_) if after > i => i = after,
                _ => return None,
            }
            let attr_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_lowercase()
                    || bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'-' | b'_'))
            {
                i += 1;
            }
            if attr_start == i {
                return None;
            }
            let attr_name = &input[attr_start..i];
            if attrs[first_attr..]
                .iter()
                .any(|&(name, _)| name == attr_name)
            {
                return None;
            }
            let value = if bytes.get(i) == Some(&b'=') {
                i += 1;
                match bytes.get(i) {
                    Some(&quote @ (b'"' | b'\'')) => {
                        let value_start = i + 1;
                        let close = input[value_start..].find(quote as char)? + value_start;
                        let raw = &input[value_start..close];
                        line = line.saturating_add(count_lines(raw));
                        i = close + 1;
                        raw
                    }
                    _ => {
                        let value_start = i;
                        while i < bytes.len()
                            && !bytes[i].is_ascii_whitespace()
                            && !matches!(bytes[i], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
                        {
                            i += 1;
                        }
                        if value_start == i {
                            return None;
                        }
                        &input[value_start..i]
                    }
                }
            } else {
                ""
            };
            attrs.push((attr_name, decode(value)?));
        }
        // The start tag steps that do more than insert an element.
        if closes_p(name) && open.contains(&"p") {
            return None;
        }
        if is_heading(name) && open.last().is_some_and(|current| is_heading(current)) {
            return None;
        }
        if name == "li" {
            for &element in open.iter().rev() {
                if element == "li" {
                    return None;
                }
                if is_special(element) && !matches!(element, "div" | "p") {
                    break;
                }
            }
        }
        if (name == "button" || name == "a") && open.contains(&name) {
            return None;
        }
        events.push(Event::Start {
            name,
            attrs: first_attr..attrs.len(),
            line,
        });
        if !is_void(name) {
            open.push(name);
        }
    }
    Some(Scan { events, attrs })
}

/// Skips the white space the tokenizer skips in a tag; `None` at a
/// carriage return (which the input stream normalizes).
fn skip_space(bytes: &[u8], i: usize, line: &mut u32) -> Option<usize> {
    for (at, &b) in bytes.iter().enumerate().skip(i) {
        match b {
            b' ' | b'\t' | b'\x0C' => {}
            b'\n' => *line = line.saturating_add(1),
            b'\r' => return None,
            _ => return Some(at),
        }
    }
    // All of it skipped (or `i` already past it).
    Some(bytes.len().max(i))
}

/// The lines `text` ends, saturating: line numbers are only reported.
pub(crate) fn count_lines(text: &str) -> u32 {
    u32::try_from(text.bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX)
}

/// Text or an attribute value with its character references replaced: the
/// few the fast path knows (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, and
/// numeric ones for characters that need no replacement), each with its
/// `;`. `None` for any other, a NUL or a carriage return, which the
/// tokenizer treats otherwise.
pub(crate) fn decode(raw: &str) -> Option<Cow<'_, str>> {
    #[cfg(debug_assertions)]
    // A count for tests: wrapping would only miscount.
    DECODES.set(DECODES.get().wrapping_add(1));
    if raw.bytes().any(|b| b == 0 || b == b'\r') {
        return None;
    }
    if !raw.contains('&') {
        return Some(Cow::Borrowed(raw));
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let (reference, after) = after.split_once(';')?;
        let c = match reference {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let digits = reference.strip_prefix('#')?;
                let value = match digits.strip_prefix(['x', 'X']) {
                    Some(hex) if !hex.is_empty() && hex.len() <= 6 => {
                        u32::from_str_radix(hex, 16).ok()?
                    }
                    None if !digits.is_empty() && digits.len() <= 7 => digits.parse().ok()?,
                    _ => return None,
                };
                // What the tokenizer passes through as it is: not a NUL,
                // a control, a surrogate, a noncharacter or past Unicode.
                let plain = matches!(value, 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0xFDCF | 0xFDF0..=0xFFFD)
                    || (0x10000..=0x10FFFD).contains(&value) && value & 0xFFFE != 0xFFFE
                    || matches!(value, 0x09 | 0x0A | 0x0C);
                if !plain {
                    return None;
                }
                char::from_u32(value)?
            }
        };
        out.push(c);
        rest = after;
    }
    out.push_str(rest);
    Some(Cow::Owned(out))
}

#[cfg(debug_assertions)]
thread_local! {
    static FAST_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(debug_assertions)]
pub(crate) fn note_fast_parse() {
    // A count for tests: wrapping would only miscount.
    FAST_PARSES.set(FAST_PARSES.get().wrapping_add(1));
}

/// How many fragments, on this thread, the fast path parsed. `None` in
/// release builds. For tests.
pub fn fast_parses_for_testing() -> Option<usize> {
    #[cfg(debug_assertions)]
    {
        Some(FAST_PARSES.get())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// How many texts and attribute values, on this thread, the fast path
/// decoded: what tells each is decoded once. `None` in release builds. For
/// tests.
pub fn decodes_for_testing() -> Option<usize> {
    #[cfg(debug_assertions)]
    {
        Some(DECODES.get())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}
