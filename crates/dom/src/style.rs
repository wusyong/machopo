//! What the DOM keeps for style and layout (Blink's `Node`'s style and
//! layout flags, `Element`'s computed style and `ElementData`,
//! `DOMNodeIds`): what changed since they last looked, set as the tree
//! changes and marked up the ancestors to the first marked already, so
//! that style and layout go down only where something did; each element's
//! style, which the style phase writes and layout reads; and the ids that
//! code off the heap names nodes by.
//!
//! Only a connected node (one in a document's tree, Blink's
//! `kIsConnectedFlag`) is marked: a tree built apart is styled whole as it
//! is inserted (its root needs its style computed again, with its
//! subtree), so building one marks nothing. Inserting into a connected
//! tree walks the inserted subtree (Blink's `InsertedInto`), to set it
//! connected and find the stylesheets that came; removing walks it again.
//!
//! An element's style is in a `TokenCell` (the thread's `LocalToken`):
//! read as a reference with the token shared, written with it exclusive,
//! so that no write overlaps a read, with no count and no flag a borrow.

use std::cell::Cell;
use std::ops::BitOr;
use std::sync::Arc;

use computed_style::{ComputedStyle, PseudoStyles};
use css::CssPropertyValueSet;
use elise::{External, Gc, GcObject, HeapVector, LocalToken, Member, TokenCell};
use gfx::NodeId;

use crate::{
    CssStyleDeclaration, Cx, DomStringMap, DomTokenList, NameId, Node, NodeKind, NodeRareData,
    NodeRef, QualifiedName, SlotData, names, sites,
};

/// A node's: it is in a document's tree (DOM's "connected").
pub(crate) const CONNECTED: u32 = 1 << 11;
/// An element's: its `class` attribute changed since its classes were made
/// (`Cx::class_names` makes them again).
const CLASSES_STALE: u32 = 1 << 25;
/// An element's: it has had an `id` attribute (Blink's `ElementData`'s
/// `HasID`), so that a subtree coming and going looks for ids only on
/// elements that may have one. Set as one is added, not cleared: changing
/// an attribute of an element out of a document's tree (most) then does
/// nothing more for ids.
pub(crate) const HAS_ID: u32 = 1 << 26;

const AUTOFOCUS_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::local::AUTOFOCUS);
/// An element's: its data holds its `style` attribute's declarations
/// (`ElementData::inline_style`), so that a change of the attribute
/// clears them. Changing an attribute of any other element looks at this
/// bit alone.
const HAS_INLINE_STYLE: u32 = 1 << 27;
/// An element's: its declarations changed through CSSOM since its `style`
/// attribute was last written (Blink's `style_attribute_is_dirty_`): the
/// attribute is serialized from them when it is next read.
pub(crate) const STYLE_ATTRIBUTE_DIRTY: u32 = 1 << 28;
/// An element's: it is in its document's `:hover` chain, the hover element
/// or an ancestor of it; in its `:active` chain (Blink's `IsHovered`,
/// `IsActive`). Selector matching reads them.
pub(crate) const HOVERED: u32 = 1 << 30;
pub(crate) const ACTIVE: u32 = 1 << 31;
/// An element's (a document's own bits there are for documents only): it
/// is its document's focused element or an ancestor of it (`FOCUS_CHAIN`,
/// whatever the page's focus); and what matches while the page has the
/// system's focus: `:focus` (`FOCUSED`), `:focus-within` (`FOCUS_WITHIN`),
/// `:focus-visible` (`FOCUS_VISIBLE`) (Blink's `IsFocused`,
/// `HasFocusWithin`).
pub(crate) const FOCUS_CHAIN: u32 = 1 << 3;
pub(crate) const FOCUSED: u32 = 1 << 4;
pub(crate) const FOCUS_WITHIN: u32 = 1 << 5;
pub(crate) const FOCUS_VISIBLE: u32 = 1 << 6;
pub(crate) const FOCUS_BITS: u32 = FOCUS_CHAIN | FOCUSED | FOCUS_WITHIN | FOCUS_VISIBLE;
/// Where a node's `StyleFlags` and `LayoutTreeFlags` are in its flags.
const STYLE_SHIFT: u32 = 12;
const LAYOUT_SHIFT: u32 = 20;
/// A new node's: it was never styled.
pub(crate) const NEW: u32 = StyleFlags::NEEDS_STYLE_RECALC.0 << STYLE_SHIFT;

/// The `class` attribute (no namespace, no prefix).
const CLASS_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::CLASS);
const ID_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::ID);
pub(crate) const STYLE_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::local::STYLE);

/// The elements whose sheets are their document's: HTML's and SVG's
/// `<style>`, and `<link>`.
const STYLE_ELEMENT: QualifiedName = QualifiedName::html(names::local::STYLE);
const SVG_STYLE_ELEMENT: QualifiedName = QualifiedName::svg(names::local::STYLE);
const LINK_ELEMENT: QualifiedName = QualifiedName::html(names::local::LINK);

/// What changed of a node for style since the style phase last looked
/// (Blink's `NodeFlags` for style recalc), a bit each. A node whose flags
/// are clear, and whose ancestors' are too, needs no style recalc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleFlags(u32);

