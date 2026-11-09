//! Which native look a form control's style gives it (Blink's
//! `LayoutTheme::AdjustAppearanceWithElementType`,
//! `AdjustAppearanceWithAuthorStyle` and `IsControlStyled`): what
//! `appearance` comes to for the element, unless the author's background
//! or border draws it as CSS does.

use computed_style::{
    Appearance, ComputedStyle, EffectiveAppearance, ItemAlignment, LengthPercentage,
    LengthPercentageOrAuto, Size,
};

/// A form control: the native look `appearance: auto` gives it (Blink's
/// `LayoutTheme::AutoAppearanceFor`; `None` for one without).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Control {
    part: EffectiveAppearance,
    /// Whether it is a menu list's inner element, which has no native look
    /// but the room the menu list's gives it.
    menu_list_inner: bool,
}

impl Control {
    /// An HTML element's of local name `local`, if it is a form control:
    /// an `<input>` by its type (`input_type`, in lowercase), a `<select>`
    /// by whether it is shown as a menu list (`menu_list`).
    pub fn for_element(local: &str, input_type: &str, menu_list: bool) -> Option<Self> {
        let part = match local {
            "input" => match input_type {
                "checkbox" => EffectiveAppearance::Checkbox,
                "radio" => EffectiveAppearance::Radio,
                "range" => EffectiveAppearance::SliderHorizontal,
                "button" | "submit" | "reset" => EffectiveAppearance::PushButton,
                "color" => EffectiveAppearance::SquareButton,
                "search" => EffectiveAppearance::SearchField,
                "hidden" | "image" | "file" => EffectiveAppearance::None,
                // The text-like types, and a type HTML doesn't have (a
                // text field).
                _ => EffectiveAppearance::TextField,
            },
            "button" => EffectiveAppearance::PushButton,
            "textarea" => EffectiveAppearance::TextArea,
            "select" if menu_list => EffectiveAppearance::Menulist,
            "select" => EffectiveAppearance::Listbox,
            "meter" => EffectiveAppearance::Meter,
            "progress" => EffectiveAppearance::ProgressBar,
            _ => return None,
        };
        Some(Self {
            part,
            menu_list_inner: false,
        })
    }

    /// A part of a control's UA shadow tree's, by its shadow pseudo id
    /// `id`, if it has a native look (or is a menu list's inner element).
    pub fn for_part(id: &str) -> Option<Self> {
        let part = match id {
            "-webkit-search-cancel-button" => EffectiveAppearance::SearchFieldCancelButton,
            "-webkit-inner-spin-button" => EffectiveAppearance::InnerSpinButton,
            "-webkit-slider-thumb" => EffectiveAppearance::SliderThumbHorizontal,
            MENU_LIST_INNER => {
                return Some(Self {
                    part: EffectiveAppearance::None,
                    menu_list_inner: true,
                });
            }
            _ => return None,
        };
        Some(Self {
            part,
            menu_list_inner: false,
        })
    }

    /// Whether it is a menu list's inner element.
    pub(crate) fn is_menu_list_inner(self) -> bool {
        self.menu_list_inner
    }
}

/// A menu list's UA shadow tree's inner element's shadow pseudo id
/// (`dom`'s).
const MENU_LIST_INNER: &str = "-internal-menulist-inner";

/// A menu list's inner element's room in it (Blink's
/// `MenuListInnerElement::CustomStyleForLayoutObject`, with
/// `LayoutThemeDefault`'s `PopupInternalPadding*`): 4px left of its text
/// and 16px right, for the arrow (1px and the 15px of a scrollbar's
/// button), and 1px above and below, while `select` has a native look;
/// none without one. In a menu list that centers it (`align-items:
/// center`, the UA's), its block margins are `auto`: it is centered where
/// there is room, at the top where there isn't.
pub(crate) fn adjust_menu_list_inner(style: &mut ComputedStyle, select: &ComputedStyle) {
    if select.effective_appearance != EffectiveAppearance::None {
        let px = |px| LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(px));
        style.margin.left = px(4.0);
        style.margin.right = px(16.0);
        style.padding.top = LengthPercentage::Px(1.0);
        style.padding.bottom = LengthPercentage::Px(1.0);
    }
    if select.align_items == ItemAlignment::Center {
        style.margin.top = LengthPercentageOrAuto::Auto;
        style.margin.bottom = LengthPercentageOrAuto::Auto;
        style.align_self = Some(ItemAlignment::Start);
    }
}

/// What the author's declarations (not the UA's) set: what
/// `IsControlStyled` looks at. Any value counts, the UA's own too.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AuthorStyled {
    pub(crate) background: bool,
    pub(crate) border: bool,
    pub(crate) border_radius: bool,
}

