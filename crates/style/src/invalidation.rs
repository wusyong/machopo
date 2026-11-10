//! Invalidation sets (Blink's `RuleFeatureSet`, as `style`'s, which this
//! follows): what a change of an element's class, id, attribute or state
//! styles again of other elements, by the rules of an index. For each
//! feature an ancestor's compound uses, the features of the descendants to
//! style again (a descendant set); for each one a compound left of a
//! sibling combinator uses, which later siblings, how far, and what below
//! them (a sibling set). They are applied in the style walk itself, not in
//! a pass of their own, and not kept on elements: the element whose
//! attributes changed looks its changes up as the walk reaches it, and
//! hands the sets down to its children and on to its later siblings
//! (`style`'s choice: the DOM doesn't know the rules).
//!
//! What no set says (a selector of a shadow tree's elements, `:nth-child(…
//! of S)`, siblings nested in `:is()`) falls back to what `style` did
//! before: the whole subtree, or every later sibling.

use css::selector::{Impl, PseudoClass, PseudoElement};
use dom::{AttributeChanges, Cx, NameId, NodeRef, names};
use rustc_hash::{FxHashMap, FxHashSet};
use selectors::parser::{Combinator, Component, Selector};

/// The state bits of `dom::AttributeChanges` a pseudo-class is of.
pub(crate) fn pseudo_class_state(class: &PseudoClass) -> u16 {
    match class {
        PseudoClass::Hover => AttributeChanges::HOVER,
        PseudoClass::Active => AttributeChanges::ACTIVE,
        PseudoClass::Focus => AttributeChanges::FOCUS,
        PseudoClass::FocusWithin => AttributeChanges::FOCUS_WITHIN,
        PseudoClass::FocusVisible => AttributeChanges::FOCUS_VISIBLE,
        PseudoClass::Defined => AttributeChanges::DEFINED,
        PseudoClass::Checked => AttributeChanges::CHECKED,
        PseudoClass::Indeterminate => AttributeChanges::INDETERMINATE,
        PseudoClass::Disabled
        | PseudoClass::Enabled
        | PseudoClass::ReadOnly
        | PseudoClass::ReadWrite => AttributeChanges::DISABLED,
        PseudoClass::PlaceholderShown => AttributeChanges::VALUE,
        PseudoClass::Default => AttributeChanges::DEFAULT,
        // A select's popup.
        PseudoClass::Open => AttributeChanges::OPEN,
        PseudoClass::InternalMultiSelectFocus => AttributeChanges::MULTI_SELECT_FOCUS,
        PseudoClass::Target => AttributeChanges::TARGET,
        // DevTools forcing them (the DOM's changes are of attributes).
        PseudoClass::Required | PseudoClass::Optional => AttributeChanges::REQUIRED,
        _ => 0,
    }
}

/// The attributes a pseudo-class reads of the element it matches, whose
/// changes are its changes (with its state's, `pseudo_class_state`): a
/// link's `href`, `:open`'s, a language's `lang`; the form ones' (an
/// input's `type` decides which apply).
pub(crate) fn pseudo_class_attributes(class: &PseudoClass) -> &'static [NameId] {
    use names::local::{
        CHECKED, DISABLED, HREF, LANG, MULTIPLE, NAME, OPEN, PLACEHOLDER, READONLY, REQUIRED,
        SELECTED, SIZE, TYPE, VALUE,
    };
    match class {
        PseudoClass::Link | PseudoClass::AnyLink | PseudoClass::Visited => &[HREF],
        PseudoClass::Open => &[OPEN],
        PseudoClass::InternalListBox => &[MULTIPLE, SIZE],
        PseudoClass::Lang(_) => &[LANG],
        PseudoClass::Checked => &[TYPE],
        // A progress's `value`; a radio button's group by its name.
        PseudoClass::Indeterminate => &[TYPE, VALUE, NAME],
        PseudoClass::Disabled | PseudoClass::Enabled => &[DISABLED],
        PseudoClass::Required | PseudoClass::Optional => &[REQUIRED, TYPE],
        // An editable element's `contenteditable` restyles its subtree
        // whatever the rules (`for_descendants`).
        PseudoClass::ReadOnly | PseudoClass::ReadWrite => &[READONLY, DISABLED, TYPE],
        PseudoClass::PlaceholderShown => &[PLACEHOLDER, VALUE, TYPE],
        // An option's `selected`.
        PseudoClass::Default => &[CHECKED, SELECTED, TYPE],
        _ => &[],
    }
}

