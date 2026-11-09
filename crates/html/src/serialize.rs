//! HTML's fragment serializing (`innerHTML`, `outerHTML`): html5ever's
//! serializer does the escaping and the rules for raw text and void
//! elements; this walks the tree for it, in an operation of the DOM.
//! Blink's counterpart is `MarkupAccumulator` with an HTML
//! `MarkupFormatter`.

use std::io;

use dom::{Cx, NameIds, NodeKind, NodeRef, names};
use html5ever::serialize::{
    AttrRef, Serialize, SerializeOpts, Serializer, TraversalScope, serialize,
};
use html5ever::{LocalName, Namespace, QualName as ParserQualName, ns};

/// The elements HTML serializes as void: no end tag and no contents.
const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// HTML's "HTML fragment serialization algorithm" of `node`'s children (a
/// `<template>`'s contents): `innerHTML`. An element that serializes as
/// void gives the empty string, whatever it holds. `scripting_enabled` is
/// whether scripting is enabled for `node` (whether `<noscript>` contents
/// are raw text).
pub fn serialize_children<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    scripting_enabled: bool,
) -> String {
    serialize_children_with(cx, node, scripting_enabled, ShadowRoots::default())
}

/// The shadow roots a serialization writes (HTML's `getHTML` options:
/// `serializableShadowRoots`, `shadowRoots`): its hosts' serializable ones
/// if `serializable`, and those `listed`; each as a `<template
/// shadowrootmode>` first in its host. None by default (`innerHTML`).
#[derive(Clone, Copy, Default)]
pub struct ShadowRoots<'a, 'gc> {
    pub serializable: bool,
    pub listed: &'a [NodeRef<'gc>],
}

/// `serialize_children` that writes the shadow roots `shadows` says
/// (`getHTML`), `node`'s own too if it is a host.
pub fn serialize_children_with<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    scripting_enabled: bool,
    shadows: ShadowRoots<'_, 'gc>,
) -> String {
    if is_void(cx, node) {
        return String::new();
    }
    // The node's own name only matters for its text children: raw in an
    // HTML `<style>` and the like. html5ever takes any element's local name
    // for that, so only an HTML element's is passed.
    let parent = cx
        .element_name(node)
        .filter(|name| name.namespace == names::HTML_NAMESPACE)
        .map(|name| element_name(&name));
    run(
        &Children(cx, node, shadows),
        SerializeOpts {
            scripting_enabled,
            traversal_scope: TraversalScope::ChildrenOnly(parent),
            ..Default::default()
        },
    )
}

/// The serialization of `node` itself and what it holds: `outerHTML`
/// (HTML's algorithm on a fictional parent of `node` alone), so a void
/// element is its start tag. `node` is one that can be a child: not a
/// document or a fragment.
pub fn serialize_node<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    scripting_enabled: bool,
) -> String {
    run(
        &Node(cx, node, ShadowRoots::default()),
        SerializeOpts {
            scripting_enabled,
            traversal_scope: TraversalScope::IncludeNode,
            ..Default::default()
        },
    )
}

fn run(what: &impl Serialize, opts: SerializeOpts) -> String {
    let mut out = Vec::new();
    serialize(&mut out, what, opts).expect("writing to a Vec doesn't fail");
    String::from_utf8(out).expect("the serializer writes the DOM's UTF-8 strings")
}

fn is_void<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.element_name(node).is_some_and(|name| {
        name.namespace == names::HTML_NAMESPACE && VOID.contains(&&*names::get(name.local))
    })
}

fn atom_namespace(namespace: names::NameId) -> Namespace {
    Namespace::from(&*names::get(namespace))
}

/// An element's name for the serializer. The HTML spec writes the local
/// name of an HTML, SVG or MathML element and the qualified name of any
/// other; html5ever writes the local name (and warns) for every namespace.
/// So another namespace's element goes as an SVG one with its qualified
/// name as the local name: SVG is like any non-HTML namespace for the
/// serializer's other rules (raw text, void elements).
fn element_name(name: &NameIds) -> ParserQualName {
    match name.namespace {
        names::HTML_NAMESPACE | names::SVG_NAMESPACE | names::MATHML_NAMESPACE => {
            ParserQualName::new(
                None,
                atom_namespace(name.namespace),
                LocalName::from(&*names::get(name.local)),
            )
        }
        _ => ParserQualName::new(None, ns!(svg), LocalName::from(qualified(name))),
    }
}

/// An attribute's name for the serializer: html5ever writes the `xml:`,
/// `xmlns:` and `xlink:` ones as HTML says, but `unknown_namespace:` and
/// the local name for any other namespace, where HTML writes the qualified
/// name. That one goes as a name in no namespace.
fn attribute_name(name: &NameIds) -> ParserQualName {
    match name.namespace {
        names::NONE | names::XML_NAMESPACE | names::XMLNS_NAMESPACE | names::XLINK_NAMESPACE => {
            ParserQualName::new(
                None,
                atom_namespace(name.namespace),
                LocalName::from(&*names::get(name.local)),
            )
        }
        _ => ParserQualName::new(None, ns!(), LocalName::from(qualified(name))),
    }
}

