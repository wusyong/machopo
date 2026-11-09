//! The documents the tests lay out: parsed into `dom`, styled by
//! `style` and built into a box tree with the test fonts, laid out at
//! 800×600. [`lay_out`] builds and lays out once; a [`Page`] is changed
//! and updated, each update checked against building it all again.

use std::collections::BTreeMap;
use std::sync::Arc;

use dom::{Cx, Dom, LayoutTreeFlags, LocalToken, NodeKind, NodeRef, NodeRoot, QualName};
use gfx::{NodeId, PointF, RectF, SizeF};
use layout::{BoxFragment, BoxTree, BoxTreeUpdate, LayoutUpdate, TextContext};
use style::StyleEngine;

pub const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

/// The URL a page's images resolve against, once [`Page::request_images`]
/// gave it a window.
pub const BASE_URL: &str = "https://example.test/";

pub fn r(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF::new(x, y, w, h)
}

/// A document, styled, and its box tree.
pub struct Page {
    // Before the DOM: a root mustn't outlive its heap.
    pub document: NodeRoot,
    pub dom: Dom,
    pub token: LocalToken,
    pub engine: StyleEngine,
    pub text: TextContext,
    pub tree: BoxTree,
    /// What [`check`](Self::check) lays out at.
    pub viewport: SizeF,
    /// The last check's fragments, and what laying them out did.
    view: Option<Arc<BoxFragment>>,
    pub layout: LayoutUpdate,
}

impl Page {
    /// `html`, parsed, styled and built, checked as [`update`](Self::update)
    /// checks it.
    pub fn new(html: &str) -> Self {
        let mut page = Self::build(html);
        page.check();
        page
    }

    /// `html`, parsed (each form control with its UA shadow tree, as a
    /// page has them), styled and built, not laid out.
    pub fn build(html: &str) -> Self {
        Self::build_with_text(html, TextContext::new(true))
    }

    /// [`build`](Self::build) with `text`'s fonts.
    pub fn build_with_text(html: &str, text: TextContext) -> Self {
        Self::styled(parse(html, true), text)
    }

    /// [`build`](Self::build), with the document changed by `prepare`
    /// before it is styled.
    pub fn build_with(
        html: &str,
        prepare: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> Self {
        let (mut dom, document) = parse(html, true);
        dom.mutate(|cx| prepare(cx, document.get(cx.gc)));
        Self::styled((dom, document), TextContext::new(true))
    }

    /// `html`, parsed, styled and built, without the form controls' UA
    /// shadow trees, for what follows them as they come.
    pub fn without_shadow_trees(html: &str) -> Self {
        let mut page = Self::styled(parse(html, false), TextContext::new(true));
        page.check();
        page
    }

    fn styled((mut dom, document): (Dom, NodeRoot), mut text: TextContext) -> Self {
        let (token, engine) = style(&mut dom, &document);
        let tree = dom.read(|cx| BoxTree::build(cx, &token, document.get(cx.gc), &mut text));
        Self {
            document,
            dom,
            token,
            engine,
            text,
            tree,
            viewport: VIEWPORT,
            view: None,
            layout: LayoutUpdate::default(),
        }
    }

    /// Reads the DOM: `read` is given the document.
    pub fn read<R>(&self, read: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>) -> R) -> R {
        let document = &self.document;
        self.dom.read(|cx| read(cx, document.get(cx.gc)))
    }

