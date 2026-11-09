//! Custom properties and `var()` (css-variables-1): checking values that
//! hold `var()` (Blink's `CSSVariableParser`), substituting it, and
//! computing an element's custom properties with their cycles (the
//! substitution in Blink's `StyleCascade`, with `CascadeResolver`'s cycle
//! detection).
//!
//! A declaration whose value has a `var()` is kept unparsed
//! ([`UnparsedValue`]); the cascade substitutes it for the element and
//! only then parses it as its property, so whether it is valid is known
//! only then ("invalid at computed-value time": the property is `unset`).
//! A shorthand's longhands all share the shorthand's unparsed value
//! (Blink's `CSSPendingSubstitutionValue`).

use std::collections::HashMap;
use std::sync::Arc;

use computed_style::StyleVariables;
use cssparser::{ParseError, Parser, ParserInput, Token};

use crate::css_url_data::UrlContext;
use crate::properties::{Declaration, LonghandId, Shorthand, parse_longhands};
use crate::property_set::is_css_white_space;

/// The most a substitution may produce (Blink's `kMaxVariableBytes`):
/// `--b: var(--a) var(--a)` and so on doubles at each step.
const MAX_SUBSTITUTION_BYTES: usize = 2 * 1024 * 1024;

/// A value with `var()` in it, as written.
#[derive(Debug, Clone, PartialEq)]
pub struct UnparsedValue {
    pub(crate) css: Box<str>,
    /// The shorthand it was written for: each of its longhands holds the
    /// same value, and takes its part of the shorthand's.
    pub(crate) shorthand: Option<Shorthand>,
    /// What the `url()`s it is substituted to resolve in: where it was
    /// parsed's (Blink's `CSSUnparsedDeclarationValue`'s parser context).
    pub(crate) url_context: Option<UrlContext>,
}

impl UnparsedValue {
    pub(crate) fn new(css: &str, shorthand: Option<Shorthand>) -> Arc<Self> {
        Arc::new(Self {
            css: css.into(),
            shorthand,
            url_context: None,
        })
    }

    /// The longhands it stands for (`id` alone, if it isn't a
    /// shorthand's), with `var()` substituted from `variables` and the
    /// result parsed; `None` if that fails (invalid at computed-value
    /// time).
    pub fn resolve(&self, id: LonghandId, variables: &StyleVariables) -> Option<Vec<Declaration>> {
        let css = substitute(&self.css, &mut |name| variables.get(name).cloned())?;
        let longhands = match self.shorthand {
            Some(shorthand) => shorthand.longhands(),
            None => vec![id],
        };
        let mut input = ParserInput::new(&css);
        let mut parser = Parser::new(&mut input);
        let declarations = parse_longhands(self.shorthand, longhands, &mut parser).ok()?;
        let mut declarations: Vec<Declaration> = declarations.into_iter().map(|(d, _)| d).collect();
        if let Some(base) = &self.url_context {
            for declaration in &mut declarations {
                declaration.resolve_urls(base, &mut Vec::new());
            }
        }
        Some(declarations)
    }
}

/// Checks a `<declaration-value>` (any tokens but bad strings and URLs and
/// unmatched closing brackets; at the top level, no `;` or `!`), and each
/// `var()` in it: `var( <custom-property-name> [, <declaration-value>? ]? )`.
/// Returns whether it has a `var()`.
pub(crate) fn check_declaration_value<'i, E>(
    input: &mut Parser<'i, '_>,
    top_level: bool,
) -> Result<bool, ParseError<'i, E>> {
    let mut has_var = false;
    while let Ok(token) = input.next_including_whitespace_and_comments() {
        let token = token.clone();
        match token {
            Token::BadString(_)
            | Token::BadUrl(_)
            | Token::CloseParenthesis
            | Token::CloseSquareBracket
            | Token::CloseCurlyBracket => return Err(input.new_unexpected_token_error(token)),
            Token::Semicolon | Token::Delim('!') if top_level => {
                return Err(input.new_unexpected_token_error(token));
            }
            Token::Function(ref name) if name.eq_ignore_ascii_case("var") => {
                input.parse_nested_block(check_var)?;
                has_var = true;
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                has_var |=
                    input.parse_nested_block(|input| check_declaration_value(input, false))?;
            }
            _ => {}
        }
    }
    Ok(has_var)
}

/// The arguments of a `var()`.
fn check_var<'i, E>(input: &mut Parser<'i, '_>) -> Result<(), ParseError<'i, E>> {
    let name = input.expect_ident()?.clone();
    if !crate::property_set::is_custom_property_name(&name) {
        return Err(input.new_unexpected_token_error(Token::Ident(name)));
    }
    if input.is_exhausted() {
        return Ok(());
    }
    input.expect_comma()?;
    // The fallback is a `<declaration-value>` of its own: no `;` or `!`
    // outside its blocks.
    check_declaration_value(input, true).map(|_| ())
}

