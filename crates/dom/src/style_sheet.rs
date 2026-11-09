//! Style sheets (CSSOM's `CSSStyleSheet`, Blink's): constructed ones
//! (`new CSSStyleSheet()`, Blink's `is_constructed_`), and those of
//! `<style>`s and `<link>`s (`cssom`, which their elements keep), and the
//! tree scopes' adopted sheets (`adoptedStyleSheets`, Blink's
//! `TreeScope::adopted_style_sheets_`).
//! A sheet holds its parsed contents (`css::StyleSheetContents`, replaced
//! whole by `replaceSync`, never changed in place: style compares them by
//! their `Arc`), its media and whether it is disabled. It doesn't keep the
//! tree scopes that adopt it (a sheet a page keeps for good would keep
//! every shadow root it was ever adopted by): a sheet whose contents or
//! `disabled` changed goes in a list style takes (`take_changed_style_sheets`),
//! and style, which knows the scopes it collected each sheet for, collects
//! those again. A change of a scope's list marks the scope's sheets as
//! changed, as a `<style>` coming does.

use std::cell::Cell;
use std::sync::Arc;

use css::StyleSheetContents;
use css::media::MediaQueryList;
use elise::{External, Gc, GcObject, HeapVector, JsEdge, Member};

use crate::{Cx, Node, NodeKind, NodeRef, sites};

/// A sheet: its document (a constructed one's constructor document), its
/// contents and its media as written and parsed (another object replaces
/// each: read with no borrow), whether it is disabled, its number (what style
/// tells it by: objects move), and whether it is in the list of changed
/// sheets. Where it came from (constructed, or an element's: `cssom`), the
/// element while it is its sheet, its `href` and whether it is
/// origin-clean (a `<link>`'s). And its rules' objects, its `cssRules`
/// and its `media`'s object, once script asked for them. Whether script
/// edited its rules (DevTools' text of it is its source until then).
#[derive(GcObject)]
pub struct CssStyleSheet {
    document: Member<Node>,
    contents: Member<SheetContents>,
    media: Member<SheetMedia>,
    disabled: Cell<bool>,
    id: Cell<u64>,
    listed: Cell<bool>,
    edited: Cell<bool>,
    wrapper: JsEdge,
    pub(crate) source: External<crate::cssom::SheetSource>,
    pub(crate) owner: Member<Node>,
    pub(crate) href: External<Option<Box<str>>>,
    pub(crate) origin_clean: Cell<bool>,
    pub(crate) rules: Member<HeapVector<crate::cssom::CssRule>>,
    pub(crate) rule_list: Member<crate::cssom::CssRuleList>,
    pub(crate) media_list: Member<crate::cssom::MediaList>,
}

/// A sheet's contents at one time.
#[derive(GcObject)]
pub struct SheetContents {
    contents: External<Arc<StyleSheetContents>>,
}

