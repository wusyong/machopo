//! The lists script holds besides collections: a list of nodes (DOM's
//! `NodeList`: static, `querySelectorAll`'s, Blink's `StaticNodeList`; or
//! live, a node's children, `childNodes`, Blink's `ChildNodeList`, or found
//! at each ask, a `RadioNodeList` or a `labels`, Blink's `LabelsNodeList`), and
//! an element's `classList`, `dataset` and `style` (Blink's
//! `DOMTokenList`, `DatasetDOMStringMap` and `InlineCSSStyleDeclaration`),
//! each the same one each time it is asked for, kept in the element's
//! data, reading the element at each ask.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member};
use gfx::RectF;

use crate::{Cx, NameId, Node, NodeRef, names, sites};

/// `Node::scope`'s: the node has its `childNodes` (a live list whose cache
/// its children's changes clear). After `shadow`'s and the custom element
/// state's.
pub(crate) const HAS_CHILD_LIST: u8 = 1 << 6;

/// `ChildList::length`: not known.
const UNKNOWN: u32 = u32::MAX;

/// Nodes as they were when it was made (`querySelectorAll`'s), or a node's
/// children as they are (`childNodes`).
#[derive(GcObject)]
pub struct NodeList {
    /// The static list's nodes; none when empty.
    nodes: Member<HeapVector<Node>>,
    /// The live list's.
    children: Member<ChildList>,
    /// A `RadioNodeList`'s: a form's elements by name (`form`).
    radio: Member<crate::RadioList>,
    /// A `labels`': the element whose labels it is (`form`).
    labeled: Member<Node>,
    /// `getElementsByName`'s: the collection that finds its elements.
    collection: Member<crate::HtmlCollection>,
    wrapper: JsEdge,
}

/// A node's children as a list (Blink's `ChildNodeList` and its
/// `CollectionIndexCache`): the last node it was asked for and its index,
/// and its length once counted, cleared as the children change (the
/// node's `HAS_CHILD_LIST`), so that walking it in order is a step a
/// child, not a walk from the first each time.
#[derive(GcObject)]
pub struct ChildList {
    parent: Member<Node>,
    cached: Member<Node>,
    cached_index: Cell<u32>,
    length: Cell<u32>,
}

/// An element's `class` attribute as a set of tokens (`classList`).
#[derive(GcObject)]
pub struct DomTokenList {
    element: Member<Node>,
    /// The attribute whose tokens it is (a plain local name's number:
    /// `class`, a form's `rel`, an output's `for`; Blink's
    /// `DOMTokenList::attribute_name_`).
    attribute: Cell<NameId>,
    wrapper: JsEdge,
}

/// An element's `style` attribute's declarations (`element.style`,
/// Blink's `InlineCSSStyleDeclaration`), a style rule's or keyframe's (its
/// `style`, Blink's `StyleRuleCSSStyleDeclaration`), or an element's
/// computed style (`getComputedStyle`, Blink's
/// `CSSComputedStyleDeclaration`: `computed` says whose, a `StyleTarget`'s
/// number; 0 for the others).
#[derive(GcObject)]
pub struct CssStyleDeclaration {
    element: Member<Node>,
    rule: Member<crate::CssRule>,
    computed: Cell<u8>,
    wrapper: JsEdge,
}

