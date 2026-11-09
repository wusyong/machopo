//! Property trees (`blink/renderer/platform/graphics/paint`): transform,
//! clip and effect nodes that paint chunks refer to, so the compositor can
//! apply them without re-painting.
//!
//! Pre-paint adds a transform node for a `transform`, a clip node for
//! `overflow: hidden` and an effect node for `opacity < 1`. A scroll
//! container gets a clip node, a transform node for its scroll offset (the
//! scroll translation, which its content is in) and a scroll node.
//!
//! A node keeps its id from one pre-paint to the next while its owner has
//! it (`NodeIds`, Blink's `ObjectPaintProperties` held by the
//! `LayoutObject`): what was painted under it is the same. A node's place
//! may be empty (a scroll node's is `None`; another's holds a node nothing
//! is under). Each node comes after its parent, as in cc's trees, so they
//! are resolved in one pass: a node whose place is before its parent's (a
//! node new around it) takes another.

use std::sync::Arc;

use compositor_paint::ImageFilter;
use computed_style::PseudoId;
use gfx::{NodeId, Path, PointF, RRectF, RectF, SizeF, Transform};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollNodeId(pub usize);

/// The nodes a paint chunk is painted under (`blink::PropertyTreeState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropertyTreeState {
    pub transform: TransformNodeId,
    pub clip: ClipNodeId,
    pub effect: EffectNodeId,
}

impl PropertyTreeState {
    pub const ROOT: Self = Self {
        transform: TransformNodeId(0),
        clip: ClipNodeId(0),
        effect: EffectNodeId(0),
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransformNode {
    pub parent: Option<TransformNodeId>,
    /// From this node's space to the parent's.
    pub matrix: Transform,
    /// Whether what is under it stays where it is when the frame's
    /// viewport scrolls (`position: fixed`): in a
    /// child frame the node already sits above the frame's scroll
    /// translation, so this says so only for the main frame, whose
    /// viewport the compositor scrolls without a node of its own.
    pub fixed: bool,
    /// A sticky box's translation (Blink's sticky translation node, with
    /// its `StickyConstraint`): by how far the box sticks, set as its
    /// scrollport scrolls.
    pub sticky: Option<StickyTranslation>,
}

/// What a sticky box's translation node moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StickyTranslation {
    pub key: layout::StickyKey,
    /// Where the box is in the node's space: the box is snapped there, so
    /// the offset is snapped with it, the two landing on the pixel the box
    /// moved to does (Blink's `pixel_snap_offset`).
    pub at: PointF,
    /// What scrolls it.
    pub scroll: StickyScroll,
    /// The effect and clip the box is under: those its layer would be
    /// under.
    pub effect: EffectNodeId,
    pub clip: ClipNodeId,
    /// The offsets it may take as its scrollport scrolls (layout's
    /// `StickyReaches`), set in place with its offset.
    pub reach: RectF,
}

/// What scrolls a sticky box's scrollport (Blink's sticky constraint's
/// scroll ancestor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyScroll {
    /// The main frame's viewport, which the compositor scrolls with no
    /// node of its own.
    Viewport,
    /// A scroll node's container, or a child frame's viewport.
    Scroller(ScrollNodeId),
    /// Nothing: it is in a box fixed to its frame's viewport, which
    /// doesn't move it.
    Fixed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipNode {
    pub parent: Option<ClipNodeId>,
    /// The space `rect` is in.
    pub transform: TransformNodeId,
    /// `None` for the root: no clip.
    pub rect: Option<RectF>,
    /// Its corners' radii (top left, top right, bottom right, bottom
    /// left): rounded by `border-radius` (Blink's clip node holds a
    /// `FloatRoundedRect`); all 0 for a plain rect.
    pub radii: [SizeF; 4],
    /// Paths it clips to as well, each filled by its fill rule, their
    /// union (`clip-path`; Blink's clip node's `ClipPath`): `rect` is then
    /// their bounds, what needs only an area reads.
    pub paths: Option<Arc<[Path]>>,
}

impl ClipNode {
    /// The clip as a rounded rect; `None` for the root.
    pub fn rrect(&self) -> Option<RRectF> {
        self.rect.map(|rect| RRectF {
            rect,
            radii: self.radii,
        })
    }

