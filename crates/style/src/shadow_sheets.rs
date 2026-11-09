//! Shadow trees' own sheets (Blink's `ShadowTreeStyleSheetCollection` and
//! a tree scope's `ScopedStyleResolver`): each shadow root's `<style>`s and
//! `<link>`s, in its tree order (titles don't count in a shadow tree, as in
//! Blink, and an alternate `<link>` doesn't apply: there is no preferred
//! set to be of), collected again before
//! the walk when the DOM says they may have changed
//! (`Cx::take_sheet_changed_roots`), and read by the walk as it is.
//!
//! What a thousand shadow roots of one component share: a `<style>`'s
//! parsed contents by its text, and each contents' rule index (Blink's
//! `RuleSet` is its `StyleSheetContents`'), which a shadow root's elements
//! match in its sheets' order.

use std::rc::Rc;
use std::sync::Arc;

use css::StyleSheetContents;
use css::media::MediaEnvironment;
use css::rule_set::RuleSet;
use dom::{Cx, NodeId, NodeRef};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::rule_index::{Features, RuleIndex};
use crate::rule_set_invalidation::{RuleSetInvalidation, SheetsChange};
use crate::{Sheet, next_in_tree_order, sheets_change};

/// A shadow root's sheets as last collected, and the indexes of those
/// whose media match, in cascade order, each with its first rule's order
/// among the root's rules (after the UA sheet's).
pub(crate) struct ShadowScope {
    sheets: Vec<Sheet>,
    /// Whether each of `sheets` applied when they were collected.
    applied: Vec<bool>,
    pub(crate) indexes: Vec<(Rc<RuleIndex>, usize)>,
}

/// Every shadow root's sheets the engine knows, by root, and what they
/// share.
#[derive(Default)]
pub(crate) struct ShadowScopes {
    scopes: FxHashMap<NodeId, ShadowScope>,
    /// Each contents' rule index, by the contents' address (the `Arc`,
    /// held, keeps it). `Rc`: the shadow roots with a sheet of these
    /// contents share it, and only collecting clones it.
    indexes: FxHashMap<usize, (Arc<StyleSheetContents>, Rc<RuleIndex>)>,
    /// What the indexes' selectors ask of siblings and of what is below
    /// an element, any of them (`rule_index::Features`' flags).
    pub(crate) features: Features,
    /// How many scopes there may be before the gone roots' are let go of.
    prune_at: usize,
}

impl ShadowScopes {
    /// The sheets of the shadow root whose id is `root`, if it has had any.
    pub(crate) fn get(&self, root: NodeId) -> Option<&ShadowScope> {
        self.scopes.get(&root)
    }

    /// Whether some media query that decided a scope's rules comes out
    /// otherwise in `environment`: a sheet's own media, or a rule's.
    pub(crate) fn media_changed(&self, environment: &MediaEnvironment) -> bool {
        self.scopes.values().any(|scope| {
            scope
                .sheets
                .iter()
                .zip(&scope.applied)
                .any(|(sheet, &applied)| sheet.applies(environment) != applied)
                || scope
                    .indexes
                    .iter()
                    .any(|(index, _)| index.set.did_media_query_results_change(environment))
        })
    }

    /// How many roots' sheets are known.
    pub(crate) fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// The roots whose sheets are known.
    pub(crate) fn roots(&self) -> Vec<NodeId> {
        self.scopes.keys().copied().collect()
    }

    /// The roots that adopted one of the sheets numbered `sheets`, as last
    /// collected.
    pub(crate) fn roots_adopting(&self, sheets: &[u64]) -> Vec<NodeId> {
        self.scopes
            .iter()
            .filter(|(_, scope)| {
                scope
                    .sheets
                    .iter()
                    .any(|sheet| sheet.adopted.is_some_and(|id| sheets.contains(&id)))
            })
            .map(|(root, _)| *root)
            .collect()
    }

    /// Every index a shadow root's sheets have.
    pub(crate) fn all_indexes(&self) -> impl Iterator<Item = &RuleIndex> {
        self.indexes.values().map(|(_, index)| &**index)
    }

