//! The animation properties' values: `<time>`, `<easing-function>`
//! (css-easing 2), `transition-property` and `transition-behavior`, the
//! `transition` shorthand (css-transitions 1), the `animation-*` values
//! and the `animation` shorthand (css-animations 1), each parsed and
//! written as Chrome writes it (Blink's `CSSParsingUtils::ConsumeAnimationValue`
//! and `ComputedStyleUtils::ValueForAnimation*`).

use std::sync::Arc;

use base::numerics::saturated_cast;
use computed_style::{
    AnimationName, AnimationPlayState, AnimationRangeBoundary, AnimationTimeline, EasingFunction,
    FillMode, LinearStop, PlaybackDirection, StepPosition, TransitionBehavior, TransitionProperty,
};
use cssparser::{Parser, ParserInput, Token, match_ignore_ascii_case};

use crate::properties::{LonghandId, Value};
use crate::values::{ParseResult, parse_number, parse_number_at_least, write_number};

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// A `<time>` as declared: Chrome writes it back in its own unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecifiedTime {
    seconds: f32,
    unit: TimeUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeUnit {
    Seconds,
    Milliseconds,
}

impl SpecifiedTime {
    pub(crate) const ZERO: Self = Self {
        seconds: 0.0,
        unit: TimeUnit::Seconds,
    };

    pub fn seconds(self) -> f32 {
        self.seconds
    }

    pub(crate) fn to_css(self, dest: &mut String) {
        match self.unit {
            TimeUnit::Seconds => {
                write_number(dest, self.seconds.into());
                dest.push('s');
            }
            TimeUnit::Milliseconds => {
                write_number(dest, f64::from(self.seconds) * 1000.0);
                dest.push_str("ms");
            }
        }
    }
}

/// `<time>`; with `non_negative`, a negative one is invalid, and a math
/// function's is clamped to 0 (css-values-4).
pub(crate) fn parse_time<'i>(
    input: &mut Parser<'i, '_>,
    non_negative: bool,
) -> ParseResult<'i, SpecifiedTime> {
    if let Ok(seconds) = input.try_parse(crate::calc::parse_time) {
        let seconds = if non_negative {
            seconds.max(0.0)
        } else {
            seconds
        };
        return Ok(SpecifiedTime {
            seconds,
            unit: TimeUnit::Seconds,
        });
    }
    let location = input.current_source_location();
    let (value, unit) = match input.next()? {
        Token::Dimension { value, unit, .. } => (*value, unit.clone()),
        _ => return Err(location.new_custom_error(())),
    };
    let time = match_ignore_ascii_case! { &unit,
        "s" => SpecifiedTime { seconds: value, unit: TimeUnit::Seconds },
        "ms" => SpecifiedTime { seconds: value / 1000.0, unit: TimeUnit::Milliseconds },
        _ => return Err(location.new_custom_error(())),
    };
    if non_negative && time.seconds < 0.0 {
        return Err(location.new_custom_error(()));
    }
    Ok(time)
}

/// A computed time: in seconds, as Chrome writes it.
pub(crate) fn seconds_to_css(seconds: f32, dest: &mut String) {
    write_number(dest, seconds.into());
    dest.push('s');
}

/// `<easing-function>` as declared: what it computes to, but for
/// `linear()`, whose control points are written back as they were given.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedEasing {
    Easing(EasingFunction),
    Linear(Arc<[SpecifiedLinearStop]>),
}

/// One argument of `linear()`: an output progress, with no, one or two
/// input progresses.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedLinearStop {
    output: f32,
    inputs: Vec<StopInput>,
}

/// A percentage, and whether it was a math function (Chrome writes it
/// back as `calc()`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct StopInput {
    percent: f64,
    calc: bool,
}

impl SpecifiedEasing {
    pub(crate) const EASE: Self = Self::Easing(EasingFunction::Ease);

    pub fn compute(&self) -> EasingFunction {
        match self {
            Self::Easing(easing) => easing.clone(),
            Self::Linear(stops) => EasingFunction::LinearPoints(linear_points(stops)),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Easing(easing) => easing_to_css(easing, dest),
            Self::Linear(stops) => {
                dest.push_str("linear(");
                for (i, stop) in stops.iter().enumerate() {
                    if i > 0 {
                        dest.push_str(", ");
                    }
                    write_number(dest, stop.output.into());
                    for input in &stop.inputs {
                        dest.push(' ');
                        if input.calc {
                            dest.push_str("calc(");
                        }
                        write_number(dest, input.percent);
                        dest.push('%');
                        if input.calc {
                            dest.push(')');
                        }
                    }
                }
                dest.push(')');
            }
        }
    }
}

