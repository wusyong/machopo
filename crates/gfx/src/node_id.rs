//! Node ids (Blink's `DOMNodeId`, in `platform/graphics`): what pipeline
//! output off the DOM's heap (boxes, fragments, display items, hit tests)
//! refers to a node by, whichever DOM gave it.

/// Identifies a node within one DOM. The DOM gives it and finds the node
/// by it while the node lives; an id is never given twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn get(self) -> u32 {
        self.0
    }

    /// The id [`get`](Self::get) gave, read back from where it was
    /// written (a WebDriver element reference), or a DOM's own number for
    /// a node. Safe whatever `raw` is: ids are never reused, and one never
    /// given, or whose node was collected, finds nothing.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

/// A map keyed by node ids. Hashed with FxHash, not SipHash: the ids are
/// the DOM's own counter, which no page chooses, so there is no flooding
/// to defend against, and these maps are on the DOM's hot paths.
pub type NodeIdHashMap<V> = rustc_hash::FxHashMap<NodeId, V>;

/// A set of node ids, hashed as [`NodeIdHashMap`] is.
pub type NodeIdHashSet = rustc_hash::FxHashSet<NodeId>;
