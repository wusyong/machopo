//! Which elements a change of the document's sheets styles again, rather
//! than all of them (Blink's `StyleEngine::InvalidateForRuleSetChanges`,
//! with the `RuleInvalidationData` of the rule sets that changed; as
//! `style`'s, which this follows).
//!
//! What a changed rule's selector needs of the elements it matches tells
//! which may match it: its subject's compound's id, class, attribute or
//! local name (`InvalidationFeatures::of`), whose elements are styled
//! again (Blink's self invalidation). A subject with none takes the
//! nearest compound to its left that has one: the subtrees of its elements
//! (an ancestor's, `.a *`), or their later siblings (`.a + *`: a sibling
//! set). A selector with none anywhere, or one with `:has()`, and an
//! `@keyframes` rule, change what any element may be: everything is styled
//! again.
//!
//! A shadow tree's `:host` rules style its host again, and its
//! `::slotted()` rules what is assigned to its slots; one whose rules may
//! match anything, each element of its tree, its host and what its slots
//! are assigned (Blink's for a tree scope's sheets that changed in full).
//!
//! The rules that changed are those of the sheets that came and went, less
//! those both had (the same `Arc`): Blink's `CompareActiveStyleSheets`,
//! with `RuleSetDiff`.

use std::sync::Arc;

use css::CssRule;
use css::media::MediaEnvironment;
use css::selector::{Impl, PseudoElement};
use dom::{Cx, NameId, NodeRef, names};
use rustc_hash::{FxHashMap, FxHashSet};
use selectors::parser::{Combinator, Component, Selector};

use crate::invalidation::{InvalidationFeatures, SiblingInvalidation, nests};

/// What a change of the document's sheets styles again.
#[expect(
    clippy::large_enum_variant,
    reason = "one a style update whose sheets changed, moved once to where it is used"
)]
pub(crate) enum SheetsChange {
    Unchanged,
    /// The elements this finds, and what they change for their children
    /// to inherit.
    Rules(RuleSetInvalidation),
    /// Every element.
    Everything,
}

/// The ids (ASCII lowercase), classes (by their names ASCII lowercase),
/// attribute names and local names (as written and ASCII lowercase) an
/// element may have one of, to look its own up in (Blink's
/// `RuleInvalidationData` maps: a sheet's worth of rules, not a list to
/// scan for each element).
#[derive(Debug, Default)]
struct FeatureLookup {
    ids: FxHashSet<String>,
    classes: FxHashSet<NameId>,
    attributes: FxHashSet<NameId>,
    names: FxHashSet<NameId>,
}

impl FeatureLookup {
    /// Adds `features`, which name some (not `any`).
    fn add(&mut self, features: &InvalidationFeatures) {
        debug_assert!(!features.any);
        self.ids.extend(features.ids.iter().cloned());
        self.classes.extend(features.classes.iter().copied());
        self.attributes.extend(features.attributes.iter().copied());
        self.names.extend(features.names.iter().copied());
    }

