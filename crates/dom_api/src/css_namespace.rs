//! CSSOM's `CSS` namespace (Blink's `core/css/dom_window_css.cc`):
//! `supports()` and `escape()`, answered by `css`. Chrome's has Typed OM's
//! unit functions (`CSS.px()`) and `registerProperty` too, which machopo
//! doesn't.

use dom::DomString;

use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// `supports(property, value)`: whether `value` is one of `property`'s.
pub(crate) fn supports_declaration<'gc>(
    _: &ApiCx<'_, 'gc>,
    property: &str,
    value: &str,
) -> Result<bool, ApiError> {
    Ok(css::supports_declaration(property, value))
}

/// `supports(conditionText)`: whether the `@supports` condition holds.
pub(crate) fn supports_condition<'gc>(
    _: &ApiCx<'_, 'gc>,
    condition: &str,
) -> Result<bool, ApiError> {
    Ok(css::supports_condition(condition))
}

/// `escape(ident)`: `ident` serialized as an identifier.
pub(crate) fn escape<'gc>(_: &ApiCx<'_, 'gc>, ident: &str) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(css::escape_identifier(
        ident,
    ))))
}
