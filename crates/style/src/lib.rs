//! Style on the DOM (`dom`; Blink's `core/css` resolver): the sheets
//! applied, selectors matched and the cascade, each element's computed
//! style written on it. What doesn't read the DOM is `style_resolver`'s:
//! the cascade of declaration blocks into a computed style
//! (`style_resolver::cascade`), the presentational hints' parsing, the UA
//! stylesheet.
//!
//! Sources, in cascade order: the UA stylesheet, every `<style>` and
//! `<link>` in tree order, the adopted sheets (`adoptedStyleSheets`, after
//! them; a disabled one kept, its rules none), and each element's `style`
//! attribute. An adopted sheet whose contents or `disabled` changed has
//! the scopes that adopted it collected again (the DOM's list of changed
//! sheets: a sheet doesn't know its scopes). [`StyleEngine::recalc_style`]
//! goes down from the document through the nodes the DOM's style flags say
//! something below them changed. A new or moved element is styled again
//! with its subtree; one whose attributes changed, and what the
//! invalidation sets (`invalidation`) find of its descendants and later
//! siblings; one whose siblings changed if a selector matches by position;
//! children whose parent's style changes what they inherit. The sheets
//! are collected again when the document's flags say they may have
//! changed, and what the rules that came and went may match is styled
//! again (`rule_set_invalidation`). Each element's visit has two halves:
//! what its changes, and what it was handed, style again (`Walk::
//! invalidate`), then its cascade. An element's style is written with the
//! thread's token (`dom::LocalToken`), and read by others with it
//! shared.
//!
//! Names compare as numbers: each rule index (`rule_index`) numbers its
//! selectors' names on this thread as it is made, elements hold theirs.
//!
//! An HTML element's presentational hints are `style_resolver`'s
//! (`hints`). The
//! `<style>`s' and `<link>`s' sheets are the DOM's (`dom::Cx::
//! owner_sheet`): the renderer loads the `<link>`s' (`dom::Cx::
//! set_link_sheet`, and `stylesheet_link_href` for what to).
//!
//! Shadow trees: the walk is the flat tree's (`dom::Cx::flat_tree_children`:
//! a host's shadow root's children, a slot's assigned nodes, which inherit
//! from it; then a host's children assigned to no slot, which inherit from
//! the host and have no box), the slots' assignment made fresh first. A shadow tree's own
//! sheets (`shadow_sheets`) are its elements' (the document's match none
//! of them), its host's by `:host` and what its slots are assigned by
//! `::slotted()`, each tree's rules in the shadow-including tree order of
//! css-scoping's cascade (`Walk::sources`); collected before the walk
//! where the DOM says they may have changed, what the rules that came and
//! went may match styled again (`rule_set_invalidation`: in the tree, its
//! host for `:host`, what its slots are assigned for `::slotted()`).
//!
//! An SVG element's presentation attributes are `style_resolver`'s too
//! (`hints`).
//!
//! Not yet: animations; and what the DOM doesn't have
//! (forms).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod bloom;
mod hints;
mod invalidation;
mod query;
mod rule_index;
mod rule_set_invalidation;
mod selector;
mod shadow_sheets;
mod sheets;

use std::borrow::Cow;
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use computed_style::{ComputedStyle, Content, ContentItem, Display, PseudoId, PseudoStyles};
use css::media::{MediaEnvironment, MediaQueryList};
use css::properties::LonghandId;
use css::rule_set::{RuleData, RuleSet};
use css::selector::PseudoElement;
use css::stylesheet::{Origin, ParseLog, Stylesheet};
use css::{CssPropertyValueSet, StyleSheetContents};
use dom::{
    Cx, LocalToken, NodeId, NodeIdHashSet, NodeKind, NodeRef, QuirksMode, StyleFlags, names,
};
use gfx::SizeF;
use selectors::context::SelectorCaches;
use selectors::matching::{
    ElementSelectorFlags, MatchingContext, MatchingForInvalidation, MatchingMode,
    NeedsSelectorFlags, QuirksMode as SelectorsQuirksMode, matches_selector, selector_may_match,
};
use selectors::parser::AncestorHashes;
use style_resolver::animation_style::{self, Recascade, Restyle, StyleChange, Timeline};
use style_resolver::cascade::{
    BlockSource, Blocks, CascadePriority, Environment, MatchedPropertiesCache, cascade_highlight, push_block,
};

use crate::bloom::StyleBloom;
use crate::invalidation::{Invalidated, InvalidationFeatures, SiblingInvalidation};
use crate::rule_index::{Features, RuleIndex};
use crate::rule_set_invalidation::{
    Found, RuleSetInvalidation, ScopeSheetRules, SheetsChange, changed_rules,
};
use crate::selector::StyleElement;
use crate::shadow_sheets::ShadowScopes;

pub use crate::query::{SelectorQuery, compound_elements_for_testing, full_matches_for_testing};

/// What an element's own style is cascaded from (`StyleEngine::explain`).
pub struct Explained {
    /// The rules it matches, in cascade order: the UA's, then the author's,
    /// each by specificity, then where it comes; the winner last.
    pub rules: Vec<ExplainedRule>,
    /// Its presentational hints (HTML's attributes and SVG's presentation
    /// attributes, one block, as Blink's `attributesStyle`).
    pub presentation: Option<Arc<CssPropertyValueSet>>,
    /// Its `style` attribute's declarations.
    pub inline: Option<Arc<CssPropertyValueSet>>,
}

/// A rule an element matches, and how.
pub struct ExplainedRule {
    pub rule: Arc<css::StyleRule>,
    pub origin: Origin,
    /// The indexes of its selectors the element matches.
    pub matching_selectors: Vec<usize>,
}

/// A document's style (Blink's `StyleEngine` and `StyleResolver`): its
/// sheets, what the cascade shares between elements, and the frame's
/// environment. One a document, on the thread of its DOM.
pub struct StyleEngine {
    /// The thread's serial: what its selectors' numbers are of.
    serial: u32,
    ua: Rc<RuleIndex>,
    author: Option<AuthorSheets>,
    /// The document's large sheets' indexes by their contents' address
    /// (the `Arc`, held, keeps it), whether they apply now or not: a large
    /// sheet's media coming to match again takes its index again (the
    /// engine before cached it in the contents).
    large_indexes: rustc_hash::FxHashMap<usize, (Arc<StyleSheetContents>, Rc<RuleIndex>)>,
    viewport: SizeF,
    device_pixel_ratio: f32,
    system_colors: gfx::SystemColors,
    /// The document's link colors as last styled (`<body link alink>`).
    link_colors: css::values::TextLinkColors,
    /// The preferred style sheet set's name: the title of the first sheet
    /// with one that isn't an alternate `<link>`, the document's once
    /// found (HTML's; Blink's `preferred_stylesheet_set_name_`).
    preferred: Option<String>,
    /// What cascading sets of declaration blocks gave.
    matched_properties: MatchedPropertiesCache,
    /// SVG presentation attributes' declarations by attribute and value
    /// (`hints`): moved into each recalc's walk and back.
    presentation_attributes: style_resolver::PresentationAttributeCache<dom::NameId>,
    /// The images the backgrounds of the elements styled since the host
    /// last took them show, by element (`take_background_image_urls`).
    background_image_urls: Vec<(gfx::NodeId, computed_style::ImageUrl)>,
    /// What the `url()`s of the document's own sheets (its `<style>`s and
    /// its shadow trees') resolve against: its URL (`set_url_context`).
    url_context: Option<css::UrlContext>,
    /// The elements the last recalc styled again (the debug build's, for
    /// the tests of what invalidation styles).
    #[cfg(debug_assertions)]
    restyled: Vec<gfx::NodeId>,
    /// Whether the debug build checks what the sets pass over (on, but
    /// for the tests of what the walk goes down into without it).
    #[cfg(debug_assertions)]
    verify: bool,
    /// Some media query the author index was made with may come out
    /// otherwise: it is made again at the next recalc.
    environment_changed: bool,
    /// The viewport's size changed with every media query coming out the
    /// same: the elements whose styles use viewport units are styled
    /// again, if some ever did (`viewport_used`).
    viewport_changed: bool,
    viewport_used: bool,
    /// The document it styled last (none before the first).
    document: Option<NodeId>,
    /// Whether the page had the system's focus at the last recalc (none
    /// before the first): `:window-inactive` matched as it said.
    page_focused: Option<bool>,
    /// The attributes `attr()` has read of the document's elements, so
    /// far: the DOM notes their changes (`dom::StyleNames`).
    attr_names: rustc_hash::FxHashSet<dom::NameId>,
    /// What the DOM was last given of the names style reads is stale: the
    /// sheets changed, or `attr()` read another.
    names_stale: bool,
    /// The shadow trees' own sheets.
    shadow: ShadowScopes,
    /// What the change of a shadow tree's sheets styles again, by its
    /// root, for the next walk (one whose host the DOM marked: the walk
    /// goes there).
    shadow_changes: rustc_hash::FxHashMap<dom::NodeId, RuleSetInvalidation>,
    /// What the tests read of the work done, and the modes they compare
    /// the optimizations with (`*_for_testing`).
    testing: Testing,
}

/// The engine's counts of its work so far, and the modes that turn an
/// optimization off for a test to compare the styles with: plain numbers
/// and flags, each a statistic (wrapping only miscounts).
#[derive(Default)]
struct Testing {
    /// Selectors matched in full against an element (past the buckets
    /// and the bloom filter).
    matched_selectors: usize,
    /// Tree scopes whose sheets were collected (their tree walked for
    /// them): the document's and shadow roots'.
    collected_tree_scopes: usize,
    /// Rules put in the document's rule indexes, and the indexes made.
    document_rules_indexed: usize,
    document_indexes_made: usize,
    /// Every selector matched against every element, as if there were no
    /// buckets.
    linear_matching: bool,
    /// Every tree scope's sheets collected at each recalc, as if the DOM
    /// had marked them all.
    collect_every_tree_scope: bool,
}

/// The document's `<style>` sheets as last collected, in runs (`SheetGroup`)
/// each with its rule index; the indexes in order with how many rules come
/// before each (as a shadow scope's), and what their selectors ask of
/// siblings and of what is below an element, all of them together.
struct AuthorSheets {
    sheets: Vec<Sheet>,
    groups: Vec<SheetGroup>,
    indexes: Vec<(Rc<RuleIndex>, usize)>,
    features: rule_index::Features,
}

/// How many rules make a document's sheet large: its rules are an index
/// of their own, made again only when it changes; the smaller ones between
/// two large ones are one index. An element looks in each index's buckets,
/// so a page of hundreds of `<style>`s must not be hundreds of indexes
/// (the engine before measured an index at about 30 ns an element for a
/// full recalc, against 0.15 to 0.2 µs a rule to make one again).
const LARGE_SHEET_RULES: usize = 512;

/// A run of `AuthorSheets::sheets` with one rule index: a large sheet
/// (whose index a change of another sheet leaves alone), or the small ones
/// between two large ones (made again when one of them changes).
struct SheetGroup {
    sheets: std::ops::Range<usize>,
    /// A large sheet's media: whether it applied when its index was taken
    /// (`None` for a run of small ones, whose index evaluated theirs).
    matched: Option<bool>,
    /// Its index; `None` for a large sheet that doesn't apply.
    index: Option<Rc<RuleIndex>>,
}

impl SheetGroup {
    /// Whether some media query that decided its rules comes out otherwise
    /// in `environment`.
    fn media_changed(&self, sheets: &[Sheet], environment: &MediaEnvironment) -> bool {
        self.matched
            .is_some_and(|matched| sheets[self.sheets.start].applies(environment) != matched)
            || self
                .index
                .as_ref()
                .is_some_and(|index| index.set.did_media_query_results_change(environment))
    }
}

impl AuthorSheets {
    fn media_changed(&self, environment: &MediaEnvironment) -> bool {
        self.groups
            .iter()
            .any(|group| group.media_changed(&self.sheets, environment))
    }
}

/// A `<style>`'s or `<link>`'s sheet, or an adopted one: its rules and
/// its media.
struct Sheet {
    contents: Arc<StyleSheetContents>,
    media: Arc<MediaQueryList>,
    /// An adopted sheet's number (`dom::Cx::style_sheet_id`).
    adopted: Option<u64>,
    /// A sheet that is disabled: kept (it may be enabled, and its scope
    /// collected again then), its rules none.
    disabled: bool,
}

impl Sheet {
    /// What style keeps of `sheet`, an element's.
    fn of_owner<'gc>(cx: &Cx<'_, 'gc>, sheet: elise::Gc<'gc, dom::CssStyleSheet>) -> Self {
        Self {
            contents: cx.style_sheet_contents(sheet).clone(),
            media: cx.style_sheet_media(sheet).1.clone(),
            adopted: None,
            disabled: cx.style_sheet_disabled(sheet),
        }
    }

    /// Whether its rules apply in `environment`.
    fn applies(&self, environment: &MediaEnvironment) -> bool {
        !self.disabled && self.media.matches(environment)
    }

    /// `scope`'s (a document's or a shadow root's) adopted sheets, after
    /// its own (CSSOM's order).
    fn adopted<'gc>(cx: &Cx<'_, 'gc>, scope: NodeRef<'gc>) -> impl Iterator<Item = Self> {
        cx.adopted_style_sheets(scope)
            .into_iter()
            .map(move |sheet| Self {
                adopted: Some(cx.style_sheet_id(sheet)),
                ..Self::of_owner(cx, sheet)
            })
    }
}

thread_local! {
    /// The UA stylesheet's index, made once on a thread (its names are
    /// numbered on it) and shared by its engines.
    static USER_AGENT: OnceCell<Rc<RuleIndex>> = const { OnceCell::new() };
}