    /// Whether its corners are rounded.
    pub fn is_rounded(&self) -> bool {
        self.rrect().is_some_and(|r| !r.is_rect())
    }
}

/// An opacity and a filter applied to what is painted under it as a
/// group (`blink::EffectPaintPropertyNode`).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectNode {
    pub parent: Option<EffectNodeId>,
    pub opacity: f32,
    /// `filter`, applied before the opacity.
    pub filter: Option<ImageFilter>,
    /// The space the filter is in (Blink's `LocalTransformSpace`), and
    /// the clip on what it makes, which reaches past what it filtered
    /// (`OutputClip`): what was painted under it is in clips of its own.
    pub transform: TransformNodeId,
    pub output_clip: ClipNodeId,
}

/// A scroll container (`blink::ScrollPaintPropertyNode`): what scrolling
/// it needs to know. Only data, as Blink's: how far it scrolls is the
/// compositor's math. The main frame's viewport isn't one; the compositor
/// scrolls it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollNode {
    /// The iframe whose child frame the container is in (whose scroll
    /// offsets it is among); `None` in the main frame. A child frame's
    /// viewport is one, its element the document.
    pub frame: Option<NodeId>,
    /// The nearest scroll container around this one, in a frame around
    /// too.
    pub parent: Option<ScrollNodeId>,
    pub element: NodeId,
    /// The padding box, in `transform`'s space: what shows the content.
    pub container: RectF,
    /// The border box, in `transform`'s space: where a wheel picks this
    /// container (hit testing counts the border as the box's own).
    pub border_box: RectF,
    /// The space the container is in (outside its own scroll offset).
    pub transform: TransformNodeId,
    /// The clip the container is under: what is outside it, not its own
    /// overflow clip.
    pub clip: ClipNodeId,
    /// Its own overflow clip, which its content is under.
    pub overflow_clip: ClipNodeId,
    /// The effect the container is under (its own opacity included).
    pub effect: EffectNodeId,
    /// Its scroll translation: the content's space.
    pub scroll_translation: TransformNodeId,
    /// The scrollable overflow's size.
    pub contents: SizeF,
    /// Whether the user can scroll each axis (`auto` / `scroll`).
    pub user_scrollable_x: bool,
    pub user_scrollable_y: bool,
}

/// All the trees, each node at its id: a parent may come after its
/// children, and a place may be empty (see the module).
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTrees {
    pub transform: Vec<TransformNode>,
    pub clip: Vec<ClipNode>,
    pub effect: Vec<EffectNode>,
    /// No root: the viewport isn't in it. `None`: an empty place.
    pub scroll: Vec<Option<ScrollNode>>,
}

impl PropertyTrees {
    /// Just the roots: identity transform, no clip, opacity 1.
    pub fn root_only() -> Self {
        Self {
            transform: vec![TransformNode {
                parent: None,
                matrix: Transform::IDENTITY,
                fixed: false,
                sticky: None,
            }],
            clip: vec![ClipNode {
                parent: None,
                transform: TransformNodeId(0),
                rect: None,
                radii: [SizeF::new(0.0, 0.0); 4],
                paths: None,
            }],
            effect: vec![EffectNode {
                parent: None,
                opacity: 1.0,
                filter: None,
                transform: TransformNodeId(0),
                output_clip: ClipNodeId(0),
            }],
            scroll: Vec::new(),
        }
    }

    pub fn add_scroll(&mut self, node: ScrollNode) -> ScrollNodeId {
        let id = ScrollNodeId(self.scroll.len());
        self.scroll.push(Some(node));
        id
    }

    /// Scroll node `id`, which a chunk, a node or a fragment refers to.
    pub fn scroll(&self, id: ScrollNodeId) -> &ScrollNode {
        self.scroll[id.0]
            .as_ref()
            .expect("a scroll node referred to is there")
    }

