//! The parser for the DOM (`dom`): html5ever's tokenizer and tree
//! builder, a `TreeSink` over the DOM (`Sink`, `FragmentSink`), a
//! document and HTML's fragment parsing algorithm; the serializer
//! (`serialize_children`, `serialize_node`); and the html5lib
//! tree-construction dump of a result.
//!
//! - **A document's handles** are roots (`SinkHandle`, an `Rc` of a
//!   `NodeRoot`): the tree builder keeps them in its own lists (the stack
//!   of open elements, the list of active formatting elements) across the
//!   sink's calls, each its own operation of the DOM, at whose start the
//!   heap may collect; Blink's parser holds traced `Member`s. A text
//!   continues through the sink with no handle (`append_text_in`).
//! - **A document's parser** (`Parser`) takes its input as it comes and
//!   stops at every `</script>`. Its engine (`ParserHost`) is lent to it
//!   for each run, the sink borrowing the DOM from it for each of its
//!   calls: an element whose name its window's custom element registry
//!   defines, the engine constructs at once, script running in the parse
//!   (HTML's "create an element for a token" that will execute script;
//!   Blink's `HTMLConstructionSite`). Its
//!   `<template shadowrootmode>` attaches a declarative shadow root to its
//!   parent (html5ever's `attach_declarative_shadow`), whose contents the
//!   template's are; the fragment parsing algorithm's are templates for
//!   `innerHTML` (`parse_fragment_in`) and shadow roots for `setHTMLUnsafe`
//!   (`parse_fragment_unsafe_in`, the fast path not taken); a whole
//!   document parsed in an operation (`parse_document_unsafe_in`,
//!   `Document.parseHTMLUnsafe`) attaches them too. The serializer writes a
//!   host's shadow root first in it as a `<template shadowrootmode>` when
//!   asked (`serialize_children_with`, `ShadowRoots`: `getHTML`).
//! - **A fragment's parse** (`parse_fragment_in`: `innerHTML` and the
//!   like) runs in the caller's operation, its handles the operation's
//!   nodes (`FragmentSink`): it runs no script, so it needs no root and no
//!   safepoint; markup the fast path knows (`fast_path_build`) skips the
//!   tokenizer and the tree builder.
//! - **A form owner**: a listed element made while the form element
//!   pointer is set gets that form (`associate_with_form`; HTML's parser
//!   inserted flag: `dom`'s `set_parser_form_owner`).
//! - **The lines elements start on**: a script's, where its text begins;
//!   an element's that loads something, where DevTools says the parser
//!   asked for its load (`Parser::take_load_lines`).
//! - **Not yet**: the fast path for a whole document, element hooks
//!   (a script's or a style's attribute changing).

use std::borrow::Cow;
use std::cell::Cell;
use std::fmt::Write as _;
use std::rc::Rc;

use dom::{Cx, Dom, NodeKind, NodeRef, NodeRoot, QualName, QuirksMode, ScriptFlags, names};
use html5ever::driver::parse_fragment_for_element;
use html5ever::interface::{
    ElemName, ElementFlags, NodeOrText, QuirksMode as ParserQuirksMode, TreeSink,
};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{
    Attribute as ParserAttribute, LocalName, Namespace as ParserNamespace, ParseOpts,
    QualName as ParserQualName, parse_document as html5ever_parse,
};

pub use crate::serialize::{
    ShadowRoots, serialize_children, serialize_children_with, serialize_node,
};

const HTML: &str = "http://www.w3.org/1999/xhtml";
const SVG: &str = "http://www.w3.org/2000/svg";
const MATHML: &str = "http://www.w3.org/1998/Math/MathML";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

/// A node the tree builder holds: a root, kept past the sink's calls. In
/// a `RefCell`: a custom element the document's parser makes is
/// constructed as it is inserted (only then is it known to go in the
/// document, not a template's contents), and the tree builder's handle
/// made the constructed one's.
#[derive(Clone)]
pub struct SinkHandle(Rc<std::cell::RefCell<NodeRoot>>);

impl SinkHandle {
    fn new<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Self {
        Self::of(cx.root(node))
    }

    fn of(root: NodeRoot) -> Self {
        Self(Rc::new(std::cell::RefCell::new(root)))
    }

    fn get<'gc>(&self, cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
        self.0.borrow().get(cx.gc)
    }

    /// The root, for the parse's caller.
    pub fn root(&self) -> std::cell::Ref<'_, NodeRoot> {
        self.0.borrow()
    }
}

// The DOM the sink builds in, lent for each of a parser's runs (`lend`):
// html5ever's sink is `&self` and lives across them (a document's parser
// waits for its scripts), while the DOM's owner (the engine) runs script
// between, so the sink holds no borrow of it.
scoped_tls::scoped_thread_local!(static DOM: Dom);

/// Runs `f`, a run of a parser, with `dom` lent to its sink: a safepoint
/// first, as each of the sink's operations is a read (none in the run: its
/// nodes are roots anyway, `SinkHandle`).
fn lend<R>(dom: &mut Dom, f: impl FnOnce() -> R) -> R {
    dom.mutate(|_| ());
    DOM.set(dom, f)
}

/// What a document's parser asks of its engine, lent to it for a run
/// (`Parser::resume`): the DOM for each of its sink's calls, and a
/// custom element constructed at once.
pub trait ParserHost {
    fn parser_dom(&mut self) -> &mut Dom;

