//! Media queries (https://drafts.csswg.org/mediaqueries-4/), parsed from
//! text and evaluated against a [`MediaEnvironment`] (Blink's
//! `MediaQuerySet`, `MediaQueryEvaluator` and `MediaValues`), and HTML's
//! `sizes` attribute (Blink's `SizesAttributeParser`): what an `<img>`'s
//! source selection evaluates, and what stylesheets' `@media` and `media`
//! evaluate.
//!
//! The features are those a page in a tab has an answer for: `width`,
//! `height`, `aspect-ratio`, `orientation`, `resolution` (and Chrome's
//! `-webkit-device-pixel-ratio`), and the user's settings as Chrome's
//! defaults on a desktop (`prefers-color-scheme: light`, `hover: hover`,
//! `pointer: fine`...). Any other feature is unknown: Level 4's third
//! truth value, which a query evaluates as false, `not` included.

use cssparser::{Delimiter, ParseError, Parser, ParserInput, Token, match_ignore_ascii_case};

/// What a query is evaluated against: the viewport, in CSS px, and the
/// device pixel ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaEnvironment {
    pub width: f32,
    pub height: f32,
    pub device_pixel_ratio: f32,
}

/// What `em` and `rem` are in a media query: the initial font size
/// (`medium`), not the root element's.
const FONT_SIZE: f32 = 16.0;

type Result<'i, T> = std::result::Result<T, ParseError<'i, ()>>;

fn invalid<'i, T>(input: &Parser<'i, '_>) -> Result<'i, T> {
    Err(input.new_custom_error(()))
}

/// A `<media-query-list>`: it matches if any query does; an empty one
/// always does. A query that doesn't parse matches nothing (`not all`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQueryList(Vec<Option<MediaQuery>>);

#[derive(Debug, Clone, PartialEq)]
struct MediaQuery {
    not: bool,
    only: bool,
    /// `None` for `all` (or none written).
    media_type: Option<MediaType>,
    /// The type as written, ASCII lowercase; `None` if none was.
    type_name: Option<String>,
    condition: Option<MediaCondition>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MediaType {
    Screen,
    Print,
    /// Any other name: Level 4 keeps it, and it matches nothing.
    Other,
}

impl MediaQueryList {
    /// `text` as a media query list.
    pub fn parse(text: &str) -> Self {
        let mut input = ParserInput::new(text);
        Self::parse_from(&mut Parser::new(&mut input))
    }

    /// The rest of `input` as a media query list: an `@media` rule's
    /// prelude.
    pub fn parse_from(input: &mut Parser<'_, '_>) -> Self {
        // A list with nothing in it (empty or blank text) is empty.
        if input.is_exhausted() {
            return Self(Vec::new());
        }
        let mut queries = Vec::new();
        loop {
            let query = input
                .parse_until_before(Delimiter::Comma, |input| {
                    let query = MediaQuery::parse(input)?;
                    input.expect_exhausted()?;
                    Ok(query)
                })
                .ok();
            queries.push(query);
            match input.next() {
                Ok(Token::Comma) => {}
                _ => break,
            }
        }
        Self(queries)
    }

    /// Whether it has no query: it matches everywhere (a `<style>` with
    /// no `media`).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn matches(&self, environment: &MediaEnvironment) -> bool {
        self.0.is_empty()
            || self
                .0
                .iter()
                .flatten()
                .any(|query| query.matches(environment))
    }

    /// `mediaText` (CSSOM's "serialize a media query list"): each query
    /// serialized, `not all` for one that didn't parse, joined with `, `.
    pub fn to_css(&self) -> String {
        self.queries().collect::<Vec<_>>().join(", ")
    }