/// `linear()`'s control points with every input filled in (css-easing 2,
/// "create a linear easing function"): the first at 0, the last at 1 (or
/// the largest before it), each at least the one before, and those
/// missing spaced evenly between the ones around them.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`linear()` has two stops at least, each a point: `points` isn't empty; a run \
              of missing inputs starts after the first point, whose input is set, and its \
              indices are below `points.len()`"
)]
fn linear_points(stops: &[SpecifiedLinearStop]) -> Arc<[LinearStop]> {
    let mut points: Vec<(f64, Option<f64>)> = Vec::new();
    for stop in stops {
        if stop.inputs.is_empty() {
            points.push((stop.output.into(), None));
        }
        for input in &stop.inputs {
            points.push((stop.output.into(), Some(input.percent / 100.0)));
        }
    }
    let last = points.len() - 1;
    if points[0].1.is_none() {
        points[0].1 = Some(0.0);
    }
    if points[last].1.is_none() {
        let largest = points
            .iter()
            .filter_map(|(_, input)| *input)
            .fold(1.0_f64, f64::max);
        points[last].1 = Some(largest);
    }
    let mut largest = f64::NEG_INFINITY;
    for point in &mut points {
        if let Some(input) = &mut point.1 {
            *input = input.max(largest);
            largest = *input;
        }
    }
    let mut i = 0;
    while i < points.len() {
        if points[i].1.is_some() {
            i += 1;
            continue;
        }
        // A run of missing inputs, between two known ones.
        let start = i - 1;
        let end = (i..points.len())
            .find(|&j| points[j].1.is_some())
            .expect("the last is known");
        let (from, to) = (points[start].1.unwrap(), points[end].1.unwrap());
        let steps = (end - start) as f64;
        for (k, point) in points[start + 1..end].iter_mut().enumerate() {
            point.1 = Some(from + (to - from) * (k + 1) as f64 / steps);
        }
        i = end;
    }
    points
        .into_iter()
        .map(|(output, input)| LinearStop {
            output,
            input: input.expect("filled in"),
        })
        .collect()
}

/// `<easing-function>`: a keyword, `cubic-bezier()`, `steps()` or
/// `linear()`.
/// How `easing` serializes (web-animations 1's `getTiming()`).
pub fn easing_text(easing: &EasingFunction) -> String {
    let mut css = String::new();
    easing_to_css(easing, &mut css);
    css
}

/// An easing function written by itself (web-animations 1's `easing`
/// strings): the function, and how it serializes; `None` if `text` is
/// none.
pub fn parse_easing_text(text: &str) -> Option<(EasingFunction, String)> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let easing = parser.parse_entirely(|input| parse_easing(input)).ok()?;
    let mut css = String::new();
    easing.to_css(&mut css);
    Some((easing.compute(), css))
}

pub(crate) fn parse_easing<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedEasing> {
    let location = input.current_source_location();
    let token = input.next()?.clone();
    let easing = match token {
        Token::Ident(ref ident) => match_ignore_ascii_case! { ident,
            "linear" => EasingFunction::Linear,
            "ease" => EasingFunction::Ease,
            "ease-in" => EasingFunction::EaseIn,
            "ease-out" => EasingFunction::EaseOut,
            "ease-in-out" => EasingFunction::EaseInOut,
            "step-start" => EasingFunction::Steps(1, StepPosition::Start),
            "step-end" => EasingFunction::Steps(1, StepPosition::End),
            _ => return Err(location.new_custom_error(())),
        },
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            return input.parse_nested_block(|input| match name.as_str() {
                "cubic-bezier" => parse_cubic_bezier(input),
                "steps" => parse_steps(input),
                "linear" => parse_linear(input),
                _ => invalid(input),
            });
        }
        _ => return Err(location.new_custom_error(())),
    };
    Ok(SpecifiedEasing::Easing(easing))
}

/// `cubic-bezier(x1, y1, x2, y2)`, the xs in 0 to 1: outside it a number
/// is invalid, a math function's is clamped.
fn parse_cubic_bezier<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedEasing> {
    let mut points = [0.0; 4];
    for (i, point) in points.iter_mut().enumerate() {
        if i > 0 {
            input.expect_comma()?;
        }
        let is_x = i % 2 == 0;
        if let Ok(value) = input.try_parse(crate::calc::parse_number) {
            let value = f64::from(value);
            *point = if is_x { value.clamp(0.0, 1.0) } else { value };
            continue;
        }
        let start = input.position();
        let value = input.expect_number()?;
        let value = as_written(input.slice_from(start), ' ').unwrap_or(value.into());
        if is_x && !(0.0..=1.0).contains(&value) {
            return invalid(input);
        }
        *point = value;
    }
    Ok(SpecifiedEasing::Easing(EasingFunction::CubicBezier(points)))
}

