//! The toolbar (`ToolbarView` with its `LocationBarView`): back, forward
//! and reload, then the address field. Drawn by the browser itself, as a
//! display list for the browser compositor's root layer.
//!
//! Transitional: after the MVP the UI is to become customizable with HTML
//! and CSS, a page in a renderer of its own (like WebUI) embedded with
//! another `SurfaceLayer`. That needs script, events and `<input>` editing,
//! which come later; until then this native toolbar stands in, kept to this
//! file and `omnibox` so it can go in one piece.

use std::f32::consts::PI;

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF};

use crate::omnibox::Omnibox;

/// CSS px, above the page.
pub const HEIGHT: f32 = 40.0;
const PADDING: f32 = 6.0;
const BUTTON: f32 = 28.0;
const GAP: f32 = 4.0;
const FIELD_HEIGHT: f32 = 30.0;
const FIELD_PADDING: f32 = 12.0;

/// The active tab is this color too, joined to the toolbar.
pub(crate) const BACKGROUND: Color = Color::rgb(241, 243, 244);
const SEPARATOR: Color = Color::rgb(218, 220, 224);
const ICON: Color = Color::rgb(95, 99, 104);
const ICON_DISABLED: Color = Color::rgb(189, 193, 198);
const HOVER: Color = Color::rgb(222, 225, 229);
const FIELD: Color = Color::WHITE;
const FOCUS_RING: Color = Color::rgb(26, 115, 232);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Back,
    Forward,
    Reload,
}

const BUTTONS: [Button; 3] = [Button::Back, Button::Forward, Button::Reload];

/// What is under a point in the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Button(Button),
    Field,
    /// The toolbar's background.
    Nothing,
}

pub struct Toolbar {
    width: f32,
    pub omnibox: Omnibox,
    can_go_back: bool,
    can_go_forward: bool,
    hovered: Option<Button>,
    /// The button the primary button went down on: going up on the same
    /// one presses it.
    pressed: Option<Button>,
}

impl Toolbar {
    pub fn new(test_fonts: bool) -> Self {
        Self {
            width: 0.0,
            omnibox: Omnibox::new(test_fonts),
            can_go_back: false,
            can_go_forward: false,
            hovered: None,
            pressed: None,
        }
    }

    /// The toolbar's width, CSS px: the window's, laid out by default.
    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    pub fn set_history(&mut self, can_go_back: bool, can_go_forward: bool) {
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
    }

    /// Whether back and forward are enabled.
    pub fn history(&self) -> (bool, bool) {
        (self.can_go_back, self.can_go_forward)
    }

    fn enabled(&self, button: Button) -> bool {
        match button {
            Button::Back => self.can_go_back,
            Button::Forward => self.can_go_forward,
            Button::Reload => true,
        }
    }

    fn button_rect(button: Button) -> RectF {
        let index = BUTTONS.iter().position(|b| *b == button).unwrap() as f32;
        RectF::new(
            PADDING + index * (BUTTON + GAP),
            (HEIGHT - BUTTON) / 2.0,
            BUTTON,
            BUTTON,
        )
    }

    /// The address field's box.
    pub fn field_rect(&self) -> RectF {
        let x = PADDING + BUTTONS.len() as f32 * (BUTTON + GAP) + GAP;
        RectF::new(
            x,
            (HEIGHT - FIELD_HEIGHT) / 2.0,
            (self.width - x - PADDING).max(0.0),
            FIELD_HEIGHT,
        )
    }

    /// Where the field's text goes: the field without its padding.
    pub fn text_rect(&self) -> RectF {
        let field = self.field_rect();
        RectF::new(
            field.x + FIELD_PADDING,
            field.y,
            (field.width - 2.0 * FIELD_PADDING).max(0.0),
            field.height,
        )
    }

    pub fn part_at(&self, point: PointF) -> Part {
        if let Some(&button) = BUTTONS
            .iter()
            .find(|b| Self::button_rect(**b).contains(point))
        {
            return Part::Button(button);
        }
        if self.field_rect().contains(point) {
            return Part::Field;
        }
        Part::Nothing
    }

