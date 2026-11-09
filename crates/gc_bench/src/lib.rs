//! GC microbenchmarks: DOM-like workloads on elise's heap, with
//! the GC left to schedule itself, as V8's cppgc benchmarks
//! (`test/benchmarks/cpp/cppgc/`) run their own test classes on a heap of
//! their own. The allocation patterns (lists, binary trees, random graphs,
//! most of each batch kept or most dropped) run on V8's own heap too, as JS
//! objects, object for object as on elise's: what V8's GC does
//! with the same work.
//!
//! A workload runs in turns, as an event loop's tasks: each turn mutates
//! the tree, then the heap does what it has to (elise's at the
//! start of its next operation; V8's platform tasks, which the loop runs
//! after each turn). The GC's own work is in the trace (elise's
//! `Gc.*` events and V8's, when `bindings::trace` runs), which `bench.py
//! gc` reads; this measures the workload's time, the heap's sizes after
//! each turn, and a forced full GC of what is left alive at the end (the
//! marking's throughput, as V8's `trace_perf`).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

pub mod gc_heap;
pub mod v8_heap;

use std::fmt::Write as _;
use std::str::FromStr;
use std::time::Duration;

use base::numerics::{clamp_round, saturated_cast};

/// Which heap a workload runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heap {
    /// V8's own, for JS objects: only the patterns run on it.
    V8,
    /// The new GC's (elise), marking on the heap's thread alone.
    Gc,
    /// The new GC's, marking on a worker for each core but one too.
    GcParallel,
}

impl Heap {
    pub const ALL: [Heap; 3] = [Heap::V8, Heap::Gc, Heap::GcParallel];

    pub fn name(self) -> &'static str {
        match self {
            Heap::V8 => "v8",
            Heap::Gc => "gc",
            Heap::GcParallel => "gc-parallel",
        }
    }

    /// Whether this is elise's heap ([`gc_heap::run`]).
    pub fn is_gc(self) -> bool {
        matches!(self, Heap::Gc | Heap::GcParallel)
    }

    /// Whether `workload` runs on this heap: the tree's workloads and the
    /// id table are elise's, which a JS object has no counterpart
    /// of.
    pub fn runs(self, workload: Workload) -> bool {
        match workload {
            Workload::Pattern(..) => true,
            _ => self.is_gc(),
        }
    }
}

impl FromStr for Heap {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        Heap::ALL
            .into_iter()
            .find(|heap| heap.name() == name)
            .ok_or_else(|| format!("no heap {name:?} (v8, gc, gc-parallel)"))
    }
}

/// A heap's sizes in bytes, as a report gives them (named as
/// `cppgc::HeapStatistics`'s).
/// `committed` and `resident` include `pooled`, the freed pages the heap
/// keeps to reuse. `used` is what the last GC marked plus what was
/// allocated since: garbage counts until a GC's marking ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapStatistics {
    pub committed_size_bytes: usize,
    pub resident_size_bytes: usize,
    pub used_size_bytes: usize,
    pub pooled_memory_size_bytes: usize,
}

/// A tree node's bytes: as large as the DOM's nodes were on cppgc's heap
/// (their tree links and their kind's data, flags and rare data), so that
/// runs compare with earlier ones.
pub const NODE_SIZE: usize = 344;

/// A pattern's object's bytes with `edges` edges: the edges (four bytes
/// each, padded to a word) and four words of data, as on cppgc's heap
/// (two of data and a canary's two), so that runs compare with earlier
/// ones.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a pattern's few edges: evaluated at compile time, where an overflow fails the build"
)]
pub const fn object_size(edges: usize) -> usize {
    (edges * 4).next_multiple_of(8) + 32
}

/// An allocation pattern: groups of [`GROUP_SIZE`] new objects of a shape,
/// [`GROUPS_PER_TURN`] a turn, 100 turns; each group kept (linked from the
/// last kept one) or dropped as the [`Survival`] has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// One long list (objects of 1 edge): the marking follows a single
    /// chain, with nothing to share between threads.
    Chain,
    /// Binary trees (2 edges), as V8's `binary-trees_perf`.
    Tree,
    /// Random graphs (4 edges): a chain through every group, and the other
    /// edges to objects of the group picked at random.
    Graph,
}

impl Pattern {
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Chain => "chain",
            Pattern::Tree => "tree",
            Pattern::Graph => "graph",
        }
    }

    /// An object's edges.
    pub fn edges(self) -> usize {
        match self {
            Pattern::Chain => 1,
            Pattern::Tree => 2,
            Pattern::Graph => 4,
        }
    }
}

