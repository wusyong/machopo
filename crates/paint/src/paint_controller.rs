//! `blink::PaintController`: what paint keeps from one paint to the next,
//! so a box whose painting didn't change takes its display items from the
//! last paint instead of painting them again (`DrawingRecorder::
//! UseCachedDrawingIfPossible`), and a stacking context nothing in which
//! changed takes all its items, without being walked
//! (`SubsequenceRecorder::UseCachedSubsequenceIfPossible`).
//!
//! The fragment tree is immutable, and layout keeps a box's fragment while
//! nothing about it changed, so whether a box paints the same is mostly
//! whether it is the same fragment, painted at the same place under the
//! same property tree state. A box whose fragment is new only because one
//! below changed (one without lines) is compared by what it paints itself
//! from: its style, size, border, padding and image (a bitmap or an SVG
//! image). Blink's `PaintInvalidator` decides the same with flags from
//! style and layout and a comparison of the geometry in pre-paint.
//!
//! A stacking context paints the same if it is the same fragment (so is
//! all in it), pre-paint didn't walk it again since the last paint (so its
//! nodes are the same; their matrices don't change what is painted), no
//! child frame is in it, and no element in it waits to paint anew: one
//! does marks the stacking contexts it was painted in, out to the root
//! (Blink's `PaintLayer::SetNeedsRepaint` up the ancestors).
//!
//! What each box and stacking context painted is kept across paints, the
//! boxes' items placed from their stacking context's start, so a stacking
//! context taken whole changes nothing kept but where its items start
//! (Blink keeps it on each `DisplayItemClient` and `PaintLayer`). So are
//! the targets for hit testing each recorded, which go with the items.

use std::ops::Range;
use std::sync::Arc;

use base::AtomicSequenceNumber;
use gfx::{NodeId, NodeIdHashSet, SizeF};
use layout::BoxFragment;
use rustc_hash::FxHashMap;

use crate::{BackgroundImages, FragmentProperties, PaintArtifact, PrePaint, table_part};

/// Which paint an artifact is from: the items it copied say which paint
/// they came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintId(u64);

impl PaintId {
    pub(crate) fn next() -> Self {
        static IDS: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::starting_at(1);
        Self(IDS.get_next())
    }
}

/// The items an artifact copied from the paint before it, unchanged: that
/// paint, and for each item its index there (`None` if painted anew).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedItems {
    pub from: PaintId,
    pub indices: Vec<Option<usize>>,
}

/// Where a stacking context's items and hit-test targets start.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Start {
    pub(crate) items: usize,
    pub(crate) hits: usize,
}

/// The last paint's items and targets a box takes, in that paint.
#[derive(Debug, Clone)]
pub(crate) struct CachedBox {
    pub(crate) backgrounds: Range<usize>,
    pub(crate) text: Range<usize>,
    pub(crate) text_hits: Range<usize>,
    /// Its line-phase items held a child frame's: not to take as they were.
    pub(crate) child_frame: bool,
}

/// What one box painted, and what it painted it from.
pub(crate) struct PaintedBox {
    /// Keeps the fragment, and so the address it is found by, alive.
    pub(crate) fragment: Arc<BoxFragment>,
    pub(crate) properties: FragmentProperties,
    /// Its background went to the canvas.
    pub(crate) skips_background: bool,
    /// The stacking context it was painted in, by its root's address: its
    /// items are placed from that one's start.
    pub(crate) owner: *const BoxFragment,
    /// Its background and border, in the background phase.
    pub(crate) backgrounds: Range<usize>,
    /// Its image and lines, in the line phase.
    pub(crate) text: Range<usize>,
    /// Its lines' targets, in the hit-test data, placed the same way.
    pub(crate) text_hits: Range<usize>,
    /// Hit testing goes into it: it isn't in a child frame whose iframe
    /// hit testing doesn't find.
    pub(crate) hit_testable: bool,
    /// It is an iframe that showed a child frame.
    pub(crate) child_frame: bool,
    /// While painting it: what it took from the last paint.
    pub(crate) cached: Option<CachedBox>,
}

