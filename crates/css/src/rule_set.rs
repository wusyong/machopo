//! A stylesheet's selectors sorted into buckets by what the rightmost
//! compound of each needs (Blink's `RuleSet`): an element looks only in the
//! buckets of its id, its classes and its name, and in the one for the
//! rest, and matches only the selectors there.
//!
//! The buckets only narrow down: every selector found is still matched in
//! full, so a bucket may hold selectors an element doesn't match, never
//! leave out one it does.
//!
//! A shadow tree's sheets' `:host` rules (whose rightmost compound is the
//! featureless host) and `::slotted()` rules are in no bucket but lists of
//! their own: only the shadow tree's host matches the former, and only the
//! elements assigned to its slots the latter (Blink's
//! `shadow_host_rules_` and `slotted_pseudo_element_rules_`). So are the
//! rules for an element of a UA shadow tree (`::placeholder`): only that
//! element matches them, with its host's tree's sheets (Blink's
//! `ua_shadow_pseudo_element_rules_`).
//!
//! A set holds the style rules whose `@media` and `@supports` held when it
//! was made, and remembers each media query it evaluated with its result:
//! when the environment changes, it is made again only if one of them
//! comes out otherwise (Blink's `RuleSet::DidMediaQueryResultsChange`).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use selectors::context::QuirksMode;
use selectors::parser::{AncestorHashes, Combinator, Component, Selector};

use crate::media::{MediaEnvironment, MediaQueryList};
use crate::selector::{Impl, name_hash};
use crate::stylesheet::{CssRule, KeyframesRule, StyleRule};

thread_local! {
    /// How many style rules were put in sets. Counted in debug builds
    /// only.
    static RULES_ADDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many style rules, on this thread, were put in sets: what making
/// sets again costs. `None` in release builds. For tests.
pub fn rules_added_for_testing() -> Option<usize> {
    cfg!(debug_assertions).then(|| RULES_ADDED.get())
}

/// A selector of a set (Blink's `RuleData`): its rule's index in the set's
/// rules, its own in the rule's selector list, its specificity, and the
/// hashes of names its element's ancestors must have (`selectors`'
/// `AncestorHashes`, packed; Blink's `descendant_selector_identifier_hashes_`),
/// for the ancestor bloom filter. Sorts in the set's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuleData {
    pub rule: u32,
    pub selector: u32,
    pub specificity: u32,
    pub ancestor_hashes: [u32; 3],
}

/// A style rule in a set, with the index of the sheet it came from (of
/// those the set was made of), for what its `url()`s resolve against.
pub struct ActiveRule {
    pub rule: Arc<StyleRule>,
    pub sheet: u32,
}

/// The selectors of one or more sheets by bucket. Ids and classes are
/// keyed by their `name_hash`, which is ASCII case-insensitive, whatever
/// the sheet's case: one sheet may be cascaded in a quirks mode document,
/// where they match ASCII case-insensitively, and in a standards mode one,
/// where the extra selectors found don't match.
#[derive(Default)]
pub struct RuleSet {
    /// The style rules whose conditions held, in order.
    rules: Vec<ActiveRule>,
    /// Each media query list evaluated in making it, with its result
    /// (Blink's `media_query_set_results_`).
    media_results: Vec<(Arc<MediaQueryList>, bool)>,
    ids: NameBuckets,
    classes: NameBuckets,
    /// By the name as written and, when it differs, ASCII lowercase: an
    /// HTML element in an HTML document matches the lowercase one, any
    /// other element the name as written (`clipPath`), as `selectors` does.
    tags: FxHashMap<Box<str>, Vec<RuleData>>,
    /// Whatever has none of those in its rightmost compound: `*`,
    /// attributes, pseudo-classes, `:is()`.
    other: Vec<RuleData>,
    /// The selectors whose rightmost compound is `:host` or `:host()`.
    host: Vec<RuleData>,
    /// The selectors with `::slotted()`.
    slotted: Vec<RuleData>,
    /// The selectors whose pseudo-element is an element of a UA shadow
    /// tree.
    shadow_pseudo: Vec<RuleData>,
    /// The `@keyframes` whose conditions held, by name, each with its
    /// sheet: of two with one name, the later (Blink's
    /// `RuleSet::keyframes_rules_`, which `ScopedStyleResolver` keeps by
    /// name).
    keyframes: FxHashMap<Arc<str>, (Arc<KeyframesRule>, u32)>,
}