impl StyleFlags {
    pub const CLEAN: Self = Self(0);
    /// This node and its subtree need their style computed again
    /// (`kNeedsStyleRecalc` with `kSubtreeStyleChange`): new, or inserted.
    pub const NEEDS_STYLE_RECALC: Self = Self(1);
    /// Something below has a flag set (`kChildNeedsStyleRecalc`): the
    /// style phase goes down this way. Every ancestor of a marked node has
    /// it.
    pub const CHILD_NEEDS_STYLE_RECALC: Self = Self(1 << 1);
    /// Children were inserted or removed, or a child's text changed: what
    /// structural selectors (siblings, `:nth-child`, `:empty`) match below
    /// it may have changed.
    pub const CHILDREN_CHANGED: Self = Self(1 << 2);
    /// A child's attributes changed (which in its `AttributeChanges`): for
    /// selectors across siblings.
    pub const CHILDREN_ATTRIBUTES_CHANGED: Self = Self(1 << 3);
    /// A document's: its stylesheets may have changed (a `<style>` or a
    /// `<link>` came, went or changed), so the style phase collects them
    /// again (Blink's `StyleEngine::MarkTreeScopeDirty`).
    pub const STYLE_SHEETS_CHANGED: Self = Self(1 << 4);
    /// This element's attributes changed, which in its `AttributeChanges`
    /// (`Cx::take_attribute_changes`): styled again, its subtree only if a
    /// rule for elements below uses one of them or what they inherit
    /// changed.
    pub const ATTRIBUTES_CHANGED: Self = Self(1 << 5);
    /// A sibling before it came or went, or the one just before changed
    /// its attributes (Blink's `CheckForSiblingStyleChanges`' node after
    /// the change): what matches it by position (`+`, `~`, `:nth-child`)
    /// may change.
    pub const FOLLOWS_CHANGE: Self = Self(1 << 6);
    /// A sibling after it came or went: `:last-child` and the like may
    /// match it otherwise.
    pub const PRECEDES_CHANGE: Self = Self(1 << 7);
    const ALL: u32 = (1 << 8) - 1;

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_clean(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for StyleFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What changed of a node for its boxes since layout last looked (Blink's
/// `NeedsReattachLayoutTree` and the like), a bit each: set as the tree
/// changes and by the style phase, read and cleared by layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutTreeFlags(u32);

impl LayoutTreeFlags {
    pub const CLEAN: Self = Self(0);
    /// Its boxes, and its subtree's, are built again (inserted, or a text
    /// whose data changed).
    pub const NEEDS_REATTACH: Self = Self(1);
    /// Children were inserted or removed.
    pub const CHILDREN_CHANGED: Self = Self(1 << 1);
    /// Its computed style changed (`Cx::set_style_changed`).
    pub const STYLE_CHANGED: Self = Self(1 << 2);
    /// Its pseudo-elements' styles changed.
    pub const PSEUDO_STYLES_CHANGED: Self = Self(1 << 3);
    /// Something below has a flag set: layout goes down this way.
    pub const CHILD_NEEDS_UPDATE: Self = Self(1 << 4);
    /// A state its native look shows changed (checked, disabled,
    /// hovered...: `AttributeChanges::LOOK`): painted again (Blink's
    /// `LayoutTheme::ControlStateChanged`). In `Node::layout_more`, not
    /// the flag word.
    pub const CONTROL_STATE_CHANGED: Self = Self(1 << 5);
    /// Those in the flag word.
    const ALL: u32 = (1 << 5) - 1;

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_clean(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for LayoutTreeFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What changed of an element's attributes since the style phase last
/// looked (`StyleFlags::ATTRIBUTES_CHANGED`): each attribute's local name
/// set or removed, each class added or removed, and the ids it had and
/// has; and the user action states (`:hover`, `:active`) it entered or
/// left. The style phase reads it to tell whether rules for elements below
/// may match otherwise (Blink's `ClassChangedForElement`,
/// `IdChangedForElement`, `AttributeChangedForElement` and
/// `PseudoStateChanged` ask the rules at once, but the DOM doesn't know
/// them).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttributeChanges {
    pub attributes: Vec<NameId>,
    pub classes: Vec<Box<str>>,
    pub ids: Vec<Box<str>>,
    /// The states (`HOVER`, `CHECKED`...), a bit each.
    pub states: u16,
}

impl AttributeChanges {
    /// `states`' bits.
    pub const HOVER: u16 = 1 << 0;
    pub const ACTIVE: u16 = 1 << 1;
    pub const FOCUS: u16 = 1 << 2;
    pub const FOCUS_WITHIN: u16 = 1 << 3;
    pub const FOCUS_VISIBLE: u16 = 1 << 4;
    /// `:defined`: its custom element state came to be, or stopped being,
    /// uncustomized or custom.
    pub const DEFINED: u16 = 1 << 5;
    /// The form's (what an attribute's change doesn't say): `:checked`, an
    /// input's checkedness; `:indeterminate`, a checkbox's indeterminate or
    /// a radio button's group's checkedness; `:disabled`, `:enabled` and
    /// `:read-write`, a fieldset ancestor's disabling; `:placeholder-shown`,
    /// an input's value; `:default`, a form's default button.
    pub const CHECKED: u16 = 1 << 6;
    pub const INDETERMINATE: u16 = 1 << 7;
    pub const DISABLED: u16 = 1 << 8;
    pub const VALUE: u16 = 1 << 9;
    pub const DEFAULT: u16 = 1 << 10;
    /// `:open`: a select's popup opened or closed.
    pub const OPEN: u16 = 1 << 11;
    /// `:-internal-multi-select-focus`: the option the keys moved a list
    /// box's end to without selecting it.
    pub const MULTI_SELECT_FOCUS: u16 = 1 << 12;
    /// `:target`: its document's target element changed.
    pub const TARGET: u16 = 1 << 13;
    /// The states a form control's native look shows (`ControlState`'s:
    /// hovered, pressed and focused as itself, checked, indeterminate,
    /// disabled as its control).
    pub(crate) const LOOK: u16 = Self::HOVER
        | Self::ACTIVE
        | Self::FOCUS
        | Self::CHECKED
        | Self::INDETERMINATE
        | Self::DISABLED;

    /// Attribute `name` changed from `old` to `new` (none: there was none,
    /// or is none now).
    fn note(&mut self, name: QualifiedName, old: Option<&str>, new: Option<&str>) {
        fn add(list: &mut Vec<Box<str>>, value: &str) {
            if !list.iter().any(|v| &**v == value) {
                list.push(value.into());
            }
        }
        let local = name.local();
        if !self.attributes.contains(&local) {
            self.attributes.push(local);
        }
        if name.is(names::NONE, names::ID) && old != new {
            for id in [old, new].into_iter().flatten() {
                add(&mut self.ids, id);
            }
        }
        if name.is(names::NONE, names::CLASS) {
            fn classes(value: Option<&str>) -> Vec<&str> {
                value.map_or_else(Vec::new, |value| value.split_ascii_whitespace().collect())
            }
            let (old, new) = (classes(old), classes(new));
            for class in old.iter().filter(|c| !new.contains(c)) {
                add(&mut self.classes, class);
            }
            for class in new.iter().filter(|c| !old.contains(c)) {
                add(&mut self.classes, class);
            }
        }
    }
}

/// An element's style, as the style phase worked it out (Blink's
/// `Element::computed_style_` and `ElementData`'s inline and presentation
/// attribute styles): written by it, read by layout and paint.
#[derive(Default)]
pub struct ElementStyle {
    pub computed: Option<Arc<ComputedStyle>>,
    /// Its pseudo-elements' styles; none while it has none.
    pub pseudo: Option<Box<PseudoStyles>>,
    /// Its `style` attribute's declarations, as the style phase parsed them
    /// (`AttributeChanges` says when it changed).
    pub inline: Option<Arc<CssPropertyValueSet>>,
    /// Its presentation attributes' declarations and a digest of the
    /// attributes they are of.
    pub presentation: Option<(Arc<CssPropertyValueSet>, u64)>,
}

/// A class of an element's `class` attribute (Blink's `SpaceSplitString`
/// of `AtomicString`s): its name's number as written, and ASCII
/// lowercase (a quirks mode document matches classes ASCII
/// case-insensitively, and a rule index buckets by the lowercase one),
/// and its `css::selector::name_hash` (what the ancestor bloom filter
/// takes it by).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassName {
    pub name: NameId,
    pub lower: NameId,
    pub hash: u32,
}

/// What style reads of a document's elements (Blink's `RuleFeatureSet`, as
/// `StyleEngine::AttributeChangedForElement`, `ClassChangedForElement` and
/// `PseudoStateChanged` ask it at once): the names some rule of its sheets
/// uses of an element, anywhere in a selector, and those style reads
/// besides (presentational hints, `style`, `attr()`, `lang`). A change of
/// none of them is neither noted nor marked for the style phase. Style
/// gives them (`Cx::set_style_names`) as its sheets change; a document with
/// none hasn't been styled, and is styled whole when it is: no change of
/// it is noted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StyleNames {
    /// Attributes' local names, as written and ASCII lowercase.
    pub attributes: rustc_hash::FxHashSet<NameId>,
    /// Classes, by their names ASCII lowercase.
    pub classes: rustc_hash::FxHashSet<NameId>,
    /// Ids, ASCII lowercase.
    pub ids: rustc_hash::FxHashSet<Box<str>>,
    /// `AttributeChanges`' state bits.
    pub states: u16,
    /// Every change counts (`:has()`: what is below an element decides
    /// what it matches).
    pub everything: bool,
}

impl StyleNames {
    /// Whether attribute `name`'s change from `old` to `new` is one style
    /// reads: its name, a class that came or went, an old or new id.
    fn uses(&self, name: QualifiedName, old: Option<&str>, new: Option<&str>) -> bool {
        if self.everything || self.attributes.contains(&name.local()) {
            return true;
        }
        let lower = |value: &str| -> Box<str> {
            if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
                value.to_ascii_lowercase().into()
            } else {
                value.into()
            }
        };
        if name.is(names::NONE, names::CLASS) && !self.classes.is_empty() {
            fn tokens(value: Option<&str>) -> impl Iterator<Item = &str> {
                value.into_iter().flat_map(str::split_ascii_whitespace)
            }
            let changed = |from: Option<&str>, to: Option<&str>| {
                tokens(from).any(|class| {
                    !tokens(to).any(|other| other == class)
                        && names::find(&lower(class))
                            .is_some_and(|class| self.classes.contains(&class))
                })
            };
            return changed(old, new) || changed(new, old);
        }
        name.is(names::NONE, names::ID)
            && old != new
            && [old, new]
                .into_iter()
                .flatten()
                .any(|id| self.ids.contains(&lower(id)))
    }
}

/// A document's `StyleNames`, never changed after (another replaces it), so
/// that each change reads them with no borrow.
#[derive(GcObject)]
pub struct StyleNamesData {
    names: External<StyleNames>,
}

#[cfg(debug_assertions)]
thread_local! {
    /// The changes of a styled document the style names passed over, with
    /// the document and the element (the debug build's: style checks at its
    /// next recalc that it reads none of them).
    static PASSED_OVER: std::cell::RefCell<Vec<(NodeId, NodeId, AttributeChanges)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// An element's classes, in order, each once: made as its `class`
/// attribute changes, never changed after (another list replaces it), so
/// that style reads them with no borrow.
#[derive(GcObject)]
pub struct ClassList {
    names: External<Box<[ClassName]>>,
}

/// An element's data besides its name and attributes (Blink's
/// `ElementRareData` and the style fields of `Element`): a template's
/// contents, its listeners, and what style keeps on it. Made at the first
/// of them.
#[derive(GcObject)]
pub struct ElementData {
    pub(crate) template_content: Member<Node>,
    /// The shadow root it hosts (`attach_shadow`), never taken away.
    pub(crate) shadow_root: Member<Node>,
    /// Its custom element definition, once constructed or upgraded, and
    /// its custom element reaction queue, once it has had one.
    pub(crate) custom_definition: Member<crate::CustomElementDefinition>,
    pub(crate) reactions: Member<HeapVector<crate::Reaction>>,
    /// A slot's assigned and manually assigned nodes (`slot`).
    pub(crate) slot: Member<SlotData>,
    /// A listed element's form owner, a form's elements (`form`).
    pub(crate) form: Member<crate::FormAssociation>,
    /// An `<img>`'s requests, from its first update (`image`).
    pub(crate) image: Member<crate::ImageLoader>,
    /// Its intersection observations, from the first (`intersection`).
    pub(crate) intersections: Member<HeapVector<crate::intersection::IntersectionObservation>>,
    /// Its resize observations, from the first (`resize`).
    pub(crate) resize_observations: Member<HeapVector<crate::resize::ResizeObservation>>,
    /// A `<canvas>`'s 2D context, from `getContext("2d")` (`canvas`).
    pub(crate) canvas_context: Member<crate::CanvasRenderingContext2d>,
    /// Its listeners and observers', made at the first.
    pub(crate) rare: Member<NodeRareData>,
    /// Its classes; none without any.
    classes: Member<ClassList>,
    /// Its `classList`, `dataset` and `style`, once script asked for
    /// them.
    pub(crate) class_list: Member<DomTokenList>,
    pub(crate) dataset: Member<DomStringMap>,
    pub(crate) style_declaration: Member<CssStyleDeclaration>,
    /// An SVG element's `className`, once script asked for it.
    pub(crate) svg_class_name: Member<crate::SvgAnimatedString>,
    /// An `<iframe>`'s content document (`frame`).
    pub(crate) content_document: Member<Node>,
    /// A script element's external script waiting for its turn, if it
    /// waits for no parser (`script`).
    pub(crate) script: Member<crate::script::PendingScript>,
    /// A `<style>`'s or `<link>`'s sheet, the one made for it last
    /// (`cssom`).
    pub(crate) sheet: Member<crate::CssStyleSheet>,
    /// The `Attr`s made for its attributes, and its `attributes`, once
    /// script asked for them (`attr`).
    pub(crate) attr_nodes: Member<HeapVector<Node>>,
    pub(crate) attribute_map: Member<crate::NamedNodeMap>,
    /// Its `style` attribute's declarations, the one copy (Blink's
    /// `ElementData::inline_style_`): parsed from the attribute when first
    /// asked for, or CSSOM's. In a `Cell`, taken and put back (a move, no
    /// borrow); copy on write, the style phase holding a clone.
    inline_style: External<Cell<Option<Arc<CssPropertyValueSet>>>>,
    /// What matching selectors found it depends on (`selectors`'
    /// `ElementSelectorFlags`, as bits), only ever added to.
    selector_flags: Cell<u16>,
    /// The states (`AttributeChanges`' bits) matching looked at of it,
    /// only ever added to: a change of another doesn't change what its
    /// own rules match.
    state_dependencies: Cell<u16>,
    /// The line its start tag ends on in its document's markup, where its
    /// text starts, for a `<style>` the parser made (0: none): where
    /// DevTools tells its sheet's text is.
    pub(crate) parser_line: Cell<u32>,
    /// What changed of its attributes since the style phase last looked:
    /// written by the DOM's operations, taken by the style phase (a
    /// `Cell`: a move, no borrow).
    changes: External<Cell<Option<Box<AttributeChanges>>>>,
    style: External<TokenCell<ElementStyle>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s data, made if it has none.
    pub(crate) fn element_data(&self, element: NodeRef<'gc>) -> Gc<'gc, ElementData> {
        if let Some(data) = element
            .data
            .get(self.gc)
            .and_then(|data| data.downcast::<ElementData>())
        {
            return data;
        }
        debug_assert_eq!(self.kind(element), NodeKind::Element);
        let data = self.gc.alloc_beside(
            element,
            &sites::OTHER,
            ElementDataInit {
                template_content: None,
                shadow_root: None,
                custom_definition: None,
                reactions: None,
                slot: None,
                form: None,
                image: None,
                intersections: None,
                resize_observations: None,
                canvas_context: None,
                script: None,
                sheet: None,
                rare: None,
                classes: None,
                class_list: None,
                dataset: None,
                style_declaration: None,
                svg_class_name: None,
                content_document: None,
                attr_nodes: None,
                attribute_map: None,
                inline_style: External::new(Cell::new(None)),
                selector_flags: Cell::new(0),
                state_dependencies: Cell::new(0),
                parser_line: Cell::new(0),
                changes: External::new(Cell::new(None)),
                style: External::new(TokenCell::default()),
            },
        );
        element.data.set(self.gc, Some(data.erase()));
        data
    }

    pub(crate) fn existing_element_data(&self, node: NodeRef<'gc>) -> Option<Gc<'gc, ElementData>> {
        node.data.get(self.gc)?.downcast::<ElementData>()
    }

    /// A list of `names` (`getElementsByClassName`'s too).
    pub(crate) fn new_class_list(&self, names: Box<[ClassName]>) -> Gc<'gc, ClassList> {
        self.gc.alloc_in(
            &sites::OTHER,
            ClassListInit {
                names: External::new(names),
            },
        )
    }

    pub(crate) fn class_list_names(&self, list: Gc<'gc, ClassList>) -> &'gc [ClassName] {
        list.into_ref().names.get()
    }

    /// `element`'s classes, in order (none for another kind of node).
    pub fn class_names(&self, element: NodeRef<'gc>) -> &'gc [ClassName] {
        if self.flag(element, CLASSES_STALE) {
            self.make_class_list(element);
        }
        self.existing_element_data(element)
            .and_then(|data| data.into_ref().classes.get(self.gc))
            .map_or(&[], |list| list.into_ref().names.get())
    }

    /// `element`'s `class` attribute changed: its classes are made again
    /// the next time they are asked for (style's), not now, so that a page
    /// that sets `className` and never styles makes none (Blink shares a
    /// `SpaceSplitString` between equal values instead).
    fn class_changed(&self, element: NodeRef<'gc>) {
        self.set_flags(element, CLASSES_STALE);
    }

    /// `element`'s classes made from its `class` attribute as it is, each
    /// name interned.
    fn make_class_list(&self, element: NodeRef<'gc>) {
        element.flags.set(element.flags.get() & !CLASSES_STALE);
        let value = self.attribute_value_by_id(element, names::CLASS);
        let value = value
            .as_ref()
            .map(|value| value.borrow().to_str().into_owned());
        let mut names: Vec<ClassName> = Vec::new();
        for token in value
            .as_deref()
            .into_iter()
            .flat_map(str::split_ascii_whitespace)
        {
            let name = names::intern(token);
            if names.iter().any(|class| class.name == name) {
                continue;
            }
            let lower = if token.bytes().any(|byte| byte.is_ascii_uppercase()) {
                names::intern(&token.to_ascii_lowercase())
            } else {
                name
            };
            names.push(ClassName {
                name,
                lower,
                hash: css::selector::name_hash(token),
            });
        }
        if names.is_empty() && self.existing_element_data(element).is_none() {
            return;
        }
        let list = (!names.is_empty()).then(|| {
            self.gc.alloc_beside(
                element,
                &sites::OTHER,
                ClassListInit {
                    names: External::new(names.into_boxed_slice()),
                },
            )
        });
        self.element_data(element).classes.set(self.gc, list);
    }

    /// `node`'s id (its number in the heap's id table: `node_by_id` finds
    /// it while it lives), given the first time it is asked for.
    pub fn node_id(&self, node: NodeRef<'gc>) -> NodeId {
        match node.id.get() {
            0 => {
                let id = self.gc.new_id(node).get();
                node.id.set(id);
                NodeId::from_raw(id)
            }
            id => NodeId::from_raw(id),
        }
    }

    /// The node `id` was given to, if it lives.
    pub fn node_by_id(&self, id: NodeId) -> Option<NodeRef<'gc>> {
        self.gc.by_id(elise::Id::from_raw(id.get()))
    }

    /// DOM's "connected": `node` is in a document's tree.
    pub fn is_connected(&self, node: NodeRef<'gc>) -> bool {
        self.flag(node, CONNECTED)
    }

    /// Whether `object` is a node out of every document's tree (Blink's
    /// detachedness of a node's wrapper in a heap snapshot): none if it
    /// isn't a node.
    pub fn is_detached(&self, object: Gc<'gc, elise::Erased>) -> Option<bool> {
        object
            .downcast::<Node>()
            .map(|node| !self.is_connected(node))
    }

    /// How many times the DOM's trees changed (insertions, removals,
    /// attributes, text): what a cache of them checks.
    pub fn tree_version(&self) -> u64 {
        self.changes.version.get()
    }

    pub(crate) fn tree_changed(&self) {
        self.changes.version.set(
            self.changes
                .version
                .get()
                .checked_add(1)
                .expect("fewer than 2^64 changes"),
        );
    }

    pub fn style_flags(&self, node: NodeRef<'gc>) -> StyleFlags {
        StyleFlags((node.flags.get() >> STYLE_SHIFT) & StyleFlags::ALL)
    }

    /// `node`'s style flags cleared, and what changed of its attributes
    /// dropped: the style phase looked.
    pub fn clear_style_flags(&self, node: NodeRef<'gc>) {
        node.flags
            .set(node.flags.get() & !(StyleFlags::ALL << STYLE_SHIFT));
        if let Some(data) = self.existing_element_data(node) {
            data.changes.get().set(None);
        }
    }

    pub fn layout_tree_flags(&self, node: NodeRef<'gc>) -> LayoutTreeFlags {
        LayoutTreeFlags(
            ((node.flags.get() >> LAYOUT_SHIFT) & LayoutTreeFlags::ALL)
                | (u32::from(node.layout_more.get()) << 5),
        )
    }

    pub fn clear_layout_tree_flags(&self, node: NodeRef<'gc>) {
        node.flags
            .set(node.flags.get() & !(LayoutTreeFlags::ALL << LAYOUT_SHIFT));
        node.layout_more.set(0);
    }

    /// The style phase's: `node`'s computed style changed, for layout.
    pub fn set_style_changed(&self, node: NodeRef<'gc>) {
        self.mark_layout(node, LayoutTreeFlags::STYLE_CHANGED);
    }

    /// The style phase's: `node`'s pseudo-elements' styles changed.
    pub fn set_pseudo_styles_changed(&self, node: NodeRef<'gc>) {
        self.mark_layout(node, LayoutTreeFlags::PSEUDO_STYLES_CHANGED);
    }

    /// The style phase's: `node` and its subtree styled again (Blink's
    /// `SetNeedsStyleRecalc`), later in this recalc if the walk hasn't
    /// passed it yet, or in a walk after it.
    pub fn set_needs_style_recalc(&self, node: NodeRef<'gc>) {
        self.mark_style(node, StyleFlags::NEEDS_STYLE_RECALC);
    }

    /// `node`'s boxes are built again.
    pub fn set_needs_reattach(&self, node: NodeRef<'gc>) {
        self.mark_layout(node, LayoutTreeFlags::NEEDS_REATTACH);
    }

    /// What changed of `element`'s attributes since the style phase last
    /// looked, taken; none for another kind of node.
    pub fn take_attribute_changes(&self, element: NodeRef<'gc>) -> Option<Box<AttributeChanges>> {
        self.existing_element_data(element)?.changes.get().take()
    }

    /// What matching selectors found `node` depends on: a shadow root's,
    /// what they found its children depend on of it. Another node that
    /// isn't an element has every bit: none was kept (a document's only
    /// element child doesn't move).
    pub fn selector_flags(&self, node: NodeRef<'gc>) -> u16 {
        match self.kind(node) {
            NodeKind::Element => self
                .existing_element_data(node)
                .map_or(0, |data| data.selector_flags.get()),
            _ => self
                .shadow_data(node)
                .map_or(u16::MAX, |data| data.selector_flags.get()),
        }
    }

    /// The states matching looked at of `element` (`AttributeChanges`'
    /// bits): those whose change may change what its own rules match.
    pub fn state_dependencies(&self, element: NodeRef<'gc>) -> u16 {
        self.existing_element_data(element)
            .map_or(0, |data| data.state_dependencies.get())
    }

    /// `states` added to `element`'s state dependencies (an element's).
    pub fn add_state_dependencies(&self, element: NodeRef<'gc>, states: u16) {
        if self.kind(element) == NodeKind::Element
            && self.state_dependencies(element) & states != states
        {
            let data = self.element_data(element);
            data.state_dependencies
                .set(data.state_dependencies.get() | states);
        }
    }

    /// `flags` added to `node`'s selector flags (an element's; nothing for
    /// another kind).
    pub fn add_selector_flags(&self, node: NodeRef<'gc>, flags: u16) {
        if self.selector_flags(node) & flags == flags {
            return;
        }
        if self.kind(node) == NodeKind::Element {
            let data = self.element_data(node);
            data.selector_flags.set(data.selector_flags.get() | flags);
        } else if let Some(data) = self.shadow_data(node) {
            // A shadow root's children's, as their parent's.
            data.selector_flags.set(data.selector_flags.get() | flags);
        }
    }

    /// `element`'s style, as the style phase left it, read while `token`
    /// is; none for an element never styled, or another kind of node.
    pub fn element_style<'t>(
        &self,
        element: NodeRef<'gc>,
        token: &'t LocalToken,
    ) -> Option<&'t ElementStyle>
    where
        'gc: 't,
    {
        Some(
            self.existing_element_data(element)?
                .into_ref()
                .style
                .get()
                .get(token),
        )
    }

    /// `element`'s computed style, read while `token` is.
    pub fn computed_style<'t>(
        &self,
        element: NodeRef<'gc>,
        token: &'t LocalToken,
    ) -> Option<&'t Arc<ComputedStyle>>
    where
        'gc: 't,
    {
        self.element_style(element, token)?.computed.as_ref()
    }

    /// `element`'s style, written while `token` is: the style phase's.
    ///
    /// # Panics
    ///
    /// If `element` isn't an element.
    pub fn element_style_mut<'t>(
        &self,
        element: NodeRef<'gc>,
        token: &'t mut LocalToken,
    ) -> &'t mut ElementStyle
    where
        'gc: 't,
    {
        assert_eq!(
            self.kind(element),
            NodeKind::Element,
            "only an element has a style of its own"
        );
        self.element_data(element)
            .into_ref()
            .style
            .get()
            .get_mut(token)
    }

    fn set_flags(&self, node: NodeRef<'gc>, bits: u32) {
        node.flags.set(node.flags.get() | bits);
    }

    /// `flags` set on `node`, its ancestors in the flat tree (what style
    /// walks) marked to the first marked.
    /// The style phase goes down to `element` (its flat-tree ancestors
    /// marked), which looks at what changed of it there (its animations
    /// moved on), without a flag of its own.
    pub(crate) fn mark_style_path(&self, element: NodeRef<'gc>) {
        let child_needs = StyleFlags::CHILD_NEEDS_STYLE_RECALC.0 << STYLE_SHIFT;
        let mut at = self.style_parent(element);
        while let Some(ancestor) = at.filter(|ancestor| ancestor.flags.get() & child_needs == 0) {
            self.set_flags(ancestor, child_needs);
            at = self.style_parent(ancestor);
        }
    }

    fn mark_style(&self, node: NodeRef<'gc>, flags: StyleFlags) {
        let node = self.flagged_for(node);
        self.set_flags(node, flags.0 << STYLE_SHIFT);
        let child_needs = StyleFlags::CHILD_NEEDS_STYLE_RECALC.0 << STYLE_SHIFT;
        let mut at = self.style_parent(node);
        while let Some(ancestor) = at.filter(|ancestor| ancestor.flags.get() & child_needs == 0) {
            self.set_flags(ancestor, child_needs);
            at = self.style_parent(ancestor);
        }
    }

    /// The node style walks down to `node` from: its flat-tree parent, or
    /// for a host's child assigned to no slot (none), the host, which
    /// styles it after its flat-tree children.
    #[inline]
    fn style_parent(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.flat_tree_parent(node).or_else(|| {
            self.parent(node)
                .filter(|&parent| parent.scope.get() & crate::shadow::SHADOW_HOST != 0)
        })
    }

    pub(crate) fn mark_layout(&self, node: NodeRef<'gc>, flags: LayoutTreeFlags) {
        let node = self.flagged_for(node);
        if flags.0 & LayoutTreeFlags::CONTROL_STATE_CHANGED.0 != 0 {
            node.layout_more.set(1);
        }
        self.set_flags(node, (flags.0 & LayoutTreeFlags::ALL) << LAYOUT_SHIFT);
        let child_needs = LayoutTreeFlags::CHILD_NEEDS_UPDATE.0 << LAYOUT_SHIFT;
        let mut at = self.flat_tree_parent(node);
        while let Some(ancestor) = at.filter(|ancestor| ancestor.flags.get() & child_needs == 0) {
            self.set_flags(ancestor, child_needs);
            at = self.flat_tree_parent(ancestor);
        }
    }

    /// The node a flag of `node` goes on: a shadow root's on its host (whose
    /// children in the flat tree are the root's: style and layout don't
    /// walk the root itself).
    #[inline]
    fn flagged_for(&self, node: NodeRef<'gc>) -> NodeRef<'gc> {
        if node.scope.get() & crate::shadow::SHADOW_ROOT != 0 {
            return self.shadow_host(node).expect("a shadow root's host");
        }
        node
    }

    /// The nearest element before `node` among its siblings, `forward`
    /// false; after it, true.
    fn element_sibling(&self, node: Option<NodeRef<'gc>>, forward: bool) -> Option<NodeRef<'gc>> {
        let mut at = node;
        while let Some(sibling) = at {
            if self.kind(sibling) == NodeKind::Element {
                return Some(sibling);
            }
            at = if forward {
                self.next_sibling(sibling)
            } else {
                self.previous_sibling(sibling)
            };
        }
        None
    }

    /// Shadow root `root`'s children came or went: its own flag says so,
    /// for what matches them by position (its host's says its light
    /// children's); `host` is walked down to, and takes `layout`'s flags.
    #[cold]
    fn shadow_root_children_changed(&self, root: NodeRef<'gc>, host: NodeRef<'gc>, layout: u32) {
        self.set_flags(root, StyleFlags::CHILDREN_CHANGED.0 << STYLE_SHIFT);
        self.set_flags(
            host,
            (StyleFlags::CHILD_NEEDS_STYLE_RECALC.0 << STYLE_SHIFT) | layout,
        );
    }

    /// The siblings around where a node came or went (`previous` and
    /// `next` of it): what they match by position may change.
    fn siblings_changed(&self, previous: Option<NodeRef<'gc>>, next: Option<NodeRef<'gc>>) {
        if let Some(previous) = self.element_sibling(previous, false) {
            self.mark_style(previous, StyleFlags::PRECEDES_CHANGE);
        }
        if let Some(next) = self.element_sibling(next, true) {
            self.mark_style(next, StyleFlags::FOLLOWS_CHANGE);
        }
    }

    /// Whether `node` is an HTML or SVG `<style>` or an HTML `<link>`: its
    /// sheets are its document's.
    #[inline]
    fn owns_style_sheets(&self, node: NodeRef<'gc>) -> bool {
        matches!(
            node.qualified_name(),
            STYLE_ELEMENT | SVG_STYLE_ELEMENT | LINK_ELEMENT
        )
    }

    /// An element's sheet changed (its rules, media or `disabled`, by
    /// CSSOM): its tree scope's sheets are collected again.
    pub(crate) fn owner_sheet_changed(&self, owner: NodeRef<'gc>) {
        self.tree_changed();
        self.style_sheets_changed(owner);
    }

    /// The sheets of `node`'s tree scope may have changed: its document's,
    /// or its shadow root's (`shadow_sheets_changed`).
    fn style_sheets_changed(&self, node: NodeRef<'gc>) {
        match self.containing_shadow_root(node) {
            Some(root) => self.shadow_sheets_changed(root),
            None => self.mark_document_sheets_changed(self.document_of(node)),
        }
    }

    /// `document`'s sheets may have changed.
    pub(crate) fn mark_document_sheets_changed(&self, document: NodeRef<'gc>) {
        self.mark_style(document, StyleFlags::STYLE_SHEETS_CHANGED);
    }

    /// `root`'s sheets may have changed (style's too: an adopted sheet's
    /// contents changed, which the sheet doesn't tell the scopes of).
    pub fn mark_shadow_sheets_changed(&self, root: NodeRef<'gc>) {
        self.shadow_sheets_changed(root);
    }

    /// `root`'s sheets may have changed: its host marked (Blink's
    /// `StyleEngine::SetNeedsActiveStyleUpdate` for the tree scope), and
    /// it in its document's list style takes before it walks (once).
    #[cold]
    #[inline(never)]
    fn shadow_sheets_changed(&self, root: NodeRef<'gc>) {
        self.mark_style(root, StyleFlags::STYLE_SHEETS_CHANGED);
        let Some(data) = self.shadow_data(root) else {
            return;
        };
        if data.sheets_listed.replace(true) {
            return;
        }
        let data = self.ensure_document_data(self.document_of(root));
        let list = match data.sheet_changed_roots.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.sheet_changed_roots.set(self.gc, Some(list));
                list
            }
        };
        list.push(self.gc, root);
    }

    /// `document`'s shadow roots whose sheets may have changed since style
    /// last took them, left for it (the renderer's: their `<link>`s to
    /// load).
    pub fn sheet_changed_roots(&self, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        self.listed_sheet_changed_roots(document, false)
    }

    /// `document`'s shadow roots whose sheets may have changed since the
    /// last time, taken (its style's, before it walks: their sheets are
    /// collected again).
    pub fn take_sheet_changed_roots(&self, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        self.listed_sheet_changed_roots(document, true)
    }

    /// `document`'s list's roots still in it: one adopted into another
    /// document meanwhile is in that one's list (`shadow_root_adopted`).
    fn listed_sheet_changed_roots(&self, document: NodeRef<'gc>, take: bool) -> Vec<NodeRef<'gc>> {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.sheet_changed_roots.get(self.gc))
        else {
            return Vec::new();
        };
        let found: Vec<NodeRef<'gc>> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .filter(|root| Gc::ptr_eq(self.document_of(*root), document))
            .collect();
        if take {
            list.clear(self.gc);
            for root in &found {
                if let Some(data) = self.shadow_data(*root) {
                    data.sheets_listed.set(false);
                }
            }
        }
        found
    }

    /// A sheet's owner `at` came or went (`removed`) with a subtree: its
    /// shadow root's sheets may have changed, or (`document`) its
    /// document's; one gone has its sheet no more. On every node of a
    /// subtree coming or going: inline, the owner's part cold.
    #[inline]
    fn sheet_owner_moved(&self, at: NodeRef<'gc>, document: &mut bool, removed: bool) {
        if !self.owns_style_sheets(at) {
            return;
        }
        if removed {
            self.sheet_owner_removed(at);
        } else if at.qualified_name() == LINK_ELEMENT {
            // Script that connected a `<link>` asked for its sheet.
            self.element_asked_for_load(at, false);
        }
        match self.containing_shadow_root(at) {
            Some(root) => self.shadow_sheets_changed(root),
            None => *document = true,
        }
    }

    /// `node`'s ancestors marked for both style and layout (something below
    /// needs each), to the first marked for both already.
    fn mark_ancestors(&self, node: NodeRef<'gc>) {
        let both = (StyleFlags::CHILD_NEEDS_STYLE_RECALC.0 << STYLE_SHIFT)
            | (LayoutTreeFlags::CHILD_NEEDS_UPDATE.0 << LAYOUT_SHIFT);
        let mut at = self.flat_tree_parent(node);
        while let Some(ancestor) = at.filter(|ancestor| ancestor.flags.get() & both != both) {
            self.set_flags(ancestor, both);
            at = self.flat_tree_parent(ancestor);
        }
    }

    /// `node` was put in `parent`'s children (`link`), between `previous`
    /// and `next`: if `parent` is connected, `node`'s subtree is connected
    /// now, styled again and its boxes built again, with its new siblings'
    /// positions; a stylesheet that came, or text into a `<style>`, is its
    /// document's.
    #[inline]
    pub(crate) fn inserted(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        self.tree_changed();
        self.collections_children_changed();
        if self.dom_changes_wanted() {
            self.note_inserted(parent, node, previous);
        }
        if self.is_connected(parent) {
            self.inserted_connected(parent, node, previous, next);
        }
        self.form_inserted(parent, node, next);
    }

    fn inserted_connected(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        let mut sheets = self.owns_style_sheets(parent);
        if sheets {
            self.style_children_changed(parent);
        }
        // From the first input or textarea on, those connected now get
        // their UA shadow trees before the next style update.
        let controls = self.has_seen(crate::form::CONTROLS_SEEN);
        // From the first `<use>` on, those connected now are copied before
        // the next style update, and ids looked at.
        let uses = self.has_seen_uses();
        // From the first iframe, or script element that may run, on: those
        // connected now recorded in tree order, and a script that had a
        // node inserted before them.
        let steps = self.has_seen_connection_steps();
        if steps {
            self.script_children_changed(parent);
        }
        let names = self.has_seen(crate::form::NAMES_SEEN);
        // Through its hosts' shadow trees too (DOM's "shadow-including"):
        // they are connected; their ids and sheets are their shadow
        // roots', not the document's.
        self.for_each_shadow_including_inclusive_descendant(node, |at, in_shadow| {
            self.set_flags(at, CONNECTED);
            if controls {
                self.schedule_control_tree(at);
            }
            if uses {
                self.svg_use_node_moved(at, true);
            }
            if steps {
                self.connection_steps(at);
            }
            self.sheet_owner_moved(at, &mut sheets, false);
            // A shadow root connected again: its sheets may have changed
            // while it wasn't (a `<style>` taken out then marked nothing),
            // collected again (Blink's `ShadowRootInsertedToDocument`).
            if at.scope.get() & crate::shadow::SHADOW_ROOT != 0 {
                self.shadow_sheets_changed(at);
            }
            if self.may_have_reactions(at) {
                self.custom_element_connected(at);
            }
            if in_shadow {
                return;
            }
            if at.flags.get() & HAS_ID != 0 {
                self.id_added(self.document_of(parent), at);
            }
            if names {
                self.name_added(self.document_of(parent), at);
            }
        });
        self.set_flags(
            node,
            (StyleFlags::NEEDS_STYLE_RECALC.0 << STYLE_SHIFT)
                | (LayoutTreeFlags::NEEDS_REATTACH.0 << LAYOUT_SHIFT),
        );
        let flagged = self.flagged_for(parent);
        let layout = LayoutTreeFlags::CHILD_NEEDS_UPDATE.0 << LAYOUT_SHIFT;
        if Gc::ptr_eq(flagged, parent) {
            self.set_flags(
                flagged,
                ((StyleFlags::CHILDREN_CHANGED.0 | StyleFlags::CHILD_NEEDS_STYLE_RECALC.0)
                    << STYLE_SHIFT)
                    | layout,
            );
        } else {
            self.shadow_root_children_changed(parent, flagged, layout);
        }
        self.mark_ancestors(flagged);
        self.siblings_changed(previous, next);
        if sheets {
            self.style_sheets_changed(parent);
        }
        if uses {
            self.svg_use_subtree_changed(parent);
        }
    }

    /// `node` was taken out of `parent`'s children (`remove`), from
    /// between `previous` and `next`: if it was connected, its subtree
    /// isn't now; `parent`'s children and boxes change.
    #[inline]
    pub(crate) fn removed(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        self.tree_changed();
        self.collections_children_changed();
        if self.dom_changes_wanted() {
            self.note_removed(parent, node);
        }
        self.animations_removed(node);
        let connected = self.is_connected(node);
        if connected {
            self.removed_connected(parent, node, previous, next);
        }
        self.form_removed(parent, node, next, connected);
    }

    fn removed_connected(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        let mut sheets = self.owns_style_sheets(parent);
        if sheets {
            self.style_children_changed(parent);
        }
        // The `:hover` or `:active` chain goes through it: the states move
        // to `parent` (below).
        let states = node.flags.get() & (HOVERED | ACTIVE | FOCUS_CHAIN);
        let uses = self.has_seen_uses();
        let iframes = self.has_seen_removal_steps();
        let names = self.has_seen(crate::form::NAMES_SEEN);
        self.for_each_shadow_including_inclusive_descendant(node, |at, in_shadow| {
            if uses {
                self.svg_use_node_moved(at, false);
            }
            if iframes {
                self.frame_owner_removed(at);
            }
            at.flags
                .set(at.flags.get() & !(CONNECTED | HOVERED | ACTIVE | FOCUS_BITS));
            if self.may_have_reactions(at) {
                self.custom_element_disconnected(at);
            }
            // Before its tree scope is moved: its shadow root's still.
            self.sheet_owner_moved(at, &mut sheets, true);
            if in_shadow {
                return;
            }
            if at.flags.get() & HAS_ID != 0
                && let Some(key) = self.id_key_of(at)
            {
                self.id_removed(self.document_of(parent), at, key);
            }
            if names {
                self.name_removed(self.document_of(parent), at);
            }
        });
        let flagged = self.flagged_for(parent);
        let layout = LayoutTreeFlags::CHILDREN_CHANGED.0 << LAYOUT_SHIFT;
        if Gc::ptr_eq(flagged, parent) {
            self.set_flags(
                flagged,
                (StyleFlags::CHILDREN_CHANGED.0 << STYLE_SHIFT) | layout,
            );
        } else {
            self.shadow_root_children_changed(parent, flagged, layout);
        }
        self.mark_ancestors(flagged);
        self.siblings_changed(previous, next);
        if sheets {
            // The document `node` was in, `parent`'s.
            self.style_sheets_changed(parent);
        }
        if states != 0 {
            self.user_action_element_removed(parent, states);
        }
        if uses {
            self.svg_use_subtree_changed(parent);
        }
    }

    /// `element`'s attribute `name` changed from `old` to `new`: if it is
    /// connected and not to be styled whole anyway, what changed is noted
    /// for the style phase, and its next sibling and parent marked (Blink's
    /// `StyleEngine::AttributeChangedForElement` skips such an element
    /// too).
    #[inline]
    pub(crate) fn attribute_changed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        if let Some(new) = new {
            self.attribute_bloomed(element, name, new);
        }
        if self.observed() {
            self.queue_attribute_record(element, name, old);
        }
        if self.dom_changes_wanted() {
            self.note_attribute(element, name);
        }
        if self.is_custom_element(element) {
            self.custom_element_attribute_changed(element, name, old, new);
        }
        self.tree_changed();
        self.collections_attribute_changed(name);
        if name == CLASS_ATTRIBUTE {
            self.class_changed(element);
        }
        if element.flags.get() & HAS_INLINE_STYLE != 0 && name == STYLE_ATTRIBUTE {
            self.inline_style_attribute_changed(element);
        }
        // A plain name's number is its local name's: one read of a table.
        if names::handler_entry(name.raw()) != 0 {
            self.handler_attribute_changed(element, name.raw());
        }
        if name == crate::slot::SLOT_ATTRIBUTE || name == crate::slot::NAME_ATTRIBUTE {
            self.slot_name_changed(element, name, old, new);
        }
        if name == crate::slot::NAME_ATTRIBUTE {
            self.name_attribute_changed(element, old);
        }
        if name == QualifiedName::plain(names::local::HTTP_EQUIV) && new.is_some() {
            self.see(crate::form::PRAGMAS_SEEN);
        }
        if name == AUTOFOCUS_ATTRIBUTE && new.is_some() {
            self.see(crate::form::AUTOFOCUS_SEEN);
        }
        // A plain name's number is its local name's: one read of a table.
        if names::form_kind(name.raw()) & crate::form::CONTROL_ATTRIBUTE != 0 {
            self.control_attribute_changed(element, name.raw(), new.is_some());
        }
        if self.is_connected(element) {
            // The same id set again leaves the tables as they are (Blink's
            // `Element::AttributeChanged` compares the old and new ids'
            // atoms before `UpdateId`).
            if name == ID_ATTRIBUTE && old != new {
                self.connected_id_changed(element, old);
                self.form_id_changed();
            }
            // An SVG element's attributes are its geometry (`x1`, `points`,
            // `viewBox`): what layout captured of its `<svg>` is out of date.
            if element.qualified_name().namespace() == names::SVG_NAMESPACE {
                self.set_needs_reattach(element);
            }
            if self.has_seen_uses() {
                self.svg_use_attribute_changed(element, name);
            }
            if self.has_seen(
                crate::form::IFRAMES_SEEN
                    | crate::form::PRELOADS_SEEN
                    | crate::form::BASES_SEEN
                    | crate::form::PRAGMAS_SEEN,
            ) {
                self.frame_owner_attribute_changed(element, name, old, new);
            }
            self.connected_attribute_changed(element, name, old, new);
        }
    }

