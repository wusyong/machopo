//! XML parser (Blink's `core/xml/parser`, `XMLDocumentParser`): `xml5ever`
//! does the tokenizing and tree construction into the same sink as HTML's
//! (`next::parse_xml_document`), for the SVG files images are made of.
//!
//! Chrome parses XML with libxml2, which stops at the first error; an SVG
//! image with one is broken. `xml5ever` follows XML5 instead, recovering
//! from errors, and machopo draws what it builds. libxml2 reads the entities a
//! DOCTYPE declares, which `xml5ever` doesn't (Illustrator writes
//! `xmlns="&ns_svg;"`), so [`prepare`] expands them first.

use std::borrow::Cow;
use std::collections::HashMap;

/// How deep entities may refer to entities (libxml2's `xmlParserMaxDepth`
/// for them, 40).
const MAX_DEPTH: usize = 40;

/// How much expanding may add to the input, past which references are left
/// as they are: what keeps an entity defined as ten of another, ten levels
/// deep ("billion laughs"), from growing without end (libxml2 stops such a
/// file with an error).
const MAX_GROWTH: usize = 1 << 20;

/// The input as libxml2 reads it before its tree is built: without a byte
/// order mark or the XML declaration (which isn't a node), and without the
/// DOCTYPE's internal subset, the general entities it declares with a value
/// expanded where content and attribute values refer to them (not in
/// comments, CDATA sections or processing instructions). External and
/// parameter entities aren't read.
pub(crate) fn prepare(input: &str) -> Cow<'_, str> {
    let mut rest = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let mut out = String::new();
    let mut changed = rest.len() != input.len();
    if rest.starts_with("<?xml")
        && rest[5..].starts_with(|c: char| c.is_ascii_whitespace() || c == '?')
        && let Some((_, after)) = rest.split_once("?>")
    {
        rest = after;
        changed = true;
    }
    let mut entities = HashMap::new();
    if let Some((before, doctype, after)) = split_doctype(rest) {
        out.push_str(before);
        out.push_str(&doctype.head);
        out.push('>');
        if let Some(subset) = doctype.subset {
            entities = declared_entities(subset);
        }
        rest = after;
        changed = true;
    }
    if entities.is_empty() {
        if !changed {
            return Cow::Borrowed(input);
        }
        out.push_str(rest);
        return Cow::Owned(out);
    }
    // Only compared with what is taken from it: saturating doesn't change
    // that.
    let mut budget = rest.len().saturating_add(MAX_GROWTH);
    expand(rest, &entities, &mut out, &mut budget, 0);
    Cow::Owned(out)
}

/// A DOCTYPE: what comes before its internal subset (without the closing
/// `>`), and the subset, if it has one.
struct Doctype<'a> {
    head: String,
    subset: Option<&'a str>,
}

/// The text before the DOCTYPE, the DOCTYPE, and the text after it; `None`
/// if the prolog (white space, comments and processing instructions) ends
/// without one.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offsets into `input`, from its own `find`s and what they found: within its length"
)]
fn split_doctype(input: &str) -> Option<(&str, Doctype<'_>, &str)> {
    let mut at = 0;
    loop {
        let rest = &input[at..];
        let trimmed = rest.trim_start();
        at += rest.len() - trimmed.len();
        if trimmed.starts_with("<!--") {
            at += trimmed.find("-->")? + 3;
        } else if trimmed.starts_with("<?") {
            at += trimmed.find("?>")? + 2;
        } else if trimmed.starts_with("<!DOCTYPE") {
            break;
        } else {
            return None;
        }
    }
    let start = at;
    // To the subset's `[` or the end's `>`, past quoted literals.
    let bytes = input.as_bytes();
    let mut i = start + "<!DOCTYPE".len();
    let mut quote = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b @ (b'"' | b'\'')) => quote = Some(b),
            (None, b'>') => {
                let head = input[start..i].to_owned();
                return Some((
                    &input[..start],
                    Doctype { head, subset: None },
                    &input[i + 1..],
                ));
            }
            (None, b'[') => break,
            (None, _) => {}
        }
        i += 1;
    }
    let head = input[start..i].trim_end().to_owned();
    let subset_start = i + 1;
    let subset_end = subset_end(input, subset_start)?;
    let after = &input[subset_end + 1..];
    let close = after.find('>')?;
    Some((
        &input[..start],
        Doctype {
            head,
            subset: Some(&input[subset_start..subset_end]),
        },
        &after[close + 1..],
    ))
}

