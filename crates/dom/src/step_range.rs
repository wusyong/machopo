//! HTML's steps of a number and a range `<input>`
//! (https://html.spec.whatwg.org/#concept-input-step), as their value
//! sanitization and the user's steps use them: the allowed value step, the
//! step base, the minimum and the maximum (Blink's `StepRange` and the
//! input types' `CreateStepRange`). A range's value is kept within the
//! minimum and the maximum and aligned to the step; the user steps a
//! number (its spin button and arrow keys: Blink's `InputType::
//! StepUpFromLayoutObject` and `ApplyStep`) and a range (its keys: Blink's
//! `RangeInputType::HandleKeydownEvent`). Script's `stepUp()`, `stepDown()`
//! and `valueAsNumber` aren't here yet.
//!
//! The arithmetic is in doubles; a result is rounded to the decimal places
//! of the step and the step base, which Blink's `Decimal` keeps exact (0.1
//! stepped up by 0.1 twice is 0.3, not 0.30000000000000004).

use crate::numbers::{is_valid_float, number_to_string, parse_float};
use crate::{Cx, InputKind, NameId, NodeRef, names};

/// The default step of a number and a range (HTML's default step, with a
/// step scale factor of 1).
const DEFAULT_STEP: f64 = 1.0;

/// A number as HTML's "algorithm to convert a string to a number" of a
/// number and a range gives it: a valid floating-point number, finite.
fn number(value: &str) -> Option<f64> {
    is_valid_float(value)
        .then(|| parse_float(value))
        .flatten()
        .filter(|n| n.is_finite())
}

/// An element's steps: its allowed value step (none for `step="any"`), its
/// step base, minimum and maximum. A range's minimum and maximum are 0 and
/// 100 by default, the maximum at least the minimum, the step base the
/// minimum; a number without `min` or `max` has the largest doubles
/// (Blink's), its step base its `min`, else its `value` attribute, else 0.
#[derive(Clone, Copy, Debug)]
struct StepRange {
    step: Option<f64>,
    base: f64,
    min: f64,
    max: f64,
}

impl StepRange {
    fn of<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Self {
        let attribute = |local: NameId| {
            cx.attribute_value_by_id(element, local)
                .and_then(|value| number(&value.borrow().to_str()))
        };
        let step = match cx.attribute_value_by_id(element, names::local::STEP) {
            Some(step) if step.borrow().to_str().eq_ignore_ascii_case("any") => None,
            Some(step) => Some(
                number(&step.borrow().to_str())
                    .filter(|s| *s > 0.0)
                    .unwrap_or(DEFAULT_STEP),
            ),
            None => Some(DEFAULT_STEP),
        };
        let (min, max) = (attribute(names::local::MIN), attribute(names::local::MAX));
        if cx.input_kind(element) == Some(InputKind::Range) {
            let min = min.unwrap_or(0.0);
            return Self {
                step,
                base: min,
                min,
                max: max.unwrap_or(100.0).max(min),
            };
        }
        Self {
            step,
            base: min
                .or_else(|| attribute(names::local::VALUE))
                .unwrap_or(0.0),
            min: min.unwrap_or(-f64::MAX),
            max: max.unwrap_or(f64::MAX),
        }
    }

    /// `value` rounded to the decimal places of the step and the step base.
    fn round(&self, value: f64) -> f64 {
        round_to_places(value, &[self.step.unwrap_or(DEFAULT_STEP), self.base])
    }

    /// `value` within the minimum and the maximum, aligned to the step if
    /// an aligned one is within them (Blink's `ClampValue`).
    fn clamp(&self, value: f64) -> f64 {
        let clamped = value.clamp(self.min, self.max);
        let Some(step) = self.step else {
            return clamped;
        };
        let rounded = self.round(self.base + ((clamped - self.base) / step).round() * step);
        let aligned = if rounded > self.max {
            rounded - step
        } else if rounded < self.min {
            rounded + step
        } else {
            rounded
        };
        if (self.min..=self.max).contains(&aligned) {
            aligned
        } else {
            clamped
        }
    }

    /// Whether `value` minus the step base isn't a multiple of `step`, up
    /// to an error a float can't tell (Blink's `StepMismatch` and
    /// `AcceptableError`).
    fn mismatch(&self, value: f64, step: f64) -> bool {
        let distance = (value - self.base).abs();
        if !distance.is_finite() || distance / f64::from(1u32 << 30) / 8_388_608.0 > step {
            return false;
        }
        let remainder = (distance - step * (distance / step).round()).abs();
        let error = step / f64::from(1u32 << 24);
        error < remainder && remainder < step - error
    }

