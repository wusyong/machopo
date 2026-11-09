//! The box tree built from the DOM (`dom`): only what reads the DOM
//! is here (Blink's `LayoutTreeBuilder` and `LayoutObject::CreateObject`
//! read the DOM; layout reads the layout objects and their styles), the
//! rest is the tree's. The counters' and the lists' walks are
//! `counters` and `list_items`.
//!
//! The boxes are the flat tree's (`dom::Cx::flat_tree_children` and
//! `flat_tree_parent`: a host's shadow tree in place of its children, a
//! slot's assigned nodes or its fallback in it; a host's child no slot
//! takes has no boxes), as Blink's are; its dirty bits come up it.
//!
//! A form control's box is sized and placed by `control` (through
//! `ControlElement`), its content its UA shadow tree's. An `<img>` is
//! replaced, its image its element's current request's
//! (`Cx::image_state`); a `<canvas>` too, at its bitmap's size
//! (`Cx::canvas_size`), and an `<iframe>`, whose content is its frame's.
//! An outermost `<svg>` is replaced by its content, captured whole
//! (`svg::SvgSource`, through `SvgElement`); the SVG elements in it have no
//! boxes.

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use computed_style::{
    BorderCollapse, ComputedStyle, Content, ContentItem, Display, EffectiveAppearance, Float,
    ListStylePosition, ListStyleType, Overflow, PseudoId, TableDisplay,
};
use dom::{Cx, LayoutTreeFlags, LocalToken, NameId, NodeKind, NodeRef, QualifiedName, names};

use crate::control::{self, ControlElement};
use crate::fragment::ControlState;

use crate::replaced::{ImageState, Replaced};
use crate::svg_dom::SvgElement;
use crate::svg_gradient::Lookups;
use crate::text::{InlineItem, Owner};
use crate::{
    BoxRole, BoxTree, BoxTreeUpdate, Child, ContentContext, DocumentBoxes, LAYOUT_VIEW, LayoutBox,
    NodeId, NodeIdHashSet, StyleDifference, TextContext, applies_any_containment, block_item_style,
    box_style, contains_out_of_flow, counters, list_items, role_taffy_style, table,
    visible_overflow, webkit_box_item,
};

fn control<'a, 'gc>(dom: &'a Cx<'a, 'gc>, node: NodeRef<'gc>) -> ControlElement<'a, 'gc> {
    ControlElement::new(dom, node)
}

/// The states `element`'s box's native look shows, if its style gives it
/// one (Blink's `LayoutTheme::ControlStatesForNode`): checked,
/// indeterminate, disabled, read-only and focused as its control (itself,
/// or the host of the UA shadow tree it is a part of), hovered and pressed
/// as itself.
fn control_state<'gc>(
    dom: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    style: &ComputedStyle,
) -> Option<ControlState> {
    if style.effective_appearance == EffectiveAppearance::None {
        return None;
    }
    let host = control(dom, element)
        .user_agent_host()
        .map_or(element, |host| host.node);
    Some(ControlState {
        checked: dom.matches_checked(host),
        indeterminate: dom.matches_indeterminate(host),
        disabled: dom.is_disabled_form_control(host),
        read_only: !dom.matches_read_write(host),
        hovered: dom.is_hovered(element),
        pressed: dom.is_active(element),
        focused: dom.is_focused(host),
    })
}

/// Whether `element` is atomic even when its `display` is `inline`: a form
/// control, whose content is its own, or an `<img>`, `<canvas>` or
/// `<iframe>`, replaced elements.
fn is_atomic_when_inline<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    use names::local;
    if is_svg_root(dom, element) {
        return true;
    }
    [
        local::INPUT,
        local::SELECT,
        local::TEXTAREA,
        local::BUTTON,
        local::IMG,
        local::CANVAS,
        local::IFRAME,
    ]
    .into_iter()
    .any(|name| is_html(dom, element, name))
}

/// An `<img>`'s replaced content: its element's current request's image
/// (`Cx::image_state`), an SVG image the renderer's by its number; none
/// without one to show.
fn image_replaced<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Replaced {
    Replaced::new(dom.image_state(element).map(|state| match state {
        dom::ImageState::Loading => ImageState::Loading,
        dom::ImageState::Loaded(dom::Image::Raster(data, size)) => {
            ImageState::Loaded { data, size }
        }
        dom::ImageState::Loaded(dom::Image::Svg(image, natural)) => {
            ImageState::Svg { image, natural }
        }
        dom::ImageState::Failed => ImageState::Failed,
    }))
}

/// A replaced element: its box shows the image, canvas, frame or SVG,
/// not its children.
pub(crate) fn is_replaced<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    dom.is_img(element)
        || dom.is_canvas(element)
        || dom.is_iframe(element)
        || is_svg_root(dom, element)
}

/// Whether `element` is an outermost `<svg>`, a replaced element: one
/// whose parent isn't an SVG element, nor the root of a `<use>`'s copy.
fn is_svg_root<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    let is_svg = |node: NodeRef<'gc>| {
        dom.qualified_name(node)
            .is_some_and(|name| name.namespace() == names::SVG_NAMESPACE)
    };
    dom.qualified_name(element)
        .is_some_and(|name| name.is(names::SVG_NAMESPACE, names::local::SVG))
        && !dom.parent(element).is_some_and(is_svg)
        && dom.use_tree_host(element).is_none()
}

/// An outermost `<svg>`'s replaced content: its descendants captured
/// (`svg::SvgSource::capture`), and its natural size; with the ids the
/// capture looked up.
fn svg_replaced<'gc>(
    dom: Reader<'_, 'gc>,
    element: NodeRef<'gc>,
    text: &mut TextContext,
) -> (Replaced, Lookups) {
    let svg = SvgElement { dom, node: element };
    let (source, lookups) = crate::svg::SvgSource::capture_noting(svg, text);
    (
        Replaced::svg(source, crate::svg::svg_natural_size(svg)),
        lookups,
    )
}

/// `node` of the DOM as the SVG geometry queries read it
/// (`svg_geometry`, `svg_node`, `svg_shape_path`, `svg_text_chars`), its
/// style as last styled, read while `token` is.
pub fn svg_element<'c, 'gc>(
    dom: &'c Cx<'c, 'gc>,
    token: &'c LocalToken,
    node: NodeRef<'gc>,
) -> SvgElement<'c, 'gc> {
    SvgElement {
        dom: Reader { dom, token },
        node,
    }
}

/// The DOM as the box tree reads it: its nodes, and their styles read
/// while the token is.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'c, 'gc> {
    pub(crate) dom: &'c Cx<'c, 'gc>,
    pub(crate) token: &'c LocalToken,
}

impl<'gc> Reader<'_, 'gc> {
    pub(crate) fn style(&self, element: NodeRef<'gc>) -> Option<Arc<ComputedStyle>> {
        self.dom.computed_style(element, self.token).cloned()
    }

    pub(crate) fn pseudo_style(
        &self,
        element: NodeRef<'gc>,
        pseudo: PseudoId,
    ) -> Option<Arc<ComputedStyle>> {
        self.dom
            .element_style(element, self.token)?
            .pseudo
            .as_ref()?
            .get(pseudo)
            .cloned()
    }

    pub(crate) fn id(&self, node: NodeRef<'gc>) -> NodeId {
        self.dom.node_id(node)
    }

    pub(crate) fn children(&self, node: NodeRef<'gc>) -> impl Iterator<Item = NodeRef<'gc>> {
        children(self.dom, node)
    }

    pub(crate) fn is_element(&self, node: NodeRef<'gc>) -> bool {
        self.dom.kind(node) == NodeKind::Element
    }

    /// Whether `element` is an SVG element: only an outermost `<svg>` has a
    /// box.
    pub(crate) fn is_svg(&self, element: NodeRef<'gc>) -> bool {
        self.dom
            .qualified_name(element)
            .is_some_and(|name| name.namespace() == names::SVG_NAMESPACE)
    }

    /// Whether `element`, with style `style`, has boxes where its parent's
    /// content is: neither it nor its `display: contents` is `none`, and
    /// it isn't in an `<svg>` (whose content is its own).
    pub(crate) fn has_boxes(&self, element: NodeRef<'gc>, style: &ComputedStyle) -> bool {
        !(style.display.generates_no_box()
            || (style.display == Display::Contents && contents_is_none(self.dom, element))
            || (self.is_svg(element) && !is_svg_root(self.dom, element)))
    }
}