/// `css` without the white space and comments around it: what a value
/// with `var()` keeps (Chrome's `cssText` drops them too).
pub(crate) fn trim_value(css: &str) -> &str {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let (mut first, mut end) = (None, 0);
    loop {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace_and_comments() else {
            break;
        };
        let significant = !matches!(token, Token::WhiteSpace(_) | Token::Comment(_));
        if matches!(
            token,
            Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
        ) {
            let _ = parser.parse_nested_block(|input| {
                while input.next_including_whitespace_and_comments().is_ok() {}
                Ok::<_, ParseError<'_, ()>>(())
            });
        }
        if significant {
            first.get_or_insert(start);
            end = parser.position().byte_index();
        }
    }
    first.map_or("", |first| &css[first..end])
}

/// `css` with each `var()` replaced by what `lookup` gives for its name,
/// or else its fallback; `None` if one has neither, or if the result is
/// too long. White space around the result is trimmed.
pub(crate) fn substitute(
    css: &str,
    lookup: &mut dyn FnMut(&str) -> Option<Arc<str>>,
) -> Option<String> {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut out = Output::default();
    substitute_into(&mut parser, &mut out, lookup).ok()?;
    if out.failed {
        return None;
    }
    Some(out.text.trim_matches(is_css_white_space).to_owned())
}

/// The text a substitution builds. Substitution is of tokens, not text:
/// `var(--a)px` with `--a: 1` is a number then an identifier, not `1px`.
/// Where the text on the two sides of a substituted value would run
/// together into one token, a comment keeps them apart (as css-syntax's
/// serialization of tokens does).
#[derive(Default)]
struct Output {
    text: String,
    /// Where the last substituted value ends, if nothing follows it yet.
    after_value: Option<usize>,
    /// A `var()` had neither a value nor a fallback. The rest is still
    /// substituted: the custom properties it refers to may be in a cycle
    /// with the one being computed, which makes them all invalid.
    failed: bool,
}

impl Output {
    fn runs_together(a: char, b: char) -> bool {
        let word = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_');
        word(a) && (word(b) || b == '%')
    }

    fn separate(&mut self, next: &str) {
        if let (Some(a), Some(b)) = (self.text.chars().last(), next.chars().next())
            && Self::runs_together(a, b)
        {
            self.text.push_str("/**/");
        }
    }

    /// Text of the value being substituted, as written.
    fn push_raw(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.after_value == Some(self.text.len()) {
            self.separate(text);
        }
        self.text.push_str(text);
        self.after_value = None;
    }

    fn push_char(&mut self, c: char) {
        self.push_raw(c.encode_utf8(&mut [0; 4]));
    }

    /// A substituted value.
    fn push_value(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        self.separate(value);
        self.text.push_str(value);
        self.after_value = Some(self.text.len());
    }

    fn too_long(&self) -> bool {
        self.text.len() > MAX_SUBSTITUTION_BYTES
    }
}

/// Copies the tokens of `input` into `out` as written, substituting each
/// `var()`.
fn substitute_into<'i>(
    input: &mut Parser<'i, '_>,
    out: &mut Output,
    lookup: &mut dyn FnMut(&str) -> Option<Arc<str>>,
) -> Result<(), ParseError<'i, ()>> {
    let mut start = input.position();
    loop {
        let before = input.position();
        let Ok(token) = input.next_including_whitespace_and_comments() else {
            break;
        };
        let close = match token {
            Token::Function(name) if name.eq_ignore_ascii_case("var") => {
                out.push_raw(input.slice(start..before));
                input.parse_nested_block(|input| substitute_var(input, out, lookup))?;
                start = input.position();
                continue;
            }
            Token::Function(_) | Token::ParenthesisBlock => ')',
            Token::SquareBracketBlock => ']',
            Token::CurlyBracketBlock => '}',
            _ => continue,
        };
        out.push_raw(input.slice_from(start));
        input.parse_nested_block(|input| substitute_into(input, out, lookup))?;
        // The block's closing bracket (added if the input ended first,
        // as the parser closed it).
        out.push_char(close);
        start = input.position();
        if out.too_long() {
            return Err(input.new_custom_error(()));
        }
    }
    out.push_raw(input.slice_from(start));
    if out.too_long() {
        return Err(input.new_custom_error(()));
    }
    Ok(())
}

/// One `var()`'s arguments: its value, or its fallback substituted.
fn substitute_var<'i>(
    input: &mut Parser<'i, '_>,
    out: &mut Output,
    lookup: &mut dyn FnMut(&str) -> Option<Arc<str>>,
) -> Result<(), ParseError<'i, ()>> {
    let name = input.expect_ident()?.clone();
    match lookup(&name) {
        Some(value) => {
            // The fallback isn't used: skip it.
            while input.next_including_whitespace_and_comments().is_ok() {}
            out.push_value(&value);
        }
        None if input.is_exhausted() => out.failed = true,
        // Already invalid: a fallback isn't used, so what it refers to
        // isn't a reference (it makes no cycle).
        None if out.failed => while input.next_including_whitespace_and_comments().is_ok() {},
        None => {
            input.expect_comma()?;
            let mut fallback = Output::default();
            substitute_into(input, &mut fallback, lookup)?;
            out.failed |= fallback.failed;
            out.push_value(fallback.text.trim_matches(is_css_white_space));
        }
    }
    if out.too_long() {
        return Err(input.new_custom_error(()));
    }
    Ok(())
}

