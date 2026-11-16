//! The tree of views: an arena of nodes, each a view and where it is. The
//! tree lays the views out (taffy, through its low-level traits, as the
//! `layout` crate does for boxes), paints them into one display list,
//! keeping what each recorded until it asks to be painted again, hit tests
//! and dispatches the user's events.

use std::any::Any;
use std::sync::Arc;

use compositor_paint::DisplayItemList;
use gfx::{PointF, RectF, SizeF};
use taffy::{
    AvailableSpace, Cache, CacheTree, Display, Layout, LayoutFlexboxContainer, LayoutGridContainer,
    LayoutInput, LayoutOutput, LayoutPartialTree, Line, NodeId, RequestedAxis, RunMode,
    SizingMode, Style, TraversePartialTree, compute_cached_layout, compute_flexbox_layout,
    compute_grid_layout, compute_hidden_layout, compute_leaf_layout,
};

use crate::event::{
    Clipboard, Cursor, Handled, ImeEvent, KeyEvent, PointerButton, PointerEvent,
};
use crate::id::{AnyViewId, ViewId};
use crate::text::TextContexts;
use crate::view::{App, Container, EventCtx, MeasureCtx, PaintCtx, Requests, Shared, View};

/// A slot of the arena: the node it holds now, if any, and how many views
/// it has held.
struct Slot {
    generation: u32,
    node: Option<Node>,
}

struct Node {
    parent: Option<AnyViewId>,
    /// In paint order: a later child is over an earlier one.
    children: Vec<AnyViewId>,
    style: Style,
    /// Where taffy put the node, in its parent's coordinates.
    layout: Layout,
    cache: Cache,
    /// Its bounds as last laid out, in its parent's coordinates.
    bounds: RectF,
    /// It asked to be painted again (or its size changed).
    needs_paint: bool,
    /// What it recorded last, and where in the tree's coordinates it was
    /// put: what the next paint keeps, if it is still asked for.
    painted: Option<Painted>,
}

struct Painted {
    list: DisplayItemList,
    origin: PointF,
}

impl Painted {
    /// Where it may have drawn, in the tree's coordinates: every op's
    /// visual rect, an op recorded without one (which may draw anywhere)
    /// included, which the list's bounds leave out.
    fn drawn(&self) -> RectF {
        self.list
            .visual_rects()
            .iter()
            .fold(RectF::default(), |drawn, rect| drawn.union(rect))
            .offset(self.origin.x, self.origin.y)
    }
}

/// What the tree drew: its layers, the first under the others. There is
/// one; a view that scrolls or animates may get its own later.
#[derive(Debug, Clone)]
pub struct Frame {
    pub layers: Vec<FrameLayer>,
}

#[derive(Debug, Clone)]
pub struct FrameLayer {
    /// The layer's size, CSS px: the tree's.
    pub size: SizeF,
    pub display_list: Arc<DisplayItemList>,
    /// What changed since the layer's last frame, in its coordinates:
    /// only these need rastering again. `None`: all of it (the first
    /// frame, or a new size).
    pub invalidation: Option<Arc<[RectF]>>,
}

/// A tree of views of the app `A`, with a [`Container`] as its root.
pub struct ViewTree<A: App> {
    /// Each slot's view, apart from its node so that a view can be called
    /// while the tree's structure is read.
    views: Vec<Option<Box<dyn View<A>>>>,
    slots: Vec<Slot>,
    /// Slots free for a new view.
    free: Vec<u32>,
    root: AnyViewId,
    shared: Shared<A>,
    size: SizeF,
    /// Some view's size may have changed: laid out again before painting.
    needs_layout: bool,
    /// The layer is drawn again whole (first, or after a new size).
    needs_full_paint: bool,
    /// What changed outside what the next paint sees (views removed), in
    /// the tree's coordinates.
    damage: Vec<RectF>,
    /// The views under the pointer, from the root down.
    hovered: Vec<AnyViewId>,
    /// The view that handled the press of a button still down, which gets
    /// the pointer until it comes up.
    captured: Option<(AnyViewId, PointerButton)>,
    focused: Option<AnyViewId>,
}

