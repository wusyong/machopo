//! Parsing stylesheets and declaration blocks with `cssparser`.
//!
//! Style rules, the conditional group rules `@media` and `@supports` with
//! the rules in them, and `@keyframes` with its keyframes are kept; every
//! other at-rule (`@font-face`, `@import`, ...) is dropped, as is a rule
//! whose selector we can't parse.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, DeclarationParser, ParseError, ParseErrorKind,
    Parser, ParserInput, ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
    StyleSheetParser, ToCss, match_ignore_ascii_case,
};
use selectors::SelectorList;
use selectors::parser::SelectorParseErrorKind;

use computed_style::AnimationName;

use crate::animation::{animation_name_to_css, is_reserved};
use crate::css_url_data::UrlContext;
use crate::inspector::{Recorder, SourceRuleKind};
use crate::media::{MediaEnvironment, MediaQueryList};
use crate::properties::{DeclarationError, LonghandId, Shorthand};
use crate::property_set::{CssPropertyValueSet, PropertyValue, parse_property};
use crate::rule_set::RuleSet;
use crate::selector::{Impl, SelectorParser};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent,
    Author,
}

/// The declarations of a block or a `style` attribute.
pub(crate) fn parse_declaration_list(
    input: &mut Parser<'_, '_>,
    log: &mut ParseLog,
) -> CssPropertyValueSet {
    let mut parser = DeclarationListParser {
        parsed: Vec::new(),
        log,
        keyframe: false,
    };
    // Errors are already logged; the invalid declaration is dropped.
    for _ in RuleBodyParser::new(input, &mut parser) {}
    CssPropertyValueSet::from_parsed(parser.parsed)
}

/// A rule of a sheet or of a group rule (Blink's `StyleRuleBase`).
#[derive(Clone)]
pub enum CssRule {
    Style(Arc<StyleRule>),
    Media(Arc<MediaRule>),
    Supports(Arc<SupportsRule>),
    Keyframes(Arc<KeyframesRule>),
    /// Only ever one of a `@keyframes`' rules.
    Keyframe(Arc<KeyframeRule>),
}

/// `@media` (Blink's `StyleRuleMedia`): its rules apply while its queries
/// match.
pub struct MediaRule {
    pub media: Arc<MediaQueryList>,
    pub rules: Vec<CssRule>,
}

/// `@supports` (Blink's `StyleRuleSupports`): its rules apply if its
/// condition holds, which is worked out as it is parsed (it never changes).
pub struct SupportsRule {
    /// The condition as written, white space simplified (Blink's
    /// `condition_text_`).
    pub condition: String,
    pub supported: bool,
    pub rules: Vec<CssRule>,
}

/// `@keyframes` (Blink's `StyleRuleKeyframes`): a name, and its keyframes
/// in the order written.
pub struct KeyframesRule {
    /// As written: names match case-sensitively.
    pub name: Arc<str>,
    /// Each a [`CssRule::Keyframe`].
    pub rules: Vec<CssRule>,
}

impl KeyframesRule {
    pub fn keyframes(&self) -> impl Iterator<Item = &Arc<KeyframeRule>> {
        self.rules.iter().filter_map(|rule| match rule {
            CssRule::Keyframe(keyframe) => Some(keyframe),
            _ => None,
        })
    }
}

/// One keyframe of a `@keyframes` (Blink's `StyleRuleKeyframe`): where it
/// is in the animation and its declarations, none `!important` and none
/// of `animation-*` but `animation-timing-function` (css-animations 1).
pub struct KeyframeRule {
    /// From 0 to 1 (`from` is 0, `to` 1), in the order written.
    pub offsets: Vec<f32>,
    pub block: CssPropertyValueSet,
}

impl KeyframeRule {
    /// `keyText`: each offset a percentage, `from` and `to` too (Blink's
    /// `StyleRuleKeyframe::KeyText`).
    pub fn key_text(&self) -> String {
        let mut out = String::new();
        for (i, offset) in self.offsets.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            crate::values::write_number(&mut out, f64::from(*offset) * 100.0);
            out.push('%');
        }
        out
    }

    /// `cssText`: `0% { top: 0px; }`, and `0% { }` with no declarations.
    pub fn css_text(&self) -> String {
        let declarations = self.block.css_text();
        if declarations.is_empty() {
            format!("{} {{ }}", self.key_text())
        } else {
            format!("{} {{ {declarations} }}", self.key_text())
        }
    }
}