/// Where the internal subset that starts at `start` ends: its `]`, past
/// quoted literals, comments and processing instructions.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offsets into `input`, from its own `find`s and what they found: within its length"
)]
fn subset_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = start;
    let mut quote = None;
    while i < bytes.len() {
        let rest = &input[i..];
        match quote {
            Some(q) if bytes[i] == q => quote = None,
            Some(_) => {}
            None if rest.starts_with("<!--") => {
                i += rest.find("-->")? + 3;
                continue;
            }
            None if rest.starts_with("<?") => {
                i += rest.find("?>")? + 2;
                continue;
            }
            None if bytes[i] == b'"' || bytes[i] == b'\'' => quote = Some(bytes[i]),
            None if bytes[i] == b']' => return Some(i),
            None => {}
        }
        i += 1;
    }
    None
}

/// The general entities `subset` declares with a value (`<!ENTITY name
/// "value">`), the first declaration of a name winning, as in XML; not
/// parameter entities (`<!ENTITY % name ...>`) or external ones (`SYSTEM`,
/// `PUBLIC`).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offsets into `subset`, from its own `find`s, past one-byte quotes: within its length"
)]
fn declared_entities(subset: &str) -> HashMap<String, String> {
    let mut entities = HashMap::new();
    let mut rest = subset;
    while let Some(at) = rest.find("<!ENTITY") {
        rest = &rest[at + "<!ENTITY".len()..];
        // A parameter entity's `%` comes where the name would, and what
        // follows it isn't a value: it isn't read.
        let declaration = rest.trim_start();
        let name_end = declaration
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(declaration.len());
        let name = &declaration[..name_end];
        let value = declaration[name_end..].trim_start();
        let Some(quote) = value.chars().next().filter(|&c| c == '"' || c == '\'') else {
            continue;
        };
        let Some(len) = value[1..].find(quote) else {
            break;
        };
        if !name.is_empty() {
            entities
                .entry(name.to_owned())
                .or_insert_with(|| value[1..1 + len].to_owned());
        }
        rest = &value[1 + len + 1..];
    }
    entities
}

/// Appends `text` to `out` with the references to `entities` expanded, their
/// values expanded in turn, `depth` levels down; markup sections that don't
/// take references (comments, CDATA sections, processing instructions) are
/// copied. A reference left as it is: one to an entity not declared, one
/// too deep, and one whose value would take more than the `budget` left.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offsets into `text`, from its own `find`s; the budget is taken from only when \
              it holds the value; depth < MAX_DEPTH"
)]
fn expand(
    text: &str,
    entities: &HashMap<String, String>,
    out: &mut String,
    budget: &mut usize,
    depth: usize,
) {
    let mut rest = text;
    while let Some(at) = rest.find(['&', '<']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('<') {
            let end = if rest.starts_with("<!--") {
                rest.find("-->").map(|i| i + 3)
            } else if rest.starts_with("<![CDATA[") {
                rest.find("]]>").map(|i| i + 3)
            } else if rest.starts_with("<?") {
                rest.find("?>").map(|i| i + 2)
            } else {
                None
            };
            let end = end.unwrap_or(1);
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        let reference = rest[1..]
            .find(';')
            .map(|end| &rest[1..1 + end])
            .filter(|name| !name.is_empty());
        let value = reference.and_then(|name| entities.get(name));
        match (reference, value) {
            (Some(name), Some(value)) if depth < MAX_DEPTH && value.len() <= *budget => {
                *budget -= value.len();
                expand(value, entities, out, budget, depth + 1);
                rest = &rest[name.len() + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
}