    /// Each query, serialized.
    pub fn queries(&self) -> impl Iterator<Item = String> + '_ {
        self.0.iter().map(|query| match query {
            Some(query) => query.to_css(),
            None => "not all".to_owned(),
        })
    }

    /// `appendMedium(medium)`: the list with `medium` (one query) at the
    /// end, unless it doesn't parse or is in the list already (compared
    /// serialized); `None` then.
    pub fn with_medium(&self, medium: &str) -> Option<Self> {
        let query = Self::parse_medium(medium)?;
        let text = query
            .as_ref()
            .map_or_else(|| "not all".to_owned(), MediaQuery::to_css);
        if self.queries().any(|existing| existing == text) {
            return None;
        }
        let mut queries = self.0.clone();
        queries.push(query);
        Some(Self(queries))
    }

    /// `deleteMedium(medium)`: the list without the queries that serialize
    /// as `medium` does; `None` if there is none (`NotFoundError`).
    pub fn without_medium(&self, medium: &str) -> Option<Self> {
        let query = Self::parse_medium(medium)?;
        let text = query
            .as_ref()
            .map_or_else(|| "not all".to_owned(), MediaQuery::to_css);
        let kept: Vec<_> = self
            .0
            .iter()
            .zip(self.queries())
            .filter(|(_, existing)| *existing != text)
            .map(|(query, _)| query.clone())
            .collect();
        (kept.len() < self.0.len()).then_some(Self(kept))
    }

    /// One medium, as `appendMedium` and `deleteMedium` take it: a list of
    /// exactly one query (which may be `not all`); `None` for anything
    /// else.
    fn parse_medium(medium: &str) -> Option<Option<MediaQuery>> {
        let mut list = Self::parse(medium).0;
        (list.len() == 1).then(|| list.pop().expect("one"))
    }
}

impl MediaQuery {
    fn parse<'i>(input: &mut Parser<'i, '_>) -> Result<'i, Self> {
        // `<media-condition>` alone, or `not`/`only`? a type, and maybe
        // `and` a condition without `or`.
        let start = input.state();
        if let Ok(condition) = MediaCondition::parse(input, true) {
            return Ok(Self {
                not: false,
                only: false,
                media_type: None,
                type_name: None,
                condition: Some(condition),
            });
        }
        input.reset(&start);
        let mut not = false;
        let mut only = false;
        let ident = input.expect_ident()?.clone();
        let name = match_ignore_ascii_case! { &ident,
            "not" => {
                not = true;
                input.expect_ident()?.clone()
            },
            "only" => {
                only = true;
                input.expect_ident()?.clone()
            },
            _ => ident,
        };
        let media_type = match_ignore_ascii_case! { &name,
            "all" => None,
            "screen" => Some(MediaType::Screen),
            "print" => Some(MediaType::Print),
            // Reserved words, never a type.
            "not" | "only" | "and" | "or" | "layer" => return invalid(input),
            _ => Some(MediaType::Other),
        };
        let condition = if input.try_parse(|i| i.expect_ident_matching("and")).is_ok() {
            Some(MediaCondition::parse(input, false)?)
        } else {
            None
        };
        Ok(Self {
            not,
            only,
            media_type,
            type_name: Some(name.to_ascii_lowercase()),
            condition,
        })
    }

    /// Serialized as Chrome does (mediaqueries-4's serialization): `not` or
    /// `only`, the type, `and` the condition; `all and` left out before a
    /// condition alone.
    fn to_css(&self) -> String {
        let mut out = String::new();
        if self.not {
            out.push_str("not ");
        } else if self.only {
            out.push_str("only ");
        }
        let condition = self.condition.as_ref().map(|c| c.0.to_css());
        match (&self.type_name, condition) {
            (Some(name), Some(condition)) if name == "all" && !self.not && !self.only => {
                out.push_str(&condition)
            }
            (Some(name), Some(condition)) => {
                out.push_str(name);
                out.push_str(" and ");
                out.push_str(&condition);
            }
            (Some(name), None) => out.push_str(name),
            (None, Some(condition)) => out.push_str(&condition),
            (None, None) => out.push_str("all"),
        }
        out
    }

    fn matches(&self, environment: &MediaEnvironment) -> bool {
        let of_type = match self.media_type {
            None | Some(MediaType::Screen) => Some(true),
            Some(MediaType::Print | MediaType::Other) => Some(false),
        };
        let condition = self
            .condition
            .as_ref()
            .map_or(Some(true), |c| c.evaluate(environment));
        let result = and(of_type, condition);
        // An unknown result matches nothing, `not` or not.
        result.is_some_and(|result| result != self.not)
    }
}

/// Level 4's three-valued `and`: false wins, then unknown.
fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// A `<media-condition>` (or `<media-condition-without-or>`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaCondition(Condition);

#[derive(Debug, Clone, PartialEq)]
enum Condition {
    Not(Box<Condition>),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    /// A condition in parentheses, kept as one for serializing.
    Group(Box<Condition>),
    Feature(Feature),
    /// `<general-enclosed>`: well-formed but unknown; as written.
    Unknown(String),
}