/// `steps(<integer>, <step-position>?)`: at least 1 step, 2 for
/// `jump-none`, at most `i32::MAX` (Blink's integer). A math function's
/// count is rounded and clamped to that, but as in Chrome one too few for
/// `jump-none` makes it invalid (NaN too: it is 0 by then).
fn parse_steps<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedEasing> {
    let calc = input.try_parse(crate::calc::parse_number).ok();
    let integer = match calc {
        Some(_) => None,
        None => Some(input.expect_integer()?),
    };
    let position = if input.try_parse(|i| i.expect_comma()).is_ok() {
        let location = input.current_source_location();
        let ident = input.expect_ident()?;
        match_ignore_ascii_case! { ident,
            "jump-start" => StepPosition::JumpStart,
            "jump-end" => StepPosition::JumpEnd,
            "jump-none" => StepPosition::JumpNone,
            "jump-both" => StepPosition::JumpBoth,
            "start" => StepPosition::Start,
            "end" => StepPosition::End,
            _ => return Err(location.new_custom_error(())),
        }
    } else {
        StepPosition::End
    };
    let least = if position == StepPosition::JumpNone {
        2
    } else {
        1
    };
    let count = match (calc, integer) {
        (Some(value), _) => {
            let rounded = (value + 0.5).floor();
            if position == StepPosition::JumpNone && rounded < 2.0 {
                return invalid(input);
            }
            saturated_cast::<i32, _>(rounded).max(least)
        }
        (None, Some(count)) if count >= least => count,
        (None, _) => return invalid(input),
    };
    Ok(SpecifiedEasing::Easing(EasingFunction::Steps(
        count.unsigned_abs(),
        position,
    )))
}

/// `linear([ <number> && <percentage>{0,2} ]#)`, at least two of them.
fn parse_linear<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedEasing> {
    let stops = input.parse_comma_separated(|input| {
        // The percentages come together, before or after the number.
        let inputs = |input: &mut Parser<'i, '_>| {
            let mut inputs = Vec::new();
            while inputs.len() < 2
                && let Ok(stop) = input.try_parse(parse_stop_input)
            {
                inputs.push(stop);
            }
            inputs
        };
        let before = inputs(input);
        let output = parse_number(input)?;
        let inputs = if before.is_empty() {
            inputs(input)
        } else {
            before
        };
        Ok(SpecifiedLinearStop { output, inputs })
    })?;
    if stops.len() < 2 {
        return invalid(input);
    }
    Ok(SpecifiedEasing::Linear(stops.into()))
}

fn parse_stop_input<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, StopInput> {
    if let Ok(percent) = input.try_parse(crate::calc::parse_percentage) {
        return Ok(StopInput {
            percent: percent.into(),
            calc: true,
        });
    }
    let start = input.position();
    let fraction = input.expect_percentage()?;
    Ok(StopInput {
        percent: as_written(input.slice_from(start), '%').unwrap_or(f64::from(fraction) * 100.0),
        calc: false,
    })
}

/// A number token's value as written (`text`, without `unit`), in `f64`:
/// `cssparser` keeps numbers as `f32`, which holds neither `60%` as 0.6
/// nor `0.23333333333333334` closely enough for an easing (Blink parses
/// doubles). `None` if the text isn't a plain number.
fn as_written(text: &str, unit: char) -> Option<f64> {
    let text = text.trim();
    text.strip_suffix(unit).unwrap_or(text).parse().ok()
}

/// An easing function as Chrome writes it: a keyword, `steps(n)` for the
/// end positions and `steps(n, position)` for the others.
pub(crate) fn easing_to_css(easing: &EasingFunction, dest: &mut String) {
    match easing {
        EasingFunction::Linear => dest.push_str("linear"),
        EasingFunction::Ease => dest.push_str("ease"),
        EasingFunction::EaseIn => dest.push_str("ease-in"),
        EasingFunction::EaseOut => dest.push_str("ease-out"),
        EasingFunction::EaseInOut => dest.push_str("ease-in-out"),
        EasingFunction::CubicBezier(points) => {
            dest.push_str("cubic-bezier(");
            for (i, point) in points.iter().enumerate() {
                if i > 0 {
                    dest.push_str(", ");
                }
                write_number(dest, *point);
            }
            dest.push(')');
        }
        EasingFunction::Steps(count, position) => {
            dest.push_str("steps(");
            dest.push_str(&count.to_string());
            let position = match position {
                StepPosition::End | StepPosition::JumpEnd => None,
                StepPosition::Start => Some("start"),
                StepPosition::JumpStart => Some("jump-start"),
                StepPosition::JumpNone => Some("jump-none"),
                StepPosition::JumpBoth => Some("jump-both"),
            };
            if let Some(position) = position {
                dest.push_str(", ");
                dest.push_str(position);
            }
            dest.push(')');
        }
        EasingFunction::LinearPoints(points) => {
            dest.push_str("linear(");
            for (i, point) in points.iter().enumerate() {
                if i > 0 {
                    dest.push_str(", ");
                }
                write_number(dest, point.output);
                dest.push(' ');
                write_number(dest, point.input * 100.0);
                dest.push('%');
            }
            dest.push(')');
        }
    }
}

