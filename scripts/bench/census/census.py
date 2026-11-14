#!/usr/bin/env python3
"""`bench.py census`: what each binding call makes on its way, counted in
machopo built with the `census` feature (`base::census`): the scopes of V8's
a callback opens, the DOM heap's roots it registers and the operations it
runs, by callback. Counts, not times: one run of each page is enough.

Each page runs in a new machopo told where to write its counts
(`MACHOPO_CENSUS`): Blink's perf tests (`blink-perf`: the listed pages, or
`--dirs`), or Speedometer's suites, one iteration each (`speedometer`). The
counts are in `target/bench/census/<workload>/` (a `.tsv` a page), their
sums in `census.md` there.
"""

from __future__ import annotations

import collections
import csv
import re
import subprocess
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(BENCH))
import common  # noqa: E402
from common import ROOT  # noqa: E402

RESULTS = ROOT / "target" / "bench" / "census"

# What a call makes, as the file's columns name them, and each one's
# column of how many calls made any.
MADE = ("scopes", "trampoline_scopes", "report_scopes", "reads", "mutates", "roots")
WITH = ("with_scope", "with_trampoline_scope", "with_report_scope", "with_read", "with_mutate", "with_root")

# The row of what a thread made outside any callback.
OUTSIDE = "(outside a callback)"


def build() -> Path:
    """machopo with the census, in its own target folder (the feature
    changes every crate under `base`: the plain build's stays)."""
    target = ROOT / "target" / "census"
    env = dict(common.build_env(common.MACHOPO_PROFILE), CARGO_TARGET_DIR=str(target))
    done = subprocess.run(["cargo", "build", "-q", "--profile", common.MACHOPO_PROFILE, "-p", "shell", "--bin",
                           "machopo", "--features", "census"], cwd=ROOT, env=env)
    if done.returncode != 0:
        raise SystemExit("building machopo with the census failed")
    return target / common.MACHOPO_PROFILE / ("machopo.exe" if sys.platform == "win32" else "machopo")


def callback_name(row: dict) -> tuple[str, str]:
    """A row's callback: the kind of its entry (the callback's type, without
    its path and parameters: `InfoMember`) and its member (`Element.id
    (getter)`), or the entry's type with its paths taken off when the
    callback names no member."""
    entry = row["entry"]
    kind = re.sub(r"<.*", "", entry).split("::")[-1]
    if row["interface"]:
        return kind, f"{row['interface']}.{row['member']} ({row['kind']})"
    return kind, re.sub(r"\w+::", "", entry)


def read(folder: Path) -> dict[tuple[str, str], collections.Counter]:
    """The counts of every page's file in `folder`, summed by callback."""
    sums: dict[tuple[str, str], collections.Counter] = collections.defaultdict(collections.Counter)
    for path in sorted(folder.glob("*.tsv")):
        with open(path, encoding="utf-8", newline="") as file:
            for row in csv.DictReader(file, delimiter="\t"):
                counts = sums[callback_name(row)]
                for column in ("calls", *MADE, *WITH):
                    counts[column] += int(row[column])
    return dict(sums)


def share(part: int, whole: int) -> str:
    return f"{100 * part / whole:.1f}%" if whole else ""


def report(sums: dict[tuple[str, str], collections.Counter], workload: str, top: int) -> str:
    """The report: the totals, the counts by the kind of entry, and the
    callbacks that made the most of each kind."""
    total: collections.Counter = collections.Counter()
    by_kind: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    for (kind, _), counts in sums.items():
        total.update(counts)
        by_kind[kind].update(counts)
    calls = total["calls"]
    lines = [f"# What binding calls make: {workload}", "",
             f"{calls:,} calls. What each made itself (a callback called within another counts as its own).", "",
             "| | made | calls that made any | of the calls |", "| --- | ---: | ---: | ---: |"]
    for made, with_ in zip(MADE, WITH):
        lines.append(f"| {made} | {total[made]:,} | {total[with_]:,} | {share(total[with_], calls)} |")
    header = "| callback | calls | " + " | ".join(MADE) + " |"
    rule = "| --- | ---: |" + " ---: |" * len(MADE)

    def line(name: str, counts: collections.Counter) -> str:
        return f"| `{name}` | {counts['calls']:,} | " + " | ".join(f"{counts[m]:,}" for m in MADE) + " |"

    lines += ["", "## By the kind of entry", "", header, rule]
    lines += [line(kind, counts) for kind, counts in sorted(by_kind.items(), key=lambda item: -item[1]["calls"])]
    for column in ("roots", "scopes", "trampoline_scopes", "calls"):
        ranked = sorted(sums.items(), key=lambda item: -item[1][column])[:top]
        lines += ["", f"## The most {column}", "", header, rule]
        lines += [line(f"{kind} {name}" if name != kind else kind, counts)
                  for (kind, name), counts in ranked if counts[column]]
    return "\n".join(lines) + "\n"


def run_blink_perf(binary: Path, folder: Path, args) -> None:
    sys.path.insert(0, str(BENCH / "blink_perf"))
    import blink_perf

    root = blink_perf.BLINK_PERF
    dirs = [d for d in (args.dirs or "").split(",") if d]
    names = blink_perf.pages(root, dirs, [] if dirs else blink_perf.listed(blink_perf.PAGES))
    server = blink_perf.Server(root)
    try:
        with open(folder / "machopo.log", "w", encoding="utf-8") as log:
            for page in names:
                if "excluded" in blink_perf.scan(root, page):
                    continue
                counts = folder / (page.replace("/", "__") + ".tsv")
                got = blink_perf.result(common.run_in_machopo(server, binary, server.url("/" + page), args.timeout,
                                                              log, env={"MACHOPO_CENSUS": str(counts)}))
                print(f"  {page}: " + (got["error"].splitlines()[0] if "error" in got else "counted"), flush=True)
    finally:
        server.close()


def run_speedometer(binary: Path, folder: Path, args) -> None:
    sys.path.insert(0, str(BENCH / "speedometer"))
    import speedometer

    chosen = [s for s in (args.suites or "").split(",") if s]
    names = [name for name, default in speedometer.suites() if (name in chosen if chosen else default)]
    server = speedometer.Server()
    server.mode = "measure"
    server.deadline = speedometer.measure_deadline(1) * 1000
    timeout = speedometer.measure_deadline(1) + speedometer.START_TIME
    try:
        with open(folder / "machopo.log", "w", encoding="utf-8") as log:
            for name in names:
                got = speedometer.parse_run(common.run_in_machopo(
                    server, binary, speedometer.suite_url(server, name, 1), timeout, log,
                    env={"MACHOPO_CENSUS": str(folder / (name + ".tsv"))}))
                print(f"  {name}: {got.get('status') or got['error']}", flush=True)
    finally:
        server.close()


def cmd_census(args) -> int:
    folder = RESULTS / args.workload
    folder.mkdir(parents=True, exist_ok=True)
    for old in folder.glob("*.tsv"):
        old.unlink()
    binary = build()
    print(f"the census of {args.workload}, in machopo with the census feature")
    (run_speedometer if args.workload == "speedometer" else run_blink_perf)(binary, folder, args)
    sums = read(folder)
    if not sums:
        print("no page wrote its counts")
        return 1
    path = folder / "census.md"
    path.write_text(report(sums, args.workload, args.top), encoding="utf-8", newline="\n")
    print(f"the report: {path}")
    return 0