/// What a stacking context painted (Blink's subsequence of a
/// `PaintLayer`).
pub(crate) struct Subsequence {
    /// Keeps the fragment, and so the address it is found by, alive.
    pub(crate) fragment: Arc<BoxFragment>,
    /// The paint its items are last in, and where; its targets too.
    pub(crate) paint: PaintId,
    pub(crate) items: Range<usize>,
    pub(crate) hits: Range<usize>,
    /// The stacking contexts painted in it.
    pub(crate) nested: Vec<*const BoxFragment>,
    /// The out-of-flow boxes painted in it, at any depth: where their
    /// parents place them, but walked by pre-paint as their containing
    /// blocks' children, which may be outside it, so one may have other
    /// properties while it doesn't.
    pub(crate) out_of_flow: Vec<Arc<BoxFragment>>,
    /// The one it was painted in.
    pub(crate) parent: Option<*const BoxFragment>,
    /// The box whose background went to the canvas, as it was painted.
    pub(crate) skip_background: Option<NodeId>,
    /// Hit testing went into it.
    pub(crate) hit_testable: bool,
}

/// What the paints keep of each box and stacking context, from paint to
/// paint.
#[derive(Default)]
pub(crate) struct PaintCache {
    pub(crate) boxes: FxHashMap<*const BoxFragment, PaintedBox>,
    pub(crate) subsequences: FxHashMap<*const BoxFragment, Subsequence>,
    /// The element boxes' fragments, by element (not its pseudo-elements').
    pub(crate) by_node: FxHashMap<NodeId, *const BoxFragment>,
    /// How many boxes were kept after the last pruning.
    kept: usize,
}

impl PaintCache {
    /// Where the items and targets of the stacking context `owner` started
    /// in paint `paint`, if they are last there. (A box looked up in a paint is in
    /// the stacking context it was in, not painted yet: the same fragment
    /// stays under its parent, and a moved element is styled again, which
    /// its new fragment doesn't take items with.)
    pub(crate) fn start_in(&self, owner: *const BoxFragment, paint: PaintId) -> Option<Start> {
        let subsequence = self.subsequences.get(&owner)?;
        (subsequence.paint == paint).then_some(Start {
            items: subsequence.items.start,
            hits: subsequence.hits.start,
        })
    }

    /// Stacking context `root`'s items and targets, and those of the ones
    /// in it, are now in paint `paint`, `by` further on; `root` in
    /// `parent`.
    /// The out-of-flow boxes painted in the stacking contexts `nested`
    /// (each painted in one), and in those in them.
    pub(crate) fn out_of_flow_in(&self, nested: &[*const BoxFragment]) -> Vec<Arc<BoxFragment>> {
        let mut out = Vec::new();
        for address in nested {
            let subsequence = &self.subsequences[address];
            if subsequence.fragment.style.is_out_of_flow() {
                out.push(subsequence.fragment.clone());
            }
            out.extend(subsequence.out_of_flow.iter().cloned());
        }
        out
    }

    pub(crate) fn moved(
        &mut self,
        root: *const BoxFragment,
        parent: Option<*const BoxFragment>,
        paint: PaintId,
        by: (isize, isize),
    ) {
        let subsequence = self
            .subsequences
            .get_mut(&root)
            .expect("a subsequence painted");
        subsequence.parent = parent;
        subsequence.paint = paint;
        let shift = |range: &Range<usize>, by: isize| {
            range.start.saturating_add_signed(by)..range.end.saturating_add_signed(by)
        };
        subsequence.items = shift(&subsequence.items, by.0);
        subsequence.hits = shift(&subsequence.hits, by.1);
        for nested in subsequence.nested.clone() {
            self.moved(nested, Some(root), paint, by);
        }
    }

    /// The stacking contexts the elements in `invalidated` were painted
    /// in, and those around them; `None` if one wasn't painted in a box of
    /// its own.
    fn repaint(
        &self,
        invalidated: &NodeIdHashSet,
    ) -> Option<rustc_hash::FxHashSet<*const BoxFragment>> {
        let mut repaint = rustc_hash::FxHashSet::default();
        for node in invalidated {
            let painted = self.boxes.get(self.by_node.get(node)?)?;
            let mut at = Some(painted.owner);
            while let Some(address) = at
                && repaint.insert(address)
            {
                at = self.subsequences.get(&address).and_then(|s| s.parent);
            }
        }
        Some(repaint)
    }

    /// Lets go of what gone fragments painted, once the boxes kept are
    /// twice those kept after the last pruning. A fragment is gone when
    /// `pre`, the pre-paint just painted from, has no record of it. (Not
    /// when only this holds it: a gone fragment holds its children, so
    /// they'd stay until a later pruning, one level of a gone tree each,
    /// and what is kept, and so when to prune next, would keep growing.)
    fn prune(&mut self, pre: &PrePaint) {
        if self.boxes.len() <= self.kept.saturating_mul(2).max(1024) {
            return;
        }
        self.boxes.retain(|&address, _| pre.has_record(address));
        self.subsequences
            .retain(|&address, _| pre.has_record(address));
        let boxes = &self.boxes;
        self.by_node
            .retain(|_, address| boxes.contains_key(address));
        self.kept = self.boxes.len();
    }
}