    /// The largest value aligned to `step` that isn't above the maximum, if
    /// that isn't below the minimum (Blink's `StepSnappedMaximum`).
    fn snapped_max(&self, step: f64) -> Option<f64> {
        let base = self.base;
        if base - step == base || !(base / step).is_finite() {
            return None;
        }
        let mut aligned = base + ((self.max - base) / step).floor() * step;
        if aligned > self.max {
            aligned -= step;
        }
        (aligned >= self.min).then_some(aligned)
    }

    /// The value `count` steps of `step` from `current` (Blink's
    /// `InputType::ApplyStep`; with `step="any"` the default step,
    /// `exact`: the values aren't aligned): `current`, if it isn't aligned,
    /// first goes to the aligned value on the way (Blink takes that as one
    /// of the steps), then the rest; kept within the minimum and the
    /// maximum, aligned. `None`: nothing to set (the minimum is above the
    /// maximum, nothing between them is aligned, or the value would go the
    /// wrong way).
    fn apply(&self, step: f64, exact: bool, current: f64, mut count: f64) -> Option<f64> {
        if self.min > self.max {
            return None;
        }
        let aligned_max = self.snapped_max(step)?;
        let base = self.base;
        let mut value = current;
        if !exact && self.mismatch(current, step) {
            if count < 0.0 {
                value = base + ((value - base) / step).floor() * step;
                count += 1.0;
            } else if count > 0.0 {
                value = base + ((value - base) / step).ceil() * step;
                count -= 1.0;
            }
        }
        value += step * count;
        // Blink's `AlignValueForStep`: aligned to the step, unless it was
        // stepped from an unaligned value (or is too large to).
        if !exact && value < 1e21 && !self.mismatch(current, step) {
            value = base + ((value - base) / step).round() * step;
        }
        if value > self.max {
            value = aligned_max;
        } else if value < self.min {
            value = base + ((self.min - base) / step).ceil() * step;
        }
        if (count < 0.0 && current < value) || (count > 0.0 && current > value) {
            return None;
        }
        Some(round_to_places(value, &[step, base]))
    }
}

/// The decimal places of `x` as it is written shortest (`0.25`: 2,
/// `1e-7`: 7, `300`: 0).
fn decimal_places(x: f64) -> i32 {
    let written = format!("{x:e}");
    let (mantissa, exponent) = written.split_once('e').unwrap_or((&written, "0"));
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let exponent: i32 = exponent.parse().unwrap_or(0);
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a double's shortest digits are at most 17, its exponent within ±324"
    )]
    let places = i32::try_from(fraction).unwrap_or(0) - exponent;
    places.max(0)
}

/// `value` rounded to the most decimal places among `of`'s, unless that is
/// past what a double holds.
fn round_to_places(value: f64, of: &[f64]) -> f64 {
    let places = of.iter().map(|x| decimal_places(*x)).max().unwrap_or(0);
    if places > 15 || value.abs() >= 1e15 {
        return value;
    }
    let scale = 10f64.powi(places);
    let rounded = (value * scale).round() / scale;
    if rounded.is_finite() { rounded } else { value }
}

/// Why script's `stepUp()` and `stepDown()` don't step (HTML's
/// `InvalidStateError`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    /// The element's type doesn't step (not a number or a range).
    NotSteppable,
    /// It has no allowed value step (`step="any"`).
    NoAllowedStep,
}

/// A key a range takes (`range_key_value`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKey {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