    /// Changes the DOM with `change` (given the document), styles it and
    /// updates the box tree, and returns what the update did, after
    /// checking it lays out as a new tree does.
    pub fn change(
        &mut self,
        change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> BoxTreeUpdate {
        let document = &self.document;
        self.dom.mutate(|cx| change(cx, document.get(cx.gc)));
        self.update()
    }

    pub fn update(&mut self) -> BoxTreeUpdate {
        let Self {
            dom,
            document,
            token,
            engine,
            text,
            tree,
            ..
        } = self;
        dom.mutate(|cx| {
            cx.update_use_trees();
            engine.recalc_style(cx, token, document.get(cx.gc));
        });
        let update = dom.read(|cx| tree.update(cx, token, document.get(cx.gc), text));
        self.check();
        update
    }

    /// No layout tree flag is left, and the tree lays out as one built
    /// from scratch, with what a selection goes over the same.
    pub fn check(&mut self) {
        let Self {
            dom,
            document,
            token,
            text,
            tree,
            viewport,
            ..
        } = self;
        let mut fresh = dom.read(|cx| {
            let document = document.get(cx.gc);
            // What layout walks: a node no slot takes keeps what it has.
            each_flat_node(cx, document, &mut |node| {
                assert_eq!(
                    cx.layout_tree_flags(node),
                    LayoutTreeFlags::CLEAN,
                    "#{} still flagged",
                    cx.node_id(node).get()
                );
            });
            BoxTree::build(cx, token, document, text)
        });
        let (got, want): (Vec<_>, Vec<_>) = dom.read(|cx| {
            let mut nodes = Vec::new();
            each_node(cx, document.get(cx.gc), &mut |node| {
                nodes.push(cx.node_id(node))
            });
            nodes
                .into_iter()
                .map(|node| {
                    (
                        (node, tree.is_rendered(node), tree.block_of(node)),
                        (node, fresh.is_rendered(node), fresh.block_of(node)),
                    )
                })
                .unzip()
        });
        // The block of each, by its element: the index of what a
        // selection goes over points at the boxes in the tree.
        assert_eq!(got, want, "rendered, and in the block of");
        assert_eq!(tree.texts_for_testing(), fresh.texts_for_testing());
        let (got, want) = (tree.layout(*viewport), fresh.layout(*viewport));
        assert!(
            got == want,
            "updated:\n{}\nbuilt:\n{}",
            got.dump(),
            want.dump()
        );
        self.layout = self.tree.last_layout();
        self.view = Some(got);
    }

    /// What the last check laid out.
    pub fn view(&self) -> Arc<BoxFragment> {
        self.view.clone().expect("laid out")
    }

    /// What the last check's layout laid out and made: boxes and fragments.
    pub fn laid_out(&self) -> (usize, usize) {
        (self.layout.laid_out, self.layout.fragments)
    }

    /// The tree laid out at the viewport, and its elements' boxes by `id`.
    pub fn lay_out(&mut self) -> Laid {
        let view = self.tree.layout(self.viewport);
        let layout = self.tree.last_layout();
        Laid::new(view, self.ids(), layout)
    }

    /// The id of the element whose `id` is `id`.
    pub fn id(&self, id: &str) -> NodeId {
        self.read(|cx, document| cx.node_id(find(cx, document, id)))
    }

    /// The node ids of the elements with an `id`, shadow trees' too (not
    /// the copies `<use>`s make), to their `id`s.
    pub fn ids(&self) -> BTreeMap<NodeId, String> {
        fn walk<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, out: &mut BTreeMap<NodeId, String>) {
            if cx.kind(node) == NodeKind::Element
                && let Some(id) = cx.get_attribute(node, "id")
            {
                out.insert(cx.node_id(node), id.to_string());
            }
            if let Some(shadow) = cx.shadow_root(node).filter(|_| !cx.is_svg_use(node)) {
                walk(cx, shadow, out);
            }
            let mut child = cx.first_child(node);
            while let Some(c) = child {
                walk(cx, c, out);
                child = cx.next_sibling(c);
            }
        }
        self.read(|cx, document| {
            let mut ids = BTreeMap::new();
            walk(cx, document, &mut ids);
            ids
        })
    }

    /// Each `<img>` whose `src` changed asks for its image, the document
    /// given a window and [`BASE_URL`] first (which fetching needs): the
    /// URLs asked for.
    pub fn request_images(&mut self) -> Vec<String> {
        let document = &self.document;
        self.dom.mutate(|cx| {
            let document = document.get(cx.gc);
            if cx.window_of(document).is_none() {
                cx.create_window(document);
                cx.set_document_url(document, BASE_URL);
            }
            for element in cx.take_image_updates() {
                cx.run_image_update(element, cx.image_src(element));
            }
        });
        self.dom
            .take_image_fetches()
            .into_iter()
            .map(|fetch| fetch.url)
            .collect()
    }

    /// The image at `url` (relative to [`BASE_URL`]) has come: `image`, or
    /// `None` for one that failed. Updates as [`change`](Self::change) does.
    pub fn image_fetched(&mut self, url: &str, image: Option<dom::Image>) -> BoxTreeUpdate {
        let url = format!("{BASE_URL}{url}");
        self.change(|cx, _| cx.image_fetched(&url, image, true))
    }
}