/// How much of what a pattern allocates is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Survival {
    /// 90% of the groups: most of what is allocated lives on.
    High,
    /// 5%: most dies young, as the generational hypothesis has it.
    Low,
}

impl Survival {
    /// The share of the groups kept, in percent.
    pub fn percent(self) -> usize {
        match self {
            Survival::High => 90,
            Survival::Low => 5,
        }
    }
}

/// A pattern's objects a group, and groups a turn (at scale 1, 100 turns:
/// 1,024,000 objects).
pub const GROUP_SIZE: usize = 128;
pub const GROUPS_PER_TURN: usize = 80;
pub(crate) const PATTERN_TURNS: u32 = 100;
/// The seed of the patterns' choices, the same in JS.
pub(crate) const PATTERN_SEED: u32 = 0x9e37_79b9;

/// The workloads: after what a DOM does to its tree, and the allocation
/// patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// A parser building a large document: 100,000 nodes, 1,000 a turn,
    /// all kept.
    Build,
    /// A framework reconciling: in a tree of 10,000 nodes, a subtree two
    /// levels down (about 100 nodes) taken out and a new one as large built
    /// and put in its place, each turn, 1,000 turns.
    Churn,
    /// `innerHTML`: a container's 1,000 nodes replaced by 1,000 new ones,
    /// each turn, 500 turns.
    Replace,
    /// A long list scrolling: a window of 400 items of 5 nodes, 10 new
    /// items at the bottom and the 10 at the top dropped, each turn, 1,000
    /// turns.
    List,
    /// An allocation pattern, on any heap.
    Pattern(Pattern, Survival),
    /// Elise's id table at a large document's size: 1,000,000
    /// nodes, 10,000 a turn, each given an id (as the DOM gives every
    /// node one), one in 16 kept: a full GC's walk of the table.
    Ids,
}

impl Workload {
    pub const ALL: [Workload; 11] = [
        Workload::Build,
        Workload::Churn,
        Workload::Replace,
        Workload::List,
        Workload::Pattern(Pattern::Chain, Survival::High),
        Workload::Pattern(Pattern::Chain, Survival::Low),
        Workload::Pattern(Pattern::Tree, Survival::High),
        Workload::Pattern(Pattern::Tree, Survival::Low),
        Workload::Pattern(Pattern::Graph, Survival::High),
        Workload::Pattern(Pattern::Graph, Survival::Low),
        Workload::Ids,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workload::Build => "build",
            Workload::Churn => "churn",
            Workload::Replace => "replace",
            Workload::List => "list",
            Workload::Pattern(Pattern::Chain, Survival::High) => "chain-high",
            Workload::Pattern(Pattern::Chain, Survival::Low) => "chain-low",
            Workload::Pattern(Pattern::Tree, Survival::High) => "tree-high",
            Workload::Pattern(Pattern::Tree, Survival::Low) => "tree-low",
            Workload::Pattern(Pattern::Graph, Survival::High) => "graph-high",
            Workload::Pattern(Pattern::Graph, Survival::Low) => "graph-low",
            Workload::Ids => "ids",
        }
    }
}

impl FromStr for Workload {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        Workload::ALL
            .into_iter()
            .find(|workload| workload.name() == name)
            .ok_or_else(|| {
                let names: Vec<_> = Workload::ALL
                    .iter()
                    .map(|workload| workload.name())
                    .collect();
                format!("no workload {name:?} ({})", names.join(", "))
            })
    }
}

/// A tree's children per node: about a document's.
pub(crate) const FANOUT: usize = 10;
/// A list item's nodes, and the items a turn adds and drops.
pub(crate) const LIST_ITEM: usize = 5;
pub(crate) const LIST_ITEMS_PER_TURN: usize = 10;