    /// The scroll nodes there, each after its parent.
    pub fn scroll_nodes(&self) -> impl Iterator<Item = (ScrollNodeId, &ScrollNode)> {
        self.scroll
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((ScrollNodeId(index), node.as_ref()?)))
    }

    pub fn add_transform(&mut self, parent: TransformNodeId, matrix: Transform) -> TransformNodeId {
        self.add_transform_node(parent, matrix, false)
    }

    /// [`Self::add_transform`] for the content of a box fixed to the main
    /// frame's viewport.
    pub fn add_fixed_transform(
        &mut self,
        parent: TransformNodeId,
        matrix: Transform,
    ) -> TransformNodeId {
        self.add_transform_node(parent, matrix, true)
    }

    fn add_transform_node(
        &mut self,
        parent: TransformNodeId,
        matrix: Transform,
        fixed: bool,
    ) -> TransformNodeId {
        let id = TransformNodeId(self.transform.len());
        self.transform.push(TransformNode {
            parent: Some(parent),
            matrix,
            fixed,
            sticky: None,
        });
        id
    }

    /// Whether what is painted under transform node `id` is fixed to the
    /// main frame's viewport: it or one of its ancestors says so.
    pub fn is_fixed(&self, id: TransformNodeId) -> bool {
        let mut next = Some(id);
        while let Some(id) = next {
            let node = &self.transform[id.0];
            if node.fixed {
                return true;
            }
            next = node.parent;
        }
        false
    }

    pub fn add_clip(
        &mut self,
        parent: ClipNodeId,
        transform: TransformNodeId,
        rect: RectF,
    ) -> ClipNodeId {
        self.add_rounded_clip(parent, transform, RRectF::from_rect(rect))
    }

    /// A clip with rounded corners: `overflow` or a replaced element's
    /// content under `border-radius`.
    pub fn add_rounded_clip(
        &mut self,
        parent: ClipNodeId,
        transform: TransformNodeId,
        rrect: RRectF,
    ) -> ClipNodeId {
        let id = ClipNodeId(self.clip.len());
        self.clip.push(ClipNode {
            parent: Some(parent),
            transform,
            rect: Some(rrect.rect),
            radii: rrect.radii,
            paths: None,
        });
        id
    }

    pub fn add_effect(&mut self, parent: EffectNodeId, opacity: f32) -> EffectNodeId {
        let id = EffectNodeId(self.effect.len());
        self.effect.push(EffectNode {
            parent: Some(parent),
            opacity,
            filter: None,
            transform: TransformNodeId(0),
            output_clip: ClipNodeId(0),
        });
        id
    }

    /// Each transform node's matrix to the root space.
    pub fn to_root(&self) -> Vec<Transform> {
        let mut out: Vec<Transform> = Vec::with_capacity(self.transform.len());
        for node in &self.transform {
            let matrix = match node.parent {
                Some(parent) => out[parent.0].then(&node.matrix),
                None => node.matrix,
            };
            out.push(matrix);
        }
        out
    }

    /// The clip nodes from the outermost to `clip`, without the root.
    pub fn clip_chain(&self, mut clip: ClipNodeId) -> Vec<ClipNodeId> {
        let mut chain = Vec::new();
        while let Some(parent) = self.clip[clip.0].parent {
            chain.push(clip);
            clip = parent;
        }
        chain.reverse();
        chain
    }

    /// The effect nodes from the outermost to `effect`, without the root.
    pub fn effect_chain(&self, mut effect: EffectNodeId) -> Vec<EffectNodeId> {
        let mut chain = Vec::new();
        while let Some(parent) = self.effect[effect.0].parent {
            chain.push(effect);
            effect = parent;
        }
        chain.reverse();
        chain
    }
}

/// Whose a property node is, and which of its nodes: pre-paint gives a
/// node the id it had while its key comes up (Blink's
/// `ObjectPaintProperties`, which the `LayoutObject` holds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeKey {
    pub(crate) owner: NodeOwner,
    pub(crate) role: NodeRole,
}

