//! The tab strip (Chromium's `TabStrip`, with its `Tab`s and
//! `NewTabButton`): a row above the toolbar, each tab's title and close
//! button, and a button for a new tab. Drawn by the browser like the
//! toolbar, into the same layer. It knows only the titles and which tab is
//! active; what a click does goes back to the view, whose `TabStripModel`
//! has the tabs.
//!
//! Transitional, with the rest of the native UI (see `toolbar`).

use base::numerics::saturated_cast;
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF};
use parley::{FontContext, LayoutContext};
use protocol::input::MouseButton;

use crate::toolbar;
use crate::ui_text;

/// CSS px, above the toolbar.
pub const HEIGHT: f32 = 34.0;
/// Where the first tab starts.
const PADDING: f32 = 8.0;
/// The tabs' top: the strip's background shows above them.
const TAB_TOP: f32 = 6.0;
const TAB_MAX: f32 = 240.0;
const TAB_MIN: f32 = 56.0;
/// Narrower tabs have a close button only when active.
const CLOSE_MIN: f32 = 100.0;
const TAB_RADIUS: f32 = 8.0;
const TITLE_PADDING: f32 = 12.0;
const FONT_SIZE: f32 = 12.0;
const CLOSE: f32 = 16.0;
const CLOSE_MARGIN: f32 = 8.0;
const NEW_TAB: f32 = 28.0;
const GAP: f32 = 4.0;
/// How far the end of a title too long fades out, and in how many steps.
const FADE: f32 = 24.0;
const FADE_STEPS: u8 = 8;

const FRAME: Color = Color::rgb(222, 225, 230);
const ACTIVE: Color = toolbar::BACKGROUND;
const HOVER: Color = Color::rgb(232, 234, 237);
const BUTTON_HOVER: Color = Color::rgb(204, 207, 212);
const TITLE: Color = Color::rgb(32, 33, 36);
const ICON: Color = Color::rgb(95, 99, 104);
const SEPARATOR: Color = Color::rgb(160, 164, 170);

/// What is under a point in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Tab(usize),
    /// A tab's close button.
    Close(usize),
    NewTab,
    Nothing,
}

/// What a click in the strip asks of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Activate(usize),
    Close(usize),
    NewTab,
}