/// One item of `transition-property`: `all`, or a property's name. The
/// CSS-wide keywords and `default` aren't names; `none` is the whole
/// list's value only.
fn parse_transition_property_item<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TransitionProperty> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?.clone();
    match_ignore_ascii_case! { &ident,
        "all" => Ok(TransitionProperty::All),
        "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" =>
            Err(location.new_custom_error(())),
        _ => Ok(TransitionProperty::Name(Arc::from(&*ident))),
    }
}

/// `none | <single-transition-property>#`.
pub(crate) fn parse_transition_property<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[TransitionProperty]>> {
    if input
        .try_parse(|i| {
            i.expect_ident_matching("none")?;
            i.expect_exhausted()
        })
        .is_ok()
    {
        return Ok(Arc::new([TransitionProperty::None]));
    }
    Ok(input
        .parse_comma_separated(parse_transition_property_item)?
        .into())
}

pub(crate) fn transition_property_to_css(property: &TransitionProperty, dest: &mut String) {
    match property {
        TransitionProperty::None => dest.push_str("none"),
        TransitionProperty::All => dest.push_str("all"),
        TransitionProperty::Name(name) => dest.push_str(name),
    }
}

pub(crate) fn parse_transition_behavior<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TransitionBehavior> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    match_ignore_ascii_case! { ident,
        "normal" => Ok(TransitionBehavior::Normal),
        "allow-discrete" => Ok(TransitionBehavior::AllowDiscrete),
        _ => Err(location.new_custom_error(())),
    }
}

pub(crate) fn transition_behavior_name(behavior: TransitionBehavior) -> &'static str {
    match behavior {
        TransitionBehavior::Normal => "normal",
        TransitionBehavior::AllowDiscrete => "allow-discrete",
    }
}

/// `transition`'s longhands, in the order Blink's parser sets them in
/// (`transitionShorthandForParsing`), which a declaration block keeps
/// them in.
pub(crate) const TRANSITION: [LonghandId; 5] = [
    LonghandId::TransitionBehavior,
    LonghandId::TransitionDuration,
    LonghandId::TransitionTimingFunction,
    LonghandId::TransitionDelay,
    LonghandId::TransitionProperty,
];

/// `transition`'s longhands as one value for them all sets them (see
/// `Shorthand::expanded_longhands`).
pub(crate) const TRANSITION_EXPANDED: [LonghandId; 5] = [
    LonghandId::TransitionProperty,
    LonghandId::TransitionDuration,
    LonghandId::TransitionTimingFunction,
    LonghandId::TransitionDelay,
    LonghandId::TransitionBehavior,
];

/// One `<single-transition>`: `[ none | <single-transition-property> ] ||
/// <time> || <easing-function> || <time> || <transition-behavior-value>`,
/// the first time the duration.
struct SingleTransition {
    property: Option<TransitionProperty>,
    duration: Option<SpecifiedTime>,
    easing: Option<SpecifiedEasing>,
    delay: Option<SpecifiedTime>,
    behavior: Option<TransitionBehavior>,
}

fn parse_single_transition<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SingleTransition> {
    let mut single = SingleTransition {
        property: None,
        duration: None,
        easing: None,
        delay: None,
        behavior: None,
    };
    loop {
        if single.duration.is_none() || single.delay.is_none() {
            let non_negative = single.duration.is_none();
            if let Ok(time) = input.try_parse(|i| parse_time(i, non_negative)) {
                if single.duration.is_none() {
                    single.duration = Some(time);
                } else {
                    single.delay = Some(time);
                }
                continue;
            }
        }
        if single.easing.is_none()
            && let Ok(easing) = input.try_parse(parse_easing)
        {
            single.easing = Some(easing);
            continue;
        }
        if single.behavior.is_none()
            && let Ok(behavior) = input.try_parse(parse_transition_behavior)
        {
            single.behavior = Some(behavior);
            continue;
        }
        if single.property.is_none() {
            if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
                single.property = Some(TransitionProperty::None);
                continue;
            }
            if let Ok(property) = input.try_parse(parse_transition_property_item) {
                single.property = Some(property);
                continue;
            }
        }
        break;
    }
    if single.property.is_none()
        && single.duration.is_none()
        && single.easing.is_none()
        && single.behavior.is_none()
    {
        return invalid(input);
    }
    Ok(single)
}

