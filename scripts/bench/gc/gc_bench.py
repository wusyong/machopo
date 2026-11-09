#!/usr/bin/env python3
"""`bench.py gc`: the GC microbenchmarks, `gc_bench` (the crate of that
name), on each heap: the `gc` crate's (`gc`, marking on one thread;
`gc-parallel`, on a worker for each core but one), and V8's own for JS
objects (`v8`, with V8's flags as machopo runs JS; `v8-single`, with
`--single-threaded-gc`: one core's speed).

The DOM-like workloads on a tree (`build`, `churn`, `replace`, `list`)
run on the `gc` crate's heaps; the allocation patterns (`chain`, `tree`,
`graph`, each `-high` or `-low`) on all four. Each runs `--runs` times,
each run a `gc_bench` process of its own (the `perf` profile) with a
trace. `gc_bench` reports
what it measured itself: the turns' time, the heap's peak sizes, its size
before the workload, the nodes left alive and a forced full GC of them.
From the trace this reads the GC's work during the turns: on the main
thread, the outermost GC slices (cppgc's `CppGC.*`, V8's `MinorGC`,
`MajorGC` and `V8.GC*`, the `gc` crate's `Gc.*`), how long they took in all
and by kind (incremental marking, the atomic pause, which is all of a `gc`
crate's `Gc.Pause`, sweeping), and the pauses: slices less than
`PAUSE_GAP_NS` apart are one pause, as nothing ran between them (an atomic
pause is several cppgc phases back to back); on the other threads, the GC
slices inside the turns (parallel scavenging, concurrent marking and
sweeping). On V8's heap, `--trace-gc-nvp`'s line for each scavenge (paired
with the trace's `MinorGC` slices in order) gives the bytes that survived
it, and the scavenge's speed is those over its time, as V8's GCTracer
reckons it. Each number is the median over the runs; the pauses'
distribution is over every run's pauses. The marking's throughput is the
bytes left after the forced GC (over the heap's own before the workload),
and the live objects, over its marking: the outermost `V8.GC_MC_MARK`,
`CppGC.AtomicMark` or `Gc.Mark`. On the `gc` crate's heaps, `gc_bench`'s
own record of the forced GC must agree with the trace: the bytes it
marked, and its marking's time. On those heaps, the `gc` crate's
concurrent markings too: their starts' pauses (a `Gc.Pause` holding
`Gc.MarkStart`) and their ends' (one holding `Gc.Mark` but no
`Gc.Scavenge`: a full GC on one thread starts with one), split into the
end's marking and the rest (sweeping, finalizing); the weak edges'
clearing in every full GC (`Gc.Weak`, the id table's walk in it); and from
`gc_bench`'s report, how many markings finished, how many at the old
generation's limit, the workers' time marking alongside, and the id
table's largest size. The `ids` workload (the id table at a large
document's size) runs on those heaps alone.

`--baseline FILE` takes an earlier run's `gc.json`: its rows of the heaps
this run leaves out go in the report next to this run's, marked
`(baseline)`, with the run they came from. An earlier run's rows of
cppgc's heaps (`standalone`, `isolate`, which `gc_bench` no longer has)
compare with the `gc` crate's this way; a comparison in one run is for
when V8 or the machine changes.

The results go to `target/bench/gc/gc.md` (and `gc.json`), the traces next
to them. Standard library only.
"""

from __future__ import annotations

import json
import math
import statistics
import subprocess
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import common  # noqa: E402
from common import ROOT  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "speedometer"))
import speedometer  # noqa: E402

RESULTS = ROOT / "target" / "bench" / "gc"

# Each heap's `gc_bench` arguments.
HEAP_ARGS = {
    "v8": ["--heap", "v8", "--v8-flags", "--trace-gc-nvp"],
    "v8-single": ["--heap", "v8", "--v8-flags", "--trace-gc-nvp --single-threaded-gc"],
    "gc": ["--heap", "gc"],
    "gc-parallel": ["--heap", "gc-parallel"],
}
HEAPS = tuple(HEAP_ARGS)
DOM_WORKLOADS = ("build", "churn", "replace", "list")
PATTERNS = tuple(f"{shape}-{survival}" for shape in ("chain", "tree", "graph") for survival in ("high", "low"))
# The `gc` crate's alone (its id table).
GC_WORKLOADS = ("ids",)
WORKLOADS = DOM_WORKLOADS + PATTERNS + GC_WORKLOADS

# GC slices closer than this are one pause: nothing else ran on the thread
# between them.
PAUSE_GAP_NS = 10_000

# V8's GC events that `speedometer.slice_phase` doesn't name: the outer
# ones of a full and a young GC.
V8_GC_SLICES = {"MajorGC", "MinorGC"}
# A full GC's marking: V8's (which holds the CppHeap's on an isolate),
# cppgc's and the `gc` crate's.
MARK_SLICES = {"V8.GC_MC_MARK", "CppGC.AtomicMark", "Gc.Mark"}
# `gc_kind`'s kinds.
KINDS = ("marking", "atomic", "young", "sweeping", "other")
# V8's incremental marking on the main thread: its steps, and its start
# (which marks the roots, the CppHeap's among them).
INCREMENTAL_MARKING = {"V8.GCIncrementalMarking", "V8.GCIncrementalMarkingStart"}