impl MediaCondition {
    /// Parses a condition; `or` only if `allow_or`.
    pub fn parse<'i>(input: &mut Parser<'i, '_>, allow_or: bool) -> Result<'i, Self> {
        Condition::parse(input, allow_or).map(Self)
    }

    /// Whether it holds; unknown counts as not.
    pub fn matches(&self, environment: &MediaEnvironment) -> bool {
        self.0.evaluate(environment) == Some(true)
    }

    fn evaluate(&self, environment: &MediaEnvironment) -> Option<bool> {
        self.0.evaluate(environment)
    }
}

impl Condition {
    fn parse<'i>(input: &mut Parser<'i, '_>, allow_or: bool) -> Result<'i, Self> {
        if input.try_parse(|i| i.expect_ident_matching("not")).is_ok() {
            return Ok(Self::Not(Box::new(Self::parse_in_parens(input)?)));
        }
        let first = Self::parse_in_parens(input)?;
        let state = input.state();
        let keyword = match input.next() {
            Ok(Token::Ident(ident)) => ident.clone(),
            _ => {
                input.reset(&state);
                return Ok(first);
            }
        };
        let is_and = keyword.eq_ignore_ascii_case("and");
        if !is_and && !(allow_or && keyword.eq_ignore_ascii_case("or")) {
            input.reset(&state);
            return Ok(first);
        }
        let mut terms = vec![first, Self::parse_in_parens(input)?];
        // The same keyword throughout: `and` and `or` don't mix.
        while input
            .try_parse(|i| i.expect_ident_matching(if is_and { "and" } else { "or" }))
            .is_ok()
        {
            terms.push(Self::parse_in_parens(input)?);
        }
        Ok(if is_and {
            Self::And(terms)
        } else {
            Self::Or(terms)
        })
    }

    /// `( <media-condition> )`, `( <media-feature> )`, or
    /// `<general-enclosed>` (a function, or parentheses of anything else).
    fn parse_in_parens<'i>(input: &mut Parser<'i, '_>) -> Result<'i, Self> {
        let outer = input.position();
        match input.next()?.clone() {
            Token::ParenthesisBlock => input.parse_nested_block(|input| {
                let start = input.state();
                if let Ok(condition) = input.try_parse(|i| {
                    let condition = Self::parse(i, true)?;
                    i.expect_exhausted()?;
                    Ok::<_, ParseError<'i, ()>>(condition)
                }) {
                    return Ok(Self::Group(Box::new(condition)));
                }
                input.reset(&start);
                if let Ok(feature) = input.try_parse(|i| {
                    let feature = Feature::parse(i)?;
                    i.expect_exhausted()?;
                    Ok::<_, ParseError<'i, ()>>(feature)
                }) {
                    return Ok(Self::Feature(feature));
                }
                input.reset(&start);
                any_value(input)?;
                Ok(Self::Unknown(format!(
                    "({})",
                    input.slice_from(start.position()).trim()
                )))
            }),
            Token::Function(_) => {
                input.parse_nested_block(any_value)?;
                Ok(Self::Unknown(input.slice_from(outer).to_owned()))
            }
            _ => invalid(input),
        }
    }

    fn evaluate(&self, environment: &MediaEnvironment) -> Option<bool> {
        match self {
            Self::Not(condition) => condition.evaluate(environment).map(|b| !b),
            // Every term counts: an unknown one doesn't end it, as a false
            // (or true) one after it decides.
            Self::And(terms) => {
                let mut result = Some(true);
                for term in terms {
                    result = and(result, term.evaluate(environment));
                }
                result
            }
            Self::Or(terms) => {
                let mut result = Some(false);
                for term in terms {
                    result = or(result, term.evaluate(environment));
                }
                result
            }
            Self::Group(condition) => condition.evaluate(environment),
            Self::Feature(feature) => feature.evaluate(environment),
            Self::Unknown(_) => None,
        }
    }

    fn to_css(&self) -> String {
        match self {
            Self::Not(condition) => format!("not {}", condition.to_css()),
            Self::And(terms) => join(terms, " and "),
            Self::Or(terms) => join(terms, " or "),
            Self::Group(condition) => format!("({})", condition.to_css()),
            Self::Feature(feature) => feature.text.clone(),
            Self::Unknown(text) => text.clone(),
        }
    }
}

