//! The tab strip (Chromium's `TabStrip`, with its `Tab`s and
//! `NewTabButton`): a row above the toolbar, each tab's title and close
//! button, and a button for a new tab, one view of the browser UI's tree.
//! It knows only the titles and which tab is active; what a click does goes
//! to the browser view as a message, whose `TabStripModel` has the tabs.
//! It works out where its tabs are itself: they paint into one another
//! (the line between two depends on both, the active one goes over the
//! others).

use base::numerics::saturated_cast;
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF, SizeF};
use ui::taffy::{AvailableSpace, Size};
use ui::{
    EventCtx, Handled, MeasureCtx, PaintCtx, PointerButton, PointerEvent, TextContexts, View,
};

use crate::browser_view::Command;
use crate::chrome::{BrowserUi, UiMessage};
use crate::theme::{BrowserTheme, ColorToken, SizeToken};

/// CSS px, above the toolbar.
pub const HEIGHT: f32 = 34.0;
/// How many steps a title too long fades out in.
const FADE_STEPS: u8 = 8;

/// The theme's sizes the strip lays its tabs out with, CSS px: kept, so
/// that where its parts are can be asked without a context.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Metrics {
    /// Where the first tab starts.
    padding: f32,
    /// The tabs' top: the strip's background shows above them.
    tab_top: f32,
    tab_max: f32,
    tab_min: f32,
    /// Narrower tabs have a close button only when active.
    close_min: f32,
    radius: f32,
    title_padding: f32,
    font_size: f32,
    close: f32,
    close_margin: f32,
    new_tab: f32,
    gap: f32,
    /// How far the end of a title too long fades out.
    fade: f32,
}

impl Metrics {
    fn new(theme: &BrowserTheme) -> Self {
        Self {
            padding: theme.size(SizeToken::TabStripPadding),
            tab_top: theme.size(SizeToken::TabTop),
            tab_max: theme.size(SizeToken::TabMaxWidth),
            tab_min: theme.size(SizeToken::TabMinWidth),
            close_min: theme.size(SizeToken::TabCloseMinWidth),
            radius: theme.size(SizeToken::TabRadius),
            title_padding: theme.size(SizeToken::TabTitlePadding),
            font_size: theme.size(SizeToken::TabFontSize),
            close: theme.size(SizeToken::TabClose),
            close_margin: theme.size(SizeToken::TabCloseMargin),
            new_tab: theme.size(SizeToken::NewTabButton),
            gap: theme.size(SizeToken::TabGap),
            fade: theme.size(SizeToken::TabFade),
        }
    }
}

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
    m: Metrics,
    titles: Vec<String>,
    active: usize,
    hovered: Option<Part>,
    /// What a button went down on, and which: the up on the same finishes
    /// the click.
    pressed: Option<(Part, PointerButton)>,
}

