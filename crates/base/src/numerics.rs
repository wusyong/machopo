//! Integer conversions that say what overflow does (Chromium's
//! `base/numerics`): a float to an integer saturating, NaN to 0
//! ([`saturated_cast`], [`clamp_round`], [`clamp_floor`], [`clamp_ceil`];
//! Chromium's `saturated_cast`, `ClampRound`, `ClampFloor`, `ClampCeil`),
//! and an integer to another, saturating ([`saturated_cast`]). An integer
//! that fits converts with `From`; one that may not with `try_from`, then
//! deciding what an error means.

/// A type a value of `S` converts to, saturating: out of range, the
/// nearest end of its range; NaN, 0.
pub trait SaturatedFrom<S>: Sized {
    fn saturated_from(value: S) -> Self;
}

/// `value` as a `T`, saturating: out of `T`'s range, its nearest end; NaN,
/// 0 (Chromium's `base::saturated_cast`).
pub fn saturated_cast<T: SaturatedFrom<S>, S>(value: S) -> T {
    T::saturated_from(value)
}

/// `value` rounded to the nearest integer, halves away from zero, as a
/// `T`, saturating (Chromium's `ClampRound`).
pub fn clamp_round<T: SaturatedFrom<f64>>(value: impl Into<f64>) -> T {
    T::saturated_from(value.into().round())
}

/// `value` rounded down, as a `T`, saturating (Chromium's `ClampFloor`).
pub fn clamp_floor<T: SaturatedFrom<f64>>(value: impl Into<f64>) -> T {
    T::saturated_from(value.into().floor())
}

/// `value` rounded up, as a `T`, saturating (Chromium's `ClampCeil`).
pub fn clamp_ceil<T: SaturatedFrom<f64>>(value: impl Into<f64>) -> T {
    T::saturated_from(value.into().ceil())
}

/// `value` rounded to the nearest f32 (Skia's `SkDoubleToScalar`): what
/// is worked out in f64 for its precision goes back to an f32. Past f32's
/// range it is infinite, as Skia's is; NaN stays NaN. (Not a saturating
/// conversion: clamp first where an infinity mustn't come of it.)
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounding to the nearest f32 is what this is for"
)]
pub fn f32_from_f64(value: f64) -> f32 {
    value as f32
}

/// A float to each integer type: Rust's `as` already saturates, NaN to 0,
/// the one place it is relied on.
macro_rules! from_float {
    ($($float:ty => signed $($signed:ty),*; unsigned $($unsigned:ty),*;)*) => {$(
        $(from_float!(@impl $float => $signed, clippy::cast_possible_truncation);)*
        $(from_float!(
            @impl $float => $unsigned, clippy::cast_possible_truncation, clippy::cast_sign_loss
        );)*
    )*};
    (@impl $float:ty => $int:ty, $($lint:path),*) => {
        impl SaturatedFrom<$float> for $int {
            #[expect($($lint),*, reason = "`as` from a float saturates, NaN to 0: what this is for")]
            fn saturated_from(value: $float) -> Self {
                value as $int
            }
        }
    };
}

from_float! {
    f32 => signed i8, i16, i32, i64, isize; unsigned u8, u16, u32, u64, usize;
    f64 => signed i8, i16, i32, i64, isize; unsigned u8, u16, u32, u64, usize;
}

/// An integer to each integer type, through `i128`, which holds every
/// value of each (`isize` and `usize` are at most 64 bits wide).
macro_rules! from_int {
    ($($from:ty),*) => {$(
        from_int!(@to $from => i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
    )*};
    (@to $from:ty => $($to:ty),*) => {$(
        impl SaturatedFrom<$from> for $to {
            fn saturated_from(value: $from) -> Self {
                let clamped = (value as i128).clamp(<$to>::MIN as i128, <$to>::MAX as i128);
                <$to>::try_from(clamped).unwrap_or(<$to>::MAX)
            }
        }
    )*};
}

from_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_saturate_and_nan_is_zero() {
        assert_eq!(saturated_cast::<i32, _>(1e20_f64), i32::MAX);
        assert_eq!(saturated_cast::<i32, _>(-1e20_f64), i32::MIN);
        assert_eq!(saturated_cast::<u8, _>(-1.0_f32), 0);
        assert_eq!(saturated_cast::<u8, _>(300.0_f32), 255);
        assert_eq!(saturated_cast::<i32, _>(f64::NAN), 0);
        assert_eq!(saturated_cast::<i64, _>(f32::INFINITY), i64::MAX);
        assert_eq!(saturated_cast::<i64, _>(f32::NEG_INFINITY), i64::MIN);
        assert_eq!(saturated_cast::<i32, _>(2.9_f64), 2, "toward zero");
        assert_eq!(saturated_cast::<i32, _>(-2.9_f64), -2, "toward zero");
    }

    #[test]
    fn rounding() {
        assert_eq!(clamp_round::<i32>(2.5_f32), 3);
        assert_eq!(clamp_round::<i32>(-2.5_f32), -3, "halves away from zero");
        assert_eq!(clamp_round::<i32>(2.4_f64), 2);
        assert_eq!(clamp_floor::<i32>(-0.5_f32), -1);
        assert_eq!(clamp_floor::<i32>(2.7_f64), 2);
        assert_eq!(clamp_ceil::<i32>(0.1_f32), 1);
        assert_eq!(clamp_round::<u32>(-7.0_f64), 0);
        assert_eq!(clamp_round::<i32>(f64::NAN), 0);
        assert_eq!(clamp_ceil::<u16>(1e9_f64), u16::MAX);
    }

    #[test]
    fn f64_to_the_nearest_f32() {
        assert_eq!(f32_from_f64(0.1), 0.1_f32);
        assert_eq!(f32_from_f64(1e39), f32::INFINITY);
        assert_eq!(f32_from_f64(-1e39), f32::NEG_INFINITY);
        assert!(f32_from_f64(f64::NAN).is_nan());
    }

    #[test]
    fn integers_saturate() {
        assert_eq!(saturated_cast::<u8, _>(300_i32), 255);
        assert_eq!(saturated_cast::<u8, _>(-1_i32), 0);
        assert_eq!(saturated_cast::<i8, _>(-300_i64), i8::MIN);
        assert_eq!(saturated_cast::<i32, _>(u64::MAX), i32::MAX);
        assert_eq!(saturated_cast::<u32, _>(7_u64), 7);
        assert_eq!(saturated_cast::<usize, _>(-5_i32), 0);
        assert_eq!(saturated_cast::<isize, _>(u64::MAX), isize::MAX);
        assert_eq!(saturated_cast::<i64, _>(isize::MIN), i64::MIN);
        assert_eq!(saturated_cast::<u64, _>(usize::MAX), usize::MAX as u64);
    }
}
