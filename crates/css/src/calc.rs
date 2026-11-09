//! Math functions (css-values-4): `calc()`, `min()`, `max()`, `clamp()`,
//! the stepped value functions (`round()`, `mod()`, `rem()`), the sign
//! related ones (`abs()`, `sign()`), the trigonometric ones (`sin()` to
//! `atan2()`) and the exponential ones (`pow()`, `sqrt()`, `hypot()`,
//! `log()`, `exp()`), parsed into a tree, simplified as they are parsed,
//! serialized as CSSOM writes them, and computed for an element (Blink's
//! `CSSMathExpressionNode` and `CSSMathFunctionValue`).
//!
//! Simplified means: products by a number are multiplied in, sums add up
//! their terms of the same unit (absolute lengths in px), `min()`,
//! `max()` and `clamp()` whose arguments are all of one unit are worked
//! out, and so are the other functions whose arguments are all numbers or
//! dimensions in their canonical units (px, deg, s, dppx), as Chrome 152
//! works them out (not percentages or relative lengths: `round(7em,
//! 2em)` stays). What is left needs the element: font-relative lengths,
//! percentages, the functions of those, and products and quotients by a
//! number that is one of them (`10px * sign(1em)`).
//!
//! A computed length-percentage is a length plus a percentage (Blink's
//! `PixelsAndPercent`), so `min()`, `max()` and `clamp()` mixing lengths
//! and percentages, which only layout could work out, don't parse.

use std::cmp::Ordering;

use base::numerics::f32_from_f64;
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::values::{ComputeContext, LengthUnit, ParseResult, finite, write_number};

/// What a math function, or one of its parts, resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Number,
    Length,
    Percent,
    /// A sum of lengths and percentages.
    LengthPercent,
    Angle,
    Time,
    Resolution,
}

impl Kind {
    /// The kind of a sum of two parts; `None` if they can't be added.
    fn add(self, other: Self) -> Option<Self> {
        use Kind::*;
        match (self, other) {
            (a, b) if a == b => Some(a),
            (Number, _)
            | (_, Number)
            | (Angle, _)
            | (_, Angle)
            | (Time, _)
            | (_, Time)
            | (Resolution, _)
            | (_, Resolution) => None,
            _ => Some(LengthPercent),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Number(f32),
    Percent(f32),
    /// A length in an absolute unit is in px.
    Length(f32, LengthUnit),
    /// In degrees.
    Angle(f32),
    /// In seconds.
    Time(f32),
    /// In dppx.
    Resolution(f32),
    /// Terms, none of them a sum, at most one of each leaf unit.
    Sum(Vec<Node>),
    /// A node that can't take the number in (a math function), times it.
    Product(f32, Box<Node>),
    Min(Vec<Node>),
    Max(Vec<Node>),
    /// The minimum, the value, the maximum.
    Clamp(Box<[Node; 3]>),
    /// Any other math function, with the kind of its arguments (which
    /// have one; an angle is in degrees).
    Function(Function, Kind, Vec<Node>),
    /// A product by a number only an element works out (`10px *
    /// sign(1em)`): its factors, the leaves multiplied into one, first.
    Multiply(Vec<Node>),
    /// A quotient by such a number (`10px / sign(1em)`).
    Divide(Box<[Node; 2]>),
    /// A term a sum subtracts that isn't a leaf (`10px - 2 * min(1px,
    /// 1em)`): written after a ` - ` as it is (css-values-4's Negate node,
    /// as Chrome 152 keeps it), where a product with a negative number
    /// in it is added (`calc(1em + (-2 * min(1px, 1em)))`).
    Negate(Box<Node>),
}

/// The math functions besides `calc()`, `min()`, `max()` and `clamp()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Function {
    Round(Rounding),
    Mod,
    Rem,
    Abs,
    Sign,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Pow,
    Sqrt,
    Hypot,
    Log,
    Exp,
}

/// `round()`'s `<rounding-strategy>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rounding {
    Nearest,
    Up,
    Down,
    ToZero,
}

impl Function {
    fn name(self) -> &'static str {
        match self {
            Self::Round(_) => "round",
            Self::Mod => "mod",
            Self::Rem => "rem",
            Self::Abs => "abs",
            Self::Sign => "sign",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Asin => "asin",
            Self::Acos => "acos",
            Self::Atan => "atan",
            Self::Atan2 => "atan2",
            Self::Pow => "pow",
            Self::Sqrt => "sqrt",
            Self::Hypot => "hypot",
            Self::Log => "log",
            Self::Exp => "exp",
        }
    }

    /// The kind of its value, of arguments of `arg`.
    fn result(self, arg: Kind) -> Kind {
        match self {
            Self::Round(_) | Self::Mod | Self::Rem | Self::Abs | Self::Hypot => arg,
            Self::Asin | Self::Acos | Self::Atan | Self::Atan2 => Kind::Angle,
            _ => Kind::Number,
        }
    }

    /// Whether it takes arguments of `kind` (css-values-4's type checking:
    /// the trigonometric ones a number or an angle, the inverse and the
    /// exponential ones a number, the others any one kind).
    fn takes(self, kind: Kind) -> bool {
        match self {
            Self::Sin | Self::Cos | Self::Tan => matches!(kind, Kind::Number | Kind::Angle),
            Self::Asin
            | Self::Acos
            | Self::Atan
            | Self::Pow
            | Self::Sqrt
            | Self::Log
            | Self::Exp => kind == Kind::Number,
            _ => true,
        }
    }

    /// Its value, of arguments of `arg` (an angle in degrees, as is an
    /// angle it gives), with css-values-4's infinities, NaN and signed
    /// zeros.
    fn apply(self, arg: Kind, values: &[f64]) -> f64 {
        let a = values[0];
        let b = values.get(1).copied();
        let radians = |v: f64| {
            if arg == Kind::Angle {
                v.to_radians()
            } else {
                v
            }
        };
        match self {
            Self::Round(rounding) => round(rounding, a, b.unwrap_or(1.0)),
            Self::Mod => modulus(a, b.unwrap_or(f64::NAN), true),
            Self::Rem => modulus(a, b.unwrap_or(f64::NAN), false),
            Self::Abs => a.abs(),
            // `signum` is ±1 of a zero: a zero's sign is kept.
            Self::Sign => {
                if a == 0.0 || a.is_nan() {
                    a
                } else {
                    a.signum()
                }
            }
            Self::Sin => radians(a).sin(),
            Self::Cos => radians(a).cos(),
            // At its asymptotes, as Chrome 152's: ±infinity (an angle in
            // degrees there is exact; radians never are).
            Self::Tan => {
                let degrees = (arg == Kind::Angle).then(|| a.rem_euclid(360.0));
                if degrees == Some(90.0) {
                    f64::INFINITY
                } else if degrees == Some(270.0) {
                    f64::NEG_INFINITY
                } else {
                    radians(a).tan()
                }
            }
            Self::Asin => a.asin().to_degrees(),
            Self::Acos => a.acos().to_degrees(),
            Self::Atan => a.atan().to_degrees(),
            Self::Atan2 => a.atan2(b.unwrap_or(f64::NAN)).to_degrees(),
            Self::Pow => a.powf(b.unwrap_or(f64::NAN)),
            Self::Sqrt => a.sqrt(),
            Self::Hypot => values.iter().fold(0.0, |sum: f64, v| sum.hypot(*v)),
            Self::Log => match b {
                Some(base) => a.ln() / base.ln(),
                None => a.ln(),
            },
            Self::Exp => a.exp(),
        }
    }
}

