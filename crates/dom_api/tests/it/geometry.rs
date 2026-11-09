//! CSSOM View's members through a frame's layout of the tests' (`Fake`):
//! what each reads of it, the layout brought up to date first (or not,
//! where no layout changes what it reads), nothing for a node the frame
//! doesn't show; and the rects script is given.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use computed_style::{ComputedStyle, Position};
use dom::{Cx, NodeId, NodeRef, NodeRoot, ObjectRef, ObjectRoot, QualName, QuirksMode};
use dom_api::{ApiError, FrameLayout, MemberKind};
use gfx::{BlockBox, ElementBox, PointF, RectF, SizeF, Viewport};

use crate::{Arg, Ret, TestDom, This, call, call_on, rc};

/// A frame's layout as a test sets it: each element's box, rects and
/// `position`, the viewport, what is at any point; and what was asked of
/// it.
pub(crate) struct Fake {
    document: NodeId,
    pub(crate) boxes: HashMap<NodeId, ElementBox>,
    pub(crate) rects: HashMap<NodeId, Vec<RectF>>,
    pub(crate) positions: HashMap<NodeId, Position>,
    /// Where each element's border box is painted.
    pub(crate) paint_offsets: HashMap<NodeId, PointF>,
    /// Elements `visibility: hidden`.
    pub(crate) hidden: Vec<NodeId>,
    pub(crate) viewport: Viewport,
    /// The browser window's rect on the screen.
    pub(crate) window: Option<RectF>,
    pub(crate) at: Option<NodeId>,
    /// What is under `at` at any point.
    pub(crate) under: Vec<NodeId>,
    log: Rc<RefCell<Vec<String>>>,
    /// The elements asked to scroll into view in their containers, in
    /// order: (container, element).
    pub(crate) scrolls: Vec<(NodeId, NodeId)>,
    /// The child frames' documents, by where their viewports are in the
    /// top-level frame's.
    pub(crate) frames: HashMap<NodeId, PointF>,
    /// A frame never laid out: the viewport's width its layout gives it.
    pub(crate) unknown_width: Option<f32>,
    /// Each text's rect, which every part of it asked for gets (each ask
    /// logged: `text {start} {end} {fixed}`).
    pub(crate) texts: HashMap<NodeId, RectF>,
    /// Elements `display: contents`.
    pub(crate) contents: Vec<NodeId>,
    /// Each element's `cursor` keyword (`auto` for one not here).
    pub(crate) cursors: HashMap<NodeId, computed_style::CursorKeyword>,
    /// Elements `user-select: none`.
    pub(crate) unselectable: Vec<NodeId>,
    /// Whether what `at` is hit is a run of its text.
    pub(crate) over_text: bool,
    /// What a key scrolls: whether it moves anything.
    pub(crate) key_scrolls: bool,
}

impl FrameLayout for Fake {
    fn shows(&mut self, document: NodeId) -> bool {
        document == self.document || self.frames.contains_key(&document)
    }