    /// HTML's "create an element for a token" that will execute script,
    /// for an HTML element `local` with `attributes` in `document`, if its
    /// window's registry defines `local` (else none: the parser makes it):
    /// a microtask checkpoint, an element queue pushed, the definition's
    /// constructor constructed (as `createElement` does), the attributes
    /// appended, the queue's reactions invoked.
    fn create_custom_element(
        &mut self,
        document: &NodeRoot,
        local: &str,
        attributes: &[(QualName<'_>, &str)],
    ) -> Option<NodeRoot>;

    /// The reactions of the element queue the parser's last insertion left
    /// (HTML's "insert an element at the adjusted insertion location"
    /// pushes one around it): a custom element's `connectedCallback` as it
    /// is inserted, before the markup after it is parsed; and an iframe's
    /// steps it connected.
    fn run_custom_element_reactions(&mut self);
}

/// html5ever's `TreeSink` over the DOM for a document's parser: each
/// call one operation of the DOM, borrowed from its engine (`host`, a
/// document parser's run's) or lent to it (`lend`, a whole parse's), its
/// handles roots. The engine in a `RefCell`, put in and taken out as a run
/// starts and ends: html5ever's sink is `&self` and lives across runs,
/// and constructing a custom element borrows the engine whole.
pub struct Sink<H: ?Sized + ParserHost = dyn ParserHost> {
    host: std::cell::RefCell<Option<Box<H>>>,
    /// The custom elements made (undefined, with their attributes) whose
    /// names are defined, to construct as they are inserted, if in the
    /// document: the tree builder makes an element, then finds where it
    /// goes (a template's contents, whose document has no browsing context:
    /// not constructed). One at a time, mostly.
    to_construct: std::cell::RefCell<Vec<(SinkHandle, ParserQualName, Vec<ParserAttribute>)>>,
    document: SinkHandle,
    errors: Cell<usize>,
    /// A parse with scripting disabled: the scripts it makes never run.
    scripts_never_run: bool,
    /// The tokenizer's line now, and the one the last script's start tag
    /// ended on (1-based), where an inline script's text begins: a script
    /// can't hold another, so the parser stopping at a `</script>` stops at
    /// the last one's.
    current_line: Cell<u64>,
    script_line: Cell<u64>,
    /// The line each element that loads something (`<link>`, `<img>`,
    /// `<script>`, `<iframe>`) starts on, for DevTools to name where the
    /// parser asked for its load (Blink's `FetchInitiatorInfo::position`),
    /// until taken (`Parser::take_load_lines`).
    load_lines: std::cell::RefCell<dom::NodeIdHashMap<u32>>,
}

impl<H: ?Sized + ParserHost> Sink<H> {
    /// The tokenizer's line, as the DOM takes it (`Cx::set_parser_line`).
    fn parser_line(&self) -> u32 {
        base::numerics::saturated_cast(self.current_line.get())
    }

    /// A sink building `document`, of the DOM lent to it.
    pub fn new(document: SinkHandle) -> Self {
        Self {
            host: std::cell::RefCell::new(None),
            to_construct: std::cell::RefCell::new(Vec::new()),
            document,
            errors: Cell::new(0),
            scripts_never_run: false,
            current_line: Cell::new(1),
            script_line: Cell::new(1),
            load_lines: std::cell::RefCell::default(),
        }
    }

    fn with<R>(&self, f: impl for<'gc> FnOnce(&Cx<'_, 'gc>) -> R) -> R {
        let mut host = self.host.borrow_mut();
        match host.as_mut() {
            Some(host) => host.parser_dom().read(f),
            None => DOM.with(|dom| dom.read(f)),
        }
    }

    /// `insert`, an insertion of the document's parser, in an element
    /// queue of its own whose reactions the engine runs after it, if it
    /// left one; and the steps of an iframe it connected (its browsing
    /// context made, Blink's `DidNotifySubtreeInsertionsToDocument`), with
    /// them.
    fn inserting(&self, insert: impl for<'gc> FnOnce(&Cx<'_, 'gc>)) {
        let line = self.parser_line();
        let (left, frames) = self.with(|cx| {
            let scope = dom::CeReactionsScope::enter(cx);
            // What the elements it connects ask to load, the parser asked
            // for.
            cx.set_parser_line(line);
            insert(cx);
            cx.set_parser_line(0);
            (scope.leave(), cx.has_element_changes())
        });
        if (left || frames)
            && let Some(host) = self.host.borrow_mut().as_mut()
        {
            host.run_custom_element_reactions();
        }
    }

    /// Whether an HTML element `name` is a custom element's the document's
    /// window's registry defines, with an engine lent to construct it.
    fn is_defined(&self, name: &ParserQualName) -> bool {
        if &*name.ns != HTML || !dom::has_hyphen(&name.local) || self.host.borrow().is_none() {
            return false;
        }
        self.with(|cx| {
            cx.custom_element_definition_in(self.document.get(cx), &name.local)
                .is_some()
        })
    }

    /// `node` (about to be inserted in `parent`) constructed by the engine,
    /// if it is a custom element to construct and `parent` is of the
    /// document: the handle made the constructed element's (with the
    /// attributes), the element made first let go of.
    fn construct_if_due(&self, parent: &SinkHandle, node: &SinkHandle) {
        let due = {
            let mut to_construct = self.to_construct.borrow_mut();
            let Some(at) = to_construct
                .iter()
                .position(|(handle, ..)| Rc::ptr_eq(&handle.0, &node.0))
            else {
                return;
            };
            to_construct.swap_remove(at)
        };
        let (_, name, attrs) = due;
        let in_document = self.with(|cx| {
            let parent = parent.get(cx);
            let document = self.document.get(cx);
            cx.kind(parent) == NodeKind::Document
                || elise::Gc::ptr_eq(cx.document_of(parent), document)
        });
        if !in_document {
            return;
        }
        let document = self.document.root().clone();
        let attributes: Vec<(QualName<'_>, &str)> = attrs
            .iter()
            .map(|attr| (qual_name(&attr.name), &*attr.value))
            .collect();
        let constructed = self
            .host
            .borrow_mut()
            .as_mut()
            .expect("an engine lent")
            .create_custom_element(&document, &name.local, &attributes);
        if let Some(element) = constructed {
            *node.0.borrow_mut() = element;
        }
    }
}

/// html5ever's name as the DOM's: an empty namespace or prefix none.
fn qual_name(name: &ParserQualName) -> QualName<'_> {
    QualName {
        namespace: Some(&*name.ns).filter(|ns| !ns.is_empty()),
        prefix: name.prefix.as_deref().filter(|prefix| !prefix.is_empty()),
        local: &name.local,
    }
}

// What both sinks do, in an operation of the DOM, on its nodes.

/// An element name in html5ever's atoms, made from the node's on each
/// call: a name the DOM keeps as a string becomes a dynamic atom here,
/// which html5ever drops, never in a GC object (dropping one takes a
/// global lock, and GC objects are dropped as finalizers).
#[derive(Debug)]
pub struct ElementName {
    ns: ParserNamespace,
    local: LocalName,
}

impl ElemName for ElementName {
    fn ns(&self) -> &ParserNamespace {
        &self.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.local
    }
}

/// `node`'s name as html5ever's.
fn element_name_of<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> ElementName {
    let name = cx
        .element_name(node)
        .expect("the tree builder only asks for element names");
    ElementName {
        ns: ParserNamespace::from(&*names::get(name.namespace)),
        local: LocalName::from(&*names::get(name.local)),
    }
}

/// What a sink makes of an element besides its name and attributes.
struct Making {
    /// Its scripts never run (the fragment parsing algorithm's, or a parse
    /// with scripting disabled).
    scripts_never_run: bool,
    /// It is a document's own parser: its `<link>`s block its scripts.
    document_parser: bool,
}

/// HTML's "create an element for a token", in `document`.
fn create_element_in<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    name: &ParserQualName,
    attrs: &[ParserAttribute],
    flags: &ElementFlags,
    making: &Making,
) -> NodeRef<'gc> {
    let html = &*name.ns == HTML;
    let is_script = html && &*name.local == "script";
    // A document's own parser's `<link>`: its stylesheet blocks the
    // parser's scripts while it loads.
    let is_parsers_link = making.document_parser && html && &*name.local == "link";
    let element = cx.create_element_by_parser(document, qual_name(name));
    for attr in attrs {
        cx.set_attribute_ns(element, qual_name(&attr.name), &attr.value);
    }
    if is_script {
        // The fragment parser's, or one with scripting disabled, is
        // already started: it never runs.
        cx.set_script_flags(
            element,
            ScriptFlags {
                parser_inserted: true,
                already_started: making.scripts_never_run,
                non_blocking: false,
            },
        );
    } else if is_parsers_link {
        cx.set_script_flags(
            element,
            ScriptFlags {
                parser_inserted: true,
                already_started: false,
                non_blocking: false,
            },
        );
    }
    if flags.mathml_annotation_xml_integration_point {
        cx.set_integration_point(element);
    }
    element
}

