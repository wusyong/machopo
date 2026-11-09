//! The numbers list items' markers show (Blink's `ListItemOrdinal`):
//! worked out list by list, when a marker shows one, and kept until the
//! list changes. A list is an element whose `counter-reset` makes the
//! `list-item` counter its items count (an `<ol>`, `<ul>` or `<menu>`, as
//! the UA stylesheet has it); its items are the list items in it, but for
//! those in a list in it.
//!
//! That is css-lists 3's `list-item` counter while nothing else takes part
//! in it: in a list, every element's counters set has the list's (or a
//! list's in it), which a list before it in its parent doesn't replace
//! ("Inheriting Counters"). What is otherwise is `Irregular`, and the
//! counters' pass over the whole document works the numbers out then: a
//! list item outside a list (counted on a list before it, whose counter
//! goes on in its following siblings, or on one of its own), or another
//! element (or a pseudo-element) changing the counter or showing it.
//! Blink numbers a list item outside a list in its parent's list.

use computed_style::{ComputedStyle, Content, ContentItem, Display, PseudoId};
use dom::NodeRef;

use crate::tree_builder::Reader;
use crate::{NodeId, NodeIdHashMap};

/// A use of the `list-item` counter that numbering lists one by one
/// doesn't give as css-lists 3 does.
#[derive(Debug)]
pub(crate) struct Irregular;

/// The lists whose items' numbers were worked out, as they were.
#[derive(Default)]
pub(crate) struct ListOrdinals {
    /// Each list's items' values: the `list-item` counter's after each
    /// changed it.
    lists: NodeIdHashMap<NodeIdHashMap<i32>>,
    /// The list of each of those items.
    items: NodeIdHashMap<NodeId>,
    /// How many times a list was numbered (for tests).
    numbered: usize,
}

impl ListOrdinals {
    /// Forgets list `list`'s values; its items' markers may show others.
    pub(crate) fn forget(&mut self, list: NodeId) {
        if let Some(values) = self.lists.remove(&list) {
            for item in values.keys() {
                if self.items.get(item) == Some(&list) {
                    self.items.remove(item);
                }
            }
        }
    }

    /// How many times a list was numbered, for tests.
    pub(crate) fn numbered(&self) -> usize {
        self.numbered
    }

    /// The lists whose values are known.
    pub(crate) fn lists(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.lists.keys().copied()
    }

    /// The items of list `list`, if its values are known.
    pub(crate) fn items_of(&self, list: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.lists
            .get(&list)
            .into_iter()
            .flat_map(|values| values.keys().copied())
    }
}

/// Whether `style` makes a list: it resets `list-item` and has a box (one
/// with `display: contents` changes no counter).
pub(crate) fn is_list(style: &ComputedStyle) -> bool {
    style.display != Display::Contents
        && !style.display.generates_no_box()
        && style
            .rare
            .counter_reset
            .iter()
            .any(|reset| &*reset.name == "list-item")
}

/// Whether `old` and `new` give an element the same part in the
/// `list-item` counter: a list item or not, and the same resets, increments
/// and sets of it.
pub(crate) fn same_part(old: &ComputedStyle, new: &ComputedStyle) -> bool {
    let resets = |style: &ComputedStyle| -> Vec<(Option<i32>, bool)> {
        style
            .rare
            .counter_reset
            .iter()
            .filter(|reset| &*reset.name == "list-item")
            .map(|reset| (reset.value, reset.reversed))
            .collect()
    };
    let changes = |list: &[computed_style::CounterChange]| -> Vec<i32> {
        list.iter()
            .filter(|change| &*change.name == "list-item")
            .map(|change| change.value)
            .collect()
    };
    (old.display == Display::ListItem) == (new.display == Display::ListItem)
        && resets(old) == resets(new)
        && changes(&old.rare.counter_increment) == changes(&new.rare.counter_increment)
        && changes(&old.rare.counter_set) == changes(&new.rare.counter_set)
}

