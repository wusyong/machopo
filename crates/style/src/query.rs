//! `querySelector`, `querySelectorAll`, `matches` and `closest` on the new
//! DOM (Blink's `SelectorQuery`): a selector list parsed once a thread and
//! kept (Blink's `SelectorQueryCache`), its names numbered on the thread;
//! a walk down from the root. A lone id in a document is found by its
//! table; a selector of compounds joined by descendants (or children) is
//! matched compound by compound as the walk goes down, each by the few
//! things it needs (Blink's `SelectorQuery::MatchCompound`); any other
//! matches each element in full, its matching context made once a query.

use std::cell::RefCell;
use std::rc::Rc;

use css::selector::Impl;
use dom::{Cx, NameId, NodeKind, NodeRef, QuirksMode, names};
use rustc_hash::FxHashMap;
use selectors::Element as _;
use selectors::attr::{AttrSelectorOperator, ParsedCaseSensitivity};
use selectors::matching::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags,
    QuirksMode as SelectorsQuirksMode, SelectorCaches, matches_selector_list,
};
use selectors::parser::{Combinator, Component, SelectorList};

use crate::selector::StyleElement;

/// How many parsed lists a thread keeps (Blink's `SelectorQueryCache` keeps
/// 256 a document): past it, they all go.
const CACHE_SIZE: usize = 256;

thread_local! {
    /// In a `RefCell`: a thread's, reached by every query with nothing to
    /// borrow it from; borrowed only to find or keep a list, which calls
    /// out to nothing.
    static CACHE: RefCell<FxHashMap<String, Rc<SelectorQuery>>> =
        RefCell::new(FxHashMap::default());
}

/// A parsed selector list, its names numbered on its thread, and how it is
/// matched: a lone id's fast path, its compounds.
pub struct SelectorQuery {
    list: SelectorList<Impl>,
    serial: u32,
    /// `#id` alone: in a document, found by its table.
    id: Option<String>,
    compounds: Option<Compounds>,
}

/// A list of one selector whose compounds are joined by descendant or
/// child combinators (Blink's `SelectorQuery::BuildCompounds`): each
/// compound's id, type, class and exact attribute, the leftmost first,
/// the subject last. A compound needs no more than what it keeps of its
/// simple selectors: what it doesn't keep (another class, a pseudo-class,
/// an attribute matched otherwise), and a child combinator taken as a
/// descendant one, leave the full match of each element they find
/// (`full`).
struct Compounds {
    compounds: Vec<Compound>,
    /// What a subtree's bloom filter (`Cx::subtree_bloom`) must have for
    /// its elements to match from each compound on: the bits of the
    /// attribute names and classes of that compound and those after it
    /// (Blink's `Compound::selector_filter`); without the classes', for a
    /// document in quirks mode (whose classes match without case).
    needs: Vec<(u32, u32)>,
    full: bool,
    /// Whether matching it in full records nothing (no pseudo-class looks
    /// at a state, which records the element's dependency on it): the
    /// debug build checks each element's result against the full match.
    checkable: bool,
}

/// One compound's simple selectors that need no matching context (Blink's
/// `SelectorQuery::Compound`), as `StyleElement` matches them.
#[derive(Default)]
struct Compound {
    id: Option<String>,
    /// The type as written and ASCII lowercase (an HTML element in an HTML
    /// document matches the latter).
    tag: Option<(NameId, NameId)>,
    /// A class, by number and as written (quirks mode compares it without
    /// ASCII case).
    class: Option<(NameId, String)>,
    /// `[name="value"]`, `[name="value" i]`: the attribute in no
    /// namespace, its value, whether without ASCII case.
    attribute: Option<(NameId, String, bool)>,
    /// The bloom bits of the attribute names its simple selectors need
    /// (those it keeps and the others), and of its classes.
    attribute_bits: u32,
    class_bits: u32,
}

