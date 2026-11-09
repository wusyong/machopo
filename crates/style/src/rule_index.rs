//! A rule set's selectors by bucket, keyed by numbers (Blink's `RuleSet`,
//! whose buckets are by `AtomicString`): a tag's and a class's name's
//! number in the thread's names, an id's string in the thread's values
//! table by its address (ids aren't names: the names table keeps every
//! name it has until the thread ends, and a list whose items each have an
//! id would grow it for good). Made on the thread that matches with it,
//! which numbers each selector's names as it is made (`css::IdentAtom`),
//! so that matching compares numbers.

use css::rule_set::{RuleData, RuleSet};
use css::selector::{CssIdent, Impl};
use dom::{DomString, NameId, names};
use rustc_hash::FxHashMap;
use selectors::attr::NamespaceConstraint;
use selectors::matching::QuirksMode as SelectorsQuirksMode;
use selectors::parser::{AncestorHashes, Combinator, Component, Selector};

/// A rule set (`css::RuleSet`: its rules and the media queries it was
/// made with), and its selectors by bucket.
pub(crate) struct RuleIndex {
    pub(crate) set: RuleSet,
    /// By the address of the id's string in the values table, which
    /// `id_strings` holds (no other string has the address while it
    /// lives).
    ids: FxHashMap<usize, Vec<RuleData>>,
    id_strings: Vec<DomString>,
    /// By the class's name ASCII lowercase (a quirks mode document matches
    /// classes ASCII case-insensitively; matching then checks the case).
    classes: FxHashMap<NameId, Vec<RuleData>>,
    /// By the tag's name as written and ASCII lowercase: an HTML element
    /// in an HTML document matches the lowercase one, any other the name
    /// as written (`clipPath`), as `selectors` does.
    tags: FxHashMap<NameId, Vec<RuleData>>,
    /// Whatever has none of those in its rightmost compound.
    other: Vec<RuleData>,
    /// The `:host` selectors, which only the host of the shadow tree whose
    /// sheets these are matches (Blink's `RuleSet::shadow_host_rules_`),
    /// and the `::slotted()` ones, which only what is assigned to its
    /// slots does (`slotted_pseudo_element_rules_`).
    pub(crate) host: Vec<RuleData>,
    pub(crate) slotted: Vec<RuleData>,
    /// The rules of a UA shadow tree's parts (`::placeholder`,
    /// `::-internal-inner-editor`: a shadow pseudo id's pseudo-element),
    /// matched against an element of a UA shadow tree as its host's
    /// pseudo-element.
    pub(crate) parts: Vec<RuleData>,
    pub(crate) features: Features,
    /// Its invalidation sets.
    pub(crate) sets: crate::invalidation::FeatureSets,
    /// The names its selectors use of an element, anywhere in them (what
    /// the DOM notes changes of: `dom::StyleNames`).
    pub(crate) names: dom::StyleNames,
}

/// What the selectors of an index ask of elements other than the one they
/// match (Blink's `RuleFeatureSet`, coarser: no invalidation sets): the
/// names of the compounds left of a combinator (and in `:is()` and the
/// like), so that a change of one on an element styles its subtree again,
/// and a change of another only itself; and whether siblings' positions
/// count.
#[derive(Default)]
pub(crate) struct Features {
    /// Classes, by their names ASCII lowercase.
    classes: rustc_hash::FxHashSet<NameId>,
    /// Ids, ASCII lowercase.
    ids: rustc_hash::FxHashSet<String>,
    /// Attributes' local names, as written and ASCII lowercase.
    attributes: rustc_hash::FxHashSet<NameId>,
    /// The user action states (`dom::AttributeChanges::HOVER`,
    /// `ACTIVE`) a selector asks of another element.
    states: u16,
    /// `:has()`: what is below an element decides what it matches; a
    /// change anywhere styles the document again.
    pub(crate) has: bool,
    /// `+` or `~`: a sibling's change may change what its later siblings
    /// match.
    pub(crate) sibling: bool,
    /// `~`: every later sibling's, not only the next.
    pub(crate) later_siblings: bool,
    /// A sibling combinator left of a descendant or child one (`.a + .b
    /// .c`): what matches below the later siblings too.
    pub(crate) sibling_then_descendant: bool,
    /// `:window-inactive`: the page taking or losing the focus styles the
    /// document again.
    pub(crate) window_inactive: bool,
    /// How many siblings after a change among them the selectors see it
    /// from: the longest run of `+` (`reach`), or any number
    /// (`reach_any`: `~`, a position, `+` left of one).
    pub(crate) reach: u8,
    pub(crate) reach_any: bool,
}