/// What a property node is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NodeOwner {
    /// An element's box, or its pseudo-element's, in the frame (by its
    /// iframe) it is in.
    Box {
        frame: Option<NodeId>,
        node: NodeId,
        pseudo: Option<PseudoId>,
    },
    /// An element in an outermost `<svg>`'s content, by its place there
    /// in tree order (a `<use>`'s copies are each drawn as the `<use>`).
    Svg {
        frame: Option<NodeId>,
        svg: NodeId,
        index: u32,
    },
}

/// Which of its owner's nodes a property node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NodeRole {
    /// Fixed to the main frame's viewport.
    Fixed,
    /// How far a sticky box sticks.
    StickyTranslation,
    Transform,
    Effect,
    /// Hidden by layout.
    Hidden,
    /// `clip` and `clip-path` (Blink's `CssClip`, `ClipPathClip`): a
    /// node for each union of paths `clip-path` intersects, by its place.
    CssClip,
    ClipPath(usize),
    OverflowClip,
    ScrollTranslation,
    Scroll,
    /// A replaced element's content's curve.
    InnerClip,
    /// An `<svg>`'s content box clip, and its content's position.
    SvgViewport,
    SvgContent,
    /// An iframe's content box clip, its child frame's origin, viewport
    /// scroll translation and viewport.
    FrameClip,
    FrameOrigin,
    FrameScrollTranslation,
    FrameScroll,
    /// An element in an `<svg>`'s content: its transform, opacity, clip
    /// and its content's transform.
    SvgTransform,
    SvgEffect,
    SvgClip,
    SvgContentTransform,
}

/// The ids pre-paint gave its nodes, by key: the next pre-paint gives the
/// same keys the same ids (see [`NodeKey`]).
#[derive(Debug, Clone)]
pub(crate) struct NodeIds {
    pub(crate) transform: Slots,
    pub(crate) clip: Slots,
    pub(crate) effect: Slots,
    pub(crate) scroll: Slots,
}

impl NodeIds {
    /// The ids of [`PropertyTrees::root_only`]'s trees.
    pub(crate) fn root_only() -> Self {
        Self {
            transform: Slots::with_roots(1),
            clip: Slots::with_roots(1),
            effect: Slots::with_roots(1),
            scroll: Slots::with_roots(0),
        }
    }

    /// Whether a tree's empty places are more than its nodes (and at least
    /// [`Slots::FREE_TO_START_AGAIN`]): the next pre-paint gives every node
    /// a new id, in a tree as small as it can be.
    pub(crate) fn mostly_free(&self) -> bool {
        [&self.transform, &self.clip, &self.effect, &self.scroll]
            .into_iter()
            .any(|slots| {
                let free = slots.free.len();
                // More than half: `free * 2 > len`, for whole numbers.
                free >= Slots::FREE_TO_START_AGAIN && free > slots.state.len() / 2
            })
    }

    /// Lets go of the node at `place`: the pre-paint's end frees it unless
    /// its key takes it again.
    pub(crate) fn release(&mut self, place: NodePlace) {
        match place {
            NodePlace::Transform(place) => self.transform.release(place),
            NodePlace::Clip(place) => self.clip.release(place),
            NodePlace::Effect(place) => self.effect.release(place),
            NodePlace::Scroll(place) => self.scroll.release(place),
        }
    }
}

/// A node's place in one of the trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodePlace {
    Transform(usize),
    Clip(usize),
    Effect(usize),
    Scroll(usize),
}

/// What a place in a tree holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaceState {
    /// A root, there for good.
    Root,
    /// A node a fragment's pre-paint made.
    Live,
    /// A node let go of in this pre-paint: its key may take it again, or
    /// its end frees it.
    Released,
    /// Empty.
    Free,
}