/// Whether `style` changes or shows the `list-item` counter other than as
/// a list item does: what a list's item, or a pseudo-element, mustn't.
fn changes_list_item(style: &ComputedStyle) -> bool {
    style
        .rare
        .counter_increment
        .iter()
        .chain(style.rare.counter_set.iter())
        .any(|change| &*change.name == "list-item")
}

fn shows_list_item(style: &ComputedStyle) -> bool {
    let Content::Items(items) = &style.rare.content else {
        return false;
    };
    items.iter().any(|item| match item {
        ContentItem::Counter { name, .. } | ContentItem::Counters { name, .. } => {
            &**name == "list-item"
        }
        _ => false,
    })
}

/// Whether an element uses the `list-item` counter as only a list or a
/// list item does: its pseudo-elements don't change or show it, nor does it
/// but as a list item. (A list resets it; an item increments it, or sets
/// it: an `<li value>`.) Style containment, which scopes it, is the
/// counters' (`counters`) to work out. Its style is `style`, and its
/// pseudo-elements' are as `pseudo_style` gives them.
pub(crate) fn regular_of(
    style: &ComputedStyle,
    pseudo_style: impl Fn(PseudoId) -> Option<std::sync::Arc<ComputedStyle>>,
) -> bool {
    let item = style.display == Display::ListItem;
    !style.rare.contain.contains(computed_style::Contain::STYLE)
        && (item || !changes_list_item(style))
        && !shows_list_item(style)
        && [PseudoId::Before, PseudoId::After, PseudoId::Marker]
            .into_iter()
            .filter_map(pseudo_style)
            .all(|pseudo| {
                !changes_list_item(&pseudo)
                    && !shows_list_item(&pseudo)
                    && !pseudo
                        .rare
                        .counter_reset
                        .iter()
                        .any(|reset| &*reset.name == "list-item")
            })
}

/// What a reversed counter without a value counts from, or where one
/// with a value is.
#[derive(Clone, Copy)]
enum Value {
    Known(i32),
    FromStart(i64),
}

/// A list being numbered, in tree order (the walk is `number`'s).
struct Numbering {
    /// Its reset of the counter's value.
    start: Option<i32>,
    reversed: bool,
    value: Value,
    /// Each item's increment and the value it set, in order (for a
    /// reversed list without a value).
    changes: Vec<(i32, Option<i32>)>,
    values: Vec<(NodeId, Value)>,
}

impl Numbering {
    /// The numbering of a list whose style is `style`, which uses the
    /// counter as a list does (`regular`).
    fn of(style: &ComputedStyle, regular: bool) -> Result<Self, Irregular> {
        // A list that is an item, or changes the counter itself, numbers
        // it in its parent's place too.
        if style.display == Display::ListItem || changes_list_item(style) || !regular {
            return Err(Irregular);
        }
        let reset = style
            .rare
            .counter_reset
            .iter()
            .rev()
            .find(|reset| &*reset.name == "list-item")
            .ok_or(Irregular)?;
        Ok(Self {
            start: reset.value,
            reversed: reset.reversed,
            value: reset.value.map_or(Value::FromStart(0), Value::Known),
            changes: Vec::new(),
            values: Vec::new(),
        })
    }

    /// The items' values, the list numbered.
    fn values(self) -> NodeIdHashMap<i32> {
        let start = match self.start {
            Some(start) => start,
            None => crate::counters::reversed_start(&self.changes),
        };
        self.values
            .into_iter()
            .map(|(item, value)| {
                let value = match value {
                    Value::Known(value) => value,
                    Value::FromStart(from) => {
                        crate::counters::clamp(i64::from(start).saturating_add(from))
                    }
                };
                (item, value)
            })
            .collect()
    }

