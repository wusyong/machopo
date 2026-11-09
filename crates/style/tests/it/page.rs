//! The tests' one harness: a document parsed into `dom`, styled by
//! a `StyleEngine`, its elements found by id (shadow trees and template
//! contents included) and their computed styles read with the token.

use std::collections::BTreeMap;
use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::{
    Cx, Dom, LocalToken, NodeKind, NodeRef, NodeRoot, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode, names,
};
use style::StyleEngine;

/// A parsed document, its engine and the thread's token.
pub struct Page {
    pub document: NodeRoot,
    pub engine: StyleEngine,
    pub token: LocalToken,
    /// What [`set_viewport`](Self::set_viewport) gave the engine, for the
    /// new one [`check`](Self::check) makes.
    viewport: (gfx::SizeF, f32),
    /// What [`set_url_context`](Self::set_url_context) gave the engine.
    url_context: Option<css::UrlContext>,
    /// Last: fields drop in order, and a root goes before its heap.
    pub dom: Dom,
}

impl Page {
    /// `html`, parsed and styled.
    pub fn new(html: &str) -> Self {
        let mut page = Self::parsed(html);
        page.recalc();
        page
    }

    /// `html`, parsed, not styled yet (for an engine set up first).
    pub fn parsed(html: &str) -> Self {
        let mut dom = Dom::new();
        let document = html::parser::parse_document(&mut dom, html, false)
            .root()
            .clone();
        Self {
            dom,
            document,
            engine: StyleEngine::new(),
            token: LocalToken::new(),
            viewport: (gfx::SizeF::default(), 1.0),
            url_context: None,
        }
    }

    /// The engine's viewport and device pixel ratio (`StyleEngine::
    /// set_viewport`): whether that makes the next recalc style everything
    /// again for them (not when an earlier change since the last recalc
    /// already did).
    pub fn set_viewport(&mut self, viewport: gfx::SizeF, device_pixel_ratio: f32) -> bool {
        self.viewport = (viewport, device_pixel_ratio);
        let before = self.engine.environment_changed_for_testing();
        self.engine.set_viewport(viewport, device_pixel_ratio);
        !before && self.engine.environment_changed_for_testing()
    }

    /// What the engine resolves the document's sheets' `url()`s against
    /// (`StyleEngine::set_url_context`).
    pub fn set_url_context(&mut self, context: Option<css::UrlContext>) {
        self.url_context.clone_from(&context);
        self.engine.set_url_context(context);
    }

    /// `html`, parsed, each form control given its UA shadow tree and each
    /// `<use>` its copy (what the renderer makes before style), styled.
    pub fn with_ua_trees(html: &str) -> Self {
        let mut page = Self::parsed(html);
        page.ensure_ua_trees();
        page.recalc();
        page
    }

    /// `html`, parsed, then each `(host, markup)`'s host given an open
    /// shadow root of `markup` (its `<style>`s too), styled.
    pub fn with_shadows(html: &str, shadows: &[(&str, &str)]) -> Self {
        let mut page = Self::parsed(html);
        for (host, markup) in shadows {
            page.with(host, |cx, host| {
                attach_shadow(cx, host, markup);
            });
        }
        page.recalc();
        page
    }

    pub fn recalc(&mut self) {
        let Self {
            dom,
            document,
            engine,
            token,
            ..
        } = self;
        dom.mutate(|cx| engine.recalc_style(cx, token, document.get(cx.gc)));
    }

    /// The UA shadow tree of each control in the document, and the copy of
    /// each `<use>`'s referenced element, made where they are missing.
    pub fn ensure_ua_trees(&mut self) {
        let document = &self.document;
        self.dom.mutate(|cx| {
            ensure_shadow_trees(cx, document.get(cx.gc));
            cx.update_use_trees();
        });
    }

    /// The computed style of the element whose `id` is `id`.
    pub fn style(&mut self, id: &str) -> Arc<ComputedStyle> {
        self.try_style(id)
            .unwrap_or_else(|| panic!("#{id} has no style"))
    }

    /// The computed style of the element whose `id` is `id`, if styled.
    pub fn try_style(&mut self, id: &str) -> Option<Arc<ComputedStyle>> {
        let Self {
            dom,
            document,
            token,
            ..
        } = self;
        dom.read(|cx| {
            let element = find(cx, document.get(cx.gc), id).unwrap_or_else(|| panic!("no #{id}"));
            cx.computed_style(element, token).cloned()
        })
    }

    /// The pseudo-elements' styles of the element whose `id` is `id`.
    pub fn pseudo(&mut self, id: &str) -> Option<Box<computed_style::PseudoStyles>> {
        let Self {
            dom,
            document,
            token,
            ..
        } = self;
        dom.read(|cx| {
            let element = find(cx, document.get(cx.gc), id).expect("an element with the id");
            cx.element_style(element, token)?.pseudo.clone()
        })
    }