/// `round(rounding, a, b)` (css-values-4): to a multiple of `b`; NaN by
/// 0 (as `a / 0` times 0 is) or of two infinities, an infinite `a` as it
/// is, by an infinite `b` 0 (or an infinity, rounding away from 0) with
/// `a`'s sign; a 0 it rounds up to is -0, one it rounds down to +0.
fn round(rounding: Rounding, a: f64, b: f64) -> f64 {
    if a.is_infinite() && b.is_infinite() {
        return f64::NAN;
    }
    if a.is_infinite() {
        return a;
    }
    if b.is_infinite() {
        let zero = if a.is_sign_negative() { -0.0 } else { 0.0 };
        return match rounding {
            Rounding::Up if a > 0.0 => f64::INFINITY,
            Rounding::Down if a < 0.0 => f64::NEG_INFINITY,
            _ => zero,
        };
    }
    let b = b.abs();
    let lower = (a / b).floor() * b;
    if lower == a {
        return a;
    }
    let upper = lower + b;
    let rounded = match rounding {
        Rounding::Up => upper,
        Rounding::Down => lower,
        Rounding::ToZero if a < 0.0 => upper,
        Rounding::ToZero => lower,
        // A tie goes up.
        Rounding::Nearest if a - lower < upper - a => lower,
        Rounding::Nearest => upper,
    };
    if rounded != 0.0 {
        rounded
    } else if rounded == upper {
        -0.0
    } else {
        0.0
    }
}

/// `mod(a, b)` (`floored`: with `b`'s sign) or `rem(a, b)` (with `a`'s),
/// css-values-4's: NaN by 0 or of an infinite `a`; by an infinite `b`,
/// `a` (`mod()`: NaN if their signs differ).
fn modulus(a: f64, b: f64, floored: bool) -> f64 {
    if b == 0.0 || a.is_infinite() {
        return f64::NAN;
    }
    if b.is_infinite() {
        return if floored && a.is_sign_negative() != b.is_sign_negative() {
            f64::NAN
        } else {
            a
        };
    }
    // `%` keeps `a`'s sign, a 0's too.
    let r = a % b;
    if !floored {
        r
    } else if r == 0.0 {
        0.0_f64.copysign(b)
    } else if (r < 0.0) != (b < 0.0) {
        r + b
    } else {
        r
    }
}

/// A math function as written, simplified: what a property's value holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Calc {
    pub(crate) node: Node,
    pub(crate) kind: Kind,
    /// The property takes no negative values: they are clamped to 0 when
    /// computed (css-values-4: a math function is never invalid for being
    /// out of range).
    pub(crate) non_negative: bool,
}

/// The math functions there are.
pub(crate) fn is_math_function(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "calc"
            | "min"
            | "max"
            | "clamp"
            | "round"
            | "mod"
            | "rem"
            | "abs"
            | "sign"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "pow"
            | "sqrt"
            | "hypot"
            | "log"
            | "exp"
    )
}

impl Calc {
    /// A math function (its name read, its arguments next) of one of the
    /// `allowed` kinds.
    pub(crate) fn parse_function<'i>(
        name: &str,
        input: &mut Parser<'i, '_>,
        allowed: &[Kind],
        non_negative: bool,
    ) -> ParseResult<'i, Self> {
        let name = name.to_ascii_lowercase();
        let (node, kind) = input.parse_nested_block(|input| parse_function_args(&name, input))?;
        if !allowed.contains(&kind) {
            return Err(input.new_custom_error(()));
        }
        Ok(Self {
            node,
            kind,
            non_negative,
        })
    }

    /// Its value for an element: a number, or px and a percentage.
    pub(crate) fn compute(&self, cx: &ComputeContext) -> Linear {
        let mut value = self.evaluate(cx);
        if self.non_negative && self.kind != Kind::LengthPercent {
            value.number = value.number.max(0.0);
            value.px = value.px.max(0.0);
            value.percent = value.percent.max(0.0);
        }
        value
    }

    /// Its value as worked out, before any range applies: infinite or NaN
    /// if it is.
    pub(crate) fn evaluate(&self, cx: &ComputeContext) -> Linear {
        evaluate(&self.node, cx)
    }

    /// As CSSOM writes it (css-values-4, "serialize a math function"): a
    /// function at the top as itself. (Chrome 152 writes an `atan2()` there
    /// in a `calc()`; machopo keeps none: an angle is worked out as parsed.)
    pub(crate) fn to_css(&self, dest: &mut String) {
        match &self.node {
            node @ (Node::Min(_) | Node::Max(_) | Node::Clamp(_)) => write_node(node, dest),
            node @ Node::Function(..) => write_node(node, dest),
            node => {
                dest.push_str("calc(");
                write_node(node, dest);
                dest.push(')');
            }
        }
    }
}

