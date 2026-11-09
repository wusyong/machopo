//! Presentational hints (HTML's "Rendering" section): attributes that map
//! to CSS. They cascade as author declarations with specificity 0, before
//! every author rule, so any stylesheet or `style` attribute overrides
//! them.
//!
//! HTML's: `<img>`'s and `<iframe>`'s `width` and `height` ("maps to the
//! dimension property"), `<iframe frameborder>`, the lists' (`<ol
//! start>`, `reversed`, `<li value>` and the `type`s), and the tables'
//! (`<table width>`, `border`, `cellspacing`, `cellpadding`, `align`; the
//! cells' and rows' `width`, `height`, `nowrap`, `valign`, `align`; `<col
//! width>`; the parts' `bgcolor` and `background`, `<table bordercolor>`).
//! The legacy ones Chrome has besides: `<body>`'s colors, background and
//! margins, `<font>`'s and `<hr>`'s, as Chrome maps them (Blink's
//! `CollectStyleForPresentationAttribute`). SVG's
//! presentation attributes are these too (SVG 2, "Presentation
//! attributes"; Blink's `SVGElement::CollectStyleForPresentationAttribute`).

use std::sync::Arc;

use cssparser::{Parser, ParserInput};
use rustc_hash::FxHashMap;

use css::properties::{Declaration, LonghandId, SizeValue, Value, parse_declaration};
use css::svg::{parse_d_attribute, parse_transform_attribute};
use css::values::{Length, LengthPercentage, LineWidth};

/// SVG presentation attributes' declarations by the attribute's name and
/// value: elements alike (a chart's many `fill="none"`s, React making them
/// anew) parse each once. Which element takes which attribute is decided
/// before, so the element isn't part of the key. Blink's
/// `PresentationAttributeCache` keeps an element's all together; one by
/// one, an element that differs in one still shares the rest. A path's
/// `d`, rarely the same twice, isn't kept here (only on its element).
/// Keyed by the attribute's name as the DOM has it (`K`: in `style`,
/// the name's number).
pub struct PresentationAttributeCache<K> {
    entries: FxHashMap<K, FxHashMap<Box<str>, Arc<[Declaration]>>>,
    len: usize,
}

impl<K> Default for PresentationAttributeCache<K> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            len: 0,
        }
    }
}

/// How many values the presentation attribute cache keeps before it
/// starts again (Blink's keeps 4096 elements').
const PRESENTATION_ATTRIBUTE_CACHE_SIZE: usize = 4096;

impl<K: Clone + Eq + std::hash::Hash> PresentationAttributeCache<K> {
    /// Attribute `name`'s declarations with `value`, parsed by `parse` if
    /// not kept.
    pub fn get(
        &mut self,
        name: &K,
        value: &str,
        parse: impl FnOnce() -> Vec<Declaration>,
    ) -> Arc<[Declaration]> {
        if let Some(found) = self.entries.get(name).and_then(|values| values.get(value)) {
            return found.clone();
        }
        if self.len >= PRESENTATION_ATTRIBUTE_CACHE_SIZE {
            self.entries.clear();
            self.len = 0;
        }
        let declarations: Arc<[Declaration]> = parse().into();
        self.entries
            .entry(name.clone())
            .or_default()
            .insert(value.into(), declarations.clone());
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "below PRESENTATION_ATTRIBUTE_CACHE_SIZE: cleared when it gets there"
        )]
        {
            self.len += 1;
        }
        declarations
    }

    /// For tests: how many values it keeps.
    pub fn len_for_testing(&self) -> usize {
        self.len
    }
}

/// An HTML element whose attributes may be presentational hints, by its
/// local name.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintElement {
    Ol,
    Ul,
    Li,
    Table,
    Td,
    Th,
    Tr,
    Thead,
    Tbody,
    Tfoot,
    Col,
    Colgroup,
    Body,
    Font,
    Hr,
    Img,
    Iframe,
}

/// The attributes HTML elements' presentational hints are of.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintAttribute {
    Align,
    Background,
    Bgcolor,
    Border,
    Bordercolor,
    Cellpadding,
    Cellspacing,
    Color,
    Face,
    Frameborder,
    Height,
    Leftmargin,
    Marginheight,
    Marginwidth,
    Noshade,
    Nowrap,
    Reversed,
    Size,
    Start,
    Text,
    Topmargin,
    Type,
    Valign,
    Value,
    Width,
}

