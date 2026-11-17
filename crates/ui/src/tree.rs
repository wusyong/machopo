//! The tree of views: an arena of nodes, each a view and where it is. The
//! tree lays the views out (taffy, through its low-level traits, as the
//! `layout` crate does for boxes), paints them into display lists (one
//! layer, and one more for each view that scrolls its children), keeping
//! what each recorded until it asks to be painted again, hit tests and
//! dispatches the user's events.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use compositor_paint::DisplayItemList;
use gfx::{PointF, RectF, SizeF};
use taffy::{
    AvailableSpace, Cache, CacheTree, Display, Layout, LayoutFlexboxContainer, LayoutGridContainer,
    LayoutInput, LayoutOutput, LayoutPartialTree, Line, NodeId, RequestedAxis, RunMode, SizingMode,
    Style, TraversePartialTree, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout,
};

use crate::element::{AnyProps, Element, ElementKey};
use crate::event::{Clipboard, Cursor, Handled, ImeEvent, KeyEvent, PointerButton, PointerEvent};
use crate::id::{AnyViewId, ViewId};
use crate::text::TextContexts;
use crate::view::{
    App, Container, CreateCtx, EventCtx, MeasureCtx, PaintCtx, Requests, Shared, View,
};

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
    /// Its bounds as last laid out, in its parent's coordinates (unmoved
    /// by its parent's scroll offset).
    bounds: RectF,
    /// How far its children reach, as last laid out (the far edges of
    /// taffy's scrollable overflow).
    content_size: SizeF,
    /// It asked to be painted again (or its size changed).
    needs_paint: bool,
    /// What it recorded last, and where in the tree's coordinates it was
    /// put: what the next paint keeps, if it is still asked for.
    painted: Option<Painted>,
}

struct Painted {
    list: DisplayItemList,
    /// The layer it is in: `None` the tree's, else the one of the view that
    /// scrolls it.
    layer: Option<AnyViewId>,
    /// In that layer's coordinates.
    origin: PointF,
}

impl Painted {
    /// Where it may have drawn, in its layer's coordinates: every op's
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

/// A view the description made, as last described: what the next
/// description is compared with.
struct Mounted<A: App> {
    id: AnyViewId,
    key: Option<ElementKey>,
    name: Option<&'static str>,
    props: Box<dyn AnyProps<A>>,
    style: Style,
    children: Vec<Mounted<A>>,
}

/// What the tree drew: its layers, in paint order. The first is the
/// tree's: every view but those a [`View::scroll_offset`] scrolls, which
/// are in a layer of that view's, over it, and those in a view that
/// [`View::paints_to_layer`], with it in a layer of its own. A view painted
/// after one of those (a later sibling) is in the layer under that one's.
#[derive(Debug, Clone)]
pub struct Frame {
    pub layers: Vec<FrameLayer>,
}

#[derive(Debug, Clone)]
pub struct FrameLayer {
    /// Whose layer: `None` the tree's, else the view that scrolls it, or
    /// that paints to it (the same view's layer from one frame to the
    /// next).
    pub owner: Option<AnyViewId>,
    /// The layer's size, CSS px: the tree's, the content's of the view
    /// that scrolls it (as large as the view at least), or the view's that
    /// paints to it.
    pub size: SizeF,
    /// Where its top left is drawn, in the tree's coordinates: moved by
    /// its view's scroll offset.
    pub origin: PointF,
    /// What of it shows, in the tree's coordinates: the bounds of the view
    /// that scrolls it, within the layers' around it. `None`: all of it.
    pub clip: Option<RectF>,
    pub display_list: Arc<DisplayItemList>,
    /// What changed since the layer's last frame, in its coordinates:
    /// only these need rastering again. `None`: all of it (the first
    /// frame, or a new size).
    pub invalidation: Option<Arc<[RectF]>>,
}

/// A layer's place, as the last frame drew it: a new one, or one that
/// moved, is a change even if no view painted again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayerPlace {
    owner: Option<AnyViewId>,
    size: SizeF,
    origin: PointF,
    clip: Option<RectF>,
}