fn join(terms: &[Condition], separator: &str) -> String {
    terms
        .iter()
        .map(Condition::to_css)
        .collect::<Vec<_>>()
        .join(separator)
}

/// The rest of a block as `<general-enclosed>` takes it, `<any-value>`:
/// anything but a closing bracket with no opening one, or a bad string or
/// URL.
fn any_value<'i>(input: &mut Parser<'i, '_>) -> Result<'i, ()> {
    while let Ok(token) = input.next() {
        if matches!(
            token,
            Token::CloseParenthesis
                | Token::CloseSquareBracket
                | Token::CloseCurlyBracket
                | Token::BadString(_)
                | Token::BadUrl(_)
        ) {
            return invalid(input);
        }
    }
    Ok(())
}

/// A feature's value as written.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Value {
    /// A length, in px (font-relative ones at the initial font size;
    /// viewport ones resolved when evaluated).
    Length(f32, LengthKind),
    /// A resolution, in dppx.
    Resolution(f32),
    /// A number (an integer, or a ratio's numerator alone).
    Number(f32),
    /// A ratio, `a / b`.
    Ratio(f32, f32),
    Ident(Keyword),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LengthKind {
    Px,
    Vw,
    Vh,
    Vmin,
    Vmax,
}

/// The keywords features take.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Keyword {
    Portrait,
    Landscape,
    Light,
    Dark,
    NoPreference,
    Reduce,
    Hover,
    None,
    Fine,
    Coarse,
    Browser,
    Other,
}

impl Value {
    /// A value, and its text as Chrome serializes it: units and keywords
    /// ASCII lowercase, a ratio as `a / b`.
    fn parse<'i>(input: &mut Parser<'i, '_>) -> Result<'i, (Self, String)> {
        let token = input.next()?.clone();
        let number = |value: f32| crate::computed::write_css_number(value.into());
        match token {
            Token::Dimension { value, unit, .. } => {
                let text = format!("{}{}", number(value), unit.to_ascii_lowercase());
                if let Some((px, kind)) = length(value, &unit) {
                    return Ok((Self::Length(px, kind), text));
                }
                let Some(dppx) = crate::calc::resolution_dppx(value, &unit) else {
                    return invalid(input);
                };
                Ok((Self::Resolution(dppx), text))
            }
            Token::Number { value, .. } => {
                if input.try_parse(|i| i.expect_delim('/')).is_ok() {
                    let denominator = input.expect_number()?;
                    let text = format!("{} / {}", number(value), number(denominator));
                    return Ok((Self::Ratio(value, denominator), text));
                }
                Ok((Self::Number(value), number(value)))
            }
            Token::Ident(ident) => Ok((
                Self::Ident(match_ignore_ascii_case! { &ident,
                    "portrait" => Keyword::Portrait,
                    "landscape" => Keyword::Landscape,
                    "light" => Keyword::Light,
                    "dark" => Keyword::Dark,
                    "no-preference" => Keyword::NoPreference,
                    "reduce" => Keyword::Reduce,
                    "hover" => Keyword::Hover,
                    "none" => Keyword::None,
                    "fine" => Keyword::Fine,
                    "coarse" => Keyword::Coarse,
                    "browser" => Keyword::Browser,
                    _ => Keyword::Other,
                }),
                ident.to_ascii_lowercase(),
            )),
            _ => invalid(input),
        }
    }

    /// As a number to compare: a length in px, a resolution in dppx, a
    /// ratio's quotient.
    fn number(self, environment: &MediaEnvironment) -> Option<f32> {
        match self {
            Self::Length(value, kind) => Some(resolve(value, kind, environment)),
            Self::Resolution(value) | Self::Number(value) => Some(value),
            Self::Ratio(a, b) => Some(if b == 0.0 { f32::INFINITY } else { a / b }),
            Self::Ident(_) => None,
        }
    }
}