impl<A: App> ViewTree<A> {
    pub fn new(theme: A::Theme, text: TextContexts) -> Self {
        let mut tree = Self {
            views: Vec::new(),
            slots: Vec::new(),
            free: Vec::new(),
            root: AnyViewId {
                index: 0,
                generation: 0,
            },
            shared: Shared {
                theme,
                text,
                clipboard: None,
                messages: Vec::new(),
            },
            size: SizeF::new(0.0, 0.0),
            needs_layout: true,
            needs_full_paint: true,
            damage: Vec::new(),
            hovered: Vec::new(),
            captured: None,
            focused: None,
        };
        tree.root = tree.insert(Container).any();
        tree
    }

    pub fn root(&self) -> AnyViewId {
        self.root
    }

    pub fn size(&self) -> SizeF {
        self.size
    }

    /// The tree's size, CSS px: its root's.
    pub fn set_size(&mut self, size: SizeF) {
        if size != self.size {
            self.size = size;
            self.needs_layout = true;
            self.needs_full_paint = true;
        }
    }

    pub fn theme(&self) -> &A::Theme {
        &self.shared.theme
    }

    /// A new theme: everything is painted again.
    pub fn set_theme(&mut self, theme: A::Theme) {
        self.shared.theme = theme;
        for slot in &mut self.slots {
            if let Some(node) = &mut slot.node {
                node.needs_paint = true;
                node.cache.clear();
            }
        }
        self.needs_layout = true;
    }

    /// The text contexts the views use, for whatever draws beside the tree.
    pub fn text(&mut self) -> &mut TextContexts {
        &mut self.shared.text
    }