/// A layer being painted.
struct LayerBuild {
    place: LayerPlace,
    list: DisplayItemList,
    damage: Vec<RectF>,
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
    /// their layers' coordinates.
    damage: Vec<(Option<AnyViewId>, RectF)>,
    /// The layers the last frame drew.
    layers: Vec<LayerPlace>,
    /// The views under the pointer, from the root down.
    hovered: Vec<AnyViewId>,
    /// The view that handled the press of a button still down, which gets
    /// the pointer until it comes up.
    captured: Option<(AnyViewId, PointerButton)>,
    focused: Option<AnyViewId>,
    /// Every paint checks that no view's kept recording is stale (debug
    /// builds; see [`ViewTree::set_check_recordings`]).
    check_recordings: bool,
    /// The root's children as last described, and its style.
    described: Vec<Mounted<A>>,
    described_root_style: Option<Style>,
    /// The described views with names, by name.
    names: HashMap<&'static str, AnyViewId>,
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
            layers: Vec::new(),
            hovered: Vec::new(),
            captured: None,
            focused: None,
            check_recordings: cfg!(debug_assertions),
            described: Vec::new(),
            described_root_style: None,
            names: HashMap::new(),
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

    /// Whether every paint has each view record again and compares it with
    /// what it kept, panicking at a view that changed without asking to be
    /// painted again: on by default in debug builds, so that any test that
    /// paints checks it. Off for a test that counts paints.
    pub fn set_check_recordings(&mut self, check: bool) {
        self.check_recordings = check;
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
        ViewId::new(self.insert_boxed(Box::new(view)))
    }