pub struct TabStrip {
    width: f32,
    titles: Vec<String>,
    active: usize,
    hovered: Option<Part>,
    /// What a button went down on, and which: the up on the same finishes
    /// the click.
    pressed: Option<(Part, MouseButton)>,
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl TabStrip {
    /// `test_fonts` as for the toolbar.
    pub fn new(test_fonts: bool) -> Self {
        Self {
            width: 0.0,
            titles: Vec::new(),
            active: 0,
            hovered: None,
            pressed: None,
            fonts: ui_text::font_context(test_fonts),
            layouts: LayoutContext::new(),
        }
    }

    /// The strip's width, CSS px.
    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    /// The tabs' titles, in order, and the active one's index. Whether the
    /// strip shows anything differently.
    pub fn set_tabs(&mut self, titles: Vec<String>, active: usize) -> bool {
        if titles == self.titles && active == self.active {
            return false;
        }
        if titles.len() != self.titles.len() {
            // What was under the pointer may have moved.
            self.hovered = None;
        }
        self.titles = titles;
        self.active = active;
        true
    }

    /// The tabs' titles, in order.
    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    /// Where the tabs end: the new tab button stays right of them.
    fn tabs_right(&self) -> f32 {
        (self.width - NEW_TAB - 2.0 * GAP).max(PADDING)
    }

    /// Each tab's width: as wide as there is room for, between the least
    /// and the most.
    pub fn tab_width(&self) -> f32 {
        let count = self.titles.len().max(1) as f32;
        ((self.tabs_right() - PADDING) / count).clamp(TAB_MIN, TAB_MAX)
    }

    pub fn tab_rect(&self, index: usize) -> RectF {
        let width = self.tab_width();
        RectF::new(
            PADDING + index as f32 * width,
            TAB_TOP,
            width,
            HEIGHT - TAB_TOP,
        )
    }

    fn has_close(&self, index: usize) -> bool {
        self.tab_width() >= CLOSE_MIN || index == self.active
    }

    pub fn close_rect(&self, index: usize) -> RectF {
        let tab = self.tab_rect(index);
        RectF::new(
            tab.right() - CLOSE_MARGIN - CLOSE,
            tab.y + (tab.height - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        )
    }

    /// Right after the last tab, or at the strip's end once the tabs
    /// overflow it.
    pub fn new_tab_rect(&self) -> RectF {
        let after = PADDING + self.titles.len() as f32 * self.tab_width();
        RectF::new(
            after.min(self.tabs_right()) + GAP,
            TAB_TOP + (HEIGHT - TAB_TOP - NEW_TAB) / 2.0,
            NEW_TAB,
            NEW_TAB,
        )
    }

    /// Where a tab's title goes: up to its close button, if it has one.
    fn title_rect(&self, index: usize) -> RectF {
        let tab = self.tab_rect(index);
        let right = if self.has_close(index) {
            self.close_rect(index).x - GAP
        } else {
            tab.right() - TITLE_PADDING
        };
        let x = tab.x + TITLE_PADDING;
        RectF::new(x, tab.y, (right - x).max(0.0), tab.height)
    }

    pub fn part_at(&self, point: PointF) -> Part {
        if self.new_tab_rect().contains(point) {
            return Part::NewTab;
        }
        if point.y < TAB_TOP || point.x < PADDING || point.x >= self.tabs_right() {
            return Part::Nothing;
        }
        let index: usize = saturated_cast((point.x - PADDING) / self.tab_width());
        if index >= self.titles.len() || point.y >= HEIGHT {
            return Part::Nothing;
        }
        if self.has_close(index) && self.close_rect(index).inset(-2.0).contains(point) {
            Part::Close(index)
        } else {
            Part::Tab(index)
        }
    }

    /// The pointer moved to `point`. Whether anything shows differently.
    pub fn mouse_move(&mut self, point: PointF) -> bool {
        let hovered = match self.part_at(point) {
            Part::Nothing => None,
            part => Some(part),
        };
        std::mem::replace(&mut self.hovered, hovered) != hovered
    }

    /// The pointer left the strip.
    pub fn mouse_leave(&mut self) -> bool {
        self.hovered.take().is_some()
    }

    /// A button went down at `point`. The primary one activates a tab at
    /// once, as in Chrome; buttons wait for the up.
    pub fn mouse_down(&mut self, point: PointF, button: MouseButton) -> Option<Action> {
        let part = self.part_at(point);
        match (button, part) {
            (MouseButton::Left, Part::Tab(index)) => Some(Action::Activate(index)),
            (MouseButton::Left, Part::Close(_) | Part::NewTab)
            | (MouseButton::Middle, Part::Tab(_) | Part::Close(_)) => {
                self.pressed = Some((part, button));
                None
            }
            _ => None,
        }
    }

    /// A button went up at `point`: a click, if it went down on the same
    /// thing. The middle button closes the tab it clicked.
    pub fn mouse_up(&mut self, point: PointF, button: MouseButton) -> Option<Action> {
        let (pressed, with) = self.pressed.take()?;
        let part = self.part_at(point);
        if with != button {
            return None;
        }
        match (button, pressed, part) {
            (MouseButton::Left, Part::Close(index), Part::Close(at)) if index == at => {
                Some(Action::Close(index))
            }
            (MouseButton::Left, Part::NewTab, Part::NewTab) => Some(Action::NewTab),
            (
                MouseButton::Middle,
                Part::Tab(index) | Part::Close(index),
                Part::Tab(at) | Part::Close(at),
            ) if index == at => Some(Action::Close(index)),
            _ => None,
        }
    }

    fn is_hovered(&self, index: usize) -> bool {
        matches!(self.hovered, Some(Part::Tab(i) | Part::Close(i)) if i == index)
    }

    /// The strip's display list, `HEIGHT` high and as wide as it is set.
    pub fn paint(&mut self) -> DisplayItemList {
        let mut list = DisplayItemList::record();
        fill(&mut list, RectF::new(0.0, 0.0, self.width, HEIGHT), FRAME);
        let clip = RectF::new(0.0, 0.0, self.tabs_right(), HEIGHT);
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::ClipRect {
            rect: clip,
            anti_alias: false,
        });
        let count = self.titles.len();
        let active = self.active;
        for index in (0..count).filter(|i| *i != active) {
            let tab = self.tab_rect(index);
            if tab.x >= clip.right() {
                break;
            }
            let background = if self.is_hovered(index) {
                let shape = tab.inset(2.0);
                list.push_op(PaintOp::DrawRRect {
                    rrect: RRectF::uniform(
                        RectF::new(shape.x, tab.y + 2.0, shape.width, tab.height - 4.0),
                        TAB_RADIUS,
                    ),
                    flags: PaintFlags::fill(HOVER),
                });
                HOVER
            } else {
                FRAME
            };
            self.paint_contents(&mut list, index, background);
            // Between two tabs neither of them active nor hovered.
            #[expect(clippy::arithmetic_side_effects, reason = "index < count")]
            let next = index + 1;
            if next < count
                && next != self.active
                && !self.is_hovered(index)
                && !self.is_hovered(next)
            {
                fill(
                    &mut list,
                    RectF::new(tab.right() - 0.5, tab.y + 8.0, 1.0, tab.height - 16.0),
                    SEPARATOR,
                );
            }
        }
        if self.active < count {
            // Rounded on top, square where it meets the toolbar.
            let tab = self.tab_rect(self.active);
            list.push_op(PaintOp::DrawRRect {
                rrect: RRectF::uniform(tab, TAB_RADIUS),
                flags: PaintFlags::fill(ACTIVE),
            });
            fill(
                &mut list,
                RectF::new(
                    tab.x,
                    tab.y + TAB_RADIUS,
                    tab.width,
                    tab.height - TAB_RADIUS,
                ),
                ACTIVE,
            );
            self.paint_contents(&mut list, self.active, ACTIVE);
        }
        list.push_op(PaintOp::Restore);
        let button = self.new_tab_rect();
        if self.hovered == Some(Part::NewTab) {
            list.push_op(PaintOp::DrawRRect {
                rrect: RRectF::uniform(button, NEW_TAB / 2.0),
                flags: PaintFlags::fill(BUTTON_HOVER),
            });
        }
        let c = center(button);
        fill(&mut list, RectF::new(c.x - 6.0, c.y - 1.0, 12.0, 2.0), ICON);
        fill(&mut list, RectF::new(c.x - 1.0, c.y - 6.0, 2.0, 12.0), ICON);
        list.finalize();
        list
    }

    /// A tab's title, fading out at its end if too long, and its close
    /// button, on `background`.
    fn paint_contents(&mut self, list: &mut DisplayItemList, index: usize, background: Color) {
        let area = self.title_rect(index);
        let layout = ui_text::label(
            &mut self.fonts,
            &mut self.layouts,
            &self.titles[index],
            FONT_SIZE,
        );
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::ClipRect {
            rect: area,
            anti_alias: false,
        });
        let top = area.y + ((area.height - layout.height()) / 2.0).round();
        ui_text::paint_glyphs(list, &layout, PointF::new(area.x, top), TITLE);
        if layout.width() > area.width {
            // Chrome fades with a gradient; steps of the background, each
            // more opaque, look the same.
            let step = FADE / f32::from(FADE_STEPS);
            for i in 0..FADE_STEPS {
                #[expect(
                    clippy::arithmetic_side_effects,
                    clippy::cast_possible_truncation,
                    reason = "i < FADE_STEPS: (i + 1) * 255 / FADE_STEPS is 255 at most"
                )]
                let alpha = (u16::from(i + 1) * 255 / u16::from(FADE_STEPS)) as u8;
                let color = Color::rgba(background.r, background.g, background.b, alpha);
                let x = area.right() - FADE + f32::from(i) * step;
                fill(list, RectF::new(x, area.y, step, area.height), color);
            }
        }
        list.push_op(PaintOp::Restore);
        if self.has_close(index) {
            let button = self.close_rect(index);
            if self.hovered == Some(Part::Close(index)) {
                list.push_op(PaintOp::DrawRRect {
                    rrect: RRectF::uniform(button, CLOSE / 2.0),
                    flags: PaintFlags::fill(BUTTON_HOVER),
                });
            }
            let c = center(button);
            let d = 4.0;
            for (from, to) in [((-d, -d), (d, d)), ((-d, d), (d, -d))] {
                list.push_op(PaintOp::DrawLine {
                    p0: PointF::new(c.x + from.0, c.y + from.1),
                    p1: PointF::new(c.x + to.0, c.y + to.1),
                    flags: PaintFlags::stroke(ICON, 1.5),
                });
            }
        }
    }
}

