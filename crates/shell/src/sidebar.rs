//! The sidebar's own views (the browser UI laid out down its left side,
//! after Zen's): its background, a row for each tab, the new tab row, and
//! the edge dragged to make it wider or narrower. The toolbar's buttons and
//! the address field are the same views as in the horizontal layout.

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF, SizeF};
use protocol::TabId;
use ui::taffy::{AvailableSpace, Size};
use ui::{
    Component, CreateCtx, Cursor, EventCtx, Handled, MeasureCtx, PaintCtx, PointerButton,
    PointerEvent, TextContexts, View,
};

use crate::browser_view::Command;
use crate::chrome::{BrowserUi, UiMessage};
use crate::theme::{BrowserTheme, ColorToken, SizeToken};

/// The close button's size, and its gap to the row's end.
const CLOSE: f32 = 16.0;
const CLOSE_MARGIN: f32 = 10.0;
/// Where a row's text starts.
const TEXT_X: f32 = 12.0;
/// How many steps a title too long fades out in.
const FADE_STEPS: u8 = 8;

/// The sidebar's background. It takes a press on itself, so that the
/// pointer stays with the sidebar until the button comes up.
pub struct SidebarPanel;

impl Component<BrowserUi> for SidebarPanel {
    type Props = ();

    fn create(_: &(), _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        SidebarPanel
    }

    fn set_props(&mut self, _: &(), _: &mut EventCtx<'_, BrowserUi>) {}
}

impl View<BrowserUi> for SidebarPanel {
    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let rect = RectF::from_size(ctx.size());
        let color = ctx.theme().color(ColorToken::SidebarBackground);
        list.push(
            PaintOp::DrawRect {
                rect,
                flags: PaintFlags::fill(color),
            },
            Some(rect),
        );
    }

    fn on_pointer(&mut self, _: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        match event {
            PointerEvent::Down {
                button: PointerButton::Primary,
                ..
            } => Handled::Yes,
            _ => Handled::No,
        }
    }
}

/// A tab's row: what it is, and whether it is the one shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarTabProps {
    pub tab: TabId,
    pub title: String,
    pub active: bool,
}

/// What is under a point in a tab's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Tab,
    Close,
}

/// A tab's row: its title, and a close button while the pointer is over it
/// or it is the tab shown. The primary button down on it shows it; its close
/// button closes it on the up on it; the middle button closes it.
pub struct SidebarTab {
    props: SidebarTabProps,
    hovered: bool,
    /// The pointer is over the close button.
    over_close: bool,
    /// What a button went down on, and which.
    pressed: Option<(Part, PointerButton)>,
}

impl SidebarTab {
    fn has_close(&self) -> bool {
        self.hovered || self.props.active
    }

    /// The close button, in a row `size`.
    fn close_rect(size: SizeF) -> RectF {
        RectF::new(
            size.width - CLOSE_MARGIN - CLOSE,
            (size.height - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        )
    }

    fn part_at(&self, size: SizeF, point: PointF) -> Option<Part> {
        if !RectF::from_size(size).contains(point) {
            return None;
        }
        if self.has_close() && Self::close_rect(size).inset(-2.0).contains(point) {
            Some(Part::Close)
        } else {
            Some(Part::Tab)
        }
    }
}

impl Component<BrowserUi> for SidebarTab {
    type Props = SidebarTabProps;

    fn create(props: &SidebarTabProps, _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        Self {
            props: props.clone(),
            hovered: false,
            over_close: false,
            pressed: None,
        }
    }