    fn point_in_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        let origin = self.frames.get(&document)?;
        Some(PointF::new(point.x - origin.x, point.y - origin.y))
    }

    fn point_from_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        let origin = self.frames.get(&document)?;
        Some(PointF::new(point.x + origin.x, point.y + origin.y))
    }

    fn update_style(&mut self, _: &Cx<'_, '_>) {
        self.log.borrow_mut().push("style".to_owned());
    }

    fn update_style_and_layout(&mut self, _: &Cx<'_, '_>) {
        self.log.borrow_mut().push("layout".to_owned());
        if let Some(width) = self.unknown_width.take() {
            self.viewport.size.width = width;
        }
    }

    fn has_viewport(&self) -> bool {
        self.unknown_width.is_none()
    }

    fn element_box(&self, node: NodeId) -> Option<ElementBox> {
        self.boxes.get(&node).copied()
    }

    fn client_rects(&self, node: NodeId) -> Vec<RectF> {
        self.rects.get(&node).cloned().unwrap_or_default()
    }

    fn text_rects(
        &self,
        text: NodeId,
        element: Option<NodeId>,
        start: u32,
        end: u32,
    ) -> Vec<RectF> {
        let fixed =
            element.is_some_and(|element| self.positions.get(&element) == Some(&Position::Fixed));
        self.log
            .borrow_mut()
            .push(format!("text {start} {end} {fixed}"));
        self.texts.get(&text).copied().into_iter().collect()
    }

    fn paint_offset(&self, node: NodeId) -> Option<PointF> {
        self.paint_offsets.get(&node).copied()
    }

    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn window_screen_rect(&self) -> Option<RectF> {
        self.window
    }

    fn computed_style<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<Arc<ComputedStyle>> {
        let id = cx.node_id(node);
        let position = self.positions.get(&id).copied().unwrap_or(Position::Static);
        let visibility = if self.hidden.contains(&id) {
            computed_style::Visibility::Hidden
        } else {
            computed_style::Visibility::Visible
        };
        let display = if self.contents.contains(&id) {
            computed_style::Display::Contents
        } else {
            ComputedStyle::default().display
        };
        let mut style = ComputedStyle {
            position,
            visibility,
            display,
            ..ComputedStyle::default()
        };
        if let Some(&keyword) = self.cursors.get(&id) {
            style.rare_inherited_mut().cursor.keyword = keyword;
        }
        if self.unselectable.contains(&id) {
            style.rare_inherited_mut().user_select = computed_style::UserSelect::None;
        }
        Some(Arc::new(style))
    }

    fn hit_test(&mut self, point: PointF) -> dom_api::Hit {
        dom_api::Hit {
            // Only whether there is one is read (the cursor's).
            element: self.element_at(point),
            text: self.over_text.then_some(self.at).flatten(),
        }
    }

    /// Logged: `key {key} {shift} {the chain's ids}`.
    fn keyboard_scroll(&mut self, chain: &[NodeId], key: &str, shift: bool) -> bool {
        let chain: Vec<String> = chain.iter().map(|id| id.get().to_string()).collect();
        self.log
            .borrow_mut()
            .push(format!("key {key} {shift} {}", chain.join(",")));
        self.key_scrolls
    }

    fn element_at(&mut self, point: PointF) -> Option<NodeId> {
        self.log
            .borrow_mut()
            .push(format!("hit {} {}", point.x, point.y));
        self.at
    }

    /// What is at a point: the one hit, then each of `under`.
    fn elements_at(&mut self, point: PointF) -> Vec<NodeId> {
        self.log
            .borrow_mut()
            .push(format!("hits {} {}", point.x, point.y));
        self.at
            .into_iter()
            .chain(self.under.iter().copied())
            .collect()
    }

    fn scroll_into_view_in(&mut self, container: NodeId, element: NodeId) {
        self.scrolls.push((container, element));
    }

    fn scroll_to(&mut self, document: NodeId, scroller: NodeId, offset: PointF) {
        let to = if scroller == document {
            "viewport".to_owned()
        } else {
            format!("{}", scroller.get())
        };
        self.log
            .borrow_mut()
            .push(format!("scroll {to} {} {}", offset.x, offset.y));
    }

    fn scroll_into_view<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) {
        self.log.borrow_mut().push(format!(
            "into view {} {:?} {:?} {}",
            cx.node_id(element).get(),
            params.block,
            params.inline,
            params.nearest_only
        ));
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A layout's log of what it was asked.
pub(crate) type Log = Rc<RefCell<Vec<String>>>;

/// A document (in quirks mode if `quirks`) with `<html><body>` and an
/// HTML element of each of `children` in the body, laid out by a `Fake`
/// with nothing in it; the document, the root, the body, then the
/// children; and the layout's log.
pub(crate) fn page(quirks: bool, children: &[&str]) -> (TestDom, Vec<Rc<NodeRoot>>, Log) {
    let mut dom = TestDom::new();
    let (nodes, document) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        if quirks {
            cx.set_quirks_mode(document, QuirksMode::Quirks);
        }
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let body = cx.create_element(document, QualName::html("body"));
        cx.append_child(html, body).unwrap();
        let mut nodes = vec![document, html, body];
        for &local in children {
            let child = cx.create_element(document, QualName::html(local));
            cx.append_child(body, child).unwrap();
            nodes.push(child);
        }
        (
            nodes
                .into_iter()
                .map(|node| Rc::new(cx.root(node)))
                .collect::<Vec<_>>(),
            cx.node_id(document),
        )
    });
    let log = Rc::new(RefCell::new(Vec::new()));
    dom.1.layout.set(Some(Box::new(Fake {
        document,
        boxes: HashMap::new(),
        rects: HashMap::new(),
        positions: HashMap::new(),
        paint_offsets: HashMap::new(),
        hidden: Vec::new(),
        viewport: Viewport {
            size: SizeF::new(800.0, 600.0),
            device_scale_factor: 1.0,
            scroll: PointF::new(0.0, 0.0),
            content_size: SizeF::new(800.0, 600.0),
        },
        window: None,
        at: None,
        under: Vec::new(),
        log: log.clone(),
        scrolls: Vec::new(),
        frames: HashMap::new(),
        unknown_width: None,
        texts: HashMap::new(),
        contents: Vec::new(),
        cursors: HashMap::new(),
        unselectable: Vec::new(),
        over_text: false,
        key_scrolls: true,
    })));
    (dom, nodes, log)
}