/// `text` appended to `parent`'s children, continuing its last child if
/// that is a text (as the tree builder requires).
fn append_text_in<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, parent: NodeRef<'gc>, text: &str) {
    match cx
        .last_child(parent)
        .filter(|last| cx.kind(*last) == NodeKind::Text)
    {
        Some(last) => cx.append_data(last, text),
        None => {
            let node = cx.create_text(document, text);
            cx.insert(parent, node, None);
        }
    }
}

/// `new_node` put before `sibling`, a text continuing a text before it.
fn append_before_sibling_in<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    sibling: NodeRef<'gc>,
    new_node: NodeOrText<NodeRef<'gc>>,
) {
    let parent = cx
        .parent(sibling)
        .expect("the tree builder only inserts before nodes in the tree");
    match new_node {
        NodeOrText::AppendNode(node) => cx.insert(parent, node, Some(sibling)),
        NodeOrText::AppendText(text) => {
            match cx
                .previous_sibling(sibling)
                .filter(|previous| cx.kind(*previous) == NodeKind::Text)
            {
                Some(previous) => cx.append_data(previous, &text),
                None => {
                    let node = cx.create_text(document, &text);
                    cx.insert(parent, node, Some(sibling));
                }
            }
        }
    }
}

fn add_attrs_if_missing_in<'gc>(cx: &Cx<'_, 'gc>, target: NodeRef<'gc>, attrs: &[ParserAttribute]) {
    for attr in attrs {
        let name = qual_name(&attr.name);
        if cx
            .get_attribute_ns(target, name.namespace, name.local)
            .is_none()
        {
            cx.set_attribute_ns(target, name, &attr.value);
        }
    }
}

fn reparent_children_in<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, new_parent: NodeRef<'gc>) {
    while let Some(child) = cx.first_child(node) {
        cx.insert(new_parent, child, None);
    }
}

/// HTML's "in head" insertion mode's template start tag with a
/// `shadowrootmode`, its "attach a shadow root" (html5ever made the
/// template and asks this): `host` given a shadow root with the template's
/// settings, declarative, the template's contents (what the parser puts
/// in the template goes in it, the template never inserted); none if
/// `host` can't have one or has one (html5ever inserts the template
/// then).
fn attach_declarative_shadow_in<'gc>(
    cx: &Cx<'_, 'gc>,
    host: NodeRef<'gc>,
    template: NodeRef<'gc>,
    attrs: &[ParserAttribute],
) -> bool {
    let value = |name: &str| {
        attrs
            .iter()
            .find(|attr| &*attr.name.local == name && attr.name.ns.is_empty())
            .map(|attr| &*attr.value)
    };
    let mode = match value("shadowrootmode") {
        Some(mode) if mode.eq_ignore_ascii_case("open") => dom::ShadowRootMode::Open,
        Some(mode) if mode.eq_ignore_ascii_case("closed") => dom::ShadowRootMode::Closed,
        _ => return false,
    };
    if cx.kind(host) != NodeKind::Element
        || cx.is_shadow_host(host)
        || cx.shadow_host_check(host).is_err()
    {
        return false;
    }
    let root = cx.attach_shadow(
        host,
        dom::ShadowRootInit {
            mode,
            delegates_focus: value("shadowrootdelegatesfocus").is_some(),
            slot_assignment: dom::SlotAssignmentMode::Named,
            clonable: value("shadowrootclonable").is_some(),
            serializable: value("shadowrootserializable").is_some(),
        },
    );
    cx.set_shadow_root_declarative(root, true);
    cx.set_template_content(template, root);
    true
}

fn template_contents_in<'gc>(cx: &Cx<'_, 'gc>, target: NodeRef<'gc>) -> NodeRef<'gc> {
    cx.template_content(target)
        .expect("the tree builder only asks templates for their contents")
}

fn quirks_mode(mode: ParserQuirksMode) -> QuirksMode {
    match mode {
        ParserQuirksMode::Quirks => QuirksMode::Quirks,
        ParserQuirksMode::LimitedQuirks => QuirksMode::LimitedQuirks,
        ParserQuirksMode::NoQuirks => QuirksMode::NoQuirks,
    }
}

/// A node or a text, its node as `get` gives it.
fn node_or_text<'gc, H>(
    child: NodeOrText<H>,
    get: impl FnOnce(&H) -> NodeRef<'gc>,
) -> NodeOrText<NodeRef<'gc>> {
    match child {
        NodeOrText::AppendNode(node) => NodeOrText::AppendNode(get(&node)),
        NodeOrText::AppendText(text) => NodeOrText::AppendText(text),
    }
}

impl<H: ?Sized + ParserHost> TreeSink for Sink<H> {
    type Handle = SinkHandle;
    type Output = SinkHandle;
    type ElemName<'a>
        = ElementName
    where
        Self: 'a;

    fn finish(self) -> SinkHandle {
        tracing::debug!(errors = self.errors.get(), "parsed document");
        self.document
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        // Only logged: wrapping would only miscount.
        self.errors.set(self.errors.get().wrapping_add(1));
        tracing::trace!(%msg, "parse error");
    }

    fn set_current_line(&self, line_number: u64) {
        self.current_line.set(line_number);
    }

    fn get_document(&self) -> SinkHandle {
        self.document.clone()
    }