/// What building boxes from the DOM needs besides the tree.
struct BuildContext<'c, 'gc> {
    dom: Reader<'c, 'gc>,
    body: Option<NodeId>,
    propagated: Option<NodeId>,
    /// The elements an update builds again, with their subtrees.
    rebuild: &'c NodeIdHashSet,
    /// Inside an element built again: no box below is kept, as what
    /// changed there was not looked at.
    fresh: bool,
    content: ContentContext<'c>,
}

// The content's fields read as the build's own, and the build lent where
// content is given.
impl<'c> Deref for BuildContext<'c, '_> {
    type Target = ContentContext<'c>;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

impl DerefMut for BuildContext<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.content
    }
}

impl<'c, 'gc> BuildContext<'c, 'gc> {
    fn new(
        dom: Reader<'c, 'gc>,
        document: &DocumentBoxes,
        text: &'c mut TextContext,
        rebuild: &'c NodeIdHashSet,
    ) -> Self {
        Self {
            dom,
            body: document.body,
            propagated: document.propagated,
            rebuild,
            fresh: false,
            content: ContentContext {
                text,
                in_line_clamp: false,
                stats: BoxTreeUpdate::default(),
            },
        }
    }
}

/// Where an update found changes: the boxes whose content is collected
/// again, and the elements built again.
struct Walk<'c, 'gc> {
    dom: Reader<'c, 'gc>,
    /// What the `LayoutView` holds changed: everything is built again.
    full: bool,
    /// Boxes collecting their content again, with their elements, in tree
    /// order.
    dirty: Vec<(NodeRef<'gc>, usize)>,
    dirty_boxes: HashSet<usize>,
    rebuild: NodeIdHashSet,
    restyled: usize,
    /// The counters may have changed: an element that takes part in them
    /// came, or one's part in them changed.
    counters_changed: bool,
    /// Some element's children changed: one that took part in the
    /// counters may have gone.
    children_changed: bool,
    /// Elements built again, or whose part in the `list-item` counter
    /// changed: the lists they are in (and those in them) are numbered
    /// again.
    lists_changed: Vec<NodeRef<'gc>>,
    /// A pseudo-element uses `list-item` as the lists can't have it their
    /// own.
    irregular: bool,
    /// The elements in `<svg>`s that changed: what the boxes drawn with
    /// what an id found may draw otherwise for (`update_svg_clients`).
    svg_changed: Vec<NodeRef<'gc>>,
}

/// The box an element's content goes into: the `LayoutView`, or a block
/// box and its element.
#[derive(Clone, Copy)]
enum Container<'gc> {
    View,
    Box(NodeRef<'gc>, usize),
}

impl<'c, 'gc> Walk<'c, 'gc> {
    fn new(dom: Reader<'c, 'gc>) -> Self {
        Self {
            dom,
            full: false,
            dirty: Vec::new(),
            dirty_boxes: HashSet::new(),
            rebuild: NodeIdHashSet::default(),
            restyled: 0,
            counters_changed: false,
            children_changed: false,
            lists_changed: Vec::new(),
            irregular: false,
            svg_changed: Vec::new(),
        }
    }

    /// `container` collects its content again.
    fn collect(&mut self, container: Container<'gc>) {
        match container {
            Container::View => self.full = true,
            Container::Box(element, index) => {
                if self.dirty_boxes.insert(index) {
                    self.dirty.push((element, index));
                }
            }
        }
    }
}

/// `node`'s children in the flat tree (`dom`'s: a host's shadow
/// root's children, a slot's assigned nodes or its fallback), what the
/// boxes are built of.
fn children<'a, 'b, 'gc>(
    dom: &'a Cx<'b, 'gc>,
    node: NodeRef<'gc>,
) -> dom::FlatChildren<'a, 'b, 'gc> {
    dom.flat_tree_children(node)
}

fn root_element<'gc>(dom: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    children(dom, document).find(|&child| dom.kind(child) == NodeKind::Element)
}

fn is_html<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>, local: NameId) -> bool {
    dom.qualified_name(element) == Some(QualifiedName::html(local))
}

/// An element for which `display: contents` is `display: none`
/// (css-display 3, "Unusual Elements"): a replaced element or a form
/// control, whose children (if any) aren't its content; laid out as any
/// other element otherwise for now, but not their children.
fn contents_is_none<'gc>(dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    use names::local;
    [
        local::IMG,
        local::IFRAME,
        local::CANVAS,
        local::BR,
        local::WBR,
        local::METER,
        local::PROGRESS,
        local::EMBED,
        local::OBJECT,
        local::AUDIO,
        local::VIDEO,
        local::FRAME,
        local::FRAMESET,
        local::INPUT,
        local::TEXTAREA,
        local::SELECT,
    ]
    .into_iter()
    .any(|name| is_html(dom, element, name))
}

/// What the whole box tree depends on (`DocumentBoxes`): the root element,
/// `<body>`, and the element whose `overflow` goes to the viewport, with
/// what it gives the viewport.
fn document_boxes<'gc>(dom: Reader<'_, 'gc>, document: NodeRef<'gc>) -> DocumentBoxes {
    let cx = dom.dom;
    let style = |element| dom.style(element);
    let root = root_element(cx, document);
    let body = root
        .filter(|&root| is_html(cx, root, names::local::HTML))
        .and_then(|root| children(cx, root).find(|&child| is_html(cx, child, names::local::BODY)));
    // Containment on the root element or `<body>` keeps `<body>`'s to
    // itself (css-contain 2; Blink's `ShouldApplyAnyContainment`).
    let contained = [root, body]
        .into_iter()
        .flatten()
        .filter_map(style)
        .any(|style| applies_any_containment(&style));
    let source = match root.and_then(style) {
        Some(style) if style.clips_overflow() || contained => root,
        _ => body,
    };
    DocumentBoxes {
        root: root.map(|root| dom.id(root)),
        body: body.map(|body| dom.id(body)),
        propagated: source.map(|source| dom.id(source)),
        // `clip` on the viewport is `hidden` (css-overflow 3,
        // "overflow viewport propagation").
        viewport_overflow: source.and_then(style).map_or(
            (Overflow::Visible, Overflow::Visible),
            |style| {
                let viewport = |axis| match axis {
                    Overflow::Clip => Overflow::Hidden,
                    axis => axis,
                };
                (viewport(style.overflow_x), viewport(style.overflow_y))
            },
        ),
    }
}

/// Clears the layout tree flags of `node` and of everything flagged below
/// it, without looking at them: its boxes are built from scratch.
fn clear_layout_tree_flags<'gc>(dom: &Cx<'_, 'gc>, node: NodeRef<'gc>) {
    let flags = dom.layout_tree_flags(node);
    dom.clear_layout_tree_flags(node);
    if flags.contains(LayoutTreeFlags::CHILD_NEEDS_UPDATE) {
        children(dom, node).for_each(|child| clear_layout_tree_flags(dom, child));
    }
}

/// The document `node` is in: the root of its flat tree (through the
/// hosts of the shadow trees it is in).
fn tree_root<'gc>(dom: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> NodeRef<'gc> {
    let mut root = node;
    while let Some(parent) = dom.flat_tree_parent(root) {
        root = parent;
    }
    root
}

/// Whether node `id` is gone from `document`'s boxes: no longer there,
/// in another document, or in what `rebuild` builds again (which may
/// have taken it out of the tree, or put it under `display: none`).
fn gone<'gc>(
    dom: Reader<'_, 'gc>,
    document: NodeRef<'gc>,
    rebuild: &NodeIdHashSet,
    id: NodeId,
) -> bool {
    dom.dom.node_by_id(id).is_none_or(|element| {
        let mut node = Some(element);
        while let Some(n) = node {
            if rebuild.contains(&dom.id(n)) {
                return true;
            }
            if dom.dom.kind(n) == NodeKind::Document {
                return dom.id(n) != dom.id(document);
            }
            node = dom.dom.flat_tree_parent(n);
        }
        true
    })
}