fn user_agent(serial: u32) -> Rc<RuleIndex> {
    USER_AGENT.with(|ua| {
        ua.get_or_init(|| {
            let mut log = ParseLog::default();
            let sheet = Stylesheet::parse(style_resolver::UA_CSS, Origin::UserAgent, &mut log);
            let mut set = RuleSet::default();
            // It has no `@media`: one set for every environment.
            set.add_rules(&sheet.rules, &media_environment(SizeF::default(), 1.0), 0);
            Rc::new(RuleIndex::new(set, serial))
        })
        .clone()
    })
}

fn media_environment(viewport: SizeF, device_pixel_ratio: f32) -> MediaEnvironment {
    MediaEnvironment {
        width: viewport.width,
        height: viewport.height,
        device_pixel_ratio,
    }
}

impl Default for StyleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleEngine {
    pub fn new() -> Self {
        let serial = names::thread_serial();
        Self {
            serial,
            ua: user_agent(serial),
            author: None,
            large_indexes: rustc_hash::FxHashMap::default(),
            viewport: SizeF::default(),
            device_pixel_ratio: 1.0,
            system_colors: gfx::SystemColors::DEFAULT,
            link_colors: css::values::TextLinkColors::DEFAULT,
            preferred: None,
            matched_properties: MatchedPropertiesCache::default(),
            presentation_attributes: style_resolver::PresentationAttributeCache::default(),
            background_image_urls: Vec::new(),
            url_context: None,
            #[cfg(debug_assertions)]
            restyled: Vec::new(),
            #[cfg(debug_assertions)]
            verify: true,
            environment_changed: false,
            viewport_changed: false,
            viewport_used: false,
            page_focused: None,
            document: None,
            attr_names: rustc_hash::FxHashSet::default(),
            names_stale: true,
            shadow: ShadowScopes::default(),
            shadow_changes: rustc_hash::FxHashMap::default(),
            testing: Testing::default(),
        }
    }

    /// What the `url()`s of the document's `<style>` sheets resolve
    /// against: set before the first
    /// recalc, as the document's URL doesn't change.
    pub fn set_url_context(&mut self, context: Option<css::UrlContext>) {
        self.url_context = context;
    }

    /// Whether `set_url_context` was given one.
    pub fn has_url_context(&self) -> bool {
        self.url_context.is_some()
    }

    /// The frame's viewport, in px, and its device pixel ratio: what
    /// viewport units and media queries see. A change styles everything
    /// again at the next recalc.
    pub fn set_viewport(&mut self, viewport: SizeF, device_pixel_ratio: f32) {
        if (viewport, device_pixel_ratio) == (self.viewport, self.device_pixel_ratio) {
            return;
        }
        let new_viewport = viewport != self.viewport;
        let new_ratio = device_pixel_ratio != self.device_pixel_ratio;
        self.viewport = viewport;
        self.device_pixel_ratio = device_pixel_ratio;
        // A media query that comes out otherwise, or another device pixel
        // ratio (an `image-set()` chooses by it; it seldom changes): the
        // sheets collected again, everything styled again (Blink's
        // `MediaQueryAffectingValueChanged`); else the viewport units alone
        // (Blink's `ViewportUnitFlags`).
        let environment = media_environment(viewport, device_pixel_ratio);
        if new_ratio
            || self.ua.set.did_media_query_results_change(&environment)
            || self
                .author
                .as_ref()
                .is_some_and(|author| author.media_changed(&environment))
            || self.shadow.media_changed(&environment)
        {
            self.environment_changed = true;
        } else if new_viewport {
            self.viewport_changed = true;
        }
    }

    /// Turns the debug build's check of what the invalidation sets pass
    /// over off (`false`): for a test of what the walk goes down into of
    /// itself, which the check's walk would hide.
    #[cfg(debug_assertions)]
    pub fn set_verify_for_testing(&mut self, verify: bool) {
        self.verify = verify;
    }

    /// How many rule indexes the shadow trees' sheets have (one each
    /// contents, shared).
    pub fn shadow_indexes_for_testing(&self) -> usize {
        self.shadow.all_indexes().count()
    }

    /// How many shadow roots' sheets the engine keeps.
    pub fn shadow_scopes_for_testing(&self) -> usize {
        self.shadow.scope_count()
    }

    /// How many selectors the engine has matched in full against an
    /// element, so far (those the buckets and the bloom filter left; not
    /// the debug build's checks').
    pub fn matched_selectors_for_testing(&self) -> usize {
        self.testing.matched_selectors
    }

    /// Matches every selector of every sheet against each element, as if
    /// there were no buckets (`linear`): the styles must come out the same.
    pub fn set_linear_matching_for_testing(&mut self, linear: bool) {
        self.testing.linear_matching = linear;
    }

    /// Collects every tree scope's sheets again at each recalc (`every`),
    /// as if the DOM had marked them all: the styles must come out the
    /// same as with only those it marked.
    pub fn set_collect_every_tree_scope_for_testing(&mut self, every: bool) {
        self.testing.collect_every_tree_scope = every;
    }

    /// How many times the engine has collected a tree scope's sheets (the
    /// document's or a shadow root's), so far.
    pub fn collected_tree_scopes_for_testing(&self) -> usize {
        self.testing.collected_tree_scopes
    }

    /// How many rule indexes the document's own sheets and its adopted
    /// ones are cascaded with as last collected: one, or none with nothing
    /// in it.
    pub fn document_sets_for_testing(&self) -> usize {
        self.author.as_ref().map_or(0, |author| {
            author
                .indexes
                .iter()
                .filter(|(index, _)| !index.set.rules().is_empty() || index.set.has_keyframes())
                .count()
        })
    }

    /// How many rules the engine has put in the document's rule indexes,
    /// and how many it has made, so far.
    pub fn document_rules_indexed_for_testing(&self) -> (usize, usize) {
        (
            self.testing.document_rules_indexed,
            self.testing.document_indexes_made,
        )
    }

    /// How many cascades the matched properties cache keeps, and how many
    /// it has given so far.
    pub fn matched_properties_for_testing(&self) -> (usize, usize) {
        (
            self.matched_properties.entries.len(),
            self.matched_properties.hits,
        )
    }

    /// How many attribute values the presentation attribute cache keeps.
    pub fn presentation_attributes_for_testing(&self) -> usize {
        self.presentation_attributes.len_for_testing()
    }

    /// Whether the next recalc styles everything again for what viewport
    /// units and media queries see ([`set_viewport`](Self::set_viewport),
    /// [`set_system_colors`](Self::set_system_colors)).
    pub fn environment_changed_for_testing(&self) -> bool {
        self.environment_changed || (self.viewport_changed && self.viewport_used)
    }

    /// The URLs of the images the backgrounds of the elements styled since
    /// the last call show (an `image-set()`'s at the device pixel ratio),
    /// with each element: for the host to load those it hasn't (Blink's
    /// style loads them as it resolves them:
    /// `ElementStyleResources::LoadPendingResources`), and paint the
    /// elements again once they come (Blink's `ImageChanged`). Not those of
    /// an element that is `display: none` or in such a subtree, which
    /// Chrome doesn't load either: once it is shown, it is styled again.
    pub fn take_background_image_urls(&mut self) -> Vec<(gfx::NodeId, computed_style::ImageUrl)> {
        std::mem::take(&mut self.background_image_urls)
    }

    /// The elements the last recalc styled again, in the order it did
    /// (the debug build's).
    #[cfg(debug_assertions)]
    pub fn restyled_for_testing(&self) -> &[gfx::NodeId] {
        &self.restyled
    }

    pub fn system_colors(&self) -> gfx::SystemColors {
        self.system_colors
    }

    pub fn set_system_colors(&mut self, colors: gfx::SystemColors) {
        if colors != self.system_colors {
            self.system_colors = colors;
            self.environment_changed = true;
        }
    }

