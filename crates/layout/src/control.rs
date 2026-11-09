//! Form controls' own sizes and baselines: a text field's and a text
//! area's (Blink's `LayoutTextControl`, which is a block container sized
//! by its `size`, `cols` and `rows` and its font, not by its content), a
//! slider's (`LayoutTheme`'s), and a check box's and a radio button's
//! baseline (their size is style's). In a block it is sized as a replaced
//! element is: an `auto` width is its own, not the containing block's
//! (Chrome's); a flex or grid container stretches it as any box. A text
//! control's content is its UA shadow tree's (the inner editor with its
//! value, and the placeholder), laid out in that size; a text field's is
//! vertically centered in it. A menu list is as wide as its widest
//! option's label and a line high, with its native look's room around its
//! UA shadow tree's inner element (Blink's `LayoutBox::
//! MenuListIntrinsicInlineSize`, `MenuListIntrinsicBlockSize`), which is
//! centered in it where there is room. A list box lays its options out as
//! blocks, as high as `size` rows of its tallest (Blink's
//! `ListBoxItemBlockSize`), as wide as they are and a scrollbar.
//!
//! What it reads of the DOM is a `ControlElement`'s.

use computed_style::{ComputedStyle, EffectiveAppearance, ItemAlignment, LineHeight};
use dom::{Cx, NameId, NodeRef, ShadowRootMode, names};
use gfx::SizeF;

/// What a form control's element is to its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlName {
    Input,
    TextArea,
    Select,
    Button,
    Option,
    Slot,
    Other,
}

/// A form control's element as its box reads it (Blink's
/// `LayoutObject::CreateObject` asks the element).
#[derive(Clone, Copy)]
pub(crate) struct ControlElement<'a, 'gc> {
    dom: &'a Cx<'a, 'gc>,
    pub(crate) node: NodeRef<'gc>,
}

impl<'a, 'gc> ControlElement<'a, 'gc> {
    pub(crate) fn new(dom: &'a Cx<'a, 'gc>, node: NodeRef<'gc>) -> Self {
        Self { dom, node }
    }

    /// What it is, an HTML element's by its local name.
    fn control_name(self) -> ControlName {
        use names::local;
        let Some(name) = self.dom.qualified_name(self.node) else {
            return ControlName::Other;
        };
        if name.namespace() != names::HTML_NAMESPACE {
            return ControlName::Other;
        }
        match name.local() {
            local::INPUT => ControlName::Input,
            local::TEXTAREA => ControlName::TextArea,
            local::SELECT => ControlName::Select,
            local::BUTTON => ControlName::Button,
            local::OPTION => ControlName::Option,
            local::SLOT => ControlName::Slot,
            _ => ControlName::Other,
        }
    }

    /// An input's type, as its attribute says, in lowercase (`text`
    /// without one).
    fn input_type(self) -> &'static str {
        self.dom
            .input_kind(self.node)
            .map_or("text", dom::InputKind::keyword)
    }

    /// Its attribute `local`'s value, if it has one: `<input size>`,
    /// `<textarea cols>` and `rows`, a range's thumb's ratio (its UA shadow
    /// tree's), a menu list's inner element's labels (one a line).
    fn attribute(self, local: NameId) -> Option<String> {
        self.dom
            .attribute_value_by_id(self.node, local)
            .map(|value| value.borrow().to_str().into_owned())
    }

    /// Whether it has a shadow root (its UA one, for a control).
    fn has_shadow_root(self) -> bool {
        self.dom.shadow_root(self.node).is_some()
    }

    /// The first child of its UA shadow root, if it has one with one.
    fn user_agent_first_child(self) -> Option<Self> {
        let root = self.dom.user_agent_shadow_root(self.node)?;
        let node = self.dom.first_child(root)?;
        Some(Self { node, ..self })
    }

    /// Its shadow pseudo id, if it is an element of a UA shadow tree.
    fn part(self) -> Option<String> {
        self.dom
            .shadow_pseudo_id(self.node)
            .map(|id| id.to_str().into_owned())
    }

    /// The host of the UA shadow tree it is in, if it is in one.
    pub(crate) fn user_agent_host(self) -> Option<Self> {
        let root = self.dom.containing_shadow_root(self.node)?;
        if self.dom.shadow_root_init(root)?.mode != ShadowRootMode::UserAgent {
            return None;
        }
        let node = self.dom.shadow_host(root)?;
        Some(Self { node, ..self })
    }
}