    /// Collects `root`'s sheets again, their media against `environment`:
    /// what their change from the last collected styles again, if they
    /// changed (the rules of the sheets that came and went, as the
    /// document's; each of its elements if they are its first, kept in
    /// another order, or the same sheets with media that come out
    /// otherwise).
    pub(crate) fn collect<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        root: NodeRef<'gc>,
        environment: &MediaEnvironment,
        serial: u32,
    ) -> Option<RuleSetInvalidation> {
        let id = cx.node_id(root);
        let old = self.scopes.remove(&id);
        let mut sheets = Vec::new();
        let walk = std::iter::successors(cx.first_child(root), |node| {
            next_in_tree_order(cx, *node, root)
        });
        for owner in walk {
            // An alternate `<link>` applies in no shadow tree (Chrome's).
            if !cx.owns_sheet(owner) || cx.is_alternate_stylesheet(owner) {
                continue;
            }
            if let Some(sheet) = cx.owner_sheet(owner) {
                sheets.push(Sheet::of_owner(cx, sheet));
            }
        }
        sheets.extend(Sheet::adopted(cx, root));
        // The UA sheet's rules come first.
        let mut first = 0usize;
        let mut indexes = Vec::new();
        for sheet in sheets.iter().filter(|sheet| sheet.applies(environment)) {
            let index = self.index(&sheet.contents, environment, serial);
            let rules = index.set.rules().len();
            indexes.push((index, first));
            first = first.saturating_add(rules);
        }
        // Other sheets, or other indexes (a sheet's media coming to match or
        // not, an index made again for its media queries): other rules,
        // other names for the DOM to note.
        // A root not collected before had none: any sheet coming lists it
        // (one gone with the root out of the document comes back with it,
        // its tree styled whole).
        let old_sheets = old.as_ref().map_or(&[][..], |old| &old.sheets[..]);
        let change = match &old {
            _ if old_sheets.len() != sheets.len()
                || old_sheets.iter().zip(&sheets).any(|(a, b)| {
                    !Arc::ptr_eq(&a.contents, &b.contents)
                        || !Arc::ptr_eq(&a.media, &b.media)
                        || a.disabled != b.disabled
                }) =>
            {
                Some(match sheets_change(old_sheets, &sheets, environment) {
                    SheetsChange::Rules(rules) => rules,
                    _ => RuleSetInvalidation::every_element(),
                })
            }
            None => None,
            Some(old)
                if old.indexes.len() != indexes.len()
                    || old
                        .indexes
                        .iter()
                        .zip(&indexes)
                        .any(|((a, _), (b, _))| !Rc::ptr_eq(a, b)) =>
            {
                Some(RuleSetInvalidation::every_element())
            }
            Some(_) => None,
        };
        // The contents this root had and has no more: their indexes let go
        // of if no other root has them (an edited `<style>`'s old text).
        let gone: Vec<usize> = old_sheets
            .iter()
            .map(|sheet| Arc::as_ptr(&sheet.contents).addr())
            .filter(|key| {
                !sheets
                    .iter()
                    .any(|sheet| Arc::as_ptr(&sheet.contents).addr() == *key)
            })
            .collect();
        // Kept with none too: a `<link>` of it may load.
        let applied = sheets
            .iter()
            .map(|sheet| sheet.applies(environment))
            .collect();
        self.scopes.insert(
            id,
            ShadowScope {
                sheets,
                applied,
                indexes,
            },
        );
        for key in gone {
            let used = self.scopes.values().any(|scope| {
                scope
                    .sheets
                    .iter()
                    .any(|sheet| Arc::as_ptr(&sheet.contents).addr() == key)
            });
            if !used {
                self.indexes.remove(&key);
            }
        }
        if self.scopes.len() >= self.prune_at {
            self.prune(cx);
        }
        change
    }

    /// `contents`' rule index, made once (again if a media query in it
    /// comes out otherwise now).
    fn index(
        &mut self,
        contents: &Arc<StyleSheetContents>,
        environment: &MediaEnvironment,
        serial: u32,
    ) -> Rc<RuleIndex> {
        let key = Arc::as_ptr(contents).addr();
        if let Some((_, index)) = self.indexes.get(&key)
            && !index.set.did_media_query_results_change(environment)
        {
            return index.clone();
        }
        let mut set = RuleSet::default();
        set.add_rules(contents.rules(), environment, 0);
        let index = Rc::new(RuleIndex::new(set, serial));
        self.features.add_flags(&index.features);
        self.indexes.insert(key, (contents.clone(), index.clone()));
        index
    }

    /// The scopes of roots gone or out of the document let go of, and the
    /// contents and indexes no scope has any more.
    fn prune<'gc>(&mut self, cx: &Cx<'_, 'gc>) {
        self.scopes
            .retain(|id, _| cx.node_by_id(*id).is_some_and(|root| cx.is_connected(root)));
        let used: FxHashSet<usize> = self
            .scopes
            .values()
            .flat_map(|scope| {
                scope
                    .sheets
                    .iter()
                    .map(|sheet| Arc::as_ptr(&sheet.contents).addr())
            })
            .collect();
        self.indexes.retain(|key, _| used.contains(key));
        self.features = Features::default();
        for (_, index) in self.indexes.values() {
            self.features.add_flags(&index.features);
        }
        self.prune_at = self.scopes.len().saturating_mul(2).max(16);
    }
}