/// A name as written and ASCII lowercase, interned: an attribute's or an
/// element's name is compared by its number, whichever way it is written.
fn both(name: &str) -> [NameId; 2] {
    [
        names::intern(name),
        names::intern(&name.to_ascii_lowercase()),
    ]
}

fn lower(name: &str) -> NameId {
    names::intern(&name.to_ascii_lowercase())
}

/// Attributes that what matches below their element depends on, whatever
/// the rules: `:lang()` and `:dir()` look up the ancestors, and
/// `:read-write` takes editability from them.
fn for_descendants(name: NameId) -> bool {
    name == names::local::LANG || name == names::local::DIR || name == names::local::CONTENTEDITABLE
}

/// Whether `component` is one `pick` picks, or nests one in a list.
pub(crate) fn nests(component: &Component<Impl>, pick: fn(&Component<Impl>) -> bool) -> bool {
    let any = |selector: &Selector<Impl>| {
        selector
            .iter_raw_match_order()
            .any(|component| nests(component, pick))
    };
    pick(component)
        || match component {
            Component::Is(list) | Component::Where(list) | Component::Negation(list) => {
                list.slice().iter().any(any)
            }
            Component::NthOf(data) => data.selectors().iter().any(any),
            Component::Has(list) => list.iter().any(|relative| any(&relative.selector)),
            Component::Host(Some(inner)) | Component::Slotted(inner) => any(inner),
            _ => false,
        }
}

/// How many siblings after a change among them selectors can see it from:
/// `Some(n)`, a run of `n` `+` at most; `None`, any number (`~`, or a
/// position counted from the first, or any position left of a `+`). What
/// tells whether a list nests siblings (a change of position itself is the
/// parent's selector flags' and `~`'s, in the walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SiblingReach(pub(crate) Option<u8>);

impl Default for SiblingReach {
    fn default() -> Self {
        Self(Some(0))
    }
}

impl SiblingReach {
    /// `selector`'s: siblings or a position nested in a list are taken as
    /// any number.
    pub(crate) fn of(selector: &Selector<Impl>) -> Self {
        fn nested_siblings<'s>(mut selectors: impl Iterator<Item = &'s Selector<Impl>>) -> bool {
            selectors.any(|selector| SiblingReach::of(selector) != SiblingReach::default())
        }
        let (mut longest, mut run) = (0u8, 0u8);
        for component in selector.iter_raw_match_order() {
            let nested = match component {
                Component::Combinator(Combinator::NextSibling) => {
                    run = run.saturating_add(1);
                    longest = longest.max(run);
                    false
                }
                Component::Combinator(Combinator::LaterSibling) | Component::NthOf(_) => true,
                Component::Combinator(_) => {
                    run = 0;
                    false
                }
                Component::Nth(data) => run > 0 || !data.is_simple_edge(),
                Component::Is(list) | Component::Where(list) | Component::Negation(list) => {
                    nested_siblings(list.slice().iter())
                        || (run > 0
                            && nests(component, |c| {
                                matches!(c, Component::Nth(_) | Component::NthOf(_))
                            }))
                }
                Component::Has(list) => nested_siblings(list.iter().map(|r| &r.selector)),
                Component::Host(Some(inner)) | Component::Slotted(inner) => {
                    nested_siblings(std::iter::once(inner))
                }
                _ => false,
            };
            if nested {
                return Self(None);
            }
        }
        Self(Some(longest))
    }
}

/// What an invalidation set looks for in an element (Blink's
/// `InvalidationSet`'s features): one of these ids (ASCII lowercase),
/// classes (by their names ASCII lowercase), attribute names or local
/// names (each as written and ASCII lowercase), or (`any`) none in
/// particular. One feature of a compound is enough to find what it
/// matches; the one taken is the rarest, as Blink's: an id, then a class,
/// an attribute, a local name.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct InvalidationFeatures {
    pub(crate) any: bool,
    pub(crate) ids: Vec<String>,
    pub(crate) classes: Vec<NameId>,
    pub(crate) attributes: Vec<NameId>,
    pub(crate) names: Vec<NameId>,
}