/// What an HTML element's presentational hints read of it
/// ([`html_hints`]): its attributes, and a cell's table.
#[doc(hidden)]
pub trait HintSource: Sized {
    fn attribute(&self, name: HintAttribute) -> Option<String>;
    /// The values of those of `names` it has, in its attributes' order.
    fn in_order(&self, names: &[HintAttribute]) -> Vec<(HintAttribute, String)>;
    /// The `<table>` whose cell it is, by HTML's table model: its `<tr>`'s
    /// parent, or its row group's. Chrome takes the nearest `<table>`
    /// ancestor in the flat tree (`HTMLTablePartElement::FindParentTable`),
    /// the same one except in markup HTML's model has no cell for.
    fn cell_table(&self) -> Option<Self>;
}

/// HTML element `element`'s presentational hints, as `source` has its
/// attributes: the lists', the tables', the legacy ones, and `<img>`'s and
/// `<iframe>`'s.
#[doc(hidden)]
pub fn html_hints(element: HintElement, source: &impl HintSource) -> Vec<Declaration> {
    let mut out = Vec::new();
    list_declarations(element, source, &mut out);
    table_declarations(element, source, &mut out);
    legacy_declarations(element, source, &mut out);
    let iframe = element == HintElement::Iframe;
    if element != HintElement::Img && !iframe {
        return out;
    }
    // Blink's `HTMLIFrameElement::CollectStyleForPresentationAttribute`:
    // `frameborder` other than a nonzero integer takes the UA stylesheet's
    // border off (its width; the HTML standard has `0` and `no` only).
    if iframe
        && let Some(value) = source.attribute(HintAttribute::Frameborder)
        && value.trim().parse::<i32>().unwrap_or(0) == 0
    {
        for id in [
            LonghandId::BorderTopWidth,
            LonghandId::BorderRightWidth,
            LonghandId::BorderBottomWidth,
            LonghandId::BorderLeftWidth,
        ] {
            out.push(Declaration {
                id,
                value: Value::LineWidth(LineWidth::Length(Length::px(0.0))),
            });
        }
    }
    for (attribute, id) in [
        (HintAttribute::Width, LonghandId::Width),
        (HintAttribute::Height, LonghandId::Height),
    ] {
        if let Some(value) = source.attribute(attribute).and_then(|v| dimension(&v)) {
            out.push(Declaration {
                id,
                value: Value::Size(SizeValue::LengthPercentage(value)),
            });
        }
    }
    out
}

