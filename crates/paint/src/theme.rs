//! Form controls' native looks (Blink's `ThemePainterDefault` over
//! `NativeThemeBase`, the look Chrome draws on Windows and Linux; macOS's
//! is another, not yet): a box whose style has a native look paints it in
//! place of its CSS background and border, recorded as the paint ops it is
//! made of. Its geometry is `NativeThemeBase`'s, its colors those of
//! Chromium's `ColorProvider` (`kColorWebNativeControl*`, light), each for
//! the control's state: disabled, else pressed, else hovered, else normal.

use compositor_paint::{PaintFlags, PaintOp};
use computed_style::EffectiveAppearance;
use gfx::{Color, Path, PointF, RRectF, RectF, SizeF};
use layout::{BoxFragment, ControlState};

/// A part's colors in its four states (Blink's `NativeThemeBase`'s state
/// arrays): normal, hovered, pressed, disabled.
#[derive(Clone, Copy)]
struct Colors {
    normal: Color,
    hovered: Color,
    pressed: Color,
    disabled: Color,
}

impl Colors {
    fn of(self, state: State) -> Color {
        match state {
            State::Normal => self.normal,
            State::Hovered => self.hovered,
            State::Pressed => self.pressed,
            State::Disabled => self.disabled,
        }
    }
}

/// `NativeTheme::State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    Hovered,
    Pressed,
    Disabled,
}

impl State {
    /// Blink's `GetWebThemeState`: a text field that can't be edited is
    /// disabled too.
    fn of(state: ControlState, text: bool) -> Self {
        if state.disabled || (text && state.read_only) {
            Self::Disabled
        } else if state.pressed {
            Self::Pressed
        } else if state.hovered {
            Self::Hovered
        } else {
            Self::Normal
        }
    }
}

const ACCENT: Colors = Colors {
    normal: Color::rgb(0x00, 0x75, 0xFF),
    hovered: Color::rgb(0x00, 0x5C, 0xC8),
    pressed: Color::rgb(0x37, 0x93, 0xFF),
    disabled: Color::rgba(0x76, 0x76, 0x76, 0x4D),
};

const BORDER: Colors = Colors {
    normal: Color::rgb(0x76, 0x76, 0x76),
    hovered: Color::rgb(0x4F, 0x4F, 0x4F),
    pressed: Color::rgb(0x8D, 0x8D, 0x8D),
    disabled: Color::rgba(0x76, 0x76, 0x76, 0x4D),
};

const FILL: Colors = Colors {
    normal: Color::rgb(0xEF, 0xEF, 0xEF),
    hovered: Color::rgb(0xE5, 0xE5, 0xE5),
    pressed: Color::rgb(0xF5, 0xF5, 0xF5),
    disabled: Color::rgba(0xEF, 0xEF, 0xEF, 0x4D),
};

const SLIDER: Colors = Colors {
    normal: Color::rgb(0x00, 0x75, 0xFF),
    hovered: Color::rgb(0x00, 0x5C, 0xC8),
    pressed: Color::rgb(0x37, 0x93, 0xFF),
    disabled: Color::rgb(0xCB, 0xCB, 0xCB),
};

/// A check box's, a radio button's and a text field's background.
const BACKGROUND: Color = Color::WHITE;
const BACKGROUND_DISABLED: Color = Color::rgba(0xFF, 0xFF, 0xFF, 0x99);

/// What a disabled button is lightened with, under its fill.
const LIGHTEN_LAYER: Color = Color::rgba(0xA9, 0xA9, 0xA9, 0x33);

/// The clear button's cross and the spin button's arrows, as Chrome 152
/// draws them on Windows (measured: its icons aren't `NativeThemeBase`'s).
const CANCEL_CROSS: Color = Color::rgb(0x2E, 0x50, 0x89);
const SPIN_ARROW: Color = Color::rgb(0x8B, 0x8B, 0x8B);

const BORDER_WIDTH: f32 = 1.0;
const CORNER_RADIUS: f32 = 2.0;
const SLIDER_TRACK_THICKNESS: f32 = 8.0;
/// A pill's radius: as round as the track is thick.
const SLIDER_TRACK_RADIUS: f32 = 40.0;

/// Whether `appearance` is a part this theme draws (a menu list's, a
/// meter's and a progress bar's are drawn by CSS, for now).
fn draws(appearance: EffectiveAppearance) -> bool {
    use EffectiveAppearance::*;
    matches!(
        appearance,
        Checkbox
            | Radio
            | PushButton
            | SquareButton
            | TextField
            | SearchField
            | TextArea
            | SliderHorizontal
            | SliderThumbHorizontal
            | SearchFieldCancelButton
            | InnerSpinButton
            | Menulist
    )
}

