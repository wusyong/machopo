//! `dom`'s integration tests, one binary.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod attributes;
mod bloom;
mod collection;
mod custom_elements;
mod editing;
mod events;
mod frames;
mod gc;
mod ids;
mod images;
mod initiators;
mod intersection;
mod kinds;
mod language;
mod mutation;
mod names;
mod probes;
mod resize;
mod scripts;
mod shadow;
mod sheets;
mod slot;
mod strings;
mod style;
mod svg_use;
mod tasks;
mod text;
mod tree;
mod ua_shadow;
mod window;

use dom::{Cx, NodeRef};

/// More children than any test makes: walking past it means the links
/// loop (a broken tree), which must fail, not grow without end.
const MAX_CHILDREN: usize = 10_000;

/// The children of `node`, as their names (`#text` for a text).
pub fn children<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Vec<String> {
    let mut names = Vec::new();
    let mut child = cx.first_child(node);
    while let Some(node) = child {
        assert!(names.len() < MAX_CHILDREN, "the siblings' links loop");
        names.push(cx.node_name(node).to_string());
        child = cx.next_sibling(node);
    }
    names
}

/// The children of `node`, walked back from its last: the same as
/// `children`, if the links agree.
pub fn children_backwards<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Vec<String> {
    let mut names = Vec::new();
    let mut child = cx.last_child(node);
    while let Some(node) = child {
        assert!(names.len() < MAX_CHILDREN, "the siblings' back links loop");
        names.push(cx.node_name(node).to_string());
        child = cx.previous_sibling(node);
    }
    names.reverse();
    names
}