use crate::replaced::Replaced;
use crate::text::{self, Edge, TextContext};

/// A slider's size, Chrome's: its track's default length and thickness.
const SLIDER_SIZE: SizeF = SizeF::new(129.0, 16.0);

/// The room a text area and a list box keep for their vertical scrollbar,
/// Chrome's (Windows and Linux): machopo draws no scrollbars, but sizes for
/// one.
pub(crate) const SCROLLBAR_THICKNESS: f32 = 15.0;

/// A list box's rows without `size` (Blink's `kDefaultListBoxSize`).
const DEFAULT_LIST_BOX_SIZE: u32 = 4;

/// HTML's defaults: `<input size>`, `<textarea cols>` and `rows`.
const DEFAULT_SIZE: u32 = 20;
const DEFAULT_COLS: u32 = 20;
const DEFAULT_ROWS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ControlKind {
    /// A text-like `<input>`: its text's baseline, centered in it.
    TextField {
        ascent: f32,
        line_height: f32,
    },
    TextArea,
    /// `<input type=range>`.
    Slider,
    /// A native check box or radio button.
    Toggle,
    /// A `<select>` shown as a menu list: its inner element's text's
    /// baseline, the inner element `padding` thick above and below its
    /// line, centered in it if `centered` (where it fits).
    MenuList {
        ascent: f32,
        line_height: f32,
        padding: f32,
        centered: bool,
    },
    /// A `<select>` shown as a list box: its rows, and a row's height
    /// without any (a line of its font and 1px: Blink's
    /// `ListBoxDefaultItemHeight`).
    ListBox {
        rows: u32,
        empty_row: f32,
    },
}

/// A menu list's room for its native look (`LayoutThemeDefault`'s
/// `PopupInternalPadding*`, which its inner element has as its margins and
/// padding): 4px left of its text, 16px right (the arrow's), 1px above
/// and below.
const MENU_LIST_INLINE_PADDING: f32 = 4.0 + 16.0;
const MENU_LIST_BLOCK_PADDING: f32 = 1.0;

/// What a form control's box has of its own.
#[derive(Debug, Clone)]
pub(crate) struct Control {
    pub(crate) kind: ControlKind,
    /// Its content size where its style doesn't give one, as a replaced
    /// element's default object size (no natural ratio); `None` for a
    /// toggle, which style sizes.
    pub(crate) sizing: Option<Replaced>,
    /// Whether it has a shadow tree: its UA one, whose content it lays
    /// out.
    shadow_tree: bool,
}