    fn elem_name<'a>(&'a self, target: &'a SinkHandle) -> ElementName {
        self.with(|cx| element_name_of(cx, target.get(cx)))
    }

    fn create_element(
        &self,
        name: ParserQualName,
        attrs: Vec<ParserAttribute>,
        flags: ElementFlags,
    ) -> SinkHandle {
        if &*name.ns == HTML && &*name.local == "script" {
            self.script_line.set(self.current_line.get());
        }
        let loads =
            &*name.ns == HTML && matches!(&*name.local, "link" | "img" | "script" | "iframe");
        let defined = self.is_defined(&name);
        let making = Making {
            scripts_never_run: self.scripts_never_run,
            document_parser: true,
        };
        let line = self.parser_line();
        let handle = self.with(|cx| {
            // Where its event handlers' sources are.
            cx.set_parser_line(line);
            let element =
                create_element_in(cx, self.document.get(cx), &name, &attrs, &flags, &making);
            cx.set_parser_line(0);
            if loads {
                self.load_lines
                    .borrow_mut()
                    .insert(cx.node_id(element), line);
            }
            // Where its sheet's text is, for DevTools (one a `<style>`): the
            // line its start tag ends on, html5ever's as it makes it.
            if &*name.ns == HTML && &*name.local == "style" {
                cx.set_style_parser_line(element, line);
            }
            SinkHandle::new(cx, element)
        });
        if defined {
            self.to_construct
                .borrow_mut()
                .push((handle.clone(), name, attrs));
        }
        handle
    }

    fn create_comment(&self, text: StrTendril) -> SinkHandle {
        self.with(|cx| SinkHandle::new(cx, cx.create_comment(self.document.get(cx), &text)))
    }

    /// XML's (the HTML tree builder never makes one).
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> SinkHandle {
        self.with(|cx| {
            let node = cx.create_processing_instruction(self.document.get(cx), &target, &data);
            SinkHandle::new(cx, node)
        })
    }

    fn append(&self, parent: &SinkHandle, child: NodeOrText<SinkHandle>) {
        match child {
            NodeOrText::AppendNode(node) => {
                self.construct_if_due(parent, &node);
                self.inserting(|cx| cx.insert(parent.get(cx), node.get(cx), None));
            }
            NodeOrText::AppendText(text) => self.with(|cx| {
                append_text_in(cx, self.document.get(cx), parent.get(cx), &text);
            }),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &SinkHandle,
        prev_element: &SinkHandle,
        child: NodeOrText<SinkHandle>,
    ) {
        let has_parent = self.with(|cx| cx.parent(element.get(cx)).is_some());
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        self.with(|cx| {
            let document = self.document.get(cx);
            let doctype = cx.create_doctype(document, &name, &public_id, &system_id);
            cx.insert(document, doctype, None);
        });
    }

    fn get_template_contents(&self, target: &SinkHandle) -> SinkHandle {
        self.with(|cx| SinkHandle::new(cx, template_contents_in(cx, target.get(cx))))
    }

    /// A document's parser allows declarative shadow roots (its document
    /// is a navigation's).
    fn allow_declarative_shadow_roots(&self, _: &SinkHandle) -> bool {
        true
    }

    fn attach_declarative_shadow(
        &self,
        host: &SinkHandle,
        template: &SinkHandle,
        attrs: &[ParserAttribute],
    ) -> bool {
        self.with(|cx| attach_declarative_shadow_in(cx, host.get(cx), template.get(cx), attrs))
    }

    fn same_node(&self, x: &SinkHandle, y: &SinkHandle) -> bool {
        Rc::ptr_eq(&x.0, &y.0) || self.with(|cx| elise::Gc::ptr_eq(x.get(cx), y.get(cx)))
    }

    fn set_quirks_mode(&self, mode: ParserQuirksMode) {
        self.with(|cx| cx.set_quirks_mode(self.document.get(cx), quirks_mode(mode)));
    }

    fn append_before_sibling(&self, sibling: &SinkHandle, new_node: NodeOrText<SinkHandle>) {
        if let NodeOrText::AppendNode(node) = &new_node
            && let Some(parent) = self.with(|cx| {
                cx.parent(sibling.get(cx))
                    .map(|parent| SinkHandle::new(cx, parent))
            })
        {
            self.construct_if_due(&parent, node);
        }
        self.inserting(|cx| {
            let new_node = node_or_text(new_node, |node| node.get(cx));
            append_before_sibling_in(cx, self.document.get(cx), sibling.get(cx), new_node);
        });
    }

    fn add_attrs_if_missing(&self, target: &SinkHandle, attrs: Vec<ParserAttribute>) {
        let line = self.parser_line();
        self.with(|cx| {
            cx.set_parser_line(line);
            add_attrs_if_missing_in(cx, target.get(cx), &attrs);
            cx.set_parser_line(0);
        });
    }

    fn associate_with_form(
        &self,
        target: &SinkHandle,
        form: &SinkHandle,
        (parent, previous): (&SinkHandle, Option<&SinkHandle>),
    ) {
        self.with(|cx| {
            associate_with_form_in(
                cx,
                target.get(cx),
                form.get(cx),
                parent.get(cx),
                previous.map(|node| node.get(cx)),
            );
        });
    }

    fn remove_from_parent(&self, target: &SinkHandle) {
        self.with(|cx| cx.remove(target.get(cx)));
    }

    fn reparent_children(&self, node: &SinkHandle, new_parent: &SinkHandle) {
        self.inserting(|cx| reparent_children_in(cx, node.get(cx), new_parent.get(cx)));
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &SinkHandle) -> bool {
        self.with(|cx| cx.is_integration_point(handle.get(cx)))
    }

    /// Customizable `<select>`: `<selectedcontent>` a copy of the selected
    /// `<option>`'s contents.
    fn maybe_clone_an_option_into_selectedcontent(&self, option: &SinkHandle) {
        self.with(|cx| clone_into_selectedcontent(cx, option.get(cx)));
    }
}

/// HTML's "create an element for a token": a form-associated element made
/// while the form element pointer is set (html5ever checks the template
/// and `form` attribute conditions) is associated with that form if the
/// intended parent is in the form's tree: the table's parent when foster
/// parenting (`previous` the element before the table), or that element if
/// the table has none.
fn associate_with_form_in<'gc>(
    cx: &Cx<'_, 'gc>,
    target: NodeRef<'gc>,
    form: NodeRef<'gc>,
    parent: NodeRef<'gc>,
    previous: Option<NodeRef<'gc>>,
) {
    let intended = match previous {
        Some(previous) if cx.parent(parent).is_none() => previous,
        _ => parent,
    };
    if elise::Gc::ptr_eq(cx.root_node(intended, false), cx.root_node(form, false)) {
        cx.set_parser_form_owner(target, form);
    }
}

