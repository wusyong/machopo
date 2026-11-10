//! The workloads on the new GC's heap (elise): nodes of
//! [`NODE_SIZE`](crate::NODE_SIZE) bytes with a DOM node's five tree links;
//! the patterns' objects of [`object_size`](crate::object_size) bytes, as
//! V8's are in JS, object for object, with the same choices. A turn is one
//! operation of the heap (`Heap::mutate`), at whose start the heap may
//! collect.
//!
//! The heap's GC phases go to the trace through its platform (`Gc.Pause`,
//! `Gc.Mark`, `Gc.Sweep`, `Gc.Finalize`), and `gc` (one thread) or
//! `gc-parallel` (a worker for each core but one) says which platform
//! marks.

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use elise::{
    Gc, GcObject, GcRecord, HeapOptions, InlinePlatform, Job, JobHandle, Member, Mutation,
    Persistent, Platform, Site, ThreadPool,
};

use crate::{
    Activity, FANOUT, FragmentationSummary, GROUP_SIZE, GROUPS_PER_TURN, Heap, HeapStatistics,
    LIST_ITEM, LIST_ITEMS_PER_TURN, NODE_SIZE, PATTERN_SEED, PATTERN_TURNS, Pattern, Report, Rng,
    Rng32, Survival, Workload, object_size, scaled,
};

/// A node's data words besides its five edges (four bytes each, padded to
/// a word): what makes it [`NODE_SIZE`] bytes.
const NODE_WORDS: usize = (NODE_SIZE - 24) / 8;

/// A tree node: parent, first and last child, previous and next sibling.
#[derive(GcObject)]
pub struct Node {
    parent: Member<Node>,
    first_child: Member<Node>,
    last_child: Member<Node>,
    previous_sibling: Member<Node>,
    next_sibling: Member<Node>,
    payload: [Cell<u64>; NODE_WORDS],
}

const _: () = assert!(size_of::<Node>() == NODE_SIZE, "a node is NODE_SIZE bytes");

/// A pattern's object's data words besides its `edges` edges: what makes
/// it [`object_size`] bytes.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "evaluated at compile time, where an overflow fails the build"
)]
const fn object_words(edges: usize, size: usize) -> usize {
    (size - (edges * 4).next_multiple_of(8)) / 8
}

/// A pattern's object, of some count of edges.
pub trait PatternObject: GcObject {
    const EDGES: usize;
    fn new<'gc>(cx: &Mutation<'gc>) -> Gc<'gc, Self>;
    fn edge(&self, slot: usize) -> &Member<Self>;
}

macro_rules! pattern_objects {
    ($($name:ident, $init:ident: $n:literal, $($edge:ident)*;)*) => {
        $(
            #[derive(GcObject)]
            pub struct $name {
                $($edge: Member<$name>,)*
                data: [Cell<u64>; object_words($n, object_size($n))],
            }

            const _: () = assert!(
                size_of::<$name>() == object_size($n),
                "a pattern's object is object_size's bytes"
            );

            impl PatternObject for $name {
                const EDGES: usize = $n;

                fn new<'gc>(cx: &Mutation<'gc>) -> Gc<'gc, Self> {
                    // The pattern allocates here alone.
                    static SITE: Site = Site::new();
                    cx.alloc_in(
                        &SITE,
                        $init {
                            $($edge: None,)*
                            data: std::array::from_fn(|_| Cell::new(0)),
                        },
                    )
                }

                fn edge(&self, slot: usize) -> &Member<Self> {
                    [$(&self.$edge),*][slot]
                }
            }
        )*
    };
}

pattern_objects! {
    Object1, Object1Init: 1, e0;
    Object2, Object2Init: 2, e0 e1;
    Object4, Object4Init: 4, e0 e1 e2 e3;
}

/// A platform that gives the heap's GC phases to the trace.
struct Traced(Arc<dyn Platform>);

impl Platform for Traced {
    fn run(&self, job: &dyn Job) {
        self.0.run(job);
    }