def runs_on(heap: str, workload: str) -> bool:
    """Whether `gc_bench` runs `workload` on `heap`: V8's runs the
    patterns alone (the tree's workloads and the id table are the `gc`
    crate's)."""
    if workload in GC_WORKLOADS:
        return heap.startswith("gc")
    return not heap.startswith("v8") or workload in PATTERNS


def is_gc(name: str) -> bool:
    return name in V8_GC_SLICES or name.startswith("Gc.") or speedometer.slice_phase("machopo", name) == "GC"


def gc_kind(name: str) -> str:
    """What part of a GC an outermost slice is: cppgc's by its scope, V8's
    by its event (a scavenge is the young generation's), the `gc` crate's
    a whole pause (`Gc.Pause`, a full GC) or a scavenge on its own
    (`Gc.Scavenge`)."""
    if name in ("MinorGC", "Gc.Scavenge") or "SCAVENGER" in name:
        return "young"
    if name.startswith("CppGC.Incremental") and "Mark" in name or name in INCREMENTAL_MARKING:
        return "marking"
    if "Sweep" in name or "SWEEP" in name:
        return "sweeping"
    if (name.startswith("CppGC.Atomic") or name.startswith("Gc.") or name in V8_GC_SLICES
            or "MARK_COMPACTOR" in name):
        return "atomic"
    return "other"


def span_track(events: list[tuple], span: str) -> int:
    track = next((event[0] for event in events if event[3] == 1 and event[4] == span), None)
    if track is None:
        raise SystemExit(f"the trace has no span {span}")
    return track


def slices(events: list[tuple], track: int) -> list[tuple[int, int, str, list[str]]]:
    """The slices `(begin, end, name, outer names)` on `track`, in the
    order they end."""
    result = []
    stack: list[tuple[int, str]] = []
    for _, time, _, kind, name, _ in sorted((e for e in events if e[0] == track), key=lambda e: (e[1], e[2])):
        if kind == 1:
            stack.append((time, name))
        elif kind == 2 and stack:
            begin, name = stack.pop()
            result.append((begin, time, name, [outer for _, outer in stack]))
    return result


def gc_slices(events: list[tuple], span: str) -> tuple[list[tuple[int, int, str]], list[tuple[int, int, str]]]:
    """The outermost GC slices `(begin, end, name)` on the track of the
    span `span` (`gc_bench`'s, on the main thread), inside it, and the
    outermost marking slices inside it (`MARK_SLICES`, at any depth: on an
    isolate's heap cppgc's are in V8's events)."""
    outermost, marks = [], []
    for begin, end, name, outer in slices(events, span_track(events, span)):
        if span not in outer or not is_gc(name):
            continue
        if not any(is_gc(name) for name in outer):
            outermost.append((begin, end, name))
        if name in MARK_SLICES and not any(name in MARK_SLICES for name in outer):
            marks.append((begin, end, name))
    outermost.sort()
    marks.sort()
    return outermost, marks


def span_bounds(events: list[tuple], span: str) -> tuple[int, int]:
    for begin, end, name, _ in slices(events, span_track(events, span)):
        if name == span:
            return begin, end
    raise SystemExit(f"the span {span} never ends")


def background_gc_ns(events: list[tuple], span: str) -> int:
    """The GC's time on the threads besides the span's, inside the span:
    each thread's outermost GC slices, cut at the span's ends."""
    first, last = span_bounds(events, span)
    return background_gc_between(events, span_track(events, span), first, last)


def background_gc_between(events: list[tuple], main: int, first: int, last: int) -> int:
    """The GC's time on the threads besides `main`'s track between `first`
    and `last`: each thread's outermost GC slices, cut there."""
    total = 0
    for track in {event[0] for event in events} - {main}:
        for begin, end, name, outer in slices(events, track):
            if is_gc(name) and not any(is_gc(name) for name in outer):
                total += max(0, min(end, last) - max(begin, first))
    return total


# The phases `longest_pause` lists: the most time of its own each took.
PAUSE_PHASES = 5


def is_gc_phase(name: str) -> bool:
    """Whether a slice is a GC's or a phase of one: V8's, cppgc's, the `gc`
    crate's."""
    return is_gc(name) or name.startswith(("V8.GC", "CppGC.", "Gc."))