pub(crate) fn id(dom: &TestDom, node: &Rc<NodeRoot>) -> NodeId {
    dom.read(|cx| cx.node_id(node.get(cx.gc)))
}

/// Changes the page's `Fake`.
pub(crate) fn fake(dom: &TestDom, f: impl FnOnce(&mut Fake)) {
    fake_in(&dom.1.layout, f);
}

/// Changes the `Fake` in `cell`, as the listeners reach it.
pub(crate) fn fake_in(cell: &dom_api::LayoutCell, f: impl FnOnce(&mut Fake)) {
    let mut layout = cell.take().expect("a page's layout");
    f(layout.as_any_mut().downcast_mut::<Fake>().expect("a fake"));
    cell.set(Some(layout));
}

/// A block box at `(x, y)`, `width` by `height`, of `border` widths (top,
/// right, bottom, left); what it scrolls is its padding box.
pub(crate) fn block(x: f32, y: f32, width: f32, height: f32, border: [f32; 4]) -> ElementBox {
    ElementBox {
        border_box: RectF::new(x, y, width, height),
        start: PointF::new(x, y),
        block: Some(BlockBox {
            border,
            padding: [0.0; 4],
            margin: [0.0; 4],
            inset: [0.0; 4],
            scroll_size: SizeF::new(
                width - border[1] - border[3],
                height - border[0] - border[2],
            ),
            scroll: None,
        }),
    }
}

/// Some block box: an element that has one.
pub(crate) fn block_box(_: NodeId) -> ElementBox {
    block(0.0, 0.0, 10.0, 10.0, [0.0; 4])
}

/// An inline element's box, its first line box at `start`.
fn inline(start: PointF, around: RectF) -> ElementBox {
    ElementBox {
        border_box: around,
        start,
        block: None,
    }
}

fn number(ret: Ret) -> f64 {
    match ret {
        Ret::F64(value) => value,
        Ret::U32(value) => value.into(),
        other => panic!("not a number: {other:?}"),
    }
}

fn get_number(dom: &mut TestDom, node: &Rc<NodeRoot>, interface: &str, name: &str) -> f64 {
    number(crate::get(dom, node, interface, name))
}

/// `name`'s node (none for null).
fn get_node(dom: &mut TestDom, node: &Rc<NodeRoot>, interface: &str, name: &str) -> Option<NodeId> {
    match crate::get(dom, node, interface, name) {
        Ret::Null => None,
        Ret::Node(found) => Some(dom.read(|cx| cx.node_id(found.get(cx.gc)))),
        other => panic!("not a node: {other:?}"),
    }
}

fn method(
    dom: &mut TestDom,
    node: &Rc<NodeRoot>,
    interface: &str,
    name: &str,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call(dom, node, interface, name, MemberKind::Method, args)
}

pub(crate) fn object(ret: Ret) -> Rc<ObjectRoot> {
    match ret {
        Ret::Object(object) => Rc::new(object),
        other => panic!("not an object: {other:?}"),
    }
}

/// A `DOMRect`'s values, as `DOMRectReadOnly`'s getters read them.
pub(crate) fn rect_values(dom: &mut TestDom, rect: &Rc<ObjectRoot>) -> Vec<f64> {
    [
        "x", "y", "width", "height", "top", "right", "bottom", "left",
    ]
    .into_iter()
    .map(|name| {
        number(
            call_on(
                dom,
                This::Object(rect.clone()),
                "DOMRectReadOnly",
                name,
                MemberKind::Getter,
                &[],
            )
            .0
            .unwrap(),
        )
    })
    .collect()
}

fn bounding_rect(dom: &mut TestDom, node: &Rc<NodeRoot>) -> Vec<f64> {
    let rect = object(method(dom, node, "Element", "getBoundingClientRect", &[]).unwrap());
    rect_values(dom, &rect)
}