    fn post_job(&self, job: Arc<dyn Job + Send>) -> Option<Box<dyn JobHandle>> {
        self.0.post_job(job)
    }

    fn posts_jobs(&self) -> bool {
        self.0.posts_jobs()
    }

    fn tracing(&self) -> &'static std::sync::atomic::AtomicBool {
        bindings::trace::recording()
    }

    fn trace_begin(&self, name: &'static str) {
        bindings::trace::begin(name);
    }

    fn trace_end(&self) {
        bindings::trace::end();
    }
}

/// A new heap that marks on `heap`'s platform, and measures the old
/// generation's fragmentation each time a sweep is done (a walk of its
/// pages, some µs).
pub(crate) fn new_heap(heap: Heap) -> elise::Heap {
    let platform: Arc<dyn Platform> = match heap {
        Heap::GcParallel => {
            let cores = std::thread::available_parallelism().map_or(1, usize::from);
            Arc::new(ThreadPool::new(cores.saturating_sub(1)))
        }
        _ => Arc::new(InlinePlatform),
    };
    let options = HeapOptions {
        measure_fragmentation: true,
        ..HeapOptions::default()
    };
    elise::Heap::with_platform(options, Arc::new(Traced(platform)))
}

/// The heap's sizes, in the report's terms (it pools no pages, and all it
/// commits is resident).
fn sizes(heap: &elise::Heap) -> HeapStatistics {
    let stats = heap.stats();
    HeapStatistics {
        committed_size_bytes: stats.committed_bytes,
        resident_size_bytes: stats.committed_bytes,
        used_size_bytes: stats.used_bytes,
        pooled_memory_size_bytes: 0,
    }
}

/// Runs `workload` at `scale` on a new heap of elise's.
///
/// # Panics
///
/// If `heap` isn't one of elise's, or doesn't run `workload`.
pub fn run(heap: Heap, workload: Workload, scale: f64) -> Report {
    assert!(
        heap.is_gc() && heap.runs(workload),
        "{} doesn't run on the {} heap",
        workload.name(),
        heap.name()
    );
    match workload {
        Workload::Pattern(Pattern::Chain, survival) => {
            run_pattern::<Object1>(heap, Pattern::Chain, survival, scale)
        }
        Workload::Pattern(Pattern::Tree, survival) => {
            run_pattern::<Object2>(heap, Pattern::Tree, survival, scale)
        }
        Workload::Pattern(Pattern::Graph, survival) => {
            run_pattern::<Object4>(heap, Pattern::Graph, survival, scale)
        }
        _ => run_dom_workload(heap, workload, scale),
    }
}

/// The heap's used size after a full GC: what it holds of its own before
/// a workload allocates.
pub(crate) fn baseline(heap: &mut elise::Heap) -> usize {
    heap.collect();
    heap.collect();
    heap.stats().used_bytes
}

/// The forced full GC at the end, in the span `span`: its
/// time, the heap after it, and its record.
pub(crate) fn forced_gc(
    heap: &mut elise::Heap,
    span: &str,
) -> (Duration, HeapStatistics, GcRecord) {
    // Twice, the first unmeasured: a GC already marking would make the
    // forced one only finish it.
    heap.collect();
    bindings::trace::begin(span);
    let start = Instant::now();
    heap.collect();
    let forced = start.elapsed();
    bindings::trace::end();
    (forced, sizes(heap), heap.stats().last_gc)
}

/// What the turns measured: their count, the nodes they allocated, their
/// time, and the heap's peak sizes after each, before the GC the next
/// operation may start with: what the turn left, its garbage included.
pub(crate) struct Turns {
    pub(crate) turns: usize,
    pub(crate) nodes_allocated: usize,
    pub(crate) run: Duration,
    pub(crate) peak_committed: usize,
    pub(crate) peak_used: usize,
    /// What the heap did meanwhile.
    pub(crate) activity: Activity,
}

/// The old generation's fragmentation as the heap measured it, each time
/// a sweep was done and a marking started.
pub(crate) struct Fragmentations {
    swept: Samples,
    marking: Samples,
}