impl InvalidationFeatures {
    /// What finds what `compound` (its components, any order) matches.
    pub(crate) fn of(compound: &[&Component<Impl>]) -> Self {
        let mut features = Self::default();
        let id = compound.iter().find_map(|component| match component {
            Component::ID(id) => Some(id.0.to_ascii_lowercase()),
            _ => None,
        });
        let class = || {
            compound.iter().find_map(|component| match component {
                Component::Class(class) => Some(lower(&class.0)),
                _ => None,
            })
        };
        let attribute = || {
            compound.iter().find_map(|component| match component {
                Component::AttributeInNoNamespaceExists { local_name, .. }
                | Component::AttributeInNoNamespace { local_name, .. } => Some(both(&local_name.0)),
                Component::AttributeOther(attribute) => Some(both(&attribute.local_name.0)),
                _ => None,
            })
        };
        let name = || {
            compound.iter().find_map(|component| match component {
                Component::LocalName(name) => Some([
                    names::intern(&name.name.0),
                    names::intern(&name.lower_name.0),
                ]),
                _ => None,
            })
        };
        if let Some(id) = id {
            features.ids.push(id);
        } else if let Some(class) = class() {
            features.classes.push(class);
        } else if let Some(attribute) = attribute() {
            features.attributes.extend(attribute);
        } else if let Some(name) = name() {
            features.names.extend(name);
        } else {
            features.any = true;
        }
        features
    }

    /// The features of `compound`, or of each of an `:is()`'s or a
    /// `:where()`'s selectors' subjects in it (Blink's features from a
    /// nested list); `None` if it has none to find it by.
    pub(crate) fn of_compound(compound: &[&Component<Impl>]) -> Option<Self> {
        let features = Self::of(compound);
        if !features.any {
            return Some(features);
        }
        compound.iter().find_map(|component| {
            let (Component::Is(list) | Component::Where(list)) = component else {
                return None;
            };
            let mut all = Self::default();
            for selector in list.slice() {
                let subject: Vec<&Component<Impl>> = selector
                    .iter_raw_match_order()
                    .take_while(|component| {
                        !matches!(component, Component::Combinator(combinator) if *combinator != Combinator::PseudoElement)
                    })
                    .filter(|component| !matches!(component, Component::Combinator(_)))
                    .collect();
                all.add(&Self::of_compound(&subject)?);
            }
            Some(all)
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.any
            && self.ids.is_empty()
            && self.classes.is_empty()
            && self.attributes.is_empty()
            && self.names.is_empty()
    }

    pub(crate) fn add(&mut self, other: &Self) {
        self.any |= other.any;
        for id in &other.ids {
            if !self.ids.contains(id) {
                self.ids.push(id.clone());
            }
        }
        for (mine, theirs) in [
            (&mut self.classes, &other.classes),
            (&mut self.attributes, &other.attributes),
            (&mut self.names, &other.names),
        ] {
            for feature in theirs {
                if !mine.contains(feature) {
                    mine.push(*feature);
                }
            }
        }
    }

    /// Whether `element` has one of them.
    pub(crate) fn matches<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
        if self.any {
            return true;
        }
        if !self.ids.is_empty()
            && cx
                .attribute_value_by_id(element, names::ID)
                .is_some_and(|id| {
                    self.ids
                        .iter()
                        .any(|f| id.borrow().to_str().eq_ignore_ascii_case(f))
                })
        {
            return true;
        }
        if !self.classes.is_empty()
            && cx
                .class_names(element)
                .iter()
                .any(|class| self.classes.contains(&class.lower))
        {
            return true;
        }
        if !self.attributes.is_empty()
            && (0..cx.attribute_count(element)).any(|index| {
                cx.attribute_name_at(element, index)
                    .is_some_and(|name| self.attributes.contains(&name.local()))
            })
        {
            return true;
        }
        !self.names.is_empty()
            && cx
                .element_name(element)
                .is_some_and(|name| self.names.contains(&name.local))
    }
}

/// The classes (ASCII lowercase), ids (ASCII lowercase), attribute names
/// (as written and ASCII lowercase) and states some selector uses of an
/// element other than the one it matches: in a compound left of a
/// combinator, at any depth of nested selector lists.
#[derive(Debug, Default)]
pub(crate) struct AncestorFeatures {
    classes: FxHashSet<NameId>,
    ids: FxHashSet<String>,
    attributes: FxHashSet<NameId>,
    states: u16,
}

impl AncestorFeatures {
    /// Adds what `selector` uses of other elements: all it uses if it is
    /// for other elements itself (`others`).
    fn add_selector(&mut self, selector: &Selector<Impl>, others: bool) {
        let mut subject = !others;
        for component in selector.iter_raw_match_order() {
            match component {
                // `::before`'s compound is its element's.
                Component::Combinator(Combinator::PseudoElement) => {}
                Component::Combinator(_) => subject = false,
                // `:is(:focus, :hover)` is the element's own.
                component if subject => self.add_nested(component, false),
                component => self.add_component(component),
            }
        }
    }