def window_gc(events: list[tuple], main: int, first: int, last: int) -> dict:
    """The GC between `first` and `last` (ns), as `analyse` reckons a
    workload's turns: on the `main` track, the outermost GC slices (cut at
    the ends), by kind, and the pauses; of them, the time of the `gc`
    crate's slices inside an engine's GC slice (a heap V8 drives: its steps
    and its pause's marking, as V8's `MC_*_EMBEDDER_TRACING` scopes hold
    cppgc's); the young generation's by heap (the `gc` crate's own
    scavenges, `Gc.Scavenge`, and the engine's); the longest pause and
    the phases that took most of it (`longest_pause`); on the other
    threads, their time."""
    outermost = []
    in_engine = 0
    inside = []
    for begin, end, name, outer in slices(events, main):
        if end <= first or begin >= last:
            continue
        if is_gc_phase(name):
            inside.append((max(begin, first), min(end, last), name, len(outer)))
        if not is_gc(name):
            continue
        if not any(is_gc(name) for name in outer):
            outermost.append((max(begin, first), min(end, last), name))
        elif name.startswith("Gc.") and not any(name.startswith("Gc.") for name in outer):
            in_engine += min(end, last) - max(begin, first)
    outermost.sort()
    by_kind: dict[str, int] = {}
    young: dict[str, int] = {}
    for begin, end, name in outermost:
        by_kind[gc_kind(name)] = by_kind.get(gc_kind(name), 0) + end - begin
        if gc_kind(name) == "young":
            heap = "gc" if name == "Gc.Scavenge" else "engine"
            young[heap] = young.get(heap, 0) + end - begin
    return {
        "window_ns": last - first,
        "gc_ns": sum(end - begin for begin, end, _ in outermost),
        "gc_by_kind_ns": by_kind,
        "young_by_heap_ns": young,
        "pauses_ns": pauses(outermost),
        "longest_pause": longest_pause(outermost, inside),
        "in_engine_ns": in_engine,
        "background_gc_ns": background_gc_between(events, main, first, last),
    }


def longest_pause(outermost: list[tuple[int, int, str]], inside: list[tuple[int, int, str, int]]) -> dict:
    """The longest pause (`pauses`' runs of slices) and the phases that took
    most of it: each GC slice's time of its own (but its phases'), summed by
    name, the `PAUSE_PHASES` most. `inside`: the GC slices and phases
    `(begin, end, name, depth)` on the track, in the order they end."""
    spans: list[list[int]] = []
    for begin, end, _ in outermost:
        if spans and begin - spans[-1][1] < PAUSE_GAP_NS:
            spans[-1][1] = max(spans[-1][1], end)
        else:
            spans.append([begin, end])
    if not spans:
        return {"ns": 0, "phases": []}
    begin, end = max(spans, key=lambda span: span[1] - span[0])
    within = [one for one in inside if one[0] >= begin and one[1] <= end]
    own: dict[str, int] = {}
    for start, stop, name, depth in within:
        # Its phases: the slices one deeper inside it.
        phases = sum(b_stop - b_start for b_start, b_stop, _, b_depth in within
                     if b_depth == depth + 1 and b_start >= start and b_stop <= stop)
        own[name] = own.get(name, 0) + max(0, stop - start - phases)
    ranked = sorted(own.items(), key=lambda item: (-item[1], item[0]))[:PAUSE_PHASES]
    return {"ns": end - begin, "phases": [[name, ns] for name, ns in ranked]}


def marking_phases(events: list[tuple], span: str) -> dict:
    """The `gc` crate's concurrent markings inside the span, from its
    `Gc.Pause` slices and what each holds: the starts' pauses (holding
    `Gc.MarkStart`), the ends' (holding `Gc.Mark` but no `Gc.Scavenge`: a
    full GC on one thread scavenges first) and the end's `Gc.Mark` in each;
    and every full GC's `Gc.Weak` (the weak edges' and the id table's
    clearing), all in ns."""
    pauses, children, weak = [], [], []
    for begin, end, name, outer in slices(events, span_track(events, span)):
        if span not in outer:
            continue
        if name == "Gc.Pause" and not any(is_gc(one) for one in outer):
            pauses.append((begin, end))
        elif outer and outer[-1] == "Gc.Pause":
            children.append((begin, end, name))
        if name == "Gc.Weak":
            weak.append(end - begin)
    starts, ends, ends_mark = [], [], []
    for begin, end in sorted(pauses):
        inside = [(b, e, name) for b, e, name in children if begin <= b and e <= end]
        names = {name for _, _, name in inside}
        if "Gc.MarkStart" in names:
            starts.append(end - begin)
        elif "Gc.Mark" in names and "Gc.Scavenge" not in names:
            ends.append(end - begin)
            ends_mark.append(sum(e - b for b, e, name in inside if name == "Gc.Mark"))
    return {"start_pauses_ns": starts, "end_pauses_ns": ends, "end_mark_ns": ends_mark, "weak_ns": weak}


def nvp_scavenges(output: str) -> list[dict]:
    """V8's `--trace-gc-nvp` lines (`GC: {...}`) of the scavenges, in
    order."""
    lines = []
    for line in output.splitlines():
        _, mark, data = line.partition("GC: {")
        if mark:
            record = json.loads("{" + data)
            if record.get("gc") == "s":
                lines.append(record)
    return lines