impl Control {
    /// `element`'s, if it is a form control with a size or a baseline of
    /// its own.
    pub(crate) fn of(
        element: ControlElement<'_, '_>,
        style: &ComputedStyle,
        cx: &mut TextContext,
    ) -> Option<Self> {
        let name = element.control_name();
        let (kind, content) = if name == ControlName::Input {
            match element.input_type() {
                "checkbox" | "radio" => {
                    if !matches!(
                        style.effective_appearance,
                        EffectiveAppearance::Checkbox | EffectiveAppearance::Radio
                    ) {
                        return None;
                    }
                    return Some(Self {
                        kind: ControlKind::Toggle,
                        sizing: None,
                        shadow_tree: false,
                    });
                }
                "range" => (ControlKind::Slider, SLIDER_SIZE),
                // Their size is what their shadow tree holds (later), or
                // they have none.
                "hidden" | "button" | "submit" | "reset" | "image" | "file" | "color" | "date"
                | "datetime-local" | "month" | "time" | "week" => return None,
                // The text-like types, and one HTML doesn't have (text).
                _ => {
                    let metrics = text::control_font_metrics(cx, style);
                    let size =
                        attribute_number(element, names::local::SIZE).unwrap_or(DEFAULT_SIZE);
                    let line_height = line_height(style, &metrics);
                    let width =
                        (size as f32 * metrics.avg_char_width + metrics.max_char_adjustment).ceil();
                    (
                        ControlKind::TextField {
                            ascent: metrics.ascent.round(),
                            line_height,
                        },
                        SizeF::new(width, line_height),
                    )
                }
            }
        } else if let Some(inner) = menu_list_inner(element) {
            // Its options' labels measured in its font, the widest
            // rounded up; a line high (with a native look, its font's:
            // style makes its `line-height` `normal`).
            let metrics = text::control_font_metrics(cx, style);
            let themed = style.effective_appearance != EffectiveAppearance::None;
            let line_height = line_height(style, &metrics);
            let labels = inner.attribute(names::local::LABELS).unwrap_or_default();
            let widest = labels
                .split('\n')
                .filter(|label| !label.is_empty())
                .map(|label| text::shape_line(cx, label, &[(0..label.len(), style)]).full_width())
                .fold(0.0, f32::max);
            let (inline, block) = if themed {
                (MENU_LIST_INLINE_PADDING, MENU_LIST_BLOCK_PADDING)
            } else {
                (0.0, 0.0)
            };
            (
                ControlKind::MenuList {
                    ascent: metrics.ascent.round(),
                    line_height,
                    padding: block,
                    centered: style.align_items == ItemAlignment::Center,
                },
                SizeF::new(widest.ceil() + inline, line_height + 2.0 * block),
            )
        } else if is_list_box(element) {
            let metrics = text::control_font_metrics(cx, style);
            return Some(Self {
                kind: ControlKind::ListBox {
                    rows: attribute_number(element, names::local::SIZE)
                        .unwrap_or(DEFAULT_LIST_BOX_SIZE),
                    empty_row: metrics.ascent.round() + metrics.descent.round() + 1.0,
                },
                sizing: None,
                shadow_tree: true,
            });
        } else if name == ControlName::TextArea {
            let metrics = text::control_font_metrics(cx, style);
            let cols = attribute_number(element, names::local::COLS).unwrap_or(DEFAULT_COLS);
            let rows = attribute_number(element, names::local::ROWS).unwrap_or(DEFAULT_ROWS);
            let width = (cols as f32 * metrics.avg_char_width + SCROLLBAR_THICKNESS).ceil();
            (
                ControlKind::TextArea,
                SizeF::new(width, rows as f32 * line_height(style, &metrics)),
            )
        } else {
            return None;
        };
        Some(Self {
            kind,
            sizing: Some(Replaced::control(content)),
            shadow_tree: element.has_shadow_root(),
        })
    }

    /// Its baseline from its border box's top, `border_box` big; `None`:
    /// its bottom margin edge (a text area, as a box that clips its
    /// overflow).
    pub(crate) fn baseline(&self, border_box: SizeF, edge: &Edge) -> Option<f32> {
        match self.kind {
            // Its text's (empty or not), its line centered in its content
            // box (Blink's text field container centers the inner editor).
            ControlKind::TextField {
                ascent,
                line_height,
            } => {
                let top = edge.border.top + edge.padding.top;
                let content = border_box.height - top - edge.border.bottom - edge.padding.bottom;
                Some(top + (content - line_height) / 2.0 + ascent)
            }
            // Its inner element's text's: where the inner element is
            // centered, at its content box's top where it doesn't fit.
            ControlKind::MenuList {
                ascent,
                line_height,
                padding,
                centered,
            } => {
                let top = edge.border.top + edge.padding.top;
                let content = border_box.height - top - edge.border.bottom - edge.padding.bottom;
                let room = content - line_height - 2.0 * padding;
                let shift = if centered { (room / 2.0).max(0.0) } else { 0.0 };
                Some(top + shift + padding + ascent)
            }
            // Its border box's bottom, its margin below (Chrome's).
            ControlKind::Slider | ControlKind::Toggle => Some(border_box.height),
            ControlKind::TextArea | ControlKind::ListBox { .. } => None,
        }
    }