/// A computed math function: `number` for a number, `px` plus `percent`
/// of the basis for a length-percentage.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Linear {
    pub(crate) number: f32,
    pub(crate) px: f32,
    pub(crate) percent: f32,
}

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

fn parse_function_args<'i>(
    name: &str,
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, (Node, Kind)> {
    match name {
        "calc" => parse_sum(input),
        "min" | "max" => {
            let args = input.parse_comma_separated(parse_sum)?;
            let kind = same_kind(input, &args)?;
            let nodes = args.into_iter().map(|(node, _)| node).collect();
            let node = if name == "min" {
                min_max(nodes, true)
            } else {
                min_max(nodes, false)
            };
            Ok((node, kind))
        }
        "clamp" => {
            let args = input.parse_comma_separated(parse_sum)?;
            if args.len() != 3 {
                return invalid(input);
            }
            let kind = same_kind(input, &args)?;
            let mut nodes = args.into_iter().map(|(node, _)| node);
            let [lo, value, hi] = [(); 3].map(|()| nodes.next().expect("three"));
            Ok((clamp(lo, value, hi), kind))
        }
        "round" => {
            let rounding = input
                .try_parse(|input| {
                    let ident = input.expect_ident()?.clone();
                    let rounding = match_ignore_ascii_case! { &ident,
                        "nearest" => Rounding::Nearest,
                        "up" => Rounding::Up,
                        "down" => Rounding::Down,
                        "to-zero" => Rounding::ToZero,
                        _ => return invalid(input),
                    };
                    input.expect_comma()?;
                    Ok(rounding)
                })
                .unwrap_or(Rounding::Nearest);
            let mut args = input.parse_comma_separated(parse_sum)?;
            // A number's step is 1 if it isn't given.
            if args.len() == 1 && args[0].1 == Kind::Number {
                args.push((Node::Number(1.0), Kind::Number));
            }
            math_function(input, Function::Round(rounding), args, 2..=2)
        }
        _ => {
            let (function, arity) = match name {
                "mod" => (Function::Mod, 2..=2),
                "rem" => (Function::Rem, 2..=2),
                "abs" => (Function::Abs, 1..=1),
                "sign" => (Function::Sign, 1..=1),
                "sin" => (Function::Sin, 1..=1),
                "cos" => (Function::Cos, 1..=1),
                "tan" => (Function::Tan, 1..=1),
                "asin" => (Function::Asin, 1..=1),
                "acos" => (Function::Acos, 1..=1),
                "atan" => (Function::Atan, 1..=1),
                "atan2" => (Function::Atan2, 2..=2),
                "pow" => (Function::Pow, 2..=2),
                "sqrt" => (Function::Sqrt, 1..=1),
                "hypot" => (Function::Hypot, 1..=usize::MAX),
                "log" => (Function::Log, 1..=2),
                "exp" => (Function::Exp, 1..=1),
                _ => return invalid(input),
            };
            let args = input.parse_comma_separated(parse_sum)?;
            math_function(input, function, args, arity)
        }
    }
}

/// `function` of `args`, as many as `arity` allows, all of one kind it
/// takes: worked out if they are all leaves in canonical units (Chrome
/// 152 leaves the function of a percentage or a relative length, even
/// when the arguments have one unit), else kept.
fn math_function<'i>(
    input: &Parser<'i, '_>,
    function: Function,
    args: Vec<(Node, Kind)>,
    arity: std::ops::RangeInclusive<usize>,
) -> ParseResult<'i, (Node, Kind)> {
    if !arity.contains(&args.len()) {
        return invalid(input);
    }
    let kind = same_kind(input, &args)?;
    if !function.takes(kind) {
        return invalid(input);
    }
    let result = function.result(kind);
    let nodes: Vec<Node> = args.into_iter().map(|(node, _)| node).collect();
    if nodes.iter().all(is_canonical_leaf) {
        let values: Vec<f64> = nodes.iter().map(|node| f64::from(value_of(node))).collect();
        let value = f32_from_f64(function.apply(kind, &values));
        return Ok((canonical_leaf(result, value), result));
    }
    Ok((Node::Function(function, kind, nodes), result))
}

/// Whether `node` is a number, or a dimension in its canonical unit.
fn is_canonical_leaf(node: &Node) -> bool {
    matches!(
        node,
        Node::Number(_)
            | Node::Length(_, LengthUnit::Px)
            | Node::Angle(_)
            | Node::Time(_)
            | Node::Resolution(_)
    )
}

/// A leaf of `kind` (not a percentage) in its canonical unit.
fn canonical_leaf(kind: Kind, value: f32) -> Node {
    match kind {
        Kind::Length => Node::Length(value, LengthUnit::Px),
        Kind::Angle => Node::Angle(value),
        Kind::Time => Node::Time(value),
        Kind::Resolution => Node::Resolution(value),
        _ => Node::Number(value),
    }
}

/// The one kind of every argument of `min()`, `max()` or `clamp()`. A
/// length-percentage can't be compared without layout (see the module).
fn same_kind<'i>(input: &Parser<'i, '_>, args: &[(Node, Kind)]) -> ParseResult<'i, Kind> {
    let kind = args[0].1;
    if kind == Kind::LengthPercent || args.iter().any(|(_, k)| *k != kind) {
        return invalid(input);
    }
    Ok(kind)
}

