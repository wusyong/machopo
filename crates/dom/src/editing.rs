//! HTML's editability (https://html.spec.whatwg.org/#contenteditable),
//! from the DOM, as Blink's `IsEditableOrEditingHost` (what its
//! `isContentEditable` and `:read-write` read): the nearest HTML element
//! whose `contenteditable` isn't in the inherit state decides, through
//! the elements a parent's editability reaches. Blink's editing reads the
//! same from style (`-webkit-user-modify`, which the attribute maps to);
//! the two differ only for what machopo hasn't (that property written by an
//! author, `inert`), so focus, the caret and the selection read this too.
//! machopo has no `designMode`.

use crate::{Cx, NodeKind, NodeRef, QualifiedName, names};

/// A `contenteditable` attribute's state (its keywords ASCII
/// case-insensitively, the empty string true; any other value inherit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentEditable {
    True,
    False,
    PlaintextOnly,
    Inherit,
}

impl ContentEditable {
    pub fn of(value: &str) -> Self {
        if value.is_empty() || value.eq_ignore_ascii_case("true") {
            Self::True
        } else if value.eq_ignore_ascii_case("false") {
            Self::False
        } else if value.eq_ignore_ascii_case("plaintext-only") {
            Self::PlaintextOnly
        } else {
            Self::Inherit
        }
    }

    /// The keyword `contentEditable` gives for it.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::PlaintextOnly => "plaintext-only",
            Self::Inherit => "inherit",
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s `contenteditable` state if it is an HTML element (inherit
    /// without the attribute); `None` for any other node.
    pub fn content_editable(&self, element: NodeRef<'gc>) -> Option<ContentEditable> {
        if self.qualified_name(element)?.namespace() != names::HTML_NAMESPACE {
            return None;
        }
        Some(
            self.attribute_value_by_id(element, names::local::CONTENTEDITABLE)
                .map_or(ContentEditable::Inherit, |value| {
                    ContentEditable::of(&value.borrow().to_str())
                }),
        )
    }

    /// Whether `node` is editable or an editing host: an HTML element
    /// whose `contenteditable` is true or plaintext-only is; false isn't;
    /// any other node is if its parent is and it is an HTML element, an
    /// `<svg>` or a `<math>`, or not an element and its parent an HTML
    /// element (the editing specification's "editable", Blink's
    /// `IsEditableOrEditingHost`).
    pub fn is_editable(&self, node: NodeRef<'gc>) -> bool {
        self.editable_from(node).is_some()
    }

    /// The element whose `contenteditable` makes `node` editable (it, or an
    /// ancestor), if it is.
    fn editable_from(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut at = node;
        loop {
            match self.content_editable(at) {
                Some(ContentEditable::True | ContentEditable::PlaintextOnly) => return Some(at),
                Some(ContentEditable::False) => return None,
                Some(ContentEditable::Inherit) | None => {}
            }
            let parent = self.parent(at)?;
            let reached = match self.qualified_name(at) {
                Some(name) => {
                    name.namespace() == names::HTML_NAMESPACE
                        || name == QualifiedName::svg(names::local::SVG)
                        || name.is(names::MATHML_NAMESPACE, names::local::MATH)
                }
                None => {
                    self.kind(at) != NodeKind::Element
                        && self
                            .qualified_name(parent)
                            .is_some_and(|name| name.namespace() == names::HTML_NAMESPACE)
                }
            };
            if !reached {
                return None;
            }
            at = parent;
        }
    }

    /// Whether `node` is an editing host as Blink has it (its
    /// `IsRootEditableElement`): an editable element whose parent isn't
    /// editable. HTML also calls an element whose `contenteditable` is true
    /// inside an editable one an editing host; Chrome takes the outermost.
    /// (What isn't an element is editable only with its parent.)
    pub fn is_editing_host(&self, node: NodeRef<'gc>) -> bool {
        self.is_editable(node)
            && !self
                .parent(node)
                .is_some_and(|parent| self.is_editable(parent))
    }

    /// Whether a caret can't be in `element`'s content (Blink's
    /// `EditingIgnoresContent`, its elements whose `CanContainRangeEndPoint`
    /// is false): a replaced element, a form control's, or `<hr>`; a caret
    /// goes before or after it.
    pub fn editing_ignores_content(&self, element: NodeRef<'gc>) -> bool {
        use names::local;
        let Some(name) = self.qualified_name(element) else {
            return false;
        };
        name.namespace() == names::HTML_NAMESPACE
            && [
                local::AUDIO,
                local::CANVAS,
                local::EMBED,
                local::HR,
                local::IFRAME,
                local::IMG,
                local::INPUT,
                local::METER,
                local::OBJECT,
                local::PROGRESS,
                local::SELECT,
                local::TEXTAREA,
                local::VIDEO,
            ]
            .contains(&name.local())
    }

    /// The editing host `node` is in (it, if it is one): its outermost
    /// editable inclusive ancestor element (Blink's `RootEditableElement`);
    /// none if it isn't editable. Each element that makes the one below
    /// editable, outward, until one's parent isn't: one walk up (a text's
    /// starts at it, as its parent's would).
    pub fn editing_host_of(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut host = self.editable_from(node)?;
        while let Some(parent) = self.parent(host)
            && let Some(from) = self.editable_from(parent)
        {
            host = from;
        }
        Some(host)
    }
}