def young_gc(events: list[tuple], span: str, scavenges: list[dict]) -> dict:
    """The scavenges inside the span: how many, the bytes that survived
    them (kept young or promoted), their main-thread time (the `MinorGC`
    slices, paired in order with V8's lines) and the young generation's
    largest capacity."""
    minor = sorted((begin, end) for begin, end, name, outer in slices(events, span_track(events, span))
                   if name == "MinorGC" and not any(is_gc(name) for name in outer))
    if len(minor) != len(scavenges):
        raise SystemExit(f"{len(minor)} MinorGC slices in the trace but {len(scavenges)} scavenges from V8")
    first, last = span_bounds(events, span)
    inside = [(end - begin, record) for (begin, end), record in zip(minor, scavenges) if first <= begin and end <= last]
    return {
        "count": len(inside),
        "survived_bytes": sum(record["new_space_survived"] + record["promoted"] for _, record in inside),
        "ns": sum(ns for ns, _ in inside),
        "capacity_bytes": max((record["new_space_capacity"] for _, record in inside), default=0),
    }


def pauses(slices: list[tuple[int, int, str]]) -> list[int]:
    """The pauses, in ns: runs of slices less than `PAUSE_GAP_NS` apart."""
    result = []
    begin = end = None
    for start, stop, _ in slices:
        if end is not None and start - end < PAUSE_GAP_NS:
            end = max(end, stop)
            continue
        if begin is not None:
            result.append(end - begin)
        begin, end = start, stop
    if begin is not None:
        result.append(end - begin)
    return result


def young_pauses(slices: list[tuple[int, int, str]]) -> list[int]:
    """The pauses, in ns, of the young generation's slices alone (a full
    GC's scavenge is inside its pause, not one of them)."""
    return pauses([one for one in slices if gc_kind(one[2]) == "young"])


def percentile(values: list[int], fraction: float) -> int:
    """The value `fraction` of the way up `values` (nearest rank)."""
    if not values:
        return 0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, math.ceil(fraction * len(ordered)) - 1))]


def analyse(report: dict, trace: Path, output: str = "") -> dict:
    """One run's numbers: `gc_bench`'s report, what its trace adds, and on
    V8's heap what V8's lines in `output` add. A trace that lost events
    stops it: its GC would be short of what ran."""
    lost: list[str] = []
    _, events = common.perfetto_track_events(trace, lost)
    if lost:
        raise SystemExit(f"{trace} lost events on {', '.join(lost)}")
    span = f"gc_bench.{report['workload']}"
    turns, _ = gc_slices(events, span)
    _, forced_marks = gc_slices(events, "gc_bench.forced_gc")
    by_kind: dict[str, int] = {}
    for begin, end, name in turns:
        by_kind[gc_kind(name)] = by_kind.get(gc_kind(name), 0) + end - begin
    # The `gc` crate's heap's thread's sweeping outside the pauses (a page
    # for an allocation, the workers' results applied), which the trace
    # leaves out: GC time, of the sweeping kind.
    on_main = report.get("sweep_on_main_ns", 0)
    if on_main:
        by_kind["sweeping"] = by_kind.get("sweeping", 0) + on_main
    scavenges = nvp_scavenges(output)
    forced_mark_ns = sum(end - begin for begin, end, _ in forced_marks)
    if "gc" in report:
        check_gc_record(report, forced_mark_ns)
    return {
        **report,
        "gc_ns": sum(end - begin for begin, end, _ in turns) + on_main,
        "gc_by_kind_ns": by_kind,
        "background_gc_ns": background_gc_ns(events, span),
        "pauses_ns": pauses(turns),
        "young_pauses_ns": young_pauses(turns),
        "forced_mark_ns": forced_mark_ns,
        "young": young_gc(events, span, scavenges) if scavenges else None,
        "marking": marking_phases(events, span) if "gc" in report else None,
    }


# How far the trace's marking slice may be from the `gc` crate's own
# timing of it, which encloses it: the trace's calls, and a slack for
# timers read apart.
MARK_SLACK_NS = 20_000


def check_gc_record(report: dict, forced_mark_ns: int) -> None:
    """Stops if the `gc` crate's record of the forced GC disagrees with
    the report and the trace: the bytes it marked are those left (the heap
    holds nothing of its own), and its marking is the trace's `Gc.Mark`."""
    record = report["gc"]
    live = report["after_forced_gc"]["used"] - report["baseline_used"]
    if record["marked_bytes"] != live:
        raise SystemExit(f"the gc crate marked {record['marked_bytes']} bytes, but {live} are left: {report}")
    if not record["mark_ns"] - MARK_SLACK_NS <= forced_mark_ns <= record["mark_ns"] + MARK_SLACK_NS:
        raise SystemExit(f"the trace's marking took {forced_mark_ns} ns, the gc crate's {record['mark_ns']} ns: "
                         f"{report}")


