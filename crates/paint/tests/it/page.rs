//! The documents the tests paint, made as the renderer makes its frames':
//! parsed into a `dom::Dom`, styled (`style`) and built into a
//! box tree (`BoxTree::build`), then laid out.

use std::collections::HashMap;
use std::sync::Arc;

use dom::{
    Cx, Dom, ListenerFlags, LocalToken, NodeKind, NodeRef, NodeRoot, ShadowRootMode, TargetRef,
    names,
};
use gfx::{NodeId, PointF, SizeF};
use layout::{BoxFragment, BoxTree, BoxTreeUpdate, TextContext};
use paint::ChildFrame;
use style::StyleEngine;

/// The URL a document with images is at: one resolves `src` against it.
pub const URL: &str = "https://example.test/";

pub struct Page {
    // Before the DOM: a root mustn't outlive its heap.
    pub document: NodeRoot,
    /// The other documents parsed into the DOM (child frames'), in order.
    frames: Vec<NodeRoot>,
    pub dom: Dom,
    engine: StyleEngine,
    pub text: TextContext,
    pub tree: BoxTree,
}

impl Page {
    /// `html`, parsed, styled and built.
    pub fn new(html: &str) -> Self {
        Self::with(html, |_, _| {})
    }

    /// `html`, parsed, changed by `setup` (given the document), then
    /// styled and built.
    pub fn with(html: &str, setup: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>)) -> Self {
        Self::with_text(html, TextContext::new(true), setup)
    }

    /// [`Page::with`], its text shaped with `text`'s fonts.
    pub fn with_text(
        html: &str,
        mut text: TextContext,
        setup: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> Self {
        let mut dom = Dom::new();
        let document = html::parser::parse_document(&mut dom, html, true)
            .root()
            .clone();
        dom.mutate(|cx| setup(cx, document.get(cx.gc)));
        let mut engine = StyleEngine::new();
        let tree = style_and_build(&mut dom, &mut engine, &document, &mut text);
        Self {
            document,
            frames: Vec::new(),
            dom,
            engine,
            text,
            tree,
        }
    }

    /// `html` with each `<img>`'s image `image`: the document has a
    /// window at [`URL`], and every image it fetches comes.
    pub fn with_images(html: &str, image: &dom::Image) -> Self {
        let mut page = Self::with(html, give_window);
        let fetched = page.fetch_images();
        page.change(|cx, _| {
            for url in &fetched {
                cx.image_fetched(url, Some(image.clone()), true);
            }
        });
        page
    }

    /// Changes the DOM with `change` (given the document), then styles it
    /// and updates the box tree.
    pub fn change(
        &mut self,
        change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> BoxTreeUpdate {
        let document = &self.document;
        self.dom.mutate(|cx| change(cx, document.get(cx.gc)));
        self.update()
    }

    /// Styles the document and updates the box tree.
    pub fn update(&mut self) -> BoxTreeUpdate {
        let Self {
            dom,
            document,
            engine,
            text,
            tree,
            ..
        } = self;
        let mut token = LocalToken::new();
        style(dom, &mut token, engine, document);
        dom.read(|cx| tree.update(cx, &token, document.get(cx.gc), text))
    }

    /// The box tree laid out in `viewport`.
    pub fn lay_out(&mut self, viewport: SizeF) -> Arc<BoxFragment> {
        self.tree.layout(viewport)
    }

    /// Runs the `<img>`s' image updates that are due, and gives the URLs
    /// they fetch.
    pub fn fetch_images(&mut self) -> Vec<String> {
        self.dom.mutate(|cx| {
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

    /// `html` parsed into this page's DOM as another document (a child
    /// frame's), changed by `setup`, styled and built: its box tree, and
    /// its document.
    pub fn frame(
        &mut self,
        html: &str,
        setup: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> (BoxTree, NodeId) {
        let document = html::parser::parse_document(&mut self.dom, html, true)
            .root()
            .clone();
        self.dom.mutate(|cx| setup(cx, document.get(cx.gc)));
        // One a document, as the renderer's frames have.
        let mut engine = StyleEngine::new();
        let tree = style_and_build(&mut self.dom, &mut engine, &document, &mut self.text);
        let id = self.dom.read(|cx| cx.node_id(document.get(cx.gc)));
        self.frames.push(document);
        (tree, id)
    }

    /// The document's id.
    pub fn document_id(&self) -> NodeId {
        self.dom.read(|cx| cx.node_id(self.document.get(cx.gc)))
    }

    /// The id of the element whose `id` is `id`, in any of the documents.
    pub fn id(&self, id: &str) -> NodeId {
        *self.ids().get(id).unwrap_or_else(|| panic!("no #{id}"))
    }

    /// Each element with an `id`, in every document and the shadow trees
    /// attached in them: the last in tree order of those with one `id`.
    pub fn ids(&self) -> HashMap<String, NodeId> {
        self.dom.read(|cx| {
            let mut ids = HashMap::new();
            for document in std::iter::once(&self.document).chain(&self.frames) {
                each_node(cx, document.get(cx.gc), &mut |node| {
                    if cx.kind(node) == NodeKind::Element
                        && let Some(id) = cx.get_attribute(node, "id")
                    {
                        ids.insert(id.to_string(), cx.node_id(node));
                    }
                });
            }
            ids
        })
    }
}

/// Documents parsed into one DOM, as a renderer's frames are, each laid
/// out on its own: their elements' ids differ. Each element with a
/// `data-wheel` attribute gets a blocking `wheel` listener.
#[derive(Default)]
pub struct Frames {
    /// The first document's page, the others parsed into its DOM.
    page: Option<Page>,
    /// Each element with an `id`, in every document.
    pub ids: HashMap<String, NodeId>,
    /// Each node's parent, in every document.
    pub parents: HashMap<NodeId, NodeId>,
    /// Each layout's document, by its root fragment.
    documents: HashMap<*const BoxFragment, NodeId>,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    /// `html` laid out in a viewport of `viewport`.
    pub fn lay_out(&mut self, html: &str, viewport: SizeF) -> Arc<BoxFragment> {
        let (view, document) = match &mut self.page {
            None => {
                let page = self.page.insert(Page::with(html, listen_for_wheels));
                (page.lay_out(viewport), page.document_id())
            }
            Some(page) => {
                let (mut tree, document) = page.frame(html, listen_for_wheels);
                (tree.layout(viewport), document)
            }
        };
        let page = self.page.as_ref().expect("laid out");
        self.ids = page.ids();
        page.dom.read(|cx| {
            let node = cx.node_by_id(document).expect("rooted");
            each_node(cx, node, &mut |node| {
                let mut child = cx.first_child(node);
                while let Some(c) = child {
                    self.parents.insert(cx.node_id(c), cx.node_id(node));
                    child = cx.next_sibling(c);
                }
            });
        });
        self.documents.insert(Arc::as_ptr(&view), document);
        view
    }

    /// The document `view` is the layout of.
    pub fn document(&self, view: &Arc<BoxFragment>) -> NodeId {
        self.documents[&Arc::as_ptr(view)]
    }

    /// `view` as a child frame, scrolled to `viewport_scroll`.
    pub fn child_frame(&self, view: &Arc<BoxFragment>, viewport_scroll: PointF) -> ChildFrame {
        ChildFrame {
            layout_view: view.clone(),
            document: self.document(view),
            viewport_scroll,
            user_scrollable_x: true,
            user_scrollable_y: true,
            scroll: Default::default(),
            document_size: layout::document_size(view),
        }
    }

    pub fn id(&self, id: &str) -> NodeId {
        self.ids[id]
    }
}

/// `html` laid out in `viewport`, and each element with an `id`.
pub fn lay_out(html: &str, viewport: SizeF) -> (Arc<BoxFragment>, HashMap<String, NodeId>) {
    let mut page = Page::new(html);
    (page.lay_out(viewport), page.ids())
}

/// `document` styled and built. The thread's token is taken for it only:
/// a test may keep two pages.
fn style_and_build(
    dom: &mut Dom,
    engine: &mut StyleEngine,
    document: &NodeRoot,
    text: &mut TextContext,
) -> BoxTree {
    let mut token = LocalToken::new();
    style(dom, &mut token, engine, document);
    dom.read(|cx| BoxTree::build(cx, &token, document.get(cx.gc), text))
}

fn style(dom: &mut Dom, token: &mut LocalToken, engine: &mut StyleEngine, document: &NodeRoot) {
    dom.mutate(|cx| {
        cx.update_use_trees();
        engine.recalc_style(cx, token, document.get(cx.gc));
    });
}

/// `node` and its descendants in tree order, into the shadow trees
/// attached in it (each before the host's children); not into the UA's (a
/// control's parts, a `<use>`'s copy).
pub fn each_node<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, f: &mut impl FnMut(NodeRef<'gc>)) {
    f(node);
    if let Some(shadow) = cx.shadow_root(node)
        && cx
            .shadow_root_init(shadow)
            .is_some_and(|init| init.mode != ShadowRootMode::UserAgent)
    {
        each_node(cx, shadow, f);
    }
    let mut child = cx.first_child(node);
    while let Some(c) = child {
        each_node(cx, c, f);
        child = cx.next_sibling(c);
    }
}

/// The first element whose `id` is `id`, in the shadow trees attached in
/// `document` too.
pub fn find<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, id: &str) -> NodeRef<'gc> {
    let mut found = None;
    each_node(cx, document, &mut |node| {
        if found.is_none()
            && cx.kind(node) == NodeKind::Element
            && cx
                .attribute_value_by_id(node, names::ID)
                .is_some_and(|value| *value.borrow() == *id)
        {
            found = Some(node);
        }
    });
    found.unwrap_or_else(|| panic!("no #{id}"))
}

/// The UA shadow tree of each control in `node`'s subtree, as the
/// renderer makes them.
pub fn ensure_shadow_trees<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) {
    cx.ensure_shadow_tree(node);
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        ensure_shadow_trees(cx, at);
        child = cx.next_sibling(at);
    }
}

/// A blocking `wheel` listener on each element with a `data-wheel`
/// attribute.
pub fn listen_for_wheels<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) {
    let mut listened = Vec::new();
    each_node(cx, document, &mut |node| {
        if cx.kind(node) == NodeKind::Element && cx.get_attribute(node, "data-wheel").is_some() {
            listened.push(node);
        }
    });
    for element in listened {
        cx.add_listener(
            TargetRef::Node(element),
            names::event::WHEEL,
            ListenerFlags::default(),
        );
    }
}

/// A window for `document`, at [`URL`]: an `<img>` fetches its image only
/// in a document with one.
pub fn give_window<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) {
    cx.create_window(document);
    cx.set_document_url(document, URL);
}