impl RuleSet {
    /// A set of `rules`, which have no conditions.
    pub fn new<'a>(rules: impl IntoIterator<Item = &'a Arc<StyleRule>>) -> Self {
        let mut set = Self::default();
        for rule in rules {
            set.add_style_rule(rule, 0);
        }
        set
    }

    /// Adds `rules`, of sheet `sheet`: the style rules, and those of the
    /// group rules whose conditions hold in `environment` (Blink's
    /// `RuleSet::AddChildRules`).
    pub fn add_rules(&mut self, rules: &[CssRule], environment: &MediaEnvironment, sheet: u32) {
        for rule in rules {
            match rule {
                CssRule::Style(rule) => self.add_style_rule(rule, sheet),
                CssRule::Media(media) => {
                    if self.match_media(&media.media, environment) {
                        self.add_rules(&media.rules, environment, sheet);
                    }
                }
                CssRule::Supports(supports) => {
                    if supports.supported {
                        self.add_rules(&supports.rules, environment, sheet);
                    }
                }
                CssRule::Keyframes(keyframes) => {
                    self.keyframes
                        .insert(keyframes.name.clone(), (keyframes.clone(), sheet));
                }
                CssRule::Keyframe(_) => {}
            }
        }
    }

    /// Whether `media` matches in `environment`, remembered with the result
    /// (Blink's `RuleSet::MatchMediaForAddRules`): a sheet's own `media`
    /// is evaluated here too.
    pub fn match_media(
        &mut self,
        media: &Arc<MediaQueryList>,
        environment: &MediaEnvironment,
    ) -> bool {
        let matches = media.matches(environment);
        self.media_results.push((media.clone(), matches));
        matches
    }

    /// Whether some media query evaluated in making it comes out otherwise
    /// in `environment`: it must be made again.
    pub fn did_media_query_results_change(&self, environment: &MediaEnvironment) -> bool {
        self.media_results
            .iter()
            .any(|(media, matched)| media.matches(environment) != *matched)
    }

    /// Whether some media query was evaluated in making it.
    pub fn depends_on_media(&self) -> bool {
        !self.media_results.is_empty()
    }

    /// Its style rules, in order: `RuleData::rule` indexes them.
    pub fn rules(&self) -> &[ActiveRule] {
        &self.rules
    }

    /// The `@keyframes` named `name` (case-sensitively), with the index of
    /// its sheet.
    pub fn keyframes(&self, name: &str) -> Option<(&Arc<KeyframesRule>, u32)> {
        self.keyframes.get(name).map(|(rule, sheet)| (rule, *sheet))
    }

    /// Whether it has a `@keyframes`.
    pub fn has_keyframes(&self) -> bool {
        !self.keyframes.is_empty()
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "rules and a rule's selectors are parsed from text in memory: fewer than \
                  2^32 of either"
    )]
    fn add_style_rule(&mut self, rule: &Arc<StyleRule>, sheet: u32) {
        if cfg!(debug_assertions) {
            RULES_ADDED.set(RULES_ADDED.get().wrapping_add(1));
        }
        let rule_index = self.rules.len() as u32;
        for (selector_index, selector) in rule.selectors.slice().iter().enumerate() {
            let data = RuleData {
                rule: rule_index,
                selector: selector_index as u32,
                specificity: selector.specificity(),
                // In a standards mode document's terms: `name_hash` is
                // case-insensitive, so they hold in a quirks mode one too.
                ancestor_hashes: AncestorHashes::new(selector, QuirksMode::NoQuirks).packed_hashes,
            };
            self.add(selector, data);
        }
        self.rules.push(ActiveRule {
            rule: rule.clone(),
            sheet,
        });
    }

    /// Puts `selector` in one bucket, the one fewest elements look in:
    /// its id's, else its class's, else its name's (Blink's
    /// `RuleSet::FindBestRuleSetAndAdd`).
    fn add(&mut self, selector: &Selector<Impl>, data: RuleData) {
        let mut id = None;
        let mut class = None;
        let mut tag = None;
        let mut host = false;
        let mut slotted = false;
        let mut shadow_pseudo = false;
        // The rightmost compound only: `iter` stops at its combinator.
        // What is inside `:is()`, `:where()`, `:not()` and `:has()` doesn't
        // count (`:not(.a)` matches what has no class `a`). A pseudo-element
        // is a compound of its own, after its element's: that one is the
        // element's (`.a::before` is in `a`'s bucket, as in Blink).
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
                        shadow_pseudo |= pseudo.shadow_pseudo_id().is_some();
                    }
                    _ => {}
                }
            }
            if iter.next_sequence() != Some(Combinator::PseudoElement) {
                break;
            }
        }
        if slotted {
            self.slotted.push(data);
        } else if shadow_pseudo {
            self.shadow_pseudo.push(data);
        } else if host {
            self.host.push(data);
        } else if let Some(id) = id {
            self.ids.push(id, data);
        } else if let Some(class) = class {
            self.classes.push(class, data);
        } else if let Some((name, lower)) = tag {
            push(&mut self.tags, name.as_str().into(), data);
            if lower != name {
                push(&mut self.tags, lower.as_str().into(), data);
            }
        } else {
            self.other.push(data);
        }
    }

    /// Adds to `out` the selectors an element with this `id`, these
    /// `classes` (each with its `name_hash`) and this `local_name` may
    /// match, in no particular order and maybe more than once (an element's
    /// `Foo` and `foo` classes look in the same bucket).
    pub fn collect(
        &self,
        id: Option<(&str, u32)>,
        classes: impl IntoIterator<Item = (impl AsRef<str>, u32)>,
        local_name: &str,
        out: &mut Vec<RuleData>,
    ) {
        if let Some((id, hash)) = id {
            out.extend_from_slice(self.ids.get(id, hash));
        }
        for (class, hash) in classes {
            out.extend_from_slice(self.classes.get(class.as_ref(), hash));
        }
        if let Some(found) = self.tags.get(local_name) {
            out.extend_from_slice(found);
        }
        out.extend_from_slice(&self.other);
    }
}