    /// `element`'s `style` attribute's declarations, the one copy: parsed
    /// from the attribute the first time (and kept), none without one.
    pub fn inline_style(&self, element: NodeRef<'gc>) -> Option<Arc<CssPropertyValueSet>> {
        if let Some(data) = self.existing_element_data(element) {
            let cell = data.into_ref().inline_style.get();
            if let Some(block) = cell.take() {
                cell.set(Some(block.clone()));
                return Some(block);
            }
        }
        let value = self.attribute_value_by_id(element, names::local::STYLE)?;
        let mut block = CssPropertyValueSet::parse(&value.borrow().to_str());
        // Its `url()`s against its document's base URL, its document's URL
        // the referrer.
        if block.has_unresolved_urls()
            && let Some(url) = self.document_url(self.document_of(element))
            && let Some(base) = self.document_base_url(self.document_of(element))
        {
            block.resolve_urls(&css::UrlContext::new(base, &url.to_str()));
        }
        let block = Arc::new(block);
        self.element_data(element)
            .inline_style
            .get()
            .set(Some(block.clone()));
        element.flags.set(element.flags.get() | HAS_INLINE_STYLE);
        Some(block)
    }

    /// `element`'s declarations taken out to change (`set_inline_style`
    /// puts them back): its own if no one else holds them (copy on write).
    pub fn take_inline_style(&self, element: NodeRef<'gc>) -> Arc<CssPropertyValueSet> {
        if self.observed() || self.is_custom_element(element) {
            // The attribute as it is before the change: its record's and
            // reaction's old value (`set_inline_style`).
            self.synchronize_attributes(element);
        }
        let block = self.inline_style(element).unwrap_or_default();
        if let Some(data) = self.existing_element_data(element) {
            drop(data.into_ref().inline_style.get().take());
        }
        block
    }