/// `<calc-sum> = <calc-product> [ [ '+' | '-' ] <calc-product> ]*`, the
/// operators with white space on both sides.
fn parse_sum<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, (Node, Kind)> {
    let (first, mut kind) = parse_product(input)?;
    let mut terms = vec![first];
    loop {
        let operator = input.try_parse(|input| {
            input.expect_whitespace()?;
            let negate = match input.next_including_whitespace()? {
                Token::Delim('+') => false,
                Token::Delim('-') => true,
                _ => return invalid(input),
            };
            input.expect_whitespace()?;
            Ok(negate)
        });
        let Ok(subtract) = operator else {
            break;
        };
        let (term, term_kind) = parse_product(input)?;
        kind = match kind.add(term_kind) {
            Some(kind) => kind,
            None => return invalid(input),
        };
        terms.push(if subtract { negate(term) } else { term });
    }
    Ok((sum(terms), kind))
}

/// `node` subtracted: a leaf negated, each term of a sum, a term
/// subtracted twice added.
fn negate(node: Node) -> Node {
    match node {
        node if leaf_key(&node).is_some() => scale(node, -1.0),
        Node::Sum(terms) => sum(terms.into_iter().map(negate).collect()),
        Node::Negate(node) => *node,
        node => Node::Negate(Box::new(node)),
    }
}

/// `<calc-product> = <calc-value> [ [ '*' | '/' ] <calc-value> ]*`: one
/// side of each a number.
fn parse_product<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, (Node, Kind)> {
    let (mut node, mut kind) = parse_value(input)?;
    loop {
        let operator = input.try_parse(|input| match input.next()? {
            Token::Delim('*') => Ok(false),
            Token::Delim('/') => Ok(true),
            _ => invalid(input),
        });
        let Ok(divide) = operator else {
            break;
        };
        let (other, other_kind) = parse_value(input)?;
        (node, kind) = if divide {
            // Only by a number: by 0 is ±infinity (or NaN, of 0), as
            // css-values-4 has it.
            match other {
                Node::Number(n) => (scale(node, 1.0 / n), kind),
                other if other_kind == Kind::Number => {
                    (Node::Divide(Box::new([node, other])), kind)
                }
                _ => return invalid(input),
            }
        } else {
            // One side a number: a number worked out as it is parsed is
            // a lone `Node::Number`; one only an element works out (of a
            // relative length: `sign(1em)`) is kept as a factor.
            match (node, other) {
                (Node::Number(n), other) => (scale(other, n), other_kind),
                (node, Node::Number(n)) => (scale(node, n), kind),
                (node, other) if kind == Kind::Number || other_kind == Kind::Number => {
                    let kind = if kind == Kind::Number {
                        other_kind
                    } else {
                        kind
                    };
                    (multiply(node, other), kind)
                }
                _ => return invalid(input),
            }
        };
    }
    Ok((node, kind))
}

/// A number, a dimension, a percentage, a sum in parentheses, or a math
/// function.
fn parse_value<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, (Node, Kind)> {
    let location = input.current_source_location();
    let token = input.next()?.clone();
    Ok(match token {
        Token::Number { value, .. } => (Node::Number(finite(value)), Kind::Number),
        Token::Percentage { unit_value, .. } => {
            (Node::Percent(finite(unit_value * 100.0)), Kind::Percent)
        }
        Token::Dimension {
            value, ref unit, ..
        } => {
            let value = finite(value);
            if let Some(unit) = LengthUnit::from_name(unit) {
                (length(value, unit), Kind::Length)
            } else if let Some(degrees) = angle_degrees(value, unit) {
                (Node::Angle(degrees), Kind::Angle)
            } else if let Some(seconds) = time_seconds(value, unit) {
                (Node::Time(seconds), Kind::Time)
            } else if let Some(dppx) = resolution_dppx(value, unit) {
                (Node::Resolution(dppx), Kind::Resolution)
            } else {
                return Err(location.new_custom_error(()));
            }
        }
        Token::ParenthesisBlock => input.parse_nested_block(parse_sum)?,
        Token::Function(ref name) if is_math_function(name) => {
            let name = name.to_ascii_lowercase();
            input.parse_nested_block(|input| parse_function_args(&name, input))?
        }
        // css-values-4's constants: an infinite or NaN value goes on
        // through the math, and is clamped once computed.
        Token::Ident(ref ident) => {
            let value = match_ignore_ascii_case! { ident,
                "pi" => std::f32::consts::PI,
                "e" => std::f32::consts::E,
                "infinity" => f32::INFINITY,
                "-infinity" => f32::NEG_INFINITY,
                "nan" => f32::NAN,
                _ => return Err(location.new_custom_error(())),
            };
            (Node::Number(value), Kind::Number)
        }
        _ => return Err(location.new_custom_error(())),
    })
}

/// A length leaf: an absolute one in px.
fn length(value: f32, unit: LengthUnit) -> Node {
    match unit.absolute_px() {
        Some(px) if unit != LengthUnit::Number => Node::Length(value * px, LengthUnit::Px),
        _ => Node::Length(value, unit),
    }
}

/// `node` times `k`.
fn scale(node: Node, k: f32) -> Node {
    match node {
        Node::Number(v) => Node::Number(v * k),
        Node::Percent(v) => Node::Percent(v * k),
        Node::Length(v, unit) => Node::Length(v * k, unit),
        Node::Angle(v) => Node::Angle(v * k),
        Node::Time(v) => Node::Time(v * k),
        Node::Resolution(v) => Node::Resolution(v * k),
        Node::Sum(terms) => Node::Sum(terms.into_iter().map(|t| scale(t, k)).collect()),
        Node::Product(j, node) => product(j * k, *node),
        Node::Multiply(factors) => multiply_all(std::iter::once(Node::Number(k)).chain(factors)),
        Node::Negate(node) => Node::Negate(Box::new(scale(*node, k))),
        Node::Divide(parts) => {
            let [numerator, denominator] = *parts;
            Node::Divide(Box::new([scale(numerator, k), denominator]))
        }
        node => product(k, node),
    }
}