    /// Whether `element` has one of them.
    fn matches<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
        if !self.ids.is_empty()
            && cx
                .attribute_value_by_id(element, names::ID)
                .is_some_and(|id| {
                    let id = id.borrow();
                    let id = id.to_str();
                    if id.bytes().any(|byte| byte.is_ascii_uppercase()) {
                        self.ids.contains(&id.to_ascii_lowercase())
                    } else {
                        self.ids.contains(&*id)
                    }
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

/// What finds the elements that the rules of a change may match: those it
/// styles again, those whose subtrees it does, and those whose later
/// siblings it does (Blink's self, descendant and sibling invalidation
/// sets of the changed rule sets' features).
#[derive(Debug, Default)]
pub(crate) struct RuleSetInvalidation {
    /// A `:host` rule changed: the host is styled again.
    pub(crate) host: bool,
    /// A `::slotted()` rule changed: what is assigned to the tree's slots
    /// is styled again (Blink's `InvalidateSlottedElements`).
    pub(crate) slotted: bool,
    /// Every element of the tree is styled again, each for itself (what
    /// their children inherit decides about those below).
    all: bool,
    own: FeatureLookup,
    subtree: FeatureLookup,
    siblings: Vec<(FeatureLookup, SiblingInvalidation)>,
    /// The changed rules' selectors, for the debug build to check that an
    /// element none of the sets found matches none of them.
    #[cfg(debug_assertions)]
    selectors: Vec<Selector<Impl>>,
}

/// What an element's features make of a change of rules: whether it is
/// styled again, and its whole subtree.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Found {
    pub(crate) own: bool,
    pub(crate) subtree: bool,
}

impl RuleSetInvalidation {
    /// What styles again each element of a shadow tree whose rules may
    /// match anything now, its host and what is assigned to its slots.
    pub(crate) fn every_element() -> Self {
        Self {
            host: true,
            slotted: true,
            all: true,
            ..Self::default()
        }
    }

    /// What finds what `rules` (those added and those removed) match:
    /// those whose conditions hold in `environment`, as the rule sets have
    /// them; `None` if they may change any element.
    pub(crate) fn of<'r>(
        rules: impl IntoIterator<Item = &'r CssRule>,
        environment: &MediaEnvironment,
    ) -> Option<Self> {
        let mut invalidation = Self::default();
        for rule in rules {
            if !invalidation.add_rule(rule, environment) {
                return None;
            }
        }
        Some(invalidation)
    }

    fn add_rule(&mut self, rule: &CssRule, environment: &MediaEnvironment) -> bool {
        match rule {
            CssRule::Style(rule) => rule
                .selectors
                .slice()
                .iter()
                .all(|selector| self.add_selector(selector)),
            CssRule::Media(rule) => {
                !rule.media.matches(environment)
                    || rule
                        .rules
                        .iter()
                        .all(|rule| self.add_rule(rule, environment))
            }
            CssRule::Supports(rule) => {
                !rule.supported
                    || rule
                        .rules
                        .iter()
                        .all(|rule| self.add_rule(rule, environment))
            }
            // What an animation-name finds, which any element may have
            // (Blink styles again those with animations).
            CssRule::Keyframes(_) | CssRule::Keyframe(_) => false,
        }
    }

    /// Adds what finds the elements `selector` may match; false if any may.
    fn add_selector(&mut self, selector: &Selector<Impl>) -> bool {
        // What matches it depends on its elements' descendants or later
        // siblings, which no set here says.
        if selector
            .iter_raw_match_order()
            .any(|component| nests(component, |c| matches!(c, Component::Has(_))))
        {
            return false;
        }
        // A host's or what is assigned to a slot (`:host(.a)`,
        // `slot::slotted(p)`): in the tree around the shadow tree (the
        // document's sheets' match neither).
        let subject: Vec<&Component<Impl>> = selector
            .iter_raw_match_order()
            .take_while(|component| {
                !matches!(component, Component::Combinator(c) if *c != Combinator::PseudoElement)
            })
            .collect();
        if subject
            .iter()
            .any(|component| matches!(component, Component::Slotted(_)))
        {
            self.slotted = true;
            return true;
        }
        if subject
            .iter()
            .any(|component| matches!(component, Component::Host(_)))
        {
            self.host = true;
            return true;
        }
        #[cfg(debug_assertions)]
        self.selectors.push(selector.clone());
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
        // A slot's or a part's: matched across tree scopes.
        if !combinators.iter().all(|combinator| {
            matches!(
                combinator,
                Combinator::Descendant
                    | Combinator::Child
                    | Combinator::NextSibling
                    | Combinator::LaterSibling
            )
        }) {
            return false;
        }
        // An element of a UA shadow tree (`input::placeholder`), whose
        // compound is its host's: the host's subtree, its shadow tree in it.
        // A `::selection` is the element's own (its highlight, which its
        // children inherit).
        let shadow_part = compounds[0].iter().any(|component| {
            matches!(component, Component::PseudoElement(p) if p.id().is_none() && *p != PseudoElement::Selection)
        });
        if let Some(features) = InvalidationFeatures::of_compound(&compounds[0]) {
            if shadow_part {
                self.subtree.add(&features);
            } else {
                self.own.add(&features);
            }
            return true;
        }
        if shadow_part {
            return false;
        }
        let Some((index, features)) =
            compounds
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(index, compound)| {
                    Some((index, InvalidationFeatures::of_compound(compound)?))
                })
        else {
            return false;
        };
        // An ancestor: the subject is in its subtree, siblings to the
        // right of a descendant staying in it.
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "index > 0: the first is skipped"
        )]
        let left = index - 1;
        if !combinators[left].is_sibling() {
            self.subtree.add(&features);
            return true;
        }
        // A sibling: the subject is one of its later siblings, at the end
        // of the run of sibling combinators, or in the subtree of one.
        let mut target = left;
        while let Some(before) = target.checked_sub(1)
            && combinators[before].is_sibling()
        {
            target = before;
        }
        let run = &combinators[target..index];
        let every = InvalidationFeatures {
            any: true,
            ..InvalidationFeatures::default()
        };
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
                every
            },
        };
        let mut lookup = FeatureLookup::default();
        lookup.add(&features);
        self.siblings.push((lookup, set));
        true
    }

    /// What `element` has of the features: whether it is styled again, or
    /// its subtree; the sets for its later siblings go in `siblings`.
    pub(crate) fn find<'s, 'gc>(
        &'s self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        siblings: &mut Vec<&'s SiblingInvalidation>,
    ) -> Found {
        for (features, set) in &self.siblings {
            if features.matches(cx, element) {
                siblings.push(set);
            }
        }
        Found {
            own: self.all || self.own.matches(cx, element),
            subtree: self.subtree.matches(cx, element),
        }
    }
}