impl TabStrip {
    pub fn new(theme: &BrowserTheme) -> Self {
        Self {
            width: 0.0,
            m: Metrics::new(theme),
            titles: Vec::new(),
            active: 0,
            hovered: None,
            pressed: None,
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
        (self.width - self.m.new_tab - 2.0 * self.m.gap).max(self.m.padding)
    }

    /// Each tab's width: as wide as there is room for, between the least
    /// and the most.
    pub fn tab_width(&self) -> f32 {
        let count = self.titles.len().max(1) as f32;
        ((self.tabs_right() - self.m.padding) / count).clamp(self.m.tab_min, self.m.tab_max)
    }

    pub fn tab_rect(&self, index: usize) -> RectF {
        let width = self.tab_width();
        RectF::new(
            self.m.padding + index as f32 * width,
            self.m.tab_top,
            width,
            HEIGHT - self.m.tab_top,
        )
    }

    fn has_close(&self, index: usize) -> bool {
        self.tab_width() >= self.m.close_min || index == self.active
    }

    pub fn close_rect(&self, index: usize) -> RectF {
        let tab = self.tab_rect(index);
        RectF::new(
            tab.right() - self.m.close_margin - self.m.close,
            tab.y + (tab.height - self.m.close) / 2.0,
            self.m.close,
            self.m.close,
        )
    }

    /// Right after the last tab, or at the strip's end once the tabs
    /// overflow it.
    pub fn new_tab_rect(&self) -> RectF {
        let after = self.m.padding + self.titles.len() as f32 * self.tab_width();
        RectF::new(
            after.min(self.tabs_right()) + self.m.gap,
            self.m.tab_top + (HEIGHT - self.m.tab_top - self.m.new_tab) / 2.0,
            self.m.new_tab,
            self.m.new_tab,
        )
    }

    /// Where a tab's title goes: up to its close button, if it has one.
    fn title_rect(&self, index: usize) -> RectF {
        let tab = self.tab_rect(index);
        let right = if self.has_close(index) {
            self.close_rect(index).x - self.m.gap
        } else {
            tab.right() - self.m.title_padding
        };
        let x = tab.x + self.m.title_padding;
        RectF::new(x, tab.y, (right - x).max(0.0), tab.height)
    }

    pub fn part_at(&self, point: PointF) -> Part {
        if self.new_tab_rect().contains(point) {
            return Part::NewTab;
        }
        if point.y < self.m.tab_top || point.x < self.m.padding || point.x >= self.tabs_right() {
            return Part::Nothing;
        }
        let index: usize = saturated_cast((point.x - self.m.padding) / self.tab_width());
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
    pub fn mouse_down(&mut self, point: PointF, button: PointerButton) -> Option<Action> {
        let part = self.part_at(point);
        match (button, part) {
            (PointerButton::Primary, Part::Tab(index)) => Some(Action::Activate(index)),
            (PointerButton::Primary, Part::Close(_) | Part::NewTab)
            | (PointerButton::Middle, Part::Tab(_) | Part::Close(_)) => {
                self.pressed = Some((part, button));
                None
            }
            _ => None,
        }
    }

    /// A button went up at `point`: a click, if it went down on the same
    /// thing. The middle button closes the tab it clicked.
    pub fn mouse_up(&mut self, point: PointF, button: PointerButton) -> Option<Action> {
        let (pressed, with) = self.pressed.take()?;
        let part = self.part_at(point);
        if with != button {
            return None;
        }
        match (button, pressed, part) {
            (PointerButton::Primary, Part::Close(index), Part::Close(at)) if index == at => {
                Some(Action::Close(index))
            }
            (PointerButton::Primary, Part::NewTab, Part::NewTab) => Some(Action::NewTab),
            (
                PointerButton::Middle,
                Part::Tab(index) | Part::Close(index),
                Part::Tab(at) | Part::Close(at),
            ) if index == at => Some(Action::Close(index)),
            _ => None,
        }
    }

    fn is_hovered(&self, index: usize) -> bool {
        matches!(self.hovered, Some(Part::Tab(i) | Part::Close(i)) if i == index)
    }

    /// Records the strip, `HEIGHT` high and as wide as it is set.
    pub fn paint_into(
        &mut self,
        text: &mut TextContexts,
        theme: &BrowserTheme,
        list: &mut DisplayItemList,
    ) {
        let list = &mut *list;
        let frame = theme.color(ColorToken::TabStrip);
        let active_color = theme.color(ColorToken::ToolbarBackground);
        let hover = theme.color(ColorToken::TabHover);
        fill(list, RectF::new(0.0, 0.0, self.width, HEIGHT), frame);
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
                        self.m.radius,
                    ),
                    flags: PaintFlags::fill(hover),
                });
                hover
            } else {
                frame
            };
            self.paint_contents(text, theme, list, index, background);
            // Between two tabs neither of them active nor hovered.
            #[expect(clippy::arithmetic_side_effects, reason = "index < count")]
            let next = index + 1;
            if next < count
                && next != self.active
                && !self.is_hovered(index)
                && !self.is_hovered(next)
            {
                fill(
                    list,
                    RectF::new(tab.right() - 0.5, tab.y + 8.0, 1.0, tab.height - 16.0),
                    theme.color(ColorToken::TabSeparator),
                );
            }
        }
        if self.active < count {
            // Rounded on top, square where it meets the toolbar.
            let tab = self.tab_rect(self.active);
            list.push_op(PaintOp::DrawRRect {
                rrect: RRectF::uniform(tab, self.m.radius),
                flags: PaintFlags::fill(active_color),
            });
            fill(
                list,
                RectF::new(
                    tab.x,
                    tab.y + self.m.radius,
                    tab.width,
                    tab.height - self.m.radius,
                ),
                active_color,
            );
            self.paint_contents(text, theme, list, self.active, active_color);
        }
        list.push_op(PaintOp::Restore);
        let button = self.new_tab_rect();
        if self.hovered == Some(Part::NewTab) {
            list.push_op(PaintOp::DrawRRect {
                rrect: RRectF::uniform(button, self.m.new_tab / 2.0),
                flags: PaintFlags::fill(theme.color(ColorToken::TabButtonHover)),
            });
        }
        let c = center(button);
        let icon = theme.color(ColorToken::Icon);
        fill(list, RectF::new(c.x - 6.0, c.y - 1.0, 12.0, 2.0), icon);
        fill(list, RectF::new(c.x - 1.0, c.y - 6.0, 2.0, 12.0), icon);
    }

    /// A tab's title, fading out at its end if too long, and its close
    /// button, on `background`.
    fn paint_contents(
        &mut self,
        text: &mut TextContexts,
        theme: &BrowserTheme,
        list: &mut DisplayItemList,
        index: usize,
        background: Color,
    ) {
        let area = self.title_rect(index);
        let layout = text.label(&self.titles[index], self.m.font_size);
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::ClipRect {
            rect: area,
            anti_alias: false,
        });
        let top = area.y + ((area.height - layout.height()) / 2.0).round();
        ui::text::paint_glyphs(
            list,
            &layout,
            PointF::new(area.x, top),
            theme.color(ColorToken::TabTitle),
        );
        if layout.width() > area.width {
            // Chrome fades with a gradient; steps of the background, each
            // more opaque, look the same.
            let step = self.m.fade / f32::from(FADE_STEPS);
            for i in 0..FADE_STEPS {
                #[expect(
                    clippy::arithmetic_side_effects,
                    clippy::cast_possible_truncation,
                    reason = "i < FADE_STEPS: (i + 1) * 255 / FADE_STEPS is 255 at most"
                )]
                let alpha = (u16::from(i + 1) * 255 / u16::from(FADE_STEPS)) as u8;
                let color = Color::rgba(background.r, background.g, background.b, alpha);
                let x = area.right() - self.m.fade + f32::from(i) * step;
                fill(list, RectF::new(x, area.y, step, area.height), color);
            }
        }
        list.push_op(PaintOp::Restore);
        if self.has_close(index) {
            let button = self.close_rect(index);
            if self.hovered == Some(Part::Close(index)) {
                list.push_op(PaintOp::DrawRRect {
                    rrect: RRectF::uniform(button, self.m.close / 2.0),
                    flags: PaintFlags::fill(theme.color(ColorToken::TabButtonHover)),
                });
            }
            let c = center(button);
            let d = 4.0;
            for (from, to) in [((-d, -d), (d, d)), ((-d, d), (d, -d))] {
                list.push_op(PaintOp::DrawLine {
                    p0: PointF::new(c.x + from.0, c.y + from.1),
                    p1: PointF::new(c.x + to.0, c.y + to.1),
                    flags: PaintFlags::stroke(theme.color(ColorToken::Icon), 1.5),
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

impl View<BrowserUi> for TabStrip {
    fn measure(
        &mut self,
        _: &mut MeasureCtx<'_, BrowserUi>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        // As wide as the window (its style stretches it).
        SizeF::new(0.0, HEIGHT)
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        self.width = ctx.size().width;
        let (text, theme) = ctx.text_and_theme();
        self.m = Metrics::new(theme);
        self.paint_into(text, theme, list);
    }

    fn on_hover(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, hovered: bool) {
        if !hovered && self.mouse_leave() {
            ctx.request_paint();
        }
    }

    /// Any button down on the strip is its: the pointer stays with it until
    /// the button comes up.
    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        self.width = ctx.size().width;
        let action = match *event {
            PointerEvent::Move { position, .. } => {
                if self.mouse_move(position) {
                    ctx.request_paint();
                }
                None
            }
            PointerEvent::Down {
                position, button, ..
            } => self.mouse_down(position, button),
            PointerEvent::Up {
                position, button, ..
            } => self.mouse_up(position, button),
        };
        match action {
            Some(Action::Activate(index)) => {
                ctx.submit(UiMessage::Command(Command::SelectTab(index)));
            }
            Some(Action::Close(index)) => ctx.submit(UiMessage::CloseTab(index)),
            Some(Action::NewTab) => ctx.submit(UiMessage::Command(Command::NewTab)),
            None => {}
        }
        if matches!(event, PointerEvent::Move { .. }) {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The light theme's sizes.
    fn m() -> Metrics {
        Metrics::new(&BrowserTheme::light())
    }

    fn strip(width: f32, tabs: usize, active: usize) -> TabStrip {
        let mut strip = TabStrip::new(&BrowserTheme::light());
        strip.set_width(width);
        strip.set_tabs((0..tabs).map(|i| format!("tab {i}")).collect(), active);
        strip
    }

    #[test]
    fn tabs_are_as_wide_as_there_is_room_for() {
        assert_eq!(strip(800.0, 1, 0).tab_width(), m().tab_max);
        let few = strip(800.0, 10, 0);
        let room = 800.0 - m().new_tab - 2.0 * m().gap - m().padding;
        assert_eq!(few.tab_width(), room / 10.0);
        // Too many: the narrowest, and the last ones go past the end.
        let many = strip(800.0, 30, 0);
        assert_eq!(many.tab_width(), m().tab_min);
        assert!(many.tab_rect(29).x > many.tabs_right());
        // The new tab button follows the last tab, until there is no room.
        let one = strip(800.0, 1, 0);
        assert_eq!(one.new_tab_rect().x, m().padding + m().tab_max + m().gap);
        assert_eq!(many.new_tab_rect().x, many.tabs_right() + m().gap);
        assert!(many.new_tab_rect().right() <= 800.0);
    }

    #[test]
    fn narrow_tabs_have_a_close_button_only_when_active() {
        let wide = strip(800.0, 3, 1);
        assert!((0..3).all(|i| wide.has_close(i)));
        let narrow = strip(800.0, 12, 4);
        assert!(narrow.tab_width() < m().close_min);
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
            strip.mouse_down(tab, PointerButton::Primary),
            Some(Action::Activate(1))
        );
        assert_eq!(strip.mouse_up(tab, PointerButton::Primary), None);
        // Buttons on the up on them.
        assert_eq!(strip.mouse_down(close, PointerButton::Primary), None);
        assert_eq!(
            strip.mouse_up(close, PointerButton::Primary),
            Some(Action::Close(2))
        );
        strip.mouse_down(close, PointerButton::Primary);
        assert_eq!(
            strip.mouse_up(new_tab, PointerButton::Primary),
            None,
            "moved off"
        );
        strip.mouse_down(center(strip.close_rect(1)), PointerButton::Primary);
        assert_eq!(strip.mouse_up(close, PointerButton::Primary), None, "another's");
        strip.mouse_down(close, PointerButton::Primary);
        assert_eq!(
            strip.mouse_up(close, PointerButton::Middle),
            None,
            "another button"
        );
        strip.mouse_down(new_tab, PointerButton::Primary);
        assert_eq!(
            strip.mouse_up(new_tab, PointerButton::Primary),
            Some(Action::NewTab)
        );
        // The middle button closes the tab it clicks, its close button too.
        assert_eq!(strip.mouse_down(tab, PointerButton::Middle), None);
        assert_eq!(
            strip.mouse_up(tab, PointerButton::Middle),
            Some(Action::Close(1))
        );
        strip.mouse_down(close, PointerButton::Middle);
        assert_eq!(
            strip.mouse_up(center(strip.tab_rect(2)), PointerButton::Middle),
            Some(Action::Close(2))
        );
        strip.mouse_down(tab, PointerButton::Middle);
        assert_eq!(strip.mouse_up(new_tab, PointerButton::Middle), None);
        strip.mouse_down(tab, PointerButton::Middle);
        assert_eq!(
            strip.mouse_up(center(strip.tab_rect(2)), PointerButton::Middle),
            None,
            "another tab"
        );
        strip.mouse_down(new_tab, PointerButton::Middle);
        assert_eq!(strip.mouse_up(new_tab, PointerButton::Middle), None);
        assert_eq!(strip.mouse_down(tab, PointerButton::Secondary), None);
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

    fn paint(strip: &mut TabStrip) -> DisplayItemList {
        let mut list = DisplayItemList::record();
        strip.paint_into(
            &mut TextContexts::new(true),
            &BrowserTheme::light(),
            &mut list,
        );
        list.finalize();
        list
    }

    /// The fade's steps, in a display list.
    fn fades(list: &DisplayItemList) -> usize {
        list.ops()
            .iter()
            .filter(|op| {
                matches!(op, PaintOp::DrawRect { rect, .. } if rect.width == m().fade / f32::from(FADE_STEPS))
            })
            .count()
    }

    #[test]
    fn a_title_too_long_fades_out() {
        let mut strip = strip(800.0, 2, 0);
        assert_eq!(fades(&paint(&mut strip)), 0);
        strip.set_tabs(vec!["short".into(), "a title far too long ".repeat(10)], 0);
        assert_eq!(fades(&paint(&mut strip)), usize::from(FADE_STEPS));
    }

    #[test]
    fn the_active_tab_is_the_toolbars_color() {
        let mut strip = strip(800.0, 2, 1);
        let list = paint(&mut strip);
        let active = strip.tab_rect(1);
        assert!(list.ops().iter().any(|op| matches!(
            op,
            PaintOp::DrawRRect { rrect, flags } if rrect.rect == active && flags.color == BrowserTheme::light().color(ColorToken::ToolbarBackground)
        )));
        assert!(strip.set_tabs(vec!["tab 0".into(), "tab 1".into()], 0));
        assert!(!strip.set_tabs(vec!["tab 0".into(), "tab 1".into()], 0));
    }
}