/// What each member asks of the layout: the members that read it lay it
/// out first, once; the window's size and scale don't.
#[test]
fn a_member_lays_out_first_unless_layout_changes_nothing_it_reads() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let div = &nodes[3];
    let window = dom.mutate(|cx| {
        Rc::new(cx.root_object(ObjectRef::Window(cx.create_window(nodes[0].get(cx.gc)))))
    });
    let asks = |dom: &mut TestDom, f: &dyn Fn(&mut TestDom)| {
        log.borrow_mut().clear();
        f(dom);
        log.borrow().clone()
    };
    let layout = vec!["layout".to_owned()];
    for name in ["offsetWidth", "offsetLeft", "offsetParent"] {
        assert_eq!(
            asks(&mut dom, &|dom| drop(crate::get(
                dom,
                div,
                "HTMLElement",
                name
            ))),
            layout,
            "{name}"
        );
    }
    for name in ["clientWidth", "clientTop", "scrollTop", "scrollWidth"] {
        assert_eq!(
            asks(&mut dom, &|dom| drop(crate::get(dom, div, "Element", name))),
            layout,
            "{name}"
        );
    }
    for name in ["getBoundingClientRect", "getClientRects"] {
        assert_eq!(
            asks(&mut dom, &|dom| drop(method(
                dom,
                div,
                "Element",
                name,
                &[]
            ))),
            layout,
            "{name}"
        );
    }
    let window_get = |dom: &mut TestDom, name: &str| {
        call_on(
            dom,
            This::Object(window.clone()),
            "Window",
            name,
            MemberKind::Getter,
            &[],
        )
        .0
        .unwrap()
    };
    for name in ["scrollX", "scrollY"] {
        assert_eq!(
            asks(&mut dom, &|dom| drop(window_get(dom, name))),
            layout,
            "{name}"
        );
    }
    for name in ["innerWidth", "innerHeight", "devicePixelRatio"] {
        assert_eq!(
            asks(&mut dom, &|dom| drop(window_get(dom, name))),
            Vec::<String>::new(),
            "{name}"
        );
    }
    // A standards-mode document's scrolling element is its root, whatever
    // the layout.
    assert_eq!(
        asks(&mut dom, &|dom| drop(crate::get(
            dom,
            &nodes[0],
            "Document",
            "scrollingElement"
        ))),
        Vec::<String>::new()
    );
}

/// A node the frame doesn't show (detached, or another document's) reads
/// as having no box, and nothing is laid out; nor without a layout.
#[test]
fn nothing_for_a_node_the_frame_doesnt_show() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let div = &nodes[3];
    let div_id = id(&dom, div);
    fake(&dom, |fake| {
        fake.boxes
            .insert(div_id, block(1.0, 2.0, 30.0, 40.0, [0.0; 4]));
        fake.rects
            .insert(div_id, vec![RectF::new(1.0, 2.0, 30.0, 40.0)]);
    });
    assert_eq!(
        get_number(&mut dom, div, "HTMLElement", "offsetWidth"),
        30.0
    );
    dom.mutate(|cx| cx.remove(div.get(cx.gc)));
    log.borrow_mut().clear();
    assert_eq!(get_number(&mut dom, div, "HTMLElement", "offsetWidth"), 0.0);
    assert_eq!(get_number(&mut dom, div, "Element", "clientWidth"), 0.0);
    assert_eq!(bounding_rect(&mut dom, div), [0.0; 8]);
    let list = object(method(&mut dom, div, "Element", "getClientRects", &[]).unwrap());
    let length = call_on(
        &mut dom,
        This::Object(list),
        "DOMRectList",
        "length",
        MemberKind::Getter,
        &[],
    )
    .0
    .unwrap();
    assert_eq!(number(length), 0.0);
    let other = rc(Ret::Node(dom.mutate(|cx| {
        let document = cx.create_document(true);
        let element = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, element).unwrap();
        cx.root(element)
    })));
    assert_eq!(
        get_node(&mut dom, &other, "HTMLElement", "offsetParent"),
        None
    );
    assert_eq!(get_number(&mut dom, &other, "Element", "clientWidth"), 0.0);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());

    // No layout at all.
    let mut dom = TestDom::new();
    let window = dom.mutate(|cx| {
        let document = cx.create_document(true);
        Rc::new(cx.root_object(ObjectRef::Window(cx.create_window(document))))
    });
    let mut window_get = |name: &str| {
        number(
            call_on(
                &mut dom,
                This::Object(window.clone()),
                "Window",
                name,
                MemberKind::Getter,
                &[],
            )
            .0
            .unwrap(),
        )
    };
    assert_eq!(window_get("innerWidth"), 0.0);
    assert_eq!(window_get("scrollY"), 0.0);
    assert_eq!(window_get("devicePixelRatio"), 1.0);
}