/// What a run measured.
#[derive(Clone, Debug)]
pub struct Report {
    pub workload: Workload,
    pub heap: Heap,
    pub scale: f64,
    pub turns: usize,
    /// Nodes (a pattern's objects) allocated by the turns (not the setup).
    pub nodes_allocated: usize,
    /// The turns' time, the heap's tasks included, and then running what
    /// the heap still had to do, but for `idle`.
    pub run: Duration,
    /// Waiting for the heap's delayed tasks once the turns were over, as an
    /// idle loop would: not work, but how late the heap finished.
    pub idle: Duration,
    /// The largest committed and used sizes at the end of a turn, before
    /// the heap's tasks there run: what the turn left, its garbage
    /// included, on every heap alike (elise's collects at the
    /// start of the next operation).
    pub peak_committed_bytes: usize,
    pub peak_used_bytes: usize,
    /// The nodes (objects) alive at the end: what the forced GC marks.
    pub live_nodes: usize,
    /// The forced full GC of what is alive at the end, and the heap after.
    pub forced_gc: Duration,
    pub after_forced_gc: HeapStatistics,
    /// The used size after a full GC before the workload allocated
    /// anything: the heap's own (on V8's, the script's and the realm's).
    pub baseline_used_bytes: usize,
    /// A node's or object's Rust data (the heap adds its header); 0 for a
    /// JS object, whose size only V8 knows.
    pub object_size: usize,
    /// Elise's own record of the forced GC, on its heaps: to check
    /// the trace's against.
    pub gc: Option<elise::GcRecord>,
    /// On elise's heaps, what it did during the turns.
    pub activity: Option<Activity>,
}

/// What elise's heap did during the turns, besides its pauses
/// (which the trace has): its young generation, its concurrent markings
/// and its id table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// The nursery's largest capacity at the end of a turn: the young
    /// garbage the peak used may hold.
    pub peak_nursery_bytes: usize,
    /// Objects allocated in the old generation because their site was
    /// tenured.
    pub pretenured_objects: u64,
    /// Concurrent markings finished during the turns, and of those, the
    /// ones the old generation's limit finished before the workers were
    /// done.
    pub concurrent_markings: u64,
    pub markings_at_limit: u64,
    /// The workers' time marking alongside the mutator during the turns.
    pub background_mark_ns: u64,
    /// The workers' time sweeping alongside the mutator during the turns,
    /// and the heap's thread's sweeping outside the pauses (GC time the
    /// trace doesn't show) and its pages.
    pub background_sweep_ns: u64,
    pub sweep_on_main_ns: u64,
    pub pages_swept_on_main: u64,
    /// The id table's largest size, in chunks, at the end of a turn.
    pub peak_id_chunks: usize,
    /// The free ranges allocation in the old generation took during the
    /// turns (its slow path), and their slots.
    pub old_ranges: u64,
    pub old_range_slots: u64,
    /// The old generation's fragmentation at the sweeps done during the
    /// turns, and at the markings started.
    pub fragmentation: FragmentationSummary,
    pub fragmentation_at_marking: FragmentationSummary,
}

/// The old generation's fragmentation each time the heap measured it at a
/// point of its GCs during the turns (`elise::Fragmentation`), shares in
/// thousandths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FragmentationSummary {
    /// How many times.
    pub count: usize,
    /// Of the old generation's committed bytes, the share of the pages V8's
    /// choice of pages to evacuate would free: the median over the times,
    /// and the most.
    pub released_median: u32,
    pub released_max: u32,
    /// Of the slots of the size-class pages with objects, the free share:
    /// the median and the most.
    pub free_median: u32,
    pub free_max: u32,
    /// The most bytes V8's choice would move, and the most size-class pages
    /// with objects.
    pub evacuated_max_bytes: usize,
    pub pages_max: usize,
}

impl FragmentationSummary {
    /// As a JSON object.
    fn json(&self) -> String {
        format!(
            "{{\"count\":{},\"released_median\":{},\"released_max\":{},\"free_median\":{},\
             \"free_max\":{},\"evacuated_max_bytes\":{},\"pages_max\":{}}}",
            self.count,
            self.released_median,
            self.released_max,
            self.free_median,
            self.free_max,
            self.evacuated_max_bytes,
            self.pages_max,
        )
    }
}