    /// The pointer moved to `point`. Whether anything shows differently.
    pub fn mouse_move(&mut self, point: PointF) -> bool {
        let hovered = match self.part_at(point) {
            Part::Button(button) if self.enabled(button) => Some(button),
            _ => None,
        };
        self.omnibox.mouse_move(self.in_text(point));
        let changed = hovered != self.hovered;
        self.hovered = hovered;
        changed || self.omnibox.is_focused()
    }

    /// The pointer left the toolbar.
    pub fn mouse_leave(&mut self) -> bool {
        self.hovered.take().is_some()
    }

    pub fn mouse_down(&mut self, point: PointF, shift: bool) {
        match self.part_at(point) {
            Part::Button(button) if self.enabled(button) => self.pressed = Some(button),
            Part::Field => self.omnibox.mouse_down(self.in_text(point), shift),
            _ => {}
        }
    }

    /// The primary button came up at `point`: the button pressed, if it
    /// went down on it too.
    pub fn mouse_up(&mut self, point: PointF) -> Option<Button> {
        self.omnibox.mouse_up();
        let pressed = self.pressed.take()?;
        (self.part_at(point) == Part::Button(pressed) && self.enabled(pressed)).then_some(pressed)
    }

    fn in_text(&self, point: PointF) -> PointF {
        let text = self.text_rect();
        PointF::new(point.x - text.x, point.y - text.y)
    }

    /// The IME's candidate window area, in the toolbar's coordinates.
    pub fn ime_cursor_area(&mut self) -> RectF {
        let text = self.text_rect();
        let area = self.omnibox.ime_cursor_area();
        let top = text.y + (text.height - area.height) / 2.0;
        RectF::new(text.x + area.x, top, area.width, area.height)
    }

    /// The toolbar's display list, `HEIGHT` high and as wide as it is set.
    pub fn paint(&mut self) -> DisplayItemList {
        let mut list = DisplayItemList::record();
        let fill = |list: &mut DisplayItemList, rect: RectF, color| {
            list.push(
                PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(color),
                },
                Some(rect),
            )
        };
        fill(
            &mut list,
            RectF::new(0.0, 0.0, self.width, HEIGHT),
            BACKGROUND,
        );
        fill(
            &mut list,
            RectF::new(0.0, HEIGHT - 1.0, self.width, 1.0),
            SEPARATOR,
        );
        for button in BUTTONS {
            let rect = Self::button_rect(button);
            if self.hovered == Some(button) {
                list.push(
                    PaintOp::DrawRRect {
                        rrect: RRectF::uniform(rect, BUTTON / 2.0),
                        flags: PaintFlags::fill(HOVER),
                    },
                    Some(rect),
                );
            }
            let color = if self.enabled(button) {
                ICON
            } else {
                ICON_DISABLED
            };
            let center = PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            list.push(
                PaintOp::DrawPath {
                    path: gfx::Path::polygons(icon(button, center)),
                    flags: PaintFlags::fill(color),
                },
                Some(rect),
            );
        }
        let field = self.field_rect();
        let focused = self.omnibox.is_focused();
        if focused {
            let ring = RectF::new(
                field.x - 1.0,
                field.y - 1.0,
                field.width + 2.0,
                field.height + 2.0,
            );
            list.push(
                PaintOp::DrawRRect {
                    rrect: RRectF::uniform(ring, ring.height / 2.0),
                    flags: PaintFlags::fill(FOCUS_RING),
                },
                Some(ring),
            );
        }
        list.push(
            PaintOp::DrawRRect {
                rrect: RRectF::uniform(field, field.height / 2.0),
                flags: PaintFlags::fill(if focused { FIELD } else { HOVER }),
            },
            Some(field),
        );
        let text = self.text_rect();
        self.omnibox.paint(&mut list, text);
        list.finalize();
        list
    }
}