fn qualified(name: &NameIds) -> String {
    match name.prefix {
        names::NONE => names::get(name.local).to_string(),
        prefix => format!("{}:{}", names::get(prefix), names::get(name.local)),
    }
}

/// A node's children, or a `<template>`'s contents: what HTML serializes
/// inside an element, each given to `f` until it fails.
fn for_each_content<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    mut f: impl FnMut(NodeRef<'gc>) -> io::Result<()>,
) -> io::Result<()> {
    let mut child = cx.first_child(cx.template_content(node).unwrap_or(node));
    while let Some(at) = child {
        f(at)?;
        child = cx.next_sibling(at);
    }
    Ok(())
}

struct Children<'a, 'b, 'gc>(&'a Cx<'b, 'gc>, NodeRef<'gc>, ShadowRoots<'a, 'gc>);

impl Serialize for Children<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, s: &mut S, _: TraversalScope) -> io::Result<()> {
        write_shadow(self.0, s, self.1, self.2)?;
        for_each_content(self.0, self.1, |child| write_node(self.0, s, child, self.2))
    }
}

struct Node<'a, 'b, 'gc>(&'a Cx<'b, 'gc>, NodeRef<'gc>, ShadowRoots<'a, 'gc>);

impl Serialize for Node<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, s: &mut S, _: TraversalScope) -> io::Result<()> {
        write_node(self.0, s, self.1, self.2)
    }
}

/// `host`'s shadow root, if `shadows` takes it: a `<template>` with its
/// mode and settings (HTML's order: `shadowrootmode`,
/// `shadowrootdelegatesfocus`, `shadowrootserializable`,
/// `shadowrootclonable`), its children inside.
fn write_shadow<'gc, S: Serializer>(
    cx: &Cx<'_, 'gc>,
    s: &mut S,
    host: NodeRef<'gc>,
    shadows: ShadowRoots<'_, 'gc>,
) -> io::Result<()> {
    let Some(root) = cx.shadow_root(host) else {
        return Ok(());
    };
    let Some(init) = cx.shadow_root_init(root) else {
        return Ok(());
    };
    let mode = match init.mode {
        dom::ShadowRootMode::Open => "open",
        dom::ShadowRootMode::Closed => "closed",
        dom::ShadowRootMode::UserAgent => return Ok(()),
    };
    let listed = shadows.listed.iter().any(|at| elise::Gc::ptr_eq(*at, root));
    if !(listed || shadows.serializable && init.serializable) {
        return Ok(());
    }
    let attribute = |local: &str| ParserQualName::new(None, ns!(), LocalName::from(local));
    let mut attrs = vec![(attribute("shadowrootmode"), mode)];
    for (on, name) in [
        (init.delegates_focus, "shadowrootdelegatesfocus"),
        (init.serializable, "shadowrootserializable"),
        (init.clonable, "shadowrootclonable"),
    ] {
        if on {
            attrs.push((attribute(name), ""));
        }
    }
    let template = ParserQualName::new(None, ns!(html), LocalName::from("template"));
    s.start_elem(
        template.clone(),
        attrs
            .iter()
            .map(|(name, value)| -> AttrRef<'_> { (name, value) }),
    )?;
    let mut child = cx.first_child(root);
    while let Some(at) = child {
        write_node(cx, s, at, shadows)?;
        child = cx.next_sibling(at);
    }
    s.end_elem(template)
}

fn write_node<'gc, S: Serializer>(
    cx: &Cx<'_, 'gc>,
    s: &mut S,
    node: NodeRef<'gc>,
    shadows: ShadowRoots<'_, 'gc>,
) -> io::Result<()> {
    let data = || {
        cx.data(node)
            .map(|data| data.to_str().into_owned())
            .unwrap_or_default()
    };
    match cx.kind(node) {
        NodeKind::Element => {
            let name = element_name(&cx.element_name(node).expect("an element"));
            let attrs: Vec<(ParserQualName, String)> = (0..cx.attribute_count(node))
                .filter_map(|index| cx.attribute_at(node, index))
                .map(|attr| (attribute_name(&attr.name), attr.value.to_str().into_owned()))
                .collect();
            s.start_elem(
                name.clone(),
                attrs
                    .iter()
                    .map(|(name, value)| -> AttrRef<'_> { (name, value) }),
            )?;
            // HTML writes nothing of what a void element holds (script can
            // give it children). html5ever leaves out the end tag and child
            // elements, but not text, so the contents aren't walked at all.
            if !is_void(cx, node) {
                write_shadow(cx, s, node, shadows)?;
                for_each_content(cx, node, |child| write_node(cx, s, child, shadows))?;
            }
            s.end_elem(name)
        }
        NodeKind::Text => s.write_text(&data()),
        NodeKind::Comment => s.write_comment(&data()),
        NodeKind::ProcessingInstruction => {
            let target = cx
                .target(node)
                .map(|target| target.to_str().into_owned())
                .unwrap_or_default();
            s.write_processing_instruction(&target, &data())
        }
        NodeKind::DocumentType => {
            s.write_doctype(&cx.doctype(node).expect("a doctype").name.to_str())
        }
        // `serialize_node` takes neither, and they are no node's child; an
        // `Attr` is in no tree.
        NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Attr => {
            unreachable!("a document, a fragment or an attr is no node's child")
        }
    }
}