/// One custom property's declared value, for [`compute_custom_properties`].
#[derive(Debug, Clone, Copy)]
pub enum CustomDeclared<'a> {
    Value(&'a Arc<str>),
    /// `initial`: the guaranteed-invalid value.
    Initial,
    /// `inherit` or `unset` (custom properties inherit).
    Inherit,
}

impl<'a> CustomDeclared<'a> {
    /// What `--name: <value>` declares.
    pub fn new(value: &'a Arc<str>) -> Self {
        if value.eq_ignore_ascii_case("initial") {
            Self::Initial
        } else if value.eq_ignore_ascii_case("inherit") || value.eq_ignore_ascii_case("unset") {
            Self::Inherit
        } else {
            Self::Value(value)
        }
    }
}

/// An element's custom properties: its parent's, with the ones it
/// declares (the winning declaration of each) computed. A declared value's
/// `var()`s refer to the element's own custom properties, so they are
/// resolved on demand, depth first; every property in a cycle is invalid
/// at computed-value time, and so is one whose `var()` has neither a value
/// nor a fallback.
pub fn compute_custom_properties(
    parent: &StyleVariables,
    declared: &[(&Arc<str>, CustomDeclared<'_>)],
) -> StyleVariables {
    let mut variables = parent.clone();
    if declared.is_empty() {
        return variables;
    }
    let by_name: HashMap<&str, CustomDeclared<'_>> = declared
        .iter()
        .map(|(name, value)| (&***name, *value))
        .collect();
    let mut resolver = Resolver {
        parent,
        declared: &by_name,
        done: HashMap::new(),
        stack: Vec::new(),
        cycle_start: usize::MAX,
        cycle_end: 0,
    };
    for (name, value) in declared {
        let computed = match value {
            CustomDeclared::Initial => None,
            CustomDeclared::Inherit => {
                if !parent.contains(name) {
                    continue;
                }
                parent.get(name).cloned()
            }
            CustomDeclared::Value(_) => resolver.resolve(name),
        };
        variables.set((*name).clone(), computed);
    }
    variables
}

struct Resolver<'a, 'd> {
    parent: &'a StyleVariables,
    declared: &'a HashMap<&'d str, CustomDeclared<'d>>,
    done: HashMap<&'d str, Option<Arc<str>>>,
    /// The properties being resolved, outermost first.
    stack: Vec<&'d str>,
    /// The places in `stack` of the cycle found (Blink's
    /// `CascadeResolver`): from the property a reference came back to, up
    /// to the one that made it. A property pushed after that, while the
    /// cycle's are still being resolved, isn't in it unless it refers
    /// back too.
    cycle_start: usize,
    cycle_end: usize,
}

impl<'a, 'd> Resolver<'a, 'd> {
    fn resolve(&mut self, name: &str) -> Option<Arc<str>> {
        let Some((&name, &declared)) = self.declared.get_key_value(name) else {
            return self.parent.get(name).cloned();
        };
        let value = match declared {
            CustomDeclared::Initial => return None,
            CustomDeclared::Inherit => return self.parent.get(name).cloned(),
            CustomDeclared::Value(value) => value,
        };
        if let Some(done) = self.done.get(name) {
            return done.clone();
        }
        if let Some(i) = self.stack.iter().position(|&n| n == name) {
            self.cycle_start = self.cycle_start.min(i);
            self.cycle_end = self.stack.len();
            return None;
        }
        let computed = if has_var(value) {
            // Where it goes on the stack.
            let depth = self.stack.len();
            self.stack.push(name);
            let substituted = substitute(value, &mut |n| self.resolve(n));
            self.stack.pop();
            let in_cycle = (self.cycle_start..self.cycle_end).contains(&depth);
            if self.cycle_start >= self.stack.len() {
                self.cycle_start = usize::MAX;
                self.cycle_end = 0;
            }
            match substituted {
                _ if in_cycle => None,
                // A CSS-wide keyword alone, once substituted, is that
                // keyword (css-variables-2).
                Some(text) => match CustomDeclared::new(&Arc::from(text)) {
                    CustomDeclared::Initial => None,
                    CustomDeclared::Inherit => self.parent.get(name).cloned(),
                    CustomDeclared::Value(text) => Some(text.clone()),
                },
                None => None,
            }
        } else {
            Some(value.clone())
        };
        self.done.insert(name, computed.clone());
        computed
    }
}

/// Whether `css` may have a `var()`: a quick check before tokenizing.
fn has_var(css: &str) -> bool {
    css.as_bytes()
        .windows(4)
        .any(|w| w[3] == b'(' && w[..3].eq_ignore_ascii_case(b"var"))
}