/// The document's modes a walk matches in: its root's document's.
#[derive(Clone, Copy)]
struct Modes {
    quirks: bool,
    html: bool,
}

impl SelectorQuery {
    /// `text` parsed (a thread keeps what it parsed); `None` if it isn't a
    /// valid selector list (the DOM's `SyntaxError`).
    pub fn parse(text: &str) -> Option<Rc<Self>> {
        if let Some(kept) = CACHE.with_borrow(|cache| cache.get(text).cloned()) {
            return Some(kept);
        }
        let list = css::selector::parse_selector_list(text)?;
        let serial = names::thread_serial();
        for selector in list.slice() {
            crate::rule_index::number_names(selector, serial);
        }
        let id = lone_id(&list);
        let compounds = Compounds::of(&list);
        let query = Rc::new(Self {
            list,
            serial,
            id,
            compounds,
        });
        CACHE.with_borrow_mut(|cache| {
            if cache.len() >= CACHE_SIZE {
                cache.clear();
            }
            cache.insert(text.to_owned(), query.clone());
        });
        Some(query)
    }

    /// `querySelector`: the first element in tree order in `root`'s
    /// subtree (not `root` itself) that matches.
    pub fn query_first<'gc>(&self, cx: &Cx<'_, 'gc>, root: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if let Some(id) = &self.id
            && cx.kind(root) == NodeKind::Document
            && cx.is_connected(root)
        {
            return cx.element_by_id(root, id);
        }
        let mut found = None;
        self.walk(cx, root, |element| {
            found = Some(element);
            false
        });
        found
    }

    /// `querySelectorAll`: every element `query_first` would find first,
    /// in tree order.
    pub fn query_all<'gc>(&self, cx: &Cx<'_, 'gc>, root: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut found = Vec::new();
        self.walk(cx, root, |element| {
            found.push(element);
            true
        });
        found
    }

    /// `matches`: whether `element` matches, `:scope` being it.
    pub fn matches<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
        self.with_full_match(cx, element, Some(element), |matches| matches(element))
    }

    /// `closest`: `element` or its nearest ancestor element that matches,
    /// `:scope` being `element`.
    pub fn closest<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.with_full_match(cx, element, Some(element), |matches| {
            let mut at = Some(element);
            while let Some(node) = at.filter(|node| cx.kind(*node) == NodeKind::Element) {
                if matches(node) {
                    return Some(node);
                }
                at = cx.parent(node);
            }
            None
        })
    }

    /// Each element below `root` in tree order that matches, to `found`
    /// until it says to stop.
    fn walk<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        root: NodeRef<'gc>,
        mut found: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let scope = (cx.kind(root) == NodeKind::Element).then_some(root);
        self.with_full_match(cx, root, scope, |full| match &self.compounds {
            Some(compounds) => compounds.walk(cx, root, modes(cx, root), full, found),
            None => {
                let mut at = cx.first_child(root);
                while let Some(node) = at {
                    if cx.kind(node) == NodeKind::Element && counted(full)(node) && !found(node) {
                        return;
                    }
                    at = crate::next_in_tree_order(cx, node, root);
                }
            }
        });
    }

    /// `f` given what matches an element of `node`'s document in full,
    /// `:scope` being `scope` (the root element with none): the matching
    /// context made once, for every element `f` asks about (Blink's
    /// `SelectorChecker` made once a query).
    fn with_full_match<'gc, R>(
        &self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
        scope: Option<NodeRef<'gc>>,
        f: impl FnOnce(&mut dyn FnMut(NodeRef<'gc>) -> bool) -> R,
    ) -> R {
        let quirks = if modes(cx, node).quirks {
            SelectorsQuirksMode::Quirks
        } else {
            SelectorsQuirksMode::NoQuirks
        };
        let mut caches = SelectorCaches::default();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            quirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        let element = |node| StyleElement {
            cx,
            node,
            serial: self.serial,
            shadow_pseudo: false,
            selection: false,
        };
        context.scope_element = scope.map(|scope| element(scope).opaque());
        f(&mut |node| matches_selector_list(&self.list, &element(node), &mut context))
    }
}

