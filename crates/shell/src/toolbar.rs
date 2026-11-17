//! The toolbar (`ToolbarView` with its `LocationBarView`): a row of views
//! on the browser UI's tree, back, forward and reload, then the address
//! field, over the toolbar's background.

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
                path: icon(self.button, center),
                flags: icon_flags(color),
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

/// How thick an icon's lines are, CSS px: all of them, as Chrome's (and
/// Material's) line icons are drawn.
const ICON_STROKE: f32 = 2.0;

/// The reload arrow's circle, its head's arms, and how far above the
/// middle its tip is.
const RELOAD_RADIUS: f32 = 6.5;
const RELOAD_ARM: f32 = 4.0;
const RELOAD_TIP: f32 = 2.5;

/// The flags an icon's lines are drawn with: round ends and joins.
fn icon_flags(color: gfx::Color) -> PaintFlags {
    PaintFlags {
        cap: compositor_paint::LineCap::Round,
        join: compositor_paint::LineJoin::Round,
        ..PaintFlags::stroke(color, ICON_STROKE)
    }
}

/// A button's icon around `c`, as lines to stroke (`icon_flags`): an arrow
/// for back and forward, a shaft and an open head; a circular arrow for
/// reload, most of a circle and an open head at its end along it.
fn icon(button: Button, c: PointF) -> gfx::Path {
    let p = |x: f32, y: f32| PointF::new(c.x + x, c.y + y);
    let mut path = gfx::Path::new();
    // An open head whose tip is at `tip`, pointing along the unit vector
    // `(dx, dy)`, its two sides `wing` long at 45 degrees from it.
    let head = |path: &mut gfx::Path, tip: PointF, dx: f32, dy: f32, wing: f32| {
        let (back_x, back_y) = (-dx * wing, -dy * wing);
        let side = |sign: f32| {
            // `back` turned 45 degrees one way or the other.
            let (cos, sin) = (
                std::f32::consts::FRAC_1_SQRT_2,
                sign * std::f32::consts::FRAC_1_SQRT_2,
            );
            PointF::new(
                tip.x + back_x * cos - back_y * sin,
                tip.y + back_x * sin + back_y * cos,
            )
        };
        path.move_to(side(1.0));
        path.line_to(tip);
        path.line_to(side(-1.0));
    };
    match button {
        Button::Back | Button::Forward => {
            let dir = if button == Button::Back { -1.0 } else { 1.0 };
            path.move_to(p(-6.5 * dir, 0.0));
            path.line_to(p(6.5 * dir, 0.0));
            head(&mut path, p(6.5 * dir, 0.0), dir, 0.0, 5.5);
        }
        Button::Reload => {
            // Clockwise (y points down) from a little below the right,
            // round to the top right, then on to a corner: the head, a
            // right angle whose arms go up and left, its corner the tip,
            // heading down and right (the line icons' "rotate clockwise").
            // The gap at the right shows where it starts.
            let (radius, arm) = (RELOAD_RADIUS, RELOAD_ARM);
            path.arc_to(p(-radius, -radius), p(radius, radius), 30.0, 285.0, true);
            let tip = p(radius, -RELOAD_TIP);
            path.line_to(tip);
            path.move_to(p(radius, -RELOAD_TIP - arm));
            path.line_to(tip);
            path.line_to(p(radius - arm, -RELOAD_TIP));
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the path's points are, relative to `c`.
    fn points(path: &gfx::Path, c: PointF) -> Vec<PointF> {
        path.elements()
            .iter()
            .flat_map(|element| match *element {
                gfx::PathElement::MoveTo(p) | gfx::PathElement::LineTo(p) => vec![p],
                gfx::PathElement::QuadTo(a, b) | gfx::PathElement::ConicTo(a, b, _) => vec![a, b],
                gfx::PathElement::CubicTo(a, b, d) => vec![a, b, d],
                gfx::PathElement::Close => vec![],
            })
            .map(|p| PointF::new(p.x - c.x, p.y - c.y))
            .collect()
    }

    #[test]
    fn icons_are_lines_of_one_width_with_round_ends_and_joins() {
        let flags = icon_flags(gfx::Color::BLACK);
        assert_eq!(
            flags.style,
            compositor_paint::PaintStyle::Stroke { width: 2.0 }
        );
        assert_eq!(flags.cap, compositor_paint::LineCap::Round);
        assert_eq!(flags.join, compositor_paint::LineJoin::Round);
    }

    #[test]
    fn icons_fit_their_button_with_their_lines() {
        let c = PointF::new(14.0, 14.0);
        for button in Button::ALL {
            let path = icon(button, c);
            // The curve's own bounds: an arc's conics' control points lie
            // outside it.
            let bounds = path.tight_bounds().unwrap();
            // Within 9 of the middle: an 18 px icon (with its lines' half
            // width, 20, as Chrome's) in a 28 px button.
            assert!(
                bounds.x >= c.x - 9.0 && bounds.right() <= c.x + 9.0,
                "{button:?} {bounds:?}"
            );
            assert!(
                bounds.y >= c.y - 9.0 && bounds.bottom() <= c.y + 9.0,
                "{button:?} {bounds:?}"
            );
        }
    }

    #[test]
    fn arrows_have_an_open_head_at_their_end() {
        let c = PointF::new(14.0, 14.0);
        // Back points left, forward right.
        for (button, dir) in [(Button::Back, -1.0f32), (Button::Forward, 1.0)] {
            let points = points(&icon(button, c), c);
            // The head is the last three points: a side, the tip, a side.
            let [.., a, tip, b] = points[..] else {
                panic!("{points:?}")
            };
            assert!(
                (tip.x - 6.5 * dir).abs() < 1e-4 && tip.y.abs() < 1e-4,
                "{tip:?}"
            );
            for side in [a, b] {
                // Behind the tip, at 45 degrees to the shaft.
                let (back, across) = ((tip.x - side.x) * dir, side.y - tip.y);
                assert!(back > 0.0, "{button:?} {side:?}");
                assert!((back - across.abs()).abs() < 1e-3, "{button:?} {side:?}");
            }
            // One above the shaft, one below.
            assert!((a.y + b.y).abs() < 1e-3 && a.y != b.y);
        }
    }

    #[test]
    fn the_reload_arrow_s_head_is_a_corner_at_its_circle_s_end() {
        let c = PointF::new(14.0, 14.0);
        let points = points(&icon(Button::Reload, c), c);
        let near = |a: PointF, b: PointF| (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4;
        // The head is the last three points: up, the corner, left; the arc
        // goes on to the corner.
        let [.., arc_end, up, tip, left] = points[..] else {
            panic!("{points:?}")
        };
        let (r, arm) = (RELOAD_RADIUS, RELOAD_ARM);
        let corner = PointF::new(r, -RELOAD_TIP);
        assert!(near(arc_end, corner) && near(tip, corner), "{points:?}");
        assert!(near(up, PointF::new(r, -RELOAD_TIP - arm)), "{up:?}");
        assert!(near(left, PointF::new(r - arm, -RELOAD_TIP)), "{left:?}");
        // The arc starts 30 degrees below the right and ends at the top
        // right, on the circle: a gap at the right, under the corner.
        let on_circle = |p: PointF, degrees: f32| {
            let a = degrees.to_radians();
            near(p, PointF::new(r * a.cos(), r * a.sin()))
        };
        assert!(on_circle(points[0], 30.0), "{:?}", points[0]);
        assert!(on_circle(points[points.len() - 5], 315.0));
        assert!(points[0].y - corner.y > 4.0, "a gap that shows");
    }
}