/// A `<length>` in px, or the viewport-relative value, if `unit` is one.
fn length(value: f32, unit: &str) -> Option<(f32, LengthKind)> {
    Some(match_ignore_ascii_case! { unit,
        "px" => (value, LengthKind::Px),
        "em" | "rem" => (value * FONT_SIZE, LengthKind::Px),
        "in" => (value * 96.0, LengthKind::Px),
        "cm" => (value * 96.0 / 2.54, LengthKind::Px),
        "mm" => (value * 96.0 / 25.4, LengthKind::Px),
        "q" => (value * 96.0 / 101.6, LengthKind::Px),
        "pt" => (value * 96.0 / 72.0, LengthKind::Px),
        "pc" => (value * 16.0, LengthKind::Px),
        "vw" => (value, LengthKind::Vw),
        "vh" => (value, LengthKind::Vh),
        "vmin" => (value, LengthKind::Vmin),
        "vmax" => (value, LengthKind::Vmax),
        _ => return None,
    })
}

fn resolve(value: f32, kind: LengthKind, environment: &MediaEnvironment) -> f32 {
    let (width, height) = (environment.width, environment.height);
    match kind {
        LengthKind::Px => value,
        LengthKind::Vw => value * width / 100.0,
        LengthKind::Vh => value * height / 100.0,
        LengthKind::Vmin => value * width.min(height) / 100.0,
        LengthKind::Vmax => value * width.max(height) / 100.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Comparison {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Comparison {
    fn holds(self, a: f32, b: f32) -> bool {
        match self {
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Eq => a == b,
            Self::Ge => a >= b,
            Self::Gt => a > b,
        }
    }

    /// The same comparison with its sides swapped: `5 < x` is `x > 5`.
    fn flipped(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Eq => Self::Eq,
            Self::Ge => Self::Le,
            Self::Gt => Self::Lt,
        }
    }

    fn to_css(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Eq => "=",
            Self::Ge => ">=",
            Self::Gt => ">",
        }
    }

    fn parse<'i>(input: &mut Parser<'i, '_>) -> Result<'i, Self> {
        let delim = match input.next_including_whitespace()? {
            Token::Delim(c) => *c,
            Token::WhiteSpace(_) => return Self::parse(input),
            _ => return invalid(input),
        };
        let or_equal = input
            .try_parse(|i| match i.next_including_whitespace()? {
                Token::Delim('=') => Ok(()),
                _ => Err(i.new_custom_error::<_, ()>(())),
            })
            .is_ok();
        Ok(match (delim, or_equal) {
            ('<', false) => Self::Lt,
            ('<', true) => Self::Le,
            ('>', false) => Self::Gt,
            ('>', true) => Self::Ge,
            ('=', false) => Self::Eq,
            _ => return invalid(input),
        })
    }
}

/// A `<media-feature>`: boolean, plain (with `min-` and `max-`), or a range.
#[derive(Debug, Clone, PartialEq)]
struct Feature {
    name: FeatureName,
    test: Test,
    /// As Chrome serializes it, parentheses and all.
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Test {
    /// `(name)`.
    Boolean,
    /// `(name: value)` and the range forms: the feature compared with each.
    Compare(Vec<(Comparison, Value)>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FeatureName {
    Width,
    Height,
    AspectRatio,
    Orientation,
    Resolution,
    PrefersColorScheme,
    PrefersReducedMotion,
    Hover,
    AnyHover,
    Pointer,
    AnyPointer,
    Color,
    Monochrome,
    Grid,
    DisplayMode,
    Unknown,
}

impl FeatureName {
    /// The name, and the comparison `min-` or `max-` makes of it.
    fn parse(name: &str) -> (Self, Option<Comparison>) {
        let lower = name.to_ascii_lowercase();
        let (prefix, base) = if let Some(base) = lower.strip_prefix("min-") {
            (Some(Comparison::Ge), base)
        } else if let Some(base) = lower.strip_prefix("max-") {
            (Some(Comparison::Le), base)
        } else if let Some(base) = lower.strip_prefix("-webkit-min-") {
            (Some(Comparison::Ge), base)
        } else if let Some(base) = lower.strip_prefix("-webkit-max-") {
            (Some(Comparison::Le), base)
        } else {
            (None, lower.strip_prefix("-webkit-").unwrap_or(&lower))
        };
        let feature = match base {
            "width" => Self::Width,
            "height" => Self::Height,
            "aspect-ratio" => Self::AspectRatio,
            "orientation" => Self::Orientation,
            // Chrome's `-webkit-device-pixel-ratio` is a resolution in
            // dppx, written as a number.
            "resolution" | "device-pixel-ratio" => Self::Resolution,
            "prefers-color-scheme" => Self::PrefersColorScheme,
            "prefers-reduced-motion" => Self::PrefersReducedMotion,
            "hover" => Self::Hover,
            "any-hover" => Self::AnyHover,
            "pointer" => Self::Pointer,
            "any-pointer" => Self::AnyPointer,
            "color" => Self::Color,
            "monochrome" => Self::Monochrome,
            "grid" => Self::Grid,
            "display-mode" => Self::DisplayMode,
            _ => Self::Unknown,
        };
        (feature, prefix)
    }

    /// Whether `min-` and `max-` and the range forms apply.
    fn is_range(self) -> bool {
        matches!(
            self,
            Self::Width
                | Self::Height
                | Self::AspectRatio
                | Self::Resolution
                | Self::Color
                | Self::Monochrome
                | Self::Unknown
        )
    }
}

impl Feature {
    fn parse<'i>(input: &mut Parser<'i, '_>) -> Result<'i, Self> {
        // `name`, `name: value`, `name op value`.
        if let Ok(written) = input.try_parse(|i| i.expect_ident().cloned()) {
            let written = written.to_ascii_lowercase();
            let (name, prefix) = FeatureName::parse(&written);
            if input.is_exhausted() {
                if prefix.is_some() {
                    return invalid(input);
                }
                return Ok(Self {
                    name,
                    test: Test::Boolean,
                    text: format!("({written})"),
                });
            }
            if input.try_parse(|i| i.expect_colon()).is_ok() {
                let (value, value_text) = Value::parse(input)?;
                if prefix.is_some() && !name.is_range() {
                    return invalid(input);
                }
                let comparison = prefix.unwrap_or(Comparison::Eq);
                return Ok(Self {
                    name,
                    test: Test::Compare(vec![(comparison, value)]),
                    text: format!("({written}: {value_text})"),
                });
            }
            if prefix.is_some() || !name.is_range() {
                return invalid(input);
            }
            let comparison = Comparison::parse(input)?;
            let (value, value_text) = Value::parse(input)?;
            return Ok(Self {
                name,
                test: Test::Compare(vec![(comparison, value)]),
                text: format!("({written} {} {value_text})", comparison.to_css()),
            });
        }
        // `value op name` and `value op name op value`.
        let (first, first_text) = Value::parse(input)?;
        let first_comparison = Comparison::parse(input)?;
        let written = input.expect_ident()?.to_ascii_lowercase();
        let (name, prefix) = FeatureName::parse(&written);
        if prefix.is_some() || !name.is_range() {
            return invalid(input);
        }
        let mut text = format!("({first_text} {} {written}", first_comparison.to_css());
        let mut tests = vec![(first_comparison.flipped(), first)];
        if !input.is_exhausted() {
            let second_comparison = Comparison::parse(input)?;
            // Both sides point the same way: `a < x < b`, `a > x > b`.
            let lower = matches!(first_comparison, Comparison::Lt | Comparison::Le);
            let second_lower = matches!(second_comparison, Comparison::Lt | Comparison::Le);
            if lower != second_lower
                || first_comparison == Comparison::Eq
                || second_comparison == Comparison::Eq
            {
                return invalid(input);
            }
            let (second, second_text) = Value::parse(input)?;
            text.push_str(&format!(" {} {second_text}", second_comparison.to_css()));
            tests.push((second_comparison, second));
        }
        text.push(')');
        Ok(Self {
            name,
            test: Test::Compare(tests),
            text,
        })
    }

    fn evaluate(&self, environment: &MediaEnvironment) -> Option<bool> {
        let actual = match self.name {
            FeatureName::Width => Actual::Number(environment.width),
            FeatureName::Height => Actual::Number(environment.height),
            FeatureName::AspectRatio => Actual::Number(if environment.height == 0.0 {
                f32::INFINITY
            } else {
                environment.width / environment.height
            }),
            FeatureName::Orientation => {
                Actual::Keyword(if environment.height >= environment.width {
                    Keyword::Portrait
                } else {
                    Keyword::Landscape
                })
            }
            FeatureName::Resolution => Actual::Number(environment.device_pixel_ratio),
            FeatureName::PrefersColorScheme => Actual::Keyword(Keyword::Light),
            FeatureName::PrefersReducedMotion => Actual::Keyword(Keyword::NoPreference),
            FeatureName::Hover | FeatureName::AnyHover => Actual::Keyword(Keyword::Hover),
            FeatureName::Pointer | FeatureName::AnyPointer => Actual::Keyword(Keyword::Fine),
            // Bits per color component, as Chrome on a usual screen.
            FeatureName::Color => Actual::Number(8.0),
            FeatureName::Monochrome | FeatureName::Grid => Actual::Number(0.0),
            FeatureName::DisplayMode => Actual::Keyword(Keyword::Browser),
            FeatureName::Unknown => return None,
        };
        match (&self.test, actual) {
            // Boolean context: true unless zero or `none`
            // (`no-preference` for the preferences).
            (Test::Boolean, Actual::Number(n)) => Some(n != 0.0),
            (Test::Boolean, Actual::Keyword(k)) => {
                Some(!matches!(k, Keyword::None | Keyword::NoPreference))
            }
            (Test::Compare(tests), Actual::Number(n)) => {
                let mut result = true;
                for (comparison, value) in tests {
                    // A value of the wrong kind makes the query invalid in
                    // Blink; here it is unknown.
                    let kind_ok = match (self.name, value) {
                        (FeatureName::Width | FeatureName::Height, Value::Length(..)) => true,
                        // A unitless 0 is a length too.
                        (FeatureName::Width | FeatureName::Height, Value::Number(v)) => *v == 0.0,
                        (FeatureName::Resolution, Value::Resolution(_)) => true,
                        // `-webkit-device-pixel-ratio: 2`.
                        (FeatureName::Resolution, Value::Number(_)) => true,
                        (FeatureName::AspectRatio, Value::Ratio(..) | Value::Number(_)) => true,
                        (
                            FeatureName::Color | FeatureName::Monochrome | FeatureName::Grid,
                            Value::Number(_),
                        ) => true,
                        _ => false,
                    };
                    let expected = value.number(environment)?;
                    if !kind_ok {
                        return None;
                    }
                    result &= comparison.holds(n, expected);
                }
                Some(result)
            }
            (Test::Compare(tests), Actual::Keyword(k)) => match tests.as_slice() {
                [(Comparison::Eq, Value::Ident(expected))] => Some(k == *expected),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Actual {
    Number(f32),
    Keyword(Keyword),
}

/// HTML's "parse a sizes attribute": the first size whose media condition
/// holds (or that has none), in CSS px; `None` if none does, for the
/// default (`100vw`). A size that doesn't parse is skipped, as is a
/// negative one, a percentage and `auto`; a math function's is clamped
/// to 0, unless it works out to NaN or an infinity, which is skipped too.
pub fn source_size(sizes: &str, environment: &MediaEnvironment) -> Option<f32> {
    let mut input = ParserInput::new(sizes);
    let mut input = Parser::new(&mut input);
    loop {
        let size = input
            .parse_until_before(Delimiter::Comma, |input| {
                // The value is the last component, the condition what is
                // before it: a lone `calc()` is a value, though it would
                // parse as an (unknown) condition too.
                let with_condition = input.try_parse(|input| {
                    let condition = MediaCondition::parse(input, true)?;
                    let value = size_value(input, environment)?;
                    input.expect_exhausted()?;
                    Ok((Some(condition), value))
                });
                if with_condition.is_ok() {
                    return with_condition;
                }
                let value = size_value(input, environment)?;
                input.expect_exhausted()?;
                Ok((None, value))
            })
            .ok();
        if let Some((condition, value)) = size
            && condition.is_none_or(|c| c.matches(environment))
        {
            return Some(value);
        }
        match input.next() {
            Ok(Token::Comma) => {}
            _ => return None,
        }
    }
}

/// A `<source-size-value>`: a non-negative `<length>` (a unitless 0 too).
fn size_value<'i>(input: &mut Parser<'i, '_>, environment: &MediaEnvironment) -> Result<'i, f32> {
    let viewport = gfx::SizeF::new(environment.width, environment.height);
    crate::values::parse_length_px(input, FONT_SIZE, viewport)
}

/// Whether `sizes` starts with `auto` (HTML's "allows auto-sizes", given a
/// lazy `<img>`): its first source size is the keyword, any case.
pub fn sizes_start_with_auto(sizes: &str) -> bool {
    let first = sizes.split(',').next().unwrap_or("");
    first
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .eq_ignore_ascii_case("auto")
}

/// The viewport's width, the default size (`100vw`).
pub fn default_source_size(environment: &MediaEnvironment) -> f32 {
    environment.width
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV: MediaEnvironment = MediaEnvironment {
        width: 800.0,
        height: 600.0,
        device_pixel_ratio: 2.0,
    };

    fn matches(query: &str) -> bool {
        MediaQueryList::parse(query).matches(&ENV)
    }

    #[test]
    fn media_query_lists() {
        for (query, expected) in [
            ("", true),
            ("all", true),
            ("screen", true),
            ("print", false),
            ("tv", false),
            ("not print", true),
            ("not screen", false),
            ("only screen and (min-width: 800px)", true),
            ("screen and (min-width: 801px)", false),
            ("(max-width: 799px), (orientation: landscape)", true),
            ("(max-width: 799px), print", false),
            ("bogus garbage", false),
            ("(width >= 800px)", true),
            ("(width > 800px)", false),
            ("(400px < width <= 800px)", true),
            ("(400px < width < 800px)", false),
            ("(800px >= width)", true),
            ("(min-resolution: 2dppx)", true),
            ("(min-resolution: 3x)", false),
            ("(resolution: 192dpi)", true),
            ("(-webkit-min-device-pixel-ratio: 1.5)", true),
            ("(prefers-color-scheme: dark)", false),
            ("(prefers-color-scheme: light)", true),
            ("(prefers-reduced-motion)", false),
            ("(hover)", true),
            ("(min-aspect-ratio: 4/3)", true),
            ("(min-aspect-ratio: 16/9)", false),
            ("(width: 50em)", true),
            ("(min-width: 100vw)", true),
            ("(min-width: 101vw)", false),
            // Unknown: false, `not` too.
            ("(fancy-feature)", false),
            ("not (fancy-feature)", false),
            ("not all and (fancy-feature)", false),
            ("(fancy-feature) or (min-width: 1px)", true),
            ("(width: 800)", false),
            ("not (width: 800px)", false),
            ("(min-width: 1px) and (max-width: 799px)", false),
            ("(min-width: 1px) and (max-width: 801px)", true),
            ("(min-width: 1px) and (max-width: 801px) or (hover)", false),
            // False `and` unknown is false, so `not` it is true; true `and`
            // unknown, or false `or` unknown, stays unknown (as Chrome).
            ("not ((max-width: 1px) and (fancy))", true),
            ("not ((min-width: 1px) and (fancy))", false),
            ("not ((max-width: 1px) or (fancy))", false),
            ("(min-width: 0) or (])", false),
        ] {
            assert_eq!(matches(query), expected, "{query}");
        }
        // A square viewport is portrait.
        let square = MediaEnvironment {
            width: 500.0,
            height: 500.0,
            device_pixel_ratio: 1.0,
        };
        assert!(MediaQueryList::parse("(orientation: portrait)").matches(&square));
        assert!(!MediaQueryList::parse("(orientation: landscape)").matches(&square));
    }

    #[test]
    fn sizes() {
        for (sizes, expected) in [
            ("", None),
            ("100px", Some(100.0)),
            ("(max-width: 500px) 100px, 50vw", Some(400.0)),
            ("(min-width: 500px) 10em, 50vw", Some(160.0)),
            ("bogus, 30px", Some(30.0)),
            ("-10px, 20px", Some(20.0)),
            ("50%, 20px", Some(20.0)),
            ("calc(10px + 1em), 20px", Some(26.0)),
            ("calc(50vw - 10px)", Some(390.0)),
            ("calc(-10px), 20px", Some(0.0)),
            ("calc(0px / 0), 20px", Some(20.0)),
            ("calc(-1px / 0), 20px", Some(20.0)),
            ("calc(1px / 0), 20px", Some(20.0)),
            ("10svw", Some(80.0)),
            ("auto, 20px", Some(20.0)),
            ("(max-width: 1px) 100px", None),
            ("0", Some(0.0)),
            ("(fancy) 5px, 6px", Some(6.0)),
            // A true `or` an unknown is true; a stray `]` in the unknown
            // one makes the condition invalid instead.
            ("(min-width: 0) or (fancy) 5px, 6px", Some(5.0)),
            ("(min-width: 0) or (]) 5px, 6px", Some(6.0)),
            ("(min-width: 0) or f(}) 5px, 6px", Some(6.0)),
            ("50vh", Some(300.0)),
        ] {
            assert_eq!(source_size(sizes, &ENV), expected, "{sizes:?}");
        }
    }
}