impl CssRule {
    /// Whether `self` and `other` are the same rule: rules never change, so
    /// the same `Arc`.
    pub fn same(&self, other: &CssRule) -> bool {
        match (self, other) {
            (Self::Style(a), Self::Style(b)) => Arc::ptr_eq(a, b),
            (Self::Media(a), Self::Media(b)) => Arc::ptr_eq(a, b),
            (Self::Supports(a), Self::Supports(b)) => Arc::ptr_eq(a, b),
            (Self::Keyframes(a), Self::Keyframes(b)) => Arc::ptr_eq(a, b),
            (Self::Keyframe(a), Self::Keyframe(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The style rule it is; `None` for a group rule.
    pub fn as_style(&self) -> Option<&Arc<StyleRule>> {
        match self {
            Self::Style(rule) => Some(rule),
            _ => None,
        }
    }

    /// `cssText` (CSSOM's "serialize a CSS rule", as Blink writes it).
    pub fn css_text(&self) -> String {
        match self {
            Self::Style(rule) => rule.css_text(),
            Self::Media(rule) => {
                let mut out = format!("@media {}", rule.media.to_css());
                write_child_rules(&rule.rules, &mut out);
                out
            }
            Self::Supports(rule) => {
                let mut out = format!("@supports {}", rule.condition);
                write_child_rules(&rule.rules, &mut out);
                out
            }
            // Blink's `CSSKeyframesRule::cssText`: ` { ` and a line break,
            // then each keyframe on a line of its own.
            Self::Keyframes(rule) => {
                let mut out = String::from("@keyframes ");
                animation_name_to_css(&AnimationName::Name(rule.name.clone()), &mut out);
                out.push_str(" { \n");
                for keyframe in rule.keyframes() {
                    out.push_str("  ");
                    out.push_str(&keyframe.css_text());
                    out.push('\n');
                }
                out.push('}');
                out
            }
            Self::Keyframe(rule) => rule.css_text(),
        }
    }

    /// A conditional group rule's `conditionText`; `None` for any other.
    pub fn condition_text(&self) -> Option<String> {
        match self {
            Self::Style(_) | Self::Keyframes(_) | Self::Keyframe(_) => None,
            Self::Media(rule) => Some(rule.media.to_css()),
            Self::Supports(rule) => Some(rule.condition.clone()),
        }
    }

    /// A grouping rule's rules, or a `@keyframes`' keyframes; `None` for a
    /// style rule or a keyframe.
    pub fn child_rules(&self) -> Option<&[CssRule]> {
        match self {
            Self::Style(_) | Self::Keyframe(_) => None,
            Self::Media(rule) => Some(&rule.rules),
            Self::Supports(rule) => Some(&rule.rules),
            Self::Keyframes(rule) => Some(&rule.rules),
        }
    }

    /// This grouping rule (or `@keyframes`) with `rules` as its rules.
    ///
    /// # Panics
    ///
    /// If it is a style rule or a keyframe.
    pub fn with_child_rules(&self, rules: Vec<CssRule>) -> CssRule {
        match self {
            Self::Style(_) | Self::Keyframe(_) => panic!("a style rule or keyframe has no rules"),
            Self::Keyframes(rule) => Self::Keyframes(Arc::new(KeyframesRule {
                name: rule.name.clone(),
                rules,
            })),
            Self::Media(rule) => Self::Media(Arc::new(MediaRule {
                media: rule.media.clone(),
                rules,
            })),
            Self::Supports(rule) => Self::Supports(Arc::new(SupportsRule {
                condition: rule.condition.clone(),
                supported: rule.supported,
                rules,
            })),
        }
    }

    /// This `@keyframes` named `name` (`CSSKeyframesRule.name`: any
    /// string, written as a string when it isn't an identifier).
    ///
    /// # Panics
    ///
    /// If it isn't a `@keyframes`.
    pub fn with_keyframes_name(&self, name: &str) -> CssRule {
        let Self::Keyframes(rule) = self else {
            panic!("only a @keyframes has a name");
        };
        Self::Keyframes(Arc::new(KeyframesRule {
            name: Arc::from(name),
            rules: rule.rules.clone(),
        }))
    }

    /// `CSSKeyframesRule.appendRule(rule)`: this `@keyframes` with `rule`
    /// parsed as a keyframe and put last; `None` if it isn't exactly one
    /// keyframe (nothing happens, css-animations 1).
    ///
    /// # Panics
    ///
    /// If it isn't a `@keyframes`.
    pub fn append_keyframe(&self, rule: &str) -> Option<CssRule> {
        let Self::Keyframes(keyframes) = self else {
            panic!("only a @keyframes has keyframes");
        };
        let keyframe = parse_keyframe(rule)?;
        let mut rules = keyframes.rules.clone();
        rules.push(keyframe);
        Some(self.with_child_rules(rules))
    }

    /// The index of the last keyframe of this `@keyframes` whose selectors
    /// are `key` (`findRule()`, `deleteRule()`: `25%, 75%` matches only
    /// those two, in that order); `None` if none is or `key` doesn't parse.
    pub fn find_keyframe(&self, key: &str) -> Option<usize> {
        let Self::Keyframes(keyframes) = self else {
            return None;
        };
        let offsets = parse_key_text(key)?;
        keyframes.rules.iter().rposition(|rule| match rule {
            CssRule::Keyframe(keyframe) => keyframe.offsets == offsets,
            _ => false,
        })
    }

    /// This keyframe with `key` as its selectors (`keyText`); `None` if
    /// they don't parse (a `SyntaxError`).
    ///
    /// # Panics
    ///
    /// If it isn't a keyframe.
    pub fn with_key_text(&self, key: &str) -> Option<CssRule> {
        let Self::Keyframe(keyframe) = self else {
            panic!("only a keyframe has a key");
        };
        Some(Self::Keyframe(Arc::new(KeyframeRule {
            offsets: parse_key_text(key)?,
            block: keyframe.block.clone(),
        })))
    }

    /// This keyframe with `block` as its declarations (its `style`
    /// changed).
    ///
    /// # Panics
    ///
    /// If it isn't a keyframe.
    pub fn with_keyframe_block(&self, block: CssPropertyValueSet) -> CssRule {
        let Self::Keyframe(keyframe) = self else {
            panic!("only a keyframe has a block of its own here");
        };
        Self::Keyframe(Arc::new(KeyframeRule {
            offsets: keyframe.offsets.clone(),
            block,
        }))
    }

    /// This media rule with `media` as its queries.
    ///
    /// # Panics
    ///
    /// If it isn't a media rule.
    pub fn with_media(&self, media: MediaQueryList) -> CssRule {
        let Self::Media(rule) = self else {
            panic!("only a media rule has media");
        };
        Self::Media(Arc::new(MediaRule {
            media: Arc::new(media),
            rules: rule.rules.clone(),
        }))
    }

    /// The grouping rule's `insertRule(rule, index)`: this rule with
    /// `rule` parsed and put at `index` (CSSOM's "insert a CSS rule").
    pub fn insert_rule(&self, rule: &str, index: usize) -> Result<CssRule, InsertRuleError> {
        let rules = insert_rule(self.child_rules().unwrap_or_default(), rule, index)?;
        Ok(self.with_child_rules(rules))
    }

    /// The grouping rule's `deleteRule(index)`: this rule without the rule
    /// at `index`; `None` if there is none (`IndexSizeError`).
    pub fn delete_rule(&self, index: usize) -> Option<CssRule> {
        let rules = delete_rule(self.child_rules().unwrap_or_default(), index)?;
        Some(self.with_child_rules(rules))
    }

    /// This grouping rule with `new` where `old` (the same rule) was;
    /// `None` if `old` isn't one of its rules.
    pub fn replace_rule(&self, old: &CssRule, new: CssRule) -> Option<CssRule> {
        let rules = replace_rule(self.child_rules()?, old, new)?;
        Some(self.with_child_rules(rules))
    }
}

/// A grouping rule's block as Blink's `AppendCSSTextForItems` writes it:
/// ` {`, each rule on a line of its own indented by two spaces, `}`.
fn write_child_rules(rules: &[CssRule], out: &mut String) {
    out.push_str(" {\n");
    for rule in rules {
        let text = rule.css_text();
        if !text.is_empty() {
            out.push_str("  ");
            out.push_str(&text);
            out.push('\n');
        }
    }
    out.push('}');
}

/// `rules` with `rule` parsed and put at `index`, the index checked first
/// as Blink does.
fn insert_rule(
    rules: &[CssRule],
    rule: &str,
    index: usize,
) -> Result<Vec<CssRule>, InsertRuleError> {
    if index > rules.len() {
        return Err(InsertRuleError::IndexSize);
    }
    let rule = parse_rule(rule).ok_or(InsertRuleError::Syntax)?;
    let mut rules = rules.to_vec();
    rules.insert(index, rule);
    Ok(rules)
}

fn delete_rule(rules: &[CssRule], index: usize) -> Option<Vec<CssRule>> {
    if index >= rules.len() {
        return None;
    }
    let mut rules = rules.to_vec();
    rules.remove(index);
    Some(rules)
}

fn replace_rule(rules: &[CssRule], old: &CssRule, new: CssRule) -> Option<Vec<CssRule>> {
    let index = rules.iter().position(|rule| rule.same(old))?;
    let mut rules = rules.to_vec();
    rules[index] = new;
    Some(rules)
}

/// A style rule: its selectors and its declarations (Blink's `StyleRule`).
/// A `CSSStyleRule` holds one, and keeps it when it is taken out of its
/// sheet; changing it makes another (rules are shared, never changed in
/// place).
pub struct StyleRule {
    pub selectors: SelectorList<Impl>,
    pub block: CssPropertyValueSet,
}

impl StyleRule {
    /// `selectorText`: its selector list, serialized.
    pub fn selector_text(&self) -> String {
        self.selectors.to_css_string()
    }

    /// Each selector of its list, serialized, with its specificity as
    /// (ids, classes and the like, elements): DevTools' `SelectorList`
    /// (Blink's `InspectorStyleSheet::BuildObjectForSelectorList`).
    pub fn selectors_for_inspector(&self) -> Vec<(String, [u32; 3])> {
        // How `selectors` packs a specificity: ten bits each, the ids
        // highest.
        const TEN_BITS: u32 = (1 << 10) - 1;
        self.selectors
            .slice()
            .iter()
            .map(|selector| {
                let specificity = selector.specificity();
                (
                    selector.to_css_string(),
                    [
                        specificity >> 20,
                        (specificity >> 10) & TEN_BITS,
                        specificity & TEN_BITS,
                    ],
                )
            })
            .collect()
    }

    /// `cssText` (Blink's `CSSStyleRule::cssText`): `.a { color: red; }`,
    /// and `.a { }` with no declarations.
    pub fn css_text(&self) -> String {
        let declarations = self.block.css_text();
        if declarations.is_empty() {
            format!("{} {{ }}", self.selector_text())
        } else {
            format!("{} {{ {declarations} }}", self.selector_text())
        }
    }

    /// Its declarations, for `CSSStyleRule.style`.
    pub fn style(&self) -> &CssPropertyValueSet {
        &self.block
    }

    /// The rule with `text` as its selectors; `None` if `text` isn't a
    /// valid selector list (setting `selectorText` then does nothing).
    pub fn with_selector_text(&self, text: &str) -> Option<Self> {
        let selectors = crate::selector::parse_selector_list(text)?;
        Some(Self {
            selectors,
            block: self.block.clone(),
        })
    }

    /// The rule with `block` as its declarations.
    pub fn with_style(&self, block: CssPropertyValueSet) -> Self {
        Self {
            selectors: self.selectors.clone(),
            block,
        }
    }
}

/// A stylesheet's rules, parsed (Blink's `StyleSheetContents`): what a
/// `CSSStyleSheet` that script made carries, and what `style`'s
/// `StyleEngine` cascades when the sheet is adopted. Never changed in place: an edit
/// makes new contents, sharing the rules it didn't touch, so another `Arc`
/// is another version.
///
/// Plain data, and it must stay so: no GC pointers or roots, no `Drop` of
/// its own, no global or thread-local caches. `dom` keeps it type-erased in
/// a GC object, whose finalizer drops it.
pub struct StyleSheetContents {
    rules: Vec<CssRule>,
    /// What its `url()`s resolved in as they were parsed (a constructed
    /// sheet's `baseURL`, a `<style>`'s document's base URL), and those of
    /// rules script puts in it later; `None` if there was none, which
    /// leaves them as written.
    url_context: Option<UrlContext>,
    /// The rules whose conditions held by bucket, made the first time a
    /// document cascades it, and shared by every document that adopts it;
    /// made again when one cascades it in an environment where one of its
    /// media queries comes out otherwise (Blink's
    /// `StyleSheetContents::EnsureRuleSet`). Locked only to look at it or
    /// put a new one in, never while dropping.
    rule_set: Mutex<Option<Arc<RuleSet>>>,
}

// `dom` holds it across threads' reach (the GC's marker), and may drop it
// from a finalizer: no `Rc`, `Cell`, raw pointers or roots anywhere inside.
const _: () = {
    const fn plain_data<T: Send + Sync + 'static>() {}
    plain_data::<StyleSheetContents>();
};

/// Why `insertRule` failed (CSSOM's "insert a CSS rule").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertRuleError {
    /// The index is past the end (`IndexSizeError`).
    IndexSize,
    /// Not exactly one rule machopo parses (`SyntaxError`): every at-rule but
    /// `@media`, `@supports` and `@keyframes`, `@import` included, is
    /// one.
    Syntax,
}

impl StyleSheetContents {
    /// `replace` / `replaceSync`'s parse: every rule of `css`. At-rules but
    /// `@media`, `@supports` and `@keyframes` are dropped (constructed sheets drop
    /// `@import`; machopo drops the rest everywhere), as are rules whose
    /// selector doesn't parse.
    pub fn parse(css: &str, url_context: Option<UrlContext>) -> Self {
        let mut log = ParseLog::default();
        let mut sheet = Stylesheet::parse(css, Origin::Author, &mut log);
        log.report();
        if let Some(context) = &url_context {
            resolve_rule_urls(&mut sheet.rules, context);
        }
        Self {
            rules: sheet.rules,
            url_context,
            rule_set: Mutex::new(None),
        }
    }

    pub fn rules(&self) -> &[CssRule] {
        &self.rules
    }

    pub fn base_url(&self) -> Option<&url::Url> {
        self.url_context.as_ref().map(|context| &context.base)
    }

    pub fn url_context(&self) -> Option<&UrlContext> {
        self.url_context.as_ref()
    }

    /// Its rules whose conditions hold in `environment` by bucket, for the
    /// cascade: the set made last time, unless one of its media queries
    /// comes out otherwise in `environment`.
    pub fn rule_set(&self, environment: &MediaEnvironment) -> Arc<RuleSet> {
        let mut cached = self.rule_set.lock().unwrap_or_else(|e| e.into_inner());
        match &*cached {
            Some(set) if !set.did_media_query_results_change(environment) => set.clone(),
            _ => {
                let mut set = RuleSet::default();
                set.add_rules(&self.rules, environment, 0);
                let set = Arc::new(set);
                *cached = Some(set.clone());
                set
            }
        }
    }

    /// `insertRule(rule, index)`: these contents with `rule` parsed and put
    /// at `index`. The index is checked first, as Blink does.
    pub fn insert_rule(&self, rule: &str, index: usize) -> Result<Self, InsertRuleError> {
        Ok(self.with_rules(insert_rule(&self.rules, rule, index)?, Some(index)))
    }

    /// `deleteRule(index)`: these contents without the rule at `index`;
    /// `None` if there is none (`IndexSizeError`).
    pub fn delete_rule(&self, index: usize) -> Option<Self> {
        Some(self.with_rules(delete_rule(&self.rules, index)?, None))
    }

    /// These contents with `new` where `old` (the same rule) was: a rule
    /// changed. `None` if `old` isn't one of its rules.
    pub fn replace_rule(&self, old: &CssRule, new: CssRule) -> Option<Self> {
        let index = self.rules.iter().position(|rule| rule.same(old))?;
        let mut rules = self.rules.clone();
        rules[index] = new;
        Some(self.with_rules(rules, Some(index)))
    }

    /// Contents of `rules`, the `url()`s of the one at `changed` (just put
    /// in) resolved: the others' were when they came in, so an edit of a
    /// large sheet doesn't look through each of its rules again.
    fn with_rules(&self, mut rules: Vec<CssRule>, changed: Option<usize>) -> Self {
        if let (Some(context), Some(index)) = (&self.url_context, changed) {
            resolve_rule_urls(&mut rules[index..=index], context);
        }
        Self {
            rules,
            url_context: self.url_context.clone(),
            rule_set: Mutex::new(None),
        }
    }
}

thread_local! {
    /// How many rules `resolve_rule_urls` looked at (those in group rules
    /// whose `url()`s it resolved too). Counted in debug builds only.
    static RULES_RESOLVED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many rules, on this thread, had their `url()`s looked at to be
/// resolved: those of a sheet parsed, then only the one an edit puts in.
/// `None` in release builds. For tests.
pub fn rules_resolved_for_testing() -> Option<usize> {
    cfg!(debug_assertions).then(|| RULES_RESOLVED.get())
}

/// Resolves the `url()`s of `rules` against `base`, those not resolved
/// yet: a rule with none keeps its `Arc` (rules are shared between
/// versions of a sheet).
fn resolve_rule_urls(rules: &mut [CssRule], base: &UrlContext) {
    if cfg!(debug_assertions) {
        RULES_RESOLVED.set(RULES_RESOLVED.get().wrapping_add(rules.len()));
    }
    for rule in rules {
        match rule {
            CssRule::Style(style) if style.block.has_unresolved_urls() => {
                let mut block = style.block.clone();
                block.resolve_urls(base);
                *style = Arc::new(style.with_style(block));
            }
            CssRule::Keyframe(keyframe) if keyframe.block.has_unresolved_urls() => {
                let mut block = keyframe.block.clone();
                block.resolve_urls(base);
                *rule = rule.with_keyframe_block(block);
            }
            CssRule::Media(media) if has_unresolved_urls(&media.rules) => {
                let mut rules = media.rules.clone();
                resolve_rule_urls(&mut rules, base);
                *rule = rule.with_child_rules(rules);
            }
            CssRule::Supports(supports) if has_unresolved_urls(&supports.rules) => {
                let mut rules = supports.rules.clone();
                resolve_rule_urls(&mut rules, base);
                *rule = rule.with_child_rules(rules);
            }
            CssRule::Keyframes(keyframes) if has_unresolved_urls(&keyframes.rules) => {
                let mut rules = keyframes.rules.clone();
                resolve_rule_urls(&mut rules, base);
                *rule = rule.with_child_rules(rules);
            }
            _ => {}
        }
    }
}

/// Whether some rule of `rules` has a `url()` not resolved yet.
fn has_unresolved_urls(rules: &[CssRule]) -> bool {
    rules.iter().any(|rule| match rule {
        CssRule::Style(style) => style.block.has_unresolved_urls(),
        CssRule::Keyframe(keyframe) => keyframe.block.has_unresolved_urls(),
        CssRule::Media(media) => has_unresolved_urls(&media.rules),
        CssRule::Supports(supports) => has_unresolved_urls(&supports.rules),
        CssRule::Keyframes(keyframes) => has_unresolved_urls(&keyframes.rules),
    })
}

/// CSS's "parse a rule": exactly one rule, white space around it allowed;
/// `None` for anything else, at-rules machopo drops included.
fn parse_rule(css: &str) -> Option<CssRule> {
    let mut log = ParseLog::default();
    let mut input = ParserInput::new(css);
    let mut input = Parser::new(&mut input);
    let rule = cssparser::parse_one_rule(
        &mut input,
        &mut TopLevelParser {
            log: &mut log,
            user_agent: false,
            recorder: None,
        },
    )
    .ok();
    log.report();
    rule
}

pub struct Stylesheet {
    pub origin: Origin,
    pub rules: Vec<CssRule>,
    /// What its `url()`s resolved against (`resolve_urls`): its own URL,
    /// for a linked sheet; the document's base URL as it was parsed, for
    /// one in the document; `None` for the user agent's.
    pub base_url: Option<url::Url>,
}

impl Stylesheet {
    pub fn parse(css: &str, origin: Origin, log: &mut ParseLog) -> Self {
        let mut input = ParserInput::new(css);
        let mut input = Parser::new(&mut input);
        Self {
            origin,
            rules: parse_rule_list(&mut input, log, origin == Origin::UserAgent, None),
            base_url: None,
        }
    }

    /// Resolves its `url()`s in `context`, as it was parsed.
    pub fn resolve_urls(&mut self, context: UrlContext) {
        resolve_rule_urls(&mut self.rules, &context);
        self.base_url = Some(context.base);
    }

    /// Its style rules, and those of its group rules, in order, whatever
    /// their conditions.
    pub fn style_rules(&self) -> impl Iterator<Item = &Arc<StyleRule>> {
        let mut stack = vec![self.rules.iter()];
        std::iter::from_fn(move || {
            loop {
                let rule = match stack.last_mut()?.next() {
                    Some(rule) => rule,
                    None => {
                        stack.pop();
                        continue;
                    }
                };
                match rule {
                    CssRule::Style(rule) => return Some(rule),
                    CssRule::Media(media) => stack.push(media.rules.iter()),
                    CssRule::Supports(supports) => stack.push(supports.rules.iter()),
                    CssRule::Keyframes(_) | CssRule::Keyframe(_) => {}
                }
            }
        })
    }
}

/// The rules of a sheet, or of a group rule's block (the UA
/// stylesheet's, if `user_agent`); where each one kept is in the text told
/// `recorder`, if given.
pub(crate) fn parse_rule_list(
    input: &mut Parser<'_, '_>,
    log: &mut ParseLog,
    user_agent: bool,
    recorder: Option<&mut Recorder>,
) -> Vec<CssRule> {
    let mut parser = TopLevelParser {
        log,
        user_agent,
        recorder,
    };
    let mut rules = Vec::new();
    let mut dropped = 0;
    let mut sheet = StyleSheetParser::new(input, &mut parser);
    while let Some(result) = sheet.next() {
        match result {
            Ok(rule) => {
                rules.push(rule);
                // Where it is: what its block's parse left.
                if let Some(recorder) = sheet.parser.recorder.as_deref_mut()
                    && let Some(source) = recorder.done.take()
                {
                    recorder.rules.push(source);
                }
            }
            Err((_, source)) => {
                let what = if source.starts_with('@') {
                    "at-rule"
                } else {
                    "rule with an unsupported selector"
                };
                tracing::debug!(source, "dropped {what}");
                dropped = u32::saturating_add(dropped, 1);
            }
        }
    }
    // A count for the log: saturating is as good as exact.
    parser.log.dropped_rules = parser.log.dropped_rules.saturating_add(dropped);
    rules
}

/// What parsing dropped, logged at debug level after each style recalc: a
/// reftest that fails may be using something we don't support.
#[derive(Debug, Default)]
pub struct ParseLog {
    /// By property name.
    pub unsupported_properties: BTreeMap<String, u32>,
    /// Supported properties with values we can't parse, by property name.
    pub invalid_values: BTreeMap<String, u32>,
    pub dropped_rules: u32,
}

impl ParseLog {
    pub fn report(&self) {
        for (property, count) in &self.unsupported_properties {
            tracing::debug!(property, count, "unsupported CSS property ignored");
        }
        for (property, count) in &self.invalid_values {
            tracing::debug!(property, count, "unsupported or invalid CSS value ignored");
        }
        if self.dropped_rules > 0 {
            tracing::debug!(count = self.dropped_rules, "CSS rules dropped");
        }
    }
}

struct TopLevelParser<'a> {
    log: &'a mut ParseLog,
    /// Whether it parses the UA stylesheet.
    user_agent: bool,
    /// What is told where the rules are, for DevTools: none but when it
    /// asks.
    recorder: Option<&'a mut Recorder>,
}

impl<'i> QualifiedRuleParser<'i> for TopLevelParser<'_> {
    type Prelude = SelectorList<Impl>;
    type QualifiedRule = CssRule;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        if let Some(recorder) = self.recorder.as_deref_mut() {
            recorder.prelude_start = input.position().byte_index();
        }
        let parser = if self.user_agent {
            SelectorParser::USER_AGENT
        } else {
            SelectorParser::AUTHOR
        };
        crate::selector::parse_list(&parser, input)
    }

    fn parse_block<'t>(
        &mut self,
        selectors: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let start = input.position().byte_index();
        let block = parse_declaration_list(input, self.log);
        if let Some(recorder) = self.recorder.as_deref_mut() {
            let end = input.position().byte_index();
            recorder.done = Some(recorder.rule(SourceRuleKind::Style, start, end));
        }
        Ok(CssRule::Style(Arc::new(StyleRule { selectors, block })))
    }
}