def summarize(runs: list[dict]) -> dict:
    """The median of each number over `runs`, the pauses' distribution
    over all of theirs."""
    def median(key, of=None):
        return statistics.median((run[key] if of is None else run[key].get(of, 0)) for run in runs)

    every_pause = [pause for run in runs for pause in run["pauses_ns"]]
    every_young_pause = [pause for run in runs for pause in run.get("young_pauses_ns", [])]
    live = max(0, runs[0]["after_forced_gc"]["used"] - runs[0]["baseline_used"])
    mark_ns = median("forced_mark_ns")
    young = None
    if all(run["young"] for run in runs):
        young = {
            "count": statistics.median(run["young"]["count"] for run in runs),
            "survived_bytes": statistics.median(run["young"]["survived_bytes"] for run in runs),
            "ns": statistics.median(run["young"]["ns"] for run in runs),
            "bytes_per_s": statistics.median(
                run["young"]["survived_bytes"] / (run["young"]["ns"] / 1e9) for run in runs if run["young"]["ns"])
            if any(run["young"]["ns"] for run in runs) else None,
            "capacity_bytes": max(run["young"]["capacity_bytes"] for run in runs),
        }
    return {
        "runs": len(runs),
        "turns": runs[0]["turns"],
        "nodes_allocated": runs[0]["nodes_allocated"],
        "live_nodes": runs[0]["live_nodes"],
        "run_ns": median("run_ns"),
        "idle_ns": median("idle_ns"),
        "gc_ns": median("gc_ns"),
        "gc_by_kind_ns": {kind: median("gc_by_kind_ns", kind) for kind in KINDS},
        "background_gc_ns": median("background_gc_ns"),
        "pauses": {
            "count": statistics.median(len(run["pauses_ns"]) for run in runs),
            "median_ns": percentile(every_pause, 0.5),
            "p95_ns": percentile(every_pause, 0.95),
            "max_ns": max(every_pause, default=0),
        },
        "young_pauses": {
            "count": statistics.median(len(run.get("young_pauses_ns", [])) for run in runs),
            "median_ns": percentile(every_young_pause, 0.5),
            "p95_ns": percentile(every_young_pause, 0.95),
            "p99_ns": percentile(every_young_pause, 0.99),
            "max_ns": max(every_young_pause),
        } if every_young_pause else None,
        "peak_committed_bytes": median("peak_committed_bytes"),
        "peak_used_bytes": median("peak_used_bytes"),
        # The `gc` crate's: the most over the runs (the same every run).
        "peak_nursery_bytes": max((run["peak_nursery_bytes"] for run in runs if "peak_nursery_bytes" in run),
                                  default=None),
        "pretenured_objects": median("pretenured_objects") if all("pretenured_objects" in run for run in runs)
        else None,
        "live_bytes": live,
        "bytes_per_object": live / runs[0]["live_nodes"] if runs[0]["live_nodes"] else None,
        "forced_gc_ns": median("forced_gc_ns"),
        "forced_mark_ns": mark_ns,
        "mark_bytes_per_s": live / (mark_ns / 1e9) if mark_ns else None,
        "mark_objects_per_s": runs[0]["live_nodes"] / (mark_ns / 1e9) if mark_ns else None,
        "young": young,
        "marking": summarize_marking(runs),
        "fragmentation": summarize_fragmentation(runs, "fragmentation"),
        "fragmentation_at_marking": summarize_fragmentation(runs, "fragmentation_at_marking"),
    }


def summarize_marking(runs: list[dict]) -> dict | None:
    """On the `gc` crate's heaps, its concurrent markings over the runs:
    the medians of the counts and the workers' time, the pauses' and the
    weak clearings' distributions over all of them, the id table's largest
    size."""
    if not all(run.get("marking") is not None for run in runs):
        return None
    def every(key):
        return [value for run in runs for value in run["marking"][key]]
    starts, ends, ends_mark, weak = every("start_pauses_ns"), every("end_pauses_ns"), every("end_mark_ns"), \
        every("weak_ns")
    return {
        "count": statistics.median(run.get("concurrent_markings", 0) for run in runs),
        "at_limit": statistics.median(run.get("markings_at_limit", 0) for run in runs),
        "background_ns": statistics.median(run.get("background_mark_ns", 0) for run in runs),
        "start_p50_ns": percentile(starts, 0.5),
        "start_max_ns": max(starts, default=0),
        "end_p50_ns": percentile(ends, 0.5),
        "end_p99_ns": percentile(ends, 0.99),
        "end_max_ns": max(ends, default=0),
        "end_mark_p50_ns": percentile(ends_mark, 0.5),
        "end_mark_p99_ns": percentile(ends_mark, 0.99),
        "end_mark_max_ns": max(ends_mark, default=0),
        "weak_p50_ns": percentile(weak, 0.5),
        "weak_max_ns": max(weak, default=0),
        "peak_id_chunks": max(run.get("peak_id_chunks", 0) for run in runs),
        "background_sweep_ns": statistics.median(run.get("background_sweep_ns", 0) for run in runs),
        "sweep_on_main_ns": statistics.median(run.get("sweep_on_main_ns", 0) for run in runs),
        "pages_swept_on_main": statistics.median(run.get("pages_swept_on_main", 0) for run in runs),
    }


