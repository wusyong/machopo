//! The toolbar (`ToolbarView` with its `LocationBarView`): a row of views
//! on the browser UI's tree, back, forward and reload, then the address
//! field, over the toolbar's background.

use std::f32::consts::PI;

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{PointF, RRectF, RectF, SizeF};
use ui::taffy::{AvailableSpace, Size};
use ui::{
    Component, CreateCtx, EventCtx, Handled, MeasureCtx, PaintCtx, PointerButton, PointerEvent,
    View,
};

use crate::browser_view::Command;
use crate::chrome::{BrowserUi, UiMessage};
use crate::theme::{ColorToken, SizeToken};

/// CSS px, above the page.
pub const HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Back,
    Forward,
    Reload,
}

impl Button {
    pub const ALL: [Button; 3] = [Button::Back, Button::Forward, Button::Reload];

    fn command(self) -> Command {
        match self {
            Button::Back => Command::Back,
            Button::Forward => Command::Forward,
            Button::Reload => Command::Reload,
        }
    }
}

/// The toolbar's row: its background and the line under it. It takes a
/// press on its background too, so that the pointer stays with the
/// toolbar until the button comes up.
pub struct ToolbarRow;

impl View<BrowserUi> for ToolbarRow {
    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let size = ctx.size();
        let theme = ctx.theme();
        for (rect, color) in [
            (
                RectF::from_size(size),
                theme.color(ColorToken::ToolbarBackground),
            ),
            (
                RectF::new(0.0, size.height - 1.0, size.width, 1.0),
                theme.color(ColorToken::ToolbarSeparator),
            ),
        ] {
            list.push(
                PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(color),
                },
                Some(rect),
            );
        }
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

impl Component<BrowserUi> for ToolbarRow {
    type Props = ();

    fn create(_: &(), _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        ToolbarRow
    }

    fn set_props(&mut self, _: &(), _: &mut EventCtx<'_, BrowserUi>) {}
}

/// What a toolbar button is: which, and whether it can be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavButtonProps {
    pub button: Button,
    pub enabled: bool,
}

/// Back, forward or reload: pressed by the primary button going down and
/// up on it while it is enabled.
pub struct NavButton {
    button: Button,
    enabled: bool,
    hovered: bool,
    /// The primary button went down on it: going up on it presses it.
    pressed: bool,
}

impl NavButton {
    pub fn new(button: Button) -> Self {
        Self {
            button,
            // Reload always is; back and forward once there is history.
            enabled: button == Button::Reload,
            hovered: false,
            pressed: false,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool, ctx: &mut EventCtx<'_, BrowserUi>) {
        if self.enabled != enabled {
            self.enabled = enabled;
            ctx.request_paint();
        }
    }
}

impl Component<BrowserUi> for NavButton {
    type Props = NavButtonProps;

    fn create(props: &NavButtonProps, _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        let mut button = Self::new(props.button);
        button.enabled = props.enabled;
        button
    }

    fn set_props(&mut self, props: &NavButtonProps, ctx: &mut EventCtx<'_, BrowserUi>) {
        if self.button != props.button {
            self.button = props.button;
            ctx.request_paint();
        }
        self.set_enabled(props.enabled, ctx);
    }
}

impl View<BrowserUi> for NavButton {
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_, BrowserUi>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        let size = ctx.theme().size(SizeToken::ToolbarButton);
        SizeF::new(size, size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let rect = RectF::from_size(ctx.size());
        let theme = ctx.theme();
        // Only an enabled button lights up under the pointer.
        if self.hovered && self.enabled {
            list.push(
                PaintOp::DrawRRect {
                    rrect: RRectF::uniform(rect, rect.width / 2.0),
                    flags: PaintFlags::fill(theme.color(ColorToken::ButtonHover)),
                },
                Some(rect),
            );
        }
        let color = theme.color(if self.enabled {
            ColorToken::Icon
        } else {
            ColorToken::IconDisabled
        });
        let center = PointF::new(rect.width / 2.0, rect.height / 2.0);
        list.push(
            PaintOp::DrawPath {
                path: gfx::Path::polygons(icon(self.button, center)),
                flags: PaintFlags::fill(color),
            },
            Some(rect),
        );
    }

    fn on_hover(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, hovered: bool) {
        if self.hovered != hovered {
            self.hovered = hovered;
            if self.enabled {
                ctx.request_paint();
            }
        }
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        match *event {
            PointerEvent::Down {
                button: PointerButton::Primary,
                ..
            } if self.enabled => {
                self.pressed = true;
                Handled::Yes
            }
            PointerEvent::Up {
                button: PointerButton::Primary,
                position,
                ..
            } if self.pressed => {
                self.pressed = false;
                if self.enabled && RectF::from_size(ctx.size()).contains(position) {
                    ctx.submit(UiMessage::Command(self.button.command()));
                }
                Handled::Yes
            }
            PointerEvent::Move { .. } if self.pressed => Handled::Yes,
            _ => Handled::No,
        }
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