    /// `f` in an operation, with the element whose `id` is `id`.
    pub fn with<R>(
        &mut self,
        id: &str,
        f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>) -> R,
    ) -> R {
        let document = &self.document;
        self.dom.mutate(|cx| {
            let element = find(cx, document.get(cx.gc), id).unwrap_or_else(|| panic!("no #{id}"));
            f(cx, element)
        })
    }

    /// `f` in an operation, with the document.
    pub fn with_document<R>(
        &mut self,
        f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>) -> R,
    ) -> R {
        let document = &self.document;
        self.dom.mutate(|cx| f(cx, document.get(cx.gc)))
    }

    /// Each element with an id and a style, shadow trees and template
    /// contents included.
    pub fn styles(&mut self) -> BTreeMap<String, Arc<ComputedStyle>> {
        self.all_styles()
            .into_iter()
            .filter_map(|(id, style)| Some((id, style?)))
            .collect()
    }

    /// Each element with an id, and its style if it has one.
    pub fn all_styles(&mut self) -> BTreeMap<String, Option<Arc<ComputedStyle>>> {
        let Self {
            dom,
            document,
            token,
            ..
        } = self;
        dom.read(|cx| {
            let mut out = BTreeMap::new();
            each_element(cx, document.get(cx.gc), &mut |element| {
                if let Some(id) = id_of(cx, element) {
                    out.insert(id, cx.computed_style(element, token).cloned());
                }
            });
            out
        })
    }

    /// The ids of the elements the last recalc styled again (`""` for one
    /// without), sorted.
    pub fn restyled(&mut self) -> Vec<String> {
        let Self { dom, engine, .. } = self;
        let mut ids: Vec<String> = dom.read(|cx| {
            engine
                .restyled_for_testing()
                .iter()
                .filter_map(|&id| cx.node_by_id(id))
                .map(|element| id_of(cx, element).unwrap_or_default())
                .collect()
        });
        ids.sort();
        ids
    }

    /// Changes the document with `change`, styles it again, and returns the
    /// ids of the elements styled again (cascaded, or given another style),
    /// sorted, after checking their styles are what styling everything
    /// again with a new engine gives.
    pub fn change(
        &mut self,
        change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> Vec<String> {
        self.change_with(change).1
    }

    /// [`change`](Self::change)s the element whose `id` is `id` with `f`.
    pub fn change_at(
        &mut self,
        id: &str,
        f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
    ) -> Vec<String> {
        self.change(|cx, document| {
            let element = find(cx, document, id).unwrap_or_else(|| panic!("no #{id}"));
            f(cx, element);
        })
    }

    /// The element whose `id` is `id`, rooted: to change in a later
    /// operation (`root.get(cx.gc)`).
    pub fn root(&mut self, id: &str) -> NodeRoot {
        self.with(id, |cx, element| elise::Persistent::new(cx.gc, element))
    }

    /// [`change`](Self::change), with what `change` returns.
    pub fn change_with<R>(
        &mut self,
        change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>) -> R,
    ) -> (R, Vec<String>) {
        let before = self.styles();
        let out = self.with_document(change);
        self.recalc();
        let after = self.styles();
        let cascaded = self.restyled();
        self.check();
        let restyled = after
            .iter()
            .filter(|(id, style)| {
                cascaded.contains(id) || !before.get(*id).is_some_and(|old| Arc::ptr_eq(old, style))
            })
            .map(|(id, _)| id.clone())
            .collect();
        (out, restyled)
    }

    /// That each element's style is what styling the document again with
    /// a new engine (no sheets parsed, nothing cached: everything styled,
    /// every shadow tree's sheets collected) gives. The new engine's styles
    /// stay.
    pub fn check(&mut self) {
        let before = self.styles();
        let mut engine = StyleEngine::new();
        engine.set_viewport(self.viewport.0, self.viewport.1);
        engine.set_system_colors(self.engine.system_colors());
        engine.set_url_context(self.url_context.clone());
        let Self {
            dom,
            document,
            token,
            ..
        } = self;
        dom.mutate(|cx| {
            let document = document.get(cx.gc);
            each_shadow_root(cx, document, &mut |root| {
                cx.mark_shadow_sheets_changed(root)
            });
            engine.recalc_style(cx, token, document);
        });
        let all = self.styles();
        for (id, style) in &before {
            assert_eq!(**style, *all[id], "#{id}: what styling everything gives");
        }
    }
}

/// `change` made, styled: [`Page::restyled`], the ids of the elements
/// styled again (without [`Page::change`]'s check).
pub fn restyled(
    page: &mut Page,
    change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
) -> Vec<String> {
    page.with_document(change);
    page.recalc();
    page.restyled()
}

/// `element` focused in `document` (none if `None`), showing a ring if
/// `visible`; its page having the system's focus if `page_focused` (a
/// window made for the document if it has none: `:focus` matches in a
/// focused page alone).
pub fn focus<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    element: Option<NodeRef<'gc>>,
    page_focused: bool,
    visible: bool,
) {
    let window = cx
        .window_of(document)
        .unwrap_or_else(|| cx.create_window(document));
    cx.set_page_focused(window, page_focused);
    cx.set_focused_element(document, element, visible);
}