    /// `element`'s declarations put back as they were taken
    /// (`take_inline_style`), unchanged: no notice, the attribute as it is.
    pub fn restore_inline_style(&self, element: NodeRef<'gc>, block: Arc<CssPropertyValueSet>) {
        self.element_data(element)
            .inline_style
            .get()
            .set(Some(block));
        element.flags.set(element.flags.get() | HAS_INLINE_STYLE);
    }

    /// CSSOM's "update style attribute for": `block` is `element`'s
    /// declarations now, its `style` attribute serialized from them when it
    /// is next read; style is worked out again as for the attribute's
    /// change.
    pub fn set_inline_style(&self, element: NodeRef<'gc>, block: Arc<CssPropertyValueSet>) {
        if self.dom_changes_wanted() {
            self.note_inline_style(element);
        }
        let custom = self.is_custom_element(element);
        if self.observed() || custom {
            // The attribute as it was (written from the declarations
            // first, if `cssText` replaces ones not taken out).
            let old = self
                .attribute_value_by_id(element, names::local::STYLE)
                .map(|value| value.borrow().to_str().into_owned());
            if self.observed() {
                self.queue_attribute_record(element, STYLE_ATTRIBUTE, old.as_deref());
            }
            if custom {
                // Its `attributeChangedCallback` with the attribute as it
                // will be written (Blink's `StyleAttributeMutationScope`).
                self.custom_element_attribute_changed(
                    element,
                    STYLE_ATTRIBUTE,
                    old.as_deref(),
                    Some(&block.css_text()),
                );
            }
        }
        self.element_data(element)
            .inline_style
            .get()
            .set(Some(block));
        element
            .flags
            .set(element.flags.get() | HAS_INLINE_STYLE | STYLE_ATTRIBUTE_DIRTY);
        self.tree_changed();
        if self.is_connected(element) {
            self.connected_attribute_changed(element, STYLE_ATTRIBUTE, None, None);
        }
    }