/// One tree's places and ids by key.
#[derive(Debug, Clone)]
pub(crate) struct Slots {
    by_key: FxHashMap<NodeKey, usize>,
    /// Each place's key, if its node has one.
    keys: Vec<Option<NodeKey>>,
    state: Vec<PlaceState>,
    /// The free places, going up: given before new ones at the end.
    free: Vec<usize>,
    /// The places let go of in this pre-paint.
    released: Vec<usize>,
}

impl Slots {
    pub(crate) const FREE_TO_START_AGAIN: usize = 32;

    fn with_roots(roots: usize) -> Self {
        Self {
            by_key: FxHashMap::default(),
            keys: vec![None; roots],
            state: vec![PlaceState::Root; roots],
            free: Vec::new(),
            released: Vec::new(),
        }
    }

    /// The place for a node with `key` (a new one without, or for a key
    /// whose node another holds) under the node at `parent`, after it.
    /// The tree's next place at the end is the place count.
    pub(crate) fn give(&mut self, key: Option<NodeKey>, parent: Option<usize>) -> usize {
        let after_parent = |place: usize| parent.is_none_or(|parent| place > parent);
        let kept = key.and_then(|key| self.by_key.get(&key).copied());
        if let Some(place) = kept
            && self.state[place] == PlaceState::Released
            && after_parent(place)
        {
            self.state[place] = PlaceState::Live;
            return place;
        }
        // The free places go up: the last is the one most likely after the
        // parent.
        let place = match self.free.last() {
            Some(&free) if after_parent(free) => {
                self.free.pop();
                free
            }
            _ => self.state.len(),
        };
        if place == self.state.len() {
            self.state.push(PlaceState::Live);
            self.keys.push(key);
        } else {
            self.state[place] = PlaceState::Live;
            self.keys[place] = key;
        }
        // A key whose node another holds keeps its place; one whose place
        // was before its parent's takes this one (and the old place is
        // freed at the end).
        if let Some(key) = key
            && kept.is_none_or(|kept| self.state[kept] != PlaceState::Live)
        {
            self.by_key.insert(key, place);
        }
        place
    }

    fn release(&mut self, place: usize) {
        debug_assert_eq!(self.state[place], PlaceState::Live);
        self.state[place] = PlaceState::Released;
        self.released.push(place);
    }

    /// The pre-paint is over: the places let go of and not taken again
    /// are free for the next. Returns them, which it empties.
    pub(crate) fn end(&mut self) -> Vec<usize> {
        let mut freed = Vec::new();
        for place in std::mem::take(&mut self.released) {
            if self.state[place] != PlaceState::Released {
                continue;
            }
            self.state[place] = PlaceState::Free;
            if let Some(key) = self.keys[place].take()
                && self.by_key.get(&key) == Some(&place)
            {
                self.by_key.remove(&key);
            }
            freed.push(place);
        }
        if !freed.is_empty() {
            self.free.extend_from_slice(&freed);
            self.free.sort_unstable();
        }
        freed
    }

    /// How many places hold a node (for checks).
    #[cfg(debug_assertions)]
    pub(crate) fn nodes(&self) -> usize {
        self.state
            .iter()
            .filter(|state| matches!(state, PlaceState::Root | PlaceState::Live))
            .count()
    }
}

impl TransformNode {
    /// What an empty place holds.
    pub(crate) const EMPTY: Self = Self {
        parent: Some(TransformNodeId(0)),
        matrix: Transform::IDENTITY,
        fixed: false,
        sticky: None,
    };
}

impl ClipNode {
    /// What an empty place holds.
    pub(crate) fn empty() -> Self {
        Self {
            parent: Some(ClipNodeId(0)),
            transform: TransformNodeId(0),
            rect: None,
            radii: [SizeF::new(0.0, 0.0); 4],
            paths: None,
        }
    }
}

impl EffectNode {
    /// What an empty place holds.
    pub(crate) const EMPTY: Self = Self {
        parent: Some(EffectNodeId(0)),
        opacity: 1.0,
        filter: None,
        transform: TransformNodeId(0),
        output_clip: ClipNodeId(0),
    };
}