    fn set_props(&mut self, props: &SidebarTabProps, ctx: &mut EventCtx<'_, BrowserUi>) {
        if *props != self.props {
            self.props = props.clone();
            ctx.request_paint();
        }
    }
}

impl View<BrowserUi> for SidebarTab {
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_, BrowserUi>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        // As wide as the sidebar (its style stretches it).
        SizeF::new(0.0, ctx.theme().size(SizeToken::SidebarRowHeight))
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let size = ctx.size();
        let row = RectF::from_size(size);
        let (text, theme) = ctx.text_and_theme();
        let background = if self.props.active {
            Some(theme.color(ColorToken::SidebarTabActive))
        } else if self.hovered {
            Some(theme.color(ColorToken::SidebarRowHover))
        } else {
            None
        };
        if let Some(color) = background {
            list.push(
                PaintOp::DrawRRect {
                    rrect: RRectF::uniform(row, theme.size(SizeToken::SidebarRowRadius)),
                    flags: PaintFlags::fill(color),
                },
                Some(row),
            );
        }
        let right = if self.has_close() {
            Self::close_rect(size).x - 4.0
        } else {
            size.width - TEXT_X
        };
        let area = RectF::new(TEXT_X, 0.0, (right - TEXT_X).max(0.0), size.height);
        paint_label(
            text,
            theme,
            list,
            &self.props.title,
            area,
            background.unwrap_or(theme.color(ColorToken::SidebarBackground)),
        );
        if self.has_close() {
            let button = Self::close_rect(size);
            if self.over_close {
                list.push(
                    PaintOp::DrawRRect {
                        rrect: RRectF::uniform(button, CLOSE / 2.0),
                        flags: PaintFlags::fill(theme.color(ColorToken::TabButtonHover)),
                    },
                    Some(button),
                );
            }
            let c = PointF::new(button.x + CLOSE / 2.0, button.y + CLOSE / 2.0);
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

    fn on_hover(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, hovered: bool) {
        if self.hovered != hovered {
            self.hovered = hovered;
            self.over_close &= hovered;
            ctx.request_paint();
        }
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        let size = ctx.size();
        let part = self.part_at(size, event.position());
        match *event {
            PointerEvent::Move { .. } => {
                let over_close = part == Some(Part::Close);
                if over_close != self.over_close {
                    self.over_close = over_close;
                    ctx.request_paint();
                }
                return Handled::No;
            }
            PointerEvent::Down { button, .. } => match (button, part) {
                (PointerButton::Primary, Some(Part::Tab)) => {
                    ctx.submit(UiMessage::SelectTabById(self.props.tab));
                }
                (PointerButton::Primary, Some(Part::Close)) | (PointerButton::Middle, Some(_)) => {
                    self.pressed = part.map(|part| (part, button));
                }
                _ => return Handled::No,
            },
            PointerEvent::Up { button, .. } => {
                let Some((pressed, with)) = self.pressed.take() else {
                    return Handled::No;
                };
                let closes = with == button
                    && match button {
                        PointerButton::Primary => {
                            pressed == Part::Close && part == Some(Part::Close)
                        }
                        PointerButton::Middle => part.is_some(),
                        PointerButton::Secondary => false,
                    };
                if closes {
                    ctx.submit(UiMessage::CloseTabById(self.props.tab));
                }
            }
        }
        Handled::Yes
    }
}

/// The row that opens a new tab, as its button: down and up on it.
pub struct NewTabRow {
    hovered: bool,
    pressed: bool,
}

impl Component<BrowserUi> for NewTabRow {
    type Props = ();

    fn create(_: &(), _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        Self {
            hovered: false,
            pressed: false,
        }
    }

    fn set_props(&mut self, _: &(), _: &mut EventCtx<'_, BrowserUi>) {}
}

impl View<BrowserUi> for NewTabRow {
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_, BrowserUi>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        SizeF::new(0.0, ctx.theme().size(SizeToken::SidebarRowHeight))
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let size = ctx.size();
        let row = RectF::from_size(size);
        let (text, theme) = ctx.text_and_theme();
        let background = if self.hovered {
            let color = theme.color(ColorToken::SidebarRowHover);
            list.push(
                PaintOp::DrawRRect {
                    rrect: RRectF::uniform(row, theme.size(SizeToken::SidebarRowRadius)),
                    flags: PaintFlags::fill(color),
                },
                Some(row),
            );
            color
        } else {
            theme.color(ColorToken::SidebarBackground)
        };
        // A plus, then the label.
        let icon = theme.color(ColorToken::Icon);
        let c = PointF::new(TEXT_X + 6.0, size.height / 2.0);
        for rect in [
            RectF::new(c.x - 6.0, c.y - 1.0, 12.0, 2.0),
            RectF::new(c.x - 1.0, c.y - 6.0, 2.0, 12.0),
        ] {
            list.push(
                PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(icon),
                },
                Some(rect),
            );
        }
        let x = TEXT_X + 24.0;
        let area = RectF::new(x, 0.0, (size.width - x - TEXT_X).max(0.0), size.height);
        paint_label(text, theme, list, "New Tab", area, background);
    }