#[cfg(debug_assertions)]
impl RuleSetInvalidation {
    /// Panics if `element`, which the sets didn't find, matches one of the
    /// changed rules (`::before`'s as its element, as the cascade does).
    pub(crate) fn assert_unmatched(
        &self,
        element: &crate::selector::StyleElement<'_, '_>,
        quirks_mode: selectors::matching::QuirksMode,
        host: Option<selectors::OpaqueElement>,
    ) {
        use selectors::context::SelectorCaches;
        use selectors::matching::{
            MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags,
            matches_selector,
        };
        for selector in &self.selectors {
            let mode = if selector
                .pseudo_element()
                .and_then(|pseudo| pseudo.id())
                .is_some()
            {
                MatchingMode::ForStatelessPseudoElement
            } else {
                MatchingMode::Normal
            };
            let mut caches = SelectorCaches::default();
            let mut context = MatchingContext::new(
                mode,
                None,
                &mut caches,
                quirks_mode,
                NeedsSelectorFlags::No,
                MatchingForInvalidation::No,
            );
            context.current_host = host;
            assert!(
                !matches_selector(selector, 0, None, element, &mut context),
                "a changed rule matches what its sets passed over: {selector:?}",
            );
        }
    }
}

/// One of the document's sheets, old or new, as a change compares them:
/// its rules if its condition holds (`None` if not), and whether it is one
/// of those both have.
pub(crate) struct ScopeSheetRules<'a> {
    pub(crate) rules: Option<&'a [CssRule]>,
    pub(crate) kept: bool,
}

/// The address of `rule`'s own `Arc`: the same rule in two sheets' rules
/// by its address.
fn address(rule: &CssRule) -> *const () {
    match rule {
        CssRule::Style(rule) => Arc::as_ptr(rule).cast(),
        CssRule::Media(rule) => Arc::as_ptr(rule).cast(),
        CssRule::Supports(rule) => Arc::as_ptr(rule).cast(),
        CssRule::Keyframes(rule) => Arc::as_ptr(rule).cast(),
        CssRule::Keyframe(rule) => Arc::as_ptr(rule).cast(),
    }
}