    fn add_component(&mut self, component: &Component<Impl>) {
        match component {
            Component::Class(class) => {
                self.classes.insert(lower(&class.0));
            }
            Component::ID(id) => {
                self.ids.insert(id.0.to_ascii_lowercase());
            }
            Component::AttributeInNoNamespaceExists { local_name, .. }
            | Component::AttributeInNoNamespace { local_name, .. } => {
                self.attributes.extend(both(&local_name.0));
            }
            Component::AttributeOther(attribute) => {
                self.attributes.extend(both(&attribute.local_name.0));
            }
            Component::NonTSPseudoClass(class) => {
                self.states |= pseudo_class_state(class);
                self.attributes.extend(pseudo_class_attributes(class));
            }
            component => self.add_nested(component, true),
        }
    }

    /// What `component`'s nested selectors use of other elements, or all
    /// they use if `component` is of others itself.
    fn add_nested(&mut self, component: &Component<Impl>, others: bool) {
        match component {
            Component::Is(list) | Component::Where(list) | Component::Negation(list) => {
                for selector in list.slice() {
                    self.add_selector(selector, others);
                }
            }
            Component::Has(list) => {
                for relative in list.iter() {
                    self.add_selector(&relative.selector, true);
                }
            }
            Component::NthOf(data) => {
                for selector in data.selectors() {
                    self.add_selector(selector, others);
                }
            }
            Component::Host(Some(inner)) | Component::Slotted(inner) => {
                self.add_selector(inner, others);
            }
            _ => {}
        }
    }

    #[cfg(debug_assertions)]
    fn merge(&mut self, other: &Self) {
        self.classes.extend(other.classes.iter().copied());
        self.ids.extend(other.ids.iter().cloned());
        self.attributes.extend(other.attributes.iter().copied());
        self.states |= other.states;
    }