/// `a` times `b`, one of them a number only an element works out.
fn multiply(a: Node, b: Node) -> Node {
    multiply_all([a, b])
}

/// The product of `factors`, flattened: their leaves (and the numbers of
/// `Product`s) multiplied into one, first, as Chrome 152 writes it
/// (`calc(sign(1em) * 2 * 10px)` is `calc(20px * sign(1em))`).
fn multiply_all(factors: impl IntoIterator<Item = Node>) -> Node {
    let mut leaf: Option<Node> = None;
    let mut others = Vec::new();
    let mut take = |factor: Node, others: &mut Vec<Node>| {
        if leaf_key(&factor).is_none() {
            others.push(factor);
            return;
        }
        leaf = Some(match leaf.take() {
            None => factor,
            // At most one isn't a number: the other's value scales it.
            Some(Node::Number(n)) => scale(factor, n),
            Some(other) => scale(other, value_of(&factor)),
        });
    };
    for factor in factors {
        match factor {
            Node::Multiply(inner) => {
                for factor in inner {
                    take(factor, &mut others);
                }
            }
            Node::Product(k, node) => {
                take(Node::Number(k), &mut others);
                take(*node, &mut others);
            }
            factor => take(factor, &mut others),
        }
    }
    // A number 1 says nothing.
    let mut factors: Vec<Node> = leaf
        .filter(|leaf| *leaf != Node::Number(1.0) || others.is_empty())
        .into_iter()
        .collect();
    factors.extend(others);
    if factors.len() == 1 {
        return factors.pop().expect("one");
    }
    Node::Multiply(factors)
}

fn product(k: f32, node: Node) -> Node {
    if k == 1.0 {
        node
    } else {
        Node::Product(k, Box::new(node))
    }
}

/// Where a leaf goes among a sum's terms (css-values-4: numbers, then
/// percentages, then dimensions by unit); `None` for other nodes, which
/// follow in order.
fn leaf_key(node: &Node) -> Option<(u8, &'static str)> {
    match node {
        Node::Number(_) => Some((0, "")),
        Node::Percent(_) => Some((1, "")),
        Node::Length(_, unit) => Some((2, unit.name())),
        Node::Angle(_) => Some((2, "deg")),
        Node::Time(_) => Some((2, "s")),
        Node::Resolution(_) => Some((2, "dppx")),
        _ => None,
    }
}

/// The sum of `terms`, simplified: nested sums flattened, leaves of the
/// same unit added up; a single term is itself.
fn sum(terms: Vec<Node>) -> Node {
    let mut leaves: Vec<Node> = Vec::new();
    let mut others: Vec<Node> = Vec::new();
    let add = |term: Node, leaves: &mut Vec<Node>, others: &mut Vec<Node>| {
        let Some(key) = leaf_key(&term) else {
            others.push(term);
            return;
        };
        match leaves.iter_mut().find(|leaf| leaf_key(leaf) == Some(key)) {
            Some(Node::Number(v)) => *v += value_of(&term),
            Some(Node::Percent(v)) => *v += value_of(&term),
            Some(Node::Length(v, _))
            | Some(Node::Angle(v))
            | Some(Node::Time(v))
            | Some(Node::Resolution(v)) => *v += value_of(&term),
            _ => leaves.push(term),
        }
    };
    for term in terms {
        match term {
            Node::Sum(inner) => {
                for term in inner {
                    add(term, &mut leaves, &mut others);
                }
            }
            term => add(term, &mut leaves, &mut others),
        }
    }
    leaves.sort_by(|a, b| leaf_key(a).cmp(&leaf_key(b)));
    let mut terms = leaves;
    terms.extend(others);
    match terms.len() {
        0 => Node::Length(0.0, LengthUnit::Px),
        1 => terms.pop().expect("one"),
        _ => Node::Sum(terms),
    }
}

fn value_of(leaf: &Node) -> f32 {
    match *leaf {
        Node::Number(v)
        | Node::Percent(v)
        | Node::Length(v, _)
        | Node::Angle(v)
        | Node::Time(v)
        | Node::Resolution(v) => v,
        _ => unreachable!("a leaf"),
    }
}

/// `min()` (or `max()`) of `nodes`: worked out if they are leaves of one
/// unit, else kept.
fn min_max(nodes: Vec<Node>, min: bool) -> Node {
    let first = leaf_key(&nodes[0]);
    if first.is_some() && nodes.iter().all(|node| leaf_key(node) == first) {
        let pick = |a: f32, b: f32| if min { nan_min(a, b) } else { nan_max(a, b) };
        let best = nodes
            .iter()
            .map(value_of)
            .reduce(pick)
            .expect("an argument");
        return with_value(&nodes[0], best);
    }
    if nodes.len() == 1 {
        return nodes.into_iter().next().expect("one");
    }
    if min {
        Node::Min(nodes)
    } else {
        Node::Max(nodes)
    }
}

fn clamp(lo: Node, value: Node, hi: Node) -> Node {
    let key = leaf_key(&value);
    if key.is_some() && leaf_key(&lo) == key && leaf_key(&hi) == key {
        let v = nan_max(value_of(&lo), nan_min(value_of(&value), value_of(&hi)));
        return with_value(&value, v);
    }
    Node::Clamp(Box::new([lo, value, hi]))
}

/// A leaf like `leaf`, with `value`.
fn with_value(leaf: &Node, value: f32) -> Node {
    match *leaf {
        Node::Number(_) => Node::Number(value),
        Node::Percent(_) => Node::Percent(value),
        Node::Length(_, unit) => Node::Length(value, unit),
        Node::Angle(_) => Node::Angle(value),
        Node::Time(_) => Node::Time(value),
        Node::Resolution(_) => Node::Resolution(value),
        _ => unreachable!("a leaf"),
    }
}