/// The value of `element`'s `id`, if it has one.
pub fn id_of<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<String> {
    cx.attribute_value_by_id(element, names::ID)
        .map(|id| id.borrow().to_str().into_owned())
}

/// Each element in `root`'s subtree, in tree order, a host's shadow tree
/// before its children, a template's contents after them.
pub fn each_element<'gc>(cx: &Cx<'_, 'gc>, root: NodeRef<'gc>, f: &mut impl FnMut(NodeRef<'gc>)) {
    if cx.kind(root) == NodeKind::Element {
        f(root);
    }
    if let Some(shadow) = cx.shadow_root(root) {
        each_element(cx, shadow, f);
    }
    let mut child = cx.first_child(root);
    while let Some(node) = child {
        each_element(cx, node, f);
        child = cx.next_sibling(node);
    }
    if let Some(content) = cx.template_content(root) {
        each_element(cx, content, f);
    }
}

/// Each shadow root in `root`'s shadow-including subtree.
fn each_shadow_root<'gc>(cx: &Cx<'_, 'gc>, root: NodeRef<'gc>, f: &mut impl FnMut(NodeRef<'gc>)) {
    if let Some(shadow) = cx.shadow_root(root) {
        f(shadow);
        each_shadow_root(cx, shadow, f);
    }
    let mut child = cx.first_child(root);
    while let Some(node) = child {
        each_shadow_root(cx, node, f);
        child = cx.next_sibling(node);
    }
}

/// The first element in `root`'s subtree, in tree order, whose `id` is
/// `id` (not in shadow trees).
pub fn by_id<'gc>(cx: &Cx<'_, 'gc>, root: NodeRef<'gc>, id: &str) -> Option<NodeRef<'gc>> {
    if cx.kind(root) == NodeKind::Element
        && cx
            .attribute_value_by_id(root, names::ID)
            .is_some_and(|value| *value.borrow() == *id)
    {
        return Some(root);
    }
    let mut child = cx.first_child(root);
    while let Some(node) = child {
        if let Some(found) = by_id(cx, node, id) {
            return Some(found);
        }
        child = cx.next_sibling(node);
    }
    None
}

/// The first element whose `id` is `id` in `root`'s tree, the shadow trees
/// in it and its templates' contents, in [`each_element`]'s order.
pub fn find<'gc>(cx: &Cx<'_, 'gc>, root: NodeRef<'gc>, id: &str) -> Option<NodeRef<'gc>> {
    if cx.kind(root) == NodeKind::Element
        && cx
            .attribute_value_by_id(root, names::ID)
            .is_some_and(|value| *value.borrow() == *id)
    {
        return Some(root);
    }
    if let Some(found) = cx.shadow_root(root).and_then(|shadow| find(cx, shadow, id)) {
        return Some(found);
    }
    let mut child = cx.first_child(root);
    while let Some(node) = child {
        if let Some(found) = find(cx, node, id) {
            return Some(found);
        }
        child = cx.next_sibling(node);
    }
    cx.template_content(root)
        .and_then(|content| find(cx, content, id))
}

/// An open shadow root attached to `host`, of `markup` (parsed in `host`'s
/// context).
pub fn attach_shadow<'gc>(cx: &Cx<'_, 'gc>, host: NodeRef<'gc>, markup: &str) -> NodeRef<'gc> {
    let root = cx.attach_shadow(host, open_init());
    let fragment = html::parser::parse_fragment_in(cx, host, markup, false);
    cx.append_child(root, fragment).unwrap();
    root
}

/// An open shadow root's init, its slots assigned by name.
pub fn open_init() -> ShadowRootInit {
    ShadowRootInit {
        mode: ShadowRootMode::Open,
        delegates_focus: false,
        slot_assignment: SlotAssignmentMode::Named,
        clonable: false,
        serializable: false,
    }
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

/// `html`, parsed and styled: each element with an id and a style (shadow
/// trees and template contents included), its computed style.
pub fn styles(html: &str) -> BTreeMap<String, ComputedStyle> {
    Page::new(html)
        .styles()
        .into_iter()
        .map(|(id, style)| (id, (*style).clone()))
        .collect()
}

/// `css` parsed as a loaded sheet's contents.
pub fn sheet(css: &str) -> Arc<css::StyleSheetContents> {
    Arc::new(css::StyleSheetContents::parse(css, None))
}

/// `css` as the loaded sheet of the `<link>` whose `id` is `id` (the
/// renderer's `dom::Cx::set_link_sheet`).
pub fn give_link_sheet(page: &mut Page, id: &str, css: &str) {
    page.with(id, |cx, link| {
        cx.set_link_sheet(link, sheet(css), "a.css", true)
    });
}
