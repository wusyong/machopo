//! CSSOM's objects besides the sheets (`style_sheet`): a sheet's rules'
//! objects (Blink's `CSSRule` and its kinds: one type here, the kind its
//! rule's), their lists (`CSSRuleList`), media lists (`MediaList`), a tree
//! scope's sheets (`StyleSheetList`); and the sheets of `<style>`s and
//! `<link>`s (Blink's `StyleElement::sheet_`, `LinkStyle::sheet_`), which
//! their elements keep.
//!
//! A sheet's rules are `css::StyleSheetContents`, never changed in place:
//! an edit (`insertRule`, a rule's `selectorText`, `style` or `media`)
//! makes new contents, sharing the rules it didn't touch (Blink copies a
//! shared `StyleSheetContents` before changing it). A rule in a group rule
//! changed makes its group rule new too, and so on up to the sheet's
//! contents: each object on the way takes its new rule. A rule's objects
//! are made when script first asks for one of its parent's (the sheet's,
//! or its group rule's: every one of them then), kept at their rules'
//! positions (Blink's `child_rule_cssom_wrappers_`), so `cssRules[0]` is
//! the same object each time; inserting or deleting one moves the later
//! ones along. One taken out keeps its rule, with no parent and no sheet.
//!
//! An element keeps the sheet made for it last; the sheet keeps the
//! element (`ownerNode`) while it is its sheet. It is made when style or
//! script first needs it (`owner_sheet`): a `<style>`'s from its child
//! text, against its document's URL; a `<link>`'s as the host loaded it
//! (`set_link_sheet`). Once a `<style>`'s children changed (every change
//! of its text is one), or the element is taken out of its document or is
//! no longer of a sheet, it is no longer its sheet, and the next ask makes
//! a new one (HTML's "update a style block": Blink makes a new
//! `CSSStyleSheet`), a `<link>`'s of the contents it loaded. A change of its `media` attribute is the same
//! sheet's new media. The `<style>`s of a document with the same text
//! share their contents (Blink's `StyleEngine::text_to_sheet_cache_`),
//! weakly: the shadow trees of a component each have one, whose rules
//! style indexes once.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use css::media::MediaQueryList;
use css::{CssRule as Rule, StyleSheetContents};
use elise::{External, Gc, GcObject, HeapVector, JsEdge, Member};

use crate::{
    CssStyleDeclaration, CssStyleSheet, Cx, Node, NodeKind, NodeRef, QualifiedName, names, sites,
};

/// Where a sheet came from: constructed by script; an element's, a
/// `<style>`'s child text or a `<link>`'s loaded contents (what a new one
/// of it starts from).
pub(crate) enum SheetSource {
    Constructed,
    Inline,
    Loaded(Arc<StyleSheetContents>),
}

/// The contents of a document's `<style>`s by their text, weakly (a text
/// is the page's: SipHash), and the count past which the gone ones are
/// let go of.
#[derive(Default)]
pub(crate) struct SheetTextCache {
    texts: HashMap<Box<str>, Weak<StyleSheetContents>>,
    prune_at: usize,
}

/// A rule's object: its rule now (an edit replaces it; in a `Cell`, taken
/// and put back), its sheet and its group rule (none: taken out, or a
/// sheet's own), its rules' objects and `cssRules` (a group rule's, a
/// `@keyframes`'), its `style` (a style rule's, a keyframe's) and `media`
/// (a media rule's).
#[derive(GcObject)]
pub struct CssRule {
    rule: External<Cell<Option<Rule>>>,
    sheet: Member<CssStyleSheet>,
    parent: Member<CssRule>,
    children: Member<HeapVector<CssRule>>,
    list: Member<CssRuleList>,
    style: Member<CssStyleDeclaration>,
    media: Member<MediaList>,
    wrapper: JsEdge,
}

