//! A document's content language (HTML's pragma-set default language, and
//! the language a higher-level protocol gives it): its response's
//! `Content-Language`, as the host commits it, or its
//! `<meta http-equiv="content-language">`'s, as one is connected or
//! changed, the last set (Blink's `Document::SetContentLanguage`, which
//! both set). An element with no `lang` of its own or its ancestors' is of
//! it (`:lang()`).

use crate::{Cx, NodeRef, QualifiedName, names};

impl<'gc> Cx<'_, 'gc> {
    /// `document`'s content language; none if neither its response nor a
    /// `<meta>` gave it one.
    pub fn content_language(&self, document: NodeRef<'gc>) -> Option<String> {
        let data = self.document_data(document)?;
        data.into_ref()
            .content_language
            .borrow()
            .as_deref()
            .map(String::from)
    }

    /// `document`'s content language is `value`'s first language: up to
    /// a comma, without the whitespace around it (Blink takes a
    /// `Content-Language` so); none for an empty one. Its elements are
    /// styled again if it changed: `:lang()` reads it.
    pub fn set_content_language(&self, document: NodeRef<'gc>, value: Option<&str>) {
        let language = value
            .and_then(|value| value.split(',').next())
            .map(|first| first.trim_matches(|c: char| c.is_ascii_whitespace()))
            .filter(|first| !first.is_empty())
            .map(Box::from);
        let data = self.ensure_document_data(document);
        let changed = {
            let mut kept = data.content_language.borrow_mut();
            let changed = *kept != language;
            *kept = language;
            changed
        };
        // Its document element styled again with its subtree (the
        // document node itself has no style for the walk to start from).
        if changed && let Some(root) = self.document_element(document) {
            self.set_needs_style_recalc(root);
        }
    }

    /// `node` connected, or its `http-equiv` or `content` changed while it
    /// is: a `<meta http-equiv="content-language">`'s pragma (HTML's: its
    /// `content`'s first word, none if it has a comma, sets the document's
    /// pragma-set default language).
    pub(crate) fn pragma_connected(&self, node: NodeRef<'gc>) {
        if self.qualified_name(node) != Some(QualifiedName::html(names::META)) {
            return;
        }
        let equiv = self.attribute_value_by_id(node, names::local::HTTP_EQUIV);
        if !equiv.is_some_and(|equiv| {
            equiv
                .borrow()
                .to_str()
                .eq_ignore_ascii_case("content-language")
        }) {
            return;
        }
        let Some(content) = self.attribute_value_by_id(node, names::CONTENT) else {
            return;
        };
        let content = content.borrow().to_str().into_owned();
        if content.contains(',') {
            return;
        }
        let word = content
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .split(|c: char| c.is_ascii_whitespace())
            .next()
            .unwrap_or_default();
        if word.is_empty() {
            return;
        }
        self.set_content_language(self.document_of(node), Some(word));
    }
}