/// `transition`: `<single-transition>#`, `none` only alone.
pub(crate) fn parse_transition<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    let transitions = input.parse_comma_separated(parse_single_transition)?;
    if transitions.len() > 1
        && transitions
            .iter()
            .any(|t| t.property == Some(TransitionProperty::None))
    {
        return invalid(input);
    }
    let properties: Arc<[TransitionProperty]> = transitions
        .iter()
        .map(|t| t.property.clone().unwrap_or(TransitionProperty::All))
        .collect();
    let durations: Arc<[SpecifiedTime]> = transitions
        .iter()
        .map(|t| t.duration.unwrap_or(SpecifiedTime::ZERO))
        .collect();
    let easings: Arc<[SpecifiedEasing]> = transitions
        .iter()
        .map(|t| t.easing.clone().unwrap_or(SpecifiedEasing::EASE))
        .collect();
    let delays: Arc<[SpecifiedTime]> = transitions
        .iter()
        .map(|t| t.delay.unwrap_or(SpecifiedTime::ZERO))
        .collect();
    let behaviors: Arc<[TransitionBehavior]> = transitions
        .iter()
        .map(|t| t.behavior.unwrap_or(TransitionBehavior::Normal))
        .collect();
    Ok(vec![
        Value::TransitionBehaviors(behaviors),
        Value::Times(durations),
        Value::Easings(easings),
        Value::Times(delays),
        Value::TransitionProperties(properties),
    ])
}

/// `transition` written from its longhands (Blink's
/// `StylePropertySerializer::GetLayeredShorthandValue` for it): each
/// transition's values but those at their initial value, the duration
/// kept when there is a delay; `all` for one with nothing to write.
/// `None` unless every list has the same length.
pub(crate) fn transition_to_css(values: &[(&Value, bool)]) -> Option<String> {
    let [
        (Value::TransitionBehaviors(behaviors), _),
        (Value::Times(durations), _),
        (Value::Easings(easings), _),
        (Value::Times(delays), _),
        (Value::TransitionProperties(properties), _),
    ] = values
    else {
        return None;
    };
    let n = properties.len();
    if [
        durations.len(),
        easings.len(),
        delays.len(),
        behaviors.len(),
    ]
    .iter()
    .any(|&len| len != n)
    {
        return None;
    }
    let mut items = Vec::new();
    for i in 0..n {
        let mut parts = Vec::new();
        if properties[i] != TransitionProperty::All {
            let mut text = String::new();
            transition_property_to_css(&properties[i], &mut text);
            parts.push(text);
        }
        let delayed = delays[i].seconds() != 0.0;
        if durations[i].seconds() != 0.0 || delayed {
            let mut text = String::new();
            durations[i].to_css(&mut text);
            parts.push(text);
        }
        if easings[i] != SpecifiedEasing::EASE {
            let mut text = String::new();
            easings[i].to_css(&mut text);
            parts.push(text);
        }
        if delayed {
            let mut text = String::new();
            delays[i].to_css(&mut text);
            parts.push(text);
        }
        if behaviors[i] != TransitionBehavior::Normal {
            parts.push(transition_behavior_name(behaviors[i]).to_owned());
        }
        if parts.is_empty() {
            parts.push("all".to_owned());
        }
        items.push(parts.join(" "));
    }
    Some(items.join(", "))
}

/// The CSS-wide keywords and `default`: never a `<custom-ident>`.
pub(crate) fn is_reserved(ident: &str) -> bool {
    match_ignore_ascii_case! { ident,
        "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" => true,
        _ => false,
    }
}

/// `none | <keyframes-name>`: `<custom-ident> | <string>`, the string not
/// empty.
pub(crate) fn parse_animation_name<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, AnimationName> {
    let location = input.current_source_location();
    match input.next()? {
        Token::Ident(ident) if ident.eq_ignore_ascii_case("none") => Ok(AnimationName::None),
        Token::Ident(ident) if !is_reserved(ident) => Ok(AnimationName::Name(Arc::from(&**ident))),
        Token::QuotedString(name) if !name.is_empty() => {
            Ok(AnimationName::Name(Arc::from(&**name)))
        }
        _ => Err(location.new_custom_error(())),
    }
}

/// An identifier, or a string where the identifier would read as a
/// keyword (`"none"`, `"initial"`), as Chrome writes a `<keyframes-name>`.
pub(crate) fn animation_name_to_css(name: &AnimationName, dest: &mut String) {
    match name {
        AnimationName::None => dest.push_str("none"),
        AnimationName::Name(name) if name.eq_ignore_ascii_case("none") || is_reserved(name) => {
            cssparser::serialize_string(name, dest).expect("writing to a String");
        }
        AnimationName::Name(name) => {
            cssparser::serialize_identifier(name, dest).expect("writing to a String");
        }
    }
}

