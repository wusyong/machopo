//! HTML's common microsyntaxes of numbers
//! (https://html.spec.whatwg.org/#numbers; Blink's `html_parser_idioms`):
//! a valid floating-point number, the rules for parsing one, and the best
//! representation of a number as one.

/// HTML's "rules for parsing floating-point number values": leading ASCII
/// whitespace, a sign, digits with an optional fraction and exponent;
/// anything after them is ignored. A value too large for a double is an
/// error, `-0` is 0.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`end` and `exponent` step only past a byte `bytes.get` found: at most the \
              text's length"
)]
pub fn parse_float(text: &str) -> Option<f64> {
    let text = text.trim_start_matches(is_ascii_whitespace);
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'-' | b'+')) {
        end += 1;
    }
    let integer_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digits = end > integer_start;
    // After digits a full stop needs none of its own (`1.e2` is 100); a
    // leading one does.
    if bytes.get(end) == Some(&b'.')
        && (digits || bytes.get(end + 1).is_some_and(u8::is_ascii_digit))
    {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        digits = true;
    }
    if !digits {
        return None;
    }
    // An exponent counts only with its digits.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'-' | b'+')) {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            end = exponent;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
        }
    }
    let value: f64 = text[..end].parse().ok()?;
    value
        .is_finite()
        .then_some(if value == 0.0 { 0.0 } else { value })
}

/// ECMAScript's `Number::toString(x)` for a finite `x`: HTML's "best
/// representation of the number as a floating-point number" (what a
/// `double` reflection writes back), the shortest digits that round-trip,
/// with an exponent outside 1e-7 to 1e21 (V8's `DoubleToCString`).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a double's shortest digits are at most 17 and its exponent within ±324: \
              `k` and `n` are small, and each count cast to `usize` is checked positive \
              by its branch"
)]
pub fn number_to_string(x: f64) -> String {
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // Rust's `{:e}` gives the shortest round-trip digits, as JS wants:
    // `d.ddde±x`, so `n` (JS's) is `x + 1`.
    let formatted = format!("{:e}", x.abs());
    let (mantissa, exponent) = formatted
        .split_once('e')
        .expect("`{:e}` writes an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("an exponent") + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let mantissa = if k == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{mantissa}e{sign}{}", e.abs())
    };
    format!("{sign}{body}")
}

/// HTML's "valid floating-point number": `-`, digits, an optional fraction
/// and exponent; nothing else (no `+`, no whitespace, no `1.`).
pub(crate) fn is_valid_float(value: &str) -> bool {
    let rest = value.strip_prefix('-').unwrap_or(value);
    let (number, exponent) = match rest.find(['e', 'E']) {
        // Past the `e`, one byte.
        Some(at) => (&rest[..at], Some(&rest[at..][1..])),
        None => (rest, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let number_ok = match number.split_once('.') {
        Some((integer, fraction)) => (integer.is_empty() || digits(integer)) && digits(fraction),
        None => digits(number),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    number_ok && exponent_ok
}

pub(crate) fn is_ascii_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r')
}

/// HTML's "rules for parsing integers": leading ASCII whitespace, a sign,
/// digits; anything after them is ignored; none past an `i32`.
pub(crate) fn parse_integer(text: &str) -> Option<i32> {
    let text = text.trim_start_matches(is_ascii_whitespace);
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, text.get(1..)?),
        b'+' => (false, text.get(1..)?),
        _ => (false, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let value: i64 = digits.get(..end)?.parse().ok()?;
    i32::try_from(if negative {
        value.checked_neg()?
    } else {
        value
    })
    .ok()
}
