#!/usr/bin/env python3
"""Tests of `bench.py gc` (`gc_bench.py`): what it reads from a trace (the
outermost GC slices in a span, their kinds, the other threads' GC, the
pauses), V8's `--trace-gc-nvp` lines paired with the trace's scavenges,
the medians and the report (the `gc` crate's markings' table too), and
runs of `gc_bench` (the dev profile) on each heap, small, and the id
table's workload.

    python3 scripts/bench/gc/gc_bench_test.py
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gc_bench  # noqa: E402
import common  # noqa: E402
from common import ROOT  # noqa: E402


def events(track: int, slices: list[tuple[int, int, str]]) -> list[tuple]:
    """Track events (`perfetto_track_events`'s) for slices `(begin, end,
    name)`, nested as their times are."""
    marks = []
    for begin, end, name in slices:
        marks.append((begin, 1, name))
        marks.append((end, 2, ""))
    # Ends before begins at the same time, the outer slice's begin first.
    marks.sort(key=lambda mark: (mark[0], 0 if mark[1] == 2 else 1))
    return [(track, time, order, kind, name, []) for order, (time, kind, name) in enumerate(marks)]


def nvp(**fields) -> str:
    """A `--trace-gc-nvp` line as V8 prints it."""
    return "[1:0000:0]  5 ms: GC: " + json.dumps(fields)


class ReadingTests(unittest.TestCase):
    """What the tool makes of a trace's slices."""

    def test_the_outermost_gc_slices_in_the_span(self):
        trace = events(1, [
            (0, 1000, "gc_bench.churn"),
            (10, 20, "CppGC.IncrementalMark"),
            (12, 18, "CppGC.MarkTransitiveClosure"),
            (30, 60, "MajorGC"),
            (31, 40, "CppGC.AtomicMark"),
            (41, 50, "CppGC.AtomicSweep"),
            (70, 80, "Other"),
            (100, 140, "MajorGC"),
            (101, 130, "V8.GC_MC_MARK"),
            (102, 120, "CppGC.AtomicMark"),
            (2000, 2100, "CppGC.IncrementalSweep"),
        ]) + events(2, [(15, 25, "CppGC.ConcurrentSweep")])
        outermost, marks = gc_bench.gc_slices(trace, "gc_bench.churn")
        # Not the nested ones, not the other thread's, not after the span.
        self.assertEqual(outermost, [(10, 20, "CppGC.IncrementalMark"), (30, 60, "MajorGC"), (100, 140, "MajorGC")])
        # The outermost marking: cppgc's inside V8's is part of it.
        self.assertEqual(marks, [(31, 40, "CppGC.AtomicMark"), (101, 130, "V8.GC_MC_MARK")])

    def test_a_trace_without_the_span(self):
        with self.assertRaises(SystemExit):
            gc_bench.gc_slices(events(1, [(0, 10, "CppGC.AtomicMark")]), "gc_bench.build")

    def test_the_other_threads_gc_inside_the_span(self):
        trace = events(1, [(100, 1000, "gc_bench.tree-low"), (200, 300, "MinorGC")]) + events(2, [
            (50, 150, "V8.GC_SCAVENGER_BACKGROUND_SCAVENGE_PARALLEL"),  # Half inside.
            (400, 500, "V8.GC_MC_BACKGROUND_MARKING"),
            (410, 450, "V8.GC_MC_BACKGROUND_MARKING"),  # Nested: counted once.
            (600, 700, "Other"),
            (900, 1200, "CppGC.ConcurrentSweep"),  # A third inside.
        ]) + events(3, [(1100, 1300, "V8.GC_MC_BACKGROUND_SWEEPING")])  # After.
        self.assertEqual(gc_bench.background_gc_ns(trace, "gc_bench.tree-low"), 50 + 100 + 100)
        self.assertEqual(gc_bench.span_bounds(trace, "gc_bench.tree-low"), (100, 1000))

    def test_the_gc_in_a_window(self):
        trace = events(1, [
            (0, 50_000, "MinorGC"),  # Ends inside: cut.
            (100_000, 120_000, "CppGC.IncrementalMark"),
            (105_000, 110_000, "CppGC.MarkTransitiveClosure"),  # Nested: counted once.
            (106_000, 107_000, "Gc.Step"),  # The gc crate's, in an engine's: its share.
            (125_000, 135_000, "CppGC.AtomicMark"),  # Less than 10 µs after: the same pause.
            (200_000, 300_000, "Other"),
            (400_000, 430_000, "CppGC.IncrementalSweep"),
            (500_000, 510_000, "Gc.Pause"),  # The gc crate's own GC.
            (501_000, 502_000, "Gc.Mark"),  # Inside its own: not an engine's share.
            (950_000, 1_100_000, "MajorGC"),  # Begins inside: cut.
            (960_000, 1_050_000, "Gc.Mark"),  # Ends after: cut.
            (1_200_000, 1_300_000, "MinorGC"),  # After.
        ]) + events(2, [(0, 30_000, "CppGC.ConcurrentSweep"), (500_000, 600_000, "V8.GC_MC_BACKGROUND_MARKING")])
        self.assertEqual(gc_bench.window_gc(trace, 1, 20_000, 1_000_000), {
            "window_ns": 980_000,
            "gc_ns": 30_000 + 20_000 + 10_000 + 30_000 + 10_000 + 50_000,
            "gc_by_kind_ns": {"young": 30_000, "marking": 20_000, "atomic": 10_000 + 10_000 + 50_000,
                              "sweeping": 30_000},
            "young_by_heap_ns": {"engine": 30_000},
            "pauses_ns": [30_000, 35_000, 30_000, 10_000, 50_000],
            # The last, cut at the window's end: the `gc` crate's marking in
            # V8's full GC, and V8's own time around it.
            "longest_pause": {"ns": 50_000, "phases": [["Gc.Mark", 40_000], ["MajorGC", 10_000]]},
            "in_engine_ns": 1_000 + 40_000,
            "background_gc_ns": 10_000 + 100_000,
        })
        # No GC: nothing.
        self.assertEqual(gc_bench.window_gc(trace, 1, 140_000, 390_000)["pauses_ns"], [])

    def test_the_young_generations_by_heap_and_the_longest_pause(self):
        trace = events(1, [
            (0, 10_000, "Gc.Scavenge"),  # The gc crate's own.
            (100_000, 200_000, "MinorGC"),  # The engine's, with its phases.
            (100_000, 190_000, "V8.GC_SCAVENGER_SCAVENGE"),
            (110_000, 170_000, "V8.GC_SCAVENGER_SCAVENGE_WEAK_GLOBAL_HANDLES_PROCESS"),
            (170_000, 180_000, "V8.GC_SCAVENGER_SCAVENGE_PARALLEL_PHASE"),
            (300_000, 320_000, "MinorGC"),
            (400_000, 500_000, "Other"),  # No GC: no phase.
        ])
        gc = gc_bench.window_gc(trace, 1, 0, 1_000_000)
        self.assertEqual(gc["young_by_heap_ns"], {"gc": 10_000, "engine": 100_000 + 20_000})
        # Each slice's time but its phases': the scavenge's 90 µs less the
        # 70 µs of its two phases.
        self.assertEqual(gc["longest_pause"], {"ns": 100_000, "phases": [
            ["V8.GC_SCAVENGER_SCAVENGE_WEAK_GLOBAL_HANDLES_PROCESS", 60_000],
            ["V8.GC_SCAVENGER_SCAVENGE", 20_000],
            ["MinorGC", 10_000],
            ["V8.GC_SCAVENGER_SCAVENGE_PARALLEL_PHASE", 10_000],
        ]})
        # The most of them, `PAUSE_PHASES`.
        many = events(1, [(0, 100_000, "MinorGC")] + [
            (i * 10_000, i * 10_000 + 5_000, f"V8.GC_PHASE_{i}") for i in range(8)
        ])
        self.assertEqual(len(gc_bench.window_gc(many, 1, 0, 1_000_000)["longest_pause"]["phases"]),
                         gc_bench.PAUSE_PHASES)
        # No GC: no pause.
        self.assertEqual(gc_bench.window_gc(trace, 1, 400_000, 500_000)["longest_pause"], {"ns": 0, "phases": []})

    def test_a_trace_that_lost_events(self):
        def varint(n):
            out = b""
            while True:
                out += bytes([(n & 0x7f) | (0x80 if n > 0x7f else 0)])
                n >>= 7
                if not n:
                    return out

        def message(*fields):
            return b"".join(varint(number << 3) + varint(value) if isinstance(value, int)
                            else varint(number << 3 | 2) + varint(len(value)) + value for number, value in fields)

        def packet(sequence, *fields):
            return message((1, message((10, sequence), *fields)))

        def thread(sequence, track, name):
            # The sequence's first packet, which says so of every sequence;
            # its thread's track and name.
            return (packet(sequence, (42, 1), (60, message((1, track), (4, message((5, name.encode()))))),
                           (59, message((11, message((11, track))))))
                    + packet(sequence, (8, 5), (11, message((9, 3), (23, b"work")))))
        trace = thread(1, 7, "main") + thread(2, 8, "worker")
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.pftrace"
            path.write_bytes(trace)
            lost = []
            common.perfetto_track_events(path, lost)
            self.assertEqual(lost, [])
            # A packet after the first that says the one before was dropped.
            path.write_bytes(trace + packet(2, (42, 1), (8, 9), (11, message((9, 3), (23, b"more")))))
            common.perfetto_track_events(path, lost)
            self.assertEqual(lost, ["worker"])
            with self.assertRaises(SystemExit) as stopped:
                gc_bench.analyse({"workload": "churn"}, path)
            self.assertIn("lost events on worker", str(stopped.exception))

    def test_scavenges_from_v8s_lines(self):
        output = "\n".join([
            "{\"workload\": \"not V8's\"}",
            nvp(gc="s", new_space_survived=1000, promoted=24, new_space_capacity=2**20),
            "[1:0000:0]  6 ms: Memory pool: Removed pages: 0",
            nvp(gc="mc", promoted=5),
            nvp(gc="s", new_space_survived=10, promoted=5, new_space_capacity=2**21),
            nvp(gc="s", new_space_survived=7, promoted=3, new_space_capacity=2**22),
        ])
        scavenges = gc_bench.nvp_scavenges(output)
        self.assertEqual([record["new_space_survived"] for record in scavenges], [1000, 10, 7])
        # The first scavenge is the setup's, the last the forced GC's: one
        # is in the span.
        trace = events(1, [
            (0, 10, "MinorGC"),
            (100, 1000, "gc_bench.tree-low"),
            (200, 300, "MinorGC"),
            (210, 290, "V8.GC_SCAVENGER"),
            (1100, 1200, "gc_bench.forced_gc"),
            (1110, 1150, "MinorGC"),
        ])
        young = gc_bench.young_gc(trace, "gc_bench.tree-low", scavenges)
        self.assertEqual(young, {"count": 1, "survived_bytes": 15, "ns": 100, "capacity_bytes": 2**21})
        with self.assertRaises(SystemExit):
            gc_bench.young_gc(trace, "gc_bench.tree-low", scavenges[:2])

    def test_kinds(self):
        for name, kind in [
            ("CppGC.IncrementalMark", "marking"),
            ("V8.GCIncrementalMarking", "marking"),
            ("V8.GCIncrementalMarkingStart", "marking"),
            ("CppGC.AtomicMark", "atomic"),
            ("CppGC.AtomicWeak", "atomic"),
            ("MajorGC", "atomic"),
            ("V8.GC_MARK_COMPACTOR", "atomic"),
            ("MinorGC", "young"),
            ("V8.GC_SCAVENGER", "young"),
            ("CppGC.AtomicSweep", "sweeping"),
            ("CppGC.IncrementalSweep", "sweeping"),
            ("V8.GC_MC_SWEEP", "sweeping"),
            ("V8.GC_HEAP_PROLOGUE", "other"),
            ("Gc.Pause", "atomic"),
            ("Gc.Scavenge", "young"),
        ]:
            self.assertEqual(gc_bench.gc_kind(name), kind, name)
        self.assertTrue(gc_bench.is_gc("MajorGC"))
        self.assertTrue(gc_bench.is_gc("CppGC.AtomicMark"))
        self.assertTrue(gc_bench.is_gc("Gc.Pause"))
        self.assertTrue(gc_bench.is_gc("Gc.Mark"))
        self.assertFalse(gc_bench.is_gc("gc_bench.build"))

    def test_slices_close_together_are_one_pause(self):
        gap = gc_bench.PAUSE_GAP_NS
        slices = [
            (0, 100, "CppGC.AtomicMark"),
            (100 + gap - 1, 200 + gap, "CppGC.AtomicSweep"),  # Back to back: the same pause.
            (300 + 2 * gap, 400 + 2 * gap, "CppGC.IncrementalMark"),  # After the mutator ran.
        ]
        self.assertEqual(gc_bench.pauses(slices), [200 + gap, 100])
        self.assertEqual(gc_bench.pauses([]), [])

    def test_the_young_generations_pauses(self):
        gap = gc_bench.PAUSE_GAP_NS
        slices = [
            (0, 100, "Gc.Scavenge"),
            (100 + gap, 300 + gap, "Gc.Pause"),  # A full GC: its own scavenge is inside.
            (400 + 2 * gap, 450 + 2 * gap, "MinorGC"),
            (450 + 2 * gap, 470 + 2 * gap, "V8.GC_SCAVENGER"),  # Back to back: the same pause.
        ]
        self.assertEqual(gc_bench.young_pauses(slices), [100, 70])

    def test_percentiles(self):
        values = list(range(1, 101))
        self.assertEqual(gc_bench.percentile(values, 0.5), 50)
        self.assertEqual(gc_bench.percentile(values, 0.95), 95)
        self.assertEqual(gc_bench.percentile([7], 0.95), 7)
        self.assertEqual(gc_bench.percentile([], 0.5), 0)

    def test_which_heaps_run_what(self):
        self.assertEqual(gc_bench.HEAPS, ("v8", "v8-single", "gc", "gc-parallel"))
        self.assertTrue(gc_bench.runs_on("gc", "build"))
        self.assertTrue(gc_bench.runs_on("v8", "graph-low"))
        self.assertTrue(gc_bench.runs_on("v8-single", "tree-high"))
        self.assertFalse(gc_bench.runs_on("v8", "replace"))
        self.assertTrue(gc_bench.runs_on("gc", "replace"))
        self.assertTrue(gc_bench.runs_on("gc-parallel", "chain-high"))
        # The id table is the gc crate's.
        self.assertTrue(gc_bench.runs_on("gc-parallel", "ids"))
        self.assertFalse(gc_bench.runs_on("v8", "ids"))

    def test_the_gc_crates_marking_phases(self):
        trace = events(1, [
            (0, 10_000, "gc_bench.build"),
            # A concurrent marking's start: a promoting scavenge, the roots.
            (100, 200, "Gc.Pause"), (100, 150, "Gc.Scavenge"), (150, 200, "Gc.MarkStart"),
            # A full GC on one thread: it scavenges first.
            (300, 700, "Gc.Pause"), (300, 400, "Gc.Scavenge"), (400, 600, "Gc.Mark"), (550, 590, "Gc.Weak"),
            (600, 700, "Gc.Sweep"),
            # A concurrent marking's end: its marking, then the sweep.
            (1000, 1300, "Gc.Pause"), (1000, 1100, "Gc.Mark"), (1050, 1060, "Gc.Weak"), (1100, 1300, "Gc.Sweep"),
            # A scavenge on its own.
            (2000, 2050, "Gc.Scavenge"),
            # Outside the span.
            (20_000, 20_100, "Gc.Pause"), (20_000, 20_100, "Gc.MarkStart"),
        ])
        self.assertEqual(gc_bench.marking_phases(trace, "gc_bench.build"),
                         {"start_pauses_ns": [100], "end_pauses_ns": [300], "end_mark_ns": [100],
                          "weak_ns": [40, 10]})

    def test_the_heaps_sweeping_outside_the_pauses_is_gc_time(self):
        trace = events(1, [(0, 10_000, "gc_bench.build"), (100, 300, "Gc.Pause"), (100, 300, "Gc.Sweep"),
                           (20_000, 20_100, "gc_bench.forced_gc")])
        with mock.patch.object(common, "perfetto_track_events", return_value=(None, trace)):
            run = gc_bench.analyse({"workload": "build", "sweep_on_main_ns": 50}, Path("t.pftrace"))
        # The pause, and the sweeping the trace doesn't show.
        self.assertEqual(run["gc_ns"], 250)
        self.assertEqual(run["gc_by_kind_ns"]["sweeping"], 50)
        self.assertEqual(run["pauses_ns"], [200])


def run_report(**changes) -> dict:
    run = {
        "workload": "list", "heap": "standalone", "turns": 10, "nodes_allocated": 500, "live_nodes": 22,
        "run_ns": 4_000_000, "idle_ns": 0, "gc_ns": 1_000_000,
        "gc_by_kind_ns": {"marking": 600_000, "sweeping": 400_000}, "background_gc_ns": 1_500_000,
        "pauses_ns": [100_000, 300_000], "peak_committed_bytes": 2**20, "peak_used_bytes": 2**19,
        "after_forced_gc": {"used": 10_100_000}, "baseline_used": 100_000,
        "forced_gc_ns": 3_000_000, "forced_mark_ns": 2_000_000, "young": None,
    }
    run.update(changes)
    return run


def young(count, survived, ns, capacity=2**20) -> dict:
    return {"count": count, "survived_bytes": survived, "ns": ns, "capacity_bytes": capacity}


class SummaryTests(unittest.TestCase):
    def test_medians_and_every_runs_pauses(self):
        runs = [run_report(run_ns=3_000_000, pauses_ns=[100]),
                run_report(run_ns=5_000_000, pauses_ns=[200, 900]),
                run_report(run_ns=4_000_000, pauses_ns=[300])]
        row = gc_bench.summarize(runs)
        self.assertEqual(row["run_ns"], 4_000_000)
        self.assertEqual(row["pauses"], {"count": 1, "median_ns": 200, "p95_ns": 900, "max_ns": 900})
        self.assertEqual(row["gc_by_kind_ns"],
                         {"marking": 600_000, "atomic": 0, "young": 0, "sweeping": 400_000, "other": 0})
        # 10 MB over the heap's own, marked in 2 ms; 22 nodes.
        self.assertEqual(row["live_bytes"], 10_000_000)
        self.assertEqual(row["mark_bytes_per_s"], 5e9)
        self.assertEqual(row["mark_objects_per_s"], 11_000)
        self.assertAlmostEqual(row["bytes_per_object"], 10_000_000 / 22)
        self.assertIsNone(row["young"])
        self.assertIsNone(row["young_pauses"])

    def test_minor_pauses_over_every_run(self):
        runs = [run_report(young_pauses_ns=list(range(1, 51))),
                run_report(young_pauses_ns=list(range(51, 101))),
                run_report(young_pauses_ns=[1000])]
        self.assertEqual(gc_bench.summarize(runs)["young_pauses"],
                         {"count": 50, "median_ns": 51, "p95_ns": 96, "p99_ns": 100, "max_ns": 1000})

    def test_scavenges_speed(self):
        # Each run's speed, then their median: 1 GB/s, 2 GB/s, 4 GB/s.
        runs = [run_report(young=young(3, 1_000_000, 1_000_000)),
                run_report(young=young(5, 4_000_000, 2_000_000, 2**22)),
                run_report(young=young(4, 4_000_000, 1_000_000))]
        row = gc_bench.summarize(runs)["young"]
        self.assertEqual(row["count"], 4)
        self.assertEqual(row["bytes_per_s"], 2e9)
        self.assertEqual(row["capacity_bytes"], 2**22)

    def test_the_report(self):
        row = gc_bench.summarize([run_report()])
        v8 = gc_bench.summarize([run_report(heap="v8", young=young(2, 3 * 2**20, 1_500_000),
                                            young_pauses_ns=[500_000, 1_000_000])])
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(gc_bench, "RESULTS", Path(folder)):
            env = {"date": "2026-10-25", "commit": "abc", "platform": "win32", "runs": 1, "scale": 1.0}
            gc = gc_bench.summarize([run_report(heap="gc", peak_nursery_bytes=2**20, pretenured_objects=10),
                                     run_report(heap="gc", peak_nursery_bytes=2**21, pretenured_objects=30),
                                     run_report(heap="gc", peak_nursery_bytes=2**20, pretenured_objects=40)])
            results = {"list": {"standalone": row, "gc": gc}, "tree-low": {"v8": v8}}
            report = gc_bench.write_report(results, env).read_text(encoding="utf-8")
            saved = json.loads((Path(folder) / "gc.json").read_text(encoding="utf-8"))
        # Run 4 ms, GC 1 ms (25%), 1.5 ms elsewhere, pauses 0.1 and 0.3 ms.
        self.assertIn("| list | standalone | 4.00 | 1.00 | 25 | 0.60 | 0.00 | 0.00 | 0.40 | 1.50 | 2 | 0.10 | 0.30 "
                      "| 0.30 |", report)
        self.assertIn("| list | standalone | 500 | 1.0 | 0.5 |  |  | 22 | 9.5 | 454545 | 3.00 | 5000 | 0.01 |", report)
        # The gc crate's nursery, the most of its runs, and the median of its
        # pretenured objects.
        self.assertIn("| list | gc | 500 | 1.0 | 0.5 | 2.0 | 30 | 22 |", report)
        # Only V8's rows have scavenges: 3 MiB in 1.5 ms.
        self.assertIn("| tree-low | v8 | 2 | 3.0 | 1.50 | 2097 | 1.0 |", report)
        self.assertNotIn("| list | standalone | 2 |", report.split("V8's scavenges")[1])
        # Only V8's rows have minor pauses: 500 and 1000 µs.
        minor = report.split("young generation alone")[1]
        self.assertIn("| tree-low | v8 | 2 | 500.0 | 1000.0 | 1000.0 | 1000.0 |", minor)
        self.assertNotIn("| list |", minor)
        self.assertEqual(saved["results"]["list"]["standalone"]["live_nodes"], 22)


class MarkingSummaryTests(unittest.TestCase):
    def marking(self, starts, ends, ends_mark, weak) -> dict:
        return {"start_pauses_ns": starts, "end_pauses_ns": ends, "end_mark_ns": ends_mark, "weak_ns": weak}

    def test_the_gc_crates_markings_over_the_runs(self):
        runs = [run_report(heap="gc-parallel", marking=self.marking([10], [100, 300], [50, 90], [5]),
                           concurrent_markings=2, markings_at_limit=0, background_mark_ns=1_000, peak_id_chunks=3,
                           background_sweep_ns=10, sweep_on_main_ns=40, pages_swept_on_main=2),
                run_report(heap="gc-parallel", marking=self.marking([30], [200], [70], [9]),
                           concurrent_markings=1, markings_at_limit=1, background_mark_ns=3_000, peak_id_chunks=5,
                           background_sweep_ns=30, sweep_on_main_ns=60, pages_swept_on_main=4),
                run_report(heap="gc-parallel", marking=self.marking([20], [], [], [7]),
                           concurrent_markings=0, markings_at_limit=0, background_mark_ns=2_000, peak_id_chunks=4,
                           background_sweep_ns=20, sweep_on_main_ns=50, pages_swept_on_main=3)]
        self.assertEqual(gc_bench.summarize(runs)["marking"], {
            "count": 1, "at_limit": 0, "background_ns": 2_000,
            "start_p50_ns": 20, "start_max_ns": 30,
            "end_p50_ns": 200, "end_p99_ns": 300, "end_max_ns": 300,
            "end_mark_p50_ns": 70, "end_mark_p99_ns": 90, "end_mark_max_ns": 90,
            "weak_p50_ns": 7, "weak_max_ns": 9, "peak_id_chunks": 5,
            "background_sweep_ns": 20, "sweep_on_main_ns": 50, "pages_swept_on_main": 3,
        })
        # A heap without the gc crate's record: none.
        self.assertIsNone(gc_bench.summarize([run_report()])["marking"])

    def test_the_markings_table(self):
        row = gc_bench.summarize([run_report(heap="gc-parallel", marking=self.marking(
            [40_000], [900_000], [600_000], [20_000]), concurrent_markings=3, markings_at_limit=1,
            background_mark_ns=5_000_000, peak_id_chunks=977, background_sweep_ns=2_000_000,
            sweep_on_main_ns=300_000, pages_swept_on_main=12)])
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(gc_bench, "RESULTS", Path(folder)):
            env = {"date": "2026-10-26", "commit": "abc", "platform": "darwin", "runs": 1, "scale": 1.0}
            results = {"ids": {"gc-parallel": row}, "list": {"standalone": gc_bench.summarize([run_report()])}}
            report = gc_bench.write_report(results, env).read_text(encoding="utf-8")
        markings = report.split("concurrent markings during the turns")[1]
        self.assertIn("| ids | gc-parallel | 3 | 1 | 5.00 | 40.0 | 40.0 | 900.0 | 900.0 | 900.0 | 600.0 | 600.0 "
                      "| 600.0 | 20.0 | 20.0 | 977 | 2.00 | 0.30 | 12 |", markings)
        self.assertNotIn("| list |", markings)


class FragmentationSummaryTests(unittest.TestCase):
    def fragmentation(self, count, released, free, evacuated, pages) -> dict:
        return {"count": count, "released_median": released[0], "released_max": released[1],
                "free_median": free[0], "free_max": free[1], "evacuated_max_bytes": evacuated,
                "pages_max": pages}

    def test_the_sweeps_over_the_runs(self):
        runs = [run_report(heap="gc-parallel", fragmentation=self.fragmentation(4, (10, 50), (300, 600), 100, 8),
                           old_ranges=10, old_range_slots=40),
                run_report(heap="gc-parallel", fragmentation=self.fragmentation(6, (30, 90), (100, 900), 300, 9),
                           old_ranges=20, old_range_slots=20),
                run_report(heap="gc-parallel", fragmentation=self.fragmentation(5, (20, 70), (200, 500), 200, 7),
                           old_ranges=0, old_range_slots=0)]
        self.assertEqual(gc_bench.summarize(runs)["fragmentation"], {
            "count": 5, "released_median": 20, "released_max": 90, "free_median": 200, "free_max": 900,
            "evacuated_max_bytes": 300, "pages_max": 9, "ranges": 10, "slots_per_range": 1.0,
        })
        # Each point of the GCs on its own.
        self.assertEqual(gc_bench.summarize(
            [{**run, "fragmentation_at_marking": self.fragmentation(1, (0, 0), (0, 100), 0, 7)} for run in runs])[
            "fragmentation_at_marking"]["free_max"], 100)
        # A heap without the gc crate's measures: none.
        self.assertIsNone(gc_bench.summarize([run_report()])["fragmentation"])
        self.assertIsNone(gc_bench.summarize([run_report()])["fragmentation_at_marking"])

    def test_the_fragmentation_table(self):
        row = gc_bench.summarize([run_report(heap="gc", fragmentation=self.fragmentation(
            12, (125, 250), (400, 700), 3 * 2**20, 40), fragmentation_at_marking=self.fragmentation(
            11, (50, 90), (100, 200), 2**20, 39), old_ranges=500, old_range_slots=2000)])
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(gc_bench, "RESULTS", Path(folder)):
            env = {"date": "2026-10-26", "commit": "abc", "platform": "darwin", "runs": 1, "scale": 1.0}
            results = {"churn": {"gc": row}, "list": {"standalone": gc_bench.summarize([run_report()])}}
            report = gc_bench.write_report(results, env).read_text(encoding="utf-8")
        table = report.split("each time a sweep was done")[1]
        self.assertIn("| churn | gc | swept | 12 | 12.5 | 25.0 | 40.0 | 70.0 | 3.0 | 40 | 500 | 4.0 |", table)
        self.assertIn("| churn | gc | marking | 11 | 5.0 | 9.0 | 10.0 | 20.0 | 1.0 | 39 | 500 | 4.0 |", table)
        self.assertNotIn("| list |", table)


class BaselineTests(unittest.TestCase):
    def test_an_earlier_runs_rows_of_the_heaps_left_out(self):
        row = gc_bench.summarize([run_report()])
        earlier = {"env": {"commit": "abc", "date": "2026-10-25", "platform": "darwin"},
                   "results": {"list": {"standalone": row, "gc": row}, "build": {"standalone": row}}}
        results = {"list": {"gc": row}}
        gc_bench.add_baseline(results, earlier, ["gc"])
        # The heaps this run left out, first; not the workloads it didn't run.
        self.assertEqual(list(results), ["list"])
        self.assertEqual(list(results["list"]), ["standalone (baseline)", "gc"])
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(gc_bench, "RESULTS", Path(folder)):
            env = {"date": "2026-10-26", "commit": "def", "platform": "darwin", "runs": 1, "scale": 1.0,
                   "baseline": {"file": "old.json", "commit": "abc", "date": "2026-10-25", "platform": "darwin"}}
            report = gc_bench.write_report(results, env).read_text(encoding="utf-8")
        self.assertIn("Rows marked (baseline) are from old.json: machopo abc, 2026-10-25, darwin.", report)
        self.assertIn("| list | standalone (baseline) | 4.00 |", report)


class GcRecordTests(unittest.TestCase):
    def test_the_gc_crates_record_agrees_with_the_trace(self):
        report = {"after_forced_gc": {"used": 5000}, "baseline_used": 0,
                  "gc": {"marked_bytes": 5000, "mark_ns": 1_000_000}}
        gc_bench.check_gc_record(report, 1_000_000 - gc_bench.MARK_SLACK_NS)
        # Bytes left that it didn't mark, or a marking the trace times
        # otherwise.
        with self.assertRaises(SystemExit):
            gc_bench.check_gc_record({**report, "baseline_used": 8}, 1_000_000)
        with self.assertRaises(SystemExit):
            gc_bench.check_gc_record(report, 1_000_000 - gc_bench.MARK_SLACK_NS - 1)
        with self.assertRaises(SystemExit):
            gc_bench.check_gc_record(report, 1_000_000 + gc_bench.MARK_SLACK_NS + 1)


class RunTests(unittest.TestCase):
    """`gc_bench` itself, small, on each heap, and what its trace and V8's
    lines give."""

    @classmethod
    def setUpClass(cls):
        built = subprocess.run(["cargo", "build", "-q", "-p", "gc_bench", "--bin", "gc_bench"], cwd=ROOT)
        assert built.returncode == 0, "building gc_bench failed"
        cls.binary = ROOT / "target" / "debug" / ("gc_bench.exe" if sys.platform == "win32" else "gc_bench")

    def run_bench(self, heap: str, workload: str) -> tuple[dict, tuple[int, int]]:
        with tempfile.TemporaryDirectory() as folder:
            trace = Path(folder) / "trace.pftrace"
            run = gc_bench.run_once(self.binary, heap, workload, 0.2, trace)
            _, trace_events = gc_bench.common.perfetto_track_events(trace)
            span = gc_bench.span_bounds(trace_events, f"gc_bench.{workload}")
        self.assertEqual((run["workload"], run["heap"]), (workload, heap.removesuffix("-single")))
        return run, span

    def test_the_gc_crates_heaps(self):
        for heap in ("gc", "gc-parallel"):
            run, (begin, end) = self.run_bench(heap, "replace")
            self.assertLessEqual(run["run_ns"], end - begin, heap)
            # 100 turns of 200 new nodes: it scavenged during the turns
            # (the nodes die young), and its record of the forced GC agreed
            # with the trace (or `analyse` would have stopped).
            self.assertGreater(run["gc_by_kind_ns"]["young"], 0, heap)
            self.assertTrue(run["pauses_ns"], heap)
            self.assertTrue(run["young_pauses_ns"], heap)
            self.assertGreaterEqual(run["peak_nursery_bytes"], 2**20, heap)
            self.assertIn("pretenured_objects", run, heap)
            self.assertGreater(run["forced_mark_ns"], 0, heap)
            self.assertEqual(run["gc"]["marked_objects"], run["live_nodes"], heap)

    def test_the_id_tables_workload(self):
        # A fifth: 20 turns of 10,000 nodes, each with an id. On one thread
        # its full GCs walk the table (`Gc.Weak`), none alongside; on a pool
        # some markings ran alongside, the workers timed.
        for heap in ("gc", "gc-parallel"):
            run, _ = self.run_bench(heap, "ids")
            self.assertGreater(run["peak_id_chunks"], 0, heap)
            self.assertTrue(run["marking"]["weak_ns"], heap)
            if heap == "gc":
                self.assertEqual(run["concurrent_markings"], 0)
                self.assertEqual(run["marking"]["end_pauses_ns"], [])
            else:
                self.assertGreater(run["concurrent_markings"], 0)
                self.assertEqual(len(run["marking"]["end_pauses_ns"]), run["concurrent_markings"])
                self.assertGreater(run["background_mark_ns"], 0)

    def test_v8s_heap(self):
        for heap in ("v8", "v8-single"):
            run, (begin, end) = self.run_bench(heap, "tree-low")
            self.assertLessEqual(run["run_ns"] + run["idle_ns"], end - begin, heap)
            # 200,000 objects, most dropped: V8 scavenged during the turns,
            # some survived, and the forced GC marked what was kept.
            self.assertGreater(run["young"]["count"], 0, heap)
            self.assertGreater(run["young"]["survived_bytes"], 0, heap)
            self.assertGreater(run["young"]["ns"], 0, heap)
            self.assertGreater(run["young"]["capacity_bytes"], 0, heap)
            self.assertGreater(run["gc_by_kind_ns"]["young"], 0, heap)
            self.assertGreater(run["forced_mark_ns"], 0, heap)
            self.assertGreater(run["after_forced_gc"]["used"], run["baseline_used"], heap)


if __name__ == "__main__":
    unittest.main()