/// `<body>`'s, `<font>`'s and `<hr>`'s presentational hints, as Chrome has
/// them (Blink's `CollectStyleForPresentationAttribute` of
/// `HTMLBodyElement`, `HTMLFontElement` and `HTMLHRElement`): `<body>`'s
/// background, text color and margins (its `link` and `alink` are the
/// document's link colors, `-webkit-link`'s); `<font>`'s color, face and
/// legacy size; `<hr>`'s alignment, width, color, `noshade` and size, which
/// Chrome maps otherwise than HTML (`color` the border's and the
/// background's, `size` a height).
fn legacy_declarations(element: HintElement, source: &impl HintSource, out: &mut Vec<Declaration>) {
    let attribute = |name| source.attribute(name);
    if element == HintElement::Body {
        background_declarations(source, out);
        if let Some(color) = legacy_color(source, HintAttribute::Text) {
            push_color(out, &[LonghandId::Color], color);
        }
        // In the attributes' order, a later one winning (Blink collects
        // them so).
        let margins = source.in_order(&[
            HintAttribute::Marginwidth,
            HintAttribute::Leftmargin,
            HintAttribute::Marginheight,
            HintAttribute::Topmargin,
        ]);
        for (name, value) in margins {
            let sides: &[LonghandId] =
                if matches!(name, HintAttribute::Marginwidth | HintAttribute::Leftmargin) {
                    &[LonghandId::MarginRight, LonghandId::MarginLeft]
                } else {
                    &[LonghandId::MarginBottom, LonghandId::MarginTop]
                };
            if let Some(length) = dimension(&value) {
                for &id in sides {
                    out.push(Declaration {
                        id,
                        value: Value::AutoOr(Some(length.clone())),
                    });
                }
            }
        }
        return;
    }
    if element == HintElement::Font {
        if let Some(color) = legacy_color(source, HintAttribute::Color) {
            push_color(out, &[LonghandId::Color], color);
        }
        if let Some(face) = attribute(HintAttribute::Face).filter(|face| !face.is_empty()) {
            out.extend(property_declarations("font-family", &face));
        }
        if let Some(size) = attribute(HintAttribute::Size).and_then(|size| legacy_font_size(&size))
        {
            out.extend(property_declarations("font-size", size));
        }
        return;
    }
    if element != HintElement::Hr {
        return;
    }
    if let Some(align) = attribute(HintAttribute::Align) {
        let auto = Value::AutoOr(None);
        let zero = Value::AutoOr(Some(LengthPercentage::Length(Length::px(0.0))));
        let (left, right) = if align.eq_ignore_ascii_case("left") {
            (zero, auto)
        } else if align.eq_ignore_ascii_case("right") {
            (auto, zero)
        } else {
            (auto.clone(), auto)
        };
        out.push(Declaration {
            id: LonghandId::MarginLeft,
            value: left,
        });
        out.push(Declaration {
            id: LonghandId::MarginRight,
            value: right,
        });
    }
    if let Some(width) = attribute(HintAttribute::Width).and_then(|width| dimension(&width)) {
        out.push(Declaration {
            id: LonghandId::Width,
            value: Value::Size(SizeValue::LengthPercentage(width)),
        });
    }
    if attribute(HintAttribute::Color).is_some() {
        push_border_style(out, computed_style::BorderStyle::Solid);
        if let Some(color) = legacy_color(source, HintAttribute::Color) {
            push_color(out, &BORDER_COLOR, color);
            push_color(out, &[LonghandId::BackgroundColor], color);
        }
    } else if attribute(HintAttribute::Noshade).is_some() {
        push_border_style(out, computed_style::BorderStyle::Solid);
        let gray = gfx::Color::rgb(128, 128, 128);
        push_color(out, &BORDER_COLOR, gray);
        push_color(out, &[LonghandId::BackgroundColor], gray);
    }
    if let Some(size) = attribute(HintAttribute::Size) {
        // Its integer (`5px` is 5), none being 0.
        let size = parse_integer(&size).unwrap_or(0);
        if size <= 1 {
            out.push(Declaration {
                id: LonghandId::BorderBottomWidth,
                value: Value::LineWidth(LineWidth::Length(Length::px(0.0))),
            });
        } else {
            #[expect(clippy::arithmetic_side_effects, reason = "size > 1")]
            let height = size - 2;
            out.push(Declaration {
                id: LonghandId::Height,
                value: Value::Size(SizeValue::LengthPercentage(LengthPercentage::Length(
                    Length::px(height as f32),
                ))),
            });
        }
    }
}

/// HTML's rules for parsing a legacy font size (`<font size>`): 1 to 7, or
/// relative to 3 with a sign, held to that range; the keyword of that size
/// (Blink's `HTMLFontElement::CssValueFromFontSizeNumber`).
fn legacy_font_size(value: &str) -> Option<&'static str> {
    let value = value.trim_start_matches(HTML_WHITESPACE);
    let (sign, rest) = match value.as_bytes().first()? {
        b'+' => (1, &value[1..]),
        b'-' => (-1, &value[1..]),
        _ => (0, value),
    };
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let number: i64 = rest[..end].parse().unwrap_or(i64::MAX);
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "number is of digits, not negative: 3 less up to 3 of it"
    )]
    let size = match sign {
        1 => number.saturating_add(3),
        -1 => 3 - number.min(3),
        _ => number,
    };
    const SIZES: [&str; 7] = [
        "x-small",
        "small",
        "medium",
        "large",
        "x-large",
        "xx-large",
        "xxx-large",
    ];
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::cast_sign_loss,
        reason = "clamped to 1 to 7"
    )]
    let index = size.clamp(1, 7) as usize - 1;
    Some(SIZES[index])
}

