//! What a view is ([`View`]), and what it gets to work with while it
//! measures, paints and handles events: the contexts.

use std::any::Any;

use compositor_paint::DisplayItemList;
use gfx::{PointF, SizeF};
use taffy::{AvailableSpace, Size};

use crate::event::{Clipboard, Cursor, Handled, ImeEvent, KeyEvent, PointerEvent};
use crate::text::TextContexts;

/// The types a tree's views share: what they send whoever drives the tree,
/// and the theme they paint with. One type for both, so views and contexts
/// take one parameter.
pub trait App: 'static {
    /// What a view sends ([`EventCtx::submit`]), handled once the event
    /// it came from has been: the app's commands.
    type Message: 'static;
    /// The theme views paint with, the app's tokens (see [`crate::Token`]).
    type Theme: 'static;
}

/// A view: a node of the tree that measures, paints and handles events.
/// Where it is, and how it and its children are laid out, is the tree's
/// (its style); a view without children is a leaf, sized by
/// [`View::measure`].
///
/// Everything is in the view's own coordinates: CSS px from its top left.
pub trait View<A: App>: Any {
    /// A leaf's size, within what its parent offers: a known width or
    /// height is fixed, the available space is where it may wrap. The
    /// style's sizes, if set, win over it. None by default.
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_, A>,
        known: Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> SizeF {
        let _ = (ctx, known, available);
        SizeF::new(0.0, 0.0)
    }

    /// Records the view's own drawing (not its children's: the tree adds
    /// those after it, over it). Called only when the view asked to be
    /// painted again, or its size changed: what it records is kept.
    fn paint(&mut self, ctx: &mut PaintCtx<'_, A>, list: &mut DisplayItemList) {
        let _ = (ctx, list);
    }

    /// Whether `position` is on the view, for hit testing; it is within
    /// the view's bounds. A view that isn't hit passes the point on to
    /// what is under it (its parent's other children, then its parent).
    fn hit_test(&self, position: PointF) -> bool {
        let _ = position;
        true
    }

    /// The pointer: over the view, or anywhere while the view has captured
    /// it (by handling a press). One not handled goes on to the parent.
    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, A>, event: &PointerEvent) -> Handled {
        let _ = (ctx, event);
        Handled::No
    }

    /// The pointer came over the view (or one of its children), or left
    /// it.
    fn on_hover(&mut self, ctx: &mut EventCtx<'_, A>, hovered: bool) {
        let _ = (ctx, hovered);
    }

    /// A key, while the view or one of its children has the focus.
    fn on_key(&mut self, ctx: &mut EventCtx<'_, A>, event: &KeyEvent) -> Handled {
        let _ = (ctx, event);
        Handled::No
    }

    /// The input method's, while the view or one of its children has the
    /// focus.
    fn on_ime(&mut self, ctx: &mut EventCtx<'_, A>, event: &ImeEvent) -> Handled {
        let _ = (ctx, event);
        Handled::No
    }

    /// The view got the focus, or lost it.
    fn on_focus(&mut self, ctx: &mut EventCtx<'_, A>, focused: bool) {
        let _ = (ctx, focused);
    }

    /// What the pointer looks like at `position`.
    fn cursor(&self, position: PointF) -> Cursor {
        let _ = position;
        Cursor::Default
    }
}

/// A view that only holds others: it draws nothing and isn't hit itself,
/// only its children are. A tree's root is one.
#[derive(Debug, Default)]
pub struct Container;

impl<A: App> View<A> for Container {
    fn hit_test(&self, _: PointF) -> bool {
        false
    }
}

/// What the tree's views share, lent to each while it is called.
pub(crate) struct Shared<A: App> {
    pub theme: A::Theme,
    pub text: TextContexts,
    pub clipboard: Option<Box<dyn Clipboard>>,
    pub messages: Vec<A::Message>,
}

/// While a view measures.
pub struct MeasureCtx<'a, A: App> {
    pub(crate) shared: &'a mut Shared<A>,
}

impl<A: App> MeasureCtx<'_, A> {
    pub fn theme(&self) -> &A::Theme {
        &self.shared.theme
    }

    pub fn text(&mut self) -> &mut TextContexts {
        &mut self.shared.text
    }
}

/// While a view paints.
pub struct PaintCtx<'a, A: App> {
    pub(crate) shared: &'a mut Shared<A>,
    pub(crate) size: SizeF,
    pub(crate) focused: bool,
}

impl<A: App> PaintCtx<'_, A> {
    pub fn theme(&self) -> &A::Theme {
        &self.shared.theme
    }

    pub fn text(&mut self) -> &mut TextContexts {
        &mut self.shared.text
    }

    /// The view's size, as laid out.
    pub fn size(&self) -> SizeF {
        self.size
    }

    /// Whether the view has the focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }
}

/// What a view asked of the tree while it was called; the tree does it
/// after.
#[derive(Debug, Default)]
pub(crate) struct Requests {
    pub paint: bool,
    pub layout: bool,
    /// `Some(true)`: take the focus; `Some(false)`: give it up.
    pub focus: Option<bool>,
}

/// While a view handles an event, or is changed from outside the tree
/// ([`crate::ViewTree::update`]). The view says what of it changed: the
/// tree paints or lays out again only what is asked.
pub struct EventCtx<'a, A: App> {
    pub(crate) shared: &'a mut Shared<A>,
    pub(crate) size: SizeF,
    pub(crate) focused: bool,
    pub(crate) requests: Requests,
}

impl<A: App> EventCtx<'_, A> {
    pub fn theme(&self) -> &A::Theme {
        &self.shared.theme
    }

    pub fn text(&mut self) -> &mut TextContexts {
        &mut self.shared.text
    }

    /// The clipboard, if whoever drives the tree lent one.
    pub fn clipboard(&mut self) -> Option<&mut (dyn Clipboard + 'static)> {
        self.shared.clipboard.as_deref_mut()
    }

    /// The view's size, as last laid out.
    pub fn size(&self) -> SizeF {
        self.size
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// What the view draws changed: it is painted again.
    pub fn request_paint(&mut self) {
        self.requests.paint = true;
    }

    /// What decides the view's size changed: it is measured and laid out
    /// again, with its ancestors, and painted again.
    pub fn request_layout(&mut self) {
        self.requests.layout = true;
        self.requests.paint = true;
    }

    /// Sends `message` to whoever drives the tree, once the event has been
    /// handled.
    pub fn submit(&mut self, message: A::Message) {
        self.shared.messages.push(message);
    }

    /// Takes the keys.
    pub fn request_focus(&mut self) {
        self.requests.focus = Some(true);
    }

    /// Gives the keys up, if the view has them.
    pub fn resign_focus(&mut self) {
        self.requests.focus = Some(false);
    }
}