/// `auto | <time [0s,∞]>`: `None` is `auto`.
pub(crate) fn parse_animation_duration<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Option<SpecifiedTime>> {
    if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
        return Ok(None);
    }
    Ok(Some(parse_time(input, true)?))
}

pub(crate) fn animation_duration_to_css(duration: &Option<SpecifiedTime>, dest: &mut String) {
    match duration {
        None => dest.push_str("auto"),
        Some(time) => time.to_css(dest),
    }
}

/// `infinite | <number [0,∞]>`: infinity is `infinite`.
pub(crate) fn parse_iteration_count<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    if input
        .try_parse(|i| i.expect_ident_matching("infinite"))
        .is_ok()
    {
        return Ok(f32::INFINITY);
    }
    parse_number_at_least(input, 0.0)
}

pub(crate) fn iteration_count_to_css(count: f32, dest: &mut String) {
    if count.is_infinite() {
        dest.push_str("infinite");
    } else {
        write_number(dest, count.into());
    }
}

pub(crate) fn parse_playback_direction<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, PlaybackDirection> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    match_ignore_ascii_case! { ident,
        "normal" => Ok(PlaybackDirection::Normal),
        "reverse" => Ok(PlaybackDirection::Reverse),
        "alternate" => Ok(PlaybackDirection::Alternate),
        "alternate-reverse" => Ok(PlaybackDirection::AlternateReverse),
        _ => Err(location.new_custom_error(())),
    }
}

pub(crate) fn playback_direction_name(direction: PlaybackDirection) -> &'static str {
    match direction {
        PlaybackDirection::Normal => "normal",
        PlaybackDirection::Reverse => "reverse",
        PlaybackDirection::Alternate => "alternate",
        PlaybackDirection::AlternateReverse => "alternate-reverse",
    }
}

pub(crate) fn parse_fill_mode<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FillMode> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    match_ignore_ascii_case! { ident,
        "none" => Ok(FillMode::None),
        "forwards" => Ok(FillMode::Forwards),
        "backwards" => Ok(FillMode::Backwards),
        "both" => Ok(FillMode::Both),
        _ => Err(location.new_custom_error(())),
    }
}

pub(crate) fn fill_mode_name(fill: FillMode) -> &'static str {
    match fill {
        FillMode::None => "none",
        FillMode::Forwards => "forwards",
        FillMode::Backwards => "backwards",
        FillMode::Both => "both",
    }
}

pub(crate) fn parse_play_state<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, AnimationPlayState> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    match_ignore_ascii_case! { ident,
        "running" => Ok(AnimationPlayState::Running),
        "paused" => Ok(AnimationPlayState::Paused),
        _ => Err(location.new_custom_error(())),
    }
}

pub(crate) fn play_state_name(state: AnimationPlayState) -> &'static str {
    match state {
        AnimationPlayState::Running => "running",
        AnimationPlayState::Paused => "paused",
    }
}

/// `animation-timeline`'s values machopo has: `auto` only, so that
/// `@supports` doesn't claim a scroll-driven animation machopo would run on
/// the document's timeline.
pub(crate) fn parse_animation_timeline<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, AnimationTimeline> {
    input.expect_ident_matching("auto")?;
    Ok(AnimationTimeline::Auto)
}

/// `animation-range-start`'s and `-end`'s values machopo has: `normal` only,
/// as `animation-timeline`'s.
pub(crate) fn parse_animation_range_boundary<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, AnimationRangeBoundary> {
    input.expect_ident_matching("normal")?;
    Ok(AnimationRangeBoundary::Normal)
}

pub(crate) fn animation_timeline_to_css(timeline: &AnimationTimeline, dest: &mut String) {
    match timeline {
        AnimationTimeline::Auto => dest.push_str("auto"),
    }
}

pub(crate) fn animation_range_boundary_to_css(
    boundary: &AnimationRangeBoundary,
    dest: &mut String,
) {
    match boundary {
        AnimationRangeBoundary::Normal => dest.push_str("normal"),
    }
}

/// `animation-range`: `[ <'animation-range-start'> <'animation-range-end'>?
/// ]#`, the end `normal` where left out.
pub(crate) fn parse_animation_range<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    let ranges = input.parse_comma_separated(|input| {
        let start = parse_animation_range_boundary(input)?;
        let end = input
            .try_parse(parse_animation_range_boundary)
            .unwrap_or(AnimationRangeBoundary::Normal);
        Ok((start, end))
    })?;
    Ok(vec![
        Value::AnimationRangeBoundaries(ranges.iter().map(|range| range.0).collect()),
        Value::AnimationRangeBoundaries(ranges.iter().map(|range| range.1).collect()),
    ])
}

