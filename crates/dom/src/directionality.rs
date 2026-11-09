//! HTML's directionality of an element (https://html.spec.whatwg.org/
//! #the-directionality): its `dir` attribute, or for `auto` (and a `bdi`
//! with none) the first strong character of its text, or its parent
//! element's; ltr at the top. Worked out when asked, walking up (and for
//! `auto` down) the tree, as Blink's `CachedDirectionality` is before it
//! is cached: what a canvas's `direction = "inherit"` draws with. machopo's
//! style has no `direction` property to read it from.

use computed_style::Direction;
use icu_properties::CodePointMapData;
use icu_properties::props::BidiClass;

use elise::Gc;

use crate::{Cx, NodeKind, NodeRef, names};

/// The states of HTML's `dir` attribute but the undefined one.
enum DirState {
    Ltr,
    Rtl,
    Auto,
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s directionality.
    pub fn directionality(&self, element: NodeRef<'gc>) -> Direction {
        let mut element = element;
        loop {
            let auto = match self.dir_state(element) {
                Some(DirState::Ltr) => return Direction::Ltr,
                Some(DirState::Rtl) => return Direction::Rtl,
                Some(DirState::Auto) => true,
                None => self.is_html_named(element, names::local::BDI),
            };
            if auto {
                return self.auto_directionality(element).unwrap_or(Direction::Ltr);
            }
            match self.parent(element) {
                Some(parent) if self.kind(parent) == NodeKind::Element => element = parent,
                _ => return Direction::Ltr,
            }
        }
    }

    /// An HTML element's `dir`, if it is one of its keywords.
    fn dir_state(&self, element: NodeRef<'gc>) -> Option<DirState> {
        if self.qualified_name(element)?.namespace() != names::HTML_NAMESPACE {
            return None;
        }
        let dir = self.attribute_value_by_id(element, names::local::DIR)?;
        let dir = dir.borrow().to_str().to_ascii_lowercase();
        match dir.as_str() {
            "ltr" => Some(DirState::Ltr),
            "rtl" => Some(DirState::Rtl),
            "auto" => Some(DirState::Auto),
            _ => None,
        }
    }

    /// The way the first strong character of `element`'s text goes (HTML's
    /// "contained text auto directionality"), leaving out what `bdi`,
    /// `script`, `style` and `textarea` elements hold, and elements with
    /// their own `dir`; `None` if there is none.
    fn auto_directionality(&self, element: NodeRef<'gc>) -> Option<Direction> {
        let bidi = CodePointMapData::<BidiClass>::new();
        let skipped = |node: NodeRef<'gc>| {
            [
                names::local::BDI,
                names::local::SCRIPT,
                names::local::STYLE,
                names::local::TEXTAREA,
            ]
            .into_iter()
            .any(|local| self.is_html_named(node, local))
                || self.dir_state(node).is_some()
        };
        let mut next = self.first_child(element);
        while let Some(node) = next {
            if let Some(text) = self
                .text_data(node)
                .filter(|_| self.kind(node) == NodeKind::Text)
            {
                let strong = text
                    .borrow()
                    .to_str()
                    .chars()
                    .find_map(|c| match bidi.get(c) {
                        BidiClass::LeftToRight => Some(Direction::Ltr),
                        BidiClass::RightToLeft | BidiClass::ArabicLetter => Some(Direction::Rtl),
                        _ => None,
                    });
                if strong.is_some() {
                    return strong;
                }
            }
            // In tree order: down unless skipped, else across, else across
            // from the nearest ancestor below `element` that has a next one.
            next = self.first_child(node).filter(|_| !skipped(node));
            let mut at = node;
            while next.is_none() {
                next = self.next_sibling(at);
                if next.is_none() {
                    at = self
                        .parent(at)
                        .filter(|&parent| !Gc::ptr_eq(parent, element))?;
                }
            }
        }
        None
    }
}