/// The last paint, as the next one finds it.
pub(crate) struct Previous {
    pub(crate) artifact: Arc<PaintArtifact>,
    /// The elements to paint anew, whatever else says.
    pub(crate) invalidated: NodeIdHashSet,
    /// The stacking contexts they were painted in, out to the root; `None`
    /// if one wasn't found (an inline element's), when none is taken whole.
    pub(crate) repaint: Option<rustc_hash::FxHashSet<*const BoxFragment>>,
    /// The pre-paint it was painted from (`PrePaint::generation`).
    pub(crate) pre: (PaintId, u64),
}

impl Previous {
    /// What `fragment`, at `properties`, painted last time, if it would
    /// paint the same now: its background-phase and line-phase items, and
    /// its lines' targets.
    pub(crate) fn cached(
        &self,
        cache: &PaintCache,
        fragment: &Arc<BoxFragment>,
        properties: &FragmentProperties,
        skips_background: bool,
        hit_testable: bool,
    ) -> Option<CachedBox> {
        if self.is_invalidated(fragment) {
            return None;
        }
        let address = Arc::as_ptr(fragment);
        let painted = match cache.boxes.get(&address) {
            Some(painted) => painted,
            // A new fragment: the same painting only if it has no lines
            // (laid out again, or restyled, if it has) and what it paints
            // itself from is the same. Not a table part's: it paints from
            // where its children are too (a table's border goes around
            // its grid, not its captions; a row's background is painted
            // in its cells; an empty cell may paint nothing).
            None if fragment.lines.is_empty()
                && fragment.pseudo.is_none()
                && table_part(fragment).is_none() =>
            {
                let old = cache.boxes.get(cache.by_node.get(&fragment.node?)?)?;
                let (a, b) = (&old.fragment, fragment);
                // Not the role: what it changes (whose background goes to
                // the canvas) is compared below. Not the border and
                // padding: the border's widths are the style's, and the
                // padding changes only an image's content box, which is
                // in its size (`box-sizing` aside, a replaced element is
                // sized by its content box). A native look paints its
                // control's state too, which isn't in its style.
                let same = a.lines.is_empty()
                    && Arc::ptr_eq(&a.style, &b.style)
                    && a.size == b.size
                    && a.image == b.image
                    && a.svg_image == b.svg_image
                    && a.control_state == b.control_state;
                if !same {
                    return None;
                }
                old
            }
            None => return None,
        };
        if painted.properties != *properties
            || painted.skips_background != skips_background
            || painted.hit_testable != hit_testable
        {
            return None;
        }
        let start = cache.start_in(painted.owner, self.artifact.id)?;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the box's items, at `start` in the artifact being painted, are in it"
        )]
        let at = |start: usize, range: &Range<usize>| start + range.start..start + range.end;
        Some(CachedBox {
            backgrounds: at(start.items, &painted.backgrounds),
            text: at(start.items, &painted.text),
            text_hits: at(start.hits, &painted.text_hits),
            child_frame: painted.child_frame,
        })
    }

    /// Whether stacking context `root` paints as it did last time, with
    /// `skip_background` going to the canvas: then its items and targets
    /// are those of its subsequence there.
    pub(crate) fn subsequence<'c>(
        &self,
        cache: &'c PaintCache,
        pre: &PrePaint,
        root: &BoxFragment,
        skip_background: Option<NodeId>,
        hit_testable: bool,
    ) -> Option<&'c Subsequence> {
        let address = root as *const BoxFragment;
        if self
            .repaint
            .as_ref()
            .is_none_or(|repaint| repaint.contains(&address))
        {
            return None;
        }
        let subsequence = cache.subsequences.get(&address)?;
        (subsequence.paint == self.artifact.id
            && subsequence.skip_background == skip_background
            && subsequence.hit_testable == hit_testable
            && pre.unchanged_since(root, self.pre)
            && subsequence
                .out_of_flow
                .iter()
                .all(|fragment| pre.unchanged_since(fragment, self.pre)))
        .then_some(subsequence)
    }
}