/// The list elements' presentational hints (HTML, "Lists"): `<ol start>`
/// and `reversed` reset the `list-item` counter (to one before the start,
/// or a reversed one from one after it, or from its items' count), `<li
/// value>` sets it, and `type` is the `list-style-type` (`<ol>`'s and
/// `<li>`'s numbering ones case-sensitive, `<ul>`'s and `<li>`'s bullets
/// not).
fn list_declarations(element: HintElement, source: &impl HintSource, out: &mut Vec<Declaration>) {
    use computed_style::{CounterChange, CounterReset, ListStyleType};
    let (ol, ul, li) = (
        element == HintElement::Ol,
        element == HintElement::Ul,
        element == HintElement::Li,
    );
    if ol {
        let start = source
            .attribute(HintAttribute::Start)
            .and_then(|value| parse_integer(&value));
        let reversed = source.attribute(HintAttribute::Reversed).is_some();
        let reset = match (reversed, start) {
            (true, start) => Some(CounterReset {
                name: "list-item".into(),
                value: start.map(|start| start.saturating_add(1)),
                reversed: true,
            }),
            (false, Some(start)) => Some(CounterReset {
                name: "list-item".into(),
                value: Some(start.saturating_sub(1)),
                reversed: false,
            }),
            (false, None) => None,
        };
        if let Some(reset) = reset {
            out.push(Declaration {
                id: LonghandId::CounterReset,
                value: Value::CounterReset([reset].into()),
            });
        }
    }
    if li
        && let Some(value) = source
            .attribute(HintAttribute::Value)
            .and_then(|value| parse_integer(&value))
    {
        out.push(Declaration {
            id: LonghandId::CounterSet,
            value: Value::CounterChanges(
                [CounterChange {
                    name: "list-item".into(),
                    value,
                }]
                .into(),
            ),
        });
    }
    if !(ol || ul || li) {
        return;
    }
    let Some(kind) = source.attribute(HintAttribute::Type) else {
        return;
    };
    let numbered = match kind.as_str() {
        "1" => Some(ListStyleType::Decimal),
        "a" => Some(ListStyleType::LowerAlpha),
        "A" => Some(ListStyleType::UpperAlpha),
        "i" => Some(ListStyleType::LowerRoman),
        "I" => Some(ListStyleType::UpperRoman),
        _ => None,
    };
    let bullet = match kind.to_ascii_lowercase().as_str() {
        "none" => Some(ListStyleType::None),
        "disc" => Some(ListStyleType::Disc),
        "circle" => Some(ListStyleType::Circle),
        "square" => Some(ListStyleType::Square),
        _ => None,
    };
    let style = if ol {
        numbered
    } else if ul {
        bullet
    } else {
        numbered.or(bullet)
    };
    if let Some(style) = style {
        out.push(Declaration {
            id: LonghandId::ListStyleType,
            value: Value::ListStyleType(style),
        });
    }
}

