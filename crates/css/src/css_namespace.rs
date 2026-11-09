//! What CSSOM's `CSS` namespace asks of style (Blink's `DOMWindowCSS`):
//! whether a declaration parses, conditions of them (css-conditional 3's
//! `<supports-condition>`; machopo has no `@supports` rule yet, whose
//! condition would be this one), and identifiers escaped.

use cssparser::{ParseError, Parser, ParserInput, Token, match_ignore_ascii_case};

use crate::properties::parse_declaration;
use crate::property_set::{is_custom_property_name, parse_custom_property_value};

/// `CSS.supports(property, value)`: whether `property` is one machopo
/// supports and `value` parses as it (a CSS-wide keyword does, and so
/// does any value with a `var()`), or `property` is a custom property and
/// `value` a `<declaration-value>`.
pub fn supports_declaration(property: &str, value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    if is_custom_property_name(property) {
        return parser
            .parse_entirely(parse_custom_property_value::<()>)
            .is_ok();
    }
    parse_declaration(property, &mut parser).is_ok()
}

/// `CSS.supports(conditionText)`: `conditionText` as a
/// `<supports-condition>`, or else, in parentheses, as a declaration
/// (`"color: red"`); false if it is neither.
pub fn supports_condition(text: &str) -> bool {
    condition_text(text)
        .or_else(|| condition_text(&format!("({text})")))
        .unwrap_or(false)
}

type Result<'i, T> = std::result::Result<T, ParseError<'i, ()>>;

fn condition_text(text: &str) -> Option<bool> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    parser.parse_entirely(condition).ok()
}

/// `not <in-parens> | <in-parens> [and <in-parens>]* | <in-parens> [or
/// <in-parens>]*`: `and` and `or` don't mix without parentheses.
/// An `@supports` rule's prelude, `<supports-condition>`: whether it holds.
pub(crate) fn parse_supports_condition<'i>(input: &mut Parser<'i, '_>) -> Result<'i, bool> {
    condition(input)
}

fn condition<'i>(input: &mut Parser<'i, '_>) -> Result<'i, bool> {
    if input.try_parse(|i| i.expect_ident_matching("not")).is_ok() {
        return Ok(!in_parens(input)?);
    }
    let mut value = in_parens(input)?;
    let mut operator: Option<bool> = None;
    loop {
        let and = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?.clone();
            match_ignore_ascii_case! { &ident,
                "and" => Ok(true),
                "or" => Ok(false),
                _ => Err(location.new_custom_error::<(), ()>(())),
            }
        });
        let Ok(and) = and else {
            return Ok(value);
        };
        if operator.is_some_and(|operator| operator != and) {
            return Err(input.new_custom_error(()));
        }
        operator = Some(and);
        let next = in_parens(input)?;
        value = if and { value && next } else { value || next };
    }
}

/// `( <condition> ) | ( <declaration> ) | selector( <complex-selector> )`,
/// or anything else in parentheses or a function, which is false
/// (`<general-enclosed>`).
fn in_parens<'i>(input: &mut Parser<'i, '_>) -> Result<'i, bool> {
    let location = input.current_source_location();
    match input.next()?.clone() {
        Token::ParenthesisBlock => input.parse_nested_block(|i| {
            if let Ok(value) = i.try_parse(|i| i.parse_entirely(condition)) {
                return Ok(value);
            }
            if let Ok(value) = i.try_parse(declaration) {
                return Ok(value);
            }
            skip_rest(i);
            Ok(false)
        }),
        Token::Function(name) if name.eq_ignore_ascii_case("selector") => {
            input.parse_nested_block(|i| {
                let start = i.position();
                skip_rest(i);
                Ok(crate::selector::supports_selector(i.slice_from(start)))
            })
        }
        Token::Function(_) => input.parse_nested_block(|i| {
            skip_rest(i);
            Ok(false)
        }),
        _ => Err(location.new_custom_error(())),
    }
}

/// `property: value`, with or without `!important`.
fn declaration<'i>(input: &mut Parser<'i, '_>) -> Result<'i, bool> {
    let name = input.expect_ident()?.clone();
    input.expect_colon()?;
    let start = input.position();
    skip_rest(input);
    let value = input.slice_from(start).trim_end();
    let value = match value.len().checked_sub("important".len()) {
        Some(at) if value[at..].eq_ignore_ascii_case("important") => {
            match value[..at].trim_end().strip_suffix('!') {
                Some(value) => value,
                None => value,
            }
        }
        _ => value,
    };
    Ok(supports_declaration(&name, value))
}

fn skip_rest(input: &mut Parser<'_, '_>) {
    while input.next_including_whitespace_and_comments().is_ok() {}
}

/// `CSS.escape(ident)`: CSSOM's "serialize an identifier" (a leading
/// digit as a code point, `\0` as U+FFFD).
pub fn escape_identifier(ident: &str) -> String {
    let mut out = String::new();
    cssparser::serialize_identifier(ident, &mut out).expect("writing to a String");
    out
}