    /// `element`'s `style` attribute written from its declarations, with no
    /// notice (the change was noticed as CSSOM made it): before any of its
    /// attributes is read (Blink's `SynchronizeStyleAttributeInternal`).
    #[cold]
    #[inline(never)]
    pub(crate) fn synchronize_style_attribute(&self, element: NodeRef<'gc>) {
        element
            .flags
            .set(element.flags.get() & !STYLE_ATTRIBUTE_DIRTY);
        let text = self
            .existing_element_data(element)
            .and_then(|data| {
                let cell = data.into_ref().inline_style.get();
                let block = cell.take()?;
                let text = block.css_text();
                cell.set(Some(block));
                Some(text)
            })
            .unwrap_or_default();
        self.write_attribute_quietly(element, STYLE_ATTRIBUTE, &text);
    }

    /// `element`'s `style` attribute changed (`setAttribute`, the parser,
    /// removed): its declarations are parsed from it again when asked for.
    #[cold]
    #[inline(never)]
    fn inline_style_attribute_changed(&self, element: NodeRef<'gc>) {
        element
            .flags
            .set(element.flags.get() & !(HAS_INLINE_STYLE | STYLE_ATTRIBUTE_DIRTY));
        if let Some(data) = self.existing_element_data(element) {
            drop(data.into_ref().inline_style.get().take());
        }
    }

