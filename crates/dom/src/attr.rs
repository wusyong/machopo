//! DOM's `Attr` nodes (Blink's `Attr` and `AttrNodeList`): an attribute as
//! a node, made the first time script asks for one (an element's
//! attributes are its list of `Attribute`s, not nodes). An `Attr` holds
//! its attribute object and, while the attribute is one of an element's,
//! that element; the element keeps the `Attr`s made for its attributes
//! (`ElementData::attr_nodes`), so it is the same one each time. Taken off
//! its element (`removeAttribute`, replaced by `setAttributeNode`), an
//! `Attr` keeps its attribute object, so its value is what it was; one
//! made by `createAttribute` has none until it is put on one.

use std::cell::RefCell;

use elise::{Gc, GcObject, HeapVector, LocalExternal, Member};

use crate::{
    Attribute, AttributeInit, Cx, DomError, DomString, NameIds, Node, NodeKind, NodeRareData,
    NodeRef, QualName, QualifiedName, StrRef, sites, string,
};

/// An `Attr`'s data: its element while it is one of its attributes, and
/// its attribute; its listeners.
#[derive(GcObject)]
pub struct AttrData {
    element: Member<Node>,
    attribute: Member<Attribute>,
    pub(crate) rare: Member<NodeRareData>,
}

impl<'gc> Cx<'_, 'gc> {
    fn attr_data(&self, attr: NodeRef<'gc>) -> Gc<'gc, AttrData> {
        attr.data
            .get(self.gc)
            .and_then(|data| data.downcast::<AttrData>())
            .expect("an Attr's data")
    }

    fn attr_attribute(&self, attr: NodeRef<'gc>) -> Gc<'gc, Attribute> {
        self.attr_data(attr)
            .attribute
            .get(self.gc)
            .expect("an Attr's attribute")
    }

    /// A new `Attr` node of `document`'s, of no element (`createAttribute`).
    pub fn create_attr(
        &self,
        document: NodeRef<'gc>,
        name: QualName<'_>,
        value: &str,
    ) -> NodeRef<'gc> {
        let name = QualifiedName::new(self.intern_name(name));
        let attribute = self.gc.alloc_in(
            &sites::ATTRIBUTE,
            AttributeInit {
                name: std::cell::Cell::new(name.raw()),
                value: LocalExternal::new(RefCell::new(string::intern_value(value))),
            },
        );
        self.new_attr_node(self.document_of(document), None, attribute)
    }

    fn new_attr_node(
        &self,
        document: NodeRef<'gc>,
        element: Option<NodeRef<'gc>>,
        attribute: Gc<'gc, Attribute>,
    ) -> NodeRef<'gc> {
        let node = self.node(NodeKind::Attr, Some(document), &sites::OTHER);
        let data = self.gc.alloc_beside(
            node,
            &sites::OTHER,
            AttrDataInit {
                element,
                attribute: Some(attribute),
                rare: None,
            },
        );
        node.data.set(self.gc, Some(data.erase()));
        node
    }

    /// The `Attr` of `element`'s attribute at `index`, made the first time
    /// (the same one after); none past its last.
    pub fn attr_node_at(&self, element: NodeRef<'gc>, index: usize) -> Option<NodeRef<'gc>> {
        self.synchronize_attributes(element);
        let attribute = element.attributes.get(self.gc)?.get(self.gc, index)?;
        Some(self.attr_node_of(element, attribute))
    }

    /// `getAttributeNode(qualifiedName)` (lowercased first by the caller
    /// for an HTML element in an HTML document).
    pub fn attr_node(&self, element: NodeRef<'gc>, qualified: &str) -> Option<NodeRef<'gc>> {
        let (_, attribute) = self.attribute_by_name(element, qualified)?;
        Some(self.attr_node_of(element, attribute))
    }

    /// `getAttributeNodeNS(namespace, localName)`.
    pub fn attr_node_ns(
        &self,
        element: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<NodeRef<'gc>> {
        let (_, attribute) = self.attribute_ns(element, namespace, local)?;
        Some(self.attr_node_of(element, attribute))
    }

    /// The `Attr` the element keeps for `attribute`, made if it has none.
    fn attr_node_of(&self, element: NodeRef<'gc>, attribute: Gc<'gc, Attribute>) -> NodeRef<'gc> {
        if let Some(attr) = self.kept_attr_node(element, attribute) {
            return attr;
        }
        let attr = self.new_attr_node(self.document_of(element), Some(element), attribute);
        self.keep_attr_node(element, attr);
        attr
    }

    fn kept_attr_node(
        &self,
        element: NodeRef<'gc>,
        attribute: Gc<'gc, Attribute>,
    ) -> Option<NodeRef<'gc>> {
        let kept = self
            .existing_element_data(element)?
            .attr_nodes
            .get(self.gc)?;
        (0..kept.len())
            .filter_map(|index| kept.get(self.gc, index))
            .find(|&attr| Gc::ptr_eq(self.attr_attribute(attr), attribute))
    }

    fn keep_attr_node(&self, element: NodeRef<'gc>, attr: NodeRef<'gc>) {
        let data = self.element_data(element);
        let kept = match data.attr_nodes.get(self.gc) {
            Some(kept) => kept,
            None => {
                let kept = HeapVector::new_beside(self.gc, element, 1);
                data.attr_nodes.set(self.gc, Some(kept));
                kept
            }
        };
        kept.push(self.gc, attr);
    }

    /// `attribute`, one of `element`'s no longer: its `Attr` (if it has
    /// one) taken off the element, keeping the attribute (its value).
    pub(crate) fn detach_attr_node(&self, element: NodeRef<'gc>, attribute: Gc<'gc, Attribute>) {
        let Some(kept) = self
            .existing_element_data(element)
            .and_then(|data| data.attr_nodes.get(self.gc))
        else {
            return;
        };
        let at = (0..kept.len()).find(|&index| {
            kept.get(self.gc, index)
                .is_some_and(|attr| Gc::ptr_eq(self.attr_attribute(attr), attribute))
        });
        if let Some(at) = at {
            let attr = kept.remove(self.gc, at);
            self.attr_data(attr).element.set(self.gc, None);
        }
    }

    /// An `Attr`'s name.
    pub fn attr_name(&self, attr: NodeRef<'gc>) -> NameIds {
        self.attr_attribute(attr).qualified_name().ids()
    }

    /// An `Attr`'s value, where it is.
    pub fn attr_value(&self, attr: NodeRef<'gc>) -> StrRef<'gc> {
        StrRef(self.attr_attribute(attr).into_ref().value.get())
    }

    /// An `Attr`'s element (`ownerElement`), while it is one of its
    /// attributes.
    pub fn attr_owner(&self, attr: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.attr_data(attr).element.get(self.gc)
    }

    /// Setting an `Attr`'s value (DOM's "set an existing attribute
    /// value"): through its element's attribute change steps while it has
    /// one, else its own.
    pub fn set_attr_value(&self, attr: NodeRef<'gc>, value: &str) {
        let attribute = self.attr_attribute(attr);
        match self.attr_owner(attr) {
            Some(element) => self.change_attribute(element, attribute, value),
            None => *attribute.value.get().borrow_mut() = string::intern_value(value),
        }
    }

    /// DOM's "set an attribute" (`setAttributeNode`, `setAttributeNodeNS`):
    /// `attr` put on `element`, in place of the attribute of its namespace
    /// and local name (DOM's "replace an attribute": that one's `Attr`
    /// given back, taken off); an `InUseAttributeError` for one of another
    /// element's; itself for one of `element`'s already.
    pub fn set_attribute_node(
        &self,
        element: NodeRef<'gc>,
        attr: NodeRef<'gc>,
    ) -> Result<Option<NodeRef<'gc>>, DomError> {
        match self.attr_owner(attr) {
            Some(owner) if Gc::ptr_eq(owner, element) => return Ok(Some(attr)),
            Some(_) => return Err(DomError::InUseAttribute),
            None => {}
        }
        let attribute = self.attr_attribute(attr);
        let name = attribute.qualified_name();
        let ids = name.ids();
        let old = self.find_attribute(element, |at| {
            at.qualified_name().is(ids.namespace, ids.local)
        });
        let new_value = attribute.value.get().borrow().clone();
        self.attr_data(attr).element.set(self.gc, Some(element));
        // Of the element's document now (DOM's "append" and "replace").
        attr.owner.set(self.gc, Some(self.document_of(element)));
        let Some((index, old_attribute)) = old else {
            self.append_attribute_object(element, attribute);
            self.keep_attr_node(element, attr);
            return Ok(None);
        };
        let old_attr = self.attr_node_of(element, old_attribute);
        let old_value = old_attribute.value.get().borrow().clone();
        let attributes = element.attributes.get(self.gc).expect("its attributes");
        attributes.set(self.gc, index, attribute);
        self.detach_attr_node(element, old_attribute);
        self.keep_attr_node(element, attr);
        self.attribute_changed(
            element,
            name,
            Some(&old_value.to_str()),
            Some(&new_value.to_str()),
        );
        Ok(Some(old_attr))
    }

    /// `removeAttributeNode(attr)`: `attr` taken off `element`, which it is
    /// one of; else a `NotFoundError`.
    pub fn remove_attribute_node(
        &self,
        element: NodeRef<'gc>,
        attr: NodeRef<'gc>,
    ) -> Result<NodeRef<'gc>, DomError> {
        if !self
            .attr_owner(attr)
            .is_some_and(|owner| Gc::ptr_eq(owner, element))
        {
            return Err(DomError::NotFound);
        }
        let attribute = self.attr_attribute(attr);
        let (index, _) = self
            .find_attribute(element, |at| std::ptr::eq(at, attribute.into_ref()))
            .ok_or(DomError::NotFound)?;
        self.remove_attribute_at(element, index);
        Ok(attr)
    }

    /// `element`'s `Attr`s, its document's now (it was adopted).
    pub(crate) fn adopt_attr_nodes(&self, element: NodeRef<'gc>, document: NodeRef<'gc>) {
        let Some(kept) = self
            .existing_element_data(element)
            .and_then(|data| data.attr_nodes.get(self.gc))
        else {
            return;
        };
        for index in 0..kept.len() {
            if let Some(attr) = kept.get(self.gc, index) {
                attr.owner.set(self.gc, Some(document));
            }
        }
    }

    /// A copy of `attr` (`cloneNode`), of `document`'s, of no element.
    pub(crate) fn clone_attr(&self, attr: NodeRef<'gc>, document: NodeRef<'gc>) -> NodeRef<'gc> {
        let attribute = self.attr_attribute(attr);
        let copy = self.gc.alloc_in(
            &sites::ATTRIBUTE,
            AttributeInit {
                name: std::cell::Cell::new(attribute.name.get()),
                value: LocalExternal::new(RefCell::new(attribute.value.get().borrow().clone())),
            },
        );
        self.new_attr_node(self.document_of(document), None, copy)
    }

    /// The `Attr`'s value as a `DomString` (its node value, its text
    /// content).
    pub fn attr_value_string(&self, attr: NodeRef<'gc>) -> DomString {
        self.attr_attribute(attr).value.get().borrow().clone()
    }
}