impl Features {
    /// Takes on `other`'s flags (what it asks of siblings and of what is
    /// below an element), not its names.
    pub(crate) fn add_flags(&mut self, other: &Self) {
        self.has |= other.has;
        self.sibling |= other.sibling;
        self.later_siblings |= other.later_siblings;
        self.sibling_then_descendant |= other.sibling_then_descendant;
        self.window_inactive |= other.window_inactive;
        self.reach = self.reach.max(other.reach);
        self.reach_any |= other.reach_any;
    }

    /// How far after a change among siblings the selectors see it (`None`:
    /// any number).
    pub(crate) fn sibling_reach(&self) -> Option<u8> {
        (!self.reach_any).then_some(self.reach)
    }

    fn add_name(set: &mut rustc_hash::FxHashSet<NameId>, name: &str) {
        set.insert(names::intern(name));
        set.insert(names::intern(&name.to_ascii_lowercase()));
    }

    /// The names of `selector` past its rightmost compound (`outer`: all
    /// of them, a selector inside another's pseudo-class), and its
    /// combinators.
    fn add(&mut self, selector: &Selector<Impl>, outer: bool) {
        let mut left = outer;
        let mut descended = false;
        for component in selector.iter_raw_match_order() {
            match component {
                Component::Combinator(combinator) => {
                    match combinator {
                        Combinator::NextSibling | Combinator::LaterSibling => {
                            self.sibling = true;
                            self.later_siblings |= *combinator == Combinator::LaterSibling;
                            self.sibling_then_descendant |= descended;
                        }
                        Combinator::Descendant | Combinator::Child => descended = true,
                        _ => {}
                    }
                    left |= !matches!(combinator, Combinator::PseudoElement);
                }
                Component::Class(name) if left => Self::add_name(&mut self.classes, &name.0),
                Component::ID(name) if left => {
                    self.ids.insert(name.0.to_ascii_lowercase());
                }
                Component::AttributeInNoNamespaceExists { local_name, .. }
                | Component::AttributeInNoNamespace { local_name, .. }
                    if left =>
                {
                    Self::add_name(&mut self.attributes, &local_name.0);
                }
                Component::AttributeOther(attribute) if left => {
                    Self::add_name(&mut self.attributes, &attribute.local_name.0);
                }
                Component::NonTSPseudoClass(css::selector::PseudoClass::WindowInactive) => {
                    self.window_inactive = true;
                }
                Component::NonTSPseudoClass(class @ css::selector::PseudoClass::Lang(_)) => {
                    // An ancestor's `lang` decides it.
                    self.attributes
                        .extend(crate::invalidation::pseudo_class_attributes(class));
                }
                Component::NonTSPseudoClass(class) if left => {
                    self.states |= crate::invalidation::pseudo_class_state(class);
                    self.attributes
                        .extend(crate::invalidation::pseudo_class_attributes(class));
                }
                Component::Negation(list) | Component::Is(list) | Component::Where(list) => {
                    for selector in list.slice() {
                        self.add(selector, true);
                    }
                }
                Component::NthOf(data) => {
                    for selector in data.selectors() {
                        self.add(selector, true);
                    }
                }
                Component::Has(_) => self.has = true,
                _ => {}
            }
        }
    }

    /// Whether `changes` touch a name these selectors ask of other
    /// elements: what styled the changed element's subtree again before
    /// the invalidation sets (the debug build's, to check them).
    #[cfg(debug_assertions)]
    pub(crate) fn touched_by(&self, changes: &dom::AttributeChanges) -> bool {
        self.has
            || changes.states & self.states != 0
            || changes
                .attributes
                .iter()
                .any(|name| self.attributes.contains(name))
            || changes.classes.iter().any(|class| {
                names::find(&class.to_ascii_lowercase())
                    .is_some_and(|name| self.classes.contains(&name))
            })
            || changes
                .ids
                .iter()
                .any(|id| self.ids.contains(&id.to_ascii_lowercase()))
    }
}