    /// Whether `changes` touch any of these.
    pub(crate) fn uses(&self, changes: &AttributeChanges) -> bool {
        changes.states & self.states != 0
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

/// What a change of an element's class, id, attribute or state styles
/// again among its descendants, by the rules that use it of an ancestor
/// (Blink's descendant invalidation sets).
#[derive(Debug, Default)]
struct DescendantInvalidations {
    classes: FxHashMap<NameId, InvalidationFeatures>,
    ids: FxHashMap<String, InvalidationFeatures>,
    attributes: FxHashMap<NameId, InvalidationFeatures>,
    /// By the states a selector's ancestor's compounds use, a change of
    /// any of which is enough.
    states: Vec<(u16, InvalidationFeatures)>,
}

impl DescendantInvalidations {
    fn add(&mut self, ancestors: &AncestorFeatures, set: &InvalidationFeatures) {
        for class in &ancestors.classes {
            self.classes.entry(*class).or_default().add(set);
        }
        for attribute in &ancestors.attributes {
            self.attributes.entry(*attribute).or_default().add(set);
        }
        for id in &ancestors.ids {
            self.ids.entry(id.clone()).or_default().add(set);
        }
        if ancestors.states != 0 {
            match self
                .states
                .iter_mut()
                .find(|(known, _)| *known == ancestors.states)
            {
                Some((_, known)) => known.add(set),
                None => self.states.push((ancestors.states, set.clone())),
            }
        }
    }

    /// Adds to `below` what `changes` (an element's) style again among
    /// its descendants.
    fn find(&self, changes: &AttributeChanges, below: &mut InvalidationFeatures) {
        if !self.attributes.is_empty() {
            for name in &changes.attributes {
                if let Some(set) = self.attributes.get(name) {
                    below.add(set);
                }
            }
        }
        if !self.classes.is_empty() {
            for class in &changes.classes {
                if let Some(set) = names::find(&class.to_ascii_lowercase())
                    .and_then(|name| self.classes.get(&name))
                {
                    below.add(set);
                }
            }
        }
        if !self.ids.is_empty() {
            for id in &changes.ids {
                if let Some(set) = self.ids.get(&id.to_ascii_lowercase()) {
                    below.add(set);
                }
            }
        }
        for (states, set) in &self.states {
            if changes.states & states != 0 {
                below.add(set);
            }
        }
    }
}

/// What a change of one feature of an element styles again among its
/// later siblings, for a rule that uses it left of a sibling combinator
/// (Blink's `SiblingInvalidationSet`): those within `reach` elements after
/// it (a run of `+`; `None`, any number) that have `siblings`' features,
/// themselves (`restyle`: the rule's subject is the sibling) or their
/// descendants that have `descendants`' (`.a ~ .b span`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SiblingInvalidation {
    pub(crate) reach: Option<u8>,
    pub(crate) siblings: InvalidationFeatures,
    pub(crate) restyle: bool,
    pub(crate) descendants: InvalidationFeatures,
}

/// The sibling invalidation sets of an index's rules, by the feature whose
/// change they are for.
#[derive(Debug, Default)]
struct SiblingInvalidations {
    classes: FxHashMap<NameId, Vec<SiblingInvalidation>>,
    ids: FxHashMap<String, Vec<SiblingInvalidation>>,
    attributes: FxHashMap<NameId, Vec<SiblingInvalidation>>,
    states: Vec<(u16, SiblingInvalidation)>,
    /// For `:empty`: an element's children changed.
    empty: Vec<SiblingInvalidation>,
}

impl SiblingInvalidations {
    /// Adds `set` for the feature `component` is of (in a compound left of
    /// a sibling combinator): `false` if it is of none (a nested list).
    fn add(&mut self, component: &Component<Impl>, set: &SiblingInvalidation) -> bool {
        fn push(sets: &mut Vec<SiblingInvalidation>, set: &SiblingInvalidation) {
            if !sets.contains(set) {
                sets.push(set.clone());
            }
        }
        let mut attribute = |name: NameId| push(self.attributes.entry(name).or_default(), set);
        match component {
            Component::Class(class) => push(self.classes.entry(lower(&class.0)).or_default(), set),
            Component::ID(id) => push(self.ids.entry(id.0.to_ascii_lowercase()).or_default(), set),
            Component::AttributeInNoNamespaceExists { local_name, .. }
            | Component::AttributeInNoNamespace { local_name, .. } => {
                both(&local_name.0).into_iter().for_each(&mut attribute)
            }
            Component::AttributeOther(other) => both(&other.local_name.0)
                .into_iter()
                .for_each(&mut attribute),
            Component::NonTSPseudoClass(class) => {
                // Those of an attribute: `:open`, a link's `href`, a
                // language's `lang` (an ancestor's styles its whole
                // subtree anyway: `for_descendants`), the form ones'.
                let attributes = pseudo_class_attributes(class);
                attributes.iter().copied().for_each(&mut attribute);
                let state = pseudo_class_state(class);
                if state != 0 {
                    let entry = (state, set.clone());
                    if !self.states.contains(&entry) {
                        self.states.push(entry);
                    }
                } else if attributes.is_empty() {
                    // What the sets don't say: the fallback's.
                    return false;
                }
            }
            Component::Empty => push(&mut self.empty, set),
            // What never changes of an element: its name.
            Component::LocalName(_)
            | Component::ExplicitUniversalType
            | Component::ExplicitAnyNamespace
            | Component::ExplicitNoNamespace
            | Component::DefaultNamespace(_)
            | Component::Namespace(..) => {}
            // Its position: what children coming or going style again.
            Component::Nth(_) => {}
            _ => return false,
        }
        true
    }