impl Fragmentations {
    /// The measures from now on.
    fn after(heap: &elise::Heap) -> Self {
        let stats = heap.stats();
        Self {
            swept: Samples::after(stats.fragmentation),
            marking: Samples::after(stats.fragmentation_at_marking),
        }
    }

    /// Keeps the heap's last measures, those it made since the last call:
    /// a caller making several operations a turn samples after each, so
    /// none is missed.
    pub(crate) fn sample(&mut self, heap: &elise::Heap) {
        let stats = heap.stats();
        self.swept.sample(stats.fragmentation);
        self.marking.sample(stats.fragmentation_at_marking);
    }
}

/// The measures at one point of the GCs.
struct Samples {
    seen: u64,
    measured: Vec<elise::Fragmentation>,
}

impl Samples {
    fn after(last: elise::Fragmentation) -> Self {
        Self {
            seen: last.count,
            measured: Vec::new(),
        }
    }

    fn sample(&mut self, last: elise::Fragmentation) {
        if last.count != self.seen {
            self.seen = last.count;
            self.measured.push(last);
        }
    }

    fn summary(&self) -> FragmentationSummary {
        let shares = |share: fn(&elise::Fragmentation) -> u32| {
            let mut shares: Vec<u32> = self.measured.iter().map(share).collect();
            shares.sort_unstable();
            let median = shares.get(shares.len() / 2).copied().unwrap_or(0);
            (median, shares.last().copied().unwrap_or(0))
        };
        let (released_median, released_max) =
            shares(|measured| thousandths(measured.released_bytes, measured.old_committed_bytes));
        let (free_median, free_max) =
            shares(|measured| thousandths(measured.free_bytes, measured.slot_bytes));
        FragmentationSummary {
            count: self.measured.len(),
            released_median,
            released_max,
            free_median,
            free_max,
            evacuated_max_bytes: self
                .measured
                .iter()
                .map(|measured| measured.evacuated_bytes)
                .max()
                .unwrap_or(0),
            pages_max: self
                .measured
                .iter()
                .map(|measured| measured.pages)
                .max()
                .unwrap_or(0),
        }
    }
}

/// `part` of `whole`, in thousandths (0 of nothing).
fn thousandths(part: usize, whole: usize) -> u32 {
    let wide = |value: usize| u128::try_from(value).unwrap_or(u128::MAX);
    wide(part)
        .saturating_mul(1000)
        .checked_div(wide(whole))
        .map_or(0, |share| u32::try_from(share).unwrap_or(u32::MAX))
}

/// Runs `turn` `count` times in the span `gc_bench.<name>`, each an
/// operation of the heap, keeping its peak sizes and the fragmentation's
/// measures (`turn` samples them too, after each operation if it makes
/// several); `turn` returns the nodes it allocated.
pub(crate) fn measure_turns(
    heap: &mut elise::Heap,
    name: &str,
    count: usize,
    mut turn: impl FnMut(&mut elise::Heap, &mut Fragmentations) -> usize,
) -> Turns {
    let (mut peak_committed, mut peak_used, mut nodes) = (0usize, 0usize, 0usize);
    let (mut peak_nursery, mut peak_id_chunks) = (0usize, 0usize);
    let before = heap.stats();
    let mut fragmentations = Fragmentations::after(heap);
    bindings::trace::begin(&format!("gc_bench.{name}"));
    let start = Instant::now();
    for _ in 0..count {
        nodes = nodes.saturating_add(turn(heap, &mut fragmentations));
        fragmentations.sample(heap);
        let stats = sizes(heap);
        peak_committed = peak_committed.max(stats.committed_size_bytes);
        peak_used = peak_used.max(stats.used_size_bytes);
        peak_nursery = peak_nursery.max(heap.stats().nursery_bytes);
        peak_id_chunks = peak_id_chunks.max(heap.stats().id_chunks);
    }
    let run = start.elapsed();
    bindings::trace::end();
    Turns {
        turns: count,
        nodes_allocated: nodes,
        run,
        peak_committed,
        peak_used,
        activity: activity(
            &before,
            &heap.stats(),
            peak_nursery,
            peak_id_chunks,
            &fragmentations,
        ),
    }
}