/// An at-rule's prelude, of those kept.
enum AtRulePrelude {
    Media(MediaQueryList),
    /// The condition as written, and whether it holds.
    Supports(String, bool),
    /// The name.
    Keyframes(Arc<str>),
}

/// `@media`, `@supports` and `@keyframes`; every other at-rule is
/// rejected.
impl<'i> AtRuleParser<'i> for TopLevelParser<'_> {
    type Prelude = AtRulePrelude;
    type AtRule = CssRule;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        let start = input.position();
        if let Some(recorder) = self.recorder.as_deref_mut() {
            recorder.prelude_start = start.byte_index();
        }
        match_ignore_ascii_case! { &name,
            // A query that doesn't parse is `not all`: the rule stays.
            "media" => Ok(AtRulePrelude::Media(MediaQueryList::parse_from(input))),
            // A condition that doesn't parse drops the rule.
            "supports" => match crate::css_namespace::parse_supports_condition(input) {
                Ok(supported) => Ok(AtRulePrelude::Supports(
                    simplify_white_space(input.slice_from(start)),
                    supported,
                )),
                Err(_) => Err(input.new_error(BasicParseErrorKind::AtRuleBodyInvalid)),
            },
            "keyframes" => {
                let name = parse_keyframes_name(input)
                    .map_err(|_| input.new_error(BasicParseErrorKind::AtRuleBodyInvalid))?;
                Ok(AtRulePrelude::Keyframes(name))
            },
            _ => Err(input.new_error(BasicParseErrorKind::AtRuleInvalid(name))),
        }
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        let start = input.position().byte_index();
        if let AtRulePrelude::Keyframes(name) = prelude {
            let (rules, source) = match self.recorder.as_deref_mut() {
                // Its keyframes' places, apart from those of the list it is
                // in.
                Some(recorder) => {
                    let mut source = recorder.rule(SourceRuleKind::Keyframes, start, start);
                    let outer = std::mem::take(&mut recorder.rules);
                    let rules = parse_keyframe_list(input, self.log, Some(&mut *recorder));
                    source.rules = std::mem::replace(&mut recorder.rules, outer);
                    source.block.end = input.position().byte_index();
                    (rules, Some(source))
                }
                None => (parse_keyframe_list(input, self.log, None), None),
            };
            if let Some(recorder) = self.recorder.as_deref_mut() {
                recorder.done = source;
            }
            return Ok(CssRule::Keyframes(Arc::new(KeyframesRule { name, rules })));
        }
        // Its rules' places, apart from those of the list it is in.
        let (rules, sources) = match self.recorder.as_deref_mut() {
            Some(recorder) => {
                let mut source = recorder.rule(
                    match prelude {
                        AtRulePrelude::Media(_) => SourceRuleKind::Media,
                        _ => SourceRuleKind::Supports,
                    },
                    start,
                    start,
                );
                let outer = std::mem::take(&mut recorder.rules);
                let rules = parse_rule_list(input, self.log, self.user_agent, Some(&mut *recorder));
                source.rules = std::mem::replace(&mut recorder.rules, outer);
                source.block.end = input.position().byte_index();
                (rules, Some(source))
            }
            None => (
                parse_rule_list(input, self.log, self.user_agent, None),
                None,
            ),
        };
        if let Some(recorder) = self.recorder.as_deref_mut() {
            recorder.done = sources;
        }
        Ok(match prelude {
            AtRulePrelude::Media(media) => CssRule::Media(Arc::new(MediaRule {
                media: Arc::new(media),
                rules,
            })),
            AtRulePrelude::Supports(condition, supported) => {
                CssRule::Supports(Arc::new(SupportsRule {
                    condition,
                    supported,
                    rules,
                }))
            }
            AtRulePrelude::Keyframes(_) => unreachable!("parsed above"),
        })
    }
}