def summarize_fragmentation(runs: list[dict], when: str) -> dict | None:
    """On the `gc` crate's heaps, its old generation each time the heap
    measured it during the turns at `when` (`fragmentation`, a sweep done;
    `fragmentation_at_marking`, a marking's start), shares in thousandths:
    the medians of the runs' counts and medians, the most of their mosts;
    and its allocation's free ranges, the median of the runs' slots a
    range."""
    if not all(run.get(when) is not None for run in runs):
        return None
    def every(key):
        return [run[when][key] for run in runs]
    return {
        "count": statistics.median(every("count")),
        "released_median": statistics.median(every("released_median")),
        "released_max": max(every("released_max")),
        "free_median": statistics.median(every("free_median")),
        "free_max": max(every("free_max")),
        "evacuated_max_bytes": max(every("evacuated_max_bytes")),
        "pages_max": max(every("pages_max")),
        "ranges": statistics.median(run.get("old_ranges", 0) for run in runs),
        "slots_per_range": statistics.median(
            run.get("old_range_slots", 0) / run["old_ranges"] if run.get("old_ranges") else 0 for run in runs),
    }


def ms(ns: float) -> str:
    return f"{ns / 1e6:.2f}"


def us(ns: float) -> str:
    return f"{ns / 1e3:.1f}"


def mib(size: float) -> str:
    return f"{size / 2**20:.1f}"


def rate(bytes_per_s: float | None) -> str:
    return "" if bytes_per_s is None else f"{bytes_per_s / 1e6:.0f}"


def millions(per_s: float | None) -> str:
    return "" if per_s is None else f"{per_s / 1e6:.2f}"


# A row of an earlier run's: its heap's name and this.
BASELINE = " (baseline)"


def add_baseline(results: dict[str, dict[str, dict]], baseline: dict, heaps: list[str]) -> None:
    """`baseline`'s rows (an earlier run's `gc.json`) of the workloads in
    `results` and the heaps not in `heaps`, first, marked `BASELINE`."""
    for name, rows in results.items():
        earlier = {f"{heap}{BASELINE}": row for heap, row in baseline["results"].get(name, {}).items()
                   if heap not in heaps and not heap.endswith(BASELINE)}
        results[name] = {**earlier, **rows}


def baseline_note(env: dict) -> str:
    baseline = env.get("baseline")
    if not baseline:
        return ""
    return (f" Rows marked (baseline) are from {baseline['file']}: machopo {baseline['commit']}, "
            f"{baseline['date']}, {baseline['platform']}.")


