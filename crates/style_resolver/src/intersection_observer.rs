//! What an `IntersectionObserver` parses as CSS: its `rootMargin` and
//! `scrollMargin` (Intersection Observer's "parse a margin"), and how they
//! read back.

use base::numerics::saturated_cast;
use cssparser::{Parser, ParserInput, Token};
use gfx::{MarginLength, Margins};

use css::values::LengthUnit;

/// `text` as a margin: one to four component values, each an absolute
/// length (in px) or a percentage, white space around them allowed; none
/// is `0px`. One is all four sides, two are top and bottom, then right and
/// left; three leave the left as the right (as CSS's `margin`). `None` if
/// it is anything else: a length relative to a font, a number, a function
/// (`calc()`), more than four. Chrome's `IntersectionObserver::ParseMargin`
/// also takes the absolute units (WPT's `root-margin-scroll-margin-
/// units.html`).
pub fn parse_intersection_margin(text: &str) -> Option<Margins> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let mut values = Vec::new();
    while let Ok(token) = parser.next() {
        let value = match *token {
            Token::Dimension {
                value, ref unit, ..
            } => {
                let per_px = LengthUnit::from_name(unit)?.absolute_px()?;
                MarginLength::Px(f64::from(value) * f64::from(per_px))
            }
            Token::Percentage { unit_value, .. } => {
                MarginLength::Percent(f64::from(unit_value * 100.0))
            }
            _ => return None,
        };
        values.push(value);
        if values.len() > 4 {
            return None;
        }
    }
    Some(match values[..] {
        [] => [MarginLength::Px(0.0); 4],
        [all] => [all; 4],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left] => [top, right, bottom, left],
        _ => unreachable!("at most four"),
    })
}

/// How `rootMargin` and `scrollMargin` read back: the four sides, each as
/// a whole number of px or percent. Chrome's `IntersectionObserver`
/// appends each `Length`'s `IntValue()`, so `10cm` (377.95px) reads back as
/// `377px` (WPT's `root-margin-scroll-margin-units.html`).
pub fn serialize_intersection_margin(margins: &Margins) -> String {
    margins
        .iter()
        .map(|side| match *side {
            MarginLength::Px(px) => format!("{}px", saturated_cast::<i64, _>(px.trunc())),
            MarginLength::Percent(percent) => {
                format!("{}%", saturated_cast::<i64, _>(percent.trunc()))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use gfx::MarginLength::{Percent, Px};

    use super::{parse_intersection_margin, serialize_intersection_margin};

    #[test]
    fn margins() {
        assert_eq!(parse_intersection_margin(""), Some([Px(0.0); 4]));
        assert_eq!(parse_intersection_margin("  "), Some([Px(0.0); 4]));
        assert_eq!(parse_intersection_margin(" 7px "), Some([Px(7.0); 4]));
        assert_eq!(
            parse_intersection_margin("10% 20px"),
            Some([Percent(10.0), Px(20.0), Percent(10.0), Px(20.0)])
        );
        assert_eq!(
            parse_intersection_margin("1px 2px 3px"),
            Some([Px(1.0), Px(2.0), Px(3.0), Px(2.0)])
        );
        assert_eq!(
            parse_intersection_margin("10px 20% 40% -30px"),
            Some([Px(10.0), Percent(20.0), Percent(40.0), Px(-30.0)])
        );
        assert_eq!(parse_intersection_margin("1in"), Some([Px(96.0); 4]));
        assert_eq!(parse_intersection_margin("2PC"), Some([Px(32.0); 4]));
        for wrong in [
            "1",
            "0",
            "2em",
            "1rem",
            "auto",
            "calc(1px + 2px)",
            "1px !important",
            "1px 1px 1px 1px 1px",
            "1px,2px",
        ] {
            assert_eq!(parse_intersection_margin(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn serialized() {
        let serialize =
            |text| serialize_intersection_margin(&parse_intersection_margin(text).unwrap());
        assert_eq!(serialize(""), "0px 0px 0px 0px");
        assert_eq!(serialize("10% 20px"), "10% 20px 10% 20px");
        assert_eq!(serialize("10cm"), "377px 377px 377px 377px");
        assert_eq!(serialize("10mm"), "37px 37px 37px 37px");
        assert_eq!(serialize("10q"), "9px 9px 9px 9px");
        assert_eq!(serialize("10pt"), "13px 13px 13px 13px");
        assert_eq!(serialize("-1.5px 2.5%"), "-1px 2% -1px 2%");
    }
}