/// The table elements' presentational hints (HTML, "Tables"; Blink's
/// `CollectStyleForPresentationAttribute` of `HTMLTableElement`,
/// `HTMLTablePartElement`, `HTMLTableCellElement` and
/// `HTMLTableColElement`). A cell takes its table's `cellpadding` and
/// `border` too: the table's attribute changing styles its whole subtree
/// again, the cells with it (`for_descendants`).
fn table_declarations(element: HintElement, source: &impl HintSource, out: &mut Vec<Declaration>) {
    let attribute = |name| source.attribute(name);
    let size = |id, value: LengthPercentage| Declaration {
        id,
        value: Value::Size(SizeValue::LengthPercentage(value)),
    };
    // "Maps to the dimension property (ignoring zero)".
    let non_zero = |value: &LengthPercentage| {
        !matches!(value, LengthPercentage::Length(l) if *l == Length::px(0.0))
            && *value != LengthPercentage::Percent(0.0)
    };
    use HintElement as E;
    let table = element == E::Table;
    let cell = matches!(element, E::Td | E::Th);
    let row = element == E::Tr;
    let section = matches!(element, E::Thead | E::Tbody | E::Tfoot);
    if table {
        if let Some(width) = attribute(HintAttribute::Width)
            .and_then(|v| dimension(&v))
            .filter(non_zero)
        {
            out.push(size(LonghandId::Width, width));
        }
        if let Some(height) = attribute(HintAttribute::Height).and_then(|v| dimension(&v)) {
            out.push(size(LonghandId::Height, height));
        }
        if let Some(width) = table_border(source) {
            for id in BORDER_WIDTH {
                out.push(Declaration {
                    id,
                    value: Value::LineWidth(LineWidth::Length(Length::px(width as f32))),
                });
            }
            // Blink's `AdditionalPresentationAttributeStyle`; HTML's
            // `table[border]` rule, "only if border is not equivalent to
            // zero". With a `bordercolor`, solid (Chrome's).
            if width > 0 {
                let style = if has_border_color(source) {
                    computed_style::BorderStyle::Solid
                } else {
                    computed_style::BorderStyle::Outset
                };
                push_border_style(out, style);
            }
        }
        // `bordercolor`, which HTML doesn't map but Chrome does
        // (`HTMLTableElement`): the sections and rows inherit it (the UA
        // stylesheet's), and the cells take it (their hints).
        if let Some(color) = legacy_color(source, HintAttribute::Bordercolor) {
            push_color(out, &BORDER_COLOR, color);
        }
        background_declarations(source, out);
        // "Maps to the pixel length property": Chrome takes a dimension,
        // a fraction too, but no percentage.
        if let Some(LengthPercentage::Length(spacing)) =
            attribute(HintAttribute::Cellspacing).and_then(|v| dimension(&v))
        {
            for id in [
                LonghandId::WebkitBorderHorizontalSpacing,
                LonghandId::WebkitBorderVerticalSpacing,
            ] {
                out.push(Declaration {
                    id,
                    value: Value::Length(spacing.clone()),
                });
            }
        }
        if let Some(align) = attribute(HintAttribute::Align) {
            if align.eq_ignore_ascii_case("center") {
                for id in [LonghandId::MarginInlineStart, LonghandId::MarginInlineEnd] {
                    out.push(Declaration {
                        id,
                        value: Value::AutoOr(None),
                    });
                }
            } else {
                let float = match align.to_ascii_lowercase().as_str() {
                    "left" => Some(computed_style::Float::Left),
                    "right" => Some(computed_style::Float::Right),
                    _ => None,
                };
                if let Some(float) = float {
                    out.push(Declaration {
                        id: LonghandId::Float,
                        value: Value::Float(float),
                    });
                }
            }
        }
        return;
    }
    let column = matches!(element, E::Col | E::Colgroup);
    if !(cell || row || section || column) {
        return;
    }
    // The table parts' `bgcolor` and `background` (Blink's
    // `HTMLTablePartElement`, a column's too).
    background_declarations(source, out);
    if column {
        // `<col width>`, and `<colgroup width>`, which HTML doesn't map but
        // Chrome does (`HTMLTableColElement`; WPT's tables expect it).
        if let Some(width) = attribute(HintAttribute::Width).and_then(|v| dimension(&v)) {
            out.push(size(LonghandId::Width, width));
        }
        return;
    }
    // `valign` and `align` parsed as the property's CSS (Blink's
    // `HTMLTablePartElement`; HTML's keywords are CSS's), and `align`'s
    // `middle` and `absmiddle` centered. Chrome's `center`, `left` and
    // `right` are `-webkit-center` and its kin, which align block children
    // too: machopo has the plain keywords only.
    if let Some(valign) = attribute(HintAttribute::Valign) {
        out.extend(property_declarations("vertical-align", &valign));
    }
    if let Some(align) = attribute(HintAttribute::Align) {
        if align.eq_ignore_ascii_case("middle") || align.eq_ignore_ascii_case("absmiddle") {
            out.push(Declaration {
                id: LonghandId::TextAlign,
                value: Value::TextAlign(computed_style::TextAlign::Center),
            });
        } else {
            out.extend(property_declarations("text-align", &align));
        }
    }
    if row && let Some(height) = attribute(HintAttribute::Height).and_then(|v| dimension(&v)) {
        out.push(size(LonghandId::Height, height));
    }
    if !cell {
        return;
    }
    if attribute(HintAttribute::Nowrap).is_some() {
        out.push(Declaration {
            id: LonghandId::WhiteSpaceCollapse,
            value: Value::WhiteSpaceCollapse(computed_style::WhiteSpaceCollapse::Collapse),
        });
        out.push(Declaration {
            id: LonghandId::TextWrapMode,
            value: Value::TextWrapMode(computed_style::TextWrapMode::Nowrap),
        });
    }
    for (name, id) in [
        (HintAttribute::Width, LonghandId::Width),
        (HintAttribute::Height, LonghandId::Height),
    ] {
        if let Some(value) = attribute(name).and_then(|v| dimension(&v)).filter(non_zero) {
            out.push(size(id, value));
        }
    }
    let Some(table) = source.cell_table() else {
        return;
    };
    // The table's `border`: each cell's is `1px inset` in its own color, or
    // solid in the table's `bordercolor` (Blink's
    // `HTMLTableElement::CreateSharedCellStyle`). Not the table's
    // `border-color` from CSS, which Chrome 152's cells inherit (a bug,
    // crbug.com/40745409; WPT's `table-border-color-not-inherited`).
    if table_border(&table).is_some_and(|width| width > 0) {
        for id in BORDER_WIDTH {
            out.push(Declaration {
                id,
                value: Value::LineWidth(LineWidth::Length(Length::px(1.0))),
            });
        }
        let style = if has_border_color(&table) {
            computed_style::BorderStyle::Solid
        } else {
            computed_style::BorderStyle::Inset
        };
        push_border_style(out, style);
        if let Some(color) = legacy_color(&table, HintAttribute::Bordercolor) {
            push_color(out, &BORDER_COLOR, color);
        }
    }
    // "Maps to the pixel length property": a non-negative integer.
    if let Some(padding) = table
        .attribute(HintAttribute::Cellpadding)
        .and_then(|value| parse_integer(&value))
        .filter(|padding| *padding >= 0)
    {
        for id in [
            LonghandId::PaddingTop,
            LonghandId::PaddingRight,
            LonghandId::PaddingBottom,
            LonghandId::PaddingLeft,
        ] {
            out.push(Declaration {
                id,
                value: Value::LengthPercentage(LengthPercentage::Length(Length::px(
                    padding as f32,
                ))),
            });
        }
    }
}