/// html5ever's `TreeSink` for HTML's fragment parsing algorithm, in one
/// operation of the DOM (the member's that parses: `innerHTML` and the
/// like), its handles the operation's nodes. No root, and no safepoint
/// while it builds: the parse runs no script (Blink's fragment parse is
/// synchronous too, its nodes on the stack, which Oilpan scans).
struct FragmentSink<'a, 'b, 'gc> {
    cx: &'a Cx<'b, 'gc>,
    document: NodeRef<'gc>,
    fragment: NodeRef<'gc>,
    /// html5ever's root, which it puts in the fragment first: kept out of
    /// the tree, its children the fragment's.
    root: Cell<Option<NodeRef<'gc>>>,
    /// The context, if it is a MathML `annotation-xml` HTML integration
    /// point by its `encoding` now (the parser that made it knew it from
    /// its start tag); the sink otherwise knows only the elements it made.
    integration_point: Option<NodeRef<'gc>>,
    errors: Cell<usize>,
    /// Its `<template shadowrootmode>`s attach declarative shadow roots
    /// (`setHTMLUnsafe`'s, `parseHTMLUnsafe`'s; not `innerHTML`'s).
    declarative_shadows: bool,
    /// A whole document's parse (`parseHTMLUnsafe`): `fragment` is the
    /// document, html5ever's root its `<html>`.
    whole_document: bool,
    /// The tokenizer's line, as the DOM takes it (`Cx::set_parser_line`):
    /// where the inline event handlers' sources are.
    current_line: Cell<u64>,
}

impl<'gc> FragmentSink<'_, '_, 'gc> {
    /// Where html5ever's `parent` is: the fragment for its root, else
    /// `parent` itself.
    fn parent(&self, parent: NodeRef<'gc>) -> NodeRef<'gc> {
        match self.root.get() {
            Some(root) if elise::Gc::ptr_eq(root, parent) => self.fragment,
            _ => parent,
        }
    }
}

impl<'gc> TreeSink for FragmentSink<'_, '_, 'gc> {
    type Handle = NodeRef<'gc>;
    type Output = ();
    type ElemName<'a>
        = ElementName
    where
        Self: 'a;

    fn finish(self) {
        tracing::debug!(errors = self.errors.get(), "parsed fragment");
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        // Only logged: wrapping would only miscount.
        self.errors.set(self.errors.get().wrapping_add(1));
        tracing::trace!(%msg, "parse error");
    }

    fn get_document(&self) -> NodeRef<'gc> {
        self.fragment
    }

    fn elem_name<'a>(&'a self, target: &'a NodeRef<'gc>) -> ElementName {
        element_name_of(self.cx, *target)
    }

    fn set_current_line(&self, line_number: u64) {
        self.current_line.set(line_number);
    }

    fn create_element(
        &self,
        name: ParserQualName,
        attrs: Vec<ParserAttribute>,
        flags: ElementFlags,
    ) -> NodeRef<'gc> {
        let making = Making {
            scripts_never_run: true,
            document_parser: false,
        };
        // Where its event handlers' sources are.
        self.cx
            .set_parser_line(base::numerics::saturated_cast(self.current_line.get()));
        let element = create_element_in(self.cx, self.document, &name, &attrs, &flags, &making);
        self.cx.set_parser_line(0);
        element
    }

    fn create_comment(&self, text: StrTendril) -> NodeRef<'gc> {
        self.cx.create_comment(self.document, &text)
    }

    /// XML's (the HTML tree builder never makes one).
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeRef<'gc> {
        self.cx
            .create_processing_instruction(self.document, &target, &data)
    }

    fn append(&self, parent: &NodeRef<'gc>, child: NodeOrText<NodeRef<'gc>>) {
        if let NodeOrText::AppendNode(node) = &child
            && !self.whole_document
            && elise::Gc::ptr_eq(*parent, self.fragment)
            && self.root.get().is_none()
        {
            self.root.set(Some(*node));
            return;
        }
        let parent = self.parent(*parent);
        match child {
            NodeOrText::AppendNode(node) => self.cx.insert(parent, node, None),
            NodeOrText::AppendText(text) => append_text_in(self.cx, self.document, parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeRef<'gc>,
        prev_element: &NodeRef<'gc>,
        child: NodeOrText<NodeRef<'gc>>,
    ) {
        if self.cx.parent(*element).is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    /// The fragment parsing algorithm never makes one; a whole document's
    /// parse does.
    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        if self.whole_document {
            let doctype = self
                .cx
                .create_doctype(self.fragment, &name, &public_id, &system_id);
            self.cx.insert(self.fragment, doctype, None);
        }
    }

    fn get_template_contents(&self, target: &NodeRef<'gc>) -> NodeRef<'gc> {
        template_contents_in(self.cx, *target)
    }

    fn allow_declarative_shadow_roots(&self, _: &NodeRef<'gc>) -> bool {
        self.declarative_shadows
    }

    fn attach_declarative_shadow(
        &self,
        host: &NodeRef<'gc>,
        template: &NodeRef<'gc>,
        attrs: &[ParserAttribute],
    ) -> bool {
        attach_declarative_shadow_in(self.cx, *host, *template, attrs)
    }

    fn same_node(&self, x: &NodeRef<'gc>, y: &NodeRef<'gc>) -> bool {
        elise::Gc::ptr_eq(*x, *y)
    }

    /// The fragment has its context's (and the tree builder never sets
    /// it there); a whole document's is the tree builder's.
    fn set_quirks_mode(&self, mode: ParserQuirksMode) {
        if self.whole_document {
            self.cx.set_quirks_mode(self.fragment, quirks_mode(mode));
        }
    }

    fn append_before_sibling(&self, sibling: &NodeRef<'gc>, new_node: NodeOrText<NodeRef<'gc>>) {
        append_before_sibling_in(self.cx, self.document, *sibling, new_node);
    }

    fn add_attrs_if_missing(&self, target: &NodeRef<'gc>, attrs: Vec<ParserAttribute>) {
        add_attrs_if_missing_in(self.cx, *target, &attrs);
    }

    fn associate_with_form(
        &self,
        target: &NodeRef<'gc>,
        form: &NodeRef<'gc>,
        (parent, previous): (&NodeRef<'gc>, Option<&NodeRef<'gc>>),
    ) {
        associate_with_form_in(self.cx, *target, *form, *parent, previous.copied());
    }

    fn remove_from_parent(&self, target: &NodeRef<'gc>) {
        self.cx.remove(*target);
    }

    fn reparent_children(&self, node: &NodeRef<'gc>, new_parent: &NodeRef<'gc>) {
        reparent_children_in(self.cx, *node, self.parent(*new_parent));
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeRef<'gc>) -> bool {
        self.integration_point
            .is_some_and(|context| elise::Gc::ptr_eq(context, *handle))
            || self.cx.is_integration_point(*handle)
    }

    fn maybe_clone_an_option_into_selectedcontent(&self, option: &NodeRef<'gc>) {
        clone_into_selectedcontent(self.cx, *option);
    }
}

/// Whether `node` is the HTML element `local`.
fn is_html<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, local: &str) -> bool {
    cx.element_name(node).is_some_and(|name| {
        name.namespace == names::HTML_NAMESPACE && names::get(name.local) == local
    })
}

/// Customizable `<select>`: the selected `<option>`'s contents copied into
/// its select's `<selectedcontent>`, its old contents gone.
fn clone_into_selectedcontent<'gc>(cx: &Cx<'_, 'gc>, option: NodeRef<'gc>) {
    let has = |node, attribute| cx.get_attribute_ns(node, None, attribute).is_some();
    let mut ancestor = cx.parent(option);
    let select = loop {
        let Some(node) = ancestor else { return };
        if is_html(cx, node, "select") {
            break node;
        }
        ancestor = cx.parent(node);
    };
    if has(select, "multiple") {
        return;
    }
    // The select's descendants, in tree order.
    let mut descendants = Vec::new();
    let mut stack = vec![select];
    while let Some(node) = stack.pop() {
        let mut child = cx.last_child(node);
        while let Some(at) = child {
            stack.push(at);
            child = cx.previous_sibling(at);
        }
        if !elise::Gc::ptr_eq(node, select) {
            descendants.push(node);
        }
    }
    let Some(&selectedcontent) = descendants
        .iter()
        .find(|node| is_html(cx, **node, "selectedcontent"))
    else {
        return;
    };
    let options: Vec<NodeRef<'gc>> = descendants
        .iter()
        .copied()
        .filter(|node| is_html(cx, *node, "option"))
        .collect();
    let selected = has(option, "selected")
        || (!options.iter().any(|other| has(*other, "selected"))
            && options
                .first()
                .is_some_and(|first| elise::Gc::ptr_eq(*first, option)));
    if !selected {
        return;
    }
    while let Some(old) = cx.first_child(selectedcontent) {
        cx.remove(old);
    }
    let mut child = cx.first_child(option);
    while let Some(at) = child {
        if matches!(
            cx.kind(at),
            NodeKind::Element | NodeKind::Text | NodeKind::Comment
        ) {
            let copy = cx.clone_node(at, None, true);
            cx.insert(selectedcontent, copy, None);
        }
        child = cx.next_sibling(at);
    }
}