/// What the heap did from `before` to `after`, with the peaks measured.
pub(crate) fn activity(
    before: &elise::HeapStats,
    after: &elise::HeapStats,
    peak_nursery_bytes: usize,
    peak_id_chunks: usize,
    fragmentations: &Fragmentations,
) -> Activity {
    Activity {
        peak_nursery_bytes,
        pretenured_objects: after.pretenured_objects,
        concurrent_markings: after
            .concurrent_markings
            .saturating_sub(before.concurrent_markings),
        markings_at_limit: after
            .markings_at_limit
            .saturating_sub(before.markings_at_limit),
        background_mark_ns: after
            .background_mark_ns
            .saturating_sub(before.background_mark_ns),
        peak_id_chunks,
        background_sweep_ns: after
            .background_sweep_ns
            .saturating_sub(before.background_sweep_ns),
        sweep_on_main_ns: after
            .sweep_on_main_ns
            .saturating_sub(before.sweep_on_main_ns),
        pages_swept_on_main: after
            .pages_swept_on_main
            .saturating_sub(before.pages_swept_on_main),
        old_ranges: after.old_ranges.saturating_sub(before.old_ranges),
        old_range_slots: after.old_range_slots.saturating_sub(before.old_range_slots),
        fragmentation: fragmentations.swept.summary(),
        fragmentation_at_marking: fragmentations.marking.summary(),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "a report's fields, from both ends of a run"
)]
fn report(
    workload: Workload,
    heap: Heap,
    scale: f64,
    turns: Turns,
    live_nodes: usize,
    forced: (Duration, HeapStatistics, GcRecord),
    baseline_used_bytes: usize,
    object_size: usize,
) -> Report {
    let (forced_gc, after_forced_gc, record) = forced;
    Report {
        workload,
        heap,
        scale,
        turns: turns.turns,
        nodes_allocated: turns.nodes_allocated,
        run: turns.run,
        idle: Duration::ZERO,
        peak_committed_bytes: turns.peak_committed,
        peak_used_bytes: turns.peak_used,
        live_nodes,
        forced_gc,
        after_forced_gc,
        baseline_used_bytes,
        object_size,
        gc: Some(record),
        activity: Some(turns.activity),
    }
}

/// A new node, of the site every workload allocates its nodes at (as the
/// DOM's `Node`s would be of a parser's or a script's).
fn new_node<'gc>(cx: &Mutation<'gc>) -> Gc<'gc, Node> {
    static SITE: Site = Site::new();
    cx.alloc_in(
        &SITE,
        NodeInit {
            parent: None,
            first_child: None,
            last_child: None,
            previous_sibling: None,
            next_sibling: None,
            payload: std::array::from_fn(|_| Cell::new(0)),
        },
    )
}

/// Links `child`, on no tree, as `parent`'s last child.
fn link_last<'gc>(cx: &Mutation<'gc>, parent: Gc<'gc, Node>, child: Gc<'gc, Node>) {
    child.parent.set(cx, Some(parent));
    match parent.last_child.get(cx) {
        Some(last) => {
            child.previous_sibling.set(cx, Some(last));
            last.next_sibling.set(cx, Some(child));
        }
        None => parent.first_child.set(cx, Some(child)),
    }
    parent.last_child.set(cx, Some(child));
}

/// Takes `node` out of its parent's children, if it has a parent.
fn unlink<'gc>(cx: &Mutation<'gc>, node: Gc<'gc, Node>) {
    let Some(parent) = node.parent.get(cx) else {
        return;
    };
    let (previous, next) = (node.previous_sibling.get(cx), node.next_sibling.get(cx));
    match previous {
        Some(previous) => previous.next_sibling.set(cx, next),
        None => parent.first_child.set(cx, next),
    }
    match next {
        Some(next) => next.previous_sibling.set(cx, previous),
        None => parent.last_child.set(cx, previous),
    }
    node.parent.set(cx, None);
    node.previous_sibling.set(cx, None);
    node.next_sibling.set(cx, None);
}