/// `html` parsed, each form control with its UA shadow tree if
/// `shadow_trees`.
pub fn parse(html: &str, shadow_trees: bool) -> (Dom, NodeRoot) {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(&mut dom, html, true)
        .root()
        .clone();
    if shadow_trees {
        dom.mutate(|cx| ensure_shadow_trees(cx, document.get(cx.gc)));
    }
    (dom, document)
}

/// Styles `document`.
pub fn style(dom: &mut Dom, document: &NodeRoot) -> (LocalToken, StyleEngine) {
    let mut token = LocalToken::new();
    let mut engine = StyleEngine::new();
    dom.mutate(|cx| {
        cx.update_use_trees();
        engine.recalc_style(cx, &mut token, document.get(cx.gc));
    });
    (token, engine)
}

/// `html`'s box tree (each form control with its UA shadow tree), built
/// with `text`, whose caches building fills.
pub fn build(html: &str, text: &mut TextContext) -> BoxTree {
    let (mut dom, document) = parse(html, true);
    let (token, _) = style(&mut dom, &document);
    dom.read(|cx| BoxTree::build(cx, &token, document.get(cx.gc), text))
}

/// A raster image of `width` × `height` (its pixels unread).
pub fn raster(width: u32, height: u32) -> dom::Image {
    dom::Image::Raster(
        gfx::ImageData::new(vec![0u8; 4]),
        gfx::Size { width, height },
    )
}

/// `html` laid out once, each form control with its UA shadow tree.
pub fn lay_out(html: &str) -> Laid {
    Page::build(html).lay_out()
}

/// A tree laid out: its fragments, and its elements' boxes by `id`.
pub struct Laid {
    pub view: Arc<BoxFragment>,
    /// The elements with an `id`, by node.
    pub ids: BTreeMap<NodeId, String>,
    /// Border boxes in LayoutView coordinates, by `id` attribute.
    pub rects: BTreeMap<String, RectF>,
    /// Fragments by `id` attribute.
    pub fragments: BTreeMap<String, Arc<BoxFragment>>,
    /// Where the fragments are, in LayoutView coordinates, by `id`
    /// attribute.
    pub origins: BTreeMap<String, PointF>,
    /// What laying it out did.
    pub layout: LayoutUpdate,
}

impl Laid {
    pub fn new(
        view: Arc<BoxFragment>,
        ids: BTreeMap<NodeId, String>,
        layout: LayoutUpdate,
    ) -> Self {
        let mut laid = Self {
            view: view.clone(),
            ids,
            rects: BTreeMap::new(),
            fragments: BTreeMap::new(),
            origins: BTreeMap::new(),
            layout,
        };
        laid.collect(&view, PointF::new(0.0, 0.0));
        laid
    }

    fn collect(&mut self, fragment: &Arc<BoxFragment>, origin: PointF) {
        // An element's own box, not its pseudo-elements', which have its
        // node too.
        if let Some(id) = fragment
            .node
            .filter(|_| fragment.pseudo.is_none())
            .and_then(|n| self.ids.get(&n))
            .cloned()
        {
            let size = fragment.size;
            self.rects.insert(
                id.clone(),
                RectF::new(origin.x, origin.y, size.width, size.height),
            );
            self.fragments.insert(id.clone(), fragment.clone());
            self.origins.insert(id, origin);
        }
        for child in &fragment.children {
            let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            self.collect(&child.fragment, at);
        }
    }

    pub fn rect(&self, id: &str) -> RectF {
        *self
            .rects
            .get(id)
            .unwrap_or_else(|| panic!("no box for #{id}:\n{}", self.view.dump()))
    }

    /// Where `id`'s border box starts, in LayoutView coordinates.
    pub fn origin(&self, id: &str) -> PointF {
        *self
            .origins
            .get(id)
            .unwrap_or_else(|| panic!("no box for #{id}:\n{}", self.view.dump()))
    }

    pub fn fragment(&self, id: &str) -> &Arc<BoxFragment> {
        self.fragments
            .get(id)
            .unwrap_or_else(|| panic!("no box for #{id}:\n{}", self.view.dump()))
    }