/// A `cssRules`: a sheet's or a rule's.
#[derive(GcObject)]
pub struct CssRuleList {
    sheet: Member<CssStyleSheet>,
    rule: Member<CssRule>,
    wrapper: JsEdge,
}

/// A `media`: a sheet's or a media rule's.
#[derive(GcObject)]
pub struct MediaList {
    sheet: Member<CssStyleSheet>,
    rule: Member<CssRule>,
    wrapper: JsEdge,
}

/// A `styleSheets`: a document's or a shadow root's.
#[derive(GcObject)]
pub struct StyleSheetList {
    scope: Member<Node>,
    wrapper: JsEdge,
}

/// What kind of rule a rule's object is (its interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Style,
    Media,
    Supports,
    Keyframes,
    Keyframe,
}

/// Whose rules: a sheet's, or a rule's (a group rule's, a `@keyframes`').
#[derive(Clone, Copy)]
pub enum RuleParent<'gc> {
    Sheet(Gc<'gc, CssStyleSheet>),
    Rule(Gc<'gc, CssRule>),
}

/// A parent's rules after an edit `css` made: a sheet's contents, a
/// rule's rule.
pub enum Rules {
    Sheet(StyleSheetContents),
    Rule(Rule),
}

/// Whose `media` a media list is.
#[derive(Clone, Copy)]
pub enum MediaOwner<'gc> {
    Sheet(Gc<'gc, CssStyleSheet>),
    Rule(Gc<'gc, CssRule>),
}

impl<'gc> Cx<'_, 'gc> {
    // --- Which elements have a sheet.

    /// Whether `node` is a `<style>`, HTML's or SVG's.
    pub fn is_style_element(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node).is_some_and(|name| {
            name.local() == names::local::STYLE
                && (name.namespace() == names::HTML_NAMESPACE
                    || name.namespace() == names::SVG_NAMESPACE)
        })
    }

    /// Whether `node` is an HTML `<link>`.
    pub fn is_link_element(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node) == Some(QualifiedName::html(names::local::LINK))
    }

    /// Whether `node`'s `type` is CSS's: none, empty, or `text/css`.
    fn is_css_type(&self, node: NodeRef<'gc>) -> bool {
        self.get_attribute(node, "type").is_none_or(|kind| {
            let kind = kind.to_str();
            kind.is_empty() || kind.trim().eq_ignore_ascii_case("text/css")
        })
    }

    /// Whether `rel` (a `<link>`'s) has `keyword`, ASCII case-insensitively.
    fn link_rel_has(&self, node: NodeRef<'gc>, keyword: &str) -> bool {
        self.get_attribute(node, "rel").is_some_and(|rel| {
            rel.to_str()
                .split_ascii_whitespace()
                .any(|token| token.eq_ignore_ascii_case(keyword))
        })
    }

    /// The `href` of `node` if it is a `<link>` whose sheet is loaded: `rel`
    /// has `stylesheet` (an alternate one too, loaded but applied only if
    /// of the preferred set, as in Chrome), its `type` is CSS's, it isn't
    /// `disabled`, and `href` isn't empty.
    pub fn stylesheet_link_href(&self, node: NodeRef<'gc>) -> Option<String> {
        if !self.is_link_element(node)
            || !self.link_rel_has(node, "stylesheet")
            || !self.is_css_type(node)
            || self.get_attribute(node, "disabled").is_some()
        {
            return None;
        }
        self.get_attribute(node, "href")
            .map(|href| href.to_str().into_owned())
            .filter(|href| !href.trim().is_empty())
    }

    /// Whether `node` is a `<link>` whose `rel` has `alternate`.
    pub fn is_alternate_stylesheet(&self, node: NodeRef<'gc>) -> bool {
        self.is_link_element(node) && self.link_rel_has(node, "alternate")
    }

    /// Whether `node` has a sheet, or will once loaded: a `<style>` of
    /// CSS's `type`, a `<link>` of a sheet (`stylesheet_link_href`).
    pub fn owns_sheet(&self, node: NodeRef<'gc>) -> bool {
        (self.is_style_element(node) && self.is_css_type(node))
            || self.stylesheet_link_href(node).is_some()
    }

    // --- An element's sheet.

    /// `style`, a `<style>` the parser made, has its start tag end on `line`
    /// of its document's markup, where its text starts (DevTools' `CSS`
    /// tells where its sheet's text is from it).
    pub fn set_style_parser_line(&self, style: NodeRef<'gc>, line: u32) {
        self.element_data(style).parser_line.set(line);
    }

    /// The line `style`'s start tag ends on in its document's markup, if
    /// the parser made it (`set_style_parser_line`).
    pub fn style_parser_line(&self, style: NodeRef<'gc>) -> Option<u32> {
        let data = style.data.get(self.gc)?.downcast::<crate::style::ElementData>()?;
        Some(data.parser_line.get()).filter(|&line| line != 0)
    }

    /// `link`'s sheet loaded: `contents`, asked for at `href`, origin-clean
    /// or not (another origin's, not by CORS, isn't: script can't read its
    /// rules). The sheet it had before is no longer its; its tree scope's
    /// sheets are collected again.
    pub fn set_link_sheet(
        &self,
        link: NodeRef<'gc>,
        contents: Arc<StyleSheetContents>,
        href: &str,
        origin_clean: bool,
    ) {
        let data = self.element_data(link);
        if let Some(old) = data.sheet.get(self.gc) {
            old.owner.set(self.gc, None);
        }
        let sheet = self.new_style_sheet(
            self.document_of(link),
            contents.clone(),
            &self.owner_media(link),
            SheetSource::Loaded(contents),
            Some(href),
            origin_clean,
        );
        self.own_sheet(link, sheet);
        self.owner_sheet_changed(link);
    }

    /// `owner`'s `media` attribute (none: the empty string).
    fn owner_media(&self, owner: NodeRef<'gc>) -> String {
        self.get_attribute(owner, "media")
            .map(|media| media.to_str().into_owned())
            .unwrap_or_default()
    }

    /// `owner`'s sheet (LinkStyle's `sheet`): none if it isn't connected or
    /// of a sheet, or is a `<link>` not loaded; the one it has if that is
    /// still its, else a new one.
    pub fn owner_sheet(&self, owner: NodeRef<'gc>) -> Option<Gc<'gc, CssStyleSheet>> {
        let current = self
            .existing_element_data(owner)
            .and_then(|data| data.sheet.get(self.gc));
        if !self.is_connected(owner) || !self.owns_sheet(owner) {
            if let Some(sheet) = current {
                sheet.owner.set(self.gc, None);
            }
            return None;
        }
        if !self.is_style_element(owner) {
            // A `<link>`'s: as loaded; a new one of those contents once it
            // was taken out or wasn't of a sheet.
            let current = current?;
            if current.owner.get(self.gc).is_some() {
                return Some(current);
            }
            let SheetSource::Loaded(contents) = current.into_ref().source.get() else {
                unreachable!("a <link>'s sheet is loaded");
            };
            let sheet = self.new_style_sheet(
                self.document_of(owner),
                contents.clone(),
                &self.owner_media(owner),
                SheetSource::Loaded(contents.clone()),
                self.style_sheet_href(current),
                current.origin_clean.get(),
            );
            return Some(self.own_sheet(owner, sheet));
        }
        match current {
            Some(current) if current.owner.get(self.gc).is_some() => Some(current),
            _ => Some(self.new_style_sheet_of(owner)),
        }
    }

    /// A `<style>`'s sheet of its child text: its first, or one after its
    /// last was no longer its.
    #[cold]
    fn new_style_sheet_of(&self, owner: NodeRef<'gc>) -> Gc<'gc, CssStyleSheet> {
        let document = self.document_of(owner);
        let text = self.child_text_content(owner);
        let sheet = self.new_style_sheet(
            document,
            self.style_text_contents(document, &text.to_str()),
            &self.owner_media(owner),
            SheetSource::Inline,
            None,
            true,
        );
        self.own_sheet(owner, sheet)
    }

    /// `sheet` is `owner`'s now.
    fn own_sheet(
        &self,
        owner: NodeRef<'gc>,
        sheet: Gc<'gc, CssStyleSheet>,
    ) -> Gc<'gc, CssStyleSheet> {
        sheet.owner.set(self.gc, Some(owner));
        self.element_data(owner).sheet.set(self.gc, Some(sheet));
        sheet
    }

    /// The contents of a `<style>` of `document` with `text`: those of one
    /// with that text already, if they are of its base URL now (and its
    /// URL, which DevTools names as asking for their `url()`s); else
    /// parsed against it.
    fn style_text_contents(&self, document: NodeRef<'gc>, text: &str) -> Arc<StyleSheetContents> {
        let url = self.document_base_url(document);
        let referrer = self
            .document_url(document)
            .map(|url| url.to_str().into_owned())
            .or_else(|| url.as_ref().map(url::Url::to_string))
            .map(|url| url.split('#').next().unwrap_or_default().to_owned());
        let data = self.ensure_document_data(document);
        let mut cache = data.sheet_texts.get().take().unwrap_or_default();
        let cached = cache
            .texts
            .get(text)
            .and_then(Weak::upgrade)
            .filter(|contents| {
                contents.base_url() == url.as_ref()
                    && contents.url_context().map(|context| &*context.referrer)
                        == referrer.as_deref()
            });
        let contents = cached.unwrap_or_else(|| {
            let context =
                url.map(|url| css::UrlContext::new(url, referrer.as_deref().unwrap_or("")));
            let contents = Arc::new(StyleSheetContents::parse(text, context));
            if cache.texts.len() >= cache.prune_at {
                cache
                    .texts
                    .retain(|_, contents| contents.strong_count() > 0);
                cache.prune_at = cache.texts.len().saturating_mul(2).max(16);
            }
            cache.texts.insert(text.into(), Arc::downgrade(&contents));
            contents
        });
        data.sheet_texts.get().set(Some(cache));
        contents
    }

    /// `owner` (an element of a sheet) taken out of its document: its sheet
    /// no longer its, a new one made if it is put back.
    pub(crate) fn sheet_owner_removed(&self, owner: NodeRef<'gc>) {
        if let Some(sheet) = self
            .existing_element_data(owner)
            .and_then(|data| data.sheet.get(self.gc))
        {
            sheet.owner.set(self.gc, None);
        }
    }

    /// `owner`'s children changed: a `<style>`'s sheet is no longer its,
    /// whatever its text is now (Blink's `StyleElement::ChildrenChanged`
    /// makes a new one). Every change of its text comes here (a child's
    /// data, a child put in or taken out, while connected; one not
    /// connected has none).
    #[cold]
    pub(crate) fn style_children_changed(&self, owner: NodeRef<'gc>) {
        if self.is_style_element(owner) {
            self.sheet_owner_removed(owner);
        }
    }

    /// `owner`'s `media` attribute is `media` now: its sheet's media, if it
    /// has one (Blink's `HTMLStyleElement::ParseAttribute`).
    pub(crate) fn sheet_owner_media_changed(&self, owner: NodeRef<'gc>, media: &str) {
        if let Some(sheet) = self
            .existing_element_data(owner)
            .and_then(|data| data.sheet.get(self.gc))
            .filter(|sheet| sheet.owner.get(self.gc).is_some())
        {
            self.set_style_sheet_media(sheet, media, Arc::new(MediaQueryList::parse(media)));
        }
    }

    /// `sheet`'s element (`ownerNode`), while it is its sheet: one with an
    /// owner is its sheet now (each place that makes another its, or none,
    /// takes the owner from the one before), unless it is no longer of a
    /// sheet, which `owner_sheet` finds.
    pub fn style_sheet_owner(&self, sheet: Gc<'gc, CssStyleSheet>) -> Option<NodeRef<'gc>> {
        let owner = sheet.owner.get(self.gc)?;
        self.owner_sheet(owner).map(|_| owner)
    }

    /// A `<link>`'s sheet's URL as it asked for it (`href`); none for any
    /// other.
    pub fn style_sheet_href(&self, sheet: Gc<'gc, CssStyleSheet>) -> Option<&'gc str> {
        sheet.into_ref().href.get().as_deref()
    }

    /// `sheet`'s `title`: its element's, while it is its sheet, if not
    /// empty.
    pub fn style_sheet_title(&self, sheet: Gc<'gc, CssStyleSheet>) -> Option<String> {
        let owner = self.style_sheet_owner(sheet)?;
        self.get_attribute(owner, "title")
            .map(|title| title.to_str().into_owned())
            .filter(|title| !title.is_empty())
    }

    /// Whether `sheet` is a constructed one (CSSOM's constructed flag).
    pub fn style_sheet_is_constructed(&self, sheet: Gc<'gc, CssStyleSheet>) -> bool {
        matches!(sheet.into_ref().source.get(), SheetSource::Constructed)
    }

    /// Whether script may read `sheet`'s rules (CSSOM's origin-clean flag).
    pub fn style_sheet_origin_clean(&self, sheet: Gc<'gc, CssStyleSheet>) -> bool {
        sheet.origin_clean.get()
    }

    /// The sheets of the elements of `scope`'s tree (a document's or a
    /// shadow root's, not those in shadow trees in it), in tree order
    /// (`styleSheets`' items).
    pub fn owner_sheets_in(&self, scope: NodeRef<'gc>) -> Vec<Gc<'gc, CssStyleSheet>> {
        let mut sheets = Vec::new();
        let mut node = self.first_child(scope);
        while let Some(at) = node {
            if self.kind(at) == NodeKind::Element
                && let Some(sheet) = self.owner_sheet(at)
            {
                sheets.push(sheet);
            }
            node = self.next_in_scope(at, scope);
        }
        sheets
    }

    /// The node after `node` in tree order, within `root`'s subtree.
    fn next_in_scope(&self, node: NodeRef<'gc>, root: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if let Some(child) = self.first_child(node) {
            return Some(child);
        }
        let mut at = node;
        loop {
            if Gc::ptr_eq(at, root) {
                return None;
            }
            if let Some(next) = self.next_sibling(at) {
                return Some(next);
            }
            at = self.parent(at)?;
        }
    }

    // --- Rules.

    /// `rule`'s rule now.
    pub fn css_rule(&self, rule: Gc<'gc, CssRule>) -> Rule {
        let cell = rule.into_ref().rule.get();
        let taken = cell.take().expect("a rule's object has its rule");
        let copy = taken.clone();
        cell.set(Some(taken));
        copy
    }

    /// Where `parent`'s rules' objects are (none until asked for).
    fn rule_objects_member(&self, parent: RuleParent<'gc>) -> &'gc Member<HeapVector<CssRule>> {
        match parent {
            RuleParent::Sheet(sheet) => &sheet.into_ref().rules,
            RuleParent::Rule(rule) => &rule.into_ref().children,
        }
    }

    /// The rules `parent` has now.
    fn rules_of(&self, parent: RuleParent<'gc>) -> Vec<Rule> {
        match parent {
            RuleParent::Sheet(sheet) => self.style_sheet_contents(sheet).rules().to_vec(),
            RuleParent::Rule(rule) => self
                .css_rule(rule)
                .child_rules()
                .map(<[Rule]>::to_vec)
                .unwrap_or_default(),
        }
    }

    /// `parent`'s rules' objects, made (every one) at the first ask.
    fn rule_objects(&self, parent: RuleParent<'gc>) -> Gc<'gc, HeapVector<CssRule>> {
        let member = self.rule_objects_member(parent);
        if let Some(objects) = member.get(self.gc) {
            return objects;
        }
        let rules = self.rules_of(parent);
        let objects = HeapVector::new(self.gc, rules.len());
        for rule in rules {
            objects.push(self.gc, self.new_rule_object(parent, rule));
        }
        member.set(self.gc, Some(objects));
        objects
    }

    fn new_rule_object(&self, parent: RuleParent<'gc>, rule: Rule) -> Gc<'gc, CssRule> {
        let (sheet, parent) = match parent {
            RuleParent::Sheet(sheet) => (Some(sheet), None),
            RuleParent::Rule(parent) => (parent.sheet.get(self.gc), Some(parent)),
        };
        self.gc.alloc_in(
            &sites::OTHER,
            CssRuleInit {
                rule: External::new(Cell::new(Some(rule))),
                sheet,
                parent,
                children: None,
                list: None,
                style: None,
                media: None,
                wrapper: (),
            },
        )
    }

    /// The object of `parent`'s rule at `index`, the same each time; none
    /// past the end.
    pub fn rule_at(&self, parent: RuleParent<'gc>, index: usize) -> Option<Gc<'gc, CssRule>> {
        // One a rule, in step with them.
        self.rule_objects(parent).get(self.gc, index)
    }

    /// How many rules `parent` has.
    pub fn rule_count(&self, parent: RuleParent<'gc>) -> usize {
        match parent {
            RuleParent::Sheet(sheet) => self.style_sheet_contents(sheet).rules().len(),
            RuleParent::Rule(rule) => self.css_rule(rule).child_rules().map_or(0, <[Rule]>::len),
        }
    }

    /// `parent`'s `cssRules`, the same each time.
    pub fn css_rule_list(&self, parent: RuleParent<'gc>) -> Gc<'gc, CssRuleList> {
        let member = match parent {
            RuleParent::Sheet(sheet) => &sheet.into_ref().rule_list,
            RuleParent::Rule(rule) => &rule.into_ref().list,
        };
        if let Some(list) = member.get(self.gc) {
            return list;
        }
        let (sheet, rule) = match parent {
            RuleParent::Sheet(sheet) => (Some(sheet), None),
            RuleParent::Rule(rule) => (None, Some(rule)),
        };
        let list = self.gc.alloc_in(
            &sites::OTHER,
            CssRuleListInit {
                sheet,
                rule,
                wrapper: (),
            },
        );
        member.set(self.gc, Some(list));
        list
    }

    /// Whose rules `list` shows.
    pub fn rule_list_parent(&self, list: Gc<'gc, CssRuleList>) -> RuleParent<'gc> {
        match (list.sheet.get(self.gc), list.rule.get(self.gc)) {
            (Some(sheet), _) => RuleParent::Sheet(sheet),
            (None, Some(rule)) => RuleParent::Rule(rule),
            (None, None) => unreachable!("a list is a sheet's or a rule's"),
        }
    }

    /// `parent`'s rules are `new` now, with a rule put in at `index`
    /// (CSSOM's "insert a CSS rule"): its object made if the others' are.
    pub fn css_rule_inserted(&self, parent: RuleParent<'gc>, new: Rules, index: usize) {
        self.set_rules(parent, new);
        let Some(objects) = self.rule_objects_member(parent).get(self.gc) else {
            return;
        };
        let rule = self.rules_of(parent).swap_remove(index);
        let made = self.new_rule_object(parent, rule);
        // The later ones moved along one, from the end.
        let len = objects.len();
        let Some(last) = len.checked_sub(1).filter(|&last| index <= last) else {
            objects.push(self.gc, made);
            return;
        };
        objects.push(
            self.gc,
            objects.get(self.gc, last).expect("below the length"),
        );
        for at in (index..last).rev() {
            let moved = objects.get(self.gc, at).expect("below the length");
            // `at` < `last` < `len`.
            objects.set(self.gc, at.saturating_add(1), moved);
        }
        objects.set(self.gc, index, made);
    }

    /// `parent`'s rules are `new` now, its rule at `index` taken out
    /// (CSSOM's "remove a CSS rule"): its object, if made, with no parent
    /// or sheet from then on.
    pub fn css_rule_deleted(&self, parent: RuleParent<'gc>, new: Rules, index: usize) {
        self.set_rules(parent, new);
        if let Some(objects) = self.rule_objects_member(parent).get(self.gc)
            && index < objects.len()
        {
            let removed = objects.remove(self.gc, index);
            self.rule_taken_out(removed);
        }
    }

    /// `rule` (an object) is no longer in its parent: it and the rules in
    /// it have no sheet.
    fn rule_taken_out(&self, rule: Gc<'gc, CssRule>) {
        rule.parent.set(self.gc, None);
        self.rule_sheet_gone(rule);
    }

    fn rule_sheet_gone(&self, rule: Gc<'gc, CssRule>) {
        rule.sheet.set(self.gc, None);
        if let Some(children) = rule.children.get(self.gc) {
            for at in 0..children.len() {
                if let Some(child) = children.get(self.gc, at) {
                    self.rule_sheet_gone(child);
                }
            }
        }
    }

    /// `sheet`'s rules replaced whole (`replaceSync`): its rules' objects
    /// taken out, made again for the new rules when asked for.
    pub(crate) fn sheet_rules_replaced(&self, sheet: Gc<'gc, CssStyleSheet>) {
        if let Some(objects) = sheet.rules.get(self.gc) {
            for at in 0..objects.len() {
                if let Some(rule) = objects.get(self.gc, at) {
                    self.rule_taken_out(rule);
                }
            }
            sheet.rules.set(self.gc, None);
        }
    }

    /// `parent`'s rules are `new` now: a sheet's contents, or a rule's.
    fn set_rules(&self, parent: RuleParent<'gc>, new: Rules) {
        match (parent, new) {
            (RuleParent::Sheet(sheet), Rules::Sheet(contents)) => {
                self.set_style_sheet_rules(sheet, Arc::new(contents));
            }
            (RuleParent::Rule(rule), Rules::Rule(new)) => self.set_css_rule(rule, new),
            _ => unreachable!("a sheet's rules are contents, a rule's a rule"),
        }
    }

    /// `rule`'s kind.
    pub fn css_rule_kind(&self, rule: Gc<'gc, CssRule>) -> RuleKind {
        match self.css_rule(rule) {
            Rule::Style(_) => RuleKind::Style,
            Rule::Media(_) => RuleKind::Media,
            Rule::Supports(_) => RuleKind::Supports,
            Rule::Keyframes(_) => RuleKind::Keyframes,
            Rule::Keyframe(_) => RuleKind::Keyframe,
        }
    }

    /// `rule`'s rule is `new` now (an edit's, which `css` made): its group
    /// rule's too, up to its sheet's contents, which style sees at the
    /// next recalc. One taken out changes alone.
    pub fn set_css_rule(&self, rule: Gc<'gc, CssRule>, new: Rule) {
        let old = rule
            .into_ref()
            .rule
            .get()
            .replace(Some(new.clone()))
            .expect("a rule's object has its rule");
        if let Some(parent) = rule.parent.get(self.gc) {
            if let Some(replaced) = self.css_rule(parent).replace_rule(&old, new) {
                self.set_css_rule(parent, replaced);
            }
        } else if let Some(sheet) = rule.sheet.get(self.gc)
            && let Some(contents) = self.style_sheet_contents(sheet).replace_rule(&old, new)
        {
            self.set_style_sheet_rules(sheet, Arc::new(contents));
        }
    }

    /// `rule`'s group rule (`parentRule`).
    pub fn css_rule_parent(&self, rule: Gc<'gc, CssRule>) -> Option<Gc<'gc, CssRule>> {
        rule.parent.get(self.gc)
    }

    /// `rule`'s sheet (`parentStyleSheet`): none once it, or a group rule
    /// it is in, was taken out.
    pub fn css_rule_sheet(&self, rule: Gc<'gc, CssRule>) -> Option<Gc<'gc, CssStyleSheet>> {
        rule.sheet.get(self.gc)
    }

    /// `rule`'s `style` (a style rule's, a keyframe's), the same each time.
    pub fn css_rule_style(&self, rule: Gc<'gc, CssRule>) -> Gc<'gc, CssStyleDeclaration> {
        if let Some(style) = rule.style.get(self.gc) {
            return style;
        }
        let style = self.new_rule_style_declaration(rule);
        rule.style.set(self.gc, Some(style));
        style
    }

    pub(crate) fn css_rule_wrapper_edge(&self, rule: Gc<'gc, CssRule>) -> &'gc JsEdge {
        &rule.into_ref().wrapper
    }

    pub(crate) fn css_rule_list_wrapper_edge(&self, list: Gc<'gc, CssRuleList>) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    // --- Media lists.

    /// `owner`'s `media`, the same each time.
    pub fn media_list(&self, owner: MediaOwner<'gc>) -> Gc<'gc, MediaList> {
        let member = match owner {
            MediaOwner::Sheet(sheet) => &sheet.into_ref().media_list,
            MediaOwner::Rule(rule) => &rule.into_ref().media,
        };
        if let Some(list) = member.get(self.gc) {
            return list;
        }
        let (sheet, rule) = match owner {
            MediaOwner::Sheet(sheet) => (Some(sheet), None),
            MediaOwner::Rule(rule) => (None, Some(rule)),
        };
        let list = self.gc.alloc_in(
            &sites::OTHER,
            MediaListInit {
                sheet,
                rule,
                wrapper: (),
            },
        );
        member.set(self.gc, Some(list));
        list
    }

    /// Whose `media` `list` is.
    pub fn media_list_owner(&self, list: Gc<'gc, MediaList>) -> MediaOwner<'gc> {
        match (list.sheet.get(self.gc), list.rule.get(self.gc)) {
            (Some(sheet), _) => MediaOwner::Sheet(sheet),
            (None, Some(rule)) => MediaOwner::Rule(rule),
            (None, None) => unreachable!("a media list is a sheet's or a rule's"),
        }
    }

    pub(crate) fn media_list_wrapper_edge(&self, list: Gc<'gc, MediaList>) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    // --- Sheet lists.

    /// `scope`'s (a document's or a shadow root's) `styleSheets`, the same
    /// each time.
    pub fn style_sheet_list(&self, scope: NodeRef<'gc>) -> Gc<'gc, StyleSheetList> {
        let member = match self.kind(scope) {
            NodeKind::Document => &self.ensure_document_data(scope).into_ref().style_sheets,
            _ => {
                &self
                    .shadow_data(scope)
                    .expect("a document or a shadow root")
                    .into_ref()
                    .style_sheets
            }
        };
        if let Some(list) = member.get(self.gc) {
            return list;
        }
        let list = self.gc.alloc_in(
            &sites::OTHER,
            StyleSheetListInit {
                scope: Some(scope),
                wrapper: (),
            },
        );
        member.set(self.gc, Some(list));
        list
    }

    /// `list`'s tree scope.
    pub fn style_sheet_list_scope(&self, list: Gc<'gc, StyleSheetList>) -> NodeRef<'gc> {
        list.scope.get(self.gc).expect("a list's scope")
    }

    pub(crate) fn style_sheet_list_wrapper_edge(
        &self,
        list: Gc<'gc, StyleSheetList>,
    ) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }
}