fn opts(scripting_enabled: bool, quirks_mode: QuirksMode) -> ParseOpts {
    ParseOpts {
        tree_builder: TreeBuilderOpts {
            scripting_enabled,
            quirks_mode: match quirks_mode {
                QuirksMode::Quirks => ParserQuirksMode::Quirks,
                QuirksMode::LimitedQuirks => ParserQuirksMode::LimitedQuirks,
                QuirksMode::NoQuirks => ParserQuirksMode::NoQuirks,
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A new HTML document of `dom` parsed from `input` (with scripting
/// disabled, `<noscript>` is markup and the scripts never run).
pub fn parse_document(dom: &mut Dom, input: &str, scripting_enabled: bool) -> SinkHandle {
    let _span = trace_event::trace_event!("ParseHTML");
    // A document of what the fast path takes (no script runs in it).
    if !scripting_enabled
        && let Some(document) = dom.mutate(|cx| {
            let document = cx.create_document(true);
            crate::fast_path_build::parse_document(cx, document, input)
                .then(|| SinkHandle::new(cx, document))
        })
    {
        return document;
    }
    let document = dom.mutate(|cx| SinkHandle::new(cx, cx.create_document(true)));
    let mut sink = Sink::<dyn ParserHost>::new(document);
    sink.scripts_never_run = !scripting_enabled;
    lend(dom, || {
        html5ever_parse(sink, opts(scripting_enabled, QuirksMode::NoQuirks)).one(input)
    })
}

/// A new XML document of `dom` parsed from `input` (already decoded), for
/// an SVG image (Blink's `XMLDocumentParser` into an `SVGImage`'s
/// document): `xml5ever` into the same sink as HTML's,
/// the DOCTYPE's entities expanded first (`xml::prepare`); nothing runs, no
/// script, attributes only stored.
pub fn parse_xml_document(dom: &mut Dom, input: &str) -> SinkHandle {
    let _span = trace_event::trace_event!("ParseXML");
    let document = dom.mutate(|cx| SinkHandle::new(cx, cx.create_document(false)));
    let mut sink = Sink::<dyn ParserHost>::new(document);
    sink.scripts_never_run = true;
    let input = crate::xml::prepare(input);
    lend(dom, || {
        xml5ever::driver::parse_document(sink, xml5ever::driver::XmlParseOpts::default())
            .one(&*input)
    })
}

/// HTML's fragment parsing algorithm: `input` parsed as the contents of
/// the element `context`, which it only reads, into a new fragment of
/// `context`'s document, in that document's quirks mode, in the
/// operation `cx` (no safepoint while it builds); its scripts never run.
///
/// # Panics
///
/// If `context` isn't an element.
pub fn parse_fragment_in<'gc>(
    cx: &Cx<'_, 'gc>,
    context: NodeRef<'gc>,
    input: &str,
    scripting_enabled: bool,
) -> NodeRef<'gc> {
    parse_fragment_with(cx, context, input, scripting_enabled, false)
}

/// `parse_fragment_in` that attaches declarative shadow roots (HTML's
/// "unsafely set HTML": `setHTMLUnsafe`; the fast path, which knows no
/// `shadowrootmode`, not taken).
pub fn parse_fragment_unsafe_in<'gc>(
    cx: &Cx<'_, 'gc>,
    context: NodeRef<'gc>,
    input: &str,
    scripting_enabled: bool,
) -> NodeRef<'gc> {
    parse_fragment_with(cx, context, input, scripting_enabled, true)
}

/// A new HTML document of `cx`'s DOM parsed from `input` in the operation,
/// scripting disabled, declarative shadow roots attached (HTML's "parse
/// HTML from a string": `Document.parseHTMLUnsafe`'s).
pub fn parse_document_unsafe_in<'gc>(cx: &Cx<'_, 'gc>, input: &str) -> NodeRef<'gc> {
    parse_document_in(cx, input, true)
}

/// `parse_document_unsafe_in` but for declarative shadow roots, which
/// stay templates (`DOMParser`'s: HTML's "parse HTML from a string" with
/// them not allowed).
pub fn parse_document_safe_in<'gc>(cx: &Cx<'_, 'gc>, input: &str) -> NodeRef<'gc> {
    parse_document_in(cx, input, false)
}

fn parse_document_in<'gc>(
    cx: &Cx<'_, 'gc>,
    input: &str,
    declarative_shadows: bool,
) -> NodeRef<'gc> {
    let _span = trace_event::trace_event!("ParseHTML");
    let document = cx.create_document(true);
    if !declarative_shadows && crate::fast_path_build::parse_document(cx, document, input) {
        // What the tokenizer and the tree builder make of it is the same.
        #[cfg(debug_assertions)]
        {
            let full = parse_document_fully_in(cx, input, false);
            assert!(
                crate::fast_path_build::same_tree(cx, document, full)
                    && cx.quirks_mode(document) == cx.quirks_mode(full),
                "the fast path parsed the document {input:?} otherwise"
            );
        }
        return document;
    }
    parse_whole_document(cx, document, input, declarative_shadows);
    document
}