    /// Styles again what changed of `document`'s tree since the last
    /// recalc (its style flags say where), each element's style written
    /// with `token`, every flag it went past cleared.
    pub fn recalc_style<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        token: &mut LocalToken,
        document: NodeRef<'gc>,
    ) {
        #[cfg(debug_assertions)]
        self.restyled.clear();
        // An engine is its document's (Blink's `StyleEngine`): one given
        // another document has none of its sheets or styles, and collects
        // and styles everything.
        let id = cx.node_id(document);
        if self.document.replace(id).is_some_and(|was| was != id) {
            self.environment_changed = true;
        }
        // What the walk reads of the flat tree, made fresh first.
        cx.recalc_slot_assignments();
        // Adopted sheets whose contents or `disabled` changed: the scopes
        // that adopted them, as last collected, collected again (the sheet
        // doesn't know them).
        let changed_sheets: Vec<u64> = cx
            .take_changed_style_sheets(document)
            .into_iter()
            .map(|sheet| cx.style_sheet_id(sheet))
            .collect();
        let mut adopted_changed = false;
        if !changed_sheets.is_empty() {
            adopted_changed = self.author.as_ref().is_some_and(|author| {
                author
                    .sheets
                    .iter()
                    .any(|sheet| sheet.adopted.is_some_and(|id| changed_sheets.contains(&id)))
            });
            for root in self.shadow.roots_adopting(&changed_sheets) {
                if let Some(root) = cx.node_by_id(root) {
                    cx.mark_shadow_sheets_changed(root);
                }
            }
        }
        self.collect_shadow_sheets(
            cx,
            document,
            self.environment_changed || self.testing.collect_every_tree_scope,
        );
        let shadow_changes = std::mem::take(&mut self.shadow_changes);
        // For the walk after the first: those it applied.
        let no_shadow_changes = rustc_hash::FxHashMap::default();
        let flags = cx.style_flags(document);
        let mut force = std::mem::take(&mut self.environment_changed);
        let viewport_changed =
            std::mem::take(&mut self.viewport_changed) && self.viewport_used && !force;
        let mut change = SheetsChange::Unchanged;
        if self.author.is_none()
            || flags.contains(StyleFlags::STYLE_SHEETS_CHANGED)
            || adopted_changed
            || force
            || self.testing.collect_every_tree_scope
        {
            change = self.collect_sheets(cx, document);
        }
        if !matches!(change, SheetsChange::Unchanged) || self.names_stale {
            self.give_names(cx, document);
        }
        force |= matches!(change, SheetsChange::Everything);
        cx.clear_style_flags(document);
        // The `<body>`'s `link` or `alink` changed: every style that has
        // `-webkit-link` or `-webkit-activelink` (every link's) changes
        // (Blink marks the links only).
        let link_colors = hints::text_link_colors(cx, document);
        force |= std::mem::replace(&mut self.link_colors, link_colors) != link_colors;
        // The page took or lost the focus: every element `:window-inactive`
        // may match otherwise (Blink's `StyleEngine::PageFocusChanged`
        // restyles where a rule asks for it).
        let focused = cx
            .window_of(document)
            .is_some_and(|window| cx.page_focused(window));
        if self
            .page_focused
            .replace(focused)
            .is_some_and(|was| was != focused)
        {
            force |= [
                Some(&self.ua.features),
                self.author.as_ref().map(|author| &author.features),
                Some(&self.shadow.features),
            ]
            .into_iter()
            .flatten()
            .any(|features| features.window_inactive);
        }
        // What the DOM didn't note since (the debug build's, checked after
        // the walk: one with nothing else to style still walks).
        #[cfg(debug_assertions)]
        let passed_over = cx.take_passed_over_changes(document);
        #[cfg(not(debug_assertions))]
        let passed_over: [(); 0] = [];
        if !force
            && !flags.contains(StyleFlags::CHILD_NEEDS_STYLE_RECALC)
            && !matches!(change, SheetsChange::Rules(_))
            && passed_over.is_empty()
            && !viewport_changed
        {
            return;
        }
        // What is below an element decides what `:has()` matches of it: a
        // change anywhere styles everything again (Blink's `:has()`
        // invalidation is finer).
        force |= [
            Some(&self.ua.features),
            self.author.as_ref().map(|author| &author.features),
            Some(&self.shadow.features),
        ]
        .into_iter()
        .flatten()
        .any(|features| features.has);
        // The document's rules changed: what each element of its tree has
        // of their features styles it again, or its subtree.
        let rules = match &change {
            SheetsChange::Rules(rules) if !force => Some(rules),
            _ => None,
        };
        let initial = Arc::new(ComputedStyle::initial());
        // What `rem` is: the initial font size for the root element
        // itself, then the root's.
        let root_font_size = cx
            .first_child(document)
            .into_iter()
            .chain(std::iter::successors(cx.first_child(document), |node| {
                cx.next_sibling(*node)
            }))
            .find_map(|node| cx.computed_style(node, token).map(|style| style.font.size))
            .unwrap_or(initial.font.size);
        let cache = RefCell::new(std::mem::take(&mut self.matched_properties));
        let mut walk = Walk {
            cx,
            serial: self.serial,
            ua: &self.ua,
            author: self
                .author
                .as_ref()
                .map_or(&[][..], |author| &author.indexes[..]),
            author_features: self.author.as_ref().map(|author| &author.features),
            shadow: &self.shadow,
            shadow_changes: &shadow_changes,
            rules,
            env: Environment {
                viewport: self.viewport,
                system_colors: self.system_colors,
                link_colors,
            },
            quirks: cx.quirks_mode(document) == QuirksMode::Quirks,
            cache: &cache,
            candidates: Vec::new(),
            bloom: StyleBloom::new(),
            caches: SelectorCaches::default(),
            pseudo_caches: SelectorCaches::default(),
            root_font_size,
            initial_font_size: initial.font.size,
            attr_names: Vec::new(),
            device_pixel_ratio: self.device_pixel_ratio,
            background_image_urls: Vec::new(),
            url_context: self.url_context.clone(),
            presentation: std::mem::take(&mut self.presentation_attributes),
            now: cx.animation_time(document),
            met: Vec::new(),
            linear: self.testing.linear_matching,
            matched: 0,
            selector_flags: NeedsSelectorFlags::Yes,
            matched_selectors: None,
            #[cfg(debug_assertions)]
            restyled: Vec::new(),
            #[cfg(debug_assertions)]
            verify: self.verify,
            #[cfg(debug_assertions)]
            passed_over: Vec::new(),
            viewport_changed,
            viewport_used: false,
        };
        let mode = if force { Mode::Subtree } else { Mode::Clean };
        let parents = Parents {
            style: &initial,
            layout: &initial,
            rendered: true,
            moved: false,
            changed: false,
        };
        let down = |mode| Down {
            mode,
            below: None,
            verify: false,
            look: false,
            chosen: None,
        };
        walk.children(document, parents, down(mode), token);
        // What the walk marked behind itself (an assigned node's siblings
        // that an earlier slot shows): a walk down the marked paths alone.
        if cx
            .style_flags(document)
            .contains(StyleFlags::CHILD_NEEDS_STYLE_RECALC)
        {
            cx.clear_style_flags(document);
            walk.rules = None;
            walk.shadow_changes = &no_shadow_changes;
            walk.children(document, parents, down(Mode::Clean), token);
        }
        self.viewport_used |= walk.viewport_used;
        for name in std::mem::take(&mut walk.attr_names) {
            self.names_stale |= self.attr_names.insert(name);
        }
        self.background_image_urls
            .append(&mut walk.background_image_urls);
        self.presentation_attributes = std::mem::take(&mut walk.presentation);
        cx.forget_removed(&walk.met);
        // Before the debug build's checks below, which match again.
        self.testing.matched_selectors = self.testing.matched_selectors.wrapping_add(walk.matched);
        #[cfg(debug_assertions)]
        check_passed_over(
            &mut walk,
            document,
            passed_over,
            &initial,
            !matches!(change, SheetsChange::Unchanged),
            token,
        );
        #[cfg(debug_assertions)]
        {
            let restyled: rustc_hash::FxHashSet<gfx::NodeId> =
                walk.restyled.iter().copied().collect();
            let quirks = if walk.quirks {
                SelectorsQuirksMode::Quirks
            } else {
                SelectorsQuirksMode::NoQuirks
            };
            for &(id, root) in walk
                .passed_over
                .iter()
                .filter(|(id, _)| !restyled.contains(id))
            {
                let node = cx.node_by_id(id).expect("an element the walk passed");
                // The document's rules, or a shadow tree's (its host
                // theirs).
                let (changed, host) = match root {
                    None => (rules, None),
                    Some(root) => (
                        shadow_changes.get(&root),
                        cx.node_by_id(root).and_then(|root| cx.shadow_host(root)),
                    ),
                };
                if let Some(changed) = changed {
                    changed.assert_unmatched(
                        &StyleElement {
                            cx,
                            node,
                            serial: self.serial,
                            shadow_pseudo: false,
                            selection: false,
                        },
                        quirks,
                        host.map(|host| selectors::OpaqueElement::new(host.into_ref())),
                    );
                }
            }
            self.restyled = std::mem::take(&mut walk.restyled);
        }
        self.matched_properties = cache.into_inner();
        if self.names_stale {
            self.give_names(cx, document);
        }
    }

    /// What `element`'s own style is cascaded from, for DevTools' `CSS`
    /// agent (Blink's `InspectorCSSAgent::getMatchedStylesForNode`, which
    /// collects with `ElementRuleCollector`'s `kCollectingCSSRules`): the
    /// rules it matches, in cascade order, the winner last; its
    /// presentational hints; its `style` attribute's declarations. As the
    /// last `recalc_style` matched them (its sheets): its rules matched
    /// again as the walk matches them, with nothing kept or changed (no
    /// selector flags set, caches of its own). None for a node that isn't
    /// an element of this engine's document's.
    pub fn explain<'gc>(&mut self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<Explained> {
        let document = cx.document_of(element);
        if cx.kind(element) != NodeKind::Element
            || self.document != Some(cx.node_id(document))
            || !cx.is_connected(element)
        {
            return None;
        }
        let initial = ComputedStyle::initial();
        let cache = RefCell::new(MatchedPropertiesCache::default());
        let shadow_changes = rustc_hash::FxHashMap::default();
        // What only cascading reads (the environment, `rem`, animations)
        // is left as it comes: the blocks are all it asks for.
        let mut walk = Walk {
            cx,
            serial: self.serial,
            ua: &self.ua,
            author: self
                .author
                .as_ref()
                .map_or(&[][..], |author| &author.indexes[..]),
            author_features: self.author.as_ref().map(|author| &author.features),
            shadow: &self.shadow,
            shadow_changes: &shadow_changes,
            rules: None,
            env: Environment {
                viewport: self.viewport,
                system_colors: self.system_colors,
                link_colors: self.link_colors,
            },
            quirks: cx.quirks_mode(document) == QuirksMode::Quirks,
            cache: &cache,
            candidates: Vec::new(),
            bloom: StyleBloom::new(),
            caches: SelectorCaches::default(),
            pseudo_caches: SelectorCaches::default(),
            root_font_size: initial.font.size,
            initial_font_size: initial.font.size,
            attr_names: Vec::new(),
            device_pixel_ratio: self.device_pixel_ratio,
            background_image_urls: Vec::new(),
            url_context: self.url_context.clone(),
            presentation: std::mem::take(&mut self.presentation_attributes),
            now: 0.0,
            met: Vec::new(),
            linear: self.testing.linear_matching,
            matched: 0,
            selector_flags: NeedsSelectorFlags::No,
            matched_selectors: Some(Vec::new()),
            #[cfg(debug_assertions)]
            restyled: Vec::new(),
            #[cfg(debug_assertions)]
            verify: false,
            #[cfg(debug_assertions)]
            passed_over: Vec::new(),
            viewport_changed: false,
            viewport_used: false,
        };
        // The bloom filter has its flat-tree ancestors, as in the walk.
        let ancestors: Vec<NodeRef<'gc>> =
            std::iter::successors(cx.flat_tree_parent(element), |&node| {
                cx.flat_tree_parent(node)
            })
            .filter(|&node| cx.kind(node) == NodeKind::Element)
            .collect();
        for &ancestor in ancestors.iter().rev() {
            walk.bloom.push(cx, ancestor);
        }
        let inline = cx.inline_style(element);
        let presentation = hints::declarations(
            cx,
            element,
            walk.url_context.as_ref(),
            &mut walk.presentation,
        );
        let [own, ..] = walk.blocks(element, inline.as_ref(), presentation.as_ref());
        // Each block once, at its normal declarations' place (its
        // `!important` ones' is another).
        let mut rules: Vec<(CascadePriority, Arc<css::StyleRule>)> = own
            .iter()
            .filter(|(_, _, important, _)| !important)
            .filter_map(|(priority, _, _, source)| match source {
                BlockSource::Rule(rule) => Some((*priority, Arc::clone(rule))),
                BlockSource::Hints(_) | BlockSource::Own => None,
            })
            .collect();
        rules.sort_by_key(|(priority, _)| *priority);
        drop(own);
        let matched = walk.matched_selectors.take().unwrap_or_default();
        self.presentation_attributes = std::mem::take(&mut walk.presentation);
        let rules = rules
            .into_iter()
            .map(|(priority, rule)| {
                let origin = if priority.level == CascadePriority::level(Origin::UserAgent, false) {
                    Origin::UserAgent
                } else {
                    Origin::Author
                };
                let mut matching_selectors: Vec<usize> = matched
                    .iter()
                    .filter(|(matched, _)| Arc::ptr_eq(matched, &rule))
                    .map(|&(_, index)| index)
                    .collect();
                matching_selectors.sort_unstable();
                matching_selectors.dedup();
                ExplainedRule {
                    rule,
                    origin,
                    matching_selectors,
                }
            })
            .collect();
        Some(Explained {
            rules,
            presentation: presentation.map(|(hints, _)| hints),
            inline,
        })
    }

    /// Collects again the sheets of the shadow roots the DOM says may have
    /// changed (and of every one known, `all`: a `<link>`'s sheet came, or
    /// what media queries see changed): what the change of each whose
    /// changed styles again, for the walk.
    fn collect_shadow_sheets<'gc>(&mut self, cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, all: bool) {
        let mut roots = cx.take_sheet_changed_roots(document);
        if all {
            roots.extend(
                self.shadow
                    .roots()
                    .into_iter()
                    .filter_map(|id| cx.node_by_id(id)),
            );
        }
        if roots.is_empty() {
            return;
        }
        let environment = media_environment(self.viewport, self.device_pixel_ratio);
        for root in roots {
            if !cx.is_connected(root) {
                continue;
            }
            self.testing.collected_tree_scopes = self.testing.collected_tree_scopes.wrapping_add(1);
            let Some(change) = self.shadow.collect(cx, root, &environment, self.serial) else {
                continue;
            };
            self.names_stale = true;
            let Some(host) = cx.shadow_host(root) else {
                continue;
            };
            // The DOM marked the host (the walk goes there and below it):
            // what the change finds; else (a `<link>`'s sheet came, the
            // environment changed) the host's subtree.
            if cx
                .style_flags(host)
                .contains(StyleFlags::STYLE_SHEETS_CHANGED)
            {
                self.shadow_changes.insert(cx.node_id(root), change);
            } else {
                cx.set_needs_style_recalc(host);
            }
        }
    }

    /// Gives the DOM the names style reads of `document`'s elements: those
    /// the UA's and the author's selectors use, the attributes the hints
    /// are of, `style`, `lang` and `dir` (what the ancestors' decide below
    /// them), `<body>`'s `link` and `alink`, and what `attr()` has read.
    fn give_names<'gc>(&mut self, cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) {
        self.names_stale = false;
        let mut given = self.ua.names.clone();
        let author = self
            .author
            .iter()
            .flat_map(|author| author.indexes.iter().map(|(index, _)| &**index));
        for index in author.chain(self.shadow.all_indexes()) {
            let names = &index.names;
            given.attributes.extend(names.attributes.iter().copied());
            given.classes.extend(names.classes.iter().copied());
            given.ids.extend(names.ids.iter().cloned());
            given.states |= names.states;
            given.everything |= names.everything;
        }
        given.attributes.extend(hints::attribute_names());
        given
            .attributes
            .extend(style_resolver::svg_hint_attribute_names().map(names::intern));
        given.attributes.extend([
            names::local::STYLE,
            names::local::LANG,
            names::local::DIR,
            names::local::LINK,
            names::local::ALINK,
        ]);
        given.attributes.extend(self.attr_names.iter().copied());
        cx.set_style_names(document, given);
    }

    /// Collects `document`'s `<style>` and `<link>` sheets again (the
    /// DOM's, made when first asked for), in tree order: what styles again
    /// that they differ from the last collected, the rules of those that
    /// came and went (or everything, if the environment their media queries
    /// see changed or those kept are in another order).
    fn collect_sheets<'gc>(&mut self, cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> SheetsChange {
        self.testing.collected_tree_scopes = self.testing.collected_tree_scopes.wrapping_add(1);
        let mut owners = Vec::new();
        let mut walk = std::iter::successors(cx.first_child(document), |node| {
            next_in_tree_order(cx, *node, document)
        });
        for node in &mut walk {
            if cx.owns_sheet(node) {
                owners.push(node);
            }
        }
        let old = self.author.take();
        let mut sheets = Vec::with_capacity(owners.len());
        for owner in owners {
            if !sheets::of_preferred_set(cx, owner, &mut self.preferred) {
                continue;
            }
            let Some(sheet) = cx.owner_sheet(owner) else {
                continue;
            };
            sheets.push(Sheet::of_owner(cx, sheet));
        }
        sheets.extend(Sheet::adopted(cx, document));
        let environment = media_environment(self.viewport, self.device_pixel_ratio);
        let same = old.as_ref().is_some_and(|old| {
            old.sheets.len() == sheets.len()
                && old.sheets.iter().zip(&sheets).all(|(a, b)| {
                    Arc::ptr_eq(&a.contents, &b.contents)
                        && Arc::ptr_eq(&a.media, &b.media)
                        && a.disabled == b.disabled
                })
                && !old.media_changed(&environment)
        });
        if same {
            self.author = old;
            return SheetsChange::Unchanged;
        }
        // What the old sheets' rules gave isn't asked for again: let go of
        // them.
        self.matched_properties.clear();
        let change = match &old {
            Some(old) if !old.media_changed(&environment) => {
                sheets_change(&old.sheets, &sheets, &environment)
            }
            _ => SheetsChange::Everything,
        };
        let groups = self.group_sheets(&sheets, old.as_ref(), &environment);
        let mut indexes = Vec::new();
        let mut features = rule_index::Features::default();
        let mut first = 0usize;
        for index in groups.iter().filter_map(|group| group.index.as_ref()) {
            features.add_flags(&index.features);
            indexes.push((index.clone(), first));
            first = first.saturating_add(index.set.rules().len());
        }
        self.author = Some(AuthorSheets {
            sheets,
            groups,
            indexes,
            features,
        });
        change
    }

    /// `sheets` in runs (`SheetGroup`): each large one alone, its index the
    /// old one of its contents if its media came out the same; the small
    /// ones between them together, with the index of the same run of
    /// `old`'s if their media come out the same, or a new one.
    fn group_sheets(
        &mut self,
        sheets: &[Sheet],
        old: Option<&AuthorSheets>,
        environment: &MediaEnvironment,
    ) -> Vec<SheetGroup> {
        let large = |sheet: &Sheet| sheet.contents.rules().len() >= LARGE_SHEET_RULES;
        let old_groups = old.map_or(&[][..], |old| &old.groups[..]);
        let old_sheets = old.map_or(&[][..], |old| &old.sheets[..]);
        let same = |a: &Sheet, b: &Sheet| {
            Arc::ptr_eq(&a.contents, &b.contents)
                && Arc::ptr_eq(&a.media, &b.media)
                && a.disabled == b.disabled
        };
        let mut groups = Vec::new();
        let mut start = 0;
        while let Some(sheet) = sheets.get(start) {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "start < sheets.len(), and so is any index found after it"
            )]
            let end = if large(sheet) {
                start + 1
            } else {
                sheets[start..]
                    .iter()
                    .position(large)
                    .map_or(sheets.len(), |found| start + found)
            };
            let run = &sheets[start..end];
            // The old group of the same sheets, if its media came out the
            // same: its index taken again; a large sheet's that applies,
            // the one made for its contents before if theirs did.
            let known = old_groups
                .iter()
                .find(|group| {
                    let had = &old_sheets[group.sheets.clone()];
                    had.len() == run.len()
                        && had.iter().zip(run).all(|(a, b)| same(a, b))
                        && !group.media_changed(old_sheets, environment)
                })
                .map(|group| group.index.clone())
                .or_else(|| {
                    let (_, index) = self
                        .large_indexes
                        .get(&Arc::as_ptr(&sheet.contents).addr())
                        .filter(|_| large(sheet) && sheet.applies(environment))?;
                    (!index.set.did_media_query_results_change(environment))
                        .then(|| Some(index.clone()))
                });
            let index = match known {
                Some(index) => index,
                // A large sheet that doesn't apply has none.
                None if large(sheet) && !sheet.applies(environment) => None,
                None => {
                    let mut set = RuleSet::default();
                    for sheet in run {
                        // A sheet's own `media` (its element's), remembered
                        // with the rest for when the environment changes.
                        if !sheet.disabled && set.match_media(&sheet.media, environment) {
                            set.add_rules(sheet.contents.rules(), environment, 0);
                        }
                    }
                    self.testing.document_rules_indexed = self
                        .testing
                        .document_rules_indexed
                        .wrapping_add(set.rules().len());
                    self.testing.document_indexes_made =
                        self.testing.document_indexes_made.wrapping_add(1);
                    let index = Rc::new(RuleIndex::new(set, self.serial));
                    if large(sheet) {
                        self.large_indexes.insert(
                            Arc::as_ptr(&sheet.contents).addr(),
                            (sheet.contents.clone(), index.clone()),
                        );
                    }
                    Some(index)
                }
            };
            groups.push(SheetGroup {
                sheets: start..end,
                matched: large(sheet).then(|| sheet.applies(environment)),
                index,
            });
            start = end;
        }
        // Those of contents the document has no more let go of.
        self.large_indexes.retain(|key, _| {
            sheets
                .iter()
                .any(|sheet| Arc::as_ptr(&sheet.contents).addr() == *key)
        });
        groups
    }
}

