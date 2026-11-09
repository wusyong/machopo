//! The allocation patterns on V8's own heap: `patterns.js`'s objects, as
//! many and linked as on elise's (`gc_heap`), with the same
//! choices (`Rng32`'s, in JS). A turn is one script run, after which the
//! event loop runs V8's platform tasks (its incremental marking's steps,
//! its finalization); V8 runs the rest on its own threads.

use std::time::{Duration, Instant};

use bindings::{ScriptState, V8PerIsolateData};

use crate::{
    FORCED_GC, GROUP_SIZE, GROUPS_PER_TURN, Heap, HeapStatistics, PATTERN_SEED, PATTERN_TURNS,
    Pattern, Report, Survival, Workload, scaled,
};

/// The patterns in JS, object for object as `gc_heap`'s and with the same
/// choices (`Rng32`).
const PATTERNS_JS: &str = include_str!("patterns.js");

/// The isolate's heap's sizes now.
fn sizes(data: &mut V8PerIsolateData) -> HeapStatistics {
    let stats = data.isolate().get_heap_statistics();
    HeapStatistics {
        committed_size_bytes: stats.total_heap_size(),
        resident_size_bytes: stats.total_physical_size(),
        used_size_bytes: stats.used_heap_size(),
        pooled_memory_size_bytes: 0,
    }
}

/// The event loop's view of the heap: after each turn it keeps the heap's
/// largest sizes, then runs V8's tasks.
struct EventLoop {
    peak_committed: usize,
    peak_used: usize,
}

impl EventLoop {
    fn new() -> Self {
        Self {
            peak_committed: 0,
            peak_used: 0,
        }
    }

    /// The end of a turn: the heap measured as the turn left it, then its
    /// tasks ready run, as the loop's next tasks would.
    fn turn(&mut self, data: &mut V8PerIsolateData) {
        let stats = sizes(data);
        self.peak_committed = self.peak_committed.max(stats.committed_size_bytes);
        self.peak_used = self.peak_used.max(stats.used_size_bytes);
        data.pump_platform_tasks();
    }

    /// Runs the tasks V8 posted until none is left, then measures.
    fn finish(&mut self, data: &mut V8PerIsolateData) {
        // Bounded: a heap that keeps posting is a bug the numbers show.
        for _ in 0..10_000 {
            if !data.pump_platform_tasks() {
                break;
            }
        }
        self.turn(data);
    }
}

/// The heap's used size after a full GC: what it holds of its own (the
/// script's and the realm's) before the pattern allocates.
fn baseline(data: &mut V8PerIsolateData) -> usize {
    data.collect_garbage_for_testing();
    data.collect_garbage_for_testing();
    sizes(data).used_size_bytes
}

/// The forced full GC at the end, in the span `FORCED_GC`, and the heap's
/// sizes after it.
fn forced_gc(data: &mut V8PerIsolateData) -> (Duration, HeapStatistics) {
    // Twice, the first unmeasured: a GC already marking would make the
    // forced one only finish it.
    data.collect_garbage_for_testing();
    bindings::trace::begin(FORCED_GC);
    let start = Instant::now();
    data.collect_garbage_for_testing();
    let forced_gc = start.elapsed();
    bindings::trace::end();
    (forced_gc, sizes(data))
}

/// Runs `source` in `script`'s realm: its completion value.
fn js(data: &mut V8PerIsolateData, script: &ScriptState, source: &str) -> String {
    script
        .evaluate_classic_script(data, source)
        .unwrap_or_else(|error| panic!("gc_bench's script threw: {error:?}"))
}

/// Runs `pattern` at `scale` on a new isolate's heap, then a forced full
/// GC of what is left. V8 must have started with `--expose-gc`.
pub fn run(pattern: Pattern, survival: Survival, scale: f64) -> Report {
    let workload = Workload::Pattern(pattern, survival);
    let mut data = V8PerIsolateData::new();
    let script = ScriptState::new(&mut data);
    js(&mut data, &script, PATTERNS_JS);
    let baseline_used_bytes = baseline(&mut data);
    let setup = format!(
        "setup({}, {GROUP_SIZE}, {GROUPS_PER_TURN}, {}, {PATTERN_SEED})",
        pattern.edges(),
        survival.percent()
    );
    js(&mut data, &script, &setup);
    // What the setup left to the heap, done first.
    EventLoop::new().finish(&mut data);
    let mut event_loop = EventLoop::new();
    bindings::trace::begin(&format!("gc_bench.{}", workload.name()));
    let start = Instant::now();
    let turns = scaled(PATTERN_TURNS, scale);
    for _ in 0..turns {
        js(&mut data, &script, "turn()");
        event_loop.turn(&mut data);
    }
    event_loop.finish(&mut data);
    let run = start.elapsed();
    bindings::trace::end();
    let live_nodes = js(&mut data, &script, "count()")
        .parse()
        .expect("count() gives a number");
    let (forced_gc, after_forced_gc) = forced_gc(&mut data);
    js(&mut data, &script, "reset()");
    Report {
        workload,
        heap: Heap::V8,
        scale,
        turns,
        nodes_allocated: turns.saturating_mul(GROUPS_PER_TURN * GROUP_SIZE),
        run,
        idle: Duration::ZERO,
        peak_committed_bytes: event_loop.peak_committed,
        peak_used_bytes: event_loop.peak_used,
        live_nodes,
        forced_gc,
        after_forced_gc,
        baseline_used_bytes,
        object_size: 0,
        gc: None,
        activity: None,
    }
}