impl Previous {
    /// Whether `fragment`, an inline element on its lines, or a table's
    /// column or column group (whose backgrounds are among its items: a
    /// table paints its columns' in their cells; Blink's `LayoutTableColumn::
    /// ImageChanged` invalidates the table), is to paint anew.
    fn is_invalidated(&self, fragment: &BoxFragment) -> bool {
        if self.invalidated.is_empty() {
            return false;
        }
        let contains =
            |node: Option<NodeId>| node.is_some_and(|node| self.invalidated.contains(&node));
        // A run's element too: text right in an anonymous block, or in an
        // inline element with no box of its own on the line, has no other
        // node here (a selection over it changes); and a `<br>` ending a
        // line (a selection over it too).
        contains(fragment.node)
            || fragment.lines.iter().any(|line| {
                line.boxes
                    .iter()
                    .any(|inline_box| self.invalidated.contains(&inline_box.node))
                    || line.runs.iter().any(|run| contains(run.node))
                    || line
                        .end
                        .as_ref()
                        .is_some_and(|end| end.br().is_some_and(|br| contains(Some(br))))
            })
            || fragment.columns.iter().any(|column| contains(column.node))
    }
}

/// `blink::PaintController` for one document: the last paint, to take the
/// items of what didn't change from.
#[derive(Default)]
pub struct PaintController {
    previous: Option<Previous>,
    cache: PaintCache,
    /// What paints anew next time ([`Self::invalidate`]).
    invalidated: NodeIdHashSet,
    /// The blocks whose caret changed ([`Self::invalidate_caret`]).
    carets: NodeIdHashSet,
    /// What the last paint snapped to: at another, nothing is the same.
    device_scale_factor: f32,
    /// How many boxes the last paint walked, and how many stacking
    /// contexts it took whole.
    walked: (usize, usize),
}

impl PaintController {
    /// Paints the fragment tree (see [`crate::paint`]), taking from the last
    /// paint the items of each box that paints the same, and of each
    /// stacking context nothing in which changed.
    pub fn paint(
        &mut self,
        layout_view: &Arc<BoxFragment>,
        pre: &PrePaint,
        document_size: SizeF,
        images: &dyn BackgroundImages,
    ) -> Arc<PaintArtifact> {
        if pre.device_scale_factor != self.device_scale_factor {
            self.previous = None;
            self.cache = PaintCache::default();
            self.device_scale_factor = pre.device_scale_factor;
        }
        if let Some(previous) = &mut self.previous {
            previous.invalidated = std::mem::take(&mut self.invalidated);
            previous.repaint = if self.carets.is_empty() {
                self.cache.repaint(&previous.invalidated)
            } else {
                // Walked to, but not painted anew.
                let mut walked = previous.invalidated.clone();
                walked.extend(self.carets.iter().copied());
                self.cache.repaint(&walked)
            };
        }
        self.invalidated.clear();
        self.carets.clear();
        let (artifact, walked) = crate::paint_with(
            layout_view,
            pre,
            document_size,
            images,
            self.previous.as_ref(),
            &mut self.cache,
        );
        let artifact = Arc::new(artifact);
        self.walked = walked;
        self.cache.prune(pre);
        self.previous = Some(Previous {
            artifact: artifact.clone(),
            invalidated: NodeIdHashSet::default(),
            repaint: Some(Default::default()),
            pre: pre.generation(),
        });
        artifact
    }

    /// The elements in `nodes` paint anew next time, whatever else says
    /// they'd paint the same (Blink's `SetShouldDoFullPaintInvalidation`):
    /// an image their background shows has loaded.
    pub fn invalidate(&mut self, nodes: impl IntoIterator<Item = NodeId>) {
        self.invalidated.extend(nodes);
    }

    /// The caret in the boxes of the elements in `blocks` came, went,
    /// moved or blinked: the stacking contexts they are in are walked
    /// next time, not taken whole, but the boxes take what they painted
    /// themselves, and paint only the caret anew (Blink's
    /// `CaretDisplayItemClient` is a client of its own).
    pub fn invalidate_caret(&mut self, blocks: impl IntoIterator<Item = NodeId>) {
        self.carets.extend(blocks);
    }

    /// What the last paint painted, if there was one.
    pub fn last_artifact(&self) -> Option<&Arc<PaintArtifact>> {
        self.previous.as_ref().map(|previous| &previous.artifact)
    }

    /// How many boxes the last paint walked, and how many stacking
    /// contexts it took whole from the paint before. For tests.
    pub fn walked_for_testing(&self) -> (usize, usize) {
        self.walked
    }

    /// How many boxes' and stacking contexts' paintings are kept for the
    /// next paint. For tests.
    pub fn kept_for_testing(&self) -> (usize, usize) {
        (self.cache.boxes.len(), self.cache.subsequences.len())
    }
}
