//! CSS counters (css-lists 3, "Automatic Numbering With Counters"; Blink's
//! `CountersAttachmentContext`): worked out over the whole document in tree
//! order, as the box tree is built, into the text of what shows them: a
//! list item's marker, and a `::before` or `::after` with `counter()` or
//! `counters()` in its `content`.
//!
//! Only the elements that generate boxes take part, as in `collect`: not
//! one with `display: none` or below it, nor the children of a replaced
//! element. An element's `::marker`, then its `::before`, come right after
//! it, and its `::after` after its children: the pseudo-elements are its
//! first and last children.
//!
//! An element with style containment scopes the counters in its subtree
//! (css-contain 2, "style containment"; Blink's `CountersAttachmentContext`):
//! what is in it increments or sets of a counter from outside it (its own
//! included) is a new counter instead, which its following siblings have
//! too, and the counters made there end with it.
//!
//! The `list-item` counter is the lists' own (`list_items`, Blink's
//! `ListItemOrdinal`) while it is only theirs: a pass for the others then
//! leaves it out (`lists`), and a whole document's runs only when they
//! change (Blink's `MarkCountersDirty`).

use std::sync::Arc;

use computed_style::{
    ComputedStyle, Contain, Content, ContentItem, Display, ListStyleType, PseudoId,
};
use dom::NodeRef;

use crate::tree_builder::Reader;
use crate::{NodeId, NodeIdHashMap};

/// What a pass over the document found.
#[derive(Default)]
pub(crate) struct Counters {
    /// The text each pseudo-element showing a counter generates, and each
    /// list item's marker's (none for one with nothing to show).
    pub(crate) texts: rustc_hash::FxHashMap<(NodeId, PseudoId), Arc<str>>,
    /// The elements that change a counter, are list items, or have a
    /// pseudo-element that changes or shows one, with what of their styles
    /// the counters depend on (`relevant_of`, `Directives`): only another
    /// of those, one of them going, or such an element coming, changes the
    /// counters (Blink's `MarkCountersDirty`).
    pub(crate) relevant: NodeIdHashMap<Directives>,
    /// Whether the `list-item` counter is the lists' own: the pass leaves
    /// it out, and `relevant` has only what takes part in the others.
    pub(crate) lists: bool,
}

/// What of an element's style, and its pseudo-elements', the counters
/// depend on: the counters it changes, its list item's marker, and its
/// `::before`'s and `::after`'s counters and `content`. (Another `display`
/// builds its boxes again, which the box tree looks at.)
#[derive(PartialEq)]
pub(crate) struct Directives {
    own: Changes,
    /// Whether it has style containment.
    contained: bool,
    marker: Option<(Content, ListStyleType)>,
    before: Option<(bool, Changes, Content)>,
    after: Option<(bool, Changes, Content)>,
}

/// A style's `counter-reset`, `counter-increment` and `counter-set`.
type Changes = (
    Arc<[computed_style::CounterReset]>,
    Arc<[computed_style::CounterChange]>,
    Arc<[computed_style::CounterChange]>,
);

/// What of an element's style the counters depend on, if it takes part in
/// them ([`relevant_of`]); with `lists`, those but `list-item`. Its style
/// is `style`, and its pseudo-elements' are as `pseudo_style` gives them.
pub(crate) fn directives_of(
    style: &ComputedStyle,
    pseudo_style: impl Fn(PseudoId) -> Option<Arc<ComputedStyle>>,
    lists: bool,
) -> Option<Directives> {
    if !relevant_of(style, &pseudo_style, lists) {
        return None;
    }
    let changes = |style: &ComputedStyle| -> Changes {
        if !lists {
            return (
                style.rare.counter_reset.clone(),
                style.rare.counter_increment.clone(),
                style.rare.counter_set.clone(),
            );
        }
        (
            style
                .rare
                .counter_reset
                .iter()
                .filter(|reset| &*reset.name != "list-item")
                .cloned()
                .collect(),
            style
                .rare
                .counter_increment
                .iter()
                .filter(|change| &*change.name != "list-item")
                .cloned()
                .collect(),
            style
                .rare
                .counter_set
                .iter()
                .filter(|change| &*change.name != "list-item")
                .cloned()
                .collect(),
        )
    };
    let pseudo = |pseudo| {
        pseudo_style(pseudo).map(|style| {
            (
                style.display.generates_no_box(),
                changes(&style),
                style.rare.content.clone(),
            )
        })
    };
    Some(Directives {
        own: changes(style),
        contained: style.rare.contain.contains(Contain::STYLE),
        // A `normal` one shows the lists' own number.
        marker: pseudo_style(PseudoId::Marker)
            .filter(|marker| !lists || matches!(marker.rare.content, Content::Items(_)))
            .map(|marker| {
                (
                    marker.rare.content.clone(),
                    marker.rare_inherited.list_style_type.clone(),
                )
            }),
        before: pseudo(PseudoId::Before),
        after: pseudo(PseudoId::After),
    })
}