/// `<keyframes-name>`: a `<custom-ident>` (not `none`) or a string, not
/// empty (`cssparser` checks that nothing follows it in the prelude).
fn parse_keyframes_name<'i>(input: &mut Parser<'i, '_>) -> Result<Arc<str>, ParseError<'i, ()>> {
    let location = input.current_source_location();
    Ok(match input.next()? {
        cssparser::Token::Ident(ident)
            if !ident.eq_ignore_ascii_case("none") && !is_reserved(ident) =>
        {
            Arc::from(&**ident)
        }
        cssparser::Token::QuotedString(name) if !name.is_empty() => Arc::from(&**name),
        _ => return Err(location.new_custom_error(())),
    })
}

/// A `@keyframes`' block: its keyframes, each a [`CssRule::Keyframe`]. A
/// keyframe whose selector doesn't parse is dropped, as are at-rules.
fn parse_keyframe_list(
    input: &mut Parser<'_, '_>,
    log: &mut ParseLog,
    recorder: Option<&mut Recorder>,
) -> Vec<CssRule> {
    let mut parser = KeyframeListParser { log, recorder };
    let mut rules = Vec::new();
    let mut dropped = 0;
    let mut list = StyleSheetParser::new(input, &mut parser);
    while let Some(result) = list.next() {
        match result {
            Ok(rule) => {
                rules.push(rule);
                if let Some(recorder) = list.parser.recorder.as_deref_mut()
                    && let Some(source) = recorder.done.take()
                {
                    recorder.rules.push(source);
                }
            }
            Err((_, source)) => {
                tracing::debug!(source, "dropped keyframe");
                dropped = u32::saturating_add(dropped, 1);
            }
        }
    }
    // A count for the log: saturating is as good as exact.
    parser.log.dropped_rules = parser.log.dropped_rules.saturating_add(dropped);
    rules
}