fn evaluate(node: &Node, cx: &ComputeContext) -> Linear {
    match node {
        // An angle is a number of degrees, a time one of seconds, a
        // resolution one of dppx: nothing else mixes with them.
        Node::Number(v) | Node::Angle(v) | Node::Time(v) | Node::Resolution(v) => Linear {
            number: *v,
            ..Linear::default()
        },
        Node::Percent(v) => Linear {
            percent: *v,
            ..Linear::default()
        },
        // Clamped once the whole function is worked out, not here.
        Node::Length(v, unit) => Linear {
            px: crate::values::Length::Dimension {
                value: *v,
                unit: *unit,
            }
            .unclamped_px(cx),
            ..Linear::default()
        },
        Node::Sum(terms) => terms.iter().fold(Linear::default(), |a, term| {
            let b = evaluate(term, cx);
            Linear {
                number: a.number + b.number,
                px: a.px + b.px,
                percent: a.percent + b.percent,
            }
        }),
        Node::Product(k, node) => {
            let v = evaluate(node, cx);
            Linear {
                number: v.number * k,
                px: v.px * k,
                percent: v.percent * k,
            }
        }
        // The arguments are all of one kind, so one of the three is
        // what they differ in.
        Node::Min(nodes) | Node::Max(nodes) => {
            let min = matches!(node, Node::Min(_));
            nodes
                .iter()
                .map(|node| evaluate(node, cx))
                .reduce(|a, b| {
                    // NaN wins, as css-values-4's comparison has it: `a`
                    // does below (no order is taken as equal).
                    if key(b).is_nan() {
                        return b;
                    }
                    // Of two zeros, -0 is the smaller.
                    let (x, y) = (key(a), key(b));
                    let order = if x == 0.0 && y == 0.0 {
                        y.is_sign_negative().cmp(&x.is_sign_negative())
                    } else {
                        x.partial_cmp(&y).unwrap_or(Ordering::Equal)
                    };
                    let take_a = if min {
                        order != Ordering::Greater
                    } else {
                        order != Ordering::Less
                    };
                    if take_a { a } else { b }
                })
                .expect("an argument")
        }
        Node::Function(function, kind, args) => {
            let values: Vec<f64> = args
                .iter()
                .map(|arg| f64::from(key(evaluate(arg, cx))))
                .collect();
            let value = f32_from_f64(function.apply(*kind, &values));
            // Into the part its kind is in.
            match function.result(*kind) {
                Kind::Length => Linear {
                    px: value,
                    ..Linear::default()
                },
                Kind::Percent => Linear {
                    percent: value,
                    ..Linear::default()
                },
                _ => Linear {
                    number: value,
                    ..Linear::default()
                },
            }
        }
        Node::Multiply(factors) => factors
            .iter()
            .map(|factor| evaluate(factor, cx))
            .reduce(times)
            .expect("a factor"),
        Node::Negate(node) => {
            let v = evaluate(node, cx);
            Linear {
                number: -v.number,
                px: -v.px,
                percent: -v.percent,
            }
        }
        Node::Divide(parts) => {
            let [numerator, denominator] = &**parts;
            let by = key(evaluate(denominator, cx));
            times(
                evaluate(numerator, cx),
                Linear {
                    number: 1.0 / by,
                    ..Linear::default()
                },
            )
        }
        Node::Clamp(nodes) => {
            let [lo, value, hi] = &**nodes;
            let (lo, value, hi) = (evaluate(lo, cx), evaluate(value, cx), evaluate(hi, cx));
            let v = nan_max(key(lo), nan_min(key(value), key(hi)));
            // Back into whichever of the three it is.
            if lo.number != 0.0 || value.number != 0.0 || hi.number != 0.0 {
                Linear {
                    number: v,
                    ..Linear::default()
                }
            } else if lo.percent != 0.0 || value.percent != 0.0 || hi.percent != 0.0 {
                Linear {
                    percent: v,
                    ..Linear::default()
                }
            } else {
                Linear {
                    px: v,
                    ..Linear::default()
                }
            }
        }
    }
}

/// The product of two computed factors, one of them a number (only its
/// `number` part: an angle's, a time's or a resolution's too).
fn times(a: Linear, b: Linear) -> Linear {
    let (value, n) = if b.px == 0.0 && b.percent == 0.0 {
        (a, b.number)
    } else {
        (b, a.number)
    };
    Linear {
        number: value.number * n,
        px: value.px * n,
        percent: value.percent * n,
    }
}

/// The smaller of two numbers, NaN if either is (css-values-4's `min()`);
/// of two zeros, -0 if either is.
fn nan_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

/// The bigger of two numbers, NaN if either is; of two zeros, +0 if
/// either is.
fn nan_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() { a } else { b }
    } else {
        a.max(b)
    }
}

/// A number in a math function as CSSOM writes it: an infinite or NaN one
/// as its constant (css-values-4, "serialize a math function").
fn write_calc_number(dest: &mut String, value: f32) {
    if value.is_nan() {
        dest.push_str("NaN");
    } else if value == f32::INFINITY {
        dest.push_str("infinity");
    } else if value == f32::NEG_INFINITY {
        dest.push_str("-infinity");
    } else {
        write_number(dest, value.into());
    }
}

/// A value of `unit`: an infinite or NaN one as its constant times one of
/// the unit (`infinity * 1px`), as Chrome writes it.
fn write_dimension(dest: &mut String, value: f32, unit: &str) {
    if value.is_finite() {
        write_number(dest, value.into());
    } else {
        write_calc_number(dest, value);
        dest.push_str(" * 1");
    }
    dest.push_str(unit);
}