/// The native look `appearance` gives `control` (`None`: not one), with
/// what the author set.
pub(crate) fn effective_appearance(
    control: Option<Control>,
    appearance: Appearance,
    author: AuthorStyled,
) -> EffectiveAppearance {
    let Some(Control { part: auto, .. }) = control else {
        return EffectiveAppearance::None;
    };
    if auto == EffectiveAppearance::None {
        return EffectiveAppearance::None;
    }
    // The compat keywords are `auto` (css-ui 4), but `textfield` on a
    // search field and `menulist-button` on a menu list, which draw it
    // otherwise.
    let part = match appearance {
        Appearance::None => return EffectiveAppearance::None,
        Appearance::Textfield if auto == EffectiveAppearance::SearchField => {
            EffectiveAppearance::TextField
        }
        Appearance::MenulistButton if auto == EffectiveAppearance::Menulist => {
            EffectiveAppearance::MenulistButton
        }
        // A customizable select is drawn by CSS (its base appearance).
        Appearance::BaseSelect
            if matches!(
                auto,
                EffectiveAppearance::Menulist | EffectiveAppearance::Listbox
            ) =>
        {
            return EffectiveAppearance::None;
        }
        _ => auto,
    };
    let styled = matches!(
        part,
        EffectiveAppearance::PushButton
            | EffectiveAppearance::SquareButton
            | EffectiveAppearance::Listbox
            | EffectiveAppearance::Menulist
            | EffectiveAppearance::Meter
            | EffectiveAppearance::ProgressBar
            | EffectiveAppearance::SearchField
            | EffectiveAppearance::TextField
            | EffectiveAppearance::TextArea
    ) && (author.background || author.border || author.border_radius);
    match part {
        // A styled menu list keeps its arrow.
        EffectiveAppearance::Menulist if styled => EffectiveAppearance::MenulistButton,
        _ if styled => EffectiveAppearance::None,
        _ => part,
    }
}

/// What the theme adjusts a style to (Blink's `LayoutTheme::AdjustStyle`):
/// an element with an `appearance` (any element, a control or not; not its
/// effective one) is no table part and not `inline` (`inline`,
/// `inline-table` and the table-internal values are `inline-block`, `table`
/// and `list-item` are `block`), and the native parts' `auto` sizes are
/// theirs: a check box's and a radio button's width and height 13px, a
/// spin button's width 15px (the native part's), a slider thumb's 16px
/// square (`kSliderThumbThickness`), a clear button a square of its font
/// size (Chrome 152's on Windows: `round(0.7 × size + 3)`, from 8 to
/// 24px).
pub(crate) fn adjust_style(style: &mut ComputedStyle) {
    // A menu list's `line-height` is `normal`: it is as high as its font
    // (Blink's `LayoutTheme::AdjustMenuListStyle`).
    if matches!(
        style.effective_appearance,
        EffectiveAppearance::Menulist | EffectiveAppearance::MenulistButton
    ) {
        style.line_height = computed_style::LineHeight::Normal;
    }
    use computed_style::{Display, TableDisplay};
    if style.appearance != Appearance::None {
        style.display = match style.display {
            Display::Table(TableDisplay::Table) | Display::ListItem => Display::Block,
            Display::Inline | Display::InlineTable | Display::Table(_) => Display::InlineBlock,
            display => display,
        };
    }
    const TOGGLE_SIZE: f32 = 13.0;
    const SPIN_BUTTON_WIDTH: f32 = 15.0;
    const SLIDER_THUMB_SIZE: f32 = 16.0;
    let (width, height) = match style.effective_appearance {
        EffectiveAppearance::Checkbox | EffectiveAppearance::Radio => {
            (Some(TOGGLE_SIZE), Some(TOGGLE_SIZE))
        }
        EffectiveAppearance::InnerSpinButton => (Some(SPIN_BUTTON_WIDTH), None),
        EffectiveAppearance::SliderThumbHorizontal => {
            (Some(SLIDER_THUMB_SIZE), Some(SLIDER_THUMB_SIZE))
        }
        EffectiveAppearance::SearchFieldCancelButton => {
            let size = (0.7 * style.font.size + 3.0).round().clamp(8.0, 24.0);
            (Some(size), Some(size))
        }
        _ => (None, None),
    };
    for (size, theirs) in [(&mut style.width, width), (&mut style.height, height)] {
        if let Some(theirs) = theirs
            && *size == Size::Auto
        {
            *size = Size::LengthPercentage(LengthPercentage::Px(theirs));
        }
    }
}