    /// `element`'s blocking wheel listeners (its own, or the document's and
    /// window's of the root) may have changed: styled again, if connected
    /// (what is below it follows if its `inside_blocking_wheel_event_handler`
    /// changes: inherited).
    pub(crate) fn wheel_listener_styled(&self, element: NodeRef<'gc>) {
        self.tree_changed();
        if self.is_connected(element) {
            self.mark_style(element, StyleFlags::ATTRIBUTES_CHANGED);
        }
    }

    /// `host` got a shadow root: its tree changed, and its boxes are built
    /// again (as its flat tree's children are its shadow tree's).
    pub(crate) fn shadow_attached(&self, host: NodeRef<'gc>) {
        self.tree_changed();
        if self.dom_changes_wanted() {
            self.note_shadow_attached(host);
        }
        // Its children in the flat tree are its shadow root's now, none
        // yet (each marks itself as it comes): its boxes made again.
        if self.is_connected(host) {
            self.set_needs_reattach(host);
        }
    }

    /// `slot`'s children in the flat tree changed (its assigned nodes, or
    /// its fallback while it has none): styled and laid out again below.
    pub(crate) fn flat_tree_children_changed(&self, slot: NodeRef<'gc>) {
        self.tree_changed();
        if self.is_connected(slot) {
            // Style: what moved between slots is marked as the assignment
            // is worked out (`assign_slottables`); the slot's own style
            // doesn't change. Its fallback may come into the flat tree,
            // never styled there: styled, the slot walked down to.
            let needs = StyleFlags::NEEDS_STYLE_RECALC.0 << STYLE_SHIFT;
            let mut child = self.first_child(slot);
            while let Some(at) = child {
                if self.kind(at) == NodeKind::Element {
                    self.set_flags(at, needs);
                }
                child = self.next_sibling(at);
            }
            self.set_flags(slot, StyleFlags::CHILD_NEEDS_STYLE_RECALC.0 << STYLE_SHIFT);
            self.mark_style_path(slot);
            // A node that comes into the flat tree may have flags of its own
            // that went up no further (it had no parent in it): the slot's
            // child bit has the build clear them.
            self.mark_layout(
                slot,
                LayoutTreeFlags(
                    LayoutTreeFlags::NEEDS_REATTACH.0 | LayoutTreeFlags::CHILD_NEEDS_UPDATE.0,
                ),
            );
        }
    }