    /// The clipboard views may use ([`EventCtx::clipboard`]).
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.shared.clipboard = Some(clipboard);
    }

    /// What the views sent since the last call, in order.
    pub fn take_messages(&mut self) -> Vec<A::Message> {
        std::mem::take(&mut self.shared.messages)
    }

    // --- Structure ----------------------------------------------------------

    /// Adds `view` to the tree, in no parent yet (see
    /// [`ViewTree::append_child`]), with the default style.
    pub fn insert<T: View<A>>(&mut self, view: T) -> ViewId<T> {
        let node = Node {
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            layout: Layout::default(),
            cache: Cache::new(),
            bounds: RectF::default(),
            needs_paint: true,
            painted: None,
        };
        let id = match self.free.pop() {
            Some(index) => {
                let id = AnyViewId {
                    index,
                    generation: self.slots[index as usize].generation,
                };
                self.slots[id.slot()].node = Some(node);
                self.views[id.slot()] = Some(Box::new(view));
                id
            }
            None => {
                let index = u32::try_from(self.slots.len())
                    .expect("fewer than u32::MAX views at once");
                self.slots.push(Slot {
                    generation: 0,
                    node: Some(node),
                });
                self.views.push(Some(Box::new(view)));
                AnyViewId {
                    index,
                    generation: 0,
                }
            }
        };
        ViewId::new(id)
    }

    fn node(&self, id: AnyViewId) -> Option<&Node> {
        let slot = self.slots.get(id.slot())?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()
    }

    fn node_mut(&mut self, id: AnyViewId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(id.slot())?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_mut()
    }

    /// Whether `id` is a view in the tree.
    pub fn contains(&self, id: impl Into<AnyViewId>) -> bool {
        self.node(id.into()).is_some()
    }

    pub fn parent(&self, id: impl Into<AnyViewId>) -> Option<AnyViewId> {
        self.node(id.into())?.parent
    }

    pub fn children(&self, id: impl Into<AnyViewId>) -> &[AnyViewId] {
        self.node(id.into())
            .map(|node| node.children.as_slice())
            .unwrap_or_default()
    }

    /// Puts `child` last among `parent`'s children (over the others),
    /// taking it from its parent, if it had one.
    pub fn append_child(&mut self, parent: impl Into<AnyViewId>, child: impl Into<AnyViewId>) {
        let parent = parent.into();
        let index = self.children(parent).len();
        self.insert_child(parent, index, child);
    }

    /// Puts `child` at `index` among `parent`'s children (clamped to their
    /// number), taking it from its parent, if it had one.
    pub fn insert_child(
        &mut self,
        parent: impl Into<AnyViewId>,
        index: usize,
        child: impl Into<AnyViewId>,
    ) {
        let (parent, child) = (parent.into(), child.into());
        assert!(self.contains(parent) && self.contains(child), "views of the tree");
        assert!(
            child != self.root && !self.is_ancestor(child, parent),
            "a view can't go under itself"
        );
        self.detach(child);
        let node = self.node_mut(parent).expect("checked");
        let index = index.min(node.children.len());
        node.children.insert(index, child);
        self.node_mut(child).expect("checked").parent = Some(parent);
        self.mark_layout(parent);
    }

    /// Whether `ancestor` is `id` or above it.
    fn is_ancestor(&self, ancestor: AnyViewId, mut id: AnyViewId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            match self.parent(id) {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }

    /// Takes `id` from its parent; what it drew is drawn no more.
    fn detach(&mut self, id: AnyViewId) {
        let Some(parent) = self.parent(id) else {
            return;
        };
        if let Some(node) = self.node_mut(parent) {
            node.children.retain(|&child| child != id);
        }
        if let Some(node) = self.node_mut(id) {
            node.parent = None;
        }
        self.forget_painted(id);
        self.mark_layout(parent);
    }

    /// What `id` and the views under it drew is damage, and is drawn no
    /// more until they are painted again.
    fn forget_painted(&mut self, id: AnyViewId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let Some(node) = self.node_mut(id) else {
                continue;
            };
            let painted = node.painted.take();
            node.needs_paint = true;
            stack.extend(node.children.iter().copied());
            if let Some(painted) = painted {
                self.damage.push(painted.drawn());
            }
        }
    }

    /// Removes `id` and the views under it from the tree.
    pub fn remove(&mut self, id: impl Into<AnyViewId>) {
        let id = id.into();
        assert!(id != self.root, "the root stays");
        if !self.contains(id) {
            return;
        }
        self.detach(id);
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let slot = &mut self.slots[id.slot()];
            let Some(node) = slot.node.take() else {
                continue;
            };
            stack.extend(node.children);
            self.views[id.slot()] = None;
            // A slot whose generation has run out isn't used again: an id
            // of one of its old views would find the next.
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(id.index);
            }
        }
        // The focus, the capture and the views hovered may still name a
        // removed view: what reads them asks whether it is still there,
        // and its id finds nothing (not even a later view in its slot).
    }

    pub fn style(&self, id: impl Into<AnyViewId>) -> Option<&Style> {
        Some(&self.node(id.into())?.style)
    }

    /// How `id` is laid out among its siblings, and its children within
    /// it (taffy's `Style`: flex or grid containers; a view without
    /// children is sized by its `measure`).
    pub fn set_style(&mut self, id: impl Into<AnyViewId>, style: Style) {
        let id = id.into();
        if let Some(node) = self.node_mut(id) {
            node.style = style;
            self.mark_layout(id);
        }
    }

    /// `id`'s bounds as last laid out, in its parent's coordinates.
    pub fn bounds(&self, id: impl Into<AnyViewId>) -> Option<RectF> {
        Some(self.node(id.into())?.bounds)
    }

    /// `id`'s bounds as last laid out, in the tree's coordinates.
    pub fn absolute_bounds(&self, id: impl Into<AnyViewId>) -> Option<RectF> {
        let id = id.into();
        let bounds = self.node(id)?.bounds;
        let origin = self.absolute_origin(id)?;
        Some(RectF::new(origin.x, origin.y, bounds.width, bounds.height))
    }

    fn absolute_origin(&self, mut id: AnyViewId) -> Option<PointF> {
        let mut origin = PointF::new(0.0, 0.0);
        loop {
            let node = self.node(id)?;
            origin.x += node.bounds.x;
            origin.y += node.bounds.y;
            match node.parent {
                Some(parent) => id = parent,
                None if id == self.root => return Some(origin),
                // Not under the root: nowhere.
                None => return None,
            }
        }
    }

    /// `id` and its ancestors are laid out again: their taffy caches are
    /// stale.
    fn mark_layout(&mut self, id: AnyViewId) {
        self.needs_layout = true;
        let mut at = Some(id);
        while let Some(id) = at {
            let Some(node) = self.node_mut(id) else {
                break;
            };
            node.cache.clear();
            at = node.parent;
        }
    }

    // --- Views --------------------------------------------------------------

    /// The view `id` names, if it is still in the tree.
    pub fn get<T: View<A>>(&self, id: ViewId<T>) -> Option<&T> {
        let id = id.any();
        self.node(id)?;
        let view: &dyn Any = self.views[id.slot()].as_deref()?;
        view.downcast_ref()
    }

    /// Changes the view `id` names from outside the tree: `change` gets it
    /// and a context to say what of it changed (as an event handler does),
    /// and its result is returned. `None` if the view is gone.
    pub fn update<T: View<A>, R>(
        &mut self,
        id: ViewId<T>,
        change: impl FnOnce(&mut T, &mut EventCtx<'_, A>) -> R,
    ) -> Option<R> {
        let any = id.any();
        self.call(any, |view, ctx| {
            let view: &mut dyn Any = view;
            let view = view.downcast_mut::<T>().expect("a ViewId<T> names a T");
            change(view, ctx)
        })
    }

    /// Calls the view `id` with a context, then does what it asked.
    fn call<R>(
        &mut self,
        id: AnyViewId,
        f: impl FnOnce(&mut dyn View<A>, &mut EventCtx<'_, A>) -> R,
    ) -> Option<R> {
        let size = self.node(id)?.bounds.size();
        let focused = self.focused == Some(id);
        let view = self.views[id.slot()].as_deref_mut()?;
        let mut ctx = EventCtx {
            shared: &mut self.shared,
            size,
            focused,
            requests: Requests::default(),
        };
        let result = f(view, &mut ctx);
        let requests = ctx.requests;
        self.apply(id, requests);
        Some(result)
    }

    fn apply(&mut self, id: AnyViewId, requests: Requests) {
        if requests.layout {
            self.mark_layout(id);
        }
        if requests.paint
            && let Some(node) = self.node_mut(id)
        {
            node.needs_paint = true;
        }
        match requests.focus {
            Some(true) => self.set_focus(Some(id)),
            Some(false) if self.focused == Some(id) => self.set_focus(None),
            _ => {}
        }
    }

    // --- Layout -------------------------------------------------------------

    /// Lays the tree out, if anything changed that may move or size a view.
    pub fn layout(&mut self) {
        if !self.needs_layout {
            return;
        }
        self.needs_layout = false;
        let (width, height) = (self.size.width, self.size.height);
        let inputs = LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: taffy::Size {
                width: Some(width),
                height: Some(height),
            },
            known_dimensions_are_definite: taffy::Size {
                width: true,
                height: true,
            },
            parent_size: taffy::Size {
                width: Some(width),
                height: Some(height),
            },
            available_space: taffy::Size {
                width: AvailableSpace::Definite(width),
                height: AvailableSpace::Definite(height),
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let root = self.root;
        {
            let mut pass = LayoutPass {
                views: &mut self.views,
                slots: &mut self.slots,
                shared: &mut self.shared,
            };
            pass.compute_child_layout(node_id(root), inputs);
        }
        // The root is where the tree is; every other view where taffy put
        // it. One whose size changed paints again.
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let node = self.node_mut(id).expect("a child of the tree");
            let bounds = if id == root {
                RectF::new(0.0, 0.0, width, height)
            } else {
                let layout = node.layout;
                RectF::new(
                    layout.location.x,
                    layout.location.y,
                    layout.size.width,
                    layout.size.height,
                )
            };
            if bounds.size() != node.bounds.size() {
                node.needs_paint = true;
            }
            node.bounds = bounds;
            stack.extend(node.children.iter().copied());
        }
    }

    // --- Painting -----------------------------------------------------------

    /// Lays out and paints what changed since the last frame; `None` if
    /// nothing did. Each view's recording is kept and drawn again where
    /// the view is now, until it asks to paint again.
    pub fn paint(&mut self) -> Option<Frame> {
        self.layout();
        let full = std::mem::take(&mut self.needs_full_paint);
        let mut damage = std::mem::take(&mut self.damage);
        let mut changed = full || !damage.is_empty();
        let mut list = DisplayItemList::new();
        let mut stack = vec![(self.root, PointF::new(0.0, 0.0))];
        while let Some((id, parent_origin)) = stack.pop() {
            let node = self.node(id).expect("a child of the tree");
            if node.style.display == Display::None {
                if node.painted.is_some() {
                    changed = true;
                    self.forget_painted(id);
                    damage.append(&mut self.damage);
                }
                continue;
            }
            let origin = PointF::new(
                parent_origin.x + node.bounds.x,
                parent_origin.y + node.bounds.y,
            );
            let repaint = node.needs_paint || node.painted.is_none();
            if repaint {
                let painted = self.paint_view(id, origin);
                let node = self.node_mut(id).expect("a child of the tree");
                if let Some(old) = node.painted.replace(painted) {
                    damage.push(old.drawn());
                }
                changed = true;
                damage.push(node.painted.as_ref().expect("just painted").drawn());
            } else {
                let node = self.node_mut(id).expect("a child of the tree");
                let painted = node.painted.as_mut().expect("painted before");
                if painted.origin != origin {
                    // Moved: where it was and where it is.
                    damage.push(painted.drawn());
                    painted.origin = origin;
                    damage.push(painted.drawn());
                    changed = true;
                }
            }
            let node = self.node(id).expect("a child of the tree");
            let painted = node.painted.as_ref().expect("painted");
            list.push_list_at(&painted.list, origin.x, origin.y);
            // Later children over earlier ones: pushed so the first pops
            // first.
            stack.extend(node.children.iter().rev().map(|&child| (child, origin)));
        }
        if !changed {
            return None;
        }
        list.finalize();
        // Within the layer (an op that may draw anywhere damages it all).
        let layer = RectF::from_size(self.size);
        let mut damage: Vec<RectF> = damage
            .iter()
            .map(|rect| rect.intersect(&layer))
            .filter(|rect| !rect.is_empty())
            .collect();
        damage.dedup();
        Some(Frame {
            layers: vec![FrameLayer {
                size: self.size,
                display_list: Arc::new(list),
                invalidation: (!full).then(|| damage.into()),
            }],
        })
    }

    /// Records what `id` draws, to be put at `origin`.
    fn paint_view(&mut self, id: AnyViewId, origin: PointF) -> Painted {
        let list = self.record(id);
        let node = self.node_mut(id).expect("a child of the tree");
        node.needs_paint = false;
        Painted { list, origin }
    }

    fn record(&mut self, id: AnyViewId) -> DisplayItemList {
        let size = self.node(id).expect("a child of the tree").bounds.size();
        let focused = self.focused == Some(id);
        let mut list = DisplayItemList::record();
        if let Some(view) = self.views[id.slot()].as_deref_mut() {
            let mut ctx = PaintCtx {
                shared: &mut self.shared,
                size,
                focused,
            };
            view.paint(&mut ctx, &mut list);
        }
        list.finalize();
        list
    }

    /// The views whose kept recording isn't what they would record now:
    /// one changed without asking to paint again. For tests: a view
    /// should list none after a paint.
    pub fn stale_views(&mut self) -> Vec<AnyViewId> {
        let ids: Vec<AnyViewId> = self.descendants(self.root);
        ids.into_iter()
            .filter(|&id| {
                let fresh = self.record(id);
                self.node(id)
                    .and_then(|node| node.painted.as_ref())
                    .is_some_and(|painted| painted.list != fresh)
            })
            .collect()
    }

    /// `id` and every view under it, parents first.
    fn descendants(&self, id: AnyViewId) -> Vec<AnyViewId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.children(id).iter().rev().copied());
        }
        out
    }

    // --- Hit testing and events ---------------------------------------------

    /// The deepest view at `point` (the tree's coordinates) that is hit,
    /// and `point` in its coordinates: later children before earlier ones,
    /// children before their parent.
    pub fn hit_test(&self, point: PointF) -> Option<(AnyViewId, PointF)> {
        self.hit_test_in(self.root, point)
    }

    fn hit_test_in(&self, id: AnyViewId, point: PointF) -> Option<(AnyViewId, PointF)> {
        let node = self.node(id)?;
        if node.style.display == Display::None {
            return None;
        }
        let local = PointF::new(point.x - node.bounds.x, point.y - node.bounds.y);
        if !RectF::from_size(node.bounds.size()).contains(local) {
            return None;
        }
        for &child in node.children.iter().rev() {
            if let Some(hit) = self.hit_test_in(child, local) {
                return Some(hit);
            }
        }
        let view = self.views[id.slot()].as_deref()?;
        view.hit_test(local).then_some((id, local))
    }

    /// `point`, in the tree's coordinates, in `id`'s.
    fn to_local(&self, id: AnyViewId, point: PointF) -> Option<PointF> {
        let origin = self.absolute_origin(id)?;
        Some(PointF::new(point.x - origin.x, point.y - origin.y))
    }

    /// `id` and its ancestors, from the root down.
    fn path(&self, id: AnyViewId) -> Vec<AnyViewId> {
        let mut path = vec![id];
        let mut at = id;
        while let Some(parent) = self.parent(at) {
            path.push(parent);
            at = parent;
        }
        path.reverse();
        path
    }

    /// The pointer, in the tree's coordinates: to the view that has
    /// captured it, or else to the view under it, then up its ancestors
    /// until one handles it. A press handled captures the pointer for its
    /// handler until that button comes up. The views under the pointer
    /// hear that they are hovered, or no more.
    pub fn pointer(&mut self, event: PointerEvent) -> Handled {
        let hit = self.hit_test(event.position());
        self.set_hovered(hit.map(|(id, _)| id));
        let captured = self
            .captured
            .filter(|(id, _)| self.contains(*id))
            .map(|(id, _)| id);
        let handled = match captured {
            Some(id) => {
                let local = self.to_local(id, event.position()).unwrap_or_default();
                self.call(id, |view, ctx| view.on_pointer(ctx, &event.at(local)))
                    .unwrap_or(Handled::No)
            }
            None => match hit {
                Some((target, _)) => self.bubble_pointer(target, event),
                None => Handled::No,
            },
        };
        if let PointerEvent::Up { button, .. } = event
            && self.captured.is_some_and(|(_, with)| with == button)
        {
            self.captured = None;
        }
        handled
    }

    fn bubble_pointer(&mut self, target: AnyViewId, event: PointerEvent) -> Handled {
        let mut at = Some(target);
        while let Some(id) = at {
            let local = self.to_local(id, event.position()).unwrap_or_default();
            let handled = self
                .call(id, |view, ctx| view.on_pointer(ctx, &event.at(local)))
                .unwrap_or(Handled::No);
            if handled.is_handled() {
                if let PointerEvent::Down { button, .. } = event
                    && self.captured.is_none()
                {
                    self.captured = Some((id, button));
                }
                return Handled::Yes;
            }
            at = self.parent(id);
        }
        Handled::No
    }

    /// The pointer left the tree (it is over something else, or out of the
    /// window): nothing in the tree is hovered.
    pub fn pointer_left(&mut self) {
        self.set_hovered(None);
    }

    /// The view that has captured the pointer, if any.
    pub fn captured(&self) -> Option<AnyViewId> {
        self.captured
            .map(|(id, _)| id)
            .filter(|&id| self.contains(id))
    }

    /// The views from the root down to `target` are hovered, the others
    /// not: those that left hear it first, deepest first; then those that
    /// came, outermost first.
    fn set_hovered(&mut self, target: Option<AnyViewId>) {
        let now = target.map(|id| self.path(id)).unwrap_or_default();
        let before = std::mem::replace(&mut self.hovered, now.clone());
        for &id in before.iter().rev().filter(|id| !now.contains(id)) {
            self.call(id, |view, ctx| view.on_hover(ctx, false));
        }
        for &id in now.iter().filter(|id| !before.contains(id)) {
            self.call(id, |view, ctx| view.on_hover(ctx, true));
        }
    }

    /// What the pointer looks like at `point`: as the view that has
    /// captured it says, or else the view under it.
    pub fn cursor(&self, point: PointF) -> Cursor {
        let target = match self.captured() {
            Some(id) => self.to_local(id, point).map(|local| (id, local)),
            None => self.hit_test(point),
        };
        target
            .and_then(|(id, local)| Some(self.views[id.slot()].as_deref()?.cursor(local)))
            .unwrap_or_default()
    }

    /// The view that has the keys.
    pub fn focused(&self) -> Option<AnyViewId> {
        self.focused.filter(|&id| self.contains(id))
    }

    /// Gives the keys to `id` (`None`: to no view); the view that had them
    /// hears it first.
    pub fn set_focus(&mut self, id: Option<AnyViewId>) {
        let id = id.filter(|&id| self.contains(id));
        if id == self.focused {
            return;
        }
        if let Some(old) = std::mem::replace(&mut self.focused, id) {
            self.call(old, |view, ctx| view.on_focus(ctx, false));
        }
        if let Some(new) = id {
            self.call(new, |view, ctx| view.on_focus(ctx, true));
        }
    }

    /// A key, to the focused view, then up its ancestors until one handles
    /// it. Not handled if no view has the focus.
    pub fn key(&mut self, event: &KeyEvent) -> Handled {
        self.bubble_focused(|view, ctx| view.on_key(ctx, event))
    }

    /// The input method's, as a key goes.
    pub fn ime(&mut self, event: &ImeEvent) -> Handled {
        self.bubble_focused(|view, ctx| view.on_ime(ctx, event))
    }

    fn bubble_focused(
        &mut self,
        mut handle: impl FnMut(&mut dyn View<A>, &mut EventCtx<'_, A>) -> Handled,
    ) -> Handled {
        let mut at = self.focused();
        while let Some(id) = at {
            let handled = self
                .call(id, |view, ctx| handle(view, ctx))
                .unwrap_or(Handled::No);
            if handled.is_handled() {
                return Handled::Yes;
            }
            at = self.parent(id);
        }
        Handled::No
    }
}