/// The rules of `old`'s sheets that went and `new`'s that came, in cascade
/// order (those both kept in the same order), less those of both: a rule
/// in a sheet that went and in one that came where as many kept sheets
/// come before it and in the same order as the others' (Blink's
/// `RuleSetDiff`). `None` if one is elsewhere: what it cascades with
/// changed.
pub(crate) fn changed_rules<'a>(
    old: &[ScopeSheetRules<'a>],
    new: &[ScopeSheetRules<'a>],
) -> Option<Vec<&'a CssRule>> {
    // Each side's rules of the sheets that went or came, with how many kept
    // sheets come before theirs.
    fn changed<'a>(sheets: &[ScopeSheetRules<'a>]) -> Vec<(usize, &'a CssRule)> {
        let mut kept = 0;
        let mut rules = Vec::new();
        for sheet in sheets {
            if sheet.kept {
                #[expect(clippy::arithmetic_side_effects, reason = "one per sheet: a slice's")]
                {
                    kept += 1;
                }
            } else if let Some(sheet_rules) = sheet.rules {
                rules.extend(sheet_rules.iter().map(|rule| (kept, rule)));
            }
        }
        rules
    }
    let (went, came) = (changed(old), changed(new));
    // Each address once on a side: one twice isn't taken as the same rule.
    let once = |rules: &[(usize, &'a CssRule)]| {
        let mut count: FxHashMap<*const (), (usize, usize)> = FxHashMap::default();
        for (index, (place, rule)) in rules.iter().enumerate() {
            count
                .entry(address(rule))
                .and_modify(|entry| entry.1 = usize::MAX)
                .or_insert((*place, index));
        }
        count
    };
    let (old_rules, new_rules) = (once(&went), once(&came));
    let both = |address: *const ()| {
        let (old_place, old_index) = *old_rules.get(&address)?;
        let (new_place, new_index) = *new_rules.get(&address)?;
        (old_index != usize::MAX && new_index != usize::MAX)
            .then_some((old_place, old_index, new_place))
    };
    // In the same place, and the same order, as before.
    let mut last = None;
    for (_, rule) in &came {
        if let Some((old_place, old_index, new_place)) = both(address(rule)) {
            if old_place != new_place || last.is_some_and(|last| last > old_index) {
                return None;
            }
            last = Some(old_index);
        }
    }
    Some(
        went.iter()
            .chain(&came)
            .filter(|(_, rule)| both(address(rule)).is_none())
            .map(|(_, rule)| *rule)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use css::stylesheet::{Origin, ParseLog, Stylesheet};

    fn rules(css: &str) -> Vec<CssRule> {
        Stylesheet::parse(css, Origin::Author, &mut ParseLog::default()).rules
    }

    fn sheet(rules: &[CssRule], kept: bool) -> ScopeSheetRules<'_> {
        ScopeSheetRules {
            rules: Some(rules),
            kept,
        }
    }

    /// Rules both sides have in the same place cancel; a sheet kept has
    /// none; one in another place or order, or twice on a side, doesn't.
    #[test]
    fn changed_rules_leave_out_what_both_have() {
        let (a, b, c) = (rules("a {}"), rules("b {}"), rules("c {}"));
        let kept = rules("k {}");
        let ab: Vec<CssRule> = a.iter().chain(&b).cloned().collect();
        let abc: Vec<CssRule> = ab.iter().chain(&c).cloned().collect();
        let ba: Vec<CssRule> = b.iter().chain(&a).cloned().collect();
        let names = |found: Option<Vec<&CssRule>>| {
            found.map(|found| {
                found
                    .iter()
                    .map(|rule| match rule {
                        CssRule::Style(rule) => rule.selector_text(),
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>()
            })
        };
        // Edited: `c` added after what it had.
        assert_eq!(
            names(changed_rules(
                &[sheet(&kept, true), sheet(&ab, false)],
                &[sheet(&kept, true), sheet(&abc, false)],
            )),
            Some(vec!["c".to_owned()])
        );
        // Moved past a kept sheet: what it cascades with changed.
        assert_eq!(
            names(changed_rules(
                &[sheet(&ab, false), sheet(&kept, true)],
                &[sheet(&kept, true), sheet(&abc, false)],
            )),
            None
        );
        // In another order.
        assert_eq!(
            names(changed_rules(&[sheet(&ab, false)], &[sheet(&ba, false)])),
            None
        );
        // Twice on a side: none cancel.
        assert_eq!(
            names(changed_rules(
                &[sheet(&a, false)],
                &[sheet(&a, false), sheet(&a, false)]
            )),
            Some(vec!["a".to_owned(), "a".to_owned(), "a".to_owned()])
        );
        // A condition that doesn't hold: its rules don't count.
        assert_eq!(
            names(changed_rules(
                &[ScopeSheetRules {
                    rules: None,
                    kept: false
                }],
                &[sheet(&a, false)],
            )),
            Some(vec!["a".to_owned()])
        );
    }
}