/// What a pass found: the texts, each with its pseudo-element (in tree
/// order), the elements that take part (`Counters::relevant`), and
/// whether something uses the `list-item` counter as the lists can't have
/// it their own (`list_items::Irregular`; found with `lists` only).
pub(crate) type Found<'gc> = (
    Vec<(NodeRef<'gc>, PseudoId, Arc<str>)>,
    NodeIdHashMap<Directives>,
    bool,
);

/// Whether an element's style, or one of its pseudo-elements', changes or
/// shows a counter, or it is a list item; with `lists`, a counter but
/// `list-item` (a list item's marker showing it is the lists'). Its style
/// is `style`, and its pseudo-elements' are as `pseudo_style` gives them.
pub(crate) fn relevant_of(
    style: &ComputedStyle,
    pseudo_style: impl Fn(PseudoId) -> Option<Arc<ComputedStyle>>,
    lists: bool,
) -> bool {
    (!lists && style.display == Display::ListItem)
        || style.rare.contain.contains(Contain::STYLE)
        || changes_counters(style, lists)
        || (lists && pseudo_style(PseudoId::Marker).is_some_and(|marker| shows_counters(&marker)))
        || [PseudoId::Before, PseudoId::After]
            .into_iter()
            .filter_map(pseudo_style)
            .any(|style| changes_counters(&style, lists) || shows_counters(&style))
}

/// Whether `style` changes a counter (with `lists`, one but `list-item`).
fn changes_counters(style: &ComputedStyle, lists: bool) -> bool {
    let other = |name: &str| !lists || name != "list-item";
    style
        .rare
        .counter_reset
        .iter()
        .any(|reset| other(&reset.name))
        || style
            .rare
            .counter_increment
            .iter()
            .chain(style.rare.counter_set.iter())
            .any(|change| other(&change.name))
}

fn shows_counters(style: &ComputedStyle) -> bool {
    let Content::Items(items) = &style.rare.content else {
        return false;
    };
    items.iter().any(|item| {
        matches!(
            item,
            ContentItem::Counter { .. } | ContentItem::Counters { .. }
        )
    })
}

/// The text of a marker with `content: normal` (css-lists 3, "Markers"):
/// its `list-style-type`'s marker for the list item's `list-item` counter,
/// or the string; `None` for `none`.
fn marker_pieces(style: &ListStyleType, list_item: Option<(usize, Value)>) -> Option<Vec<Piece>> {
    match style {
        ListStyleType::None => None,
        ListStyleType::String(string) => Some(vec![Piece::Text(string.to_string())]),
        style => Some(vec![Piece::Counters {
            values: list_item.into_iter().collect(),
            separator: Arc::from(""),
            style: style.clone(),
            marker: true,
        }]),
    }
}

/// A counter's value as far as the pass got: known, or, for a reversed
/// counter without a value, so much from the start it works out once the
/// pass has seen all of its changes.
#[derive(Debug, Clone, Copy)]
enum Value {
    Known(i32),
    FromStart(i64),
}

/// A counter (css-lists 3, "CSS counters set": a name, the element it
/// originated on, a value), which each element whose set has it shares.
struct Instance {
    name: Arc<str>,
    /// The parent of the element (or pseudo-element) it originated on:
    /// one that element's following siblings reset again takes its place.
    origin_parent: NodeId,
    reversed: bool,
    /// `None` for a reversed one without a value, until the pass ends.
    start: Option<i32>,
    value: Value,
    /// For one without a start: each element's increment of it and the
    /// value it set, in order.
    changes: Vec<(i32, Option<i32>)>,
    /// Made in place of one from outside a style containment: its
    /// element's following siblings have it whatever their parent has.
    contained: bool,
}