    /// Whether a box with this control lays out none of its element's
    /// children: a text area's are its default value, not its content
    /// (with its UA shadow tree, its content is the tree's, which comes
    /// in their place).
    pub(crate) fn ignores_children(&self) -> bool {
        self.kind == ControlKind::TextArea && !self.shadow_tree
    }

    /// A list box's rows (its `size`, else 4) and a row's height without
    /// any rows to measure, if it is one.
    pub(crate) fn list_box(&self) -> Option<(u32, f32)> {
        match self.kind {
            ControlKind::ListBox { rows, empty_row } => Some((rows, empty_row)),
            _ => None,
        }
    }

    /// Whether its content is centered in its content box, and whether
    /// safely (at its top where it doesn't fit): a text field's line, not
    /// safely (Blink centers the inner editor); a menu list's inner
    /// element, safely (its `auto` margins), unless its `align-items`
    /// isn't `center`.
    pub(crate) fn centers_content(&self) -> Option<bool> {
        match self.kind {
            ControlKind::TextField { .. } => Some(false),
            ControlKind::MenuList { centered: true, .. } => Some(true),
            _ => None,
        }
    }
}

/// Whether `element` is a `<select>` shown as a list box: its UA shadow
/// tree is a slot of its options.
fn is_list_box(element: ControlElement<'_, '_>) -> bool {
    element.control_name() == ControlName::Select
        && element
            .user_agent_first_child()
            .is_some_and(|first| first.control_name() == ControlName::Slot)
}

/// Whether `element`'s box is a row of a list box (Blink's list items): an
/// option, or an optgroup's label (its UA shadow tree's).
pub(crate) fn is_list_box_row(element: ControlElement<'_, '_>) -> bool {
    element.control_name() == ControlName::Option
        || element.part().as_deref() == Some(OPTGROUP_LABEL)
}

/// The height of the line a text control's inner editor has even when it
/// is empty (Blink's `HasLineIfEmpty`, an editable block's: the inner
/// editor is one), if `element` is one: in a UA shadow tree of an
/// `<input>` or `<textarea>`, the element of the inner editor's shadow
/// pseudo id.
pub(crate) fn empty_line_height(
    element: ControlElement<'_, '_>,
    style: &ComputedStyle,
    cx: &mut TextContext,
) -> Option<f32> {
    let text_control = element.user_agent_host().is_some_and(|host| {
        matches!(
            host.control_name(),
            ControlName::Input | ControlName::TextArea
        )
    });
    let editor = element.part().as_deref() == Some(INNER_EDITOR);
    (text_control && editor).then(|| line_height(style, &text::control_font_metrics(cx, style)))
}

/// A caret's font in an empty editable element of `style` (Blink's
/// `LocalCaretRectForEmptyElement`, of its first available font): its
/// ascent and descent, and the height of the line it would be on, as an
/// empty text control's inner editor has.
pub fn empty_caret_metrics(cx: &mut TextContext, style: &ComputedStyle) -> (f32, f32, f32) {
    let metrics = text::control_font_metrics(cx, style);
    (
        metrics.ascent,
        metrics.descent,
        line_height(style, &metrics),
    )
}

/// Whether `element` is a text control's placeholder (in its UA shadow
/// tree): shown, it is where the empty inner editor's line would be
/// (Blink's `HandleTextControlPlaceholder` lays it out there).
pub(crate) fn is_placeholder(element: ControlElement<'_, '_>) -> bool {
    element.part().as_deref() == Some(PLACEHOLDER)
}