/// A new document parsed from `input` with the tokenizer and the tree
/// builder, whatever the input (the debug build's check of the fast path).
#[cfg(debug_assertions)]
fn parse_document_fully_in<'gc>(
    cx: &Cx<'_, 'gc>,
    input: &str,
    declarative_shadows: bool,
) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    parse_whole_document(cx, document, input, declarative_shadows);
    document
}

fn parse_whole_document<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    input: &str,
    declarative_shadows: bool,
) {
    let sink = FragmentSink {
        cx,
        document,
        fragment: document,
        root: Cell::new(None),
        integration_point: None,
        errors: Cell::new(0),
        declarative_shadows,
        whole_document: true,
        current_line: Cell::new(1),
    };
    html5ever_parse(sink, opts(false, QuirksMode::NoQuirks)).one(input);
}

fn parse_fragment_with<'gc>(
    cx: &Cx<'_, 'gc>,
    context: NodeRef<'gc>,
    input: &str,
    scripting_enabled: bool,
    declarative_shadows: bool,
) -> NodeRef<'gc> {
    let _span = trace_event::trace_event!("ParseHTML");
    let name = cx
        .element_name(context)
        .expect("the fragment parsing context is an element");
    let document = cx.document_of(context);
    // A MathML `annotation-xml` is an HTML integration point by its
    // `encoding`, as it is now.
    let integration_point = name.namespace == names::MATHML_NAMESPACE
        && names::get(name.local) == "annotation-xml"
        && cx
            .get_attribute_ns(context, None, "encoding")
            .is_some_and(|encoding| {
                let encoding = encoding.to_str();
                encoding.eq_ignore_ascii_case("text/html")
                    || encoding.eq_ignore_ascii_case("application/xhtml+xml")
            });
    let fragment = cx.create_document_fragment(document);
    if !declarative_shadows
        && crate::fast_path_build::parse_fragment(cx, document, fragment, context, input)
    {
        // What the fragment parsing algorithm makes of it is the same.
        #[cfg(debug_assertions)]
        {
            let full = cx.create_document_fragment(document);
            let options = FragmentOptions {
                scripting_enabled,
                declarative_shadows: false,
            };
            parse_fragment_fully(
                cx,
                context,
                document,
                full,
                integration_point,
                input,
                options,
            );
            assert!(
                crate::fast_path_build::same_tree(cx, fragment, full),
                "the fast path parsed {input:?} otherwise"
            );
        }
        return fragment;
    }
    let options = FragmentOptions {
        scripting_enabled,
        declarative_shadows,
    };
    parse_fragment_fully(
        cx,
        context,
        document,
        fragment,
        integration_point,
        input,
        options,
    );
    fragment
}

/// How a fragment parses: whether scripting is enabled for its context,
/// whether its `<template shadowrootmode>`s attach shadow roots.
#[derive(Clone, Copy)]
struct FragmentOptions {
    scripting_enabled: bool,
    declarative_shadows: bool,
}

/// `parse_fragment_in` into `fragment` with the tokenizer and the tree
/// builder, whatever the input.
fn parse_fragment_fully<'gc>(
    cx: &Cx<'_, 'gc>,
    context: NodeRef<'gc>,
    document: NodeRef<'gc>,
    fragment: NodeRef<'gc>,
    integration_point: bool,
    input: &str,
    options: FragmentOptions,
) {
    let FragmentOptions {
        scripting_enabled,
        declarative_shadows,
    } = options;
    let sink = FragmentSink {
        cx,
        document,
        fragment,
        root: Cell::new(None),
        integration_point: integration_point.then_some(context),
        errors: Cell::new(0),
        declarative_shadows,
        whole_document: false,
        current_line: Cell::new(1),
    };
    // The form element pointer: the nearest `<form>` from the context up,
    // itself too (HTML's "parsing HTML fragments"): a `<form>` in the
    // fragment is a nested form there. None for a `<template>`'s contents
    // (whatwg/html#12257, Chromium's): what goes in them is no form's.
    let is = |node, local| cx.qualified_name(node) == Some(dom::QualifiedName::html(local));
    let form = (!is(context, names::local::TEMPLATE))
        .then(|| {
            std::iter::successors(Some(context), |&node| cx.parent(node))
                .find(|&node| is(node, names::local::FORM))
        })
        .flatten();
    parse_fragment_for_element(
        sink,
        opts(scripting_enabled, cx.quirks_mode(document)),
        context,
        scripting_enabled,
        form,
    )
    .one(input);
}

/// `parse_fragment_in` in an operation of its own, the fragment rooted.
///
/// # Panics
///
/// If `context` isn't an element.
pub fn parse_fragment(
    dom: &mut Dom,
    context: &NodeRoot,
    input: &str,
    scripting_enabled: bool,
) -> SinkHandle {
    dom.mutate(|cx| {
        let fragment = parse_fragment_in(cx, context.get(cx.gc), input, scripting_enabled);
        SinkHandle::new(cx, fragment)
    })
}