thread_local! {
    /// How many elements queries' walks matched in full (not counting the
    /// debug build's checks), on this thread. Counted in debug builds only.
    static FULL_MATCHES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// How many elements compound walks matched a compound against (not
    /// those a bloom filter skipped), on this thread. Debug builds only.
    static COMPOUND_ELEMENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many elements, on this thread, walks of selectors of compounds
/// matched a compound against: what tells a subtree whose bloom filter
/// misses what a selector needs is skipped. `None` in release builds. For
/// tests.
pub fn compound_elements_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| COMPOUND_ELEMENTS.get())
}

/// `full`, counted.
fn counted<'a, 'gc>(
    full: &'a mut dyn FnMut(NodeRef<'gc>) -> bool,
) -> impl FnMut(NodeRef<'gc>) -> bool + 'a {
    move |node| {
        if cfg!(debug_assertions) {
            FULL_MATCHES.set(FULL_MATCHES.get().wrapping_add(1));
        }
        full(node)
    }
}

/// How many elements, on this thread, queries' walks matched in full:
/// what tells a selector of compounds is matched by them. `None` in
/// release builds. For tests.
pub fn full_matches_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| FULL_MATCHES.get())
}

/// The bloom bits a subtree needs for an attribute named `local`: none
/// for `style`, which the DOM writes back lazily and may not have counted
/// yet.
fn attribute_bits(local: &str) -> u32 {
    let local = names::intern(local);
    if local == names::local::STYLE {
        0
    } else {
        dom::attribute_bloom(local)
    }
}

/// Whether a bloom filter has every bit of `need`.
fn has(bloom: u32, need: u32) -> bool {
    bloom & need == need
}

/// `node`'s document's modes.
fn modes<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Modes {
    let document = cx.document_of(node);
    Modes {
        quirks: cx.quirks_mode(document) == QuirksMode::Quirks,
        html: cx.is_html_document(document),
    }
}

/// `#id`, if `list` is that alone.
fn lone_id(list: &SelectorList<Impl>) -> Option<String> {
    let [selector] = list.slice() else {
        return None;
    };
    let mut components = selector
        .iter_raw_match_order()
        .filter(|component| !matches!(component, Component::ExplicitUniversalType));
    match (components.next(), components.next()) {
        (Some(Component::ID(id)), None) => Some(id.0.clone()),
        _ => None,
    }
}