/// `animation-range` from its longhands: each range's start, the end
/// left out while it is `normal`. `None` unless both lists have the same
/// length.
pub(crate) fn animation_range_to_css(values: &[(&Value, bool)]) -> Option<String> {
    let [
        (Value::AnimationRangeBoundaries(starts), _),
        (Value::AnimationRangeBoundaries(ends), _),
    ] = values
    else {
        return None;
    };
    if starts.len() != ends.len() {
        return None;
    }
    let mut dest = String::new();
    for (i, (start, end)) in starts.iter().zip(ends.iter()).enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        animation_range_boundary_to_css(start, &mut dest);
        if *end != AnimationRangeBoundary::Normal {
            dest.push(' ');
            animation_range_boundary_to_css(end, &mut dest);
        }
    }
    Some(dest)
}

/// What `animation` resets beside [`ANIMATION`] (css-animations 2), to
/// their initial values: it can't set them.
pub(crate) const ANIMATION_RESET: [LonghandId; 3] = [
    LonghandId::AnimationTimeline,
    LonghandId::AnimationRangeStart,
    LonghandId::AnimationRangeEnd,
];

fn animation_reset_values() -> [Value; 3] {
    [
        Value::AnimationTimelines(Arc::new([AnimationTimeline::Auto])),
        Value::AnimationRangeBoundaries(Arc::new([AnimationRangeBoundary::Normal])),
        Value::AnimationRangeBoundaries(Arc::new([AnimationRangeBoundary::Normal])),
    ]
}

/// `animation`'s longhands, in Blink's order (which its parsing and
/// serialization go by); it also resets [`ANIMATION_RESET`].
pub(crate) const ANIMATION: [LonghandId; 8] = [
    LonghandId::AnimationDuration,
    LonghandId::AnimationTimingFunction,
    LonghandId::AnimationDelay,
    LonghandId::AnimationIterationCount,
    LonghandId::AnimationDirection,
    LonghandId::AnimationFillMode,
    LonghandId::AnimationPlayState,
    LonghandId::AnimationName,
];

/// One `<single-animation>`: `<'animation-duration'> || <easing-function>
/// || <time> || <iteration-count> || <direction> || <fill-mode> ||
/// <play-state> || [ none | <keyframes-name> ]`.
#[derive(Default)]
struct SingleAnimation {
    duration: Option<Option<SpecifiedTime>>,
    easing: Option<SpecifiedEasing>,
    delay: Option<SpecifiedTime>,
    iteration_count: Option<f32>,
    direction: Option<PlaybackDirection>,
    fill_mode: Option<FillMode>,
    play_state: Option<AnimationPlayState>,
    name: Option<AnimationName>,
}

/// Each value goes to the first of the longhands, in [`ANIMATION`]'s
/// order, that hasn't one and takes it (Blink's
/// `ConsumeAnimationShorthand`): `ease` is the easing function, a second
/// `ease` the name; a negative time alone is the delay.
fn parse_single_animation<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SingleAnimation> {
    let mut single = SingleAnimation::default();
    let mut any = false;
    loop {
        if single.duration.is_none()
            && let Ok(duration) = input.try_parse(parse_animation_duration)
        {
            single.duration = Some(duration);
        } else if single.easing.is_none()
            && let Ok(easing) = input.try_parse(parse_easing)
        {
            single.easing = Some(easing);
        } else if single.delay.is_none()
            && let Ok(delay) = input.try_parse(|i| parse_time(i, false))
        {
            single.delay = Some(delay);
        } else if single.iteration_count.is_none()
            && let Ok(count) = input.try_parse(parse_iteration_count)
        {
            single.iteration_count = Some(count);
        } else if single.direction.is_none()
            && let Ok(direction) = input.try_parse(parse_playback_direction)
        {
            single.direction = Some(direction);
        } else if single.fill_mode.is_none()
            && let Ok(fill) = input.try_parse(parse_fill_mode)
        {
            single.fill_mode = Some(fill);
        } else if single.play_state.is_none()
            && let Ok(state) = input.try_parse(parse_play_state)
        {
            single.play_state = Some(state);
        } else if single.name.is_none()
            && let Ok(name) = input.try_parse(parse_animation_name)
        {
            single.name = Some(name);
        } else {
            break;
        }
        any = true;
    }
    if !any {
        return invalid(input);
    }
    Ok(single)
}