/// Adds the names `selector` uses of an element, anywhere in it (in a
/// nested list too): its classes, ids, attributes and states, the
/// attributes the pseudo-classes it has read; `:has()`, every one.
fn add_names(names: &mut dom::StyleNames, selector: &Selector<Impl>) {
    let both = |name: &str| {
        [
            names::intern(name),
            names::intern(&name.to_ascii_lowercase()),
        ]
    };
    for component in selector.iter_raw_match_order() {
        match component {
            Component::Class(class) => {
                names
                    .classes
                    .insert(names::intern(&class.0.to_ascii_lowercase()));
            }
            Component::ID(id) => {
                names.ids.insert(id.0.to_ascii_lowercase().into());
            }
            Component::AttributeInNoNamespaceExists { local_name, .. }
            | Component::AttributeInNoNamespace { local_name, .. } => {
                names.attributes.extend(both(&local_name.0));
            }
            Component::AttributeOther(attribute) => {
                names.attributes.extend(both(&attribute.local_name.0));
            }
            Component::NonTSPseudoClass(class) => {
                names.states |= crate::invalidation::pseudo_class_state(class);
                names
                    .attributes
                    .extend(crate::invalidation::pseudo_class_attributes(class));
            }
            Component::Negation(list) | Component::Is(list) | Component::Where(list) => {
                for selector in list.slice() {
                    add_names(names, selector);
                }
            }
            Component::NthOf(data) => {
                for selector in data.selectors() {
                    add_names(names, selector);
                }
            }
            Component::Host(Some(inner)) | Component::Slotted(inner) => add_names(names, inner),
            Component::Has(_) => names.everything = true,
            _ => {}
        }
    }
}

impl RuleIndex {
    /// `set`'s selectors by bucket, their names numbered on this thread
    /// (`serial`'s). A `:host` selector and a `::slotted()` one are in
    /// lists of their own, a UA shadow tree's pseudo-element's in none
    /// (the DOM has no UA shadow trees).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a set's rules and a rule's selectors are parsed from text in memory: fewer \
                  than 2^32 of either, as `RuleData` numbers them"
    )]
    pub(crate) fn new(set: RuleSet, serial: u32) -> Self {
        let mut index = Self {
            set,
            ids: FxHashMap::default(),
            id_strings: Vec::new(),
            classes: FxHashMap::default(),
            tags: FxHashMap::default(),
            other: Vec::new(),
            host: Vec::new(),
            slotted: Vec::new(),
            parts: Vec::new(),
            features: Features::default(),
            sets: crate::invalidation::FeatureSets::default(),
            names: dom::StyleNames::default(),
        };
        let mut found = Vec::new();
        for (rule, active) in index.set.rules().iter().enumerate() {
            for (at, selector) in active.rule.selectors.slice().iter().enumerate() {
                number_names(selector, serial);
                index.features.add(selector, false);
                match crate::invalidation::SiblingReach::of(selector).0 {
                    Some(run) => index.features.reach = index.features.reach.max(run),
                    None => index.features.reach_any = true,
                }
                index.sets.add_selector(selector);
                add_names(&mut index.names, selector);
                let data = RuleData {
                    rule: rule as u32,
                    selector: at as u32,
                    specificity: selector.specificity(),
                    // `name_hash` is ASCII case-insensitive: they hold in
                    // a quirks mode document too.
                    ancestor_hashes: AncestorHashes::new(selector, SelectorsQuirksMode::NoQuirks)
                        .packed_hashes,
                };
                found.push((bucket(selector), data));
            }
        }
        for (bucket, data) in found {
            match bucket {
                Bucket::Host => index.host.push(data),
                Bucket::Slotted => index.slotted.push(data),
                Bucket::ShadowPart => index.parts.push(data),
                Bucket::Id(id) => {
                    let string = dom::intern_value(&id);
                    index.ids.entry(string.addr()).or_default().push(data);
                    if !index
                        .id_strings
                        .iter()
                        .any(|s| DomString::ptr_eq(s, &string))
                    {
                        index.id_strings.push(string);
                    }
                }
                Bucket::Class(class) => {
                    let lower = names::intern(&class.to_ascii_lowercase());
                    index.classes.entry(lower).or_default().push(data);
                }
                Bucket::Tag(name, lower) => {
                    let (name, lower) = (names::intern(&name), names::intern(&lower));
                    index.tags.entry(name).or_default().push(data);
                    if lower != name {
                        index.tags.entry(lower).or_default().push(data);
                    }
                }
                Bucket::Other => index.other.push(data),
            }
        }
        index
    }

    /// Adds to `out` the selectors an element with this id (its string's
    /// address in the values table), these classes (their names ASCII
    /// lowercase) and this local name may match, in no particular order
    /// and maybe more than once. In a quirks mode document every id
    /// selector is a candidate: its ids match ASCII case-insensitively.
    pub(crate) fn collect(
        &self,
        id: Option<usize>,
        quirks: bool,
        classes: impl IntoIterator<Item = NameId>,
        local: NameId,
        out: &mut Vec<RuleData>,
    ) {
        if quirks {
            for rules in self.ids.values() {
                out.extend_from_slice(rules);
            }
        } else if let Some(rules) = id.and_then(|id| self.ids.get(&id)) {
            out.extend_from_slice(rules);
        }
        for class in classes {
            if let Some(rules) = self.classes.get(&class) {
                out.extend_from_slice(rules);
            }
        }
        if let Some(rules) = self.tags.get(&local) {
            out.extend_from_slice(rules);
        }
        out.extend_from_slice(&self.other);
    }
}