/// Appends `count` new nodes under `parent` in document order, each with
/// up to `fanout` children before its next sibling.
fn append_new_subtree<'gc>(cx: &Mutation<'gc>, parent: Gc<'gc, Node>, count: usize, fanout: usize) {
    let (mut depth, mut fits, mut level) = (0usize, 0usize, 1usize);
    while fits < count {
        level = level.saturating_mul(fanout);
        fits = fits.saturating_add(level);
        depth = depth.saturating_add(1);
    }
    let mut left = count;
    append_children(cx, parent, &mut left, fanout, depth);
}

fn append_children<'gc>(
    cx: &Mutation<'gc>,
    parent: Gc<'gc, Node>,
    left: &mut usize,
    fanout: usize,
    depth: usize,
) {
    let Some(below) = depth.checked_sub(1) else {
        return;
    };
    for _ in 0..fanout {
        let Some(rest) = left.checked_sub(1) else {
            return;
        };
        *left = rest;
        let child = new_node(cx);
        link_last(cx, parent, child);
        append_children(cx, child, left, fanout, below);
    }
}

fn children<'gc>(cx: &Mutation<'gc>, node: Gc<'gc, Node>) -> Vec<Gc<'gc, Node>> {
    let mut children = Vec::new();
    let mut next = node.first_child.get(cx);
    while let Some(child) = next {
        children.push(child);
        next = child.next_sibling.get(cx);
    }
    children
}

/// How many nodes `node`'s subtree has, `node` included: a walk in
/// document order, back up through the parents.
fn subtree_size<'gc>(cx: &Mutation<'gc>, node: Gc<'gc, Node>) -> usize {
    let mut size = 1usize;
    let mut next = node.first_child.get(cx);
    while let Some(current) = next {
        size = size.saturating_add(1);
        next = current.first_child.get(cx);
        let mut up = current;
        while next.is_none() && !Gc::ptr_eq(up, node) {
            next = up.next_sibling.get(cx);
            if next.is_none() {
                up = up
                    .parent
                    .get(cx)
                    .expect("a node under the root has a parent");
            }
        }
    }
    size
}

/// Churn's pick: a node two levels down from `root`, its parent, and its
/// subtree's size.
fn churn_pick<'gc>(
    cx: &Mutation<'gc>,
    root: Gc<'gc, Node>,
    rng: &mut Rng,
) -> (Gc<'gc, Node>, Gc<'gc, Node>, usize) {
    let level1 = children(cx, root);
    for _ in 0..1000 {
        let parent = level1[rng.below(level1.len())];
        let level2 = children(cx, parent);
        if level2.is_empty() {
            continue;
        }
        let old = level2[rng.below(level2.len())];
        return (parent, old, subtree_size(cx, old));
    }
    panic!("churn's tree has no node two levels down");
}

fn append_list_item<'gc>(cx: &Mutation<'gc>, list: Gc<'gc, Node>) {
    let item = new_node(cx);
    append_new_subtree(
        cx,
        item,
        LIST_ITEM.saturating_sub(1),
        LIST_ITEM.saturating_sub(1),
    );
    link_last(cx, list, item);
}