fn node_id(id: AnyViewId) -> NodeId {
    NodeId::from(id.slot())
}

fn slot_of(id: NodeId) -> usize {
    usize::from(id)
}

/// The tree as taffy sees it while laying out: its nodes by slot, the views
/// (which measure the leaves) and what they share, borrowed apart.
struct LayoutPass<'a, A: App> {
    views: &'a mut [Option<Box<dyn View<A>>>],
    slots: &'a mut [Slot],
    shared: &'a mut Shared<A>,
}

impl<A: App> LayoutPass<'_, A> {
    fn node(&self, id: NodeId) -> &Node {
        self.slots[slot_of(id)]
            .node
            .as_ref()
            .expect("taffy lays out the tree's nodes")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[slot_of(id)]
            .node
            .as_mut()
            .expect("taffy lays out the tree's nodes")
    }

    fn compute(&mut self, id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let node = self.node(id);
        let display = node.style.display;
        let has_children = !node.children.is_empty();
        match display {
            Display::None => compute_hidden_layout(self, id),
            Display::Flex if has_children => compute_flexbox_layout(self, id, inputs),
            Display::Grid if has_children => compute_grid_layout(self, id, inputs),
            // Block containers aren't laid out yet: none of the views
            // needs one.
            _ => {
                debug_assert!(!has_children, "a view with children is a flex or grid container");
                let index = slot_of(id);
                let style = &self.slots[index]
                    .node
                    .as_ref()
                    .expect("taffy lays out the tree's nodes")
                    .style;
                let view = self.views[index].as_deref_mut();
                let shared = &mut *self.shared;
                compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
                    let Some(view) = view else {
                        return taffy::Size::ZERO;
                    };
                    let size = view.measure(&mut MeasureCtx { shared }, known, available);
                    taffy::Size {
                        width: size.width,
                        height: size.height,
                    }
                })
            }
        }
    }
}