/// The bucket fewest elements look in: the rightmost compound's id, else
/// its class, else its name (Blink's `RuleSet::FindBestRuleSetAndAdd`).
enum Bucket {
    Host,
    Slotted,
    /// A UA shadow tree's element's (`::placeholder`): the DOM has
    /// none.
    ShadowPart,
    Id(String),
    Class(String),
    Tag(String, String),
    Other,
}

fn bucket(selector: &Selector<Impl>) -> Bucket {
    let mut id = None;
    let mut class = None;
    let mut tag = None;
    let (mut host, mut slotted, mut part) = (false, false, false);
    // The rightmost compound only: `iter` stops at its combinator. A
    // pseudo-element is a compound of its own, after its element's: that
    // one is the element's (`.a::before` is in `a`'s bucket, as in Blink).
    let mut iter = selector.iter();
    loop {
        for component in &mut iter {
            match component {
                Component::ID(name) => id = Some(&name.0),
                Component::Class(name) => class = class.or(Some(&name.0)),
                Component::LocalName(name) => tag = Some((&name.name.0, &name.lower_name.0)),
                Component::Host(_) => host = true,
                Component::Slotted(_) => slotted = true,
                Component::PseudoElement(pseudo) => {
                    part |= pseudo.shadow_pseudo_id().is_some();
                }
                _ => {}
            }
        }
        if iter.next_sequence() != Some(Combinator::PseudoElement) {
            break;
        }
    }
    if slotted {
        Bucket::Slotted
    } else if host {
        Bucket::Host
    } else if part {
        Bucket::ShadowPart
    } else if let Some(id) = id {
        Bucket::Id(id.clone())
    } else if let Some(class) = class {
        Bucket::Class(class.clone())
    } else if let Some((name, lower)) = tag {
        Bucket::Tag(name.clone(), lower.clone())
    } else {
        Bucket::Other
    }
}

/// Numbers each name `selector` compares (tags, classes, namespaces,
/// attributes' local names, inside `:is()` and the like too) on the thread
/// of serial `serial`: matching then compares numbers.
pub(crate) fn number_names(selector: &Selector<Impl>, serial: u32) {
    let number = |ident: &CssIdent| {
        if ident.1.get(serial).is_none() {
            ident.1.set(serial, names::intern(&ident.0));
        }
    };
    for component in selector.iter_raw_match_order() {
        match component {
            Component::LocalName(name) => {
                number(&name.name);
                number(&name.lower_name);
            }
            Component::Class(name) => number(name),
            Component::DefaultNamespace(url) | Component::Namespace(_, url) => number(url),
            Component::AttributeInNoNamespaceExists {
                local_name,
                local_name_lower,
            } => {
                number(local_name);
                number(local_name_lower);
            }
            Component::AttributeInNoNamespace { local_name, .. } => number(local_name),
            Component::AttributeOther(attribute) => {
                number(&attribute.local_name);
                number(&attribute.local_name_lower);
                if let Some(NamespaceConstraint::Specific((_, url))) = &attribute.namespace {
                    number(url);
                }
            }
            Component::Negation(list) | Component::Is(list) | Component::Where(list) => {
                for selector in list.slice() {
                    number_names(selector, serial);
                }
            }
            Component::NthOf(data) => {
                for selector in data.selectors() {
                    number_names(selector, serial);
                }
            }
            Component::Has(relatives) => {
                for relative in relatives {
                    number_names(&relative.selector, serial);
                }
            }
            Component::Slotted(selector) | Component::Host(Some(selector)) => {
                number_names(selector, serial);
            }
            _ => {}
        }
    }
}