fn fill(list: &mut DisplayItemList, rect: RectF, color: Color) {
    list.push_op(PaintOp::DrawRect {
        rect,
        flags: PaintFlags::fill(color),
    });
}

fn center(rect: RectF) -> PointF {
    PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(width: f32, tabs: usize, active: usize) -> TabStrip {
        let mut strip = TabStrip::new(true);
        strip.set_width(width);
        strip.set_tabs((0..tabs).map(|i| format!("tab {i}")).collect(), active);
        strip
    }

    #[test]
    fn tabs_are_as_wide_as_there_is_room_for() {
        assert_eq!(strip(800.0, 1, 0).tab_width(), TAB_MAX);
        let few = strip(800.0, 10, 0);
        let room = 800.0 - NEW_TAB - 2.0 * GAP - PADDING;
        assert_eq!(few.tab_width(), room / 10.0);
        // Too many: the narrowest, and the last ones go past the end.
        let many = strip(800.0, 30, 0);
        assert_eq!(many.tab_width(), TAB_MIN);
        assert!(many.tab_rect(29).x > many.tabs_right());
        // The new tab button follows the last tab, until there is no room.
        let one = strip(800.0, 1, 0);
        assert_eq!(one.new_tab_rect().x, PADDING + TAB_MAX + GAP);
        assert_eq!(many.new_tab_rect().x, many.tabs_right() + GAP);
        assert!(many.new_tab_rect().right() <= 800.0);
    }

    #[test]
    fn narrow_tabs_have_a_close_button_only_when_active() {
        let wide = strip(800.0, 3, 1);
        assert!((0..3).all(|i| wide.has_close(i)));
        let narrow = strip(800.0, 12, 4);
        assert!(narrow.tab_width() < CLOSE_MIN);
        assert!(narrow.has_close(4));
        assert!(!narrow.has_close(3));
        // Its title then goes to the tab's end.
        assert!(narrow.title_rect(3).right() > narrow.close_rect(3).x);
    }

    #[test]
    fn parts_and_their_places() {
        let strip = strip(800.0, 3, 0);
        assert_eq!(strip.part_at(center(strip.tab_rect(1))), Part::Tab(1));
        assert_eq!(strip.part_at(center(strip.close_rect(2))), Part::Close(2));
        assert_eq!(strip.part_at(center(strip.new_tab_rect())), Part::NewTab);
        assert_eq!(strip.part_at(PointF::new(4.0, 20.0)), Part::Nothing);
        assert_eq!(
            strip.part_at(PointF::new(100.0, 2.0)),
            Part::Nothing,
            "above"
        );
        let after = strip.new_tab_rect().right() + 10.0;
        assert_eq!(strip.part_at(PointF::new(after, 20.0)), Part::Nothing);
        // Past the end, the tabs that overflow can't be clicked.
        let many = self::strip(400.0, 30, 0);
        let end = PointF::new(many.tabs_right() + 1.0, 20.0);
        assert!(!matches!(many.part_at(end), Part::Tab(_) | Part::Close(_)));
    }

    #[test]
    fn clicks_activate_close_and_open() {
        let mut strip = strip(800.0, 3, 0);
        let tab = center(strip.tab_rect(1));
        let close = center(strip.close_rect(2));
        let new_tab = center(strip.new_tab_rect());
        // A tab at once, on the down.
        assert_eq!(
            strip.mouse_down(tab, MouseButton::Left),
            Some(Action::Activate(1))
        );
        assert_eq!(strip.mouse_up(tab, MouseButton::Left), None);
        // Buttons on the up on them.
        assert_eq!(strip.mouse_down(close, MouseButton::Left), None);
        assert_eq!(
            strip.mouse_up(close, MouseButton::Left),
            Some(Action::Close(2))
        );
        strip.mouse_down(close, MouseButton::Left);
        assert_eq!(
            strip.mouse_up(new_tab, MouseButton::Left),
            None,
            "moved off"
        );
        strip.mouse_down(center(strip.close_rect(1)), MouseButton::Left);
        assert_eq!(strip.mouse_up(close, MouseButton::Left), None, "another's");
        strip.mouse_down(close, MouseButton::Left);
        assert_eq!(
            strip.mouse_up(close, MouseButton::Middle),
            None,
            "another button"
        );
        strip.mouse_down(new_tab, MouseButton::Left);
        assert_eq!(
            strip.mouse_up(new_tab, MouseButton::Left),
            Some(Action::NewTab)
        );
        // The middle button closes the tab it clicks, its close button too.
        assert_eq!(strip.mouse_down(tab, MouseButton::Middle), None);
        assert_eq!(
            strip.mouse_up(tab, MouseButton::Middle),
            Some(Action::Close(1))
        );
        strip.mouse_down(close, MouseButton::Middle);
        assert_eq!(
            strip.mouse_up(center(strip.tab_rect(2)), MouseButton::Middle),
            Some(Action::Close(2))
        );
        strip.mouse_down(tab, MouseButton::Middle);
        assert_eq!(strip.mouse_up(new_tab, MouseButton::Middle), None);
        strip.mouse_down(tab, MouseButton::Middle);
        assert_eq!(
            strip.mouse_up(center(strip.tab_rect(2)), MouseButton::Middle),
            None,
            "another tab"
        );
        strip.mouse_down(new_tab, MouseButton::Middle);
        assert_eq!(strip.mouse_up(new_tab, MouseButton::Middle), None);
        assert_eq!(strip.mouse_down(tab, MouseButton::Right), None);
    }

    #[test]
    fn hovering_shows() {
        let mut strip = strip(800.0, 3, 0);
        assert!(strip.mouse_move(center(strip.tab_rect(1))));
        assert!(!strip.mouse_move(center(strip.tab_rect(1))), "the same");
        assert!(strip.mouse_move(center(strip.close_rect(1))));
        assert!(strip.mouse_leave());
        assert!(!strip.mouse_leave());
    }

    /// The fade's steps, in a display list.
    fn fades(list: &DisplayItemList) -> usize {
        list.ops()
            .iter()
            .filter(|op| {
                matches!(op, PaintOp::DrawRect { rect, .. } if rect.width == FADE / f32::from(FADE_STEPS))
            })
            .count()
    }

    #[test]
    fn a_title_too_long_fades_out() {
        let mut strip = strip(800.0, 2, 0);
        assert_eq!(fades(&strip.paint()), 0);
        strip.set_tabs(vec!["short".into(), "a title far too long ".repeat(10)], 0);
        assert_eq!(fades(&strip.paint()), usize::from(FADE_STEPS));
    }

    #[test]
    fn the_active_tab_is_the_toolbars_color() {
        let mut strip = strip(800.0, 2, 1);
        let list = strip.paint();
        let active = strip.tab_rect(1);
        assert!(list.ops().iter().any(|op| matches!(
            op,
            PaintOp::DrawRRect { rrect, flags } if rrect.rect == active && flags.color == ACTIVE
        )));
        assert!(strip.set_tabs(vec!["tab 0".into(), "tab 1".into()], 0));
        assert!(!strip.set_tabs(vec!["tab 0".into(), "tab 1".into()], 0));
    }
}