/// Whose declarations a `CSSStyleDeclaration` is.
#[derive(Clone, Copy)]
pub enum DeclarationOwner<'gc> {
    Element(NodeRef<'gc>),
    Rule(Gc<'gc, crate::CssRule>),
    /// An element's computed style, or one of its pseudo-elements': read
    /// only.
    Computed(NodeRef<'gc>, StyleTarget),
}

/// Which style a computed declaration shows: the element's own, one of
/// its pseudo-elements' that style keeps, its placeholder's, its
/// `::selection`'s, or one of a pseudo-element machopo doesn't have (no style
/// at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleTarget {
    Element,
    Pseudo(computed_style::PseudoId),
    Placeholder,
    Selection,
    OtherPseudo,
}

impl StyleTarget {
    fn number(self) -> u8 {
        use computed_style::PseudoId;
        match self {
            Self::Element => 1,
            Self::Pseudo(PseudoId::Before) => 2,
            Self::Pseudo(PseudoId::After) => 3,
            Self::Pseudo(PseudoId::Marker) => 4,
            Self::Placeholder => 5,
            Self::Selection => 6,
            Self::OtherPseudo => 7,
        }
    }

    fn of(number: u8) -> Option<Self> {
        use computed_style::PseudoId;
        Some(match number {
            1 => Self::Element,
            2 => Self::Pseudo(PseudoId::Before),
            3 => Self::Pseudo(PseudoId::After),
            4 => Self::Pseudo(PseudoId::Marker),
            5 => Self::Placeholder,
            6 => Self::Selection,
            7 => Self::OtherPseudo,
            _ => return None,
        })
    }
}

/// A rectangle script was given (`DOMRect`: `getBoundingClientRect`'s;
/// `DOMRectReadOnly`: an observer entry's), CSS px.
#[derive(GcObject)]
pub struct DomRect {
    x: Cell<f64>,
    y: Cell<f64>,
    width: Cell<f64>,
    height: Cell<f64>,
    /// A `DOMRectReadOnly`, whose values script can't set.
    read_only: bool,
    wrapper: JsEdge,
}

/// Rectangles as they were when it was made (`DOMRectList`:
/// `getClientRects`'s).
#[derive(GcObject)]
pub struct DomRectList {
    /// None when empty.
    rects: Member<HeapVector<DomRect>>,
    wrapper: JsEdge,
}

/// A document's `implementation` (DOM's `DOMImplementation`, Blink's): what
/// makes doctypes and documents for it.
#[derive(GcObject)]
pub struct DomImplementation {
    document: Member<Node>,
    wrapper: JsEdge,
}

/// An element's `data-*` attributes by name (`dataset`).
#[derive(GcObject)]
pub struct DomStringMap {
    element: Member<Node>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new static list of `nodes`.
    pub fn static_node_list(&self, nodes: &[NodeRef<'gc>]) -> Gc<'gc, NodeList> {
        let vector = (!nodes.is_empty()).then(|| {
            let vector = HeapVector::new(self.gc, nodes.len());
            for &node in nodes {
                vector.push(self.gc, node);
            }
            vector
        });
        self.gc.alloc_in(
            &sites::OTHER,
            NodeListInit {
                nodes: vector,
                children: None,
                radio: None,
                labeled: None,
                collection: None,
                wrapper: (),
            },
        )
    }

    /// `document`'s `implementation`: the same each time (Blink's
    /// `Document::implementation`).
    pub fn dom_implementation(&self, document: NodeRef<'gc>) -> Gc<'gc, DomImplementation> {
        let data = self.ensure_document_data(document);
        if let Some(implementation) = data.implementation.get(self.gc) {
            return implementation;
        }
        let implementation = self.gc.alloc_in(
            &sites::OTHER,
            DomImplementationInit {
                document: Some(document),
                wrapper: (),
            },
        );
        data.implementation.set(self.gc, Some(implementation));
        implementation
    }

    /// The document whose `implementation` it is.
    pub fn dom_implementation_document(
        &self,
        implementation: Gc<'gc, DomImplementation>,
    ) -> NodeRef<'gc> {
        implementation
            .document
            .get(self.gc)
            .expect("an implementation's document")
    }

    pub(crate) fn dom_implementation_wrapper_edge(
        &self,
        implementation: Gc<'gc, DomImplementation>,
    ) -> &'gc JsEdge {
        &implementation.into_ref().wrapper
    }

    /// `node`'s `childNodes`: the same each time.
    pub fn child_nodes(&self, node: NodeRef<'gc>) -> Gc<'gc, NodeList> {
        let rare = self.node_rare(node, true).expect("any node has rare data");
        if let Some(list) = rare.child_nodes.get(self.gc) {
            return list;
        }
        let children = self.gc.alloc_beside(
            node,
            &sites::OTHER,
            ChildListInit {
                parent: Some(node),
                cached: None,
                cached_index: Cell::new(0),
                length: Cell::new(UNKNOWN),
            },
        );
        let list = self.gc.alloc_in(
            &sites::OTHER,
            NodeListInit {
                nodes: None,
                children: Some(children),
                radio: None,
                labeled: None,
                collection: None,
                wrapper: (),
            },
        );
        rare.child_nodes.set(self.gc, Some(list));
        node.scope.set(node.scope.get() | HAS_CHILD_LIST);
        list
    }

    /// `parent`'s children changed (it has a `childNodes`): its cache
    /// cleared.
    pub(crate) fn child_list_changed(&self, parent: NodeRef<'gc>) {
        let Some(list) = self
            .node_rare(parent, false)
            .and_then(|rare| rare.child_nodes.get(self.gc))
        else {
            return;
        };
        let children = list.children.get(self.gc).expect("`childNodes` is live");
        children.cached.set(self.gc, None);
        children.length.set(UNKNOWN);
    }

    /// `length`.
    pub fn node_list_length(&self, list: Gc<'gc, NodeList>) -> u32 {
        if let Some(children) = list.children.get(self.gc) {
            return self.child_list_length(children.into_ref());
        }
        let mut count = 0u32;
        if self.for_each_found(list, |_| {
            count = count.saturating_add(1);
            true
        }) {
            return count;
        }
        list.nodes.get(self.gc).map_or(0, |nodes| {
            u32::try_from(nodes.len())
                .expect("a list of nodes is no longer than the heap's u32 ids")
        })
    }

    /// `item(index)`: the node at `index`, if there is one.
    pub fn node_list_item(&self, list: Gc<'gc, NodeList>, index: u32) -> Option<NodeRef<'gc>> {
        if let Some(children) = list.children.get(self.gc) {
            return self.child_list_item(children.into_ref(), index);
        }
        let (mut left, mut item) = (index, None);
        if self.for_each_found(list, |node| match left.checked_sub(1) {
            Some(rest) => {
                left = rest;
                true
            }
            None => {
                item = Some(node);
                false
            }
        }) {
            return item;
        }
        let index = usize::try_from(index).ok()?;
        list.nodes.get(self.gc)?.get(self.gc, index)
    }

    /// How many children: counted on from the cached child, once.
    fn child_list_length(&self, list: &ChildList) -> u32 {
        if list.length.get() != UNKNOWN {
            return list.length.get();
        }
        let (mut at, mut count) = match list.cached.get(self.gc) {
            Some(cached) => (Some(cached), list.cached_index.get()),
            None => (
                list.parent
                    .get(self.gc)
                    .and_then(|parent| self.first_child(parent)),
                0,
            ),
        };
        while let Some(node) = at {
            // A list's nodes are in the heap: fewer than its u32 ids.
            count = count.saturating_add(1);
            at = self.next_sibling(node);
        }
        list.length.set(count);
        count
    }

    /// The child at `index`: from the cached one, or the first or (the
    /// length known) the last, whichever is nearest (Blink's
    /// `CollectionIndexCache::NodeAt`).
    fn child_list_item(&self, list: &ChildList, index: u32) -> Option<NodeRef<'gc>> {
        let parent = list.parent.get(self.gc).expect("a child list's node");
        let length = list.length.get();
        if length != UNKNOWN && index >= length {
            return None;
        }
        let (mut at, mut at_index) = match list.cached.get(self.gc) {
            Some(cached) if index >= list.cached_index.get() => (cached, list.cached_index.get()),
            Some(cached) if index > list.cached_index.get() / 2 => {
                (cached, list.cached_index.get())
            }
            _ if length != UNKNOWN && index > length / 2 => {
                (self.last_child(parent)?, length.saturating_sub(1))
            }
            _ => (self.first_child(parent)?, 0),
        };
        while at_index < index {
            match self.next_sibling(at) {
                Some(next) => at = next,
                None => {
                    list.length.set(at_index.saturating_add(1));
                    return None;
                }
            }
            at_index = at_index.saturating_add(1);
        }
        while at_index > index {
            at = self
                .previous_sibling(at)
                .expect("a child before the cached one's index");
            at_index = at_index.saturating_sub(1);
        }
        list.cached.set(self.gc, Some(at));
        list.cached_index.set(index);
        Some(at)
    }

    /// `element`'s `classList`: the same each time.
    pub fn class_list(&self, element: NodeRef<'gc>) -> Gc<'gc, DomTokenList> {
        let data = self.element_data(element);
        if let Some(list) = data.class_list.get(self.gc) {
            return list;
        }
        let list = self.gc.alloc_in(
            &sites::OTHER,
            DomTokenListInit {
                element: Some(element),
                attribute: Cell::new(names::CLASS),
                wrapper: (),
            },
        );
        data.class_list.set(self.gc, Some(list));
        list
    }

    /// A form's `relList` or an output's `htmlFor` (`local` the attribute:
    /// `rel`, `for`): the same each time.
    pub fn attribute_token_list(
        &self,
        element: NodeRef<'gc>,
        local: NameId,
    ) -> Gc<'gc, DomTokenList> {
        let association = self.association_or_new(element);
        if let Some(list) = association.token_list.get(self.gc) {
            return list;
        }
        let list = self.gc.alloc_in(
            &sites::OTHER,
            DomTokenListInit {
                element: Some(element),
                attribute: Cell::new(local),
                wrapper: (),
            },
        );
        association.token_list.set(self.gc, Some(list));
        list
    }

    /// The attribute whose tokens `list` is (`class`'s is the class table's).
    pub fn token_list_attribute(&self, list: Gc<'gc, DomTokenList>) -> NameId {
        list.attribute.get()
    }

    /// The element whose `class` attribute `list` is.
    pub fn token_list_element(&self, list: Gc<'gc, DomTokenList>) -> NodeRef<'gc> {
        list.element.get(self.gc).expect("a token list's element")
    }

    /// `element`'s `dataset`: the same each time.
    pub fn dataset(&self, element: NodeRef<'gc>) -> Gc<'gc, DomStringMap> {
        let data = self.element_data(element);
        if let Some(map) = data.dataset.get(self.gc) {
            return map;
        }
        let map = self.gc.alloc_in(
            &sites::OTHER,
            DomStringMapInit {
                element: Some(element),
                wrapper: (),
            },
        );
        data.dataset.set(self.gc, Some(map));
        map
    }

    /// `element`'s `style`: the same each time.
    pub fn style_declaration(&self, element: NodeRef<'gc>) -> Gc<'gc, CssStyleDeclaration> {
        let data = self.element_data(element);
        if let Some(declaration) = data.style_declaration.get(self.gc) {
            return declaration;
        }
        let declaration = self.gc.alloc_in(
            &sites::OTHER,
            CssStyleDeclarationInit {
                element: Some(element),
                rule: None,
                computed: Cell::new(0),
                wrapper: (),
            },
        );
        data.style_declaration.set(self.gc, Some(declaration));
        declaration
    }

    /// A new declaration of `element`'s computed style, `target`'s
    /// (`getComputedStyle`: `[NewObject]`, live).
    pub fn new_computed_style_declaration(
        &self,
        element: NodeRef<'gc>,
        target: StyleTarget,
    ) -> Gc<'gc, CssStyleDeclaration> {
        self.gc.alloc_in(
            &sites::OTHER,
            CssStyleDeclarationInit {
                element: Some(element),
                rule: None,
                computed: Cell::new(target.number()),
                wrapper: (),
            },
        )
    }

    /// A new declaration of `rule`'s (`cssom`'s, which keeps it).
    pub(crate) fn new_rule_style_declaration(
        &self,
        rule: Gc<'gc, crate::CssRule>,
    ) -> Gc<'gc, CssStyleDeclaration> {
        self.gc.alloc_in(
            &sites::OTHER,
            CssStyleDeclarationInit {
                element: None,
                rule: Some(rule),
                computed: Cell::new(0),
                wrapper: (),
            },
        )
    }

    /// Whose declarations `declaration` is.
    pub fn style_declaration_owner(
        &self,
        declaration: Gc<'gc, CssStyleDeclaration>,
    ) -> DeclarationOwner<'gc> {
        match declaration.element.get(self.gc) {
            Some(element) => match StyleTarget::of(declaration.computed.get()) {
                Some(target) => DeclarationOwner::Computed(element, target),
                None => DeclarationOwner::Element(element),
            },
            None => DeclarationOwner::Rule(
                declaration
                    .rule
                    .get(self.gc)
                    .expect("a declaration's element or rule"),
            ),
        }
    }

    pub(crate) fn style_declaration_wrapper_edge(
        &self,
        declaration: Gc<'gc, CssStyleDeclaration>,
    ) -> &'gc JsEdge {
        &declaration.into_ref().wrapper
    }

    /// A new `DOMRect` of `rect`.
    pub fn new_rect(&self, rect: RectF) -> Gc<'gc, DomRect> {
        self.new_rect_of(rect_values(rect), false)
    }

    /// A new `DOMRectReadOnly` of `rect`.
    pub fn new_read_only_rect(&self, rect: RectF) -> Gc<'gc, DomRect> {
        self.new_rect_of(rect_values(rect), true)
    }

    /// A new rectangle of x, y, width and height, a `DOMRectReadOnly` if
    /// `read_only`.
    pub fn new_rect_of(
        &self,
        [x, y, width, height]: [f64; 4],
        read_only: bool,
    ) -> Gc<'gc, DomRect> {
        self.gc.alloc_in(
            &sites::OTHER,
            DomRectInit {
                x: Cell::new(x),
                y: Cell::new(y),
                width: Cell::new(width),
                height: Cell::new(height),
                read_only,
                wrapper: (),
            },
        )
    }

    /// Whether `rect` is a `DOMRectReadOnly`.
    pub fn is_read_only_rect(&self, rect: Gc<'gc, DomRect>) -> bool {
        rect.read_only
    }

    /// `rect`'s x, y, width and height.
    pub fn rect(&self, rect: Gc<'gc, DomRect>) -> [f64; 4] {
        [
            rect.x.get(),
            rect.y.get(),
            rect.width.get(),
            rect.height.get(),
        ]
    }

    /// Sets `rect`'s x, y, width or height (`index` 0 to 3).
    pub fn set_rect_value(&self, rect: Gc<'gc, DomRect>, index: usize, value: f64) {
        let cell = match index {
            0 => &rect.x,
            1 => &rect.y,
            2 => &rect.width,
            3 => &rect.height,
            _ => unreachable!("a rect has four values"),
        };
        cell.set(value);
    }

    /// A new list of new `DOMRect`s of `rects`.
    pub fn new_rect_list(&self, rects: &[RectF]) -> Gc<'gc, DomRectList> {
        let vector = (!rects.is_empty()).then(|| {
            let vector = HeapVector::new(self.gc, rects.len());
            for &rect in rects {
                vector.push(self.gc, self.new_rect(rect));
            }
            vector
        });
        self.gc.alloc_in(
            &sites::OTHER,
            DomRectListInit {
                rects: vector,
                wrapper: (),
            },
        )
    }

    pub fn rect_list_length(&self, list: Gc<'gc, DomRectList>) -> u32 {
        list.rects.get(self.gc).map_or(0, |rects| {
            u32::try_from(rects.len())
                .expect("a list of rects is no longer than the heap's u32 ids")
        })
    }

    pub fn rect_list_item(
        &self,
        list: Gc<'gc, DomRectList>,
        index: u32,
    ) -> Option<Gc<'gc, DomRect>> {
        let index = usize::try_from(index).ok()?;
        list.rects.get(self.gc)?.get(self.gc, index)
    }

    pub(crate) fn rect_wrapper_edge(&self, rect: Gc<'gc, DomRect>) -> &'gc JsEdge {
        &rect.into_ref().wrapper
    }

    pub(crate) fn rect_list_wrapper_edge(&self, list: Gc<'gc, DomRectList>) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    /// The element whose `data-*` attributes `map` is.
    pub fn dataset_element(&self, map: Gc<'gc, DomStringMap>) -> NodeRef<'gc> {
        map.element.get(self.gc).expect("a dataset's element")
    }

    /// A live list of `radio`'s nodes (a `RadioNodeList`).
    pub(crate) fn new_live_node_list(&self, radio: Gc<'gc, crate::RadioList>) -> Gc<'gc, NodeList> {
        self.gc.alloc_in(
            &sites::OTHER,
            NodeListInit {
                nodes: None,
                children: None,
                radio: Some(radio),
                labeled: None,
                collection: None,
                wrapper: (),
            },
        )
    }

    /// A new `labels` of `element`, live.
    pub(crate) fn new_labels_list(&self, element: NodeRef<'gc>) -> Gc<'gc, NodeList> {
        self.gc.alloc_in(
            &sites::OTHER,
            NodeListInit {
                nodes: None,
                children: None,
                radio: None,
                labeled: Some(element),
                collection: None,
                wrapper: (),
            },
        )
    }

    /// A live list's nodes found at each ask (a `RadioNodeList`'s, a
    /// `labels`'), each given to `f` until it says stop; false for another
    /// list.
    fn for_each_found(&self, list: Gc<'gc, NodeList>, f: impl FnMut(NodeRef<'gc>) -> bool) -> bool {
        if let Some(radio) = list.radio.get(self.gc) {
            self.for_each_radio_list_node(radio, f);
            return true;
        }
        if let Some(element) = list.labeled.get(self.gc) {
            self.for_each_label(element, f);
            return true;
        }
        if let Some(collection) = list.collection.get(self.gc) {
            self.collection_walk(collection, f);
            return true;
        }
        false
    }

    /// `getElementsByName(name)` (HTML's): a live list of `document`'s HTML
    /// elements whose `name` is `name`, a new one each time.
    pub fn elements_by_name_list(&self, document: NodeRef<'gc>, name: &str) -> Gc<'gc, NodeList> {
        let collection = self.elements_by_name(document, name);
        self.gc.alloc_in(
            &sites::OTHER,
            NodeListInit {
                nodes: None,
                children: None,
                radio: None,
                labeled: None,
                collection: Some(collection),
                wrapper: (),
            },
        )
    }

    /// A `RadioNodeList`'s radio list.
    pub(crate) fn node_list_radio(
        &self,
        list: Gc<'gc, NodeList>,
    ) -> Option<Gc<'gc, crate::RadioList>> {
        list.radio.get(self.gc)
    }

    /// Whether `list` is a `RadioNodeList`.
    pub fn is_radio_node_list(&self, list: Gc<'gc, NodeList>) -> bool {
        list.radio.get(self.gc).is_some()
    }

    pub(crate) fn node_list_wrapper_edge(&self, list: Gc<'gc, NodeList>) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    pub(crate) fn token_list_wrapper_edge(&self, list: Gc<'gc, DomTokenList>) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    pub(crate) fn string_map_wrapper_edge(&self, map: Gc<'gc, DomStringMap>) -> &'gc JsEdge {
        &map.into_ref().wrapper
    }
}

fn rect_values(rect: RectF) -> [f64; 4] {
    [
        f64::from(rect.x),
        f64::from(rect.y),
        f64::from(rect.width),
        f64::from(rect.height),
    ]
}