/// An element's `attributes` (DOM's `NamedNodeMap`, Blink's): a live view
/// of its attributes, the same each time (the element keeps it).
#[derive(GcObject)]
pub struct NamedNodeMap {
    element: Member<Node>,
    wrapper: elise::JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s `attributes`, made the first time.
    pub fn attribute_map(&self, element: NodeRef<'gc>) -> Gc<'gc, NamedNodeMap> {
        let data = self.element_data(element);
        if let Some(map) = data.attribute_map.get(self.gc) {
            return map;
        }
        let map = self.gc.alloc_in(
            &sites::OTHER,
            NamedNodeMapInit {
                element: Some(element),
                wrapper: (),
            },
        );
        data.attribute_map.set(self.gc, Some(map));
        map
    }

    /// The element whose attributes `map` shows.
    pub fn attribute_map_element(&self, map: Gc<'gc, NamedNodeMap>) -> NodeRef<'gc> {
        map.element.get(self.gc).expect("a map's element")
    }

    pub(crate) fn attribute_map_wrapper_edge(
        &self,
        map: Gc<'gc, NamedNodeMap>,
    ) -> &'gc elise::JsEdge {
        &map.into_ref().wrapper
    }
}

/// A `DOMParser`: the window of the realm it was made in (its relevant
/// global, whose document's URL its documents get).
#[derive(GcObject)]
pub struct DomParser {
    window: Member<crate::Window>,
    wrapper: elise::JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    pub fn create_dom_parser(&self, window: Option<Gc<'gc, crate::Window>>) -> Gc<'gc, DomParser> {
        self.gc.alloc_in(
            &sites::OTHER,
            DomParserInit {
                window,
                wrapper: (),
            },
        )
    }

    pub fn dom_parser_window(&self, parser: Gc<'gc, DomParser>) -> Option<Gc<'gc, crate::Window>> {
        parser.window.get(self.gc)
    }

    pub(crate) fn dom_parser_wrapper_edge(&self, parser: Gc<'gc, DomParser>) -> &'gc elise::JsEdge {
        &parser.into_ref().wrapper
    }
}