/// Whether a `<table>` has a `bordercolor` (Blink's `border_color_attr_`):
/// its border and its cells' are solid. A value that isn't a color counts.
fn has_border_color(table: &impl HintSource) -> bool {
    table
        .attribute(HintAttribute::Bordercolor)
        .is_some_and(|value| !value.is_empty())
}

/// An attribute's legacy color (`bgcolor`), if it has one (HTML's rules for
/// parsing a legacy colour value; Blink's `AddHTMLColorToStyle`).
fn legacy_color(element: &impl HintSource, name: HintAttribute) -> Option<gfx::Color> {
    element
        .attribute(name)
        .and_then(|value| css::values::parse_legacy_color(&value))
}

fn push_color(out: &mut Vec<Declaration>, ids: &[LonghandId], color: gfx::Color) {
    for &id in ids {
        out.push(Declaration {
            id,
            value: Value::Color(css::values::SpecifiedColor::Rgba(color)),
        });
    }
}

/// `bgcolor` and `background`: the background's color and image (Blink's
/// `HTMLBodyElement`, `HTMLTableElement` and `HTMLTablePartElement`). The
/// image's URL is resolved with the rest (`declarations`).
fn background_declarations(element: &impl HintSource, out: &mut Vec<Declaration>) {
    if let Some(color) = legacy_color(element, HintAttribute::Bgcolor) {
        push_color(out, &[LonghandId::BackgroundColor], color);
    }
    let image = element
        .attribute(HintAttribute::Background)
        .map(|value| value.trim_matches(HTML_WHITESPACE).to_owned())
        .filter(|value| !value.is_empty());
    if let Some(url) = image {
        let mut css = String::from("url(");
        cssparser::serialize_string(&url, &mut css).expect("writing to a String");
        css.push(')');
        out.extend(property_declarations("background-image", &css));
    }
}

/// HTML's ASCII whitespace.
const HTML_WHITESPACE: [char; 5] = [' ', '\t', '\n', '\x0c', '\r'];

const BORDER_COLOR: [LonghandId; 4] = [
    LonghandId::BorderTopColor,
    LonghandId::BorderRightColor,
    LonghandId::BorderBottomColor,
    LonghandId::BorderLeftColor,
];

const BORDER_WIDTH: [LonghandId; 4] = [
    LonghandId::BorderTopWidth,
    LonghandId::BorderRightWidth,
    LonghandId::BorderBottomWidth,
    LonghandId::BorderLeftWidth,
];

fn push_border_style(out: &mut Vec<Declaration>, style: computed_style::BorderStyle) {
    for id in [
        LonghandId::BorderTopStyle,
        LonghandId::BorderRightStyle,
        LonghandId::BorderBottomStyle,
        LonghandId::BorderLeftStyle,
    ] {
        out.push(Declaration {
            id,
            value: Value::BorderStyle(style),
        });
    }
}

