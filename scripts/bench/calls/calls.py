#!/usr/bin/env python3
"""`bench.py calls`: what one call of a DOM member costs from JS: the
instructions (and cycles) per call, not a time.

Each member (`CASES`) is called `--n` times in a loop, in a test binary of
the `perf` profile: `dom_js`'s ignored test `bench::bench_calls`, a
page's script (`MACHOPO_BENCH_SCRIPT`) on a document of the DOM's
(`machopoDom`) in a realm of its own. `perf stat`
counts the whole process's user-space instructions and cycles; the same
loop with nothing in it (`empty`) is taken away and the rest divided by
`--n`. Each count is the median of `--runs` runs (the empty loop's of
`EMPTY_RUNS` times as many: its count jumps by 10 million instructions in
some runs, which the median of three let through once in a few rounds,
moving every case alike), on one of the fastest CPUs
(`common.fastest_cpus`).

Instructions hardly move from run to run (0.2%), nor with where the code
lands in the binary, so two builds compare: a time per call moves by a
tenth or more between builds of the same code, and runs/s even more. The
results go to `target/bench/calls/calls.md` (and `calls.json`). Linux with
`perf` only.

Where the CPU counts them, `perf stat` counts store-to-load forwarding
conflicts too (`stlf`: a load that waits for an older store it overlaps
to reach the cache, as a narrow store then a wide load of the same bytes
does), per call as the others: a hot member's call should have none (an
inlined 32-bit store of a callback's return value, which the builtin
read whole, cost 2.3 ns of a 5.4 ns empty getter on x86-64). The event is
the CPU's own (`STLF_EVENTS`); with none, the column is left out.

Standard library only.
"""

from __future__ import annotations

import json
import os
import shutil
import statistics
import subprocess
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import common  # noqa: E402
from common import ROOT  # noqa: E402

RESULTS = ROOT / "target" / "bench" / "calls"

# What each JS loop has before it: a `<body>` with a child and an `id`;
# an element in no tree with a listener, and an event of its type.
EVENTS_SETUP = (
    "const t = document.createElement('i'); const f = () => {}; t.addEventListener('x', f); "
    "const ev = new Event('x');"
)
# The members, by name: the loop's body.
CASES = {
    "empty": "x = n;",
    "firstChild": "x = n.firstChild;",
    "nodeType": "x = n.nodeType;",
    "getAttribute": "x = n.getAttribute('id');",
    "setAttribute": "n.setAttribute('a', 'b');",
    "appendChild": "n.appendChild(c);",
    "createElement": "x = document.createElement('div');",
    # The listener it has already: its lookup, the callbacks compared.
    "addEventListener": "t.addEventListener('x', f);",
    # A dispatch at a target alone, its one listener empty: the fixed cost
    # (the roots, the operations, the call into script).
    "dispatchEvent": "t.dispatchEvent(ev);",
}

# What each loop has before it, in a block (a global `const document` is an
# error where the global is a window, whose is unforgeable): a `<body>` of a
# document of the DOM's with a child and an `id`; an element in no tree
# with a listener, and an event of its type.
JS_SETUP = (
    "const document = machopoDom.createDocument(); const n = document.createElement('body'); "
    "document.appendChild(n); n.appendChild(document.createElement('p')); "
    f"n.setAttribute('id', 'x'); const c = n.firstChild; let x; const Event = machopoDom.Event; {EVENTS_SETUP}"
)

# The cold paths: a member's call that isn't its usual one, which needs
# more before the loop than the others (`{n}` is the loop's count) and so
# its own loop to take away, of the same setup (the setup is in the counts
# too). By name: what is before the loop, the loop's body, and the body of
# the loop taken away.
BIG_SETUP = "const big = document.createElement('div'); big.innerHTML = '<i></i>'.repeat({n} + 1);"
COLD = {
    # An object returned for the first time (it has no wrapper): rooted
    # across the operation, its wrapper made in a scope after.
    "classList (first)": (
        "const els = []; for (let i = 0; i < {n}; i++) els.push(document.createElement('i'));",
        "x = els[i].classList;", "x = els[i];"),
    # A node script hasn't seen, returned by a getter and by a collection's
    # index.
    "nextSibling (first)": (BIG_SETUP + " let w = big.firstChild;", "w = w.nextSibling;", "x = w;"),
    "list[i] (first)": (BIG_SETUP + " const list = big.childNodes;", "x = list[i];", "x = list;"),
    # The same of a node that has its wrapper.
    "list[0]": ("const list = n.childNodes;", "x = list[0];", "x = list;"),
    # Whether a collection has an index: the interceptor's query.
    "0 in list": ("const list = n.childNodes;", "x = 0 in list;", "x = list;"),
}

# The name of the loop taken away from a cold path's.
BASE = ":base"