/// The ops of `fragment`'s native look, its border box `rect`, if its
/// style has one this theme draws: in place of its CSS background and
/// border. `scale` is the device pixels per CSS pixel, which a few lines
/// are as thick as whatever it is (Blink's, which `zoom` doesn't scale).
pub(crate) fn ops(fragment: &BoxFragment, rect: RectF, scale: f32) -> Option<Vec<PaintOp>> {
    let appearance = fragment.style.effective_appearance;
    let state = fragment.control_state?;
    if !draws(appearance) {
        return None;
    }
    let mut ops = Vec::new();
    match appearance {
        EffectiveAppearance::Checkbox => checkbox(&mut ops, rect, state),
        EffectiveAppearance::Radio => radio(&mut ops, rect, state),
        EffectiveAppearance::PushButton | EffectiveAppearance::SquareButton => {
            button(&mut ops, rect, state)
        }
        EffectiveAppearance::TextField
        | EffectiveAppearance::SearchField
        | EffectiveAppearance::TextArea => text_field(&mut ops, rect, state),
        EffectiveAppearance::SliderHorizontal => slider_track(&mut ops, fragment, rect, state),
        EffectiveAppearance::SliderThumbHorizontal => slider_thumb(&mut ops, rect, state),
        EffectiveAppearance::SearchFieldCancelButton => cancel_button(&mut ops, rect),
        EffectiveAppearance::InnerSpinButton => spin_button(&mut ops, rect),
        EffectiveAppearance::Menulist => menu_list(&mut ops, fragment, rect, state, scale),
        _ => unreachable!("drawn by CSS"),
    }
    Some(ops)
}

/// The ops of the part of `fragment`'s native look drawn over its CSS
/// background and border, `rect` its border box: a styled menu list's
/// arrow (Blink's `ThemePainter::PaintDecorations`, `menulist-button`).
pub(crate) fn decorations(fragment: &BoxFragment, rect: RectF, scale: f32) -> Option<Vec<PaintOp>> {
    if fragment.style.effective_appearance != EffectiveAppearance::MenulistButton {
        return None;
    }
    let mut ops = Vec::new();
    menu_list_arrow(&mut ops, fragment, rect, scale);
    Some(ops)
}

/// `NativeThemeBase::PaintMenuList` without an author's border radius: a
/// text field's rounded box, filled with a check box's background (not
/// the select's CSS one), and the arrow.
fn menu_list(
    ops: &mut Vec<PaintOp>,
    fragment: &BoxFragment,
    rect: RectF,
    state: ControlState,
    scale: f32,
) {
    let theme = State::of(state, false);
    let background = if theme == State::Disabled {
        BACKGROUND_DISABLED
    } else {
        BACKGROUND
    };
    let shape = rrect(inset(rect, BORDER_WIDTH * 0.5), CORNER_RADIUS);
    fill(ops, shape, background);
    stroke(ops, shape, BORDER.of(theme), BORDER_WIDTH);
    menu_list_arrow(ops, fragment, rect, scale);
}

/// A menu list's arrow (Blink's `ThemePainterDefault::SetupMenuListArrow`
/// and `NativeThemeBase::PaintArrow`): a `∨` 8px wide and 4px high,
/// stroked 2 device pixels thick (`scale` the device pixels per CSS
/// pixel) in the select's `color`, in the middle of the 15px left of its
/// right border, halfway down.
fn menu_list_arrow(ops: &mut Vec<PaintOp>, fragment: &BoxFragment, rect: RectF, scale: f32) {
    const ARROW_BOX: f32 = 15.0;
    const ARROW_WIDTH: f32 = 8.0;
    const ARROW_HEIGHT: f32 = 4.0;
    let right = rect.x + rect.width - fragment.border.right;
    let x = right - (ARROW_BOX + ARROW_WIDTH) / 2.0;
    let top = rect.y + rect.height / 2.0 - ARROW_HEIGHT / 2.0;
    let mut arrow = Path::new();
    arrow.move_to(PointF::new(x, top));
    arrow.line_to(PointF::new(x + ARROW_WIDTH / 2.0, top + ARROW_HEIGHT));
    arrow.line_to(PointF::new(x + ARROW_WIDTH, top));
    ops.push(PaintOp::DrawPath {
        path: arrow,
        flags: PaintFlags::stroke(fragment.style.color, 2.0 / scale),
    });
}

fn inset(rect: RectF, by: f32) -> RectF {
    RectF::new(
        rect.x + by,
        rect.y + by,
        (rect.width - 2.0 * by).max(0.0),
        (rect.height - 2.0 * by).max(0.0),
    )
}