/// The tree workloads (what a DOM does to its tree), on `Node`s.
fn run_dom_workload(kind: Heap, workload: Workload, scale: f64) -> Report {
    let mut heap = new_heap(kind);
    let baseline_used_bytes = baseline(&mut heap);
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    // The root, and what the turns work in under it (churn's tree hangs
    // off the root itself: its turns pick nodes two levels down from it).
    let (root, container, size) = heap.mutate(|cx| {
        let root = new_node(cx);
        let container = new_node(cx);
        link_last(cx, root, container);
        let mut size = 0;
        match workload {
            Workload::Churn => {
                unlink(cx, container);
                append_new_subtree(cx, root, scaled(10_000, scale), FANOUT);
            }
            Workload::Replace => {
                size = scaled(1000, scale);
                append_new_subtree(cx, container, size, FANOUT);
            }
            Workload::List => {
                for _ in 0..scaled(400, scale) {
                    append_list_item(cx, container);
                }
            }
            _ => {}
        }
        (
            Persistent::new(cx, root),
            Persistent::new(cx, container),
            size,
        )
    });
    let (count, mut left) = match workload {
        Workload::Build => {
            let total = scaled(100_000, scale);
            (total.div_ceil(1000), total)
        }
        Workload::Churn => (scaled(1000, scale), 0),
        Workload::Replace => (scaled(500, scale), 0),
        Workload::List => (scaled(1000, scale), 0),
        Workload::Ids => (scaled(100, scale), 0),
        Workload::Pattern(..) => unreachable!("the patterns run on their own"),
    };
    let turns = measure_turns(&mut heap, workload.name(), count, |heap, _| {
        heap.mutate(|cx| match workload {
            Workload::Build => {
                let nodes = left.min(1000);
                left = left.saturating_sub(nodes);
                let subtree = new_node(cx);
                append_new_subtree(cx, subtree, nodes.saturating_sub(1), FANOUT);
                link_last(cx, root.get(cx), subtree);
                nodes
            }
            Workload::Churn => {
                let (parent, old, nodes) = churn_pick(cx, root.get(cx), &mut rng);
                unlink(cx, old);
                let new = new_node(cx);
                append_new_subtree(cx, new, nodes.saturating_sub(1), FANOUT);
                link_last(cx, parent, new);
                nodes
            }
            Workload::Replace => {
                let container = container.get(cx);
                while let Some(child) = container.first_child.get(cx) {
                    unlink(cx, child);
                }
                append_new_subtree(cx, container, size, FANOUT);
                size
            }
            Workload::List => {
                let container = container.get(cx);
                for _ in 0..LIST_ITEMS_PER_TURN {
                    append_list_item(cx, container);
                }
                for _ in 0..LIST_ITEMS_PER_TURN {
                    let first = container.first_child.get(cx).expect("the window has items");
                    unlink(cx, first);
                }
                LIST_ITEMS_PER_TURN.saturating_mul(LIST_ITEM)
            }
            Workload::Ids => {
                let container = container.get(cx);
                for i in 0..IDS_PER_TURN {
                    let node = new_node(cx);
                    cx.new_id(node);
                    if i % IDS_KEPT_ONE_IN == 0 {
                        link_last(cx, container, node);
                    }
                }
                IDS_PER_TURN
            }
            Workload::Pattern(..) => unreachable!("the patterns run on their own"),
        })
    });
    let live_nodes = heap.mutate(|cx| subtree_size(cx, root.get(cx)));
    let forced = forced_gc(&mut heap, crate::FORCED_GC);
    drop((root, container));
    report(
        workload,
        kind,
        scale,
        turns,
        live_nodes,
        forced,
        baseline_used_bytes,
        NODE_SIZE,
    )
}

/// The `ids` workload's nodes a turn, and which it keeps.
const IDS_PER_TURN: usize = 10_000;
const IDS_KEPT_ONE_IN: usize = 16;

/// `GROUP_SIZE` new objects linked as `pattern` says, as `patterns.js`
/// links them: a chain through edge 0; or a complete binary tree in
/// breadth-first order (object `i`'s edges 0 and 1 to objects `2i + 1` and
/// `2i + 2`); or a chain with every other edge to an object of the group
/// picked by `random`. Its entry, and the object whose edge 0 is free.
fn new_group<'gc, O: PatternObject>(
    cx: &Mutation<'gc>,
    pattern: Pattern,
    random: &mut impl FnMut(usize) -> usize,
) -> (Gc<'gc, O>, Gc<'gc, O>) {
    let mut objects: Vec<Gc<'gc, O>> = Vec::with_capacity(GROUP_SIZE);
    objects.push(O::new(cx));
    for index in 1..GROUP_SIZE {
        let object = O::new(cx);
        let (parent, slot) = match pattern {
            Pattern::Chain | Pattern::Graph => (index.saturating_sub(1), 0),
            Pattern::Tree => (index.saturating_sub(1) / 2, index.saturating_sub(1) % 2),
        };
        objects[parent].edge(slot).set(cx, Some(object));
        objects.push(object);
    }
    if pattern == Pattern::Graph {
        for object in &objects {
            for slot in 1..O::EDGES {
                let target = objects[random(GROUP_SIZE)];
                object.edge(slot).set(cx, Some(target));
            }
        }
    }
    let last = *objects.last().expect("a group has objects");
    (objects[0], last)
}