/// A piece of generated text: as is, or counters' values.
enum Piece {
    Text(String),
    Counters {
        values: Vec<(usize, Value)>,
        separator: Arc<str>,
        style: ListStyleType,
        /// A marker's: with the style's suffix.
        marker: bool,
    },
}

/// An element's (or pseudo-element's) CSS counters set: its counters, by
/// their index in `Pass::instances`, outermost first.
type Set = Vec<usize>;

/// A pass over a document: walking the DOM in the flat tree's order, the
/// counters.
struct Pass<'gc> {
    instances: Vec<Instance>,
    pending: Vec<(NodeRef<'gc>, PseudoId, Vec<Piece>)>,
    relevant: NodeIdHashMap<Directives>,
    /// Leaving out `list-item`, the lists' own (`Counters::lists`).
    lists: bool,
    /// Something uses `list-item` as the lists can't have it their own.
    irregular: bool,
    /// The first of `instances` made inside the innermost element with
    /// style containment the pass is in (0 outside any): those before it
    /// are from outside, which isn't changed there.
    contained_from: usize,
}

impl<'gc> Pass<'gc> {
    fn new(lists: bool) -> Self {
        Self {
            instances: Vec::new(),
            pending: Vec::new(),
            relevant: NodeIdHashMap::default(),
            lists,
            irregular: false,
            contained_from: 0,
        }
    }

    /// The pass is over: the texts of what shows counters, the elements
    /// that take part, and whether the lists can't number their own.
    fn found(mut self) -> Found<'gc> {
        self.finish();
        let pending = std::mem::take(&mut self.pending);
        let texts = pending
            .into_iter()
            .map(|(element, pseudo, pieces)| (element, pseudo, self.text(&pieces).into()))
            .collect();
        (texts, self.relevant, self.irregular)
    }
}

impl<'gc> Pass<'gc> {
    /// The set an element starts with (css-lists 3, "Inheriting
    /// Counters"): its parent's, and those of its preceding sibling's
    /// whose names the parent's hasn't. Each counter's value is the latest,
    /// the one the element just before in tree order left it at.
    fn inherit(&self, parent_set: &Set, sibling_set: &Set) -> Set {
        let mut set = parent_set.clone();
        for &instance in sibling_set {
            let counter = &self.instances[instance];
            if (counter.contained && !set.contains(&instance))
                || !parent_set
                    .iter()
                    .any(|&i| self.instances[i].name == counter.name)
            {
                set.push(instance);
            }
        }
        set
    }

    /// The `content` items of a pseudo-element of `element` whose set is
    /// `set`: a counter it hasn't is made on it, at 0.
    fn items(&mut self, set: &mut Set, element: NodeId, items: &[ContentItem]) -> Vec<Piece> {
        items
            .iter()
            .map(|item| match item {
                ContentItem::String(string) => Piece::Text(string.to_string()),
                ContentItem::Attr { value, .. } => Piece::Text(value.to_string()),
                ContentItem::Counter { name, style } => {
                    let instance = self.innermost_or_new(set, name, element);
                    Piece::Counters {
                        values: vec![(instance, self.instances[instance].value)],
                        separator: Arc::from(""),
                        style: style.clone(),
                        marker: false,
                    }
                }
                ContentItem::Counters {
                    name,
                    separator,
                    style,
                } => {
                    self.innermost_or_new(set, name, element);
                    let values = set
                        .iter()
                        .filter(|&&i| self.instances[i].name == *name)
                        .map(|&i| (i, self.instances[i].value))
                        .collect();
                    Piece::Counters {
                        values,
                        separator: separator.clone(),
                        style: style.clone(),
                        marker: false,
                    }
                }
            })
            .collect()
    }

