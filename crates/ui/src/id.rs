//! Ids of the views in a tree: where a view's node is in the tree's arena,
//! and which of the views that slot has held it is.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// A view in a tree, whatever its type: what the tree's structure (parents,
/// children, hit testing, capture, focus) refers to views by.
///
/// A slot freed by a removed view goes to a later one with its generation
/// one higher, so an id of the removed view finds nothing there; a slot
/// whose generation has run out isn't used again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnyViewId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl AnyViewId {
    pub(crate) fn slot(self) -> usize {
        // Widening: machopo's targets are 32 or 64 bits.
        self.index as usize
    }
}

/// A view of type `T` in a tree. Only [`crate::ViewTree::insert`] makes one,
/// so the view a live id finds is a `T`; a removed view's id finds nothing.
pub struct ViewId<T> {
    id: AnyViewId,
    // `fn() -> T`: the id neither owns a `T` nor stops being `Send` or
    // `Sync` for one.
    _type: PhantomData<fn() -> T>,
}

impl<T> ViewId<T> {
    pub(crate) fn new(id: AnyViewId) -> Self {
        Self {
            id,
            _type: PhantomData,
        }
    }

    pub fn any(self) -> AnyViewId {
        self.id
    }
}

impl<T> From<ViewId<T>> for AnyViewId {
    fn from(id: ViewId<T>) -> Self {
        id.id
    }
}

// By hand: derived, they'd want `T` to be `Clone`, `PartialEq`... too.
impl<T> Clone for ViewId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ViewId<T> {}

impl<T> PartialEq for ViewId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for ViewId<T> {}

impl<T> PartialEq<AnyViewId> for ViewId<T> {
    fn eq(&self, other: &AnyViewId) -> bool {
        self.id == *other
    }
}

impl<T> Hash for ViewId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> fmt::Debug for ViewId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ViewId<{}>({}v{})",
            std::any::type_name::<T>(),
            self.id.index,
            self.id.generation
        )
    }
}