    /// A list item: its increments of the counter (or the one a list item
    /// makes), then what it sets (`counters::Pass::change`).
    fn item(&mut self, item: NodeId, style: &ComputedStyle) {
        let mut increments: Vec<i32> = style
            .rare
            .counter_increment
            .iter()
            .filter(|change| &*change.name == "list-item")
            .map(|change| change.value)
            .collect();
        if increments.is_empty() {
            increments.push(if self.reversed { -1 } else { 1 });
        }
        let mut total: i32 = 0;
        for by in increments {
            self.value = match self.value {
                Value::Known(value) => Value::Known(value.checked_add(by).unwrap_or(value)),
                // A sum of i32s in an i64: past it only after 2^32 of them.
                Value::FromStart(from) => Value::FromStart(from.saturating_add(i64::from(by))),
            };
            total = total.saturating_add(by);
        }
        let mut set = None;
        for change in style
            .rare
            .counter_set
            .iter()
            .filter(|change| &*change.name == "list-item")
        {
            self.value = Value::Known(change.value);
            set = Some(change.value);
        }
        self.changes.push((total, set));
        self.values.push((item, self.value));
    }
}

impl ListOrdinals {
    /// List item `item`'s value, worked out with its list's if it isn't
    /// known.
    pub(crate) fn value<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        item: NodeRef<'gc>,
    ) -> Result<i32, Irregular> {
        let id = dom.id(item);
        if let Some(value) = self
            .items
            .get(&id)
            .and_then(|list| self.lists.get(list))
            .and_then(|values| values.get(&id))
        {
            return Ok(*value);
        }
        let list = enclosing_list(dom, item).ok_or(Irregular)?;
        self.numbered = self.numbered.wrapping_add(1);
        let values = number(dom, list)?;
        let value = *values.get(&id).ok_or(Irregular)?;
        let list = dom.id(list);
        self.forget(list);
        for &item in values.keys() {
            self.items.insert(item, list);
        }
        self.lists.insert(list, values);
        Ok(value)
    }
}

/// The list `node` is in: its nearest ancestor that is one.
pub(crate) fn enclosing_list<'gc>(
    dom: Reader<'_, 'gc>,
    node: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    let mut ancestor = dom.dom.flat_tree_parent(node);
    while let Some(a) = ancestor {
        if !dom.is_element(a) {
            return None;
        }
        if dom.style(a).is_some_and(|style| is_list(&style)) {
            return Some(a);
        }
        ancestor = dom.dom.flat_tree_parent(a);
    }
    None
}

/// Whether `element` uses the `list-item` counter as only a list or a list
/// item does (`list_items::regular_of`).
pub(crate) fn regular<'gc>(
    dom: Reader<'_, 'gc>,
    element: NodeRef<'gc>,
    style: &computed_style::ComputedStyle,
) -> bool {
    regular_of(style, |pseudo| dom.pseudo_style(element, pseudo))
}

/// The values of list `list`'s items, as css-lists 3 has the `list-item`
/// counter it resets (`counters::Pass` the same, for these).
fn number<'gc>(dom: Reader<'_, 'gc>, list: NodeRef<'gc>) -> Result<NodeIdHashMap<i32>, Irregular> {
    let style = dom.style(list).ok_or(Irregular)?;
    let mut numbering = Numbering::of(&style, regular(dom, list, &style))?;
    numbering.children_next(dom, list)?;
    Ok(numbering.values())
}

impl Numbering {
    /// `parent`'s children, those with boxes.
    fn children_next<'gc>(
        &mut self,
        dom: Reader<'_, 'gc>,
        parent: NodeRef<'gc>,
    ) -> Result<(), Irregular> {
        for child in dom.children(parent) {
            if !dom.is_element(child) {
                continue;
            }
            let Some(style) = dom.style(child) else {
                continue;
            };
            if !dom.has_boxes(child, &style) {
                continue;
            }
            if !regular(dom, child, &style) {
                return Err(Irregular);
            }
            // A list in it numbers its own items.
            if is_list(&style) {
                if style.display == Display::ListItem {
                    return Err(Irregular);
                }
                continue;
            }
            if style.display == Display::ListItem {
                self.item(dom.id(child), &style);
            }
            self.children_next(dom, child)?;
        }
        Ok(())
    }
}