/// `<keyframe-selector>#`: each `from`, `to` or a percentage from 0% to
/// 100% (a math function's too), as an offset from 0 to 1.
pub(crate) fn parse_keyframe_selectors<'i>(
    input: &mut Parser<'i, '_>,
) -> Result<Vec<f32>, ParseError<'i, ()>> {
    input.parse_comma_separated(|input| {
        let location = input.current_source_location();
        let percent = if let Ok(percent) = input.try_parse(crate::calc::parse_percentage) {
            percent
        } else {
            match input.next()? {
                cssparser::Token::Ident(ident) if ident.eq_ignore_ascii_case("from") => 0.0,
                cssparser::Token::Ident(ident) if ident.eq_ignore_ascii_case("to") => 100.0,
                cssparser::Token::Percentage { unit_value, .. } => unit_value * 100.0,
                _ => return Err(location.new_custom_error(())),
            }
        };
        if !(0.0..=100.0).contains(&percent) {
            return Err(location.new_custom_error(()));
        }
        Ok(percent / 100.0)
    })
}

/// A keyframe's text (`keyText`) parsed; `None` if it isn't valid.
pub fn parse_key_text(text: &str) -> Option<Vec<f32>> {
    let mut input = ParserInput::new(text);
    let mut input = Parser::new(&mut input);
    input.parse_entirely(parse_keyframe_selectors).ok()
}