    /// An attribute named `name` was added to `element`: an `id` flags it.
    #[inline]
    pub(crate) fn attribute_added(&self, element: NodeRef<'gc>, name: QualifiedName) {
        if name == ID_ATTRIBUTE {
            element.flags.set(element.flags.get() | HAS_ID);
        }
    }

    /// `element`'s id changed from `old` in its document's tree: its
    /// document's table follows. The old value is still the values table's
    /// (the caller holds it). Out of line: most attributes aren't ids.
    #[inline(never)]
    fn connected_id_changed(&self, element: NodeRef<'gc>, old: Option<&str>) {
        // One in a shadow tree is its shadow root's, not its document's.
        if self.shadow_scope(element).is_some() {
            return;
        }
        let document = self.document_of(element);
        if let Some(old) = old.filter(|old| !old.is_empty())
            && let Some(old) = crate::find_value(old)
        {
            self.id_removed(document, element, old.addr());
        }
        self.id_added(document, element);
    }

    /// `element`'s `name` changed from `old`: if it is one whose name the
    /// window's named access finds (`ids::name_table`), the names are
    /// looked at from now on, and its document's table follows if it is in
    /// its tree. Out of line: most elements' names aren't these.
    #[inline(never)]
    fn name_attribute_changed(&self, element: NodeRef<'gc>, old: Option<&str>) {
        let Some(table) = self.name_table(element) else {
            return;
        };
        self.see(crate::form::NAMES_SEEN);
        // One in a shadow tree is its shadow root's, not its document's.
        if self.is_connected(element) && self.shadow_scope(element).is_none() {
            self.connected_name_changed(self.document_of(element), element, table, old);
        }
    }