    /// The counters `style` resets, increments and sets, in that order,
    /// on an element (or pseudo-element) whose parent is `parent` and
    /// whose set is `set`; a list item increments `list-item` unless it
    /// names it (by -1 if it is reversed).
    fn change(&mut self, set: &mut Set, style: &ComputedStyle, parent: NodeId, list_item: bool) {
        // Leaving out the lists' own.
        let lists = self.lists;
        let counted = |name: &str| !lists || name != "list-item";
        for reset in style
            .rare
            .counter_reset
            .iter()
            .filter(|reset| counted(&reset.name))
        {
            self.instantiate(set, &reset.name, parent, reset.value, reset.reversed);
        }
        let mut increments: Vec<(Arc<str>, i32)> = style
            .rare
            .counter_increment
            .iter()
            .filter(|change| counted(&change.name))
            .map(|change| (change.name.clone(), change.value))
            .collect();
        if list_item && !lists && !increments.iter().any(|(name, _)| &**name == "list-item") {
            // One from outside a style containment isn't changed here: the
            // new one counts up.
            let reversed = self
                .innermost(set, "list-item")
                .filter(|&instance| instance >= self.contained_from)
                .is_some_and(|instance| self.instances[instance].reversed);
            increments.push((Arc::from("list-item"), if reversed { -1 } else { 1 }));
        }
        // What this element did to each counter it changed: its increment,
        // and the value it set.
        let mut changed: Vec<(usize, i32, Option<i32>)> = Vec::new();
        for (name, by) in increments {
            let instance = self.innermost_in_scope_or_new(set, &name, parent);
            let counter = &mut self.instances[instance];
            // An increment past what an integer holds leaves the value as it
            // is.
            counter.value = match counter.value {
                Value::Known(value) => Value::Known(value.checked_add(by).unwrap_or(value)),
                // A sum of i32s in an i64: past it only after 2^32 of them.
                Value::FromStart(from) => Value::FromStart(from.saturating_add(i64::from(by))),
            };
            match changed.iter_mut().find(|(i, ..)| *i == instance) {
                Some((_, total, _)) => *total = total.saturating_add(by),
                None => changed.push((instance, by, None)),
            }
        }
        for change in style
            .rare
            .counter_set
            .iter()
            .filter(|change| counted(&change.name))
        {
            let instance = self.innermost_in_scope_or_new(set, &change.name, parent);
            self.instances[instance].value = Value::Known(change.value);
            match changed.iter_mut().find(|(i, ..)| *i == instance) {
                Some((_, _, to)) => *to = Some(change.value),
                None => changed.push((instance, 0, Some(change.value))),
            }
        }
        for (instance, by, to) in changed {
            let counter = &mut self.instances[instance];
            if counter.start.is_none() {
                counter.changes.push((by, to));
            }
        }
    }

    /// The innermost counter named `name` in `set`.
    fn innermost(&self, set: &Set, name: &str) -> Option<usize> {
        set.iter()
            .rev()
            .copied()
            .find(|&i| &*self.instances[i].name == name)
    }

    /// The innermost counter named `name` in `set` that what is in the
    /// innermost style containment may change, or a new one at 0: one from
    /// outside isn't.
    fn innermost_in_scope_or_new(
        &mut self,
        set: &mut Set,
        name: &Arc<str>,
        parent: NodeId,
    ) -> usize {
        match self.innermost(set, name) {
            Some(instance) if instance >= self.contained_from => instance,
            Some(_) => {
                let instance = self.instantiate(set, name, parent, Some(0), false);
                self.instances[instance].contained = true;
                instance
            }
            None => self.instantiate(set, name, parent, Some(0), false),
        }
    }

    fn innermost_or_new(&mut self, set: &mut Set, name: &Arc<str>, parent: NodeId) -> usize {
        match self.innermost(set, name) {
            Some(instance) => instance,
            None => self.instantiate(set, name, parent, Some(0), false),
        }
    }

    /// A new counter named `name` in `set`, of an element whose parent is
    /// `parent`: it takes the place of the innermost one of that name if
    /// that one originated on the element or a preceding sibling.
    fn instantiate(
        &mut self,
        set: &mut Set,
        name: &Arc<str>,
        parent: NodeId,
        start: Option<i32>,
        reversed: bool,
    ) -> usize {
        if let Some(i) = set.iter().rposition(|&i| self.instances[i].name == *name)
            && self.instances[set[i]].origin_parent == parent
        {
            set.remove(i);
        }
        let instance = self.instances.len();
        self.instances.push(Instance {
            name: name.clone(),
            origin_parent: parent,
            reversed,
            start,
            value: start.map_or(Value::FromStart(0), Value::Known),
            changes: Vec::new(),
            contained: false,
        });
        set.push(instance);
        instance
    }