    fn insert_boxed(&mut self, view: Box<dyn View<A>>) -> AnyViewId {
        let node = Node {
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            layout: Layout::default(),
            cache: Cache::new(),
            bounds: RectF::default(),
            content_size: SizeF::default(),
            needs_paint: true,
            painted: None,
        };
        match self.free.pop() {
            Some(index) => {
                let id = AnyViewId {
                    index,
                    generation: self.slots[index as usize].generation,
                };
                self.slots[id.slot()].node = Some(node);
                self.views[id.slot()] = Some(view);
                id
            }
            None => {
                let index =
                    u32::try_from(self.slots.len()).expect("fewer than u32::MAX views at once");
                self.slots.push(Slot {
                    generation: 0,
                    node: Some(node),
                });
                self.views.push(Some(view));
                AnyViewId {
                    index,
                    generation: 0,
                }
            }
        }
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
        assert!(
            self.contains(parent) && self.contains(child),
            "views of the tree"
        );
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
                self.damage.push((painted.layer, painted.drawn()));
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

    /// `id`'s bounds as last laid out, in the tree's coordinates: moved by
    /// the scroll offsets of the views around it.
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
                Some(parent) => {
                    if let Some(offset) = self.scroll_offset(parent) {
                        origin.x -= offset.x;
                        origin.y -= offset.y;
                    }
                    id = parent;
                }
                None if id == self.root => return Some(origin),
                // Not under the root: nowhere.
                None => return None,
            }
        }
    }

    /// Whether `id` paints to a layer of its own ([`View::paints_to_layer`]).
    fn paints_to_layer(&self, id: AnyViewId) -> bool {
        self.views
            .get(id.slot())
            .and_then(|view| view.as_deref())
            .is_some_and(|view| view.paints_to_layer())
    }

    /// Whether `id` is `ancestor` or under it.
    pub fn is_within(&self, id: impl Into<AnyViewId>, ancestor: impl Into<AnyViewId>) -> bool {
        let (id, ancestor) = (id.into(), ancestor.into());
        self.contains(id) && self.is_ancestor(ancestor, id)
    }

    /// How far `id` scrolls its children, if it does
    /// ([`View::scroll_offset`]).
    fn scroll_offset(&self, id: AnyViewId) -> Option<PointF> {
        self.views.get(id.slot())?.as_deref()?.scroll_offset()
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
        let node = self.node(id)?;
        let (size, content_size) = (node.bounds.size(), node.content_size);
        let focused = self.focused == Some(id);
        let view = self.views[id.slot()].as_deref_mut()?;
        let mut ctx = EventCtx {
            shared: &mut self.shared,
            size,
            content_size,
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

    // --- Description --------------------------------------------------------

    /// Makes the tree what `root` describes: `root` is the tree's root (a
    /// [`Container`]), its children the root's. The views described before
    /// are kept where the same type is at the same key (or place, among
    /// siblings without keys), given the new props only if they changed,
    /// moved if they moved; the others are made, or removed. The root's
    /// children are the description's: don't add views to the root by
    /// hand as well.
    pub fn describe(&mut self, root: Element<A>) {
        assert!(
            root.props.view_type() == std::any::TypeId::of::<Container>(),
            "the root is a container"
        );
        if self.described_root_style.as_ref() != Some(&root.style) {
            self.set_style(self.root, root.style.clone());
            self.described_root_style = Some(root.style);
        }
        let old = std::mem::take(&mut self.described);
        self.described = self.reconcile_children(self.root, old, root.children);
        self.names.clear();
        let mut stack: Vec<&Mounted<A>> = self.described.iter().collect();
        while let Some(mounted) = stack.pop() {
            if let Some(name) = mounted.name {
                let unique = self.names.insert(name, mounted.id).is_none();
                debug_assert!(unique, "two views named {name}");
            }
            stack.extend(&mounted.children);
        }
    }

    /// The described view named `name`, if it is a `T`.
    pub fn find<T: View<A>>(&self, name: &str) -> Option<ViewId<T>> {
        let id = *self.names.get(name)?;
        let view: &dyn Any = self.views[id.slot()].as_deref()?;
        view.is::<T>().then(|| ViewId::new(id))
    }

    /// `parent`'s children as `new` describes them, from `old`.
    fn reconcile_children(
        &mut self,
        parent: AnyViewId,
        old: Vec<Mounted<A>>,
        new: Vec<Element<A>>,
    ) -> Vec<Mounted<A>> {
        let mut keyed: HashMap<ElementKey, Mounted<A>> = HashMap::new();
        let mut unkeyed: VecDeque<Mounted<A>> = VecDeque::new();
        for mounted in old {
            match mounted.key.clone() {
                Some(key) => {
                    keyed.insert(key, mounted);
                }
                None => unkeyed.push_back(mounted),
            }
        }
        let mut out = Vec::with_capacity(new.len());
        for (index, element) in new.into_iter().enumerate() {
            let candidate = match &element.key {
                Some(key) => keyed.remove(key),
                None => unkeyed.pop_front(),
            };
            let mounted = match candidate {
                Some(mounted) if mounted.props.view_type() == element.props.view_type() => {
                    self.update_mounted(mounted, element)
                }
                Some(mounted) => {
                    self.remove(mounted.id);
                    self.mount(element)
                }
                None => self.mount(element),
            };
            if self.children(parent).get(index) != Some(&mounted.id) {
                self.insert_child(parent, index, mounted.id);
            }
            out.push(mounted);
        }
        for mounted in keyed.into_values().chain(unkeyed) {
            self.remove(mounted.id);
        }
        out
    }

    /// A new view for `element`, and for its children.
    fn mount(&mut self, element: Element<A>) -> Mounted<A> {
        let view = element.props.create(&mut CreateCtx {
            shared: &mut self.shared,
        });
        let id = self.insert_boxed(view);
        self.set_style(id, element.style.clone());
        let children = element
            .children
            .into_iter()
            .map(|child| {
                let child = self.mount(child);
                self.append_child(id, child.id);
                child
            })
            .collect();
        Mounted {
            id,
            key: element.key,
            name: element.name,
            props: element.props,
            style: element.style,
            children,
        }
    }

    /// `mounted`'s view as `element` describes it now.
    fn update_mounted(&mut self, mut mounted: Mounted<A>, element: Element<A>) -> Mounted<A> {
        let id = mounted.id;
        if element.style != mounted.style {
            self.set_style(id, element.style.clone());
            mounted.style = element.style;
        }
        if !element.props.same(&*mounted.props) {
            let props = &element.props;
            self.call(id, |view, ctx| props.update(view, ctx));
            mounted.props = element.props;
        }
        mounted.name = element.name;
        mounted.children = self.reconcile_children(id, mounted.children, element.children);
        mounted
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
        // it. One whose size changed paints again; one whose size or
        // content's changed hears it.
        let mut resized = Vec::new();
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
            // Its scrollable overflow's far edges (the content's border
            // boxes and the padding after them, from its top left).
            let overflow = node.layout.scrollable_overflow_rect.rect();
            let content_size = SizeF::new(overflow.right, overflow.bottom);
            if bounds.size() != node.bounds.size() {
                node.needs_paint = true;
            }
            if bounds.size() != node.bounds.size() || content_size != node.content_size {
                resized.push(id);
            }
            node.bounds = bounds;
            node.content_size = content_size;
            stack.extend(node.children.iter().copied());
        }
        for id in resized {
            self.call(id, |view, ctx| view.on_layout(ctx));
        }
    }

    // --- Painting -----------------------------------------------------------

    /// Lays out and paints what changed since the last frame; `None` if
    /// nothing did. Each view's recording is kept and drawn again where
    /// the view is now, until it asks to paint again.
    pub fn paint(&mut self) -> Option<Frame> {
        let frame = self.paint_changed();
        if self.check_recordings {
            let stale = self.stale_views();
            assert!(
                stale.is_empty(),
                "views changed without asking to be painted again: {stale:?}"
            );
        }
        frame
    }

    fn paint_changed(&mut self) -> Option<Frame> {
        self.layout();
        let full = std::mem::take(&mut self.needs_full_paint);
        let mut damage = std::mem::take(&mut self.damage);
        let mut changed = full || !damage.is_empty();
        let mut layers = vec![LayerBuild {
            place: LayerPlace {
                owner: None,
                size: self.size,
                origin: PointF::new(0.0, 0.0),
                clip: None,
            },
            list: DisplayItemList::new(),
            damage: Vec::new(),
        }];
        // A view, where its parent is in their layer, and that layer.
        let mut stack = vec![(self.root, PointF::new(0.0, 0.0), 0)];
        while let Some((id, parent_origin, layer)) = stack.pop() {
            let node = self.node(id).expect("a child of the tree");
            if node.style.display == Display::None {
                if node.painted.is_some() {
                    changed = true;
                    self.forget_painted(id);
                    damage.append(&mut self.damage);
                }
                continue;
            }
            let mut origin = PointF::new(
                parent_origin.x + node.bounds.x,
                parent_origin.y + node.bounds.y,
            );
            // A view that paints to a layer starts one, at its top left.
            let mut layer = layer;
            if self.paints_to_layer(id) {
                debug_assert!(self.scroll_offset(id).is_none(), "a layer, or a scroller");
                let around = layers[layer].place;
                let index = layers.len();
                layers.push(LayerBuild {
                    place: LayerPlace {
                        owner: Some(id),
                        size: node.bounds.size(),
                        origin: PointF::new(around.origin.x + origin.x, around.origin.y + origin.y),
                        clip: around.clip,
                    },
                    list: DisplayItemList::new(),
                    damage: Vec::new(),
                });
                layer = index;
                origin = PointF::new(0.0, 0.0);
            }
            let node = self.node(id).expect("a child of the tree");
            let owner = layers[layer].place.owner;
            let repaint = node.needs_paint || node.painted.is_none();
            if repaint {
                let painted = self.paint_view(id, owner, origin);
                let node = self.node_mut(id).expect("a child of the tree");
                if let Some(old) = node.painted.replace(painted) {
                    damage.push((old.layer, old.drawn()));
                }
                changed = true;
                damage.push((owner, node.painted.as_ref().expect("just painted").drawn()));
            } else {
                let node = self.node_mut(id).expect("a child of the tree");
                let painted = node.painted.as_mut().expect("painted before");
                if painted.origin != origin || painted.layer != owner {
                    // Moved: where it was and where it is.
                    damage.push((painted.layer, painted.drawn()));
                    painted.origin = origin;
                    painted.layer = owner;
                    damage.push((owner, painted.drawn()));
                    changed = true;
                }
            }
            let node = self.node(id).expect("a child of the tree");
            let painted = node.painted.as_ref().expect("painted");
            layers[layer]
                .list
                .push_list_at(&painted.list, origin.x, origin.y);
            // A view that scrolls has its children in a layer of its own,
            // over its parent's, cut to it and moved by its offset.
            let (child_origin, child_layer) = match self.scroll_offset(id) {
                Some(offset) => {
                    let around = layers[layer].place;
                    let at = RectF::new(
                        around.origin.x + origin.x,
                        around.origin.y + origin.y,
                        node.bounds.width,
                        node.bounds.height,
                    );
                    let clip = around.clip.map_or(at, |clip| clip.intersect(&at));
                    let content = node.content_size;
                    let index = layers.len();
                    layers.push(LayerBuild {
                        place: LayerPlace {
                            owner: Some(id),
                            size: SizeF::new(
                                content.width.max(at.width),
                                content.height.max(at.height),
                            ),
                            origin: PointF::new(at.x - offset.x, at.y - offset.y),
                            clip: Some(clip),
                        },
                        list: DisplayItemList::new(),
                        damage: Vec::new(),
                    });
                    (PointF::new(0.0, 0.0), index)
                }
                None => (origin, layer),
            };
            // Later children over earlier ones: pushed so the first pops
            // first.
            stack.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|&child| (child, child_origin, child_layer)),
            );
        }
        let places: Vec<LayerPlace> = layers.iter().map(|layer| layer.place).collect();
        if places != self.layers {
            changed = true;
        }
        if !changed {
            return None;
        }
        // Each layer's damage within it (an op that may draw anywhere
        // damages it all); a layer gone has none.
        for (owner, rect) in damage {
            if let Some(layer) = layers.iter_mut().find(|layer| layer.place.owner == owner) {
                let rect = rect.intersect(&RectF::from_size(layer.place.size));
                if !rect.is_empty() {
                    layer.damage.push(rect);
                }
            }
        }
        let before = std::mem::replace(&mut self.layers, places);
        let frame = Frame {
            layers: layers
                .into_iter()
                .map(|mut layer| {
                    layer.list.finalize();
                    layer.damage.dedup();
                    // Drawn whole: the tree's after a new size, or a layer
                    // new or of a new size.
                    let whole = match layer.place.owner {
                        None => full,
                        Some(_) => !before.iter().any(|old| {
                            old.owner == layer.place.owner && old.size == layer.place.size
                        }),
                    };
                    FrameLayer {
                        owner: layer.place.owner,
                        size: layer.place.size,
                        origin: layer.place.origin,
                        clip: layer.place.clip,
                        display_list: Arc::new(layer.list),
                        invalidation: (!whole).then(|| layer.damage.into()),
                    }
                })
                .collect(),
        };
        Some(frame)
    }

    /// Records what `id` draws, to be put at `origin` in `layer`.
    fn paint_view(&mut self, id: AnyViewId, layer: Option<AnyViewId>, origin: PointF) -> Painted {
        let list = self.record(id);
        let node = self.node_mut(id).expect("a child of the tree");
        node.needs_paint = false;
        Painted {
            list,
            layer,
            origin,
        }
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

    /// Every view of type `T` in the tree, in paint order (an earlier one
    /// under a later one).
    pub fn find_all<T: View<A>>(&self) -> Vec<ViewId<T>> {
        self.descendants(self.root)
            .into_iter()
            .filter(|&id| {
                self.views[id.slot()]
                    .as_deref()
                    .is_some_and(|view| (view as &dyn Any).is::<T>())
            })
            .map(ViewId::new)
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
        // Its children where it has scrolled them.
        let offset = self.scroll_offset(id).unwrap_or_default();
        let in_children = PointF::new(local.x + offset.x, local.y + offset.y);
        for &child in node.children.iter().rev() {
            if let Some(hit) = self.hit_test_in(child, in_children) {
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
        if let PointerEvent::Wheel { .. } = event {
            // To what is under the pointer, whatever has captured it.
            let handled = match hit {
                Some((target, _)) => self.bubble_pointer(target, event),
                None => Handled::No,
            };
            // What is under it may have scrolled.
            let hit = self.hit_test(event.position());
            self.set_hovered(hit.map(|(id, _)| id));
            return handled;
        }
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
        // Under a hidden view, every view is hidden too, whatever its own
        // display (as `TaffyTree` does).
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, id);
        }
        match display {
            Display::None => compute_hidden_layout(self, id),
            Display::Flex if has_children => compute_flexbox_layout(self, id, inputs),
            Display::Grid if has_children => compute_grid_layout(self, id, inputs),
            // Block containers aren't laid out yet: none of the views
            // needs one.
            _ => {
                debug_assert!(
                    !has_children,
                    "a view with children is a flex or grid container"
                );
                let index = slot_of(id);
                let style = &self.slots[index]
                    .node
                    .as_ref()
                    .expect("taffy lays out the tree's nodes")
                    .style;
                let view = self.views[index].as_deref_mut();
                let shared = &mut *self.shared;
                compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, available| {
                        let Some(view) = view else {
                            return taffy::Size::ZERO;
                        };
                        let size = view.measure(&mut MeasureCtx { shared }, known, available);
                        taffy::Size {
                            width: size.width,
                            height: size.height,
                        }
                    },
                )
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
        compute_cached_layout(self, id, inputs, |pass, id, inputs| {
            pass.compute(id, inputs)
        })
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