/// A text control's placeholder's shadow pseudo id (`dom`'s).
const PLACEHOLDER: &str = "-webkit-input-placeholder";

/// A text control's inner editor's shadow pseudo id (`dom`'s).
const INNER_EDITOR: &str = "-internal-inner-editor";

/// An optgroup's UA shadow tree's label's shadow pseudo id (`dom`'s).
const OPTGROUP_LABEL: &str = "-internal-optgroup-label";

/// `element`'s UA shadow tree's menu list inner element, if it is a
/// `<select>` shown as one.
fn menu_list_inner<'a, 'gc>(element: ControlElement<'a, 'gc>) -> Option<ControlElement<'a, 'gc>> {
    if element.control_name() != ControlName::Select {
        return None;
    }
    element
        .user_agent_first_child()
        .filter(|inner| inner.part().as_deref() == Some(MENU_LIST_INNER))
}

/// The menu list inner element's shadow pseudo id (`dom`'s).
const MENU_LIST_INNER: &str = "-internal-menulist-inner";

/// Whether `element` has HTML's button layout (Blink's `LayoutButton`): a
/// `<button>`, or an `<input>` of the button types. In a block its `auto`
/// width is fit-content (a flex or grid container stretches it as any
/// box), it is a flow root, and as a block container its content is
/// vertically centered in it.
pub(crate) fn is_button(element: ControlElement<'_, '_>) -> bool {
    element.control_name() == ControlName::Button || is_input_button(element)
}

/// Whether `element` is an `<input>` of the button types, whose content is
/// its label (its UA shadow tree's).
pub(crate) fn is_input_button(element: ControlElement<'_, '_>) -> bool {
    element.control_name() == ControlName::Input
        && matches!(element.input_type(), "button" | "submit" | "reset")
}

/// Whether `element`'s `::before` and `::after` are rendered (Blink's
/// `CanHaveGeneratedChildren`): not a text control's, a `<select>`'s, nor
/// those of an `<input>` of the button types or `image`; a check box's, a
/// slider's and a date field's are, inside its box.
pub(crate) fn has_generated_children(element: ControlElement<'_, '_>) -> bool {
    match element.control_name() {
        ControlName::TextArea | ControlName::Select => false,
        ControlName::Input => matches!(
            element.input_type(),
            "checkbox"
                | "radio"
                | "range"
                | "color"
                | "file"
                | "date"
                | "datetime-local"
                | "month"
                | "time"
                | "week"
        ),
        _ => true,
    }
}

/// A range's thumb's ratio (its UA shadow tree's thumb, which `dom`
/// gives it), if `element` is one.
pub(crate) fn slider_ratio(element: ControlElement<'_, '_>) -> Option<f32> {
    element.user_agent_host()?;
    element
        .attribute(names::local::RATIO)?
        .parse::<f32>()
        .ok()
        .map(|ratio| ratio.clamp(0.0, 1.0))
}

/// The line height of a text control's line, `line-height: normal` as
/// Chrome rounds it (its ascent and descent each).
fn line_height(style: &ComputedStyle, metrics: &text::ControlFontMetrics) -> f32 {
    match style.line_height {
        LineHeight::Normal => metrics.ascent.round() + metrics.descent.round(),
        LineHeight::Number(n) => LineHeight::number_px(n, style.font.size),
        LineHeight::Px(px) => px,
    }
}

/// The attribute `local`'s value by HTML's rules for non-negative integers
/// (its leading digits, after white space), if above 0.
fn attribute_number(element: ControlElement<'_, '_>, attribute: NameId) -> Option<u32> {
    let value = element.attribute(attribute)?;
    let digits = value.trim_start_matches(['\t', '\n', '\x0c', '\r', ' ']);
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok().filter(|&n| n > 0)
}