/// What a change of the document's sheets from `old` to `new` styles
/// again: the rules of the sheets that came and went (Blink's
/// `CompareActiveStyleSheets`), those whose media match (the others match
/// nothing either way). Cascaded in another order, the ones kept may give
/// an element another value: everything.
fn sheets_change(old: &[Sheet], new: &[Sheet], environment: &MediaEnvironment) -> SheetsChange {
    let mut taken = vec![false; old.len()];
    let mut kept_order = Vec::new();
    let kept: Vec<bool> = new
        .iter()
        .map(|sheet| {
            let found = (0..old.len()).find(|&at| {
                !taken[at]
                    && Arc::ptr_eq(&old[at].contents, &sheet.contents)
                    && Arc::ptr_eq(&old[at].media, &sheet.media)
                    && old[at].disabled == sheet.disabled
            });
            if let Some(at) = found {
                taken[at] = true;
                kept_order.push(at);
            }
            found.is_some()
        })
        .collect();
    if !kept_order.is_sorted() {
        return SheetsChange::Everything;
    }
    fn rules<'a>(
        sheets: &'a [Sheet],
        kept: &[bool],
        environment: &MediaEnvironment,
    ) -> Vec<ScopeSheetRules<'a>> {
        sheets
            .iter()
            .zip(kept)
            .map(|(sheet, &kept)| ScopeSheetRules {
                rules: sheet.applies(environment).then(|| sheet.contents.rules()),
                kept,
            })
            .collect()
    }
    changed_rules(
        &rules(old, &taken, environment),
        &rules(new, &kept, environment),
    )
    .and_then(|rules| RuleSetInvalidation::of(rules, environment))
    .map_or(SheetsChange::Everything, SheetsChange::Rules)
}

/// The debug build's check of the changes of `document`'s elements the
/// DOM didn't note (`dom::StyleNames` passed over them): none is one
/// a set finds others by (unless the sheets changed since: what the new
/// rules find the change of rules styled again), and each element, still
/// styled, cascades as its style is (with its flat-tree ancestors in the
/// bloom filter, as the walk has them).
#[cfg(debug_assertions)]
fn check_passed_over<'gc>(
    walk: &mut Walk<'_, '_, 'gc>,
    document: NodeRef<'gc>,
    passed_over: Vec<(gfx::NodeId, dom::AttributeChanges)>,
    initial: &Arc<ComputedStyle>,
    sheets_changed: bool,
    token: &mut LocalToken,
) {
    let cx = walk.cx;
    for (id, changes) in passed_over {
        let Some(element) = cx.node_by_id(id) else {
            continue;
        };
        for index in walk.all_indexes().filter(|_| !sheets_changed) {
            let mut invalidated = Invalidated::default();
            index.sets.invalidated(&changes, &mut invalidated);
            assert!(
                !invalidated.subtree
                    && invalidated.below.is_empty()
                    && invalidated.siblings.is_empty()
                    && !invalidated.later
                    && !index.features.touched_by(&changes)
                    && !hints::for_descendants(cx, element, &changes),
                "a change the DOM didn't note styles others again: {changes:?}"
            );
        }
        // Its flat-tree ancestors, the nearest first, if they reach the
        // document (it is shown).
        let mut ancestors = Vec::new();
        let mut at = cx.flat_tree_parent(element);
        let shown = loop {
            match at {
                Some(node) if cx.kind(node) == NodeKind::Document => {
                    break ::elise::Gc::ptr_eq(node, document);
                }
                Some(node) => {
                    ancestors.push(node);
                    at = cx.flat_tree_parent(node);
                }
                None => break false,
            }
        };
        let Some(style) = cx.computed_style(element, token).cloned() else {
            continue;
        };
        if !shown {
            continue;
        }
        let styles: Vec<Arc<ComputedStyle>> = ancestors
            .iter()
            .filter_map(|&ancestor| cx.computed_style(ancestor, token).cloned())
            .collect();
        let parent = styles.first().unwrap_or(initial).clone();
        let layout = styles
            .iter()
            .find(|style| style.display != Display::Contents)
            .unwrap_or(initial)
            .clone();
        for &ancestor in ancestors.iter().rev() {
            walk.bloom.push(cx, ancestor);
        }
        walk.verify(
            element,
            Parents {
                style: &parent,
                layout: &layout,
                rendered: true,
                moved: false,
                changed: false,
            },
            &style,
        );
        for _ in &ancestors {
            walk.bloom.pop();
        }
    }
}

/// The node after `node` in tree order, within `root`'s subtree.
fn next_in_tree_order<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    root: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    if let Some(child) = cx.first_child(node) {
        return Some(child);
    }
    let mut at = node;
    loop {
        if ::elise::Gc::ptr_eq(at, root) {
            return None;
        }
        if let Some(next) = cx.next_sibling(at) {
            return Some(next);
        }
        at = cx.parent(at)?;
    }
}

/// What an element's visit hands its later siblings: whether it came or
/// moved (`positional`), its sibling sets, whether every later sibling is
/// to be styled again (`later`: what the sets don't say), and whether the
/// debug build checks them (`verify_later`).
struct Visited<'s> {
    positional: bool,
    siblings: Vec<&'s SiblingInvalidation>,
    later: bool,
    verify_later: bool,
}

/// What a walk hands an element down from its ancestors: what it styles
/// again of it whatever its flags (`mode`; from its earlier siblings' too),
/// the descendant sets it is under (`below`), whether the debug build
/// cascades it again to check what the sets passed over (`verify`), and
/// whether it is below a host whose shadow tree's rules changed (`look`:
/// each element looks its features up); for a parent's children, those a
/// change among them styles again by position (`chosen`).
#[derive(Clone, Copy)]
struct Down<'p> {
    mode: Mode,
    below: Option<&'p InvalidationFeatures>,
    verify: bool,
    look: bool,
    chosen: Option<&'p NodeIdHashSet>,
}

/// An element's parent's style, and that of what lays it out (its nearest
/// ancestor that isn't `display: contents`); and what its animations need
/// of its flat-tree ancestors.
#[derive(Clone, Copy)]
struct Parents<'p> {
    style: &'p Arc<ComputedStyle>,
    layout: &'p Arc<ComputedStyle>,
    /// None is `display: none`: else its animations are canceled.
    rendered: bool,
    /// One was removed from its tree since it was styled and is back: its
    /// style is from before (`Cx::removed_since_styled`).
    moved: bool,
    /// The parent's style changed: its children with animations are
    /// styled again (their keyframes may take its values).
    changed: bool,
}

/// An element's style change event as its restyle has it: its style and
/// its pseudo-elements' as of the last one, whether its ancestors are
/// rendered, whether it moved (`Parents`).
struct StyleChangeOf<'o> {
    old: Option<&'o Arc<ComputedStyle>>,
    old_pseudo: Option<&'o PseudoStyles>,
    rendered: bool,
    moved: bool,
}

/// One target's (an element's, or a pseudo-element's of it) style change
/// event: its style as of the last one, whether it is rendered (it and its
/// ancestors not `display: none`, a pseudo-element with a box), whether it
/// moved.
struct TargetChange<'o> {
    pseudo: Option<PseudoId>,
    old: Option<&'o Arc<ComputedStyle>>,
    rendered: bool,
    moved: bool,
}

/// What a walk of a parent's children carries from one to the next: every
/// later sibling styled again (`later`: a `~`, or what the sets don't
/// say), the sibling sets of earlier children's changes with how many more
/// elements each reaches (`None`: all), and whether the debug build checks
/// the later siblings (`verify_later`).
struct Siblings<'s> {
    later: Mode,
    pending: Vec<(&'s SiblingInvalidation, Option<u8>)>,
    verify_later: bool,
}

impl Default for Siblings<'_> {
    fn default() -> Self {
        Self {
            later: Mode::Clean,
            pending: Vec::new(),
            verify_later: false,
        }
    }
}

impl<'s> Siblings<'s> {
    /// What the earlier children's changes make of `node`, the next: what
    /// is styled again of it (at least `mode`), and the descendant sets
    /// for its children.
    fn next<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
        mode: Mode,
    ) -> (Mode, Option<InvalidationFeatures>) {
        let mut own = mode.max(self.later);
        let mut below: Option<InvalidationFeatures> = None;
        self.pending.retain_mut(|(set, left)| {
            if *left == Some(0) {
                return false;
            }
            if set.siblings.matches(cx, node) {
                if set.restyle {
                    own = own.max(Mode::Element);
                }
                if !set.descendants.is_empty() {
                    below.get_or_insert_default().add(&set.descendants);
                }
            }
            if let Some(left) = left {
                *left = left.saturating_sub(1);
            }
            true
        });
        (own, below)
    }
}

/// What an element's changes, and what it was handed, style again (the
/// invalidation half of its visit, before its cascade): it (`own`), its
/// subtree, each of its children (`structural`: they match by position,
/// and they changed), the elements below it that have its own descendant
/// sets' features (`below`); what it hands its later siblings (whether it
/// came or `moved`, its sibling sets, `later`); and what the debug build
/// checks below it and after it.
struct Invalidation<'s> {
    own: bool,
    /// It hosts a shadow tree whose rules changed: what is below it looks
    /// its features up.
    hosts_change: bool,
    subtree: bool,
    structural: bool,
    below: InvalidationFeatures,
    moved: bool,
    siblings: Vec<&'s SiblingInvalidation>,
    later: bool,
    verify_below: bool,
    verify_later: bool,
}

/// What a walk styles again of an element, whatever its own flags:
/// nothing, it alone (what it inherits changed, or a sibling's change may
/// change what matches it), or it and its subtree.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Clean,
    Element,
    Subtree,
}

/// What a pseudo-element's or the element's matched rules are for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Element,
    Pseudo(PseudoId),
    /// Its `::selection`'s (`cascade_highlight`).
    Selection,
}

/// Which of an index's rules an element matches: those of its own tree,
/// the `:host` ones (of its shadow tree, for a host), the `::slotted()`
/// ones (of a slot's tree, for an element assigned to it), the parts'
/// ones (of its host's tree, for an element of a UA shadow tree with a
/// shadow pseudo id, matched as the host's pseudo-element).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Tree,
    Host,
    Slotted,
    Part,
}

/// An index an element matches (Blink's `ElementRuleCollector` collects
/// from each `ScopedStyleResolver`): which of its rules, the host of the
/// shadow tree they are for (none for the UA's and the document's), where
/// that tree is in the shadow-including tree order among the element's
/// (css-scoping's "Shadow Trees and the Cascade": its own first, 0, then
/// the trees of the slots it is assigned through, then its own shadow
/// tree), and its first rule's order after the UA's.
struct Source<'s, 'gc> {
    index: &'s RuleIndex,
    which: Which,
    host: Option<NodeRef<'gc>>,
    tree_order: u32,
    first: usize,
}

/// The form control `element` is to its native look (`style_resolver`'s
/// `theme::Control`): an HTML control by its name (an input by its type, a
/// select by whether it is a menu list), or a part of a control's UA shadow
/// tree by its shadow pseudo id.
fn form_control<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
) -> Option<style_resolver::FormControl> {
    use dom::names::local;
    if let Some(id) = cx.shadow_pseudo_id(element) {
        return style_resolver::FormControl::for_part(&id.to_str());
    }
    let name = cx.qualified_name(element)?;
    if name.namespace() != dom::names::HTML_NAMESPACE {
        return None;
    }
    let local = match name.local() {
        local::INPUT => {
            return style_resolver::FormControl::for_element(
                "input",
                cx.input_kind(element)?.keyword(),
                false,
            );
        }
        local::SELECT => {
            return style_resolver::FormControl::for_element(
                "select",
                "",
                cx.select_uses_menu_list(element),
            );
        }
        local::BUTTON => "button",
        local::TEXTAREA => "textarea",
        local::METER => "meter",
        local::PROGRESS => "progress",
        _ => return None,
    };
    style_resolver::FormControl::for_element(local, "", false)
}