impl Compounds {
    /// `list`'s compounds, if it is one selector whose compounds are
    /// joined by descendant or child combinators.
    fn of(list: &SelectorList<Impl>) -> Option<Self> {
        let [selector] = list.slice() else {
            return None;
        };
        let mut compounds = Vec::new();
        let mut current = Compound::default();
        let mut full = false;
        let mut checkable = true;
        // The subject's compound first.
        for component in selector.iter_raw_match_order() {
            current.attribute_bits |= match component {
                Component::ID(_) => dom::attribute_bloom(names::ID),
                Component::AttributeInNoNamespace { local_name, .. } => {
                    attribute_bits(&local_name.0)
                }
                Component::AttributeInNoNamespaceExists {
                    local_name,
                    local_name_lower,
                } if local_name == local_name_lower => attribute_bits(&local_name.0),
                _ => 0,
            };
            if let Component::Class(class) = component {
                current.class_bits |= dom::class_bloom(&class.0);
            }
            match component {
                Component::Combinator(Combinator::Descendant) => {
                    compounds.push(std::mem::take(&mut current));
                }
                Component::Combinator(Combinator::Child) => {
                    compounds.push(std::mem::take(&mut current));
                    full = true;
                }
                // A sibling's, a pseudo-element's, a slot's or a part's.
                Component::Combinator(_) => return None,
                Component::ExplicitUniversalType => {}
                Component::ID(id) if current.id.is_none() => current.id = Some(id.0.clone()),
                Component::LocalName(name) if current.tag.is_none() => {
                    current.tag = Some((
                        names::intern(&name.name.0),
                        names::intern(&name.lower_name.0),
                    ));
                }
                Component::Class(class) if current.class.is_none() => {
                    current.class = Some((names::intern(&class.0), class.0.clone()));
                }
                Component::AttributeInNoNamespace {
                    local_name,
                    operator: AttrSelectorOperator::Equal,
                    value,
                    case_sensitivity,
                } if current.attribute.is_none()
                    && !matches!(
                        case_sensitivity,
                        ParsedCaseSensitivity::AsciiCaseInsensitiveIfInHtmlElementInHtmlDocument
                    ) =>
                {
                    current.attribute = Some((
                        names::intern(&local_name.0),
                        value.as_ref().to_owned(),
                        *case_sensitivity == ParsedCaseSensitivity::AsciiCaseInsensitive,
                    ));
                }
                other => {
                    full = true;
                    checkable &= matches!(
                        other,
                        Component::ID(_)
                            | Component::LocalName(_)
                            | Component::Class(_)
                            | Component::AttributeInNoNamespace { .. }
                            | Component::AttributeInNoNamespaceExists { .. }
                            | Component::AttributeOther(_)
                    );
                }
            }
        }
        compounds.push(current);
        compounds.reverse();
        let mut needs = vec![(0, 0); compounds.len()];
        let mut after = (0, 0);
        for (need, compound) in needs.iter_mut().zip(&compounds).rev() {
            after.0 |= compound.attribute_bits | compound.class_bits;
            after.1 |= compound.attribute_bits;
            *need = after;
        }
        Some(Self {
            compounds,
            needs,
            full,
            checkable,
        })
    }

    /// Each element below `root` in tree order that matches, to `found`
    /// until it says to stop (`full` matching it in full). The walk goes
    /// down with the compound each element is to match next: an element
    /// that matches it gives its descendants the next one, and the
    /// subject's compound, matched, is a match (in full, if needed). Taking
    /// the first ancestor that matches a compound is right with descendant
    /// combinators alone (a later one would only leave less to match).
    fn walk<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        root: NodeRef<'gc>,
        modes: Modes,
        full: &mut dyn FnMut(NodeRef<'gc>) -> bool,
        mut found: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let subject = self.compounds.len().saturating_sub(1);
        let mut next = self.matched_above(cx, root, modes);
        let need = |next: usize| {
            let (all, no_classes) = self.needs[next];
            if modes.quirks { no_classes } else { all }
        };
        // Nothing below the root has what it takes (Blink's early exit).
        if cx.kind(root) == NodeKind::Element && !has(cx.subtree_bloom(root), need(next)) {
            self.check_skipped(cx, root, full);
            return;
        }
        // The compound to match of each level the walk went down from.
        let mut levels = Vec::new();
        let mut at = cx.first_child(root);
        while let Some(node) = at {
            let mut below = next;
            let skip =
                cx.kind(node) == NodeKind::Element && !has(cx.subtree_bloom(node), need(next));
            if skip {
                // Neither it nor its descendants have what it takes.
                self.check_skipped(cx, node, full);
            } else if cx.kind(node) == NodeKind::Element {
                if cfg!(debug_assertions) {
                    COMPOUND_ELEMENTS.set(COMPOUND_ELEMENTS.get().wrapping_add(1));
                }
                let compound = &self.compounds[next];
                let matched = if next == subject {
                    compound.matches(cx, node, modes) && (!self.full || counted(full)(node))
                } else {
                    if compound.matches(cx, node, modes) {
                        below = next.saturating_add(1);
                    }
                    false
                };
                if cfg!(debug_assertions) && self.checkable {
                    assert_eq!(
                        matched,
                        full(node),
                        "a compound's match differs from the full one"
                    );
                }
                if matched && !found(node) {
                    return;
                }
            }
            if let Some(child) = cx.first_child(node).filter(|_| !skip) {
                levels.push(next);
                next = below;
                at = Some(child);
                continue;
            }
            // The next sibling of this node or of its nearest ancestor below
            // `root` that has one.
            let mut from = node;
            at = loop {
                if let Some(sibling) = cx.next_sibling(from) {
                    break Some(sibling);
                }
                match cx.parent(from) {
                    Some(parent) if !elise::Gc::ptr_eq(parent, root) => {
                        from = parent;
                        next = levels
                            .pop()
                            .expect("a level for each element gone down from");
                    }
                    _ => break None,
                }
            };
        }
    }