/// A button's icon around `c`, as filled polygons: arrows for back and
/// forward, a circular arrow for reload.
fn icon(button: Button, c: PointF) -> Vec<Vec<PointF>> {
    let p = |x: f32, y: f32| PointF::new(c.x + x, c.y + y);
    let arrow = |dir: f32| {
        vec![
            // The head, then the shaft.
            vec![p(-7.0 * dir, 0.0), p(-dir, -6.0), p(-dir, 6.0)],
            vec![
                p(-2.0 * dir, -1.0),
                p(7.0 * dir, -1.0),
                p(7.0 * dir, 1.0),
                p(-2.0 * dir, 1.0),
            ],
        ]
    };
    match button {
        Button::Back => arrow(1.0),
        Button::Forward => arrow(-1.0),
        Button::Reload => {
            // Three quarters of a ring from the top right, clockwise, with
            // a head at its start.
            let (outer, inner) = (7.0, 5.0);
            let steps = 24;
            let start = -PI / 4.0;
            let sweep = 1.5 * PI;
            let at = |r: f32, i: usize| {
                let a = start + sweep * i as f32 / steps as f32;
                p(r * a.cos(), r * a.sin())
            };
            let mut ring: Vec<PointF> = (0..=steps).map(|i| at(outer, i)).collect();
            ring.extend((0..=steps).rev().map(|i| at(inner, i)));
            // The head, a triangle on the ring's start pointing along it.
            let tip = at(6.0, 0);
            let q = |dx: f32, dy: f32| PointF::new(tip.x + dx, tip.y + dy);
            let head = vec![q(3.5, -4.0), q(4.0, 3.0), q(-3.5, 0.5)];
            vec![ring, head]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolbar() -> Toolbar {
        let mut toolbar = Toolbar::new(true);
        toolbar.set_width(800.0);
        toolbar
    }

    fn center(rect: RectF) -> PointF {
        PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn parts_and_their_places() {
        let toolbar = toolbar();
        for button in BUTTONS {
            let rect = Toolbar::button_rect(button);
            assert_eq!(toolbar.part_at(center(rect)), Part::Button(button));
        }
        // The field takes the rest of the width.
        let field = toolbar.field_rect();
        assert!(field.x > Toolbar::button_rect(Button::Reload).right());
        assert_eq!(field.right(), 800.0 - PADDING);
        assert_eq!(toolbar.part_at(center(field)), Part::Field);
        assert_eq!(toolbar.part_at(PointF::new(1.0, 1.0)), Part::Nothing);
    }

    #[test]
    fn a_button_is_pressed_by_down_and_up_on_it_while_enabled() {
        let mut toolbar = toolbar();
        let back = center(Toolbar::button_rect(Button::Back));
        let reload = center(Toolbar::button_rect(Button::Reload));
        // Back is disabled with nothing to go back to.
        toolbar.mouse_down(back, false);
        assert_eq!(toolbar.mouse_up(back), None);
        toolbar.set_history(true, false);
        toolbar.mouse_down(back, false);
        assert_eq!(toolbar.mouse_up(back), Some(Button::Back));
        // Down on one, up on another: nothing.
        toolbar.mouse_down(back, false);
        assert_eq!(toolbar.mouse_up(reload), None);
        // Hovering an enabled button highlights it; a disabled one doesn't.
        assert!(toolbar.mouse_move(reload));
        assert_eq!(toolbar.hovered, Some(Button::Reload));
        toolbar.mouse_move(center(Toolbar::button_rect(Button::Forward)));
        assert_eq!(toolbar.hovered, None);
    }

    #[test]
    fn a_click_in_the_field_focuses_it() {
        let mut toolbar = toolbar();
        toolbar.omnibox.set_url("https://a.test/");
        toolbar.mouse_down(center(toolbar.field_rect()), false);
        toolbar.mouse_up(center(toolbar.field_rect()));
        assert!(toolbar.omnibox.is_focused());
        assert_eq!(toolbar.omnibox.selected_text(), Some("https://a.test/"));
        // The focus ring paints under the field.
        let list = toolbar.paint();
        assert!(list.ops().iter().any(|op| matches!(
            op,
            PaintOp::DrawRRect { flags, .. } if flags.color == FOCUS_RING
        )));
    }
}