/// A sheet's media at one time: as written (or serialized, once script
/// changed them) and parsed.
#[derive(GcObject)]
pub struct SheetMedia {
    media: External<(Box<str>, Arc<MediaQueryList>)>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new constructed sheet of `document` (its constructor document),
    /// with `contents`, `media` (as written) and `disabled`.
    pub fn create_style_sheet(
        &self,
        document: NodeRef<'gc>,
        contents: Arc<StyleSheetContents>,
        media: &str,
        disabled: bool,
    ) -> Gc<'gc, CssStyleSheet> {
        let sheet = self.new_style_sheet(
            document,
            contents,
            media,
            crate::cssom::SheetSource::Constructed,
            None,
            true,
        );
        sheet.disabled.set(disabled);
        sheet
    }

    /// A new sheet of `document` with `contents` and `media`, from
    /// `source` (an `href` and origin-clean or not: a `<link>`'s).
    pub(crate) fn new_style_sheet(
        &self,
        document: NodeRef<'gc>,
        contents: Arc<StyleSheetContents>,
        media: &str,
        source: crate::cssom::SheetSource,
        href: Option<&str>,
        origin_clean: bool,
    ) -> Gc<'gc, CssStyleSheet> {
        let id = self
            .changes
            .sheets_made
            .get()
            .checked_add(1)
            .expect("fewer than 2^64 sheets");
        self.changes.sheets_made.set(id);
        let contents = self.sheet_contents_object(contents);
        let media = self.sheet_media_object(media, Arc::new(MediaQueryList::parse(media)));
        self.gc.alloc_in(
            &sites::OTHER,
            CssStyleSheetInit {
                document: Some(document),
                contents: Some(contents),
                media: Some(media),
                disabled: Cell::new(false),
                id: Cell::new(id),
                listed: Cell::new(false),
                edited: Cell::new(false),
                wrapper: (),
                source: External::new(source),
                owner: None,
                href: External::new(href.map(Into::into)),
                origin_clean: Cell::new(origin_clean),
                rules: None,
                rule_list: None,
                media_list: None,
            },
        )
    }

    fn sheet_media_object(&self, text: &str, media: Arc<MediaQueryList>) -> Gc<'gc, SheetMedia> {
        self.gc.alloc_in(
            &sites::OTHER,
            SheetMediaInit {
                media: External::new((text.into(), media)),
            },
        )
    }

    fn sheet_contents_object(&self, contents: Arc<StyleSheetContents>) -> Gc<'gc, SheetContents> {
        self.gc.alloc_in(
            &sites::OTHER,
            SheetContentsInit {
                contents: External::new(contents),
            },
        )
    }

    /// `sheet`'s constructor document.
    pub fn style_sheet_document(&self, sheet: Gc<'gc, CssStyleSheet>) -> NodeRef<'gc> {
        sheet
            .document
            .get(self.gc)
            .expect("a sheet's constructor document")
    }

    pub(crate) fn style_sheet_wrapper_edge(&self, sheet: Gc<'gc, CssStyleSheet>) -> &'gc JsEdge {
        &sheet.into_ref().wrapper
    }

    /// `sheet`'s number, the same while it lives, another sheet's never.
    pub fn style_sheet_id(&self, sheet: Gc<'gc, CssStyleSheet>) -> u64 {
        sheet.id.get()
    }

    /// `sheet`'s contents now.
    pub fn style_sheet_contents(
        &self,
        sheet: Gc<'gc, CssStyleSheet>,
    ) -> &'gc Arc<StyleSheetContents> {
        sheet
            .into_ref()
            .contents
            .get(self.gc)
            .expect("a sheet's contents")
            .into_ref()
            .contents
            .get()
    }

    /// `sheet`'s media, as written (or serialized, once script changed
    /// them) and parsed.
    pub fn style_sheet_media(
        &self,
        sheet: Gc<'gc, CssStyleSheet>,
    ) -> (&'gc str, &'gc Arc<MediaQueryList>) {
        let media = sheet
            .into_ref()
            .media
            .get(self.gc)
            .expect("a sheet's media")
            .into_ref()
            .media
            .get();
        (&media.0, &media.1)
    }

    /// `sheet`'s media are `text` now (parsed), seen at the next recalc.
    pub fn set_style_sheet_media(
        &self,
        sheet: Gc<'gc, CssStyleSheet>,
        text: &str,
        media: Arc<MediaQueryList>,
    ) {
        let media = self.sheet_media_object(text, media);
        sheet.media.set(self.gc, Some(media));
        self.sheet_changed(sheet);
    }

    pub fn style_sheet_disabled(&self, sheet: Gc<'gc, CssStyleSheet>) -> bool {
        sheet.disabled.get()
    }

    /// `sheet`'s `disabled` set: the scopes adopting it see it at the next
    /// recalc.
    pub fn set_style_sheet_disabled(&self, sheet: Gc<'gc, CssStyleSheet>, disabled: bool) {
        if sheet.disabled.replace(disabled) != disabled {
            self.sheet_changed(sheet);
        }
    }

    /// `sheet`'s contents replaced by `contents` (`replaceSync`): its
    /// rules' objects taken out; the scopes adopting it see them at the
    /// next recalc.
    pub fn replace_style_sheet_contents(
        &self,
        sheet: Gc<'gc, CssStyleSheet>,
        contents: Arc<StyleSheetContents>,
    ) {
        self.sheet_rules_replaced(sheet);
        self.set_style_sheet_rules(sheet, contents);
    }

    /// `sheet`'s contents are `contents` now, an edit of its rules (whose
    /// objects `cssom` keeps in step): seen at the next recalc.
    pub(crate) fn set_style_sheet_rules(
        &self,
        sheet: Gc<'gc, CssStyleSheet>,
        contents: Arc<StyleSheetContents>,
    ) {
        let contents = self.sheet_contents_object(contents);
        sheet.contents.set(self.gc, Some(contents));
        sheet.edited.set(true);
        self.tree_changed();
        self.sheet_changed(sheet);
    }

    /// Whether script edited `sheet`'s rules (`insertRule`, `deleteRule`, a
    /// rule's `style`, `replace`): until then its contents are what its
    /// source (a `<style>`'s text, a `<link>`'s response) parsed to.
    pub fn style_sheet_edited(&self, sheet: Gc<'gc, CssStyleSheet>) -> bool {
        sheet.edited.get()
    }

    /// `sheet` changed (its contents, media or `disabled`): a constructed
    /// one in the list style takes, an element's its tree scope's sheets
    /// marked as changed.
    pub(crate) fn sheet_changed(&self, sheet: Gc<'gc, CssStyleSheet>) {
        if let Some(owner) = sheet.owner.get(self.gc) {
            self.owner_sheet_changed(owner);
        } else if self.style_sheet_is_constructed(sheet) {
            self.style_sheet_changed(sheet);
        }
    }

    /// `sheet` in its document's list style takes (once): only scopes of
    /// that document adopt it.
    fn style_sheet_changed(&self, sheet: Gc<'gc, CssStyleSheet>) {
        if sheet.listed.replace(true) {
            return;
        }
        let data = self.ensure_document_data(self.style_sheet_document(sheet));
        let list = match data.changed_sheets.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.changed_sheets.set(self.gc, Some(list));
                list
            }
        };
        list.push(self.gc, sheet);
    }

    /// The sheets `document` made whose contents or `disabled` changed
    /// since the last time, taken (its style's: it collects again the
    /// scopes it found adopting them).
    pub fn take_changed_style_sheets(&self, document: NodeRef<'gc>) -> Vec<Gc<'gc, CssStyleSheet>> {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.changed_sheets.get(self.gc))
        else {
            return Vec::new();
        };
        let taken: Vec<Gc<'gc, CssStyleSheet>> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect();
        list.clear(self.gc);
        for sheet in &taken {
            sheet.listed.set(false);
        }
        taken
    }

    /// `scope`'s (a document's or a shadow root's) adopted sheets' list,
    /// made at the first ask.
    fn adopted_list(&self, scope: NodeRef<'gc>) -> Gc<'gc, HeapVector<CssStyleSheet>> {
        let member = match self.kind(scope) {
            NodeKind::Document => &self.ensure_document_data(scope).into_ref().adopted,
            _ => {
                &self
                    .shadow_data(scope)
                    .expect("a document or a shadow root")
                    .into_ref()
                    .adopted
            }
        };
        if let Some(list) = member.get(self.gc) {
            return list;
        }
        let list = HeapVector::new(self.gc, 0);
        member.set(self.gc, Some(list));
        list
    }

    /// `scope`'s (a document's or a shadow root's) adopted sheets, in
    /// order (`adoptedStyleSheets`' backing list).
    pub fn adopted_style_sheets(&self, scope: NodeRef<'gc>) -> Vec<Gc<'gc, CssStyleSheet>> {
        let existing = match self.kind(scope) {
            NodeKind::Document => self
                .document_data(scope)
                .and_then(|data| data.into_ref().adopted.get(self.gc)),
            _ => self
                .shadow_data(scope)
                .and_then(|data| data.into_ref().adopted.get(self.gc)),
        };
        existing.map_or_else(Vec::new, |list| {
            (0..list.len())
                .filter_map(|index| list.get(self.gc, index))
                .collect()
        })
    }

    /// `sheet` at `index` of `scope`'s adopted sheets (the one there
    /// replaced; at their count, added at the end), its sheets marked as
    /// changed.
    ///
    /// # Panics
    ///
    /// If `index` is past the count.
    pub fn set_adopted_style_sheet(
        &self,
        scope: NodeRef<'gc>,
        index: usize,
        sheet: Gc<'gc, CssStyleSheet>,
    ) {
        let list = self.adopted_list(scope);
        assert!(index <= list.len(), "an index no greater than the count");
        if index == list.len() {
            list.push(self.gc, sheet);
        } else {
            list.set(self.gc, index, sheet);
        }
        self.adopted_changed(scope);
    }

    /// The last of `scope`'s adopted sheets taken off, its sheets marked as
    /// changed.
    pub fn remove_last_adopted_style_sheet(&self, scope: NodeRef<'gc>) {
        let list = self.adopted_list(scope);
        if let Some(last) = list.len().checked_sub(1) {
            list.remove(self.gc, last);
            self.adopted_changed(scope);
        }
    }

    /// `shadow` moved from `old` to `document`: its adopted sheets dropped
    /// (constructed for its document before), unless one is the other's
    /// template document (a host into or out of a `<template>`'s
    /// contents, as Blink's `TreeScopeAdopter` keeps them).
    pub(crate) fn shadow_root_adopted(
        &self,
        shadow: NodeRef<'gc>,
        old: NodeRef<'gc>,
        document: NodeRef<'gc>,
    ) {
        // Listed in its old document's list, which no longer gives it:
        // in its new one's.
        if let Some(data) = self.shadow_data(shadow)
            && data.sheets_listed.replace(false)
        {
            self.mark_shadow_sheets_changed(shadow);
        }
        let Some(list) = self
            .shadow_data(shadow)
            .and_then(|data| data.into_ref().adopted.get(self.gc))
            .filter(|list| !list.is_empty())
        else {
            return;
        };
        let template_of = |inert: NodeRef<'gc>, of: NodeRef<'gc>| {
            self.document_data(of)
                .and_then(|data| data.template_document.get(self.gc))
                .is_some_and(|template| Gc::ptr_eq(template, inert))
        };
        if template_of(document, old) || template_of(old, document) {
            return;
        }
        list.clear(self.gc);
        self.adopted_changed(shadow);
    }

    fn adopted_changed(&self, scope: NodeRef<'gc>) {
        self.tree_changed();
        match self.kind(scope) {
            NodeKind::Document => self.mark_document_sheets_changed(scope),
            _ => self.mark_shadow_sheets_changed(scope),
        }
    }
}