    /// The pass is over: a reversed counter without a value starts at what
    /// its changes add up to (css-lists 3, "Instantiating Counters": each
    /// element's increment, negated, up to the first one that sets it,
    /// which adds the value it sets; then the last nonzero increment,
    /// negated): 1 at an ordered list's last item.
    fn finish(&mut self) {
        for counter in &mut self.instances {
            if counter.start.is_none() {
                counter.start = Some(reversed_start(&counter.changes));
            }
        }
    }

    fn value(&self, instance: usize, value: Value) -> i32 {
        match value {
            Value::Known(value) => value,
            Value::FromStart(from) => {
                let start = self.instances[instance].start.unwrap_or(0);
                clamp(i64::from(start).saturating_add(from))
            }
        }
    }

    fn text(&self, pieces: &[Piece]) -> String {
        let mut text = String::new();
        for piece in pieces {
            match piece {
                Piece::Text(string) => text.push_str(string),
                Piece::Counters {
                    values,
                    separator,
                    style,
                    marker,
                } => {
                    for (i, &(instance, value)) in values.iter().enumerate() {
                        if i > 0 {
                            text.push_str(separator);
                        }
                        let value = self.value(instance, value);
                        text.push_str(&represent(value, style));
                    }
                    if *marker {
                        text.push_str(suffix(style));
                    }
                }
            }
        }
        text
    }
}

/// Where a reversed counter without a value starts, from what its
/// changes add up to (css-lists 3, "Instantiating Counters": each
/// element's increment, negated, up to the first one that sets it, which
/// adds the value it sets; then the last nonzero increment, negated): 1 at
/// an ordered list's last item. `changes`: each element's increment of it
/// and the value it set, in order.
pub(crate) fn reversed_start(changes: &[(i32, Option<i32>)]) -> i32 {
    let mut start: i64 = 0;
    let mut last_negated: i64 = 0;
    for &(by, to) in changes {
        // An i32 negated in an i64 fits.
        let negated = i64::from(by).wrapping_neg();
        if negated != 0 {
            last_negated = negated;
        }
        // Sums of i32s in an i64: past it only after 2^32 of them.
        if let Some(to) = to {
            start = start.saturating_add(i64::from(to));
            break;
        }
        start = start.saturating_add(negated);
    }
    clamp(start.saturating_add(last_negated))
}

pub(crate) fn clamp(value: i64) -> i32 {
    base::numerics::saturated_cast(value)
}

/// `value` in counter style `style` (css-counter-styles 3, the predefined
/// styles): `decimal` where the style's range doesn't reach (and for a
/// style machopo doesn't have).
pub(crate) fn represent(value: i32, style: &ListStyleType) -> String {
    const LOWER_LATIN: &str = "abcdefghijklmnopqrstuvwxyz";
    const UPPER_LATIN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    // Without the final sigma.
    const LOWER_GREEK: &str = "αβγδεζηθικλμνξοπρστυφχψω";
    match style {
        ListStyleType::Disc => "•".into(),
        ListStyleType::Circle => "◦".into(),
        ListStyleType::Square => "▪".into(),
        ListStyleType::DisclosureOpen => "▾".into(),
        ListStyleType::DisclosureClosed => "▸".into(),
        ListStyleType::None | ListStyleType::String(_) => String::new(),
        ListStyleType::DecimalLeadingZero if (-9..=9).contains(&value) => {
            let sign = if value < 0 { "-" } else { "" };
            format!("{sign}0{}", value.unsigned_abs())
        }
        ListStyleType::LowerRoman | ListStyleType::UpperRoman if (1..=3999).contains(&value) => {
            let roman = roman(value);
            if *style == ListStyleType::LowerRoman {
                roman.to_ascii_lowercase()
            } else {
                roman
            }
        }
        ListStyleType::LowerAlpha | ListStyleType::LowerLatin if value >= 1 => {
            alphabetic(value, LOWER_LATIN)
        }
        ListStyleType::UpperAlpha | ListStyleType::UpperLatin if value >= 1 => {
            alphabetic(value, UPPER_LATIN)
        }
        ListStyleType::LowerGreek if value >= 1 => alphabetic(value, LOWER_GREEK),
        _ => value.to_string(),
    }
}