/// `node`'s children in the html5lib tree-construction test format: one `| `-prefixed line per node and
/// attribute, two spaces a level, a `<template>`'s contents under a
/// `content` line.
pub fn dump_children(dom: &mut Dom, node: &NodeRoot) -> String {
    dom.read(|cx| {
        let mut out = String::new();
        let mut child = cx.first_child(node.get(cx.gc));
        while let Some(at) = child {
            dump_node(cx, &mut out, at, 0);
            child = cx.next_sibling(at);
        }
        out
    })
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "depth is how deep the tree is: what recursion on the stack reaches, far from usize::MAX / 2"
)]
fn line(out: &mut String, depth: usize, text: &str) {
    let _ = writeln!(out, "| {:indent$}{text}", "", indent = depth * 2);
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "depth is how deep the recursion is, a few more than the tree's: far from usize::MAX"
)]
fn dump_node<'gc>(cx: &Cx<'_, 'gc>, out: &mut String, node: NodeRef<'gc>, depth: usize) {
    let data = || {
        cx.data(node)
            .map(|data| data.to_string())
            .unwrap_or_default()
    };
    match cx.kind(node) {
        NodeKind::DocumentType => {
            let doctype = cx.doctype(node).expect("a doctype");
            if doctype.public_id.is_empty() && doctype.system_id.is_empty() {
                line(out, depth, &format!("<!DOCTYPE {}>", doctype.name));
            } else {
                line(
                    out,
                    depth,
                    &format!(
                        "<!DOCTYPE {} \"{}\" \"{}\">",
                        doctype.name, doctype.public_id, doctype.system_id
                    ),
                );
            }
        }
        NodeKind::Comment => line(out, depth, &format!("<!-- {} -->", data())),
        // A CDATA section (only script makes one, in an XML document):
        // html5lib's form.
        NodeKind::Text if cx.is_cdata_section(node) => {
            line(out, depth, &format!("<![CDATA[{}]]>", data()));
        }
        NodeKind::Text => line(out, depth, &format!("\"{}\"", data())),
        // In no tree.
        NodeKind::Attr => {}
        NodeKind::ProcessingInstruction => {
            let target = cx.target(node).expect("a target");
            line(out, depth, &format!("<?{target} {}>", data()));
        }
        NodeKind::Element => {
            let name = cx.element_name(node).expect("an element");
            let prefix = match &*names::get(name.namespace) {
                SVG => "svg ",
                MATHML => "math ",
                _ => "",
            };
            line(out, depth, &format!("<{prefix}{}>", names::get(name.local)));
            let mut attrs: Vec<(String, String)> = (0..cx.attribute_count(node))
                .filter_map(|index| cx.attribute_at(node, index))
                .map(|attr| {
                    let prefix = match &*names::get(attr.name.namespace) {
                        XLINK => "xlink ",
                        XML => "xml ",
                        XMLNS => "xmlns ",
                        _ => "",
                    };
                    (
                        format!("{prefix}{}", names::get(attr.name.local)),
                        attr.value.to_string(),
                    )
                })
                .collect();
            // By attribute name, in UTF-16 code units, as the format asks.
            attrs.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            for (name, value) in &attrs {
                line(out, depth + 1, &format!("{name}=\"{value}\""));
            }
            if let Some(content) = cx.template_content(node) {
                line(out, depth + 1, "content");
                let mut child = cx.first_child(content);
                while let Some(at) = child {
                    dump_node(cx, out, at, depth + 2);
                    child = cx.next_sibling(at);
                }
            }
        }
        NodeKind::Document | NodeKind::DocumentFragment => {}
    }
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        dump_node(cx, out, at, depth + 1);
        child = cx.next_sibling(at);
    }
}

/// A document's parser: its input as
/// it comes, stopping at every `</script>` so the script runs before the
/// rest is parsed (HTML's "parser pause"; Blink's `HTMLDocumentParser`).
/// The caller lends it its engine for each run (`resume`, `finish`), so
/// while it waits the caller uses the DOM, runs script and comes back.
pub struct Parser<H: ?Sized + ParserHost = dyn ParserHost> {
    tokenizer:
        html5ever::tokenizer::Tokenizer<html5ever::tree_builder::TreeBuilder<SinkHandle, Sink<H>>>,
    input: html5ever::tokenizer::BufferQueue,
}

/// Where a `Parser` stopped.
pub enum ParserState {
    /// It has parsed all the input it was given: `push` more, or `finish`.
    NeedsInput,
    /// At a script element's end tag: run the script (or wait for it),
    /// then `resume`.
    Script {
        element: SinkHandle,
        /// The line its text begins on (1-based).
        line: u64,
    },
}

impl<H: ?Sized + ParserHost> Parser<H> {
    /// A parser filling `document` (a new, empty one: its window's),
    /// scripting enabled.
    pub fn new(dom: &Dom, document: &NodeRoot) -> Self {
        Self::with_srcdoc(dom, document, false)
    }

    /// [`new`](Self::new), for an iframe's `srcdoc` document (HTML's
    /// "iframe srcdoc document"): never in quirks mode for lack of a
    /// doctype.
    pub fn new_srcdoc(dom: &Dom, document: &NodeRoot) -> Self {
        Self::with_srcdoc(dom, document, true)
    }

    fn with_srcdoc(dom: &Dom, document: &NodeRoot, iframe_srcdoc: bool) -> Self {
        let document = dom.read(|cx| SinkHandle::new(cx, document.get(cx.gc)));
        let sink = Sink::new(document);
        let tree_builder = html5ever::tree_builder::TreeBuilder::new(
            sink,
            TreeBuilderOpts {
                iframe_srcdoc,
                ..opts(true, QuirksMode::NoQuirks).tree_builder
            },
        );
        Self {
            tokenizer: html5ever::tokenizer::Tokenizer::new(tree_builder, Default::default()),
            input: html5ever::tokenizer::BufferQueue::default(),
        }
    }

    /// Appends `text` to the input; `resume` parses it.
    pub fn push(&mut self, text: &str) {
        self.input.push_back(StrTendril::from_slice(text));
    }

    /// Parses until the next script element's end tag or the end of the
    /// input given so far, `host` lent to it (and given back).
    pub fn resume(&mut self, host: Box<H>) -> (ParserState, Box<H>) {
        let _span = trace_event::trace_event!("ParseHTML");
        self.lend(host, |parser| parser.run())
    }

    /// `f` with `host` in the sink: a safepoint first (each of the sink's
    /// operations is a read), `host` taken back after.
    fn lend<R>(&mut self, mut host: Box<H>, f: impl FnOnce(&mut Self) -> R) -> (R, Box<H>) {
        host.parser_dom().mutate(|_| ());
        *self.tokenizer.sink.sink.host.borrow_mut() = Some(host);
        let result = f(self);
        let host = self
            .tokenizer
            .sink
            .sink
            .host
            .borrow_mut()
            .take()
            .expect("the engine lent");
        (result, host)
    }

    fn run(&mut self) -> ParserState {
        loop {
            match self.tokenizer.feed(&self.input) {
                html5ever::interface::TokenizerResult::Done => return ParserState::NeedsInput,
                html5ever::interface::TokenizerResult::Script(element) => {
                    let line = self.tokenizer.sink.sink.script_line.get();
                    return ParserState::Script { element, line };
                }
                // The document was decoded before parsing.
                html5ever::interface::TokenizerResult::EncodingIndicator(_) => {}
            }
        }
    }

    /// The end of the input: parses what is left and closes every open
    /// element, `host` lent to it (and given back). A script cut off by the
    /// end isn't run.
    pub fn finish(self, host: Box<H>) -> Box<H> {
        self.finish_taking_load_lines(host).0
    }

    /// `finish`, and the [load lines](Self::take_load_lines) of the
    /// elements made since last taken.
    pub fn finish_taking_load_lines(mut self, host: Box<H>) -> (Box<H>, dom::NodeIdHashMap<u32>) {
        let _span = trace_event::trace_event!("ParseHTML");
        let ((), host) = self.lend(host, |parser| parser.tokenizer.end());
        (host, self.take_load_lines())
    }

    /// The line (1-based) each element it made since last asked starts on,
    /// of those that load something (`<link>`, `<img>`, `<script>`,
    /// `<iframe>`): where the parser asked for its load, as DevTools names
    /// it.
    pub fn take_load_lines(&self) -> dom::NodeIdHashMap<u32> {
        std::mem::take(&mut *self.tokenizer.sink.sink.load_lines.borrow_mut())
    }
}