/// What a computed argument of one kind compares by: the part it is in
/// (the others are +0, and would make a -0 one +0).
fn key(v: Linear) -> f32 {
    let sum = v.number + v.px + v.percent;
    let negative_zero = |part: f32| part == 0.0 && part.is_sign_negative();
    if sum == 0.0 && [v.number, v.px, v.percent].into_iter().any(negative_zero) {
        -0.0
    } else {
        sum
    }
}

fn write_leaf(node: &Node, dest: &mut String) {
    match *node {
        Node::Number(v) => write_calc_number(dest, v),
        Node::Percent(v) => write_dimension(dest, v, "%"),
        Node::Length(v, unit) => write_dimension(dest, v, unit.name()),
        Node::Angle(v) => write_dimension(dest, v, "deg"),
        Node::Time(v) => write_dimension(dest, v, "s"),
        Node::Resolution(v) => write_dimension(dest, v, "dppx"),
        _ => unreachable!("a leaf"),
    }
}

/// Whether a sum writes `node` after a ` - `, negated: a negative leaf
/// (one it subtracts that isn't is a `Node::Negate`).
fn is_negative(node: &Node) -> bool {
    match node {
        Node::Number(v)
        | Node::Percent(v)
        | Node::Length(v, _)
        | Node::Angle(v)
        | Node::Time(v)
        | Node::Resolution(v) => *v < 0.0,
        _ => false,
    }
}

fn write_node(node: &Node, dest: &mut String) {
    match node {
        Node::Number(_)
        | Node::Percent(_)
        | Node::Length(..)
        | Node::Angle(_)
        | Node::Time(_)
        | Node::Resolution(_) => write_leaf(node, dest),
        Node::Sum(terms) => {
            // A product in a sum in parentheses, as Chrome 152 writes it:
            // `calc(1px + (2 * min(1em, 2px)))`.
            let write_term = |term: &Node, dest: &mut String| {
                if matches!(
                    term,
                    Node::Product(..) | Node::Multiply(_) | Node::Divide(_)
                ) {
                    dest.push('(');
                    write_node(term, dest);
                    dest.push(')');
                } else {
                    write_node(term, dest);
                }
            };
            for (i, term) in terms.iter().enumerate() {
                if let Node::Negate(term) = term
                    && i > 0
                {
                    dest.push_str(" - ");
                    write_term(term, dest);
                } else if i == 0 {
                    write_term(term, dest);
                } else if is_negative(term) {
                    dest.push_str(" - ");
                    write_term(&scale(term.clone(), -1.0), dest);
                } else {
                    dest.push_str(" + ");
                    write_term(term, dest);
                }
            }
        }
        Node::Product(k, node) => {
            write_calc_number(dest, *k);
            dest.push_str(" * ");
            write_node(node, dest);
        }
        Node::Min(nodes) | Node::Max(nodes) => {
            dest.push_str(if matches!(node, Node::Min(_)) {
                "min("
            } else {
                "max("
            });
            write_args(nodes, dest);
            dest.push(')');
        }
        Node::Clamp(nodes) => {
            dest.push_str("clamp(");
            write_args(&nodes[..], dest);
            dest.push(')');
        }
        Node::Function(function, _, args) => {
            dest.push_str(function.name());
            dest.push('(');
            // `nearest` is the one it has if none is given.
            let rounding = match function {
                Function::Round(Rounding::Up) => Some("up"),
                Function::Round(Rounding::Down) => Some("down"),
                Function::Round(Rounding::ToZero) => Some("to-zero"),
                _ => None,
            };
            if let Some(rounding) = rounding {
                dest.push_str(rounding);
                dest.push_str(", ");
            }
            write_args(args, dest);
            dest.push(')');
        }
        Node::Multiply(factors) => {
            for (i, factor) in factors.iter().enumerate() {
                if i > 0 {
                    dest.push_str(" * ");
                }
                write_factor(factor, dest);
            }
        }
        Node::Divide(parts) => {
            write_factor(&parts[0], dest);
            dest.push_str(" / ");
            write_factor(&parts[1], dest);
        }
        // Not after a ` - ` (the first term of a sum): as a product.
        Node::Negate(node) => write_node(&product(-1.0, (**node).clone()), dest),
    }
}

/// A factor of a product or quotient: a sum in parentheses.
fn write_factor(node: &Node, dest: &mut String) {
    if matches!(node, Node::Sum(_)) {
        dest.push('(');
        write_node(node, dest);
        dest.push(')');
    } else {
        write_node(node, dest);
    }
}

fn write_args(nodes: &[Node], dest: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        write_node(node, dest);
    }
}

/// A computed length-percentage with both parts, as Chrome writes it:
/// `calc(50% - 10px)`.
pub(crate) fn write_computed(px: f32, percent: f32, dest: &mut String) {
    dest.push_str("calc(");
    write_number(dest, percent.into());
    dest.push('%');
    dest.push_str(if px < 0.0 { " - " } else { " + " });
    write_number(dest, px.abs().into());
    dest.push_str("px)");
}

/// An angle unit's size in degrees.
fn angle_degrees(value: f32, unit: &str) -> Option<f32> {
    Some(match_ignore_ascii_case! { unit,
        "deg" => value,
        "grad" => value * 0.9,
        "rad" => value.to_degrees(),
        "turn" => value * 360.0,
        _ => return None,
    })
}

/// A resolution in dppx, of a resolution unit (css-values-4's
/// `<resolution>`: `x` is `dppx`).
pub(crate) fn resolution_dppx(value: f32, unit: &str) -> Option<f32> {
    Some(match_ignore_ascii_case! { unit,
        "dppx" | "x" => value,
        "dpi" => value / 96.0,
        "dpcm" => value * 2.54 / 96.0,
        _ => return None,
    })
}

/// A time unit's size in seconds.
fn time_seconds(value: f32, unit: &str) -> Option<f32> {
    Some(match_ignore_ascii_case! { unit,
        "s" => value,
        "ms" => value / 1000.0,
        _ => return None,
    })
}