/// The text of a list item's marker with `content: normal` whose
/// `list-item` counter is `value` (`marker_pieces`); `None` for none.
pub(crate) fn marker_text(style: &ListStyleType, value: i32) -> Option<String> {
    match style {
        ListStyleType::None => None,
        ListStyleType::String(string) => Some(string.to_string()),
        style => Some(format!("{}{}", represent(value, style), suffix(style))),
    }
}

/// What follows a marker's counter: a space after a symbol, `. ` after a
/// number.
fn suffix(style: &ListStyleType) -> &'static str {
    match style {
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed => " ",
        ListStyleType::None | ListStyleType::String(_) => "",
        _ => ". ",
    }
}

/// An alphabetic counter style's representation of `value` (at least 1):
/// `a` … `z`, `aa`, `ab` ….
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value >= 1 (the callers fall back to decimal otherwise), and goes down by one \
              only while above 0; the symbols are a few dozen, at least one"
)]
fn alphabetic(value: i32, symbols: &str) -> String {
    let symbols: Vec<char> = symbols.chars().collect();
    let n = symbols.len() as u32;
    let mut value = value as u32;
    let mut out = Vec::new();
    while value > 0 {
        value -= 1;
        out.push(symbols[(value % n) as usize]);
        value /= n;
    }
    out.iter().rev().collect()
}

/// `upper-roman`'s representation of `value`, 1 to 3999.
fn roman(mut value: i32) -> String {
    const ROMAN: [(i32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (n, symbol) in ROMAN {
        while value >= n {
            out.push_str(symbol);
            #[expect(clippy::arithmetic_side_effects, reason = "value >= n > 0")]
            {
                value -= n;
            }
        }
    }
    out
}

/// What of `element`'s style the counters depend on, if it takes part in
/// them (`is_relevant`); with `lists`, those but `list-item`.
pub(crate) fn directives<'gc>(
    dom: Reader<'_, 'gc>,
    element: NodeRef<'gc>,
    lists: bool,
) -> Option<Directives> {
    let style = dom.style(element)?;
    directives_of(&style, |pseudo| dom.pseudo_style(element, pseudo), lists)
}

/// Whether `element` takes part in the counters (`counters::relevant_of`).
pub(crate) fn is_relevant<'gc>(dom: Reader<'_, 'gc>, element: NodeRef<'gc>, lists: bool) -> bool {
    dom.style(element)
        .is_some_and(|style| relevant_of(&style, |pseudo| dom.pseudo_style(element, pseudo), lists))
}

/// Works out the counters of the document whose root element is `root`,
/// in the flat tree's order; with `lists`, all but `list-item`.
pub(crate) fn compute<'gc>(
    dom: Reader<'_, 'gc>,
    root: Option<NodeRef<'gc>>,
    lists: bool,
) -> Found<'gc> {
    let mut pass = Pass::new(lists);
    if let Some(root) = root {
        let document = dom.dom.parent(root).unwrap_or(root);
        pass.element(dom, root, dom.id(document), &Set::new(), &Set::new());
    }
    pass.found()
}