    fn on_hover(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, hovered: bool) {
        if self.hovered != hovered {
            self.hovered = hovered;
            ctx.request_paint();
        }
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        match *event {
            PointerEvent::Down {
                button: PointerButton::Primary,
                ..
            } => {
                self.pressed = true;
                Handled::Yes
            }
            PointerEvent::Up {
                button: PointerButton::Primary,
                position,
                ..
            } if self.pressed => {
                self.pressed = false;
                if RectF::from_size(ctx.size()).contains(position) {
                    ctx.submit(UiMessage::Command(Command::NewTab));
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// How wide the sidebar is, for the edge dragged to change it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarEdgeProps {
    pub width: f32,
}

/// The sidebar's right edge: dragging it sends the width the sidebar would
/// be, for the browser view to clamp and lay out.
pub struct SidebarEdge {
    width: f32,
    /// Where in the edge the primary button went down, while it is down.
    grab: Option<f32>,
}

impl Component<BrowserUi> for SidebarEdge {
    type Props = SidebarEdgeProps;

    fn create(props: &SidebarEdgeProps, _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        Self {
            width: props.width,
            grab: None,
        }
    }

    fn set_props(&mut self, props: &SidebarEdgeProps, _: &mut EventCtx<'_, BrowserUi>) {
        // Only where the drag starts from: it draws the same.
        self.width = props.width;
    }
}

impl View<BrowserUi> for SidebarEdge {
    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let size = ctx.size();
        let theme = ctx.theme();
        let rect = RectF::from_size(size);
        list.push(
            PaintOp::DrawRect {
                rect,
                flags: PaintFlags::fill(theme.color(ColorToken::SidebarBackground)),
            },
            Some(rect),
        );
        let line = RectF::new(size.width - 1.0, 0.0, 1.0, size.height);
        list.push(
            PaintOp::DrawRect {
                rect: line,
                flags: PaintFlags::fill(theme.color(ColorToken::SidebarEdge)),
            },
            Some(line),
        );
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        match *event {
            PointerEvent::Down {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                self.grab = Some(position.x);
                Handled::Yes
            }
            PointerEvent::Move { position, .. } => match self.grab {
                // The edge moves with the width: where it is grabbed stays
                // under the pointer.
                Some(grab) => {
                    ctx.submit(UiMessage::ResizeSidebar(self.width + position.x - grab));
                    Handled::Yes
                }
                None => Handled::No,
            },
            PointerEvent::Up {
                button: PointerButton::Primary,
                ..
            } if self.grab.is_some() => {
                self.grab = None;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn cursor(&self, _: PointF) -> Cursor {
        Cursor::ResizeHorizontal
    }
}

/// `label` on one line in `area`, centered down it, fading out over
/// `background` at its end if too long.
fn paint_label(
    text: &mut TextContexts,
    theme: &BrowserTheme,
    list: &mut DisplayItemList,
    label: &str,
    area: RectF,
    background: Color,
) {
    let layout = text.label(label, theme.size(SizeToken::SidebarFontSize));
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
        // As the horizontal tabs do: steps of the background, each more
        // opaque.
        let fade = theme.size(SizeToken::TabFade);
        let step = fade / f32::from(FADE_STEPS);
        for i in 0..FADE_STEPS {
            #[expect(
                clippy::arithmetic_side_effects,
                clippy::cast_possible_truncation,
                reason = "i < FADE_STEPS: (i + 1) * 255 / FADE_STEPS is 255 at most"
            )]
            let alpha = (u16::from(i + 1) * 255 / u16::from(FADE_STEPS)) as u8;
            let rect = RectF::new(
                area.right() - fade + f32::from(i) * step,
                area.y,
                step,
                area.height,
            );
            list.push(
                PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(Color::rgba(
                        background.r,
                        background.g,
                        background.b,
                        alpha,
                    )),
                },
                Some(rect),
            );
        }
    }
    list.push_op(PaintOp::Restore);
}