def write_report(results: dict[str, dict[str, dict]], env: dict) -> Path:
    lines = [
        f"# GC microbenchmarks ({env['date']})",
        "",
        f"machopo {env['commit']}, {env['platform']}; `gc_bench` (`perf`), {env['runs']} runs each, "
        f"scale {env['scale']}. Times in ms, sizes in MiB, speeds in MB/s (objects in millions a second); "
        "medians over the runs." + baseline_note(env),
        "",
        "During the turns: the run, the main thread's GC, "
        "its share of the run and by kind (incremental marking, a full GC's atomic pause, V8's scavenges, "
        "sweeping), the other threads' GC (which may overlap the main thread's pauses: V8 hands even a single "
        "scavenging task to a worker while the main thread waits), and the main thread's pauses (over every "
        "run's).",
        "",
        "| Workload | Heap | Run | GC | GC % | Marking | Atomic | Young | Sweeping | Background | Pauses | Median "
        "| p95 | Max |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for workload, heaps in results.items():
        for heap, row in heaps.items():
            kinds = row["gc_by_kind_ns"]
            share = 100 * row["gc_ns"] / row["run_ns"] if row["run_ns"] else 0
            lines.append(
                f"| {workload} | {heap} | {ms(row['run_ns'])} | {ms(row['gc_ns'])} | {share:.0f} | "
                f"{ms(kinds['marking'])} | {ms(kinds['atomic'])} | {ms(kinds['young'])} | {ms(kinds['sweeping'])} | "
                f"{ms(row['background_gc_ns'])} | "
                f"{row['pauses']['count']:g} | {ms(row['pauses']['median_ns'])} | {ms(row['pauses']['p95_ns'])} | "
                f"{ms(row['pauses']['max_ns'])} |")
    lines += [
        "",
        "Memory and the forced full GC of what is alive at the end: the peak sizes after a turn (and on the `gc` "
        "crate's heaps the nursery's largest capacity then, the young garbage the peak may hold, and the objects "
        "allocated in the old generation because their site was tenured), the live nodes "
        "(objects) and the bytes they take after the forced GC (over the heap's own before the workload), its "
        "time and the marking's speed (those bytes, and the live objects, over its outermost marking slice).",
        "",
        "| Workload | Heap | Allocated | Peak committed | Peak used | Nursery | Pretenured | Live | Live MiB "
        "| Bytes each | Forced GC | Marking | Objects |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for workload, heaps in results.items():
        for heap, row in heaps.items():
            each = row["bytes_per_object"]
            nursery = row.get("peak_nursery_bytes")
            pretenured = row.get("pretenured_objects")
            lines.append(
                f"| {workload} | {heap} | {row['nodes_allocated']} | {mib(row['peak_committed_bytes'])} | "
                f"{mib(row['peak_used_bytes'])} | {'' if nursery is None else mib(nursery)} | "
                f"{'' if pretenured is None else f'{pretenured:g}'} | {row['live_nodes']} | "
                f"{mib(row['live_bytes'])} | "
                f"{'' if each is None else f'{each:.0f}'} | {ms(row['forced_gc_ns'])} | "
                f"{rate(row['mark_bytes_per_s'])} | {millions(row['mark_objects_per_s'])} |")
    young = [(workload, heap, row) for workload, heaps in results.items() for heap, row in heaps.items()
             if row["young"]]
    if young:
        lines += [
            "",
            "V8's scavenges during the turns: how many, the bytes that survived them (kept young or promoted), "
            "their main-thread time, the speed (survived bytes over that time, as V8's GCTracer) and the young "
            "generation's largest capacity.",
            "",
            "| Workload | Heap | Scavenges | Survived MiB | Time | Speed | Capacity MiB |",
            "| --- | --- | --- | --- | --- | --- | --- |",
        ]
        for workload, heap, row in young:
            data = row["young"]
            lines.append(
                f"| {workload} | {heap} | {data['count']:g} | {mib(data['survived_bytes'])} | {ms(data['ns'])} | "
                f"{rate(data['bytes_per_s'])} | {mib(data['capacity_bytes'])} |")
    minor = [(workload, heap, row) for workload, heaps in results.items() for heap, row in heaps.items()
             if row.get("young_pauses")]
    if minor:
        lines += [
            "",
            "The main thread's pauses for the young generation alone (V8's scavenges, the `gc` crate's scavenges "
            "outside a full GC), over every run's: how many a run, and their distribution, in µs.",
            "",
            "| Workload | Heap | Pauses | Median | p95 | p99 | Max |",
            "| --- | --- | --- | --- | --- | --- | --- |",
        ]
        for workload, heap, row in minor:
            data = row["young_pauses"]
            lines.append(
                f"| {workload} | {heap} | {data['count']:g} | {us(data['median_ns'])} | {us(data['p95_ns'])} | "
                f"{us(data['p99_ns'])} | {us(data['max_ns'])} |")
    marking = [(workload, heap, row) for workload, heaps in results.items() for heap, row in heaps.items()
               if row.get("marking")]
    if marking:
        lines += [
            "",
            "The `gc` crate's concurrent markings during the turns (on `gc-parallel`; `gc` marks on one thread, in "
            "its full GCs): how many finished and how many at the old generation's limit (the rest marked in the "
            "pause), the workers' time marking alongside, the starts' pauses, the ends' pauses and the end's "
            "marking in them (`Gc.Mark`: the target is 1 ms; the rest is sweeping), over every run's; and in every "
            "full GC, the weak edges' and the id table's clearing (`Gc.Weak`), and the id table's largest size, "
            "in chunks of 1024 ids. Then the sweep alongside the mutator: the workers' time, and the heap's "
            "thread's outside the pauses (in the GC time above) and the pages it swept.",
            "",
            "| Workload | Heap | Markings | At limit | Background | Start p50 | Start max | End p50 | End p99 "
            "| End max | End marking p50 | p99 | Max | Weak p50 | Weak max | Id chunks | Sweep background "
            "| Sweep on main | Pages on main |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- "
            "| --- | --- |",
        ]
        for workload, heap, row in marking:
            data = row["marking"]
            lines.append(
                f"| {workload} | {heap} | {data['count']:g} | {data['at_limit']:g} | {ms(data['background_ns'])} | "
                f"{us(data['start_p50_ns'])} | {us(data['start_max_ns'])} | {us(data['end_p50_ns'])} | "
                f"{us(data['end_p99_ns'])} | {us(data['end_max_ns'])} | {us(data['end_mark_p50_ns'])} | "
                f"{us(data['end_mark_p99_ns'])} | {us(data['end_mark_max_ns'])} | {us(data['weak_p50_ns'])} | "
                f"{us(data['weak_max_ns'])} | {data['peak_id_chunks']:g} | {ms(data['background_sweep_ns'])} | "
                f"{ms(data['sweep_on_main_ns'])} | {data['pages_swept_on_main']:g} |")
    fragmented = [(workload, heap, row) for workload, heaps in results.items() for heap, row in heaps.items()
                  if row.get("fragmentation")]
    if fragmented:
        lines += [
            "",
            "The `gc` crate's old generation during the turns, each time a sweep was done (the most fragmented it "
            "gets) and a marking started (where V8 chooses the pages to evacuate), in % (medians over the times "
            "and the runs; Max, the most of any): the share of its committed bytes that V8's choice of pages to "
            "evacuate (at least 70% free, the least live first, 4 MiB moved at most, none if no page is freed) "
            "would free, and the free share of the slots of the size-class pages with objects; the most bytes it "
            "would move, in MiB, and the most pages with objects. Then the free ranges allocation in the old "
            "generation took (its slow path) and their slots, on average.",
            "",
            "| Workload | Heap | When | Times | Freed | Max | Free slots | Max | Moved max | Pages max | Ranges "
            "| Slots a range |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
        for workload, heap, row in fragmented:
            for when, key in (("swept", "fragmentation"), ("marking", "fragmentation_at_marking")):
                data = row.get(key)
                if data is None:
                    continue
                lines.append(
                    f"| {workload} | {heap} | {when} | {data['count']:g} | {data['released_median'] / 10:.1f} | "
                    f"{data['released_max'] / 10:.1f} | {data['free_median'] / 10:.1f} | "
                    f"{data['free_max'] / 10:.1f} | {mib(data['evacuated_max_bytes'])} | {data['pages_max']:g} | "
                    f"{data['ranges']:g} | {data['slots_per_range']:.1f} |")
    RESULTS.mkdir(parents=True, exist_ok=True)
    report = RESULTS / "gc.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (RESULTS / "gc.json").write_text(json.dumps({"env": env, "results": results}, indent=1) + "\n",
                                     encoding="utf-8", newline="\n")
    return report


def build() -> Path:
    done = subprocess.run(["cargo", "build", "-q", "--profile", "perf", "-p", "gc_bench", "--bin", "gc_bench"],
                          cwd=ROOT, env=common.build_env("perf"))
    if done.returncode != 0:
        raise SystemExit("building gc_bench failed")
    return ROOT / "target" / "perf" / ("gc_bench.exe" if sys.platform == "win32" else "gc_bench")


def run_once(binary: Path, heap: str, workload: str, scale: float, trace: Path) -> dict:
    """One `gc_bench` process: its report, analysed. Its report goes to a
    file, as V8's lines go to its stdout."""
    report = trace.with_suffix(".json")
    done = subprocess.run(
        [str(binary), *HEAP_ARGS[heap], "--workloads", workload, "--scale", str(scale), "--trace", str(trace),
         "--report", str(report)],
        cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        raise SystemExit(f"gc_bench failed on {workload}, {heap}:\n{done.stderr[-3000:]}")
    line = report.read_text(encoding="utf-8").splitlines()[-1]
    return analyse(json.loads(line), trace, done.stdout)


def cmd_gc(args) -> int:
    heaps = args.heaps.split(",") if args.heaps else list(HEAPS)
    workloads = args.workloads.split(",") if args.workloads else list(WORKLOADS)
    for name, chosen, known in (("heap", heaps, HEAPS), ("workload", workloads, WORKLOADS)):
        unknown = [value for value in chosen if value not in known]
        if unknown:
            print(f"no {name} {', '.join(unknown)} ({', '.join(known)})")
            return 2
    baseline = None
    if getattr(args, "baseline", None):
        baseline_file = Path(args.baseline)
        if not baseline_file.is_file():
            print(f"no baseline {baseline_file}")
            return 2
        baseline = json.loads(baseline_file.read_text(encoding="utf-8"))
    binary = build()
    RESULTS.mkdir(parents=True, exist_ok=True)
    results: dict[str, dict[str, dict]] = {}
    for workload in workloads:
        for heap in heaps:
            if not runs_on(heap, workload):
                continue
            runs = [run_once(binary, heap, workload, args.scale, RESULTS / f"{workload}-{heap}-{run}.pftrace")
                    for run in range(args.runs)]
            results.setdefault(workload, {})[heap] = summarize(runs)
            row = results[workload][heap]
            print(f"{workload}, {heap}: run {ms(row['run_ns'])} ms, GC {ms(row['gc_ns'])} ms "
                  f"(+{ms(row['background_gc_ns'])} ms elsewhere), "
                  f"{row['pauses']['count']:g} pauses up to {ms(row['pauses']['max_ns'])} ms", flush=True)
    env = {
        "date": date.today().isoformat(),
        "commit": common.git_head(),
        "platform": sys.platform,
        "runs": args.runs,
        "scale": args.scale,
    }
    if baseline:
        add_baseline(results, baseline, heaps)
        env["baseline"] = {"file": args.baseline, **{key: baseline["env"][key]
                                                    for key in ("commit", "date", "platform")}}
    print(f"report: {write_report(results, env)}")
    return 0