/// `animation`: `<single-animation>#`, and [`ANIMATION_RESET`] reset
/// (implicit).
pub(crate) fn parse_animation<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Vec<(Value, bool)>> {
    let animations = input.parse_comma_separated(parse_single_animation)?;
    fn list<T>(animations: &[SingleAnimation], f: impl Fn(&SingleAnimation) -> T) -> Arc<[T]> {
        animations.iter().map(f).collect()
    }
    Ok(vec![
        Value::AnimationDurations(list(&animations, |a| a.duration.unwrap_or(None))),
        Value::Easings(list(&animations, |a| {
            a.easing.clone().unwrap_or(SpecifiedEasing::EASE)
        })),
        Value::Times(list(&animations, |a| {
            a.delay.unwrap_or(SpecifiedTime::ZERO)
        })),
        Value::IterationCounts(list(&animations, |a| a.iteration_count.unwrap_or(1.0))),
        Value::PlaybackDirections(list(&animations, |a| {
            a.direction.unwrap_or(PlaybackDirection::Normal)
        })),
        Value::FillModes(list(&animations, |a| a.fill_mode.unwrap_or(FillMode::None))),
        Value::PlayStates(list(&animations, |a| {
            a.play_state.unwrap_or(AnimationPlayState::Running)
        })),
        Value::AnimationNames(list(&animations, |a| {
            a.name.clone().unwrap_or(AnimationName::None)
        })),
    ]
    .into_iter()
    .map(|value| (value, false))
    .chain(animation_reset_values().map(|value| (value, true)))
    .collect())
}

/// Which of the shorthand's other values `name` would be read as, were
/// it written without them: its index in [`ANIMATION`].
fn name_taken_by(name: &AnimationName) -> Option<usize> {
    let AnimationName::Name(name) = name else {
        return None;
    };
    Some(match_ignore_ascii_case! { name,
        "auto" => 0,
        "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end" => 1,
        "infinite" => 3,
        "normal" | "reverse" | "alternate" | "alternate-reverse" => 4,
        "forwards" | "backwards" | "both" => 5,
        "running" | "paused" => 6,
        _ => return None,
    })
}

/// `animation` written from its longhands (Blink's
/// `StylePropertySerializer::GetLayeredShorthandValue` for it): each
/// animation's values but those at their initial value (a duration of
/// `0s` too), the duration kept when there is a delay, and the value a
/// name would otherwise be read as; `none` for one with nothing to write.
/// `None` unless every list has the same length, and what it resets is
/// as it resets it (Chrome's).
pub(crate) fn animation_to_css(values: &[(&Value, bool)]) -> Option<String> {
    if values[ANIMATION.len()..]
        .iter()
        .zip(animation_reset_values())
        .any(|((value, _), reset)| **value != reset)
    {
        return None;
    }
    let [
        (Value::AnimationDurations(durations), _),
        (Value::Easings(easings), _),
        (Value::Times(delays), _),
        (Value::IterationCounts(counts), _),
        (Value::PlaybackDirections(directions), _),
        (Value::FillModes(fills), _),
        (Value::PlayStates(states), _),
        (Value::AnimationNames(names), _),
        ..,
    ] = values
    else {
        return None;
    };
    let n = names.len();
    if [
        durations.len(),
        easings.len(),
        delays.len(),
        counts.len(),
        directions.len(),
        fills.len(),
        states.len(),
    ]
    .iter()
    .any(|&len| len != n)
    {
        return None;
    }
    let mut items = Vec::new();
    for i in 0..n {
        let taken = name_taken_by(&names[i]);
        let needed = |index: usize| taken == Some(index);
        let mut text = String::new();
        let part = |text: &mut String, write: &dyn Fn(&mut String)| {
            if !text.is_empty() {
                text.push(' ');
            }
            write(text);
        };
        let delayed = delays[i].seconds() != 0.0;
        if durations[i].is_some_and(|d| d.seconds() != 0.0) || delayed || needed(0) {
            part(&mut text, &|dest| {
                animation_duration_to_css(&durations[i], dest)
            });
        }
        if easings[i] != SpecifiedEasing::EASE || needed(1) {
            part(&mut text, &|dest| easings[i].to_css(dest));
        }
        if delayed {
            part(&mut text, &|dest| delays[i].to_css(dest));
        }
        if counts[i] != 1.0 || needed(3) {
            part(&mut text, &|dest| iteration_count_to_css(counts[i], dest));
        }
        if directions[i] != PlaybackDirection::Normal || needed(4) {
            part(&mut text, &|dest| {
                dest.push_str(playback_direction_name(directions[i]))
            });
        }
        if fills[i] != FillMode::None || needed(5) {
            part(&mut text, &|dest| dest.push_str(fill_mode_name(fills[i])));
        }
        if states[i] != AnimationPlayState::Running || needed(6) {
            part(&mut text, &|dest| dest.push_str(play_state_name(states[i])));
        }
        if names[i] != AnimationName::None || text.is_empty() {
            part(&mut text, &|dest| animation_name_to_css(&names[i], dest));
        }
        items.push(text);
    }
    Some(items.join(", "))
}