fn rrect(rect: RectF, radius: f32) -> RRectF {
    RRectF::new(rect, [SizeF::new(radius, radius); 4])
}

fn fill(ops: &mut Vec<PaintOp>, rrect: RRectF, color: Color) {
    ops.push(PaintOp::DrawRRect {
        rrect,
        flags: PaintFlags::fill(color),
    });
}

fn stroke(ops: &mut Vec<PaintOp>, rrect: RRectF, color: Color, width: f32) {
    ops.push(PaintOp::DrawRRect {
        rrect,
        flags: PaintFlags::stroke(color, width),
    });
}

/// `NativeThemeBase::PaintCheckbox`: a rounded square, its border; checked
/// or indeterminate, filled with the accent color and marked in the
/// background's.
fn checkbox(ops: &mut Vec<PaintOp>, rect: RectF, state: ControlState) {
    let theme = State::of(state, false);
    let background = if theme == State::Disabled {
        BACKGROUND_DISABLED
    } else {
        BACKGROUND
    };
    if !(state.checked || state.indeterminate) {
        fill(
            ops,
            rrect(inset(rect, BORDER_WIDTH * 0.2), CORNER_RADIUS),
            background,
        );
        stroke(
            ops,
            rrect(inset(rect, BORDER_WIDTH * 0.5), CORNER_RADIUS),
            BORDER.of(theme),
            BORDER_WIDTH,
        );
        return;
    }
    fill(ops, rrect(rect, CORNER_RADIUS), ACCENT.of(theme));
    let (w, h) = (rect.width, rect.height);
    if state.indeterminate {
        // An 8×2 dash in the middle of 13×13.
        let dash = RectF::new(
            rect.x + w * 2.5 / 13.0,
            rect.y + h * 5.5 / 13.0,
            w * 8.0 / 13.0,
            h * 2.0 / 13.0,
        );
        ops.push(PaintOp::DrawRect {
            rect: dash,
            flags: PaintFlags::fill(background),
        });
        return;
    }
    let mut check = Path::new();
    check.move_to(PointF::new(rect.x + 0.2 * w, rect.y + 0.5 * h));
    check.line_to(PointF::new(rect.x + 0.4 * w, rect.y + 0.7 * h));
    check.line_to(PointF::new(rect.x + 0.8 * w, rect.y + 0.3 * h));
    ops.push(PaintOp::DrawPath {
        path: check,
        flags: PaintFlags::stroke(background, 0.16 * h),
    });
}

/// `NativeThemeBase::PaintRadio`: a circle, its border (the accent's when
/// checked) and, checked, an accent dot 60% across.
fn radio(ops: &mut Vec<PaintOp>, rect: RectF, state: ControlState) {
    let theme = State::of(state, false);
    let background = if theme == State::Disabled {
        BACKGROUND_DISABLED
    } else {
        BACKGROUND
    };
    let round = |rect: RectF| rrect(rect, rect.width.max(rect.height) * 0.5);
    fill(ops, round(inset(rect, BORDER_WIDTH * 0.2)), background);
    let border = if state.checked {
        ACCENT.of(theme)
    } else {
        BORDER.of(theme)
    };
    stroke(
        ops,
        round(inset(rect, BORDER_WIDTH * 0.5)),
        border,
        BORDER_WIDTH,
    );
    if state.checked {
        let dot = RectF::new(
            rect.x + 0.2 * rect.width,
            rect.y + 0.2 * rect.height,
            0.6 * rect.width,
            0.6 * rect.height,
        );
        fill(ops, round(dot), ACCENT.of(theme));
    }
}

/// `NativeThemeBase::PaintButton`: a rounded rect, filled and bordered; a
/// disabled one lightened first.
fn button(ops: &mut Vec<PaintOp>, rect: RectF, state: ControlState) {
    let theme = State::of(state, false);
    let shape = rrect(inset(rect, BORDER_WIDTH * 0.5), CORNER_RADIUS);
    if theme == State::Disabled {
        fill(ops, shape, LIGHTEN_LAYER);
    }
    fill(ops, shape, FILL.of(theme));
    stroke(ops, shape, BORDER.of(theme), BORDER_WIDTH);
}

/// `NativeThemeBase::PaintTextField`: the background, and a rounded
/// border.
fn text_field(ops: &mut Vec<PaintOp>, rect: RectF, state: ControlState) {
    let theme = State::of(state, true);
    let shape = rrect(inset(rect, BORDER_WIDTH * 0.5), CORNER_RADIUS);
    fill(ops, shape, BACKGROUND);
    stroke(ops, shape, BORDER.of(theme), BORDER_WIDTH);
}