    /// `id`'s lines, in LayoutView coordinates: where each starts, and how
    /// wide its content is.
    pub fn lines(&self, id: &str) -> Vec<RectF> {
        let origin = self.origin(id);
        self.fragment(id)
            .lines
            .iter()
            .map(|line| line.rect.offset(origin.x, origin.y))
            .collect()
    }

    /// The ids of `id`'s children's fragments, in order; `-` for one
    /// without (an anonymous box).
    pub fn children(&self, id: &str) -> Vec<String> {
        self.fragment(id)
            .children
            .iter()
            .map(|child| {
                child
                    .fragment
                    .node
                    .and_then(|n| self.ids.get(&n).cloned())
                    .unwrap_or_else(|| "-".into())
            })
            .collect()
    }

    /// The node id of the element whose `id` is `id`.
    pub fn node(&self, id: &str) -> NodeId {
        self.ids
            .iter()
            .find(|(_, i)| *i == id)
            .map(|(node, _)| *node)
            .unwrap_or_else(|| panic!("no #{id}"))
    }
}

/// Whether `node` has a fragment in `fragment`'s tree, or text on a line.
pub fn has_fragment(fragment: &BoxFragment, node: NodeId) -> bool {
    fragment.node == Some(node)
        || fragment.lines.iter().any(|line| {
            line.runs.iter().any(|run| run.node == Some(node))
                || line.boxes.iter().any(|b| b.node == node)
        })
        || fragment
            .children
            .iter()
            .any(|child| has_fragment(&child.fragment, node))
}

/// The UA shadow tree of each control in `node`'s subtree, in a document
/// without a window.
pub fn ensure_shadow_trees<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) {
    cx.ensure_shadow_tree(node);
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        ensure_shadow_trees(cx, at);
        child = cx.next_sibling(at);
    }
}

/// `node` and its descendants in tree order, into the shadow trees in it
/// (each before the host's children).
pub fn each_node<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, f: &mut impl FnMut(NodeRef<'gc>)) {
    f(node);
    if let Some(shadow) = cx.shadow_root(node) {
        each_node(cx, shadow, f);
    }
    let mut child = cx.first_child(node);
    while let Some(c) = child {
        each_node(cx, c, f);
        child = cx.next_sibling(c);
    }
}

/// `node` and its descendants in the flat tree.
pub fn each_flat_node<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, f: &mut impl FnMut(NodeRef<'gc>)) {
    f(node);
    for child in cx.flat_tree_children(node) {
        each_flat_node(cx, child, f);
    }
}

/// The first element in tree order (shadow trees' too) whose `id` is `id`.
pub fn find<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, id: &str) -> NodeRef<'gc> {
    let mut found = None;
    each_node(cx, document, &mut |node| {
        if found.is_none()
            && cx.kind(node) == NodeKind::Element
            && cx
                .attribute_value_by_id(node, dom::names::ID)
                .is_some_and(|value| *value.borrow() == *id)
        {
            found = Some(node);
        }
    });
    found.unwrap_or_else(|| panic!("no #{id}"))
}

/// An open shadow root's init, its slots assigned as `slot_assignment`
/// says.
pub fn open(slot_assignment: dom::SlotAssignmentMode) -> dom::ShadowRootInit {
    dom::ShadowRootInit {
        mode: dom::ShadowRootMode::Open,
        delegates_focus: false,
        slot_assignment,
        clonable: false,
        serializable: false,
    }
}

/// Attaches an open shadow root (its slots named) to `host`, with what
/// `html` parses to as its children.
pub fn attach_shadow<'gc>(cx: &Cx<'_, 'gc>, host: NodeRef<'gc>, html: &str) -> NodeRef<'gc> {
    let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Named));
    let fragment = html::parser::parse_fragment_in(cx, host, html, false);
    cx.append_child(root, fragment).unwrap();
    root
}

/// An HTML element's name.
pub fn html(local: &str) -> QualName<'_> {
    QualName {
        namespace: Some("http://www.w3.org/1999/xhtml"),
        prefix: None,
        local,
    }
}

/// An SVG element's name.
pub fn svg(local: &str) -> QualName<'_> {
    QualName {
        namespace: Some("http://www.w3.org/2000/svg"),
        prefix: None,
        local,
    }
}
