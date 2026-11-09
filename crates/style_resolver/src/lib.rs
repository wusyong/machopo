//! What style shares that doesn't read the DOM (`blink/renderer/core/css`'s
//! resolver, the parts that take declarations rather than elements): the
//! style engine (`style`) matches selectors on its DOM and collects
//! each element's declaration blocks, and this crate turns them into a
//! `computed_style::ComputedStyle`.
//!
//! - [`cascade`]: the cascade of an element's declaration blocks into its
//!   computed style, the adjustments after it, and the matched properties
//!   cache.
//! - [`animation_style`]: what a style change event does to a target's CSS
//!   animations and transitions.
//! - Presentational hints ([`html_hints`], [`svg_hint`]): HTML's and SVG's
//!   attributes that map to CSS, parsed from what an element's
//!   [`HintSource`] gives.
//! - [`FormControl`]: the native look `appearance: auto` gives a form
//!   control.
//! - [`UA_CSS`]: the UA stylesheet's text.
//!
//! Parsing CSS, its values and their serialization are the `css` crate's,
//! which doesn't know the DOM either (so that the DOM can hold a
//! stylesheet's parsed contents); what other crates use from it is
//! re-exported here. For CSSOM, [`CssPropertyValueSet`] is a declaration
//! block that script reads and changes (`element.style` parses the `style`
//! attribute into one and writes it back serialized), and
//! [`css_property_for_attribute`] maps `CSSStyleDeclaration`'s camel-cased
//! names to properties.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

pub mod animation_style;
pub mod cascade;
mod hints;
mod intersection_observer;
mod theme;

pub use crate::cascade::pseudo_style_without_rules;
pub use crate::hints::{
    HintAttribute, HintElement, HintSource, PresentationAttributeCache, SvgHint, html_hints,
    svg_hint, svg_hint_attribute_names, svg_hint_declarations,
};
pub use crate::intersection_observer::{parse_intersection_margin, serialize_intersection_margin};
// A form control's native look, of an element the style engine names.
pub use crate::theme::Control as FormControl;

pub use css::media::MediaQueryList;
pub use css::stylesheet::rules_resolved_for_testing;
pub use css::{
    CssPropertyValueSet, CssRule, InsertRuleError, StyleRule, StyleSheetContents, UrlContext,
    computable_properties, computed_value, css_property_attributes, css_property_for_attribute,
    custom_property_names, is_layout_dependent, parse_key_text, path_from_string,
    physical_property, resolved_transform, resolved_transform_origin, shorthand_from_style,
    shorthand_longhands, shorthand_value, supports_declaration, write_css_number,
};

/// The UA stylesheet's text (`resources/html.css`), which the style engine
/// parses.
pub const UA_CSS: &str = include_str!("../resources/html.css");