    /// In debug builds, for a selector whose full match records nothing:
    /// that no element in the subtree a bloom filter skipped (`node`'s, or
    /// a root's descendants) matches (Blink's
    /// `VerifyBloomFilterTreeConsistencyIncludingChildren`).
    fn check_skipped<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
        full: &mut dyn FnMut(NodeRef<'gc>) -> bool,
    ) {
        if !(cfg!(debug_assertions) && self.checkable) {
            return;
        }
        let mut at = Some(node);
        while let Some(element) = at {
            if cx.kind(element) == NodeKind::Element {
                assert!(
                    !full(element),
                    "a subtree its bloom filter skipped has a match"
                );
            }
            at = crate::next_in_tree_order(cx, element, node);
        }
    }

    /// The compound `root`'s descendants are to match first: past those
    /// that `root` and its ancestor elements match, from the top (Blink's
    /// `FirstCompoundNotSeenBeforeRoot`).
    fn matched_above<'gc>(&self, cx: &Cx<'_, 'gc>, root: NodeRef<'gc>, modes: Modes) -> usize {
        let subject = self.compounds.len().saturating_sub(1);
        let mut chain = Vec::new();
        let mut at = Some(root);
        while let Some(node) = at.filter(|node| cx.kind(*node) == NodeKind::Element) {
            chain.push(node);
            at = cx.parent(node);
        }
        let mut next = 0;
        for node in chain.into_iter().rev() {
            if next < subject && self.compounds[next].matches(cx, node, modes) {
                next = next.saturating_add(1);
            }
        }
        next
    }
}

impl Compound {
    /// Whether `element` matches what the compound keeps, as `StyleElement`
    /// matches each (Blink's `SelectorQuery::MatchCompound`).
    fn matches<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>, modes: Modes) -> bool {
        if let Some(id) = &self.id {
            let Some(value) = cx.attribute_value_by_id(element, names::ID) else {
                return false;
            };
            let value = value.borrow();
            let value = value.to_str();
            let same = if modes.quirks {
                value.eq_ignore_ascii_case(id)
            } else {
                *value == **id
            };
            if !same {
                return false;
            }
        }
        if let Some((name, lower)) = self.tag {
            let Some(qualified) = cx.qualified_name(element) else {
                return false;
            };
            let html = modes.html && qualified.namespace() == names::HTML_NAMESPACE;
            if qualified.local() != if html { lower } else { name } {
                return false;
            }
        }
        if let Some((number, name)) = &self.class {
            let classes = cx.class_names(element);
            let has = if modes.quirks {
                classes
                    .iter()
                    .any(|class| names::get(class.name).eq_ignore_ascii_case(name))
            } else {
                classes.iter().any(|class| class.name == *number)
            };
            if !has {
                return false;
            }
        }
        if let Some((local, wanted, insensitive)) = &self.attribute {
            let Some(value) = cx.attribute_value_by_id(element, *local) else {
                return false;
            };
            let value = value.borrow();
            let value = value.to_str();
            let same = if *insensitive {
                value.eq_ignore_ascii_case(wanted)
            } else {
                *value == **wanted
            };
            if !same {
                return false;
            }
        }
        true
    }
}