/// A math function of `kind` whose value needs no element (numbers and
/// angles have no relative units): worked out as it is parsed. CSSOM then
/// writes the value, not the function (Chrome writes `calc(0.5)`).
fn parse_absolute<'i>(input: &mut Parser<'i, '_>, kind: Kind) -> ParseResult<'i, f32> {
    let name = input.expect_function()?.clone();
    if !is_math_function(&name) {
        return Err(input.new_custom_error(()));
    }
    let calc = Calc::parse_function(&name, input, &[kind], false)?;
    // A number of a relative length (`sign(1em)`), which only an element
    // works out, isn't one here (Chrome 152 keeps it, and works it out).
    if needs_element(&calc.node, true) {
        return Err(input.new_custom_error(()));
    }
    let value = calc.compute(&NO_ELEMENT).number;
    Ok(match kind {
        Kind::Angle => clamp_angle(value),
        _ => censor(value),
    })
}

/// Where a math function is worked out with no element: nothing in it
/// needs one ([`needs_element`]).
const NO_ELEMENT: ComputeContext<'static> = ComputeContext {
    font_size: 0.0,
    parent_font_size: 0.0,
    root_font_size: 0.0,
    viewport: None,
    system_colors: &gfx::SystemColors::DEFAULT,
    link_colors: crate::values::TextLinkColors::DEFAULT,
};

/// Whether working `node` out takes an element: a relative length is in
/// it, or (if `percentages`) a percentage.
fn needs_element(node: &Node, percentages: bool) -> bool {
    let any = |nodes: &[Node]| nodes.iter().any(|node| needs_element(node, percentages));
    match node {
        Node::Number(_) | Node::Angle(_) | Node::Time(_) | Node::Resolution(_) => false,
        Node::Length(_, unit) => *unit != LengthUnit::Px,
        Node::Percent(_) => percentages,
        Node::Product(_, node) | Node::Negate(node) => needs_element(node, percentages),
        Node::Sum(nodes)
        | Node::Min(nodes)
        | Node::Max(nodes)
        | Node::Multiply(nodes)
        | Node::Function(_, _, nodes) => any(nodes),
        Node::Clamp(nodes) => any(&nodes[..]),
        Node::Divide(parts) => any(&parts[..]),
    }
}

/// The biggest angle a math function computes to, either way, in degrees
/// (Blink's `kApproxDoubleInfinityAngle`, 2867080569122160, which Chrome
/// 152 gives `calc(infinity * 1deg)`): the f32 nearest it that is a
/// multiple of 360, so a rotation by it is none, as Chrome's is. Written,
/// it is Chrome's `2.86708e+15`.
const MAX_ANGLE: f32 = 2_867_079_917_076_480.0;

/// A math function's number, finite (Blink's `ClampToPermittedRange`):
/// NaN is 0, an infinity the biggest number there is; the property's own
/// range then applies.
fn censor(value: f32) -> f32 {
    if value.is_nan() { 0.0 } else { finite(value) }
}

/// A math function's angle, in degrees: NaN is 0, and it is no bigger
/// than [`MAX_ANGLE`] either way (Blink's `ClampAngle`).
fn clamp_angle(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-MAX_ANGLE, MAX_ANGLE)
    }
}

/// A `<number>` written as a math function (`calc(1 / 3)`).
pub(crate) fn parse_number<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    parse_absolute(input, Kind::Number)
}

/// An `<angle>` written as a math function, in degrees.
pub(crate) fn parse_angle<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    parse_absolute(input, Kind::Angle)
}

/// A `<time>` written as a math function, in seconds.
pub(crate) fn parse_time<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    parse_absolute(input, Kind::Time)
}

/// A `<resolution>` written as a math function, in dppx: negative if it
/// works out so (css-values-4 clamps it to the property's range once
/// computed).
pub(crate) fn parse_resolution<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    parse_absolute(input, Kind::Resolution)
}

/// A `<percentage>` written as a math function of percentages only, as a
/// number of percent (`calc(50% - 10%)` is 40).
pub(crate) fn parse_percentage<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    let name = input.expect_function()?.clone();
    if !is_math_function(&name) {
        return Err(input.new_custom_error(()));
    }
    let calc = Calc::parse_function(&name, input, &[Kind::Percent], false)?;
    match calc.node {
        Node::Percent(percent) => Ok(censor(percent)),
        // Of percentages only (`round(10%, 3%)`): a percentage of
        // percentages needs no element.
        ref node if !needs_element(node, false) => Ok(censor(calc.evaluate(&NO_ELEMENT).percent)),
        _ => Err(input.new_custom_error(())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The biggest angle is a whole number of turns as an f32, so a
    /// rotation by it is none (Chrome 152's `matrix(1, 0, 0, 1, 0, 0)`),
    /// and is written as Chrome writes it; NaN is 0, and finite angles past
    /// it are clamped too.
    #[test]
    fn the_biggest_angle_is_whole_turns() {
        assert_eq!(MAX_ANGLE % 360.0, 0.0);
        assert_eq!(gfx::Transform::rotate(MAX_ANGLE), gfx::Transform::IDENTITY);
        assert_eq!(gfx::Transform::rotate(-MAX_ANGLE), gfx::Transform::IDENTITY);
        let mut written = String::new();
        write_number(&mut written, MAX_ANGLE.into());
        assert_eq!(written, "2.86708e+15");
        let angle = |css: &str| {
            let mut input = cssparser::ParserInput::new(css);
            parse_angle(&mut Parser::new(&mut input)).unwrap()
        };
        assert_eq!(angle("calc(infinity * 1deg)"), MAX_ANGLE);
        assert_eq!(angle("calc(-infinity * 1turn)"), -MAX_ANGLE);
        assert_eq!(angle("calc(1e30 * 1deg)"), MAX_ANGLE);
        assert_eq!(angle("calc(NaN * 1deg)"), 0.0);
        assert_eq!(angle("calc(90deg)"), 90.0);
    }
}