impl RuleSet {
    /// The `:host` and `:host()` selectors, which only a shadow host
    /// matches (with its shadow tree's sheets).
    pub fn host_rules(&self) -> &[RuleData] {
        &self.host
    }

    /// The `::slotted()` selectors, which only elements assigned to a slot
    /// of the sheets' shadow tree match.
    pub fn slotted_rules(&self) -> &[RuleData] {
        &self.slotted
    }

    /// The selectors for an element of a UA shadow tree, which only that
    /// element matches (with its host's tree's sheets).
    pub fn shadow_pseudo_rules(&self) -> &[RuleData] {
        &self.shadow_pseudo
    }
}

fn push(map: &mut FxHashMap<Box<str>, Vec<RuleData>>, key: Box<str>, data: RuleData) {
    map.entry(key).or_default().push(data);
}

/// Buckets by an id's or a class's `name_hash`, each with its name ASCII
/// lowercase: an element looks one up by its own name's hash, worked out
/// once (`dom` keeps it), with no string hashed or lowercased again.
#[derive(Default)]
struct NameBuckets(FxHashMap<u32, Vec<NamedRules>>);

/// A name ASCII lowercase, and its bucket.
type NamedRules = (Box<str>, Vec<RuleData>);

impl NameBuckets {
    fn push(&mut self, name: &str, data: RuleData) {
        let names = self.0.entry(name_hash(name)).or_default();
        match names
            .iter_mut()
            .find(|(other, _)| other.eq_ignore_ascii_case(name))
        {
            Some((_, rules)) => rules.push(data),
            None => names.push((name.to_ascii_lowercase().into(), vec![data])),
        }
    }

    /// The bucket of `name`, whose `name_hash` is `hash`, ASCII
    /// case-insensitively.
    fn get(&self, name: &str, hash: u32) -> &[RuleData] {
        debug_assert_eq!(hash, name_hash(name));
        self.0
            .get(&hash)
            .and_then(|names| {
                names
                    .iter()
                    .find(|(other, _)| other.eq_ignore_ascii_case(name))
            })
            .map_or(&[], |(_, rules)| rules.as_slice())
    }
}