/// Whether `element` or an element in it with boxes takes part in the
/// counters (`counters::is_relevant`; with `lists`, in those but
/// `list-item`).
fn counters_below<'gc>(dom: Reader<'_, 'gc>, element: NodeRef<'gc>, lists: bool) -> bool {
    if dom
        .style(element)
        .is_none_or(|style| style.display.generates_no_box())
    {
        return false;
    }
    counters::is_relevant(dom, element, lists)
        || dom
            .children(element)
            .any(|child| dom.is_element(child) && counters_below(dom, child, lists))
}

impl BoxTree {
    /// Builds the box tree from the computed styles style recalc stored on
    /// `document`'s elements, read while `token` is, giving each node with
    /// a box its id (`Cx::node_id`), shaping inline content with `text`,
    /// and clears the DOM's layout tree flags. An element without a style
    /// (style hasn't reached it) or with `display: none` generates no box,
    /// and neither do its descendants.
    ///
    /// The `overflow` of the root element, or of `<body>` if the root's is
    /// `visible` on both axes, goes to the viewport (css-overflow 3,
    /// "overflow viewport propagation"), and that element's own is then
    /// `visible`: it doesn't clip, and isn't a scroll container. With the
    /// root's not `visible`, `<body>` keeps its own and clips.
    pub fn build<'gc>(
        dom: &Cx<'_, 'gc>,
        token: &LocalToken,
        document: NodeRef<'gc>,
        text: &mut TextContext,
    ) -> Self {
        let (tree, stats) = Self::build_document(Reader { dom, token }, document, text);
        tracing::debug!(
            built = stats.built,
            shaped = stats.shaped,
            font_family_names = stats.font_family_names,
            "build box tree"
        );
        tree
    }

    /// Brings the tree up to date with the DOM after style recalc, going
    /// down the paths the DOM's layout tree flags mark and clearing them
    /// (Blink's `RebuildLayoutTree`):
    /// - an element whose style changed only in what is painted, or whose
    ///   box stays the same kind, takes the new style in place
    ///   (`LayoutObject::SetStyle`);
    /// - inserted or moved nodes, changed text and other `display`s are
    ///   built again: the block box they are in collects its content again
    ///   (shaping its text again), keeping the block boxes in it that
    ///   haven't changed, with their subtrees.
    ///
    /// Everything is built again if what the whole tree depends on
    /// changed: the root element, `<body>`, or what goes to the viewport.
    pub fn update<'gc>(
        &mut self,
        dom: &Cx<'_, 'gc>,
        token: &LocalToken,
        document: NodeRef<'gc>,
        text: &mut TextContext,
    ) -> BoxTreeUpdate {
        let _span = trace_event::trace_event!("StyleEngine::RebuildLayoutTree");
        let stats = self.update_document(Reader { dom, token }, document, text);
        tracing::debug!(
            full = stats.full,
            built = stats.built,
            shaped = stats.shaped,
            restyled = stats.restyled,
            font_family_names = stats.font_family_names,
            "update box tree"
        );
        stats
    }

    fn build_document<'gc>(
        dom: Reader<'_, 'gc>,
        document: NodeRef<'gc>,
        text: &mut TextContext,
    ) -> (Self, BoxTreeUpdate) {
        let _span = trace_event::trace_event!("Document::AttachLayoutTree");
        let mut tree = Self::empty(document_boxes(dom, document));
        let b = &mut tree;
        // Before the boxes: their text shows them. The lists number their
        // own unless something else uses theirs.
        let root = root_element(dom.dom, document);
        let found = counters::compute(dom, root, true);
        if found.2 {
            b.set_counters(dom, counters::compute(dom, root, false), false);
        } else {
            b.set_counters(dom, found, true);
        }
        let rebuild = NodeIdHashSet::default();
        let mut cx = BuildContext::new(dom, &b.document, text, &rebuild);
        let root = b.document.root.and(root);
        let root_box = root.and_then(|root| b.build_block(&mut cx, root, BoxRole::RootElement));
        if let Some(root_box) = root_box {
            b.boxes[LAYOUT_VIEW].children.push(root_box);
            b.boxes[usize::from(root_box)].parent = Some(LAYOUT_VIEW);
        }
        clear_layout_tree_flags(dom.dom, document);
        let stats = BoxTreeUpdate {
            full: true,
            ..cx.stats
        };
        #[cfg(debug_assertions)]
        b.check_counters(dom, document);
        (tree, stats)
    }
}