impl<A: App> TraversePartialTree for LayoutPass<'_, A> {
    type ChildIter<'b>
        = std::iter::Map<std::slice::Iter<'b, AnyViewId>, fn(&AnyViewId) -> NodeId>
    where
        Self: 'b;

    fn child_ids(&self, parent: NodeId) -> Self::ChildIter<'_> {
        self.node(parent)
            .children
            .iter()
            .map((|id: &AnyViewId| node_id(*id)) as fn(&AnyViewId) -> NodeId)
    }

    fn child_count(&self, parent: NodeId) -> usize {
        self.node(parent).children.len()
    }

    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        node_id(self.node(parent).children[index])
    }
}

impl<A: App> LayoutPartialTree for LayoutPass<'_, A> {
    type CoreContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;
    type CustomIdent = String;

    fn get_core_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn set_unrounded_layout(&mut self, id: NodeId, layout: &Layout) {
        self.node_mut(id).layout = *layout;
    }

    fn compute_child_layout(&mut self, id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, id, inputs, |pass, id, inputs| pass.compute(id, inputs))
    }
}

impl<A: App> CacheTree for LayoutPass<'_, A> {
    fn cache_get(&mut self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.node_mut(id).cache.get(input)
    }

    fn cache_store(&mut self, id: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.node_mut(id).cache.store(input, output);
    }

    fn cache_clear(&mut self, id: NodeId) {
        self.node_mut(id).cache.clear();
    }
}

impl<A: App> LayoutFlexboxContainer for LayoutPass<'_, A> {
    type FlexboxContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;
    type FlexboxItemStyle<'b>
        = &'b Style
    where
        Self: 'b;

    fn get_flexbox_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn get_flexbox_child_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }
}

impl<A: App> LayoutGridContainer for LayoutPass<'_, A> {
    type GridContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;
    type GridItemStyle<'b>
        = &'b Style
    where
        Self: 'b;

    fn get_grid_container_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }

    fn get_grid_child_style(&self, id: NodeId) -> &Style {
        &self.node(id).style
    }
}