fn target_index(target: Target) -> usize {
    match target {
        Target::Element => 0,
        Target::Pseudo(PseudoId::Before) => 1,
        Target::Pseudo(PseudoId::After) => 2,
        Target::Pseudo(PseudoId::Marker) => 3,
        Target::Selection => 4,
    }
}

const TARGETS: [Target; 5] = [
    Target::Element,
    Target::Pseudo(PseudoId::Before),
    Target::Pseudo(PseudoId::After),
    Target::Pseudo(PseudoId::Marker),
    Target::Selection,
];

/// One recalc's walk down the tree: what every element's cascade shares.
struct Walk<'s, 'a, 'gc> {
    cx: &'a Cx<'a, 'gc>,
    serial: u32,
    ua: &'s RuleIndex,
    /// The document's indexes, with how many rules come before each.
    author: &'s [(Rc<RuleIndex>, usize)],
    author_features: Option<&'s rule_index::Features>,
    shadow: &'s ShadowScopes,
    /// What the change of a shadow tree's sheets styles again, by root.
    shadow_changes: &'s rustc_hash::FxHashMap<dom::NodeId, RuleSetInvalidation>,
    /// What finds the elements the document's changed rules may match.
    rules: Option<&'s RuleSetInvalidation>,
    env: Environment,
    quirks: bool,
    cache: &'s RefCell<MatchedPropertiesCache>,
    /// The candidate selectors' buffer, kept from one element to the next.
    candidates: Vec<RuleData>,
    bloom: StyleBloom,
    caches: SelectorCaches,
    pseudo_caches: SelectorCaches,
    /// What `rem` is: the root's font size, once styled (this walk's or
    /// the last one's).
    root_font_size: f32,
    /// What `rem` is for the root itself: the initial font size.
    initial_font_size: f32,
    /// The attributes `attr()` read of the elements it styled (what the
    /// DOM notes changes of from now on: `StyleEngine::attr_names`).
    attr_names: Vec<dom::NameId>,
    /// The device pixel ratio an `image-set()` picks its image by, and the
    /// images the backgrounds it styled show, by element
    /// (`StyleEngine::take_background_image_urls`).
    device_pixel_ratio: f32,
    background_image_urls: Vec<(gfx::NodeId, computed_style::ImageUrl)>,
    /// What the `url()` of a `background` attribute's hint resolves
    /// against (`StyleEngine::set_url_context`).
    url_context: Option<css::UrlContext>,
    /// `StyleEngine::presentation_attributes`, for the walk.
    presentation: style_resolver::PresentationAttributeCache<dom::NameId>,
    /// The document timeline's time, and the roots of removed subtrees
    /// back in the document the walk came to (`Cx::forget_removed` once
    /// they are styled).
    now: f64,
    met: Vec<gfx::NodeId>,
    /// `StyleEngine::set_linear_matching_for_testing`'s mode, and the
    /// selectors matched in full (`matched_selectors_for_testing`).
    linear: bool,
    matched: usize,
    /// Whether matching sets the elements' selector flags (what a change of
    /// them restyles): not for `StyleEngine::explain`, which changes
    /// nothing.
    selector_flags: NeedsSelectorFlags,
    /// For `StyleEngine::explain`: each selector the element matched as
    /// itself, by its rule and its index in the rule's list.
    matched_selectors: Option<Vec<(&'s Arc<css::StyleRule>, usize)>>,
    #[cfg(debug_assertions)]
    restyled: Vec<gfx::NodeId>,
    #[cfg(debug_assertions)]
    verify: bool,
    /// Only the viewport's size changed: every element whose style (or a
    /// pseudo-element's) uses viewport units is styled again, the walk
    /// going down everywhere.
    viewport_changed: bool,
    /// A style it made uses viewport units.
    viewport_used: bool,
    /// The elements a change of their tree's rules passed over, with their
    /// shadow root (none: the document's), for the debug build to check
    /// once the walks are done (a sibling's in another slot may mark one
    /// after it was passed).
    #[cfg(debug_assertions)]
    passed_over: Vec<(gfx::NodeId, Option<gfx::NodeId>)>,
}

impl<'s, 'a, 'gc> Walk<'s, 'a, 'gc> {
    /// `parent`'s children: each element visited with what `down` hands
    /// it and what the earlier children's changes do to it (`Siblings`);
    /// every other node's flags cleared. A child that came or moved styles
    /// again every later sibling with `~` among the rules (a run of `+` is
    /// the parent's selector flags': its children changed, all of them).
    fn children(
        &mut self,
        parent: NodeRef<'gc>,
        parents: Parents<'_>,
        down: Down<'_>,
        token: &mut LocalToken,
    ) {
        let cx = self.cx;
        // A host's children assigned to no slot have no flat-tree parent:
        // styled as its children all the same, after the flat-tree ones,
        // inheriting from it, with no box (the engine before did; Blink
        // styles one as `getComputedStyle` asks, `EnsureComputedStyle`,
        // alike). One loop: the walk is as deep as the tree, a frame a
        // level.
        let unassigned = cx
            .shadow_root(parent)
            .is_some()
            .then(|| std::iter::successors(cx.first_child(parent), |&node| cx.next_sibling(node)))
            .into_iter()
            .flatten()
            .filter(|&node| cx.assigned_slot(node).is_none())
            .map(|node| (node, true));
        // A host's shadow root's children changed: those the change chose by
        // the root's selector flags (none: all of them).
        let root_chosen = cx
            .shadow_root(parent)
            .filter(|&root| cx.style_flags(root).contains(StyleFlags::CHILDREN_CHANGED))
            .map(|root| {
                let chosen = self.children_to_restyle(root);
                cx.clear_style_flags(root);
                chosen
            });
        let mut siblings = Siblings::default();
        let mut in_flat_tree = true;
        for (node, assigned_to_none) in cx
            .flat_tree_children(parent)
            .map(|node| (node, false))
            .chain(unassigned)
        {
            if assigned_to_none && in_flat_tree {
                in_flat_tree = false;
                siblings = Siblings::default();
            }
            let parents = Parents {
                rendered: parents.rendered && !assigned_to_none,
                ..parents
            };
            if cx.kind(node) != NodeKind::Element {
                cx.clear_style_flags(node);
                continue;
            }
            let chosen = down
                .chosen
                .is_some_and(|chosen| chosen.contains(&cx.node_id(node)));
            let by_root = !assigned_to_none
                && root_chosen.as_ref().is_some_and(|chosen| {
                    chosen
                        .as_ref()
                        .is_none_or(|chosen| chosen.contains(&cx.node_id(node)))
                });
            let mode = if chosen || by_root {
                down.mode.max(Mode::Element)
            } else {
                down.mode
            };
            let (mode, siblings_below) = siblings.next(cx, node, mode);
            let visited = self.element(
                node,
                parents,
                Down {
                    mode,
                    below: down.below,
                    verify: down.verify || siblings.verify_later,
                    look: down.look,
                    chosen: None,
                },
                siblings_below.as_ref(),
                token,
            );
            if visited.later
                || (visited.positional && self.features().any(|features| features.later_siblings))
            {
                siblings.later = siblings.later.max(self.sibling_mode());
            }
            siblings
                .pending
                .extend(visited.siblings.into_iter().map(|set| (set, set.reach)));
            siblings.verify_later |= visited.verify_later;
        }
    }

    /// Whether a child of `element` in the flat tree (or one assigned to
    /// no slot) took a value that doesn't inherit with `inherit`, as last
    /// styled.
    fn a_child_inherits_explicitly(&self, element: NodeRef<'gc>, token: &LocalToken) -> bool {
        let cx = self.cx;
        let explicit = |node| {
            cx.computed_style(node, token)
                .is_some_and(|style| style.explicit_inheritance)
        };
        cx.flat_tree_children(element).any(explicit)
            || (cx.shadow_root(element).is_some()
                && std::iter::successors(cx.first_child(element), |&node| cx.next_sibling(node))
                    .any(|node| cx.assigned_slot(node).is_none() && explicit(node)))
    }

    /// The indexes whose rules `element` matches or may hand a change of on:
    /// the UA's, its tree scope's (the document's, or its shadow tree's
    /// sheets'), and, a host, its shadow tree's (`:host`).
    fn element_indexes(
        &self,
        element: NodeRef<'gc>,
    ) -> impl Iterator<Item = &'s RuleIndex> + use<'s, 'a, 'gc> {
        let cx = self.cx;
        let scope = |root| {
            self.shadow
                .get(cx.node_id(root))
                .map_or(&[][..], |scope| &scope.indexes[..])
        };
        let own = match cx.containing_shadow_root(element) {
            None => self.author,
            Some(root) => scope(root),
        };
        let hosted = cx.shadow_root(element).map_or(&[][..], scope);
        std::iter::once(self.ua).chain(own.iter().chain(hosted).map(|(index, _)| &**index))
    }

    /// Every index the walk's rules are in (the debug build's checks).
    #[cfg(debug_assertions)]
    fn all_indexes(&self) -> impl Iterator<Item = &'s RuleIndex> + use<'s, 'a, 'gc> {
        std::iter::once(self.ua)
            .chain(self.author.iter().map(|(index, _)| &**index))
            .chain(self.shadow.all_indexes())
    }

    /// What a sibling's change styles again of an element: its subtree too
    /// if a sibling combinator is left of a descendant one.
    fn sibling_mode(&self) -> Mode {
        if self
            .features()
            .any(|features| features.sibling_then_descendant)
        {
            Mode::Subtree
        } else {
            Mode::Element
        }
    }

    /// What the UA's, the document's and the shadow trees' selectors ask
    /// of siblings and of what is below an element (the shadow trees', all
    /// of theirs together).
    fn features(&self) -> impl Iterator<Item = &'s rule_index::Features> + use<'s, 'a, 'gc> {
        [
            Some(&self.ua.features),
            self.author_features,
            Some(&self.shadow.features),
        ]
        .into_iter()
        .flatten()
    }