/// `offsetParent` and where an element is from it: a positioned
/// ancestor's padding box, an inline one's first line box, the body's
/// relative to the root's border box; none for the root, the body, a fixed
/// element and one without a box.
#[test]
fn offsets_are_from_the_offset_parent() {
    let (mut dom, nodes, _) = page(false, &["div", "div", "span", "div", "div", "div"]);
    let [_, html, body, plain, outer, span, fixed, boxless, rounded] = nodes.as_slice() else {
        unreachable!()
    };
    let inner = append(&mut dom, outer, "p");
    let bold = append(&mut dom, span, "b");
    let ids: HashMap<&str, NodeId> = [
        ("html", html),
        ("body", body),
        ("plain", plain),
        ("outer", outer),
        ("inner", &inner),
        ("span", span),
        ("bold", &bold),
        ("fixed", fixed),
        ("boxless", boxless),
        ("rounded", rounded),
    ]
    .into_iter()
    .map(|(name, node)| (name, id(&dom, node)))
    .collect();
    fake(&dom, |fake| {
        fake.boxes
            .insert(ids["html"], block(1.0, 2.0, 800.0, 600.0, [0.0; 4]));
        fake.boxes
            .insert(ids["body"], block(9.0, 9.0, 700.0, 500.0, [0.0; 4]));
        fake.boxes
            .insert(ids["plain"], block(30.0, 40.0, 10.0, 10.0, [0.0; 4]));
        fake.boxes.insert(
            ids["outer"],
            block(10.0, 20.0, 100.0, 100.0, [3.0, 0.0, 0.0, 4.0]),
        );
        fake.boxes
            .insert(ids["inner"], block(16.0, 27.0, 10.0, 10.0, [0.0; 4]));
        fake.boxes.insert(
            ids["span"],
            inline(PointF::new(40.0, 50.0), RectF::new(0.0, 50.0, 100.0, 40.0)),
        );
        fake.boxes.insert(
            ids["bold"],
            inline(PointF::new(45.0, 53.0), RectF::new(45.0, 53.0, 10.0, 10.0)),
        );
        fake.boxes
            .insert(ids["fixed"], block(5.0, 5.0, 1.0, 1.0, [0.0; 4]));
        fake.boxes
            .insert(ids["rounded"], block(0.0, 0.0, 10.5, 10.4, [0.0; 4]));
        // Positioned: the body's ancestor, which isn't its offset parent all
        // the same.
        fake.positions.insert(ids["html"], Position::Relative);
        fake.positions.insert(ids["outer"], Position::Relative);
        fake.positions.insert(ids["span"], Position::Absolute);
        fake.positions.insert(ids["fixed"], Position::Fixed);
    });
    let offsets = |dom: &mut TestDom, node: &Rc<NodeRoot>| {
        (
            get_node(dom, node, "HTMLElement", "offsetParent"),
            get_number(dom, node, "HTMLElement", "offsetLeft"),
            get_number(dom, node, "HTMLElement", "offsetTop"),
        )
    };
    // From the body: the root's border box.
    assert_eq!(offsets(&mut dom, plain), (Some(ids["body"]), 29.0, 38.0));
    // From a positioned block's padding box.
    assert_eq!(offsets(&mut dom, &inner), (Some(ids["outer"]), 2.0, 4.0));
    // From a positioned inline's first line box.
    assert_eq!(offsets(&mut dom, &bold), (Some(ids["span"]), 5.0, 3.0));
    // None: from the document's origin, or none at all.
    assert_eq!(offsets(&mut dom, html), (None, 1.0, 2.0));
    assert_eq!(offsets(&mut dom, body), (None, 0.0, 0.0));
    assert_eq!(offsets(&mut dom, fixed), (None, 5.0, 5.0));
    assert_eq!(offsets(&mut dom, boxless), (None, 0.0, 0.0));
    assert_eq!(
        get_number(&mut dom, boxless, "HTMLElement", "offsetWidth"),
        0.0
    );
    // Half away from zero.
    assert_eq!(
        get_number(&mut dom, rounded, "HTMLElement", "offsetWidth"),
        11.0
    );
    assert_eq!(
        get_number(&mut dom, rounded, "HTMLElement", "offsetHeight"),
        10.0
    );
    assert_eq!(
        get_number(&mut dom, &bold, "HTMLElement", "offsetWidth"),
        10.0
    );

    // A `table`, `td` or `th` is one too, positioned or not.
    for local in ["table", "td", "th"] {
        let cell = append(&mut dom, plain, local);
        let child = append(&mut dom, &cell, "div");
        let (cell_id, child_id) = (id(&dom, &cell), id(&dom, &child));
        fake(&dom, |fake| {
            fake.boxes
                .insert(cell_id, block(30.0, 40.0, 10.0, 10.0, [1.0; 4]));
            fake.boxes
                .insert(child_id, block(32.0, 43.0, 1.0, 1.0, [0.0; 4]));
        });
        assert_eq!(
            offsets(&mut dom, &child),
            (Some(cell_id), 1.0, 2.0),
            "{local}"
        );
        dom.mutate(|cx| cx.remove(cell.get(cx.gc)));
    }
}