/// A `<table>`'s `border` in px, `None` without one: a non-negative
/// integer, 1 if it isn't one (HTML; Blink's `ParseBorderWidthAttribute`).
fn table_border(table: &impl HintSource) -> Option<u32> {
    let value = table.attribute(HintAttribute::Border)?;
    Some(
        parse_integer(&value)
            .and_then(|width| u32::try_from(width).ok())
            .unwrap_or(1),
    )
}

/// `value` parsed as property `name`'s CSS, as declarations: none if it
/// doesn't parse.
fn property_declarations(name: &str, value: &str) -> Vec<Declaration> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    parse_declaration(name, &mut parser)
        .map(|(_, declarations)| {
            declarations
                .into_iter()
                .map(|(declaration, _)| declaration)
                .collect()
        })
        .unwrap_or_default()
}

/// HTML's "rules for parsing integers": white space, a sign, then the
/// digits up to the first that isn't one (`10xyz` is 10), held to what an
/// `i32` holds.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "n is at most 2^31 before each digit, so n * 10 + 9 and -n fit an i64; \
              clamped to an i32's range before the cast"
)]
fn parse_integer(value: &str) -> Option<i32> {
    let value = value.trim_start_matches(['\t', '\n', '\x0c', '\r', ' ']);
    let (negative, rest) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit);
    let mut any = false;
    let mut n: i64 = 0;
    for digit in digits {
        any = true;
        n = (n * 10 + i64::from(digit - b'0')).min(i64::from(i32::MAX) + 1);
    }
    if !any {
        return None;
    }
    let n = if negative { -n } else { n };
    Some(n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

/// The properties whose presentation attributes every SVG element has,
/// named as the property (SVG 2's list, and Chrome's), those machopo doesn't
/// support left out by the parse. `transform` and the geometry
/// properties are on some elements only ([`svg_declarations`]).
const SVG_PRESENTATION_ATTRIBUTES: [&str; 61] = [
    "alignment-baseline",
    "baseline-shift",
    "clip-path",
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "color-rendering",
    "cursor",
    "direction",
    "display",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "flood-color",
    "flood-opacity",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "image-rendering",
    "letter-spacing",
    "lighting-color",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "mask-type",
    "opacity",
    "overflow",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-decoration",
    "text-overflow",
    "text-rendering",
    "transform-origin",
    "unicode-bidi",
    "vector-effect",
    "visibility",
    "white-space",
    "word-spacing",
    "writing-mode",
];

/// What an SVG element's attribute in no namespace gives as a
/// presentation attribute ([`svg_hint`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgHint {
    /// A `<path>`'s `d`.
    D,
    /// Its transform (`transform`, a gradient's `gradientTransform`, a
    /// pattern's `patternTransform`).
    Transform,
    /// The property of its name.
    Property,
}

/// Whether SVG element `local`'s attribute `attribute` (in no namespace)
/// is one of its presentation attributes, and of what. The style engine
/// asks this, then parses it ([`svg_hint_declarations`]).
pub fn svg_hint(local: &str, attribute: &str) -> Option<SvgHint> {
    let is = |names: &[&str]| names.contains(&local);
    // Where each geometry property's attribute is one, as in Chrome
    // (SVG 2 lists the same, but for `<mask>`'s).
    let geometry = match attribute {
        "cx" | "cy" => is(&["circle", "ellipse"]),
        "r" => local == "circle",
        "rx" | "ry" => is(&["ellipse", "rect"]),
        "x" | "y" => is(&[
            "foreignObject",
            "image",
            "mask",
            "rect",
            "svg",
            "symbol",
            "use",
        ]),
        "width" | "height" => is(&["foreignObject", "image", "mask", "rect", "svg", "symbol"]),
        _ => false,
    };
    // The elements that take a transform (SVG 2's `SVGGraphicsElement`s
    // and `<clipPath>`; Chrome's, without `<textPath>` and `<tspan>`), by
    // the attribute's name on each.
    let transform = match local {
        "a" | "circle" | "clipPath" | "defs" | "ellipse" | "foreignObject" | "g" | "image"
        | "line" | "path" | "polygon" | "polyline" | "rect" | "svg" | "switch" | "symbol"
        | "text" | "use" => Some("transform"),
        "linearGradient" | "radialGradient" => Some("gradientTransform"),
        "pattern" => Some("patternTransform"),
        _ => None,
    };
    // The animation elements' `fill` is theirs, not the property.
    let animation = is(&["animate", "animateMotion", "animateTransform", "set"]);
    if attribute == "d" && local == "path" {
        Some(SvgHint::D)
    } else if Some(attribute) == transform {
        Some(SvgHint::Transform)
    } else if geometry
        || (SVG_PRESENTATION_ATTRIBUTES.contains(&attribute) && !(animation && attribute == "fill"))
    {
        Some(SvgHint::Property)
    } else {
        None
    }
}