ENGINES = {
    "js": ("dom_js", "bench::bench_calls"),
}

EVENTS = ("instructions", "cycles")

# The store-to-load forwarding conflicts' event, by `/proc/cpuinfo`'s
# vendor: a load unable to complete for a conflict with an older store it
# can't forward from.
STLF_EVENTS = {
    "AuthenticAMD": "ls_bad_status2.stli_other",
    "GenuineIntel": "ld_blocks.store_forward",
}


def stlf_event(cpuinfo: str | None = None, listed: str | None = None) -> str | None:
    """The CPU's store-to-load forwarding conflicts' event, if its vendor
    has one and `perf list` has it."""
    if cpuinfo is None:
        try:
            cpuinfo = Path("/proc/cpuinfo").read_text(encoding="utf-8")
        except OSError:
            return None
    vendor = next((line.split(":", 1)[1].strip() for line in cpuinfo.splitlines()
                   if line.startswith("vendor_id")), None)
    event = STLF_EVENTS.get(vendor or "")
    if event is None:
        return None
    if listed is None:
        listed = subprocess.run(["perf", "list", "--no-desc", event], capture_output=True, text=True).stdout
    return event if event in listed else None


def test_binary(package: str, profile: str) -> Path:
    """`package`'s integration test binary (`tests/it`), built with
    `profile`."""
    done = subprocess.run(
        ["cargo", "test", "-q", "--profile", profile, "-p", package, "--test", "it", "--no-run",
         "--message-format=json"],
        cwd=ROOT, env=common.build_env(profile), capture_output=True, text=True)
    if done.returncode != 0:
        raise SystemExit(f"building {package}'s tests failed:\n{done.stderr[-3000:]}")
    for line in done.stdout.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        source = Path(message.get("target", {}).get("src_path", ""))
        if message.get("reason") == "compiler-artifact" and message.get("executable") \
                and source == ROOT / "crates" / package / "tests" / "it" / "main.rs":
            return Path(message["executable"])
    raise SystemExit(f"no test binary of {package} in cargo's messages")


def parse_perf_stat(text: str, stlf: str | None = None) -> dict[str, float]:
    """The counts of `perf stat -x,`'s output (`value,unit,event,...`),
    by event name (`instructions:u` is `instructions`; `stlf`'s, if any, is
    `stlf`)."""
    counts = {}
    for line in text.splitlines():
        fields = line.split(",")
        if len(fields) < 3:
            continue
        event = fields[2].split(":")[0]
        if stlf is not None and event == stlf:
            event = "stlf"
        if event in EVENTS or event == "stlf":
            try:
                counts[event] = float(fields[0])
            except ValueError:
                raise SystemExit(f"perf didn't count {event}: {line}")
    return counts


def environment(engine: str, case: str, n: int) -> dict[str, str]:
    """What the engine's test is told to run."""
    name, base = case.removesuffix(BASE), case.endswith(BASE)
    if name in COLD:
        setup, body, base_body = COLD[name]
        setup, body = setup.replace("{n}", str(n)), base_body if base else body
    else:
        setup, body = "", CASES[case]
    return {"MACHOPO_BENCH_SCRIPT": f"{{ {JS_SETUP} {setup} for (let i = 0; i < {n}; i++) {{ {body} }} }} 0"}


def run_once(binary: Path, test: str, env: dict[str, str], cpu: list[int] | None,
             stlf: str | None = None) -> dict[str, float]:
    events = [*EVENTS, *([stlf] if stlf else [])]
    command = ["perf", "stat", "-x,", "-e", ",".join(f"{e}:u" for e in events), "--", str(binary), "--ignored",
               "--exact", test, "--test-threads", "1"]
    if cpu:
        command = ["taskset", "-c", common.cpu_list(cpu), *command]
    done = subprocess.run(command, cwd=ROOT, env={**os.environ, **env}, capture_output=True,
                          text=True)
    if done.returncode != 0 or "1 passed" not in done.stdout:
        raise SystemExit(f"{test} failed:\n{done.stdout[-2000:]}\n{done.stderr[-2000:]}")
    return parse_perf_stat(done.stderr, stlf)


# How many times a round runs the empty loop: its count is the one every
# case's is taken from, and jumps by about 10 instructions a call (of
# 1,000,000 calls) in about one run of six.
EMPTY_RUNS = 3


def per_call(counts: dict[str, list[dict[str, float]]], n: int) -> dict[str, dict[str, float]]:
    """Each case's median counts less the empty loop's (a cold path's, less
    its own loop's: `BASE`), per call (each event every run counted)."""
    events = [e for e in (*EVENTS, "stlf") if all(e in run for runs in counts.values() for run in runs)]
    medians = {case: {e: statistics.median(run[e] for run in runs) for e in events} for case, runs in counts.items()}
    return {case: {e: (m[e] - medians.get(case + BASE, medians["empty"])[e]) / n for e in events}
            for case, m in medians.items() if case != "empty" and not case.endswith(BASE)}