pub(crate) fn append(dom: &mut TestDom, parent: &Rc<NodeRoot>, local: &str) -> Rc<NodeRoot> {
    rc(Ret::Node(dom.mutate(|cx| {
        let parent = parent.get(cx.gc);
        let child = cx.create_element(cx.document_of(parent), QualName::html(local));
        cx.append_child(parent, child).unwrap();
        cx.root(child)
    })))
}

/// `client*`: the padding box, the viewport's for the root (the body in
/// quirks mode), nothing for an inline element.
#[test]
fn client_sizes_are_the_padding_boxes() {
    for quirks in [false, true] {
        let (mut dom, nodes, _) = page(quirks, &["div", "span"]);
        let [_, html, body, div, span] = nodes.as_slice() else {
            unreachable!()
        };
        let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
        fake(&dom, |fake| {
            fake.viewport.size = SizeF::new(800.7, 600.2);
            for &node in &ids[1..4] {
                fake.boxes
                    .insert(node, block(0.0, 0.0, 50.0, 60.0, [1.0, 2.0, 3.0, 4.0]));
            }
            fake.boxes.insert(
                ids[4],
                inline(PointF::new(0.0, 0.0), RectF::new(0.0, 0.0, 10.0, 10.0)),
            );
        });
        let client = |dom: &mut TestDom, node: &Rc<NodeRoot>| {
            ["clientTop", "clientLeft", "clientWidth", "clientHeight"]
                .map(|name| get_number(dom, node, "Element", name))
        };
        let (viewport, own) = ([1.0, 4.0, 800.0, 600.0], [1.0, 4.0, 44.0, 56.0]);
        assert_eq!(client(&mut dom, div), own);
        assert_eq!(client(&mut dom, span), [0.0; 4]);
        assert_eq!(
            client(&mut dom, html),
            if quirks { own } else { viewport },
            "quirks {quirks}"
        );
        assert_eq!(
            client(&mut dom, body),
            if quirks { viewport } else { own },
            "quirks {quirks}"
        );
    }
}

/// `scroll*`: a scroll container's own, the viewport's for the scrolling
/// element (the root; in quirks mode the body, unless it scrolls itself),
/// nothing for one that doesn't scroll or an inline element.
#[test]
fn scroll_offsets_and_sizes() {
    let (mut dom, nodes, _) = page(false, &["div", "span"]);
    let [document, html, body, div, span] = nodes.as_slice() else {
        unreachable!()
    };
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        fake.viewport.scroll = PointF::new(7.5, 8.0);
        fake.viewport.content_size = SizeF::new(1000.4, 2000.6);
        fake.boxes
            .insert(ids[1], block(0.0, 0.0, 800.0, 600.0, [0.0; 4]));
        fake.boxes
            .insert(ids[2], block(0.0, 0.0, 800.0, 600.0, [0.0; 4]));
        let mut scroller = block(0.0, 0.0, 50.0, 60.0, [0.0; 4]);
        let inner = scroller.block.as_mut().unwrap();
        inner.scroll = Some(PointF::new(5.5, 6.0));
        inner.scroll_size = SizeF::new(300.0, 400.0);
        fake.boxes.insert(ids[3], scroller);
        fake.boxes.insert(
            ids[4],
            inline(PointF::new(0.0, 0.0), RectF::new(0.0, 0.0, 10.0, 10.0)),
        );
    });
    let scroll = |dom: &mut TestDom, node: &Rc<NodeRoot>| {
        ["scrollLeft", "scrollTop", "scrollWidth", "scrollHeight"]
            .map(|name| get_number(dom, node, "Element", name))
    };
    assert_eq!(scroll(&mut dom, div), [5.5, 6.0, 300.0, 400.0]);
    assert_eq!(scroll(&mut dom, html), [7.5, 8.0, 1000.0, 2001.0]);
    assert_eq!(scroll(&mut dom, body), [0.0, 0.0, 800.0, 600.0]);
    assert_eq!(scroll(&mut dom, span), [0.0; 4]);
    assert_eq!(
        get_node(&mut dom, document, "Document", "scrollingElement"),
        Some(ids[1])
    );

    let (mut dom, nodes, _) = page(true, &[]);
    let [document, html, body] = nodes.as_slice() else {
        unreachable!()
    };
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        fake.viewport.scroll = PointF::new(7.5, 8.0);
        fake.boxes
            .insert(ids[1], block(0.0, 0.0, 800.0, 600.0, [0.0; 4]));
        fake.boxes
            .insert(ids[2], block(0.0, 0.0, 700.0, 500.0, [0.0; 4]));
    });
    assert_eq!(
        get_node(&mut dom, document, "Document", "scrollingElement"),
        Some(ids[2])
    );
    assert_eq!(scroll(&mut dom, body)[..2], [7.5, 8.0]);
    assert_eq!(scroll(&mut dom, html)[..2], [0.0, 0.0]);
    // A body that scrolls itself: none is the viewport's.
    fake(&dom, |fake| {
        fake.boxes
            .get_mut(&ids[2])
            .unwrap()
            .block
            .as_mut()
            .unwrap()
            .scroll = Some(PointF::new(1.0, 2.0));
    });
    assert_eq!(
        get_node(&mut dom, document, "Document", "scrollingElement"),
        None
    );
    assert_eq!(scroll(&mut dom, body)[..2], [1.0, 2.0]);
    // Without a layout, the body.
    dom.1.layout.take();
    assert_eq!(
        get_node(&mut dom, document, "Document", "scrollingElement"),
        Some(ids[2])
    );
}

