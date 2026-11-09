#!/usr/bin/env python3
"""The benchmarks: machopo measured, against Chrome for Testing where there is
one to compare with. Each benchmark's scripts are in `scripts/bench/<name>/`,
its results in `target/bench/<name>/`.

    python scripts/bench.py speedometer [--inventory]  # Speedometer 3.1 in Chrome and machopo; what its suites need that machopo lacks
    python scripts/bench.py raster [--window]  # machopo's tile raster while pages scroll and its window is resized
    python scripts/bench.py blink-perf [dom/ ...] [--against REV]  # Blink's perf tests in Chrome and machopo (or two machopos): the list, or part of it
    python scripts/bench.py calls [--cases firstChild,...]  # what one DOM call costs from JS, in instructions
    python scripts/bench.py gc [--heaps gc,gc-parallel,v8]  # the GC microbenchmarks on the gc crate's heap and V8's: pauses, GC time, memory

Standard library only. Every command exits non-zero on failure.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent / "bench"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    speedometer = sub.add_parser(
        "speedometer",
        help="Speedometer 3.1 in Chrome for Testing and machopo: each suite measured in a new browser, which runs "
        "are valid (results in target/bench/speedometer/); --inventory: what the suites need that machopo lacks",
    )
    speedometer.add_argument(
        "--inventory",
        action="store_true",
        help="record what each suite uses in Chrome, probe machopo for it, run each default suite in machopo; "
        "results in target/bench/speedometer/",
    )
    speedometer.add_argument("--suites", help="only these suites (comma-separated names); default: the default ones")
    speedometer.add_argument(
        "--browser", choices=["chrome", "machopo"], help="measure in this browser only (machopo's runs are checked against "
        "Chrome's last ones, if any)"
    )
    speedometer.add_argument(
        "--iterations", type=int, default=10, help="iterations of each suite, in a row, in one page (default 10)"
    )
    speedometer.add_argument(
        "--trace",
        action="store_true",
        help="record each suite once in each browser with a trace (target/bench/speedometer/trace/, for "
        "https://ui.perfetto.dev), its times not compared",
    )
    speedometer.add_argument(
        "--profile",
        action="store_true",
        help="run each suite once in each browser with DevTools' Profiler sampling its script every ms (machopo through "
        "its automation pipe), from the navigation to the report: the profiles (target/bench/speedometer/profile/) and "
        "each browser's time by the page's JS functions, profile.md",
    )
    speedometer.add_argument(
        "--sample",
        action="store_true",
        help="sample each suite once in machopo with Linux's perf or samply (macOS; Windows, with the Windows Performance "
        "Toolkit's xperf, as Administrator: UAC asks once), only in the windows the runner times "
        "(target/bench/speedometer/sample/, the functions most samples are in: sample.md); with --browser chrome, "
        "a Chromium with symbols instead (Linux); with both's samples and times, the samples by category: "
        "categories.md",
    )
    speedometer.add_argument(
        "--chrome-binary",
        metavar="PATH",
        help="with --sample --browser chrome: the Chromium sampled (default Fedora's "
        "/usr/lib64/chromium-browser/chromium-browser, its symbols from `dnf debuginfo-install chromium`)",
    )
    speedometer.add_argument(
        "--against",
        metavar="REV",
        help="machopo at this commit (checked out in ../machopo-against, built into target/against; it must pin "
        "../third_party alike) and the working tree's, in turn: their times and peak memory, or with --sample their "
        "samples; target/bench/speedometer/compare.md",
    )
    speedometer.add_argument(
        "--runs", type=int, default=3, help="with --against: how many times each version runs (default 3)"
    )
    speedometer.add_argument(
        "--reuse-chrome",
        action="store_true",
        help="use Chrome's last results: its recording with --inventory (target/bench/speedometer/chrome.json), "
        "else its runs, to check machopo's against (chrome-runs.json)",
    )
    speedometer.set_defaults(func=cmd_speedometer)

    raster = sub.add_parser(
        "raster",
        help="machopo's tile raster while a few pages scroll and its window is resized: the compositor thread's "
        "PrepareTiles, the tiles and how many are one color (target/bench/raster/raster.md)",
    )
    raster.add_argument("--window", action="store_true", help="a real window (it stays on top); default: headless")
    raster.add_argument("--pages", help="only these pages (comma-separated): gutenberg, sqlite, newssite-next")
    raster.add_argument("--scale", type=float, default=2.0,
                        help="headless: the device scale factor (default 2, a high-DPI screen's)")
    raster.add_argument("--raster-threads", type=int,
                        help="machopo's --num-raster-threads (default: machopo's, 1)")
    raster.set_defaults(func=cmd_raster)

    blink_perf = sub.add_parser(
        "blink-perf",
        help="Blink's perf tests (../third_party/blink_perf) in machopo and Chrome for Testing, each page in a new "
        "browser of each: each page's median, how many times slower machopo is, by folder "
        "(target/bench/blink_perf/report.md)",
    )
    blink_perf.add_argument("areas", nargs="*", metavar="PATH",
                            help="only the listed pages under these paths (dom/, layout/flexbox-row-nowrap.html); "
                            "default: every page of scripts/bench/blink_perf/pages.txt")
    blink_perf.add_argument("--dirs", help="every page in these folders, listed or not (comma-separated: dom,css)")
    blink_perf.add_argument("--pages", help="these pages, listed or not (comma-separated paths: "
                            "dom/modify-element-classname.html)")
    # The slowest page measured takes some 75 s (machopo); most take under 20.
    blink_perf.add_argument("--timeout", type=float, default=90,
                            help="how long a page may take in each browser, s (default 90)")
    blink_perf.add_argument(
        "--against",
        metavar="REV",
        help="machopo at this commit (checked out in ../machopo-against, built into target/against; it must pin "
        "../third_party alike) and the working tree's, in turn, no Chrome: target/bench/blink_perf/compare.md",
    )
    blink_perf.add_argument(
        "--runs", type=int, default=3,
        help="with --against: how many times each version runs the pages (default 3)"
    )
    blink_perf.set_defaults(func=cmd_blink_perf)

    calls = sub.add_parser(
        "calls",
        help="what one call of a few DOM members costs from JS: instructions and cycles "
        "per call, counted by perf stat over a loop (target/bench/calls/calls.md); Linux",
    )
    calls.add_argument("--engines", help="only these (comma-separated): js")
    calls.add_argument("--cases", help="only these members (comma-separated): firstChild, nodeType, "
                       "getAttribute, setAttribute, appendChild, createElement")
    calls.add_argument("--n", type=int, default=1_000_000, help="calls in each loop (default 1000000)")
    calls.add_argument("--runs", type=int, default=3, help="runs of each loop, the median taken (default 3)")
    calls.add_argument("--profile", default="perf", help="the cargo profile (default perf: machopo's, measured)")
    calls.set_defaults(func=cmd_calls)

    gc = sub.add_parser(
        "gc",
        help="the GC microbenchmarks (gc_bench) on the gc crate's heap, DOM-like workloads on a tree and "
        "allocation patterns, and the patterns on V8's heap too: the GC's time on the main thread and the "
        "others, its pauses, the heap's peak, the marking's and V8's scavenges' speed (target/bench/gc/gc.md)",
    )
    gc.add_argument("--heaps", help="only these (comma-separated): v8, v8-single (V8 with "
                    "--single-threaded-gc), gc, gc-parallel (the gc crate's, marking on one thread and on "
                    "workers)")
    gc.add_argument("--workloads", help="only these (comma-separated): build, churn, replace, list (a DOM's, "
                    "on a tree), ids (the gc crate's id table), chain-high, chain-low, tree-high, tree-low, "
                    "graph-high, graph-low (the patterns)")
    gc.add_argument("--runs", type=int, default=3, help="runs of each workload on each heap, the median taken (default 3)")
    gc.add_argument("--scale", type=float, default=1.0, help="each workload's size, times its own (default 1)")
    gc.add_argument("--baseline", metavar="FILE", help="a gc.json of an earlier run: its rows of the heaps "
                    "this run leaves out go in the report next to this run's, marked (baseline)")
    gc.set_defaults(func=cmd_gc)

    args = parser.parse_args()
    return args.func(args)


def cmd_speedometer(args: argparse.Namespace) -> int:
    sys.path.insert(0, str(BENCH / "speedometer"))
    import speedometer

    return speedometer.cmd_speedometer(args)


def cmd_blink_perf(args: argparse.Namespace) -> int:
    sys.path.insert(0, str(BENCH / "blink_perf"))
    import blink_perf

    return blink_perf.cmd_blink_perf(args)


def cmd_calls(args: argparse.Namespace) -> int:
    sys.path.insert(0, str(BENCH / "calls"))
    import calls

    return calls.cmd_calls(args)


def cmd_gc(args: argparse.Namespace) -> int:
    sys.path.insert(0, str(BENCH / "gc"))
    import gc_bench

    return gc_bench.cmd_gc(args)


def cmd_raster(args: argparse.Namespace) -> int:
    sys.path.insert(0, str(BENCH / "raster"))
    import raster

    return raster.cmd_raster(args)


if __name__ == "__main__":
    sys.exit(main())
