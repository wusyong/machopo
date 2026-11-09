//! CSS itself (`blink/renderer/core/css`'s parser and properties): parsing
//! stylesheets, declaration blocks and selectors with `cssparser` and
//! `selectors`, each supported property's specified value, computing it
//! into a `computed_style::ComputedStyle`, and serializing both as CSSOM
//! reads them. Applying stylesheets to the DOM (selector matching and
//! `StyleEngine`) is `style`'s, the cascade of declaration blocks
//! `style_resolver`'s, which re-exports what other crates use from here.
//!
//! **Plain data only.** `dom` keeps a stylesheet's
//! [`StyleSheetContents`] on its GC object in a `elise::External`, so the
//! last `Arc` of anything here may be dropped by a GC finalizer, on a
//! sweep's worker thread. Everything here must therefore, as
//! `computed_style`'s types must:
//!
//! - hold no GC pointers or roots (this crate depends on neither elise nor
//!   V8), and be `Send + Sync + 'static` (checked by `External`'s bounds):
//!   no `Rc`, `Cell`, `RefCell`, raw pointers;
//! - have no hand-written `Drop`, and no interning or global /
//!   thread-local caches (their `Drop` would lock or touch thread-locals).
//!
//! A cache in the value itself is fine: [`StyleSheetContents`]'s rule set,
//! made the first time it is cascaded (and again when its media queries
//! come out otherwise), in a `Mutex` whose `Drop` only frees memory.
//!
//! Changes to these types, and new versions of `cssparser` and
//! `selectors`, whose types are in here too, are reviewed as `External`'s
//! values are; see elise's soundness argument.

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

pub mod animation;
pub mod background;
pub mod border_image;
pub mod calc;
pub mod canvas;
pub mod computed;
pub mod css_namespace;
pub mod css_url_data;
pub mod cursor;
pub mod effects;
pub mod filter;
pub mod font;
pub mod font_features;
pub mod grid;
pub mod interpolate;
pub mod masking;
pub mod media;
pub mod properties;
pub mod property_set;
pub mod rule_set;
pub mod selector;
pub mod serialize;
pub mod shadow;
pub mod stylesheet;
pub mod svg;
pub mod text_decor;
pub mod values;
pub mod variables;

pub use crate::canvas::{
    CanvasColor, CanvasSpacing, SpecifiedCanvasFont, canvas_current_color, canvas_font_parent,
    parse_canvas_color, parse_canvas_font, parse_canvas_spacing, serialize_canvas_color,
    serialize_canvas_font,
};
pub use crate::computed::{
    computable_properties, computed_value, custom_property_names, is_layout_dependent,
    physical_property, resolved_transform, resolved_transform_origin, shorthand_from_style,
    shorthand_longhands, shorthand_value, write_css_number,
};
pub use crate::css_namespace::{escape_identifier, supports_condition, supports_declaration};
pub use crate::css_url_data::UrlContext;
pub use crate::property_set::{
    CssPropertyValueSet, InspectorDeclaration, css_property_attributes, css_property_for_attribute,
};
pub use crate::stylesheet::{
    CssRule, InsertRuleError, KeyframeRule, KeyframesRule, MediaRule, StyleRule,
    StyleSheetContents, SupportsRule, parse_key_text,
};
pub use crate::svg::{parse_d_attribute, path_from_string};