/// `getClientRects()` gives each box's rect; `getBoundingClientRect()` the
/// union of those that aren't empty, else the first; a `DOMRect`'s edges
/// are its values' sides, NaN if any is.
#[test]
fn rects() {
    let (mut dom, nodes, _) = page(false, &["div", "div"]);
    let (some, empty) = (&nodes[3], &nodes[4]);
    let (some_id, empty_id) = (id(&dom, some), id(&dom, empty));
    fake(&dom, |fake| {
        fake.rects.insert(
            some_id,
            vec![
                RectF::new(1.0, 2.0, 3.0, 4.0),
                RectF::new(0.0, 0.0, 5.0, 0.0),
                RectF::new(10.0, 20.0, 5.0, 5.0),
            ],
        );
        fake.rects.insert(
            empty_id,
            vec![
                RectF::new(5.0, 6.0, 0.0, 3.0),
                RectF::new(7.0, 8.0, 4.0, 0.0),
            ],
        );
    });
    assert_eq!(
        bounding_rect(&mut dom, some),
        [1.0, 2.0, 14.0, 23.0, 2.0, 15.0, 25.0, 1.0]
    );
    assert_eq!(
        bounding_rect(&mut dom, empty),
        [5.0, 6.0, 0.0, 3.0, 6.0, 5.0, 9.0, 5.0]
    );

    let list = object(method(&mut dom, some, "Element", "getClientRects", &[]).unwrap());
    let list_call = |dom: &mut TestDom, name: &str, kind: MemberKind, args: &[Arg]| {
        call_on(
            dom,
            This::Object(list.clone()),
            "DOMRectList",
            name,
            kind,
            args,
        )
        .0
        .unwrap()
    };
    assert_eq!(
        number(list_call(&mut dom, "length", MemberKind::Getter, &[])),
        3.0
    );
    let third = object(list_call(
        &mut dom,
        "item",
        MemberKind::Method,
        &[Arg::Number(2.0)],
    ));
    assert_eq!(rect_values(&mut dom, &third)[..4], [10.0, 20.0, 5.0, 5.0]);
    assert!(matches!(
        list_call(&mut dom, "item", MemberKind::Method, &[Arg::Number(3.0)]),
        Ret::Null
    ));

    // A negative width or height: its edges are the other way round.
    let rect = object(method(&mut dom, some, "Element", "getBoundingClientRect", &[]).unwrap());
    let set = |dom: &mut TestDom, name: &str, value: f64| {
        call_on(
            dom,
            This::Object(rect.clone()),
            "DOMRect",
            name,
            MemberKind::Setter,
            &[Arg::Number(value)],
        )
        .0
        .unwrap();
    };
    set(&mut dom, "x", 4.0);
    set(&mut dom, "y", 3.0);
    set(&mut dom, "width", -10.0);
    set(&mut dom, "height", -1.0);
    assert_eq!(
        rect_values(&mut dom, &rect),
        [4.0, 3.0, -10.0, -1.0, 2.0, 4.0, 3.0, -6.0]
    );
    set(&mut dom, "height", f64::NAN);
    let values = rect_values(&mut dom, &rect);
    assert!(
        values[3].is_nan() && values[4].is_nan() && values[6].is_nan(),
        "{values:?}"
    );
    assert_eq!([values[5], values[7]], [4.0, -6.0]);
    set(&mut dom, "x", f64::NAN);
    let values = rect_values(&mut dom, &rect);
    assert!(values[5].is_nan() && values[7].is_nan(), "{values:?}");
    // A new rect each call.
    let again = object(method(&mut dom, some, "Element", "getBoundingClientRect", &[]).unwrap());
    assert_eq!(rect_values(&mut dom, &again)[..4], [1.0, 2.0, 14.0, 23.0]);
}