/// The names of the attributes any SVG element's presentation attributes
/// may be (`svg_hint`'s): what the DOM notes changes of.
pub fn svg_hint_attribute_names() -> impl Iterator<Item = &'static str> {
    SVG_PRESENTATION_ATTRIBUTES.into_iter().chain([
        "cx",
        "cy",
        "r",
        "rx",
        "ry",
        "x",
        "y",
        "width",
        "height",
        "d",
        "transform",
        "gradientTransform",
        "patternTransform",
    ])
}

/// Attribute `attribute` with `value` as the presentation attribute
/// `hint` says it is: a value is parsed as the property's in CSS, but a
/// plain number is a length in user units (Blink's `kSVGAttributeMode`);
/// one that doesn't parse gives none.
pub fn svg_hint_declarations(hint: SvgHint, attribute: &str, value: &str) -> Vec<Declaration> {
    match hint {
        SvgHint::D => parse_d_attribute(value)
            .map(|segments| Declaration {
                id: LonghandId::D,
                value: Value::Path(Some(segments)),
            })
            .into_iter()
            .collect(),
        SvgHint::Transform => parse_transform_attribute(value)
            .map(|functions| Declaration {
                id: LonghandId::Transform,
                value: Value::Transform(functions),
            })
            .into_iter()
            .collect(),
        SvgHint::Property => svg_attribute_declarations(attribute, value),
    }
}

/// Attribute `name`'s value parsed as the property of that name's, each
/// plain number taken as px where the property's CSS has no numbers
/// (`transform-origin="75 center"`).
fn svg_attribute_declarations(name: &str, value: &str) -> Vec<Declaration> {
    let parse = |value: &str| {
        let mut input = ParserInput::new(value);
        let mut parser = Parser::new(&mut input);
        parse_declaration(name, &mut parser)
            .ok()
            .map(|(_, declarations)| {
                declarations
                    .into_iter()
                    .map(|(declaration, _)| declaration)
                    .collect::<Vec<_>>()
            })
    };
    if let Some(declarations) = parse(value) {
        return declarations;
    }
    let mut numbers = false;
    let in_px: Vec<String> = value
        .split_ascii_whitespace()
        .map(|word| {
            if word.parse::<f32>().is_ok() {
                numbers = true;
                format!("{word}px")
            } else {
                word.to_owned()
            }
        })
        .collect();
    if numbers {
        return parse(&in_px.join(" ")).unwrap_or_default();
    }
    Vec::new()
}

/// HTML's "rules for parsing dimension values": leading white space, then
/// digits with an optional fraction, then optionally `%`; whatever follows
/// is ignored. `None` if there are no digits.
fn dimension(value: &str) -> Option<LengthPercentage> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let (number, rest) = value.split_at(digits);
    // "1." is 1, ".5" is nothing: the number starts with a digit.
    if !number.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let number = number.split('.').take(2).collect::<Vec<_>>();
    let number: f32 = match number.as_slice() {
        [whole] => whole.parse().ok()?,
        [whole, ""] => whole.parse().ok()?,
        [whole, fraction] => format!("{whole}.{fraction}").parse().ok()?,
        _ => return None,
    };
    Some(if rest.starts_with('%') {
        LengthPercentage::Percent(number)
    } else {
        LengthPercentage::Length(Length::px(number))
    })
}

#[cfg(test)]
mod tests {
    use css::values::{Length, LengthPercentage};

    use super::dimension;

    #[test]
    fn dimension_values() {
        let px = |v| Some(LengthPercentage::Length(Length::px(v)));
        assert_eq!(dimension("40"), px(40.0));
        assert_eq!(dimension("  12.5px"), px(12.5));
        assert_eq!(dimension("7."), px(7.0));
        assert_eq!(dimension("50%"), Some(LengthPercentage::Percent(50.0)));
        assert_eq!(dimension("3.2.1"), px(3.2));
        assert_eq!(dimension(".5"), None);
        assert_eq!(dimension("auto"), None);
        assert_eq!(dimension(""), None);
    }
}
