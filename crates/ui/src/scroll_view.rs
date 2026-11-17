//! A view that scrolls its children (Chromium's `views::ScrollView`, with
//! its contents in a layer of their own: `ScrollWithLayers`). The offset is
//! its own state, kept from one description to the next; the tree draws
//! the children in their own layer, moved by it, so scrolling paints
//! nothing again.

use gfx::{PointF, RectF, SizeF};

use crate::element::Component;
use crate::event::{Handled, PointerEvent};
use crate::view::{App, CreateCtx, EventCtx, View};

/// Its children are laid out by its style as any container's (a flex
/// column, say), and may reach past it: they scroll. Give it a size, or let
/// a flex parent size it, with `overflow: scroll` (or `min_size` 0) so that
/// it isn't as tall as its children. No scrollbar: a wheel or a touchpad
/// over it scrolls it, until it can't, and then its scrolling ancestors.
#[derive(Debug, Default)]
pub struct ScrollView {
    offset: PointF,
}

impl ScrollView {
    /// How far its children are scrolled up and left, CSS px.
    pub fn offset(&self) -> PointF {
        self.offset
    }

    /// Scrolls to `offset`, kept within how far it can scroll.
    pub fn scroll_to<A: App>(&mut self, offset: PointF, ctx: &mut EventCtx<'_, A>) {
        self.offset = clamp(offset, ctx);
    }

    /// Scrolls as little as shows `rect` (in its children's coordinates,
    /// unscrolled): its top at the top if it is above, its bottom at the
    /// bottom if below, as much of it as fits if it is taller (Chromium's
    /// `ScrollRectToVisible`, Blink's `scrollIntoView` with `nearest`).
    pub fn scroll_rect_to_visible<A: App>(&mut self, rect: RectF, ctx: &mut EventCtx<'_, A>) {
        let size = ctx.size();
        let axis = |offset: f32, start: f32, length: f32, port: f32| {
            if start < offset || length > port {
                start
            } else if start + length > offset + port {
                start + length - port
            } else {
                offset
            }
        };
        let to = PointF::new(
            axis(self.offset.x, rect.x, rect.width, size.width),
            axis(self.offset.y, rect.y, rect.height, size.height),
        );
        self.scroll_to(to, ctx);
    }
}

/// `offset` within 0 and how far the content reaches past the view.
fn clamp<A: App>(offset: PointF, ctx: &EventCtx<'_, A>) -> PointF {
    let (size, content) = (ctx.size(), ctx.content_size());
    let max = SizeF::new(
        (content.width - size.width).max(0.0),
        (content.height - size.height).max(0.0),
    );
    PointF::new(
        offset.x.clamp(0.0, max.width),
        offset.y.clamp(0.0, max.height),
    )
}

impl<A: App> View<A> for ScrollView {
    fn scroll_offset(&self) -> Option<PointF> {
        Some(self.offset)
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, A>, event: &PointerEvent) -> Handled {
        let PointerEvent::Wheel { delta, .. } = *event else {
            return Handled::No;
        };
        let to = clamp(
            PointF::new(self.offset.x + delta.x, self.offset.y + delta.y),
            ctx,
        );
        // At its end it lets an ancestor scroll (Chromium's scroll
        // chaining).
        if to == self.offset {
            return Handled::No;
        }
        self.offset = to;
        Handled::Yes
    }

    fn on_layout(&mut self, ctx: &mut EventCtx<'_, A>) {
        self.offset = clamp(self.offset, ctx);
    }
}

impl<A: App> Component<A> for ScrollView {
    type Props = ();

    fn create(_: &(), _: &mut CreateCtx<'_, A>) -> Self {
        Self::default()
    }

    fn set_props(&mut self, _: &(), _: &mut EventCtx<'_, A>) {}
}