/// `elementFromPoint()`: what the hit test finds, pre-painted, the root
/// where it finds nothing; nothing outside the viewport; a non-finite
/// coordinate is a `TypeError`.
#[test]
fn the_element_at_a_point() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let [document, html, _, div] = nodes.as_slice() else {
        unreachable!()
    };
    let (html_id, div_id) = (id(&dom, html), id(&dom, div));
    fake(&dom, |fake| {
        fake.viewport.size = SizeF::new(100.0, 50.0);
        fake.at = Some(div_id);
    });
    let at = |dom: &mut TestDom, x: f64, y: f64| match method(
        dom,
        document,
        "Document",
        "elementFromPoint",
        &[Arg::Number(x), Arg::Number(y)],
    ) {
        Ok(Ret::Null) => Ok(None),
        Ok(Ret::Node(found)) => Ok(Some(dom.read(|cx| cx.node_id(found.get(cx.gc))))),
        Ok(other) => panic!("{other:?}"),
        Err(error) => Err(error),
    };
    log.borrow_mut().clear();
    assert_eq!(at(&mut dom, 0.0, 0.0).unwrap(), Some(div_id));
    assert_eq!(*log.borrow(), ["layout", "hit 0 0"]);
    assert_eq!(at(&mut dom, 99.5, 49.5).unwrap(), Some(div_id));
    for (x, y) in [(100.0, 0.0), (0.0, 50.0), (-0.5, 0.0), (0.0, -0.5)] {
        assert_eq!(at(&mut dom, x, y).unwrap(), None, "{x} {y}");
    }
    fake(&dom, |fake| fake.at = None);
    assert_eq!(at(&mut dom, 1.0, 1.0).unwrap(), Some(html_id));
    for (x, y) in [(f64::NAN, 0.0), (0.0, f64::INFINITY)] {
        assert!(
            matches!(at(&mut dom, x, y), Err(ApiError::Type(_))),
            "{x} {y}"
        );
    }
}

/// The window's viewport: its size whole CSS px, its scroll offset, its
/// scale; the browser window's rect, 0 without one; none lays out.
#[test]
fn the_windows_viewport() {
    let (mut dom, nodes, log) = page(false, &[]);
    fake(&dom, |fake| {
        fake.viewport = Viewport {
            size: SizeF::new(800.9, 600.5),
            device_scale_factor: 2.0,
            scroll: PointF::new(7.5, 8.25),
            content_size: SizeF::new(800.0, 600.0),
        };
    });
    let window = dom.mutate(|cx| {
        Rc::new(cx.root_object(ObjectRef::Window(cx.create_window(nodes[0].get(cx.gc)))))
    });
    let get = |dom: &mut TestDom, name: &str| {
        number(
            call_on(
                dom,
                This::Object(window.clone()),
                "Window",
                name,
                MemberKind::Getter,
                &[],
            )
            .0
            .unwrap(),
        )
    };
    let got = [
        "innerWidth",
        "innerHeight",
        "scrollX",
        "pageXOffset",
        "scrollY",
        "pageYOffset",
        "devicePixelRatio",
    ]
    .map(|name| get(&mut dom, name));
    assert_eq!(got, [800.0, 600.0, 7.5, 7.5, 8.25, 8.25, 2.0]);
    let browser = [
        "outerWidth",
        "outerHeight",
        "screenX",
        "screenLeft",
        "screenY",
        "screenTop",
    ];
    assert_eq!(
        browser.map(|name| get(&mut dom, name)),
        [0.0; 6],
        "no browser window"
    );
    fake(&dom, |fake| {
        fake.window = Some(RectF::new(10.4, 20.6, 1024.0, 768.9))
    });
    log.borrow_mut().clear();
    assert_eq!(
        browser.map(|name| get(&mut dom, name)),
        [1024.0, 768.0, 10.0, 10.0, 20.0, 20.0],
        "whole CSS px, toward 0"
    );
    assert!(log.borrow().is_empty(), "no layout: {:?}", log.borrow());
}