/// How many objects `from` reaches, itself included.
fn reachable<'gc, O: PatternObject>(cx: &Mutation<'gc>, from: Gc<'gc, O>) -> usize {
    let mut seen = std::collections::HashSet::from([std::ptr::from_ref::<O>(from.into_ref())]);
    let mut left = vec![from];
    while let Some(object) = left.pop() {
        for slot in 0..O::EDGES {
            if let Some(next) = object.edge(slot).get(cx)
                && seen.insert(std::ptr::from_ref::<O>(next.into_ref()))
            {
                left.push(next);
            }
        }
    }
    seen.len()
}

/// A pattern on the new heap, of objects of `O`'s edges.
fn run_pattern<O: PatternObject>(
    kind: Heap,
    pattern: Pattern,
    survival: Survival,
    scale: f64,
) -> Report {
    debug_assert_eq!(O::EDGES, pattern.edges());
    let workload = Workload::Pattern(pattern, survival);
    let mut heap = new_heap(kind);
    let baseline_used_bytes = baseline(&mut heap);
    let root = heap.mutate(|cx| Persistent::new(cx, O::new(cx)));
    let mut attach = root.clone();
    let mut rng = Rng32(PATTERN_SEED);
    let count = scaled(PATTERN_TURNS, scale);
    let turns = measure_turns(&mut heap, workload.name(), count, |heap, _| {
        heap.mutate(|cx| {
            for _ in 0..GROUPS_PER_TURN {
                let mut random = |bound| rng.below(bound);
                let (entry, last) = new_group::<O>(cx, pattern, &mut random);
                if rng.below(100) < survival.percent() {
                    attach.get(cx).edge(0).set(cx, Some(entry));
                    attach = Persistent::new(cx, last);
                }
            }
        });
        GROUPS_PER_TURN.saturating_mul(GROUP_SIZE)
    });
    drop(attach);
    let live_nodes = heap.mutate(|cx| reachable(cx, root.get(cx)));
    let forced = forced_gc(&mut heap, crate::FORCED_GC);
    drop(root);
    report(
        workload,
        kind,
        scale,
        turns,
        live_nodes,
        forced,
        baseline_used_bytes,
        size_of::<O>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The activity is the turns': the counts since `before`, not since the
    /// heap began.
    #[test]
    fn the_activity_counts_from_before_the_turns() {
        let before = elise::HeapStats {
            concurrent_markings: 3,
            markings_at_limit: 1,
            background_mark_ns: 100,
            pretenured_objects: 7,
            background_sweep_ns: 10,
            sweep_on_main_ns: 20,
            pages_swept_on_main: 3,
            old_ranges: 10,
            old_range_slots: 100,
            ..elise::HeapStats::default()
        };
        let after = elise::HeapStats {
            concurrent_markings: 5,
            markings_at_limit: 2,
            background_mark_ns: 250,
            pretenured_objects: 9,
            background_sweep_ns: 15,
            sweep_on_main_ns: 50,
            pages_swept_on_main: 7,
            old_ranges: 15,
            old_range_slots: 160,
            ..elise::HeapStats::default()
        };
        let fragmentations = Fragmentations::after(&elise::Heap::new(HeapOptions::default()));
        assert_eq!(
            activity(&before, &after, 1 << 20, 4, &fragmentations),
            Activity {
                peak_nursery_bytes: 1 << 20,
                pretenured_objects: 9,
                concurrent_markings: 2,
                markings_at_limit: 1,
                background_mark_ns: 150,
                peak_id_chunks: 4,
                background_sweep_ns: 5,
                sweep_on_main_ns: 30,
                pages_swept_on_main: 4,
                old_ranges: 5,
                old_range_slots: 60,
                fragmentation: FragmentationSummary::default(),
                fragmentation_at_marking: FragmentationSummary::default(),
            }
        );
    }

    /// Each measure is sampled once, however often the heap is asked: the
    /// sweeps' and the markings' (a collect makes one of each).
    #[test]
    fn a_measure_is_sampled_once() {
        let mut heap = elise::Heap::new(HeapOptions {
            measure_fragmentation: true,
            ..HeapOptions::default()
        });
        heap.collect();
        let mut fragmentations = Fragmentations::after(&heap);
        fragmentations.sample(&heap);
        assert!(fragmentations.swept.measured.is_empty(), "none since");
        heap.collect();
        fragmentations.sample(&heap);
        fragmentations.sample(&heap);
        heap.collect();
        heap.collect();
        fragmentations.sample(&heap);
        let counts = |samples: &Samples| {
            samples
                .measured
                .iter()
                .map(|measured| measured.count)
                .collect::<Vec<_>>()
        };
        assert_eq!(counts(&fragmentations.swept), [2, 4]);
        assert_eq!(counts(&fragmentations.marking), [2, 4]);
    }

    /// The measures' shares: the median (the upper of two middles) and the
    /// most, each of its own measure.
    #[test]
    fn the_measures_are_summarized() {
        let sweep =
            |count, released_bytes, free_bytes, evacuated_bytes, pages| elise::Fragmentation {
                count,
                old_committed_bytes: 1000,
                pages,
                slot_bytes: 2000,
                free_bytes,
                evacuated_bytes,
                released_bytes,
                ..elise::Fragmentation::default()
            };
        let samples = Samples {
            seen: 4,
            measured: vec![
                sweep(1, 100, 1000, 5, 3),
                sweep(2, 300, 200, 9, 2),
                sweep(3, 0, 400, 0, 4),
                sweep(4, 200, 1800, 7, 1),
            ],
        };
        assert_eq!(
            samples.summary(),
            FragmentationSummary {
                count: 4,
                released_median: 200,
                released_max: 300,
                free_median: 500,
                free_max: 900,
                evacuated_max_bytes: 9,
                pages_max: 4,
            }
        );
        assert_eq!(thousandths(1, 0), 0, "of nothing");
    }

    /// The report's line carries the activity: the sweep's numbers too,
    /// and the fragmentation's.
    #[test]
    fn the_reports_line_carries_the_sweep() {
        let activity = Activity {
            background_sweep_ns: 5,
            sweep_on_main_ns: 30,
            pages_swept_on_main: 4,
            old_ranges: 8,
            old_range_slots: 80,
            fragmentation: FragmentationSummary {
                count: 3,
                released_median: 10,
                released_max: 20,
                free_median: 300,
                free_max: 400,
                evacuated_max_bytes: 5000,
                pages_max: 60,
            },
            fragmentation_at_marking: FragmentationSummary {
                count: 2,
                released_median: 1,
                ..FragmentationSummary::default()
            },
            ..Activity::default()
        };
        let json = crate::gc_json(Some(elise::GcRecord::default()), Some(activity));
        assert!(
            json.contains(
                "\"background_sweep_ns\":5,\"sweep_on_main_ns\":30,\"pages_swept_on_main\":4,\
                 \"old_ranges\":8,\"old_range_slots\":80,\"fragmentation\":{\"count\":3,\
                 \"released_median\":10,\"released_max\":20,\"free_median\":300,\"free_max\":400,\
                 \"evacuated_max_bytes\":5000,\"pages_max\":60},\"fragmentation_at_marking\":\
                 {\"count\":2,\"released_median\":1,\"released_max\":0,\"free_median\":0,\
                 \"free_max\":0,\"evacuated_max_bytes\":0,\"pages_max\":0}"
            ),
            "{json}"
        );
    }
}
