//! Each workload, small, on each heap: it does what its docs say (the
//! nodes it allocates and leaves alive), and the report reads the heap.
//! The patterns keep the same objects on every heap, V8's and
//! elise's, and their objects are as large on both of elise's.
//!
//! What catches a bug: a workload that keeps what it should drop, or drops
//! what it should keep (the live nodes, what the forced GC marked, and the
//! heap's used size after it), a pattern whose JS and Rust differ (the
//! live objects), or a report that doesn't measure (no peak, no time, no
//! nursery on elise's heap).

use gc_bench::{GROUP_SIZE, GROUPS_PER_TURN, Heap, Pattern, Report, Survival, Workload};

/// A hundredth of each workload's size: enough nodes to fill some of the
/// heap's pages, done in a moment.
const SCALE: f64 = 0.01;

fn run(heap: Heap, workload: Workload) -> Report {
    if heap.is_gc() {
        let report = gc_bench::gc_heap::run(heap, workload, SCALE);
        // The heap holds nothing of its own: its forced GC marked the
        // live nodes alone, and churn's container, off the tree but rooted
        // to the end.
        let record = report.gc.expect("elise's record");
        let container = usize::from(workload == Workload::Churn);
        assert_eq!(
            record.marked_objects,
            report.live_nodes + container,
            "{report:?}"
        );
        assert_eq!(report.baseline_used_bytes, 0, "{report:?}");
        // The nursery, 1 MiB at first, is part of what it reports, and the
        // markings (on one thread, none alongside).
        let activity = report.activity.expect("elise's activity");
        assert!(activity.peak_nursery_bytes >= 1 << 20, "{report:?}");
        if heap == Heap::Gc {
            assert_eq!(activity.concurrent_markings, 0, "{report:?}");
        }
        assert!(report.to_json().contains(&format!(
            "\"peak_nursery_bytes\":{},\"pretenured_objects\":{},\"concurrent_markings\":{},\
             \"markings_at_limit\":{},\"background_mark_ns\":{},\"peak_id_chunks\":{},\
             \"background_sweep_ns\":{},\"sweep_on_main_ns\":{},\"pages_swept_on_main\":{}",
            activity.peak_nursery_bytes,
            activity.pretenured_objects,
            activity.concurrent_markings,
            activity.markings_at_limit,
            activity.background_mark_ns,
            activity.peak_id_chunks,
            activity.background_sweep_ns,
            activity.sweep_on_main_ns,
            activity.pages_swept_on_main
        )));
        return report;
    }
    let report = gc_bench::run(heap, workload, SCALE);
    assert_eq!(report.activity, None);
    report
}

/// The nodes alive at the end, the root and the container included.
fn expected_live(workload: Workload) -> usize {
    match workload {
        // 1,000 nodes, all kept.
        Workload::Build => 2 + 1000,
        // A tree of 100 under the root; each turn puts back as many as it
        // took out.
        Workload::Churn => 1 + 100,
        // 10 under the container.
        Workload::Replace => 2 + 10,
        // 4 items of 5.
        Workload::List => 2 + 4 * 5,
        // One turn of 10,000 (not scaled: a turn is the table's chunks),
        // one in 16 kept.
        Workload::Ids => 2 + 10_000 / 16,
        Workload::Pattern(..) => unreachable!("checked on their own"),
    }
}

fn expected_allocated(workload: Workload, report: &Report) -> Option<usize> {
    match workload {
        Workload::Build => Some(1000),
        Workload::Replace => Some(5 * 10),
        Workload::List => Some(10 * 10 * 5),
        Workload::Ids => Some(10_000),
        // Each turn's subtree is the one it replaced, of a varying size.
        Workload::Churn => {
            assert!(report.nodes_allocated >= report.turns, "{report:?}");
            None
        }
        Workload::Pattern(..) => unreachable!("checked on their own"),
    }
}

fn check(heap: Heap, workload: Workload) {
    let report = run(heap, workload);
    assert_eq!(report.workload, workload);
    assert_eq!(report.heap, heap);
    assert_eq!(report.live_nodes, expected_live(workload), "{report:?}");
    if let Some(allocated) = expected_allocated(workload, &report) {
        assert_eq!(report.nodes_allocated, allocated, "{report:?}");
    }
    // The forced GC left about what is alive: the live nodes, and little
    // else (the heap's own few objects).
    let used = report.after_forced_gc.used_size_bytes;
    let live_bytes = report.live_nodes * gc_bench::NODE_SIZE;
    assert!(used >= live_bytes, "{report:?}");
    assert!(
        used <= live_bytes * 2 + 16 * 1024,
        "garbage survived the forced GC: {report:?}"
    );
    assert!(report.peak_used_bytes >= live_bytes, "{report:?}");
    assert!(
        report.peak_committed_bytes >= report.peak_used_bytes,
        "{report:?}"
    );
    // The line `bench.py gc` reads.
    let json = report.to_json();
    assert!(
        json.starts_with(&format!(
            "{{\"workload\":\"{}\",\"heap\":\"{}\"",
            workload.name(),
            heap.name()
        )),
        "{json}"
    );
    assert!(
        json.contains(&format!("\"live_nodes\":{}", report.live_nodes)),
        "{json}"
    );
}