/// One keyframe, white space around it allowed; `None` for anything else
/// (another keyframe or an at-rule after it, a selector that doesn't
/// parse).
fn parse_keyframe(css: &str) -> Option<CssRule> {
    let mut log = ParseLog::default();
    let mut input = ParserInput::new(css);
    let mut input = Parser::new(&mut input);
    let rule =
        cssparser::parse_one_rule(
            &mut input,
            &mut KeyframeListParser {
                log: &mut log,
                recorder: None,
            },
        )
        .ok();
    log.report();
    rule
}

struct KeyframeListParser<'a> {
    log: &'a mut ParseLog,
    /// What is told where the keyframes are, for DevTools.
    recorder: Option<&'a mut Recorder>,
}

impl<'i> QualifiedRuleParser<'i> for KeyframeListParser<'_> {
    type Prelude = Vec<f32>;
    type QualifiedRule = CssRule;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, ()>> {
        if let Some(recorder) = self.recorder.as_deref_mut() {
            recorder.prelude_start = input.position().byte_index();
        }
        parse_keyframe_selectors(input)
    }

    fn parse_block<'t>(
        &mut self,
        offsets: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, ()>> {
        let start = input.position().byte_index();
        let mut parser = DeclarationListParser {
            parsed: Vec::new(),
            log: self.log,
            keyframe: true,
        };
        for _ in RuleBodyParser::new(input, &mut parser) {}
        if let Some(recorder) = self.recorder.as_deref_mut() {
            let end = input.position().byte_index();
            recorder.done = Some(recorder.rule(SourceRuleKind::Keyframe, start, end));
        }
        Ok(CssRule::Keyframe(Arc::new(KeyframeRule {
            offsets,
            block: CssPropertyValueSet::from_parsed(parser.parsed),
        })))
    }
}