/// `NativeThemeBase::PaintSliderTrack`: an 8px pill across the range's
/// middle, its value's part (from its start to its thumb's middle) in the
/// slider's color, under the border, which is half transparent but when
/// disabled.
fn slider_track(ops: &mut Vec<PaintOp>, fragment: &BoxFragment, rect: RectF, state: ControlState) {
    let theme = State::of(state, false);
    let content = RectF::new(
        rect.x + fragment.border.left + fragment.padding.left,
        rect.y + fragment.border.top + fragment.padding.top,
        rect.width
            - fragment.border.left
            - fragment.border.right
            - fragment.padding.left
            - fragment.padding.right,
        rect.height
            - fragment.border.top
            - fragment.border.bottom
            - fragment.padding.top
            - fragment.padding.bottom,
    );
    let track = RectF::new(
        content.x,
        content.y + (content.height - SLIDER_TRACK_THICKNESS) / 2.0,
        content.width,
        SLIDER_TRACK_THICKNESS,
    );
    let radius = SLIDER_TRACK_RADIUS.min(track.height / 2.0);
    fill(ops, rrect(track, radius), FILL.of(theme));
    if let Some(middle) = thumb_middle(fragment) {
        let value = RectF::new(
            track.x,
            track.y,
            (rect.x + middle - track.x).max(0.0),
            track.height,
        );
        fill(ops, rrect(value, radius), SLIDER.of(theme));
    }
    let mut border = BORDER.of(theme);
    if theme != State::Disabled {
        border.a = 0x80;
    }
    stroke(
        ops,
        rrect(inset(track, BORDER_WIDTH * 0.5), radius),
        border,
        BORDER_WIDTH,
    );
}

/// The middle of a range's thumb across it, from its border box's start:
/// the thumb is its UA shadow tree's container's track's child.
fn thumb_middle(fragment: &BoxFragment) -> Option<f32> {
    let mut at = 0.0;
    let mut box_ = fragment;
    for _ in 0..3 {
        let child = box_.children.first()?;
        at += child.offset.x;
        box_ = &child.fragment;
    }
    Some(at + box_.size.width / 2.0)
}

/// `NativeThemeBase::PaintSliderThumb`: a circle in the slider's color.
fn slider_thumb(ops: &mut Vec<PaintOp>, rect: RectF, state: ControlState) {
    let shape = inset(rect, 0.5);
    fill(
        ops,
        rrect(shape, shape.width.max(shape.height) * 0.5),
        SLIDER.of(State::of(state, false)),
    );
}

/// The clear button's cross: two strokes from corner to corner of its
/// middle, 7px across and 2px wide at 12px (Chrome 152's icon,
/// measured).
fn cancel_button(ops: &mut Vec<PaintOp>, rect: RectF) {
    let size = rect.width.min(rect.height);
    let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let arm = size * 0.23;
    let mut cross = Path::new();
    cross.move_to(PointF::new(cx - arm, cy - arm));
    cross.line_to(PointF::new(cx + arm, cy + arm));
    cross.move_to(PointF::new(cx + arm, cy - arm));
    cross.line_to(PointF::new(cx - arm, cy + arm));
    ops.push(PaintOp::DrawPath {
        path: cross,
        flags: PaintFlags::stroke(CANCEL_CROSS, size / 6.0),
    });
}

/// The spin button's arrows (`NativeThemeBase::PathForArrow`, in the
/// halves of its box): up at its top, down at its bottom, 8px wide and
/// 5.5px tall at 15×16 (Chrome 152's, measured).
fn spin_button(ops: &mut Vec<PaintOp>, rect: RectF) {
    let cx = rect.x + rect.width / 2.0;
    let (half, tall) = (rect.width * 4.0 / 15.0, rect.height * 5.5 / 16.0);
    let mut arrows = Path::new();
    arrows.move_to(PointF::new(cx, rect.y + 0.3));
    arrows.line_to(PointF::new(cx + half, rect.y + 0.3 + tall));
    arrows.line_to(PointF::new(cx - half, rect.y + 0.3 + tall));
    arrows.close();
    let bottom = rect.y + rect.height - 0.3;
    arrows.move_to(PointF::new(cx, bottom));
    arrows.line_to(PointF::new(cx + half, bottom - tall));
    arrows.line_to(PointF::new(cx - half, bottom - tall));
    arrows.close();
    ops.push(PaintOp::DrawPath {
        path: arrows,
        flags: PaintFlags::fill(SPIN_ARROW),
    });
}