/// The id table's workload, on elise's heaps alone: its 10,000
/// ids fill ten chunks.
#[test]
fn ids_on_the_gc_crates_heaps() {
    for heap in [Heap::Gc, Heap::GcParallel] {
        check(heap, Workload::Ids);
        let report = run(heap, Workload::Ids);
        assert_eq!(report.activity.unwrap().peak_id_chunks, 10, "{report:?}");
    }
    assert!(!Heap::V8.runs(Workload::Ids));
}

#[test]
fn the_doms_workloads_on_the_gc_crates_heap() {
    for heap in [Heap::Gc, Heap::GcParallel] {
        for workload in [
            Workload::Build,
            Workload::Churn,
            Workload::Replace,
            Workload::List,
        ] {
            check(heap, workload);
        }
    }
}

/// A pattern on every heap: the same objects kept on each (the same
/// choices, in Rust and in JS), about the share its survival says, and
/// none of the dropped left after the forced GC.
fn check_pattern(pattern: Pattern, survival: Survival) {
    let workload = Workload::Pattern(pattern, survival);
    let reports: Vec<Report> = Heap::ALL
        .into_iter()
        .map(|heap| run(heap, workload))
        .collect();
    // One turn at this scale.
    let groups = GROUPS_PER_TURN;
    let live = reports[0].live_nodes;
    let kept = (live - 1) / GROUP_SIZE;
    assert_eq!(
        (live - 1) % GROUP_SIZE,
        0,
        "whole groups and the root: {:?}",
        reports[0]
    );
    let (fewest, most) = match survival {
        Survival::High => (groups * 80 / 100, groups),
        Survival::Low => (1, groups * 15 / 100),
    };
    assert!(
        (fewest..=most).contains(&kept),
        "{kept} groups of {groups} kept: {:?}",
        reports[0]
    );
    let object_size = gc_bench::object_size(pattern.edges());
    for report in &reports {
        assert_eq!(report.workload, workload);
        if report.heap != Heap::V8 {
            assert_eq!(report.object_size, object_size, "{report:?}");
        }
        assert_eq!(
            report.live_nodes, live,
            "the heaps keep different objects: {report:?}"
        );
        assert_eq!(report.nodes_allocated, groups * GROUP_SIZE, "{report:?}");
        assert!(
            report.peak_committed_bytes >= report.peak_used_bytes,
            "{report:?}"
        );
        let used = report.after_forced_gc.used_size_bytes - report.baseline_used_bytes;
        // A JS object is at least a map and its fields; a Rust one has its
        // size. Garbage would be most of what was allocated, on low.
        let smallest = if report.heap == Heap::V8 {
            16
        } else {
            report.object_size
        };
        assert!(used >= live * smallest, "{report:?}");
        assert!(
            used <= live * 4 * smallest.max(48) + 64 * 1024,
            "garbage survived the forced GC: {report:?}"
        );
        assert!(
            report.peak_used_bytes >= report.baseline_used_bytes + live * smallest,
            "{report:?}"
        );
    }
}

#[test]
fn chain_high() {
    check_pattern(Pattern::Chain, Survival::High);
}

#[test]
fn chain_low() {
    check_pattern(Pattern::Chain, Survival::Low);
}

#[test]
fn tree_high() {
    check_pattern(Pattern::Tree, Survival::High);
}

#[test]
fn tree_low() {
    check_pattern(Pattern::Tree, Survival::Low);
}

#[test]
fn graph_high() {
    check_pattern(Pattern::Graph, Survival::High);
}

#[test]
fn graph_low() {
    check_pattern(Pattern::Graph, Survival::Low);
}

#[test]
#[should_panic(expected = "build doesn't run on the v8 heap")]
fn the_trees_workloads_need_the_gc_crates_heap() {
    run(Heap::V8, Workload::Build);
}

#[test]
fn names_parse_back() {
    for heap in Heap::ALL {
        assert_eq!(heap.name().parse::<Heap>(), Ok(heap));
    }
    for workload in Workload::ALL {
        assert_eq!(workload.name().parse::<Workload>(), Ok(workload));
    }
    assert!("spidermonkey".parse::<Heap>().is_err());
}