    /// `element` styled again if what it was handed (`down`) or its own
    /// changes say so (`invalidate`), with its subtree if a change of it
    /// may change what matches below; its children gone down into if
    /// something below it changed, its style changes what they inherit, or
    /// a descendant set looks there (its ancestors', `siblings_below` from
    /// an earlier sibling's change, its own). What it hands its later
    /// siblings.
    fn element(
        &mut self,
        element: NodeRef<'gc>,
        parents: Parents<'_>,
        down: Down<'_>,
        siblings_below: Option<&InvalidationFeatures>,
        token: &mut LocalToken,
    ) -> Visited<'s> {
        let flags = self.cx.style_flags(element);
        let invalidation = self.invalidate(element, flags, down);
        let existing = self.cx.computed_style(element, token).cloned();
        // Its animations: moved on since (a frame's time), which their
        // values are sampled for, or, when they need the cascade, styled
        // again; its parent's style changed, which their keyframes may take.
        let moved_on = self.cx.take_animations_moved_on(element);
        let animations_restyle = (moved_on || parents.changed)
            && self
                .cx
                .read_animations(element, |targets| {
                    let own = targets[0].as_ref().map(|target| &target.animations);
                    (moved_on
                        && animation_style::needs_cascade(
                            own,
                            targets
                                .iter()
                                .flatten()
                                .map(|target| (&target.animations, &target.base)),
                        ))
                        || (parents.changed && animation_style::animated(own))
                })
                .unwrap_or(false);
        // Removed and back: its style and its subtree's are from before.
        let moved = parents.moved || {
            let root = self.cx.removed_since_styled(element);
            if root {
                self.met.push(self.cx.node_id(element));
            }
            root
        };
        let own = invalidation.own
            || existing.is_none()
            || animations_restyle
            || (self.viewport_changed && self.uses_viewport(element, existing.as_ref(), token));
        // What it had: whether its style changes (the same `Arc` if not).
        let before = existing.as_ref().map(Arc::as_ptr);
        let mut subtree = invalidation.subtree;
        let (style, inherited) = match existing {
            Some(style) if !own && moved_on => {
                let sampled = self.sample_animations(element, style, token);
                (sampled, false)
            }
            Some(style) if !own => {
                if down.verify && !moved_on && self.cx.read_animations(element, |_| ()).is_none() {
                    self.verify(element, parents, &style);
                }
                (style, false)
            }
            existing => {
                let old = existing.clone();
                let style = self.restyle(element, parents, moved, existing, token);
                // What its children inherit, or how they are laid out in
                // it (blockified in a flex container), changed; or, a
                // `display: contents` element styled again with its
                // parent, how they are laid out in its parent.
                let differs = old.as_ref().is_none_or(|old| {
                    !Arc::ptr_eq(old, &style)
                        && (!old.same_inherited(&style)
                            || old.display != style.display
                            // A child's `inherit` of a value that doesn't
                            // inherit takes the rest too (Blink's
                            // `HasExplicitInheritance`).
                            || (**old != *style
                                && self.a_child_inherits_explicitly(element, token)))
                }) || (style.display == Display::Contents
                    && down.mode != Mode::Clean);
                // `rem` changed: everything below.
                if element_is_root(self.cx, element)
                    && old
                        .as_ref()
                        .is_some_and(|old| old.font.size != style.font.size)
                {
                    subtree = true;
                }
                // `display: none` came or went, once there are animations:
                // whether every element below has a box, and so whether
                // its animations run.
                if self.cx.has_seen_animations()
                    && old.as_ref().is_some_and(|old| {
                        (old.display == Display::None) != (style.display == Display::None)
                    })
                {
                    subtree = true;
                }
                (style, differs)
            }
        };
        self.cx.clear_style_flags(element);
        if !self.viewport_used && self.uses_viewport(element, Some(&style), token) {
            self.viewport_used = true;
        }
        // Its children changed and match by position: those the change
        // among them may move (all of them, or `chosen`).
        let mut chosen = if invalidation.structural && !subtree && !inherited {
            self.children_to_restyle(element)
        } else {
            None
        };
        // A host's children are walked under the slots that show them, past
        // its shadow tree's: those the change chose are marked for it.
        if invalidation.structural && self.cx.shadow_root(element).is_some() {
            let cx = self.cx;
            let mut at = cx.first_child(element);
            while let Some(child) = at {
                if cx.kind(child) == NodeKind::Element
                    && chosen
                        .as_ref()
                        .is_none_or(|chosen| chosen.contains(&cx.node_id(child)))
                {
                    cx.set_needs_style_recalc(child);
                }
                at = cx.next_sibling(child);
            }
            chosen = Some(NodeIdHashSet::default());
        }
        let below_mode = if subtree {
            Mode::Subtree
        } else if inherited || (invalidation.structural && chosen.is_none()) {
            Mode::Element
        } else {
            Mode::Clean
        };
        let own_below = (!invalidation.below.is_empty()).then_some(&invalidation.below);
        let merged;
        let children_below = match [down.below, siblings_below, own_below] {
            [None, None, None] => None,
            [Some(one), None, None] | [None, Some(one), None] | [None, None, Some(one)] => {
                Some(one)
            }
            sets => {
                let mut all = InvalidationFeatures::default();
                for set in sets.into_iter().flatten() {
                    all.add(set);
                }
                merged = all;
                Some(&merged)
            }
        };
        let verify_children = down.verify || invalidation.verify_below;
        let look = down.look || invalidation.hosts_change;
        // Its style changed, once there are animations: its children with
        // some look (their keyframes may take its values).
        let changed = before != Some(Arc::as_ptr(&style));
        let animated_children = changed && self.cx.has_seen_animations();
        // A change of rules: each element looks its features up.
        if below_mode != Mode::Clean
            || animated_children
            || flags.contains(StyleFlags::CHILD_NEEDS_STYLE_RECALC)
            || children_below.is_some()
            || verify_children
            || self.rules.is_some()
            || look
            || self.viewport_changed
        {
            let layout_parent = if style.display == Display::Contents {
                parents.layout.clone()
            } else {
                style.clone()
            };
            self.bloom.push(self.cx, element);
            self.children(
                element,
                Parents {
                    style: &style,
                    layout: &layout_parent,
                    rendered: parents.rendered && style.display != Display::None,
                    moved,
                    changed,
                },
                Down {
                    mode: below_mode,
                    below: children_below,
                    verify: verify_children,
                    look,
                    chosen: chosen.as_ref(),
                },
                token,
            );
            self.bloom.pop();
        }
        Visited {
            positional: own && invalidation.moved,
            siblings: invalidation.siblings,
            later: invalidation.later,
            verify_later: invalidation.verify_later,
        }
    }

    /// The invalidation half of `element`'s visit, before its cascade:
    /// what its changes (its attributes', its children's, the document's
    /// rules') and what it was handed (`down`) style again of it, below it
    /// and after it, by the sets. Those of one assigned to a slot reach
    /// its later siblings at once, marked.
    fn invalidate(
        &mut self,
        element: NodeRef<'gc>,
        flags: StyleFlags,
        down: Down<'_>,
    ) -> Invalidation<'s> {
        let changes = if flags.contains(StyleFlags::ATTRIBUTES_CHANGED) {
            self.cx.take_attribute_changes(element)
        } else {
            None
        };
        let mut invalidated = Invalidated::default();
        if let Some(changes) = changes.as_deref() {
            // The states matching looked at of it alone: a rule's state of
            // an ancestor or a sibling that isn't this one's (a list box's
            // focus, not a menu list's) styles nothing else again.
            let looked_at = changes.states & self.cx.state_dependencies(element);
            let masked;
            let changes = if looked_at == changes.states {
                changes
            } else {
                masked = dom::AttributeChanges {
                    states: looked_at,
                    ..changes.clone()
                };
                &masked
            };
            for index in self.element_indexes(element) {
                index.sets.invalidated(changes, &mut invalidated);
            }
        }
        if flags.contains(StyleFlags::CHILDREN_CHANGED) {
            for index in self.element_indexes(element) {
                index.sets.empty_changed(&mut invalidated);
            }
        }
        // The document's changed rules match its own tree's elements alone,
        // a shadow tree's its own tree's, its host and what its slots are
        // assigned.
        let tree = self.cx.containing_shadow_root(element);
        let mut found = match self.rules {
            Some(rules) if tree.is_none() => {
                rules.find(self.cx, element, &mut invalidated.siblings)
            }
            _ => Found::default(),
        };
        let mut hosts_change = false;
        let mut changed_tree = None;
        if !self.shadow_changes.is_empty() {
            let cx = self.cx;
            let change = |root| self.shadow_changes.get(&cx.node_id(root));
            if let Some(root) = tree
                && let Some(change) = change(root)
            {
                let of_tree = change.find(cx, element, &mut invalidated.siblings);
                found.own |= of_tree.own;
                found.subtree |= of_tree.subtree;
                changed_tree = Some(cx.node_id(root));
            }
            if let Some(change) = cx.shadow_root(element).and_then(change) {
                hosts_change = true;
                found.own |= change.host;
            }
            let mut at = element;
            while let Some(slot) = cx.assigned_slot(at) {
                found.own |= cx
                    .containing_shadow_root(slot)
                    .and_then(change)
                    .is_some_and(|change| change.slotted);
                at = slot;
            }
        }
        let selector_flags =
            ElementSelectorFlags::from_bits_truncate(self.cx.selector_flags(element).into());
        let parent_flags =
            self.cx
                .parent(element)
                .map_or(ElementSelectorFlags::empty(), |parent| {
                    ElementSelectorFlags::from_bits_truncate(self.cx.selector_flags(parent).into())
                });
        // Its position among its siblings changed: what matches it by
        // position (a sibling combinator, `:nth-child()`, `:first-child`).
        let moved = flags.contains(StyleFlags::FOLLOWS_CHANGE)
            || flags.contains(StyleFlags::PRECEDES_CHANGE);
        let by_position = moved
            && (self.features().any(|features| features.sibling)
                || parent_flags.intersects(ElementSelectorFlags::all().for_parent()));
        let new = flags.contains(StyleFlags::NEEDS_STYLE_RECALC);
        // A table's cells take what changed as their hints.
        let hinted = changes
            .as_deref()
            .is_some_and(|changes| hints::for_descendants(self.cx, element, changes));
        let subtree = down.mode == Mode::Subtree
            || new
            || invalidated.subtree
            || found.subtree
            || hinted
            || (by_position && self.sibling_mode() == Mode::Subtree);
        // Its children changed: its `:empty`.
        let empty = flags.contains(StyleFlags::CHILDREN_CHANGED)
            && selector_flags.contains(ElementSelectorFlags::HAS_EMPTY_SELECTOR);
        // One whose position changed: an element's or shadow root's
        // children's change chooses it or not (`children_to_restyle`); a
        // document's child (nothing decides for them), by the flag.
        let by_position = by_position
            && self
                .cx
                .parent(element)
                .is_none_or(|parent| self.cx.kind(parent) == NodeKind::Document);
        // An attribute, class or id that changed is one some rule reads
        // (the DOM notes no other); a state, one matching looked at of it
        // (`Cx::state_dependencies`): a focus a rule asks of `input`s
        // alone styles no `<div>` again.
        let noted = flags.contains(StyleFlags::ATTRIBUTES_CHANGED)
            && changes.as_deref().is_none_or(|changes| {
                !changes.attributes.is_empty()
                    || !changes.classes.is_empty()
                    || !changes.ids.is_empty()
                    || changes.states & self.cx.state_dependencies(element) != 0
            });
        let own = subtree
            || down.mode == Mode::Element
            || noted
            || by_position
            || empty
            || found.own
            || down
                .below
                .is_some_and(|below| below.matches(self.cx, element));
        // A control styled again: its UA shadow tree's parts too, whose
        // rules match it as their originating element (Blink invalidates
        // its custom pseudo-elements; a part is a few elements).
        let subtree = subtree || (own && self.cx.user_agent_shadow_root(element).is_some());
        // What the sets passed over matches none of the changed rules: the
        // debug build checks once the walks are done.
        #[cfg(debug_assertions)]
        if !own {
            if self.rules.is_some() && tree.is_none() {
                self.passed_over.push((self.cx.node_id(element), None));
            }
            if changed_tree.is_some() {
                self.passed_over
                    .push((self.cx.node_id(element), changed_tree));
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = changed_tree;
        // The debug build's check: what the coarser rules before the sets
        // styled again, these pass over.
        #[cfg(debug_assertions)]
        let (verify_below, mut verify_later) = match changes.as_deref() {
            Some(changes) if self.verify => (
                !subtree
                    && self.element_indexes(element).any(|index| {
                        index.sets.all_ancestors.uses(changes) || index.features.touched_by(changes)
                    }),
                !invalidated.later
                    && self.element_indexes(element).any(|index| {
                        index.sets.all_siblings.uses(changes) || index.features.sibling
                    }),
            ),
            _ => (false, false),
        };
        #[cfg(not(debug_assertions))]
        let (verify_below, mut verify_later) = (false, false);
        // Its children came or went: what matches them by position, if a
        // selector found they depend on it.
        let structural = flags.contains(StyleFlags::CHILDREN_CHANGED)
            && selector_flags.intersects(ElementSelectorFlags::all().for_parent());
        // Assigned to a slot: its siblings are its host's children, which
        // other slots may show, before this one in the walk or after. The
        // sets mark those they reach at once, with their subtrees (Blink's
        // invalidator marks the elements its sets find, but in a pass of
        // its own, before the recalc).
        // A position counted from the end among siblings matching a list:
        // the earlier siblings, which the walk went past, are marked for
        // the walk down the marked paths after it.
        if invalidated.earlier {
            self.mark_earlier_siblings(element);
        }
        if (!invalidated.siblings.is_empty() || invalidated.later)
            && self
                .cx
                .parent(element)
                .is_some_and(|parent| self.cx.shadow_root(parent).is_some())
        {
            self.mark_siblings(element, &invalidated);
            invalidated.siblings.clear();
            invalidated.later = false;
            verify_later = false;
        }
        Invalidation {
            own,
            subtree,
            structural,
            below: invalidated.below,
            moved: new || moved,
            siblings: invalidated.siblings,
            later: invalidated.later,
            hosts_change,
            verify_below,
            verify_later,
        }
    }

    /// Marks to be styled again the later siblings of `element` (one
    /// assigned to a slot) that `invalidated`'s sibling sets reach and find
    /// (every one, for `later`), those a slot shows.
    fn mark_siblings(&self, element: NodeRef<'gc>, invalidated: &Invalidated<'s>) {
        let cx = self.cx;
        let mut after = 0usize;
        let siblings =
            std::iter::successors(cx.next_sibling(element), |node| cx.next_sibling(*node))
                .filter(|node| cx.kind(*node) == NodeKind::Element);
        for sibling in siblings {
            after = after.saturating_add(1);
            let reached = invalidated.later
                || invalidated.siblings.iter().any(|set| {
                    set.reach.is_none_or(|reach| after <= usize::from(reach))
                        && set.siblings.matches(cx, sibling)
                });
            if reached && cx.flat_tree_parent(sibling).is_some() {
                cx.set_needs_style_recalc(sibling);
            }
        }
    }

    /// Which of `parent`'s children a change among them styles again
    /// (Blink's `CheckForSiblingStyleChanges`, with the selector flags):
    /// from what matching found its children depend on (all of them, those
    /// after the change, the first and the last), how far after a change
    /// the rules see (`Features::sibling_reach`), and where the change was
    /// (the children's `FOLLOWS_CHANGE` and `PRECEDES_CHANGE`, a new one's
    /// own flag). `None`: every one.
    fn children_to_restyle(&self, parent: NodeRef<'gc>) -> Option<NodeIdHashSet> {
        use ElementSelectorFlags as Flags;
        let cx = self.cx;
        let flags = Flags::from_bits_truncate(cx.selector_flags(parent).into());
        if flags.intersects(Flags::HAS_SLOW_SELECTOR | Flags::MAY_HAVE_TREE_COUNTING_FUNCTION) {
            return None;
        }
        let children: Vec<NodeRef<'gc>> =
            std::iter::successors(cx.first_child(parent), |&node| cx.next_sibling(node)).collect();
        let changed = |child: NodeRef<'gc>, also: StyleFlags| {
            let flags = cx.style_flags(child);
            flags.contains(StyleFlags::FOLLOWS_CHANGE)
                || (also != StyleFlags::CLEAN && flags.contains(also))
        };
        let mut restyle = NodeIdHashSet::default();
        if flags.intersects(Flags::HAS_SLOW_SELECTOR_LATER_SIBLINGS) {
            // A host's shadow tree's rules (`:host(:nth-child(2))`) may
            // reach further than its parent's tree's.
            let host = children
                .iter()
                .any(|&child| cx.shadow_root(child).is_some());
            let reach = self
                .features()
                .map(Features::sibling_reach)
                .try_fold(0u8, |most, reach| reach.map(|reach| most.max(reach)));
            match reach.filter(|_| !host) {
                // From the first changed position on.
                None => {
                    if let Some(first) = children
                        .iter()
                        .position(|&child| changed(child, StyleFlags::NEEDS_STYLE_RECALC))
                    {
                        restyle.extend(children[first..].iter().map(|&child| cx.node_id(child)));
                    }
                }
                // `n` elements from each.
                Some(n) => {
                    let mut left = 0;
                    for &child in &children {
                        if changed(child, StyleFlags::CLEAN) {
                            left = n;
                        }
                        if let Some(rest) = left.checked_sub(1)
                            && cx.kind(child) == NodeKind::Element
                        {
                            restyle.insert(cx.node_id(child));
                            left = rest;
                        }
                    }
                }
            }
        }
        if flags.intersects(Flags::HAS_EDGE_CHILD_SELECTOR) {
            let mut elements = children
                .iter()
                .filter(|&&child| cx.kind(child) == NodeKind::Element);
            restyle.extend(elements.next().map(|&child| cx.node_id(child)));
            restyle.extend(elements.next_back().map(|&child| cx.node_id(child)));
            restyle.extend(
                children
                    .iter()
                    .filter(|&&child| changed(child, StyleFlags::PRECEDES_CHANGE))
                    .map(|&child| cx.node_id(child)),
            );
        }
        Some(restyle)
    }

    /// Whether `style` (`element`'s) or one of its pseudo-elements' uses
    /// viewport units, or it has animations (whose keyframes may: their
    /// values are cascaded as they are sampled).
    fn uses_viewport(
        &self,
        element: NodeRef<'gc>,
        style: Option<&Arc<ComputedStyle>>,
        token: &LocalToken,
    ) -> bool {
        style.is_some_and(|style| style.has_viewport_units)
            || self.cx.read_animations(element, |_| ()).is_some()
            || self
                .cx
                .element_style(element, token)
                .and_then(|styles| styles.pseudo.as_deref())
                .is_some_and(|pseudo| {
                    [&pseudo.before, &pseudo.after, &pseudo.marker]
                        .into_iter()
                        .flatten()
                        .any(|style| style.has_viewport_units)
                })
    }

    /// Marks to be styled again every earlier element sibling of
    /// `element`.
    fn mark_earlier_siblings(&self, element: NodeRef<'gc>) {
        let cx = self.cx;
        let mut at = cx.previous_sibling(element);
        while let Some(sibling) = at {
            if cx.kind(sibling) == NodeKind::Element {
                cx.set_needs_style_recalc(sibling);
            }
            at = cx.previous_sibling(sibling);
        }
    }

    /// The debug build's check of an element the sets passed over: it
    /// cascades as its style is.
    fn verify(&mut self, element: NodeRef<'gc>, parents: Parents<'_>, style: &Arc<ComputedStyle>) {
        // The check's work isn't the walk's: the counts as they were.
        let (matched, hits) = (self.matched, self.cache.borrow().hits);
        let inline = self.cx.inline_style(element);
        let presentation = hints::declarations(
            self.cx,
            element,
            self.url_context.as_ref(),
            &mut self.presentation,
        );
        let (again, _) = self.cascade(
            element,
            parents.style,
            parents.layout,
            inline.as_ref(),
            presentation.as_ref(),
            None,
        );
        // One with animations: its cascade's is the base its animations'
        // values went on.
        let base = self.cx.style_change_animations(element, None, |slot, _| {
            slot.as_ref().map(|animated| animated.base.clone())
        });
        assert!(
            *again == **base.as_ref().unwrap_or(style),
            "an element the invalidation sets passed over styles otherwise: #{}",
            self.cx.node_id(element).get()
        );
        self.matched = matched;
        self.cache.borrow_mut().hits = hits;
    }

    /// `element`'s style worked out again and written: its old one kept
    /// (the same `Arc`) if it comes out the same, else layout told.
    fn restyle(
        &mut self,
        element: NodeRef<'gc>,
        parents: Parents<'_>,
        moved: bool,
        old: Option<Arc<ComputedStyle>>,
        token: &mut LocalToken,
    ) -> Arc<ComputedStyle> {
        #[cfg(debug_assertions)]
        self.restyled.push(self.cx.node_id(element));
        // The element's one copy (the DOM's: CSSOM's, or parsed from the
        // attribute the first time and kept).
        let inline = self.cx.inline_style(element);
        let presentation = hints::declarations(
            self.cx,
            element,
            self.url_context.as_ref(),
            &mut self.presentation,
        );
        // Its style change event: its animations' and its
        // pseudo-elements' start, change or stop, and give their values.
        let old_pseudo = self
            .cx
            .element_style(element, token)
            .and_then(|written| written.pseudo.as_deref());
        let (style, pseudo) = self.cascade(
            element,
            parents.style,
            parents.layout,
            inline.as_ref(),
            presentation.as_ref(),
            Some(StyleChangeOf {
                old: old.as_ref(),
                old_pseudo,
                rendered: parents.rendered,
                moved,
            }),
        );
        let style = match old {
            Some(old) if Arc::ptr_eq(&old, &style) || *old == *style => old,
            _ => {
                self.cx.set_style_changed(element);
                style
            }
        };
        // Not in a `display: none` subtree either: Blink doesn't style
        // one, so loads none of its images.
        if parents.rendered && style.display != Display::None {
            let mut urls = style
                .background_image_urls(self.device_pixel_ratio)
                .peekable();
            if urls.peek().is_some() {
                let id = self.cx.node_id(element);
                self.background_image_urls
                    .extend(urls.map(|url| (id, url.clone())));
            }
        }
        if element_is_root(self.cx, element) {
            self.root_font_size = style.font.size;
        }
        let pseudo = (pseudo != PseudoStyles::default()).then(|| Box::new(pseudo));
        let written = self.cx.element_style_mut(element, token);
        if written.pseudo != pseudo {
            written.pseudo = pseudo;
            self.cx.set_pseudo_styles_changed(element);
        }
        written.inline = inline;
        written.presentation = presentation;
        written.computed = Some(style.clone());
        style
    }

    /// `target` of `element` (it, or a pseudo-element of it) at its style
    /// change event, `base` by the cascade (`restyle` cascades it again):
    /// its style with its animations' values (`animation_style`), their
    /// events queued in its document. The animations that leave it
    /// (canceled, replaced, done) stay off it while script holds them.
    fn animate(
        &self,
        element: NodeRef<'gc>,
        target: TargetChange<'_>,
        base: Arc<ComputedStyle>,
        restyle: &dyn Restyle,
    ) -> Arc<ComputedStyle> {
        let cx = self.cx;
        let find = |name: &str| self.find_keyframes(element, name);
        let (style, sent, removed) =
            cx.style_change_animations(element, target.pseudo, |slot, order| {
                if slot.is_none() && !animation_style::may_start(&base) {
                    return (base, Vec::new(), Vec::new());
                }
                let (mut sent, mut removed) = (Vec::new(), Vec::new());
                let style = animation_style::style_change(
                    slot,
                    StyleChange {
                        old: target.old,
                        base,
                        rendered: target.rendered,
                        moved: target.moved,
                        restyle,
                        find: &find,
                    },
                    &mut Timeline {
                        now: self.now,
                        order,
                        sent: &mut sent,
                        removed: &mut removed,
                    },
                );
                (style, sent, removed)
            });
        cx.queue_animation_events(element, target.pseudo, sent);
        cx.keep_detached_animations(cx.document_of(element), removed);
        style
    }

    /// `element`'s pseudo-element `pseudo` is gone: its animations are
    /// canceled.
    fn forget_pseudo_animations(&self, element: NodeRef<'gc>, pseudo: PseudoId) {
        let now = self.now;
        let (sent, removed) = self
            .cx
            .style_change_animations(element, Some(pseudo), |slot, _| {
                let mut sent = Vec::new();
                let mut removed = Vec::new();
                if let Some(mut gone) = slot.take() {
                    gone.animations.cancel_all(now, &mut sent);
                    removed = gone.animations.take_removed();
                }
                (sent, removed)
            });
        self.cx.queue_animation_events(element, Some(pseudo), sent);
        self.cx
            .keep_detached_animations(self.cx.document_of(element), removed);
    }

    /// `element` at a frame where only its animations moved on (and they
    /// don't need the cascade): its base style and its pseudo-elements',
    /// with their values now; what changed written.
    fn sample_animations(
        &self,
        element: NodeRef<'gc>,
        old: Arc<ComputedStyle>,
        token: &mut LocalToken,
    ) -> Arc<ComputedStyle> {
        let now = self.now;
        let Some(sampled) = self.cx.read_animations(element, |targets| {
            targets.each_ref().map(|target| {
                target
                    .as_ref()
                    .map(|target| target.animations.apply(now, &target.base))
            })
        }) else {
            return old;
        };
        let [own, before, after, marker] = sampled;
        let written = self.cx.element_style_mut(element, token);
        let style = match own {
            Some(style) if style != *old => {
                let style = Arc::new(style);
                written.computed = Some(style.clone());
                Some(style)
            }
            _ => None,
        };
        let mut pseudo_changed = false;
        if let Some(pseudo) = written.pseudo.as_deref_mut() {
            for (slot, new) in [
                (&mut pseudo.before, before),
                (&mut pseudo.after, after),
                (&mut pseudo.marker, marker),
            ] {
                if let (Some(old), Some(new)) = (slot.as_ref(), new)
                    && **old != new
                {
                    *slot = Some(Arc::new(new));
                    pseudo_changed = true;
                }
            }
        }
        if pseudo_changed {
            self.cx.set_pseudo_styles_changed(element);
        }
        match style {
            Some(style) => {
                self.cx.set_style_changed(element);
                style
            }
            None => old,
        }
    }

    /// The `@keyframes` named `name` that `element`'s `animation-name`
    /// finds: its own shadow tree's (for its `:host` rules), then its tree
    /// scope's and those around it, out to the document's; the last sheet's
    /// first (css-scoping's tree-scoped names).
    fn find_keyframes(&self, element: NodeRef<'gc>, name: &str) -> Option<Arc<css::KeyframesRule>> {
        let cx = self.cx;
        let in_scope = |root: NodeRef<'gc>| {
            self.shadow
                .get(cx.node_id(root))?
                .indexes
                .iter()
                .rev()
                .find_map(|(index, _)| Some(index.set.keyframes(name)?.0.clone()))
        };
        if let Some(found) = cx.shadow_root(element).and_then(in_scope) {
            return Some(found);
        }
        let mut at = cx.containing_shadow_root(element);
        while let Some(root) = at {
            if let Some(found) = in_scope(root) {
                return Some(found);
            }
            at = cx
                .shadow_host(root)
                .and_then(|host| cx.containing_shadow_root(host));
        }
        // The later index's, of two with one name (as in one index).
        self.author
            .iter()
            .rev()
            .find_map(|(index, _)| Some(index.set.keyframes(name)?.0.clone()))
    }

    /// The declaration blocks `element` cascades (Blink's
    /// `ElementRuleCollector`'s matched rules), for itself and each
    /// pseudo-element it has rules for, in `TARGETS`' order: its
    /// presentational hints, the rules it matches and its `style`
    /// attribute's (its own only), unsorted. What `cascade` cascades, and
    /// what `StyleEngine::explain` tells DevTools: the two can't differ.
    fn blocks<'b>(
        &mut self,
        element: NodeRef<'gc>,
        inline: Option<&'b Arc<CssPropertyValueSet>>,
        presentation: Option<&'b (Arc<CssPropertyValueSet>, u64)>,
    ) -> [Blocks<'b>; 5]
    where
        's: 'b,
    {
        let mut blocks: [Blocks<'b>; 5] = Default::default();
        // Author level, specificity 0, and before every rule: order 0 is a
        // UA rule's at most, and those are a level below.
        if let Some((hints, digest)) = presentation {
            push_block(
                &mut blocks[0],
                Origin::Author,
                0,
                false,
                0,
                0,
                hints,
                BlockSource::Hints(*digest),
            );
        }
        let ua_rules = self.ua.set.rules().len();
        for source in self.sources(element) {
            let Source {
                index,
                which,
                host,
                tree_order,
                first,
            } = source;
            let origin = if std::ptr::eq(index, self.ua) {
                Origin::UserAgent
            } else {
                Origin::Author
            };
            let first = if origin == Origin::UserAgent {
                0
            } else {
                ua_rules.saturating_add(first)
            };
            self.matching(
                element,
                index,
                which,
                host,
                |target, order, specificity, rule| {
                    push_block(
                        &mut blocks[target_index(target)],
                        origin,
                        tree_order,
                        false,
                        specificity,
                        first.saturating_add(order),
                        &rule.block,
                        BlockSource::Rule(rule),
                    );
                },
            );
        }
        if let Some(inline) = inline {
            push_block(
                &mut blocks[0],
                Origin::Author,
                0,
                true,
                0,
                0,
                inline,
                BlockSource::Own,
            );
        }
        blocks
    }

    /// `element`'s computed style and its pseudo-elements' (Blink's
    /// `StyleResolver::ResolveStyle`): its rules matched once, each one
    /// counting for what it is for.
    fn cascade(
        &mut self,
        element: NodeRef<'gc>,
        parent: &Arc<ComputedStyle>,
        layout_parent: &ComputedStyle,
        inline: Option<&Arc<CssPropertyValueSet>>,
        presentation: Option<&(Arc<CssPropertyValueSet>, u64)>,
        change: Option<StyleChangeOf<'_>>,
    ) -> (Arc<ComputedStyle>, PseudoStyles) {
        let [own, before, after, marker, selection] =
            self.blocks(element, inline, presentation);
        // Its pseudo-elements inherit its own.
        let blocking_wheel = self.cx.has_own_blocking_wheel_listener(element);
        let cache = self.cache;
        let is_root = element_is_root(self.cx, element);
        // The root's own `rem` can't be its last font size: a root of
        // `2rem` would double each time it is styled again.
        let rem = if is_root {
            self.initial_font_size
        } else {
            self.root_font_size
        };
        let own_cascade = Recascade {
            blocks: &own,
            parent,
            layout_parent,
            is_root,
            root_font_size: rem,
            env: self.env,
            control: form_control(self.cx, element),
            applies: |_| true,
            shared_parent: Some(parent),
            cache: Some(cache),
            blocking_wheel,
            pseudo: None,
        };
        // With its animations' values, which its pseudo-elements inherit.
        let style = own_cascade.cascade(None, None);
        // Neither it nor an ancestor is `display: none`.
        let element_rendered = change
            .as_ref()
            .is_some_and(|change| change.rendered && style.display != Display::None);
        let style = match &change {
            Some(change) => self.animate(
                element,
                TargetChange {
                    pseudo: None,
                    old: change.old,
                    rendered: element_rendered,
                    moved: change.moved,
                },
                style,
                &own_cascade,
            ),
            None => style,
        };
        // Its `::selection`'s, from its own rules and its parent's (which it
        // has inherited); one no rule matches shares the parent's (Blink's
        // `ResolveHighlightStyles`).
        let mut style = style;
        if !selection.is_empty() {
            let inherited = style.rare_inherited.highlight.clone();
            let (highlight, variables) =
                cascade_highlight(selection, &style, inherited.as_deref(), rem, self.env);
            let style = Arc::make_mut(&mut style);
            style.rare_inherited_mut().highlight = Some(Arc::new(highlight));
            if variables.is_some() {
                style.rare_mut().highlight_variables = variables;
            }
        }
        let mut pseudo_styles = PseudoStyles::default();
        let list_item = style.display == Display::ListItem;
        for (pseudo, blocks) in [
            (PseudoId::Before, before),
            (PseudoId::After, after),
            (PseudoId::Marker, marker),
        ] {
            let marker = pseudo == PseudoId::Marker;
            if if marker {
                !list_item
            } else {
                blocks.is_empty()
            } {
                continue;
            }
            let applies = if marker {
                LonghandId::applies_to_marker
            } else {
                |_| true
            };
            // A `display: contents` element's pseudo-elements' boxes go
            // where its own would.
            let pseudo_layout_parent = if style.display == Display::Contents {
                layout_parent
            } else {
                &*style
            };
            let pseudo_cascade = Recascade {
                blocks: &blocks,
                parent: &style,
                layout_parent: pseudo_layout_parent,
                is_root: false,
                // The root's, as it is now.
                root_font_size: if is_root {
                    style.font.size
                } else {
                    self.root_font_size
                },
                env: self.env,
                control: None,
                applies,
                shared_parent: None,
                cache: Some(cache),
                blocking_wheel,
                pseudo: Some(pseudo),
            };
            let mut pseudo_style = pseudo_cascade.cascade(None, None);
            // `attr()`'s values are the element's: after the matched
            // properties cache, which shares the rest.
            if let Content::Items(items) = &pseudo_style.rare.content
                && items
                    .iter()
                    .any(|item| matches!(item, ContentItem::Attr { .. }))
            {
                let items = items
                    .iter()
                    .map(|item| match item {
                        ContentItem::Attr { name, .. } => {
                            let local = self.attribute_name(element, name);
                            self.attr_names.push(names::intern(&local));
                            ContentItem::Attr {
                                name: name.clone(),
                                value: self.attribute_text(element, &local).into(),
                            }
                        }
                        other => other.clone(),
                    })
                    .collect();
                Arc::make_mut(&mut pseudo_style).rare_mut().content = Content::Items(items);
            }
            // A `::before` or `::after` with no `content` has no box: as if
            // it were not there.
            if let Some(change) = &change {
                let rendered = element_rendered
                    && pseudo_style.display != Display::None
                    && (marker || pseudo_style.rare.content != Content::None);
                let old = change.old_pseudo.and_then(|old| old.get(pseudo));
                pseudo_style = self.animate(
                    element,
                    TargetChange {
                        pseudo: Some(pseudo),
                        old,
                        rendered,
                        moved: change.moved,
                    },
                    pseudo_style,
                    &pseudo_cascade,
                );
            }
            let slot = match pseudo {
                PseudoId::Before => &mut pseudo_styles.before,
                PseudoId::After => &mut pseudo_styles.after,
                PseudoId::Marker => &mut pseudo_styles.marker,
            };
            *slot = Some(pseudo_style);
        }
        // A pseudo-element no longer there: its animations are canceled.
        if change.is_some() {
            for pseudo in [PseudoId::Before, PseudoId::After, PseudoId::Marker] {
                if pseudo_styles.get(pseudo).is_none() {
                    self.forget_pseudo_animations(element, pseudo);
                }
            }
        }
        (style, pseudo_styles)
    }

    /// The indexes `element` matches, in cascade order: the UA's; its tree
    /// scope's (the document's, or its shadow tree's sheets', for which
    /// the host is that tree's; a `<use>`'s copy's, its `<use>`'s); the `::slotted()` rules of each slot's
    /// tree it is assigned through, the nearest first; a host's own shadow
    /// tree's `:host` rules.
    fn sources(&self, element: NodeRef<'gc>) -> smallvec::SmallVec<[Source<'s, 'gc>; 4]> {
        let cx = self.cx;
        let scope = |root| {
            self.shadow
                .get(cx.node_id(root))
                .map_or(&[][..], |scope| &scope.indexes[..])
        };
        let mut sources = smallvec::smallvec![Source {
            index: self.ua,
            which: Which::Tree,
            host: None,
            tree_order: 0,
            first: 0,
        }];
        let tree = |index: &'s RuleIndex, first, which, host, tree_order| Source {
            index,
            which,
            host,
            tree_order,
            first,
        };
        // An element of a `<use>`'s copy matches the rules of the tree of
        // the `<use>` outside any copy, as itself (Blink's
        // `ScopedStyleResolverFor`, its corresponding element's tree).
        let scoped = cx.outermost_use(element).unwrap_or(element);
        match cx.containing_shadow_root(scoped) {
            None => sources.extend(
                self.author
                    .iter()
                    .map(|(index, first)| tree(index, *first, Which::Tree, None, 0)),
            ),
            Some(root) => {
                let host = cx.shadow_host(root);
                sources.extend(
                    scope(root)
                        .iter()
                        .map(|(index, first)| tree(index, *first, Which::Tree, host, 0)),
                );
                // A part of a UA shadow tree: its host's tree's rules for
                // it, the UA's and the author's (css-scoping: the host's
                // tree's context, before its own).
                if let Some(control) = host.filter(|_| cx.shadow_pseudo_id(element).is_some()) {
                    sources.push(tree(self.ua, 0, Which::Part, None, 0));
                    match cx.containing_shadow_root(control) {
                        None => sources.extend(
                            self.author
                                .iter()
                                .map(|(index, first)| tree(index, *first, Which::Part, None, 0)),
                        ),
                        Some(outer) => {
                            let outer_host = cx.shadow_host(outer);
                            sources.extend(scope(outer).iter().map(|(index, first)| {
                                tree(index, *first, Which::Part, outer_host, 0)
                            }));
                        }
                    }
                }
            }
        }
        let mut tree_order = 1u32;
        let mut at = element;
        while let Some(slot) = cx.assigned_slot(at) {
            if let Some(root) = cx.containing_shadow_root(slot) {
                let host = cx.shadow_host(root);
                sources.extend(
                    scope(root).iter().map(|(index, first)| {
                        tree(index, *first, Which::Slotted, host, tree_order)
                    }),
                );
            }
            tree_order = tree_order.saturating_add(1);
            at = slot;
        }
        if let Some(root) = cx.shadow_root(element) {
            sources.extend(
                scope(root).iter().map(|(index, first)| {
                    tree(index, *first, Which::Host, Some(element), tree_order)
                }),
            );
        }
        sources
    }

    /// The value of `element`'s attribute `name` in no namespace, empty if
    /// it has none (`attr()`).
    /// The attribute `attr(name)` reads of `element`: `name` in ASCII
    /// lowercase for an HTML element in an HTML document, as attribute
    /// names are there (Blink's `CSSAttrValue` lookup). Not an XHTML one,
    /// which machopo parses as HTML but Chrome as XML (`text/html` alone).
    fn attribute_name<'n>(&self, element: NodeRef<'gc>, name: &'n str) -> Cow<'n, str> {
        let document = self.cx.document_of(element);
        let html = self
            .cx
            .qualified_name(element)
            .is_some_and(|qualified| qualified.namespace() == names::HTML_NAMESPACE)
            && self.cx.is_html_document(document)
            && *self.cx.document_content_type(document).to_str() == *"text/html";
        if html && name.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(name.to_ascii_lowercase())
        } else {
            Cow::Borrowed(name)
        }
    }

    fn attribute_text(&self, element: NodeRef<'gc>, name: &str) -> String {
        self.cx
            .attribute_value_ns(element, None, name)
            .map(|value| value.borrow().to_str().into_owned())
            .unwrap_or_default()
    }

    /// Each rule of `index` that `element` matches, for the element itself
    /// or one of its pseudo-elements: what it is for, its order in the
    /// index, its most specific matching selector's specificity and the
    /// rule (Blink's `ElementRuleCollector`). `which` of its rules: those
    /// of the element's own tree, the `:host` ones (`element` is `host`,
    /// featureless to them: no ancestors or siblings there) or the
    /// `::slotted()` ones; `host`, that of the shadow tree whose rules
    /// they are.
    fn matching(
        &mut self,
        element: NodeRef<'gc>,
        index: &'s RuleIndex,
        which: Which,
        host: Option<NodeRef<'gc>>,
        mut found: impl FnMut(Target, usize, u32, &'s Arc<css::StyleRule>),
    ) {
        let matched = StyleElement {
            cx: self.cx,
            node: element,
            serial: self.serial,
            shadow_pseudo: which == Which::Part,
            selection: false,
        };
        let quirks = if self.quirks {
            SelectorsQuirksMode::Quirks
        } else {
            SelectorsQuirksMode::NoQuirks
        };
        self.candidates.clear();
        match which {
            Which::Tree if self.linear => {
                for (rule, active) in index.set.rules().iter().enumerate() {
                    for (selector, s) in active.rule.selectors.slice().iter().enumerate() {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a set's rules and a rule's selectors are fewer than 2^32, \
                                      as `RuleData` numbers them"
                        )]
                        self.candidates.push(RuleData {
                            rule: rule as u32,
                            selector: selector as u32,
                            specificity: s.specificity(),
                            // Matched in full, as everything is here.
                            ancestor_hashes: [0; 3],
                        });
                    }
                }
            }
            Which::Tree => {
                let id = matched.id().map(|id| id.borrow().addr());
                let classes = self.cx.class_names(element).iter().map(|class| class.lower);
                index.collect(
                    id,
                    self.quirks,
                    classes,
                    matched.name().local(),
                    &mut self.candidates,
                );
            }
            Which::Host => self.candidates.extend_from_slice(&index.host),
            Which::Slotted => self.candidates.extend_from_slice(&index.slotted),
            Which::Part => self.candidates.extend_from_slice(&index.parts),
        }
        self.candidates.sort_unstable();
        self.candidates.dedup();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut self.caches,
            quirks,
            self.selector_flags,
            MatchingForInvalidation::No,
        );
        let mut pseudo_context = MatchingContext::new(
            MatchingMode::ForStatelessPseudoElement,
            None,
            &mut self.pseudo_caches,
            quirks,
            self.selector_flags,
            MatchingForInvalidation::No,
        );
        let current_host = host.map(|host| selectors::OpaqueElement::new(host.into_ref()));
        for context in [&mut context, &mut pseudo_context] {
            context.current_host = current_host;
            context.featureless = which == Which::Host;
        }
        let rules = index.set.rules();
        let mut at = 0;
        while let Some(first) = self.candidates.get(at) {
            let rule_index = first.rule as usize;
            let rule = &rules[rule_index].rule;
            let mut specificity: [Option<u32>; 5] = [None; 5];
            while let Some(data) = self
                .candidates
                .get(at)
                .filter(|data| data.rule as usize == rule_index)
            {
                #[expect(clippy::arithmetic_side_effects, reason = "at < candidates.len()")]
                {
                    at += 1;
                }
                let selector = &rule.selectors.slice()[data.selector as usize];
                let target = match selector.pseudo_element() {
                    None => Target::Element,
                    // A part's rule is the part's own.
                    Some(pseudo) if pseudo.shadow_pseudo_id().is_some() => {
                        if which != Which::Part {
                            continue;
                        }
                        Target::Element
                    }
                    Some(PseudoElement::Selection) => Target::Selection,
                    Some(pseudo) => match pseudo.id() {
                        Some(id) => Target::Pseudo(id),
                        None => continue,
                    },
                };
                let hashes = AncestorHashes {
                    packed_hashes: data.ancestor_hashes,
                };
                // The filter has the element's ancestors in the flat tree:
                // more than its tree's, never fewer, and those a slot's tree
                // (`::slotted()`) or a host's (`:host`) asks of too.
                if !selector_may_match(&hashes, self.bloom.filter()) {
                    continue;
                }
                // A `::selection` rule is matched as the element's own
                // `::selection`, so that `:window-inactive` after it
                // matches too.
                let (context, matched_as) = match target {
                    Target::Element => (&mut context, matched),
                    Target::Selection => (
                        &mut context,
                        StyleElement {
                            selection: true,
                            ..matched
                        },
                    ),
                    Target::Pseudo(_) => (&mut pseudo_context, matched),
                };
                self.matched = self.matched.wrapping_add(1);
                if matches_selector(selector, 0, None, &matched_as, context) {
                    let slot = &mut specificity[target_index(target)];
                    *slot = (*slot).max(Some(data.specificity));
                    if target == Target::Element
                        && let Some(found) = &mut self.matched_selectors
                    {
                        found.push((rule, data.selector as usize));
                    }
                }
            }
            for (target, specificity) in TARGETS.into_iter().zip(specificity) {
                if let Some(specificity) = specificity {
                    found(target, rule_index, specificity, rule);
                }
            }
        }
    }
}

/// Whether `element` is its document's root element.
fn element_is_root<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    cx.parent(element)
        .is_some_and(|parent| cx.kind(parent) == NodeKind::Document)
}