    /// The sets for `changes`.
    fn for_change<'s>(
        &'s self,
        changes: &AttributeChanges,
        sets: &mut Vec<&'s SiblingInvalidation>,
    ) {
        for (state, set) in &self.states {
            if changes.states & state != 0 {
                sets.push(set);
            }
        }
        for class in &changes.classes {
            if let Some(found) =
                names::find(&class.to_ascii_lowercase()).and_then(|name| self.classes.get(&name))
            {
                sets.extend(found);
            }
        }
        for id in &changes.ids {
            if let Some(found) = self.ids.get(&id.to_ascii_lowercase()) {
                sets.extend(found);
            }
        }
        for name in &changes.attributes {
            if let Some(found) = self.attributes.get(name) {
                sets.extend(found);
            }
        }
    }
}

/// An index's invalidation sets: what its rules use of elements other than
/// the ones they match.
#[derive(Debug, Default)]
pub(crate) struct FeatureSets {
    /// What rules the sets don't say (a shadow tree's element's) use of
    /// other elements: a change of one styles the whole subtree.
    any: AncestorFeatures,
    descendants: DescendantInvalidations,
    /// All `any` and `descendants` take: what styled the whole subtree
    /// before the sets, for the debug build's check of what they pass over.
    #[cfg(debug_assertions)]
    pub(crate) all_ancestors: AncestorFeatures,
    siblings: SiblingInvalidations,
    /// What rules use of an element's siblings that the sets don't say
    /// (`:nth-child(2 of .a)`, `:is(.a, .b) ~ .c`): a change of one styles
    /// its later siblings again.
    sibling_fallback: AncestorFeatures,
    /// What a position counted from the end among the siblings matching a
    /// list uses (`:nth-last-child(1 of .a)`): a change of one styles its
    /// earlier siblings again too.
    earlier_fallback: AncestorFeatures,
    /// The same as `sibling_fallback` and `siblings` take, for the debug
    /// build's check (every later sibling, before the sets).
    #[cfg(debug_assertions)]
    pub(crate) all_siblings: AncestorFeatures,
}

/// What a change of an element styles again of the others, by an index's
/// sets: below it (`below`; `subtree`: all of it), and its later siblings
/// (`siblings`; `later`: all of them), and its earlier ones (`earlier`:
/// all of them).
#[derive(Default)]
pub(crate) struct Invalidated<'s> {
    pub(crate) subtree: bool,
    pub(crate) below: InvalidationFeatures,
    pub(crate) siblings: Vec<&'s SiblingInvalidation>,
    pub(crate) later: bool,
    pub(crate) earlier: bool,
}