impl BoxTree {
    /// Becomes `tree`, built again whole for what the whole tree depends
    /// on (`DocumentBoxes`), noting what the styles that changed mean for
    /// scroll anchoring: each block box's style is compared with its new
    /// one. The anchors stay: Blink keeps the layout objects when, say,
    /// the viewport's `overflow` changes.
    fn replace_keeping_anchoring<'gc>(&mut self, dom: Reader<'_, 'gc>, tree: BoxTree) {
        let old = std::mem::replace(self, tree);
        for (&id, &index) in &old.elements {
            if let (Some(&new), Some(element)) = (self.elements.get(&id), dom.dom.node_by_id(id)) {
                let new = self.boxes[new].style.clone();
                self.note_scroll_anchoring_changes(dom, element, &old.boxes[index].style, &new);
            }
        }
    }

    fn update_document<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        document: NodeRef<'gc>,
        text: &mut TextContext,
    ) -> BoxTreeUpdate {
        if document_boxes(dom, document) != self.document {
            let (tree, stats) = BoxTree::build_document(dom, document, text);
            self.replace_keeping_anchoring(dom, tree);
            return stats;
        }
        let mut walk = Walk::new(dom);
        self.walk(&mut walk, document, Container::View);
        if walk.full {
            let (tree, stats) = BoxTree::build_document(dom, document, text);
            *self = tree;
            // What the `LayoutView` holds was built again: every box in
            // it is new, and no anchor stays.
            if let Some(root) = root_element(dom.dom, document) {
                self.scroll_anchoring.rebuilt.push(dom.id(root));
            }
            return stats;
        }
        self.update_counters(document, &mut walk);
        self.update_svg_clients(&mut walk, text);
        let document_boxes = DocumentBoxes {
            root: self.document.root,
            body: self.document.body,
            propagated: self.document.propagated,
            viewport_overflow: self.document.viewport_overflow,
        };
        let mut cx = BuildContext::new(dom, &document_boxes, text, &walk.rebuild);
        // Outer boxes first: one collected again keeps the inner ones,
        // which then collect their own.
        for &(element, index) in &walk.dirty {
            self.collect_again(&mut cx, element, index);
        }
        #[cfg(debug_assertions)]
        self.check_counters(dom, document);
        BoxTreeUpdate {
            restyled: walk.restyled,
            ..cx.stats
        }
    }

    /// Reads again the states `element`'s box's native look shows, and
    /// those of its UA shadow tree's parts (which show its states), making
    /// the fragments again of those whose changed (Blink's `LayoutTheme::
    /// ControlStateChanged`: painted again, not laid out): how many.
    fn update_control_states<'gc>(&mut self, dom: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> usize {
        let children =
            |node| std::iter::successors(dom.first_child(node), |child| dom.next_sibling(*child));
        let mut nodes = vec![element];
        if let Some(root) = dom.user_agent_shadow_root(element) {
            let mut stack: Vec<NodeRef<'gc>> = children(root).collect();
            while let Some(part) = stack.pop() {
                stack.extend(children(part));
                nodes.push(part);
            }
        }
        let mut changed = 0usize;
        for node in nodes {
            let Some(&index) = self.elements.get(&dom.node_id(node)) else {
                continue;
            };
            let state = control_state(dom, node, &self.boxes[index].style);
            if self.boxes[index].control_state != state {
                self.boxes[index].control_state = state;
                self.mark_needs_fragment(index);
                changed = changed.saturating_add(1);
            }
        }
        changed
    }

    /// What the boxes show of the counters is what the whole document's
    /// pass gives, the lists' numbers too (debug builds check it after
    /// every build and update).
    #[cfg(debug_assertions)]
    fn check_counters<'gc>(&self, dom: Reader<'_, 'gc>, document: NodeRef<'gc>) {
        if !self.counters.lists {
            return;
        }
        let root = root_element(dom.dom, document);
        let (texts, ..) = counters::compute(dom, root, false);
        let texts: rustc_hash::FxHashMap<(NodeId, PseudoId), Arc<str>> = texts
            .into_iter()
            .filter(|(.., text)| !text.is_empty())
            .map(|(element, pseudo, text)| ((dom.id(element), pseudo), text))
            .collect();
        for (&(node, pseudo), text) in &texts {
            if pseudo == PseudoId::Marker {
                if self.elements.contains_key(&node) {
                    assert_eq!(
                        self.marker_texts.get(&node),
                        Some(text),
                        "#{}'s marker",
                        node.get()
                    );
                }
            } else {
                assert_eq!(
                    self.counters.texts.get(&(node, pseudo)),
                    Some(text),
                    "#{}'s {pseudo:?}",
                    node.get()
                );
            }
        }
        for (&node, text) in &self.marker_texts {
            if self.elements.contains_key(&node)
                && dom.dom.node_by_id(node).is_some_and(|element| {
                    !gone(dom, document, &NodeIdHashSet::default(), dom.id(element))
                })
            {
                assert_eq!(
                    texts.get(&(node, PseudoId::Marker)),
                    Some(text),
                    "#{}'s marker",
                    node.get()
                );
            }
        }
    }

    /// Takes the counters a pass over the document found, with or without
    /// `list-item` (`lists`: the lists' own then).
    fn set_counters<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        (texts, relevant, _): counters::Found<'gc>,
        lists: bool,
    ) {
        let texts = texts
            .into_iter()
            .map(|(element, pseudo, text)| ((dom.id(element), pseudo), text));
        self.set_counter_texts(texts.collect(), relevant, lists);
    }

    /// Works the counters out again over the whole document (Blink's
    /// `StyleEngine::UpdateCounters`) if what the walk found may change
    /// them: an element that takes part in them came or changed its part
    /// (`Walk::counters_changed`), or one that did is gone, or in what was
    /// built again (taken out of the document, or put under `display:
    /// none`). The text of a pseudo-element that shows another value then
    /// is collected again with the content it is in, unless that content
    /// is built again anyway. While the lists number their own, only the
    /// lists the walk found changes in are numbered again, and only the
    /// markers that show another number are collected again; something
    /// using `list-item` otherwise has the whole document's pass number
    /// them from then on.
    fn update_counters<'gc>(&mut self, document: NodeRef<'gc>, walk: &mut Walk<'_, 'gc>) {
        let dom = walk.dom;
        if self.counters.lists && walk.irregular {
            self.count_lists_in_pass(document, walk);
            return;
        }
        let gone = |id: NodeId, rebuild: &NodeIdHashSet| gone(dom, document, rebuild, id);
        let structure = walk.children_changed || !walk.rebuild.is_empty();
        let pass = walk.counters_changed
            || (structure
                && self
                    .counters
                    .relevant
                    .keys()
                    .any(|&id| gone(id, &walk.rebuild)));
        if pass {
            self.counter_passes = self.counter_passes.wrapping_add(1);
            let root = root_element(dom.dom, document);
            let found = counters::compute(dom, root, self.counters.lists);
            if found.2 {
                self.count_lists_in_pass(document, walk);
                return;
            }
            let changed: Vec<NodeRef<'gc>> = found
                .0
                .iter()
                .filter(|(element, pseudo, text)| {
                    self.counters
                        .texts
                        .get(&(dom.id(*element), *pseudo))
                        .is_some_and(|old| old != text)
                })
                .map(|(element, ..)| *element)
                .collect();
            let lists = self.counters.lists;
            self.set_counters(dom, found, lists);
            for element in changed {
                if let Some(container) = self.container_of(dom, element, &walk.rebuild) {
                    walk.collect(container);
                }
            }
        }
        if !self.counters.lists {
            return;
        }
        // The lists numbered so far (those whose markers show numbers)
        // that a change may have reached: the one a change is in, those
        // built again, and those an element left.
        let mut changed_lists = NodeIdHashSet::default();
        for &element in &walk.lists_changed {
            if let Some(list) = list_items::enclosing_list(dom, element) {
                changed_lists.insert(dom.id(list));
            }
        }
        for list in self.lists.lists().collect::<Vec<_>>() {
            let reached = walk.lists_changed.iter().any(|&changed| {
                let changed = dom.id(changed);
                let mut node = dom.dom.node_by_id(list);
                while let Some(n) = node {
                    if dom.id(n) == changed {
                        return true;
                    }
                    node = dom.dom.flat_tree_parent(n);
                }
                false
            });
            if reached
                || (structure
                    && (gone(list, &walk.rebuild)
                        || self
                            .lists
                            .items_of(list)
                            .any(|item| gone(item, &walk.rebuild))))
            {
                changed_lists.insert(list);
            }
        }
        // Their markers that show another number now.
        let mut shown = Vec::new();
        for list in changed_lists {
            shown.extend(
                self.lists
                    .items_of(list)
                    .filter(|item| self.marker_texts.contains_key(item))
                    .collect::<Vec<_>>(),
            );
            self.lists.forget(list);
        }
        for item in shown {
            if gone(item, &walk.rebuild) {
                self.marker_texts.remove(&item);
                continue;
            }
            let Some(element) = dom.dom.node_by_id(item) else {
                continue;
            };
            let text = match self.marker_text(dom, element) {
                Ok(text) => text,
                Err(list_items::Irregular) => {
                    self.count_lists_in_pass(document, walk);
                    return;
                }
            };
            if self.marker_texts.get(&item).map(|text| &**text) != text.as_deref()
                && let Some(container) = self.container_of(dom, element, &walk.rebuild)
            {
                walk.collect(container);
            }
        }
    }

    /// Something uses `list-item` as the lists can't have it their own:
    /// from now on the whole document's pass numbers them too. The
    /// markers that show another text then are collected again.
    fn count_lists_in_pass<'gc>(&mut self, document: NodeRef<'gc>, walk: &mut Walk<'_, 'gc>) {
        let dom = walk.dom;
        self.counter_passes = self.counter_passes.wrapping_add(1);
        let root = root_element(dom.dom, document);
        let found = counters::compute(dom, root, false);
        let mut changed: Vec<NodeRef<'gc>> = found
            .0
            .iter()
            .filter(|(element, pseudo, text)| {
                let id = dom.id(*element);
                let old = if *pseudo == PseudoId::Marker {
                    self.marker_texts.get(&id)
                } else {
                    self.counters.texts.get(&(id, *pseudo))
                };
                old != Some(text)
            })
            .map(|(element, ..)| *element)
            .collect();
        // Markers that showed a number and show none now.
        changed.extend(self.marker_texts.keys().filter_map(|&item| {
            let shown = found.0.iter().any(|(element, pseudo, _)| {
                dom.id(*element) == item && *pseudo == PseudoId::Marker
            });
            (!shown).then(|| dom.dom.node_by_id(item)).flatten()
        }));
        self.set_counters(dom, found, false);
        for element in changed {
            if let Some(container) = self.container_of(dom, element, &walk.rebuild) {
                walk.collect(container);
            }
        }
    }

    /// The box whose content `element`'s pseudo-elements (or children, for
    /// a `display: contents` element) are in: its own, or the box of the
    /// nearest ancestor that has one. `None` if one of them is built
    /// again, which builds that content again too.
    fn container_of<'gc>(
        &self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
        rebuild: &NodeIdHashSet,
    ) -> Option<Container<'gc>> {
        let mut node = Some(element);
        while let Some(n) = node {
            let id = dom.id(n);
            if rebuild.contains(&id) {
                return None;
            }
            if let Some(&index) = self.elements.get(&id) {
                let mut above = dom.dom.flat_tree_parent(n);
                while let Some(a) = above {
                    if rebuild.contains(&dom.id(a)) {
                        return None;
                    }
                    above = dom.dom.flat_tree_parent(a);
                }
                return Some(Container::Box(n, index));
            }
            node = dom.dom.flat_tree_parent(n);
        }
        None
    }

    /// Looks at `node`'s flags and clears them, then goes down to its
    /// flagged descendants. `container` is the box `node`'s content is in.
    fn walk<'gc>(
        &mut self,
        walk: &mut Walk<'_, 'gc>,
        node: NodeRef<'gc>,
        container: Container<'gc>,
    ) {
        let dom = walk.dom;
        let flags = dom.dom.layout_tree_flags(node);
        dom.dom.clear_layout_tree_flags(node);
        walk.children_changed |= flags.contains(LayoutTreeFlags::CHILDREN_CHANGED);
        // Where its children's content goes; `None` if they have no boxes,
        // or theirs are built again with its.
        let inner = match dom.dom.kind(node) {
            // Its children are the root element, and doctypes and comments,
            // which have no boxes: a root that came or went is another
            // `DocumentBoxes::root`, which builds everything.
            NodeKind::Document => Some(container),
            NodeKind::Text => {
                if flags.contains(LayoutTreeFlags::NEEDS_REATTACH) {
                    walk.collect(container);
                }
                None
            }
            NodeKind::Element => self.walk_element(walk, node, flags, container),
            _ => None,
        };
        if !flags.contains(LayoutTreeFlags::CHILD_NEEDS_UPDATE) {
            return;
        }
        for child in dom.children(node) {
            match inner {
                Some(inner) => self.walk(walk, child, inner),
                None => clear_layout_tree_flags(dom.dom, child),
            }
        }
    }

    fn walk_element<'gc>(
        &mut self,
        walk: &mut Walk<'_, 'gc>,
        element: NodeRef<'gc>,
        flags: LayoutTreeFlags,
        container: Container<'gc>,
    ) -> Option<Container<'gc>> {
        let dom = walk.dom;
        // What is in an `<svg>` is its content, captured whole: a change to
        // any of it captures it again (`collect_again`).
        if dom.is_svg(element) && !is_svg_root(dom.dom, element) {
            let changed = [
                LayoutTreeFlags::NEEDS_REATTACH,
                LayoutTreeFlags::CHILDREN_CHANGED,
                LayoutTreeFlags::STYLE_CHANGED,
                LayoutTreeFlags::PSEUDO_STYLES_CHANGED,
            ];
            if changed.into_iter().any(|flag| flags.contains(flag)) {
                walk.collect(container);
                walk.svg_changed.push(element);
            }
            return Some(container);
        }
        let id = dom.id(element);
        let block = self.elements.get(&id).copied();
        let style_changed = flags.contains(LayoutTreeFlags::STYLE_CHANGED);
        let pseudo_styles_changed = flags.contains(LayoutTreeFlags::PSEUDO_STYLES_CHANGED);
        let reattach = flags.contains(LayoutTreeFlags::NEEDS_REATTACH)
            || (style_changed && !self.restyle(walk, element, block, container));
        // A state its native look shows changed, or its style, which may
        // have given it a native look or taken it: painted again.
        if (style_changed || flags.contains(LayoutTreeFlags::CONTROL_STATE_CHANGED)) && !reattach {
            let changed = self.update_control_states(dom.dom, element);
            walk.restyled = walk.restyled.wrapping_add(changed);
        }
        if reattach {
            walk.rebuild.insert(id);
            self.scroll_anchoring.rebuilt.push(id);
            walk.collect(container);
            // Built again: what takes part in the counters in it counts
            // again (what took part and no longer does, `update_counters`
            // finds).
            let lists = self.counters.lists;
            if !walk.counters_changed {
                walk.counters_changed = counters_below(dom, element, lists);
            }
            // What changes the numbers of a list it is in, or uses
            // `list-item` as the lists can't have it their own, its
            // numbering finds.
            if lists {
                walk.lists_changed.push(element);
            }
            return None;
        }
        // Its part in the counters, or its pseudo-elements', may be
        // another. (Its own part in `list-item`, `restyle` looks at.)
        if style_changed || pseudo_styles_changed {
            let lists = self.counters.lists;
            if !walk.counters_changed {
                walk.counters_changed = counters::directives(dom, element, lists).as_ref()
                    != self.counters.relevant.get(&id);
            }
            if lists
                && pseudo_styles_changed
                && let Some(style) = dom.style(element)
                && !list_items::regular(dom, element, &style)
            {
                walk.irregular = true;
            }
        }
        // An inline element's content, and a `display: contents` one's, is
        // in the box it is in.
        let inner = match block {
            Some(index) => Some(Container::Box(element, index)),
            None if dom.style(element).is_some_and(|style| {
                matches!(style.display, Display::Inline | Display::Contents)
            }) =>
            {
                Some(container)
            }
            None => None,
        };
        // Its pseudo-elements' boxes and text are in its content: collected
        // again when their styles changed (or they came or went).
        if (flags.contains(LayoutTreeFlags::CHILDREN_CHANGED) || pseudo_styles_changed)
            && let Some(inner) = inner
        {
            walk.collect(inner);
        }
        inner
    }

    /// Notes what `element`'s style going from `old` to `new` means for
    /// scroll anchoring: what isn't adjusted for, the element if it
    /// changed a property css-scroll-anchoring's suppression triggers name
    /// (Blink's `StyleDifference::disable_scroll_anchoring`), and its
    /// scroller (the scroll container around it, or the root element for
    /// the viewport) if it went in or out of flow (the spec's second
    /// trigger, Blink's `UpdateAfterReinsert`), wherever the anchor is in
    /// it; and whether its `overflow-anchor` changed.
    fn note_scroll_anchoring_changes<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
        old: &ComputedStyle,
        new: &ComputedStyle,
    ) {
        let id = dom.id(element);
        if crate::disables_scroll_anchoring(old, new) {
            self.scroll_anchoring.disabled.push(id);
        }
        if old.rare.overflow_anchor != new.rare.overflow_anchor {
            self.scroll_anchoring.overflow_anchor.push(id);
        }
        if old.is_out_of_flow() == new.is_out_of_flow() {
            return;
        }
        let mut node = dom.dom.flat_tree_parent(element);
        while let Some(n) = node {
            let parent = dom.dom.flat_tree_parent(n);
            // (A `<body>` whose `overflow` goes to the viewport is noted
            // for it: it is on the way up from any anchor there too.)
            let scrolls = dom
                .style(n)
                .is_some_and(|style| style.is_scroll_container());
            // The root element: the viewport's.
            if scrolls || parent.is_none_or(|p| !dom.is_element(p)) {
                self.scroll_anchoring.disabled.push(dom.id(n));
                return;
            }
            node = parent;
        }
    }

    /// Gives `element`'s boxes its new computed style (Blink's
    /// `LayoutObject::SetStyle`) where it can: `false` if they have to be
    /// built again. `block` is its block box, if it has one; otherwise it
    /// is inline content of `container`, or has no boxes.
    fn restyle<'gc>(
        &mut self,
        walk: &mut Walk<'_, 'gc>,
        element: NodeRef<'gc>,
        block: Option<usize>,
        container: Container<'gc>,
    ) -> bool {
        let dom = walk.dom;
        let id = dom.id(element);
        let style = dom
            .style(element)
            .expect("style recalc flagged it with its new style");
        let Some(index) = block else {
            // An inline element: its style is in the content of the box
            // it's in (a `<br>`'s in its newline's run). One that isn't
            // there had no boxes.
            // The root element without a box (`display: none`; it is
            // blockified otherwise): what the `LayoutView` holds changes.
            let Container::Box(_, index) = container else {
                return false;
            };
            let old = self
                .contents(index)
                .find_map(|i| self.boxes[i].inline.as_ref()?.style_of(id).cloned());
            // An anonymous table box in its content inherits from it: made
            // again with the content.
            let anonymous = self
                .contents(index)
                .any(|i| self.has_anonymous_table_box(i));
            if let Some(old) = &old
                && !list_items::same_part(old, &style)
            {
                walk.lists_changed.push(element);
            }
            if let Some(old) = &old {
                self.note_scroll_anchoring_changes(dom, element, old, &style);
            }
            match old.map(|old| StyleDifference::between(&old, &style)) {
                Some(StyleDifference::Reattach) | None => return false,
                Some(StyleDifference::Layout) => walk.collect(container),
                Some(StyleDifference::Paint) if anonymous => walk.collect(container),
                Some(StyleDifference::Paint) => {
                    for i in self.contents(index).collect::<Vec<_>>() {
                        if let Some(inline) = &mut self.boxes[i].inline {
                            inline.set_style(id, &style);
                            self.mark_needs_fragment(i);
                        }
                    }
                    walk.restyled = walk.restyled.wrapping_add(1);
                }
            }
            return true;
        };
        let style = if self.document.propagated == Some(id) {
            visible_overflow(&style)
        } else {
            style
        };
        if !list_items::same_part(&self.boxes[index].style, &style) {
            walk.lists_changed.push(element);
        }
        let old = self.boxes[index].style.clone();
        self.note_scroll_anchoring_changes(dom, element, &old, &style);
        let difference = match StyleDifference::between(&self.boxes[index].style, &style) {
            // The anonymous table boxes in it inherit from it: made again.
            StyleDifference::Paint if self.has_anonymous_table_box(index) => {
                StyleDifference::Layout
            }
            // A column has no fragment: its table's carries its style; so
            // does a collapsed table's for its parts' borders.
            StyleDifference::Paint
                if matches!(
                    table::TablePart::of(style.display),
                    Some(table::TablePart::ColumnGroup | table::TablePart::Column)
                ) || (table::TablePart::of(style.display).is_some()
                    && style.border_collapse == BorderCollapse::Collapse) =>
            {
                StyleDifference::Layout
            }
            difference => difference,
        };
        match difference {
            StyleDifference::Reattach => return false,
            // Its own size follows its font and its native look, and a
            // button's taffy style its container: built again.
            StyleDifference::Layout
                if self.boxes[index].control.is_some()
                    || self.boxes[index].button
                    || self.boxes[index].empty_line.is_some() =>
            {
                return false;
            }
            StyleDifference::Layout => {
                let container = self.boxes[index]
                    .parent
                    .map(|parent| self.boxes[parent].style.clone())
                    .filter(|parent| parent.display.is_flexible_or_grid());
                let calcs = &self.calcs;
                let b = &mut self.boxes[index];
                // A replaced box is sized from its content, not stretched
                // to its containing block as other blocks are.
                b.taffy_style = match &b.replaced {
                    Some(replaced) => {
                        crate::replaced_taffy_style(&style, replaced, container.as_deref(), calcs)
                    }
                    None => role_taffy_style(&style, b.role, calcs),
                };
                if b.webkit_box_item
                    && let Some(container) = &container
                {
                    webkit_box_item(&style, container, &mut b.taffy_style);
                }
                b.block_item_style = block_item_style(&style, &b.taffy_style);
                // Its table sets its collapsed borders again.
                b.collapsed_border = None;
                b.style = style;
                self.mark_needs_layout(index);
                // Its text and anonymous blocks inherit from it: they are
                // made, and shaped, again. Even with none, as `white-space`
                // may now keep text that collapsed away.
                walk.collect(Container::Box(element, index));
                walk.restyled = walk.restyled.wrapping_add(1);
            }
            StyleDifference::Paint => {
                let anonymous = Arc::new(ComputedStyle::anonymous_block(&style));
                for i in self.contents(index).collect::<Vec<_>>() {
                    self.mark_needs_fragment(i);
                    let b = &mut self.boxes[i];
                    if b.role == BoxRole::AnonymousBlock {
                        b.style = anonymous.clone();
                    }
                    if let Some(inline) = &mut b.inline {
                        inline.set_style(id, &style);
                    }
                }
                // A `transform` or a `filter` makes the box the containing
                // block of the out-of-flow boxes in it: one coming or going
                // lays it out again if there are any.
                let root = self.boxes[index].role == BoxRole::RootElement;
                let contains_changed = contains_out_of_flow(&self.boxes[index].style, root)
                    != contains_out_of_flow(&style, root);
                self.boxes[index].style = style;
                self.mark_needs_fragment(index);
                if contains_changed && self.has_out_of_flow_below(index) {
                    self.mark_needs_layout(index);
                }
                walk.restyled = walk.restyled.wrapping_add(1);
            }
        }
        true
    }

    /// `id`'s box draws with what `lookups` found (none: with nothing an
    /// id found).
    fn note_svg_client(&mut self, id: NodeId, lookups: Lookups) {
        if lookups.is_empty() {
            self.svg_clients.remove(&id);
        } else {
            self.svg_clients.insert(id, lookups);
        }
    }

    /// The boxes that draw with what an id found (`svg_clients`) and may
    /// draw otherwise now (Blink's `SVGResourceClient`s told their
    /// resource changed): an id finds another element, or what one found
    /// changed, or something in it, in this walk. An `<svg>`'s content is
    /// captured again; another box's `clip-path` found again. One whose
    /// element left is forgotten.
    fn update_svg_clients<'gc>(&mut self, walk: &mut Walk<'_, 'gc>, text: &mut TextContext) {
        if self.svg_clients.is_empty() || (walk.svg_changed.is_empty() && !walk.children_changed) {
            return;
        }
        let dom = walk.dom;
        // The elements that changed, and those they are in.
        let mut touched = NodeIdHashSet::default();
        for &element in &walk.svg_changed {
            let mut at = Some(element);
            while let Some(node) = at {
                if !touched.insert(dom.id(node)) {
                    break;
                }
                at = dom.dom.parent(node);
            }
        }
        let mut stale = Vec::new();
        self.svg_clients.retain(|&id, lookups| {
            let Some(element) = dom
                .dom
                .node_by_id(id)
                .filter(|element| dom.dom.is_connected(*element))
            else {
                return false;
            };
            // The tree the capture looked its ids up in.
            let mut scope = element;
            while let Some(parent) = dom.dom.parent(scope) {
                scope = parent;
            }
            if lookups.iter().any(|(looked_up, found)| {
                found.is_some_and(|found| touched.contains(&found))
                    || dom
                        .dom
                        .element_by_id(scope, looked_up)
                        .map(|element| dom.id(element))
                        != *found
            }) {
                stale.push(element);
            }
            true
        });
        for element in stale {
            let id = dom.id(element);
            let Some(&index) = self.elements.get(&id) else {
                continue;
            };
            if self.boxes[index]
                .replaced
                .as_ref()
                .is_some_and(|replaced| replaced.svg.is_some())
            {
                walk.collect(Container::Box(element, index));
            } else {
                let (clip, lookups) = crate::svg::clip_source_noting(
                    SvgElement { dom, node: element },
                    &self.boxes[index].style,
                    text,
                );
                self.boxes[index].clip_source = clip;
                self.note_svg_client(id, lookups);
                self.mark_needs_layout(index);
            }
        }
    }

    /// Collects box `index`'s content from `element` again: block boxes
    /// for its block children (the same ones where nothing changed) and
    /// its inline content, shaped again. The boxes it no longer holds are
    /// unused from now on.
    fn collect_again<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        element: NodeRef<'gc>,
        index: usize,
    ) {
        // A text area's children are its value, not its content.
        if self.boxes[index]
            .control
            .as_ref()
            .is_some_and(control::Control::ignores_children)
        {
            return;
        }
        // A replaced element's children are not rendered; an `<svg>`'s are
        // its content, taken whole again.
        if let Some(replaced) = &self.boxes[index].replaced {
            if replaced.svg.is_some() {
                let node = SvgElement {
                    dom: cx.dom,
                    node: element,
                };
                let (clip, mut lookups) =
                    crate::svg::clip_source_noting(node, &self.boxes[index].style, cx.text);
                let (replaced, content) = svg_replaced(cx.dom, element, cx.text);
                cx.stats.svg_captured = cx.stats.svg_captured.wrapping_add(1);
                lookups.extend(content);
                self.note_svg_client(cx.dom.id(element), lookups);
                self.boxes[index].clip_source = clip;
                let container = self.boxes[index]
                    .parent
                    .map(|parent| self.boxes[parent].style.clone())
                    .filter(|parent| parent.display.is_flexible_or_grid());
                let taffy_style = crate::replaced_taffy_style(
                    &self.boxes[index].style,
                    &replaced,
                    container.as_deref(),
                    &self.calcs,
                );
                let b = &mut self.boxes[index];
                b.taffy_style = taffy_style;
                if b.webkit_box_item
                    && let Some(container) = &container
                {
                    webkit_box_item(&b.style, container, &mut b.taffy_style);
                }
                b.replaced = Some(replaced);
                self.mark_needs_layout(index);
            }
            return;
        }
        let old = std::mem::take(&mut self.boxes[index].children);
        self.forget_texts(index);
        self.boxes[index].inline = None;
        let style = self.boxes[index].style.clone();
        // In a line clamp container if it or a box around it is one.
        let in_line_clamp = cx.in_line_clamp;
        let mut next = Some(index);
        while let Some(i) = next {
            cx.in_line_clamp |= self.boxes[i].style.line_clamp().is_some();
            next = self.boxes[i].parent;
        }
        let mut content = Vec::new();
        self.collect(cx, element, &style, &mut content);
        self.set_content(cx, index, content);
        cx.in_line_clamp = in_line_clamp;
        self.set_marker(cx, element, index);
        self.mark_needs_layout(index);
        let kept: HashSet<taffy::NodeId> = self.boxes[index].children.iter().copied().collect();
        for child in old {
            // Not one that went into an anonymous block made here (a float
            // or an out-of-flow box that comes after inline content now).
            if !kept.contains(&child) && self.boxes[usize::from(child)].parent == Some(index) {
                self.release(child.into());
            }
        }
    }

    /// Builds `element`'s box, block-level or an atomic inline, and the
    /// boxes in it.
    fn build_block<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        element: NodeRef<'gc>,
        role: BoxRole,
    ) -> Option<taffy::NodeId> {
        let mut style = cx.dom.style(element)?;
        let dom = cx.dom.dom;
        let atomic = style.display.is_atomic_inline()
            || (style.display == Display::Inline && is_atomic_when_inline(dom, element));
        if !style.display.is_block_level() && !atomic {
            return None;
        }
        let id = cx.dom.id(element);
        if cx.propagated == Some(id) {
            style = visible_overflow(&style);
        } else {
            style = box_style(control(dom, element), style);
        }
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let mut layout_box = LayoutBox::new(Some(id), role, style.clone(), &self.calcs);
        let (clip, mut lookups) = crate::svg::clip_source_noting(
            SvgElement {
                dom: cx.dom,
                node: element,
            },
            &style,
            cx.text,
        );
        layout_box.clip_source = clip;
        if is_replaced(dom, element) {
            let replaced = if dom.is_canvas(element) {
                Replaced::canvas(dom.canvas_size(element))
            } else if dom.is_iframe(element) {
                Replaced::iframe()
            } else if dom.is_img(element) {
                image_replaced(dom, element)
            } else {
                let (replaced, content) = svg_replaced(cx.dom, element, cx.text);
                lookups.extend(content);
                replaced
            };
            self.note_svg_client(id, lookups);
            // As a flex item, its parent's `set_content` makes it again.
            layout_box.taffy_style =
                crate::replaced_taffy_style(&style, &replaced, None, &self.calcs);
            layout_box.replaced = Some(replaced);
            let index = self.allocate(layout_box);
            self.elements.insert(id, index);
            return Some(index.into());
        }
        self.note_svg_client(id, lookups);
        layout_box.span = table::span(cx.dom.dom, element);
        layout_box.control_state = control_state(dom, element, &style);
        layout_box.slider_ratio = control::slider_ratio(control(dom, element));
        // Its taffy style sized as its parent's `set_content` makes it.
        let own = control::Control::of(control(dom, element), &style, cx.text);
        let ignores_children = own.as_ref().is_some_and(control::Control::ignores_children);
        // A list box keeps the room of its scrollbar (its `overflow-y` is
        // `scroll`): taffy's, which machopo draws nothing in.
        if own.as_ref().and_then(control::Control::list_box).is_some() {
            layout_box.taffy_style.overflow.y = taffy::Overflow::Scroll;
            layout_box.taffy_style.scrollbar_width = control::SCROLLBAR_THICKNESS;
        }
        layout_box.control = own;
        layout_box.button = control::is_button(control(dom, element));
        layout_box.list_box_row = control::is_list_box_row(control(dom, element));
        layout_box.empty_line = control::empty_line_height(control(dom, element), &style, cx.text);
        layout_box.placeholder = control::is_placeholder(control(dom, element));
        let index = self.allocate(layout_box);
        self.elements.insert(id, index);
        let fresh = cx.fresh;
        cx.fresh |= cx.rebuild.contains(&id);
        let in_line_clamp = cx.in_line_clamp;
        cx.in_line_clamp |= style.line_clamp().is_some();
        let mut content = Vec::new();
        if !ignores_children {
            self.collect(cx, element, &style, &mut content);
        }
        cx.fresh = fresh;
        self.set_content(cx, index, content);
        cx.in_line_clamp = in_line_clamp;
        self.set_marker(cx, element, index);
        Some(index.into())
    }

    /// Appends `parent`'s content to `out`: block boxes for block children,
    /// and inline children (recursively) as inline items: their text, with
    /// open and close tags around each inline element. A block child keeps
    /// its box unless it, or an element it is in, is built again.
    fn collect<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        parent: NodeRef<'gc>,
        parent_style: &Arc<ComputedStyle>,
        out: &mut Vec<Child>,
    ) {
        let dom = cx.dom;
        // A column has no content, and a column group's is its columns
        // (css-tables 3: nothing else in them is rendered).
        match parent_style.display {
            Display::Table(TableDisplay::Column) => return,
            Display::Table(TableDisplay::ColumnGroup) => {
                for child in dom.children(parent) {
                    let column = dom
                        .style(child)
                        .is_some_and(|s| s.display == Display::Table(TableDisplay::Column));
                    if column && let Some(id) = self.kept_or_built(cx, child) {
                        out.push(Child::Block(id));
                    }
                }
                return;
            }
            _ => {}
        }
        let parent_id = dom.id(parent);
        if parent_style.display == Display::ListItem
            && parent_style.list_style_position == ListStylePosition::Inside
            && let Some((style, text)) = self.marker_content(dom, parent)
        {
            let owner = Owner {
                node: parent_id,
                pseudo: Some(PseudoId::Marker),
            };
            out.push(Child::Inline(InlineItem::Open(owner, style.clone())));
            out.push(Child::Inline(InlineItem::Text(
                text,
                style.clone(),
                Some(owner),
                None,
            )));
            out.push(Child::Inline(InlineItem::Close(owner, style)));
        }
        self.collect_pseudo(cx, parent, PseudoId::Before, out);
        for child in dom.children(parent) {
            match dom.dom.kind(child) {
                NodeKind::Text => {
                    let data = dom
                        .dom
                        .text_data(child)
                        .map(|data| data.borrow().to_string())
                        .unwrap_or_default();
                    out.push(Child::Inline(InlineItem::Text(
                        data,
                        parent_style.clone(),
                        Some(Owner::element(parent_id)),
                        Some(dom.id(child)),
                    )));
                }
                NodeKind::Element => {
                    let Some(style) = dom.style(child) else {
                        continue;
                    };
                    // What is in an `<svg>` is its content, not boxes.
                    if dom.is_svg(child) && !is_svg_root(dom.dom, child) {
                        continue;
                    }
                    match style.display {
                        Display::None => {}
                        // A table's columns and column groups have boxes
                        // there, and nowhere else.
                        Display::Table(TableDisplay::Column | TableDisplay::ColumnGroup)
                            if table::TablePart::of(parent_style.display)
                                == Some(table::TablePart::Table) =>
                        {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Block(id));
                            }
                        }
                        display if display.generates_no_box() => {}
                        // No box of its own: its content is this box's,
                        // but the elements for which `contents` is `none`.
                        Display::Contents => {
                            if !contents_is_none(dom.dom, child) {
                                let fresh = cx.fresh;
                                cx.fresh |= cx.rebuild.contains(&dom.id(child));
                                self.collect(cx, child, &style, out);
                                cx.fresh = fresh;
                            }
                        }
                        // A child of this box, which works out its static
                        // position, but laid out by its containing block.
                        _ if style.is_out_of_flow() => {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Inline(InlineItem::OutOfFlow(id, style.clone())));
                            }
                        }
                        // Placed by the block formatting context it is in.
                        _ if style.float != Float::None => {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Inline(InlineItem::Float(id)));
                            }
                        }
                        Display::Block
                        | Display::FlowRoot
                        | Display::Flex
                        | Display::Grid
                        | Display::WebkitBox
                        | Display::Table(_)
                        | Display::ListItem => {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Block(id));
                            }
                        }
                        Display::InlineBlock
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::WebkitInlineBox
                        | Display::InlineTable => {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Inline(InlineItem::Atomic(id, style.clone())));
                            }
                        }
                        // A form control's box is atomic whatever its
                        // `display`.
                        Display::Inline if is_atomic_when_inline(dom.dom, child) => {
                            if let Some(id) = self.kept_or_built(cx, child) {
                                out.push(Child::Inline(InlineItem::Atomic(id, style.clone())));
                            }
                        }
                        Display::Inline => {
                            let id = dom.id(child);
                            if is_html(dom.dom, child, names::local::BR) {
                                out.push(Child::Inline(InlineItem::Break(style, id)));
                            } else {
                                out.push(Child::Inline(InlineItem::Open(
                                    Owner::element(id),
                                    style.clone(),
                                )));
                                let fresh = cx.fresh;
                                cx.fresh |= cx.rebuild.contains(&id);
                                self.collect(cx, child, &style, out);
                                cx.fresh = fresh;
                                out.push(Child::Inline(InlineItem::Close(
                                    Owner::element(id),
                                    style,
                                )));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.collect_pseudo(cx, parent, PseudoId::After, out);
    }

    /// Appends what `element`'s `pseudo` generates to `out`: its `content`
    /// as text, in an inline box, or in a box of its own as its `display`
    /// says.
    fn collect_pseudo<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        element: NodeRef<'gc>,
        pseudo: PseudoId,
        out: &mut Vec<Child>,
    ) {
        if !control::has_generated_children(control(cx.dom.dom, element)) {
            return;
        }
        let Some(style) = cx.dom.pseudo_style(element, pseudo) else {
            return;
        };
        let Content::Items(items) = &style.rare.content else {
            return;
        };
        let id = cx.dom.id(element);
        // With counters, the text their pass made.
        let text: String = match self.counters.texts.get(&(id, pseudo)) {
            Some(text) => text.to_string(),
            None => items
                .iter()
                .map(|item| match item {
                    ContentItem::String(string) => &**string,
                    ContentItem::Attr { value, .. } => &**value,
                    ContentItem::Counter { .. } | ContentItem::Counters { .. } => "",
                })
                .collect(),
        };
        let owner = Owner {
            node: id,
            pseudo: Some(pseudo),
        };
        let content = |style: &Arc<ComputedStyle>| {
            (!text.is_empty()).then(|| {
                Child::Inline(InlineItem::Text(
                    text.clone(),
                    style.clone(),
                    Some(owner),
                    None,
                ))
            })
        };
        if style.display.generates_no_box() {
            return;
        }
        // `display: contents`: its text, with no box around it.
        if style.display == Display::Contents {
            out.extend(content(&style));
            return;
        }
        if style.display == Display::Inline {
            out.push(Child::Inline(InlineItem::Open(owner, style.clone())));
            out.extend(content(&style));
            out.push(Child::Inline(InlineItem::Close(owner, style.clone())));
            return;
        }
        // A box of its own, with the text in it.
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let mut b = LayoutBox::new(Some(id), BoxRole::Other, style.clone(), &self.calcs);
        b.pseudo = Some(pseudo);
        let index = self.allocate(b);
        self.set_content(cx, index, content(&style).into_iter().collect());
        let box_id = taffy::NodeId::from(index);
        out.push(if style.is_out_of_flow() {
            Child::Inline(InlineItem::OutOfFlow(box_id, style.clone()))
        } else if style.float != Float::None {
            Child::Inline(InlineItem::Float(box_id))
        } else if style.display.is_atomic_inline() {
            Child::Inline(InlineItem::Atomic(box_id, style.clone()))
        } else {
            Child::Block(box_id)
        });
    }

    /// List item `element`'s marker's style and text, if it shows one
    /// (`marker_text`), kept in `marker_texts`.
    fn marker_content<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
    ) -> Option<(Arc<ComputedStyle>, String)> {
        let style = dom.pseudo_style(element, PseudoId::Marker)?;
        let id = dom.id(element);
        let text = match self.marker_text(dom, element) {
            Ok(text) => text,
            // Something uses `list-item` as the lists can't have it their
            // own: the whole document's pass numbers them from now on.
            Err(list_items::Irregular) => {
                let document = tree_root(dom.dom, element);
                let root = root_element(dom.dom, document);
                self.counter_passes = self.counter_passes.wrapping_add(1);
                self.set_counters(dom, counters::compute(dom, root, false), false);
                self.counters
                    .texts
                    .get(&(id, PseudoId::Marker))
                    .map(|text| text.to_string())
            }
        }
        .filter(|text| !text.is_empty());
        match &text {
            Some(text) => self.marker_texts.insert(id, text.as_str().into()),
            None => self.marker_texts.remove(&id),
        };
        Some((style, text?))
    }

    /// The text list item `element`'s marker shows: the lists' number in
    /// its `list-style-type`, or what the counters' pass made. `None` for
    /// none.
    fn marker_text<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
    ) -> Result<Option<String>, list_items::Irregular> {
        let Some(style) = dom.pseudo_style(element, PseudoId::Marker) else {
            return Ok(None);
        };
        let lists = self.counters.lists;
        match &style.rare.content {
            Content::Normal if lists => {}
            // Strings and attributes: no counter's value.
            Content::Items(items)
                if lists
                    && !items.iter().any(|item| {
                        matches!(
                            item,
                            ContentItem::Counter { .. } | ContentItem::Counters { .. }
                        )
                    }) =>
            {
                return Ok(Some(
                    items
                        .iter()
                        .map(|item| match item {
                            ContentItem::String(string) => &**string,
                            ContentItem::Attr { value, .. } => &**value,
                            ContentItem::Counter { .. } | ContentItem::Counters { .. } => "",
                        })
                        .collect(),
                ));
            }
            _ => {
                return Ok(self
                    .counters
                    .texts
                    .get(&(dom.id(element), PseudoId::Marker))
                    .map(|text| text.to_string()));
            }
        }
        // Only a number needs the list's.
        if matches!(
            style.rare_inherited.list_style_type,
            ListStyleType::None | ListStyleType::String(_)
        ) {
            return Ok(counters::marker_text(
                &style.rare_inherited.list_style_type,
                0,
            ));
        }
        let value = self.lists.value(dom, element)?;
        Ok(counters::marker_text(
            &style.rare_inherited.list_style_type,
            value,
        ))
    }

    /// Gives list item box `index` its outside marker, if it shows one: a
    /// box of its own with the marker's text. The one it had is unused
    /// from now on.
    fn set_marker<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        element: NodeRef<'gc>,
        index: usize,
    ) {
        if let Some(old) = self.boxes[index].marker.take() {
            self.release(old.into());
        }
        let style = &self.boxes[index].style;
        if style.display != Display::ListItem
            || style.list_style_position != ListStylePosition::Outside
        {
            return;
        }
        let Some((style, text)) = self.marker_content(cx.dom, element) else {
            return;
        };
        let id = cx.dom.id(element);
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let mut b = LayoutBox::new(Some(id), BoxRole::Other, style.clone(), &self.calcs);
        b.pseudo = Some(PseudoId::Marker);
        b.parent = Some(index);
        let marker = self.allocate(b);
        let owner = Owner {
            node: id,
            pseudo: Some(PseudoId::Marker),
        };
        let content = vec![Child::Inline(InlineItem::Text(
            text,
            style,
            Some(owner),
            None,
        ))];
        self.set_content(cx, marker, content);
        self.boxes[index].marker = Some(marker.into());
    }

    /// `element`'s box: the one it has, unless it or an element it is in
    /// is built again; otherwise a new one.
    fn kept_or_built<'gc>(
        &mut self,
        cx: &mut BuildContext<'_, 'gc>,
        element: NodeRef<'gc>,
    ) -> Option<taffy::NodeId> {
        let id = cx.dom.id(element);
        match self.elements.get(&id) {
            Some(&index) if !cx.fresh && !cx.rebuild.contains(&id) => Some(index.into()),
            _ => {
                let role = if Some(id) == cx.body {
                    BoxRole::Body
                } else {
                    BoxRole::Other
                };
                self.build_block(cx, element, role)
            }
        }
    }
}