impl Report {
    /// One line of JSON, as `bench.py gc` reads it.
    pub fn to_json(&self) -> String {
        let mut json = String::new();
        let _ = write!(
            json,
            "{{\"workload\":\"{}\",\"heap\":\"{}\",\"scale\":{},\"turns\":{},\"nodes_allocated\":{},\
             \"run_ns\":{},\"idle_ns\":{},\"peak_committed_bytes\":{},\"peak_used_bytes\":{},\"live_nodes\":{},\
             \"forced_gc_ns\":{},\"after_forced_gc\":{{\"committed\":{},\"resident\":{},\"used\":{},\
             \"pooled\":{}}},\"baseline_used\":{},\"object_size\":{}{}}}",
            self.workload.name(),
            self.heap.name(),
            self.scale,
            self.turns,
            self.nodes_allocated,
            self.run.as_nanos(),
            self.idle.as_nanos(),
            self.peak_committed_bytes,
            self.peak_used_bytes,
            self.live_nodes,
            self.forced_gc.as_nanos(),
            self.after_forced_gc.committed_size_bytes,
            self.after_forced_gc.resident_size_bytes,
            self.after_forced_gc.used_size_bytes,
            self.after_forced_gc.pooled_memory_size_bytes,
            self.baseline_used_bytes,
            self.object_size,
            gc_json(self.gc, self.activity),
        );
        json
    }
}

/// A report's `gc` key and its young generation's, with a leading comma,
/// if it has elise's record.
pub(crate) fn gc_json(gc: Option<elise::GcRecord>, activity: Option<Activity>) -> String {
    gc.map_or_else(String::new, |gc| {
        let activity = activity.unwrap_or_default();
        format!(
            ",\"gc\":{{\"pause_ns\":{},\"mark_ns\":{},\"sweep_ns\":{},\"finalize_ns\":{},\
             \"marked_objects\":{},\"marked_bytes\":{},\"scanned_by_workers\":{}}},\
             \"peak_nursery_bytes\":{},\"pretenured_objects\":{},\"concurrent_markings\":{},\
             \"markings_at_limit\":{},\"background_mark_ns\":{},\"peak_id_chunks\":{},\
             \"background_sweep_ns\":{},\"sweep_on_main_ns\":{},\"pages_swept_on_main\":{},\
             \"old_ranges\":{},\"old_range_slots\":{},\"fragmentation\":{},\
             \"fragmentation_at_marking\":{}",
            gc.pause_ns,
            gc.mark_ns,
            gc.sweep_ns,
            gc.finalize_ns,
            gc.marked_objects,
            gc.marked_bytes,
            gc.scanned_by_workers,
            activity.peak_nursery_bytes,
            activity.pretenured_objects,
            activity.concurrent_markings,
            activity.markings_at_limit,
            activity.background_mark_ns,
            activity.peak_id_chunks,
            activity.background_sweep_ns,
            activity.sweep_on_main_ns,
            activity.pages_swept_on_main,
            activity.old_ranges,
            activity.old_range_slots,
            activity.fragmentation.json(),
            activity.fragmentation_at_marking.json(),
        )
    })
}

/// `count` scaled by `scale`, at least 1.
pub(crate) fn scaled(count: u32, scale: f64) -> usize {
    clamp_round::<usize>(f64::from(count) * scale).max(1)
}

/// A small, fixed-seed generator (xorshift64*): the same choices each run.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32;
        // `bound` is a count of nodes in memory: it fits in a u64, and the
        // remainder is below it (none of 0, which has no values below it).
        let remainder = value
            .checked_rem(saturated_cast::<u64, _>(bound))
            .unwrap_or(0);
        saturated_cast::<usize, _>(remainder)
    }
}

/// The patterns' generator (xorshift32): the same in `patterns.js`, so
/// every heap gets the same choices.
pub(crate) struct Rng32(pub(crate) u32);

impl Rng32 {
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        // `bound` is a group's size or 100: it fits in a u32 (and none is
        // 0, which has no values below it).
        let remainder = self
            .0
            .checked_rem(saturated_cast::<u32, _>(bound))
            .unwrap_or(0);
        saturated_cast::<usize, _>(remainder)
    }
}

/// The span of a workload's forced GC.
pub(crate) const FORCED_GC: &str = "gc_bench.forced_gc";

/// Runs `workload` at `scale` (1: the sizes in [`Workload`]'s and
/// [`Pattern`]'s docs) on a new heap of `heap`'s, then a forced full GC of
/// what is left. The spans `gc_bench.<workload>` and `gc_bench.forced_gc`
/// mark both in the trace. V8 must have started, with `--expose-gc` for
/// V8's heap (the forced GC).
///
/// # Panics
///
/// If `heap` doesn't run `workload` ([`Heap::runs`]).
pub fn run(heap: Heap, workload: Workload, scale: f64) -> Report {
    match (heap, workload) {
        (Heap::V8, Workload::Pattern(pattern, survival)) => v8_heap::run(pattern, survival, scale),
        _ => gc_heap::run(heap, workload, scale),
    }
}