/// No at-rules in a `@keyframes` (the defaults reject them).
impl<'i> AtRuleParser<'i> for KeyframeListParser<'_> {
    type Prelude = ();
    type AtRule = CssRule;
    type Error = ();
}

/// Whether a keyframe drops `value`: the `animation-*` longhands but
/// `animation-timing-function`, and all of what the `animation` shorthand
/// sets (Blink's `CSSProperty::IsValidForKeyframe`).
fn is_invalid_for_keyframe(value: &PropertyValue) -> bool {
    use LonghandId::*;
    if value.shorthand == Some(Shorthand::Animation) {
        return true;
    }
    let crate::property_set::PropertyDeclaration::Longhand(declaration) = &value.declaration else {
        return false;
    };
    matches!(
        declaration.id,
        AnimationDuration
            | AnimationDelay
            | AnimationIterationCount
            | AnimationDirection
            | AnimationFillMode
            | AnimationPlayState
            | AnimationName
            | AnimationTimeline
            | AnimationRangeStart
            | AnimationRangeEnd
    )
}

/// `text` with each run of white space one space, none at either end
/// (Blink's `SimplifyWhiteSpace`).
fn simplify_white_space(text: &str) -> String {
    text.split(|c: char| c.is_ascii_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) struct DeclarationListParser<'a> {
    /// In source order, duplicates and all.
    pub(crate) parsed: Vec<PropertyValue>,
    pub(crate) log: &'a mut ParseLog,
    /// A keyframe's block: `!important` declarations, and those
    /// [`is_invalid_for_keyframe`], are dropped.
    pub(crate) keyframe: bool,
}

impl<'i> DeclarationParser<'i> for DeclarationListParser<'_> {
    type Declaration = ();
    type Error = DeclarationError;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<(), ParseError<'i, DeclarationError>> {
        match parse_property(&name, input) {
            Ok(values) => {
                let dropped = self.keyframe
                    && values
                        .iter()
                        .any(|value| value.important || is_invalid_for_keyframe(value));
                if !dropped {
                    self.parsed.extend(values);
                }
                Ok(())
            }
            Err(e) => {
                let log = match e.kind {
                    ParseErrorKind::Custom(DeclarationError::UnsupportedProperty) => {
                        &mut self.log.unsupported_properties
                    }
                    _ => &mut self.log.invalid_values,
                };
                let count = log.entry(name.to_ascii_lowercase()).or_default();
                *count = count.saturating_add(1);
                Err(e)
            }
        }
    }
}

/// No nested rules or at-rules in a declaration list (the defaults reject
/// them).
impl<'i> AtRuleParser<'i> for DeclarationListParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = DeclarationError;
}

impl<'i> QualifiedRuleParser<'i> for DeclarationListParser<'_> {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = DeclarationError;
}

impl<'i> RuleBodyItemParser<'i, (), DeclarationError> for DeclarationListParser<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}