impl<'gc> Cx<'_, 'gc> {
    /// HTML's value sanitization algorithm of a range: a number within its
    /// minimum and maximum, aligned to its step (written again only if
    /// that changed it); else its default value, halfway between them.
    pub(crate) fn sanitize_range(&self, element: NodeRef<'gc>, value: &str) -> String {
        let range = StepRange::of(self, element);
        match number(value) {
            Some(n) => {
                let clamped = range.clamp(n);
                if clamped == n {
                    value.to_owned()
                } else {
                    number_to_string(clamped)
                }
            }
            None => number_to_string(range.clamp(range.min + (range.max - range.min) / 2.0)),
        }
    }

    /// Where a range's value is between its minimum and maximum, from 0 to
    /// 1; 0 if they are one (Blink's `StepRange::ProportionFromValue`):
    /// where its thumb is.
    pub(crate) fn range_ratio(&self, element: NodeRef<'gc>) -> f64 {
        let range = StepRange::of(self, element);
        if range.max <= range.min {
            return 0.0;
        }
        let value = number(&self.input_value(element).to_str()).unwrap_or(range.min);
        ((value - range.min) / (range.max - range.min)).clamp(0.0, 1.0)
    }

    /// Whether `element` steps: a number or a range (HTML's `stepUp()` and
    /// `valueAsNumber` apply to it; the date and time types aren't yet).
    pub fn steps(&self, element: NodeRef<'gc>) -> bool {
        matches!(
            self.input_kind(element),
            Some(InputKind::Number | InputKind::Range)
        )
    }

    /// `valueAsNumber`: a number's or a range's value as a number, `NaN`
    /// for none or for another type.
    pub fn input_value_as_number(&self, element: NodeRef<'gc>) -> f64 {
        if !self.steps(element) {
            return f64::NAN;
        }
        number(&self.input_value(element).to_str()).unwrap_or(f64::NAN)
    }

    /// Script's `stepUp(n)` and `stepDown(n)` (HTML's, as Blink's
    /// `InputType::StepUp`): from the value, 0 for none, `count` steps; the
    /// value it comes to, as a string, if it changes it at all (none: the
    /// minimum is above the maximum, nothing between them is aligned, or it
    /// would go the wrong way). The caller sets it as script does.
    pub fn step_for_script(
        &self,
        element: NodeRef<'gc>,
        count: f64,
    ) -> Result<Option<String>, StepError> {
        if !self.steps(element) {
            return Err(StepError::NotSteppable);
        }
        let range = StepRange::of(self, element);
        let step = range.step.ok_or(StepError::NoAllowedStep)?;
        let current = number(&self.input_value(element).to_str()).unwrap_or(0.0);
        Ok(range
            .apply(step, false, current, count)
            .map(number_to_string))
    }

    /// The user's step of a number, `n` steps up or down (its spin button
    /// and arrow keys; Blink's `InputType::StepUpFromLayoutObject`):
    /// `step="any"` is the default step. With no number, the value is
    /// first 0 (or the nearest that stepping keeps within the minimum and
    /// the maximum); below the minimum going up it becomes the minimum,
    /// above the maximum going down the maximum; at or past the end it goes
    /// to, nothing. The value it comes to, as a string, if it changes: the
    /// caller sets it for the user.
    pub fn number_step_for_user(&self, element: NodeRef<'gc>, n: i32) -> Option<String> {
        if self.input_kind(element) != Some(InputKind::Number) {
            return None;
        }
        let range = StepRange::of(self, element);
        let (step, exact) = match range.step {
            Some(step) => (step, false),
            None => (DEFAULT_STEP, true),
        };
        let n = f64::from(n);
        let before = number(&self.input_value(element).to_str());
        let current = before.unwrap_or_else(|| {
            let next = step * n;
            0.0f64.max(range.min - next).min(range.max - next)
        });
        let value = if (n > 0.0 && current < range.min) || (n < 0.0 && current > range.max) {
            Some(if n > 0.0 { range.min } else { range.max })
        } else if (n > 0.0 && current >= range.max) || (n < 0.0 && current <= range.min) {
            None
        } else {
            range.apply(step, exact, current, n)
        };
        // With no number, the one it started at is the value at least.
        let value = value.or_else(|| before.is_none().then_some(current))?;
        (Some(value) != before).then(|| number_to_string(value))
    }

    /// A range's value after `key` (Blink's `RangeInputType::
    /// HandleKeydownEvent`): up and down a step (a hundredth of its range
    /// with `step="any"`), the page keys a tenth of the range (at least a
    /// step), Home and End the minimum and the maximum; within them,
    /// aligned. The value, as a string, if it changes.
    pub fn range_key_value(&self, element: NodeRef<'gc>, key: RangeKey) -> Option<String> {
        if self.input_kind(element) != Some(InputKind::Range) {
            return None;
        }
        let range = StepRange::of(self, element);
        let current = number(&self.input_value(element).to_str()).unwrap_or(range.min);
        let step = range.step.unwrap_or((range.max - range.min) / 100.0);
        let big_step = ((range.max - range.min) / 10.0).max(step);
        let value = match key {
            RangeKey::Up => current + step,
            RangeKey::Down => current - step,
            RangeKey::PageUp => current + big_step,
            RangeKey::PageDown => current - big_step,
            RangeKey::Home => range.min,
            RangeKey::End => range.max,
        };
        let value = range.clamp(round_to_places(value, &[step, range.base]));
        (value != current).then(|| number_to_string(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_places_as_written() {
        for (x, places) in [(0.25, 2), (1e-7, 7), (300.0, 0), (0.1, 1), (1.5e-3, 4)] {
            assert_eq!(decimal_places(x), places, "{x}");
        }
        assert_eq!(round_to_places(0.1 + 0.2, &[0.1, 0.0]), 0.3);
    }
}