def write_report(results: dict[str, dict[str, dict[str, float]]], n: int, runs: int, profile: str,
                 cpu: list[int] | None) -> Path:
    RESULTS.mkdir(parents=True, exist_ok=True)
    (RESULTS / "calls.json").write_text(json.dumps({"n": n, "runs": runs, "profile": profile, "results": results},
                                                   indent=2), encoding="utf-8", newline="\n")
    engines = list(results)
    lines = [
        f"# What a DOM call costs ({date.today().isoformat()})",
        "",
        f"machopo's `{profile}` profile; each member called {n:,} times in a loop, the empty loop taken away; the "
        f"median of {runs} runs of `perf stat` (user space, the whole process), on CPU "
        f"{common.cpu_list(cpu) if cpu else 'any'}. Instructions per call (cycles in brackets; store-to-load "
        f"forwarding conflicts in square brackets where the CPU counts them).",
        "",
        "| Member | " + " | ".join(engines) + " |",
        "| --- |" + " ---: |" * len(engines),
    ]
    cases = [case for case in (*CASES, *COLD) if case != "empty"]
    for case in cases:
        cells = []
        for engine in engines:
            got = results[engine].get(case)
            stlf = f" [{got['stlf']:.2f}]" if got and "stlf" in got else ""
            cells.append(f"{got['instructions']:.0f} ({got['cycles']:.0f}){stlf}" if got else "")
        lines.append(f"| `{case}` | " + " | ".join(cells) + " |")
    report = RESULTS / "calls.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


def compare(results: dict[str, dict[str, dict[str, float]]], baseline: dict[str, dict[str, dict[str, float]]],
            name: str) -> str:
    """This run's counts next to an earlier run's (`baseline`, its
    `calls.json`'s results), by member: each event's before, now and the
    difference; a member only one of them measured is left out."""
    lines = [f"## Against {name}", ""]
    for engine, cases in results.items():
        before = baseline.get(engine, {})
        events = [e for e in (*EVENTS, "stlf")
                  if all(e in got and e in before[case] for case, got in cases.items() if case in before)]
        lines += [f"| Member ({engine}) | " + " | ".join(events) + " |", "| --- |" + " ---: |" * len(events)]
        for case, got in cases.items():
            if case not in before:
                continue
            digits = lambda e: 2 if e == "stlf" else 0
            cells = [f"{before[case][e]:.{digits(e)}f} → {got[e]:.{digits(e)}f} ({got[e] - before[case][e]:+.{digits(e)}f})"
                     for e in events]
            lines.append(f"| `{case}` | " + " | ".join(cells) + " |")
        lines.append("")
    return "\n".join(lines)


def cmd_calls(args) -> int:
    if not sys.platform.startswith("linux") or not shutil.which("perf"):
        print("bench.py calls needs Linux's perf")
        return 1
    engines = args.engines.split(",") if args.engines else list(ENGINES)
    known = [c for c in (*CASES, *COLD) if c != "empty"]
    cases = ["empty", *(args.cases.split(",") if args.cases else known)]
    for name in engines:
        if name not in ENGINES:
            print(f"no engine {name}; the engines: {', '.join(ENGINES)}")
            return 2
    for case in cases:
        if case not in known and case != "empty":
            print(f"no case {case}; the cases: {', '.join(known)}")
            return 2
    cases += [case + BASE for case in cases if case in COLD]
    fastest = common.fastest_cpus() if shutil.which("taskset") else None
    cpu = fastest[:1] if fastest else None
    stlf = stlf_event()
    results = {}
    for engine in engines:
        package, test = ENGINES[engine]
        binary = test_binary(package, args.profile)
        engine_cases = cases
        counts: dict[str, list[dict[str, float]]] = {case: [] for case in engine_cases}
        # Runs in turn, each case once a round, so that what drifts drifts
        # alike for all of them.
        for _ in range(args.runs):
            for case in engine_cases:
                for _ in range(EMPTY_RUNS if case == "empty" else 1):
                    counts[case].append(run_once(binary, test, environment(engine, case, args.n), cpu, stlf))
        results[engine] = per_call(counts, args.n)
        print(f"  {engine}: " + ", ".join(f"{case} {got['instructions']:.0f}" for case, got in results[engine].items()))
    baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8")) if args.baseline else None
    report = write_report(results, args.n, args.runs, args.profile, cpu)
    if baseline:
        with open(report, "a", encoding="utf-8", newline="\n") as file:
            file.write("\n" + compare(results, baseline["results"], args.baseline))
    print(report.read_text(encoding="utf-8"))
    return 0