impl FeatureSets {
    /// Adds what `selector` uses of others.
    pub(crate) fn add_selector(&mut self, selector: &Selector<Impl>) {
        #[cfg(debug_assertions)]
        if SiblingReach::of(selector) != SiblingReach::default() {
            self.all_siblings.add_selector(selector, true);
        }
        // Its compounds in match order (the subject's first, `::before`'s
        // with its element's), and the combinator right of each other.
        let mut compounds: Vec<Vec<&Component<Impl>>> = vec![Vec::new()];
        let mut combinators = Vec::new();
        for component in selector.iter_raw_match_order() {
            match component {
                Component::Combinator(Combinator::PseudoElement) => {}
                Component::Combinator(combinator) => {
                    combinators.push(*combinator);
                    compounds.push(Vec::new());
                }
                component => compounds.last_mut().expect("one").push(component),
            }
        }
        // A shadow tree's element's (`::slotted()`, a part's): as the
        // sets don't say.
        let plain = combinators.iter().all(|combinator| {
            matches!(
                combinator,
                Combinator::Descendant
                    | Combinator::Child
                    | Combinator::NextSibling
                    | Combinator::LaterSibling
            )
        }) && !compounds[0].iter().any(|component| {
            // A `::selection` is the element's own (its highlight, which
            // its children inherit); another pseudo-element of no id is a
            // UA shadow tree's element's.
            matches!(component, Component::PseudoElement(p) if p.id().is_none() && *p != PseudoElement::Selection)
        });
        if !plain {
            self.any.add_selector(selector, false);
            #[cfg(debug_assertions)]
            self.all_ancestors.add_selector(selector, false);
            if SiblingReach::of(selector) != SiblingReach::default() {
                self.sibling_fallback.add_selector(selector, true);
            }
            return;
        }
        let subject = InvalidationFeatures::of(&compounds[0]);
        // What a change of an ancestor's feature styles again below it:
        // the elements with the subject's features, or all.
        let below =
            InvalidationFeatures::of_compound(&compounds[0]).unwrap_or(InvalidationFeatures {
                any: true,
                ..InvalidationFeatures::default()
            });
        let mut ancestors = AncestorFeatures::default();
        for (index, compound) in compounds.iter().enumerate() {
            // Positions counted among siblings that match a list, or
            // siblings nested in a list: what it uses across siblings.
            for component in compound {
                let nested_siblings = match component {
                    Component::NthOf(_) => true,
                    Component::Is(list) | Component::Where(list) | Component::Negation(list) => {
                        list.slice()
                            .iter()
                            .any(|selector| SiblingReach::of(selector) != SiblingReach::default())
                    }
                    Component::Host(Some(inner)) | Component::Slotted(inner) => {
                        SiblingReach::of(inner) != SiblingReach::default()
                    }
                    _ => false,
                };
                if nested_siblings {
                    self.sibling_fallback.add_nested(component, true);
                    if let Component::NthOf(data) = component
                        && data.nth_data().ty.is_from_end()
                    {
                        self.earlier_fallback.add_nested(component, true);
                    }
                    if nests(component, |c| matches!(c, Component::Empty)) {
                        let every = InvalidationFeatures {
                            any: true,
                            ..InvalidationFeatures::default()
                        };
                        let set = SiblingInvalidation {
                            reach: None,
                            siblings: every.clone(),
                            restyle: true,
                            descendants: every,
                        };
                        self.siblings.add(&Component::Empty, &set);
                    }
                }
            }
            if index == 0 {
                // The subject's own nested lists (`:is(.b .a)`'s `.b` is
                // an ancestor's).
                for component in compound {
                    ancestors.add_nested(component, false);
                }
                continue;
            }
            #[expect(clippy::arithmetic_side_effects, reason = "index > 0 here")]
            let left = index - 1;
            if !combinators[left].is_sibling() {
                for component in compound {
                    ancestors.add_component(component);
                }
                continue;
            }
            // Left of a sibling combinator: a change styles again the later
            // siblings that match the compound at the end of the run of
            // sibling combinators, or the descendants of those that match
            // the subject.
            let mut target = left;
            while let Some(before) = target.checked_sub(1)
                && combinators[before].is_sibling()
            {
                target = before;
            }
            let run = &combinators[target..index];
            let set = SiblingInvalidation {
                reach: run
                    .iter()
                    .all(|combinator| *combinator == Combinator::NextSibling)
                    .then(|| u8::try_from(run.len()).unwrap_or(u8::MAX)),
                siblings: InvalidationFeatures::of(&compounds[target]),
                restyle: target == 0,
                descendants: if target == 0 {
                    InvalidationFeatures::default()
                } else {
                    subject.clone()
                },
            };
            for component in compound {
                if !self.siblings.add(component, &set) {
                    // A nested list: what it uses left of its combinators
                    // (`:is(.b .a) ~ .c`'s `.b`) is an ancestor's.
                    ancestors.add_nested(component, false);
                    self.sibling_fallback.add_component(component);
                    if nests(component, |c| matches!(c, Component::Empty)) {
                        self.siblings.add(&Component::Empty, &set);
                    }
                }
            }
        }
        self.descendants.add(&ancestors, &below);
        #[cfg(debug_assertions)]
        self.all_ancestors.merge(&ancestors);
    }

    /// What `changes` (an element's) style again of other elements.
    pub(crate) fn invalidated<'s>(&'s self, changes: &AttributeChanges, out: &mut Invalidated<'s>) {
        out.subtree |=
            self.any.uses(changes) || changes.attributes.iter().any(|name| for_descendants(*name));
        self.descendants.find(changes, &mut out.below);
        self.siblings.for_change(changes, &mut out.siblings);
        out.later |= self.sibling_fallback.uses(changes);
        out.earlier |= self.earlier_fallback.uses(changes);
    }

    /// The sibling sets for an element whose children changed (`:empty`).
    pub(crate) fn empty_changed<'s>(&'s self, out: &mut Invalidated<'s>) {
        out.siblings.extend(&self.siblings.empty);
    }
}