impl<'gc> Pass<'gc> {
    /// `element`, a child of `parent` whose set is `parent_set`, after a
    /// sibling whose set is `sibling_set`, and what is in it, if it
    /// generates boxes: its set, for its next sibling, or `None` if it has
    /// no boxes.
    fn element(
        &mut self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
        parent: NodeId,
        parent_set: &Set,
        sibling_set: &Set,
    ) -> Option<Set> {
        let style = dom.style(element)?;
        if !dom.has_boxes(element, &style) {
            return None;
        }
        let id = dom.id(element);
        if let Some(directives) = directives(dom, element, self.lists) {
            self.relevant.insert(id, directives);
        }
        if self.lists
            && !crate::list_items::regular_of(&style, |pseudo| dom.pseudo_style(element, pseudo))
        {
            self.irregular = true;
        }
        // No box: it sets, resets and increments no counter, and its
        // pseudo-elements and children are counted as its parent's
        // children, in its place.
        if style.display == Display::Contents {
            let mut sibling = sibling_set.clone();
            let before_set = self.inherit(parent_set, &sibling);
            if let Some(before) = self.pseudo(dom, element, PseudoId::Before, &before_set) {
                sibling = before;
            }
            for child in dom.children(element) {
                if dom.is_element(child)
                    && let Some(child_set) = self.element(dom, child, parent, parent_set, &sibling)
                {
                    sibling = child_set;
                }
            }
            let after_set = self.inherit(parent_set, &sibling);
            if let Some(after) = self.pseudo(dom, element, PseudoId::After, &after_set) {
                sibling = after;
            }
            return Some(sibling);
        }
        let list_item = style.display == Display::ListItem;
        let mut set = self.inherit(parent_set, sibling_set);
        self.change(&mut set, &style, parent, list_item);
        // Style containment: what its subtree changes is its own.
        let contained = style.rare.contain.contains(Contain::STYLE);
        let outer_scope = self.contained_from;
        if contained {
            self.contained_from = self.instances.len();
        }
        if crate::tree_builder::is_replaced(dom.dom, element) {
            return Some(set);
        }
        if list_item && let Some(marker) = dom.pseudo_style(element, PseudoId::Marker) {
            let pieces = match &marker.rare.content {
                Content::None => None,
                Content::Items(items) => {
                    let mut marker_set = set.clone();
                    Some(self.items(&mut marker_set, id, items))
                }
                // The lists number their own (`list_items`).
                Content::Normal if self.lists => None,
                Content::Normal => {
                    let list_item = self
                        .innermost(&set, "list-item")
                        .map(|instance| (instance, self.instances[instance].value));
                    marker_pieces(&marker.rare_inherited.list_style_type, list_item)
                }
            };
            if let Some(pieces) = pieces {
                self.pending.push((element, PseudoId::Marker, pieces));
            }
        }
        let mut sibling = Set::new();
        if let Some(before) = self.pseudo(dom, element, PseudoId::Before, &set) {
            sibling = before;
        }
        for child in dom.children(element) {
            if dom.is_element(child)
                && let Some(child_set) = self.element(dom, child, id, &set, &sibling)
            {
                sibling = child_set;
            }
        }
        let after_set = self.inherit(&set, &sibling);
        self.pseudo(dom, element, PseudoId::After, &after_set);
        self.contained_from = outer_scope;
        Some(set)
    }

    /// `element`'s `pseudo`, whose set starts as `set`, if it generates a
    /// box: its set.
    fn pseudo(
        &mut self,
        dom: Reader<'_, 'gc>,
        element: NodeRef<'gc>,
        pseudo: PseudoId,
        set: &Set,
    ) -> Option<Set> {
        let style = dom.pseudo_style(element, pseudo)?;
        let Content::Items(items) = &style.rare.content else {
            return None;
        };
        if style.display.generates_no_box() {
            return None;
        }
        let id = dom.id(element);
        let mut set = set.clone();
        self.change(&mut set, &style, id, false);
        if shows_counters(&style) {
            let pieces = self.items(&mut set, id, items);
            self.pending.push((element, pseudo, pieces));
        }
        Some(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_styles() {
        let r = |value, style: ListStyleType| represent(value, &style);
        assert_eq!(r(3, ListStyleType::Decimal), "3");
        assert_eq!(r(-3, ListStyleType::Decimal), "-3");
        assert_eq!(r(7, ListStyleType::DecimalLeadingZero), "07");
        assert_eq!(r(-7, ListStyleType::DecimalLeadingZero), "-07");
        assert_eq!(r(12, ListStyleType::DecimalLeadingZero), "12");
        assert_eq!(r(1994, ListStyleType::UpperRoman), "MCMXCIV");
        assert_eq!(r(4, ListStyleType::LowerRoman), "iv");
        assert_eq!(r(4000, ListStyleType::LowerRoman), "4000");
        assert_eq!(r(0, ListStyleType::LowerRoman), "0");
        assert_eq!(r(1, ListStyleType::LowerAlpha), "a");
        assert_eq!(r(26, ListStyleType::UpperLatin), "Z");
        assert_eq!(r(27, ListStyleType::LowerLatin), "aa");
        assert_eq!(r(0, ListStyleType::LowerAlpha), "0");
        assert_eq!(r(24, ListStyleType::LowerGreek), "ω");
        assert_eq!(r(25, ListStyleType::LowerGreek), "αα");
        assert_eq!(r(5, ListStyleType::Disc), "•");
        assert_eq!(r(5, ListStyleType::None), "");
        assert_eq!(r(5, ListStyleType::Named("armenian".into())), "5");
        assert_eq!(suffix(&ListStyleType::Square), " ");
        assert_eq!(suffix(&ListStyleType::UpperRoman), ". ");
    }
}