    /// Whether `name` is an attribute `element`'s box takes when it is
    /// built (`connected_attribute_changed`).
    fn is_box_attribute(&self, element: NodeRef<'gc>, name: QualifiedName) -> bool {
        use names::local;
        let is = |local| self.qualified_name(element) == Some(QualifiedName::html(local));
        if is(local::TD) || is(local::TH) {
            name == QualifiedName::plain(local::COLSPAN)
                || name == QualifiedName::plain(local::ROWSPAN)
        } else if is(local::COL) || is(local::COLGROUP) {
            name == QualifiedName::plain(local::SPAN)
        } else if is(local::INPUT) || is(local::SELECT) {
            name == QualifiedName::plain(local::SIZE)
        } else if is(local::TEXTAREA) {
            name == QualifiedName::plain(local::ROWS) || name == QualifiedName::plain(local::COLS)
        } else {
            false
        }
    }

    fn connected_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        if self.owns_style_sheets(element) {
            if name == QualifiedName::plain(names::local::MEDIA) {
                self.sheet_owner_media_changed(element, new.unwrap_or_default());
            }
            if element.qualified_name() == LINK_ELEMENT {
                self.element_asked_for_load(element, false);
            }
            self.style_sheets_changed(element);
        }
        // Attributes its box takes when it is built: a table cell's
        // `colspan` and `rowspan`, and a column's or column group's `span`
        // (Blink's `HTMLTableCellElement::ParseAttribute` and
        // `HTMLTableColElement::ParseAttribute` lay the table out again); a
        // text field's or list box's `size`, a text area's `rows` and `cols`
        // (its intrinsic size: `SetNeedsLayoutAndIntrinsicWidthsRecalc`).
        if self.is_box_attribute(element, name) {
            self.set_needs_reattach(element);
        }
        if self
            .style_flags(element)
            .contains(StyleFlags::NEEDS_STYLE_RECALC)
        {
            return;
        }
        match self.style_names(element) {
            Some(names) if names.uses(name, old, new) => {
                self.note_for_style(element, |changes| changes.note(name, old, new));
            }
            Some(_) => {
                #[cfg(debug_assertions)]
                self.passed_over(element, |changes| changes.note(name, old, new));
            }
            None => {}
        }
    }

    /// The names style reads of `document`'s elements, from now on: style's,
    /// as its sheets change.
    pub fn set_style_names(&self, document: NodeRef<'gc>, names: StyleNames) {
        let data = self.ensure_document_data(document);
        let names = self.gc.alloc_in(
            &sites::OTHER,
            StyleNamesDataInit {
                names: External::new(names),
            },
        );
        data.style_names.set(self.gc, Some(names));
    }

    /// What style reads of `element`'s document's elements, none if it
    /// hasn't been styled.
    #[inline]
    pub(crate) fn style_names(&self, element: NodeRef<'gc>) -> Option<&'gc StyleNames> {
        let data = self.document_data(self.document_of(element))?;
        Some(
            data.into_ref()
                .style_names
                .get(self.gc)?
                .into_ref()
                .names
                .get(),
        )
    }

    /// The debug build's: `note` notes a change of `element` the style
    /// names passed over, for style to check.
    #[cfg(debug_assertions)]
    fn passed_over(&self, element: NodeRef<'gc>, note: impl FnOnce(&mut AttributeChanges)) {
        let mut changes = AttributeChanges::default();
        note(&mut changes);
        let document = self.node_id(self.document_of(element));
        let element = self.node_id(element);
        PASSED_OVER.with(|passed| passed.borrow_mut().push((document, element, changes)));
    }

    /// The changes of `document`'s elements the style names passed over
    /// since the last time, with the element, taken (the debug build's, for
    /// style to check that it reads none of them).
    #[cfg(debug_assertions)]
    pub fn take_passed_over_changes(
        &self,
        document: NodeRef<'gc>,
    ) -> Vec<(NodeId, AttributeChanges)> {
        let document = self.node_id(document);
        PASSED_OVER.with(|passed| {
            let mut passed = passed.borrow_mut();
            let (taken, kept) = std::mem::take(&mut *passed)
                .into_iter()
                .partition::<Vec<_>, _>(|(of, ..)| *of == document);
            *passed = kept;
            taken
                .into_iter()
                .map(|(_, element, changes)| (element, changes))
                .collect()
        })
    }

    /// `element`'s `:hover`, `:active`, a form state... (`state`,
    /// `AttributeChanges` bits) changed: noted for the style phase as an attribute's change is
    /// (Blink's `PseudoStateChanged`), if it is connected and not to be
    /// styled whole anyway.
    pub(crate) fn state_changed(&self, element: NodeRef<'gc>, state: u16) {
        self.tree_changed();
        if !self.is_connected(element) {
            return;
        }
        if state & AttributeChanges::LOOK != 0 {
            self.control_look_changed(element);
        }
        if self
            .style_flags(element)
            .contains(StyleFlags::NEEDS_STYLE_RECALC)
        {
            return;
        }
        match self.style_names(element) {
            Some(names) if names.everything || names.states & state != 0 => {
                self.note_for_style(element, |changes| changes.states |= state);
            }
            Some(_) => {
                #[cfg(debug_assertions)]
                self.passed_over(element, |changes| changes.states |= state);
            }
            None => {}
        }
    }

    /// A state a control's native look shows changed (`AttributeChanges::
    /// LOOK`): flagged for layout (`LayoutTreeFlags::
    /// CONTROL_STATE_CHANGED`) if it is a form control, or a part of one's
    /// UA shadow tree (Blink's `LayoutTheme::ControlStateChanged` marks
    /// the layout object of one with a native look; the DOM doesn't know
    /// the style).
    pub(crate) fn control_look_changed(&self, element: NodeRef<'gc>) {
        let control = self.form_kind(element) & crate::form::LISTED != 0
            || (element.scope.get() & crate::shadow::IN_SHADOW_TREE != 0
                && self.shadow_pseudo_id(element).is_some());
        if control {
            self.mark_layout(element, LayoutTreeFlags::CONTROL_STATE_CHANGED);
        }
    }

    /// What `note` notes of `element`'s change kept for the style phase,
    /// and it, its next sibling and its parent marked.
    fn note_for_style(&self, element: NodeRef<'gc>, note: impl FnOnce(&mut AttributeChanges)) {
        let data = self.element_data(element);
        let mut changes = data.changes.get().take().unwrap_or_default();
        note(&mut changes);
        data.changes.get().set(Some(changes));
        // Its later siblings: what the style's sibling invalidation sets
        // find of them as the walk passes it (no mark here).
        self.mark_style(element, StyleFlags::ATTRIBUTES_CHANGED);
        if let Some(parent) = self.parent(element) {
            self.mark_style(parent, StyleFlags::CHILDREN_ATTRIBUTES_CHANGED);
        }
    }

    /// A character data node's data changed: its parent's children did
    /// (`:empty`), its boxes are built again, a `<style>`'s text is a new
    /// sheet, a textarea's may be a new value, an option's a new label.
    pub(crate) fn data_changed(&self, node: NodeRef<'gc>) {
        self.tree_changed();
        if self.dom_changes_wanted() {
            self.note_character_data(node);
        }
        if !self.is_connected(node) {
            return;
        }
        if self.kind(node) != NodeKind::Text {
            self.other_data_changed(node);
            return;
        }
        self.mark_layout(node, LayoutTreeFlags::NEEDS_REATTACH);
        let Some(parent) = self.parent(node) else {
            return;
        };
        self.mark_style(parent, StyleFlags::CHILDREN_CHANGED);
        if self.owns_style_sheets(parent) {
            self.style_children_changed(parent);
            self.style_sheets_changed(parent);
        }
        if self.has_seen_uses() {
            self.svg_use_subtree_changed(parent);
        }
        if parent.scope.get() & crate::text_control::TEXTAREA_CHILDREN != 0 {
            self.textarea_children_changed(parent);
        }
        self.option_text_changed(parent);
    }

    /// A comment's or processing instruction's data changed: its parent's
    /// children changed steps that see it (DOM's "replace data" runs them
    /// for any character data, as Blink's `CharacterData::DidModifyData`
    /// does): a `<style>`'s sheet made again, a `<use>`'s copy. No boxes,
    /// `:empty` or text reads it.
    #[cold]
    #[inline(never)]
    fn other_data_changed(&self, node: NodeRef<'gc>) {
        let Some(parent) = self.parent(node) else {
            return;
        };
        if self.owns_style_sheets(parent) {
            self.style_children_changed(parent);
            self.style_sheets_changed(parent);
        }
        if self.has_seen_uses() {
            self.svg_use_subtree_changed(parent);
        }
    }
}
