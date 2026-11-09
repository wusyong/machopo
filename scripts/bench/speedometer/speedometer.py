"""`bench.py speedometer`: Speedometer 3.1 (`../third_party/speedometer`, at
the commit `third_party.toml` pins) in Chrome and in machopo.

Both browsers load the same files from a local server (standard library),
which adds `scripts/bench/speedometer/`'s scripts to the pages on the way;
Speedometer itself is never changed. What a page has to hand back it posts
to the server. Both are driven with CDP through a pipe: Chrome for Testing
(`CHROME_VERSION`, downloaded under `target/`) runs headless with
`--remote-debugging-pipe`, machopo as `machopo --automation-pipe` (a browser
window without the platform's window, its frames every 16.7 ms); each is
told through it to navigate, and to close, so that it ends on its own.
Each page gets a new process of its own. The results go to
`target/bench/speedometer/`.

Without `--inventory`, it measures: each suite on its own, in a new
browser, `--iterations` times in a row, with `measure.mjs` in the runner's
page; once in machopo (the `perf` profile) and right after in Chrome (or in
one of them, `--browser`). That takes the times the runner measures, and
after each test a summary of the suite's document; a run is valid when it
got to the end without an exception and every summary is the one Chrome
had for that test in its first iteration (Chrome's own later ones too),
but for the classes a page sets in a frame after a test's steps, which a
test has or not as that frame comes before the runner ends it
(`TIMING_CLASSES`; a run that never had one is noted in `measure.md`). A
suite with either run not valid has no numbers. The runs go
to `chrome-runs.json` and `machopo-runs.json`, the medians of the valid
suites' times, each browser's and their ratio, to `measure.md`.

With `--trace`, it records each suite once in each browser (`--iterations`
iterations) with a trace, its times not compared: machopo's (`MACHOPO_TRACE`: its
spans and V8's events) and Chrome's (CDP's `Tracing`, from before the page
loads), both Perfetto's protobuf under `trace/`, for Perfetto's UI and
trace processor; and from them each phase's time (script, style, building
the box tree, layout...) in the windows the runner times, side by side,
`phases.md`.

With `--sample`, it runs each suite once in machopo under Linux's `perf` (its
cycles and instructions) or samply (the CPU time; macOS, and Windows
through ETW, which needs Administrator: the samples are taken in a copy of
this script that UAC lets run as one, asking once), with a trace: the
renderer's main thread's stacks in the windows the runner times, JS
functions named by V8, folded under `sample/` (for speedscope or
flamegraph.pl), and the functions most samples are in, `sample.md`. The
`perf` profile keeps frame pointers on Linux and macOS, as Chrome does.

With `--profile`, it runs each suite once in each browser with DevTools'
`Profiler` sampling the page's script every millisecond (machopo through its
automation pipe's same methods), from the navigation to the report: the
profiles under `profile/` (`.cpuprofile`, for Chrome DevTools' Performance
panel), and each browser's time in the windows the runner times by the
page's JS functions side by side, `profile.md` (`profiles.py`).

With `--against REV`, machopo at that commit and the working tree's take turns
(`--runs` each), in machopo only: their times (every iteration of a version's
runs, a median and its interval) and machopo's peak memory, or with `--sample`
their samples, compared in `compare.md`.

With `--inventory`, it finds what the suites need that machopo lacks:

1. Chrome runs every suite once, with `record.js` in each page: which Web
   API members each suite's page uses, and (from the runner's side,
   `inventory.mjs`) the CSS it has.
2. machopo loads a probe page with all of that (`probe.js`): which of those
   members, declarations, selectors, kinds of at-rules and elements machopo
   has.
3. machopo runs each default suite on its own (`machopo.mjs`): whether it gets to
   the end, and the first exceptions if it doesn't.

That goes to `inventory.json` (everything) and `inventory.md` (by suite).
"""

from __future__ import annotations

import bisect
import collections
import gzip
import json
import math
import os
import queue
import re
import shutil
import struct
import subprocess
import sys
import threading
import time
import traceback
import urllib.parse
import urllib.request
from datetime import date
from pathlib import Path

# What the benchmarks share, from `scripts/bench/common.py`, kept as names of
# this module too: `raster.py` and the tests reach them through it.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import common  # noqa: E402
from common import *  # noqa: E402,F403
import categories  # noqa: E402
import profiles  # noqa: E402

SPEEDOMETER = ROOT.parent / "third_party" / "speedometer"
# The scripts it adds to the pages (`measure.mjs`...), beside this file.
SCRIPTS = Path(__file__).resolve().parent
RESULTS = ROOT / "target" / "bench" / "speedometer"


# How long Chrome may take to record every suite; machopo to run one suite
# once, and the probe.
CHROME_TIMEOUT = 600
MACHOPO_SUITE_DEADLINE = 90
MACHOPO_PROBE_DEADLINE = 120

# How long a suite may take to be measured: the page's own deadline (it
# then posts what it has), and a little more for the browser to start.
# The slowest suite's iteration takes a fraction of a second, sampled or
# traced a few times that; far past it the browser is stuck, not slow.
MEASURE_DEADLINE = 30
MEASURE_DEADLINE_PER_ITERATION = 10
START_TIME = 30


def measure_deadline(iterations: int) -> int:
    return MEASURE_DEADLINE + MEASURE_DEADLINE_PER_ITERATION * iterations


# --- Chrome for Testing ---------------------------------------------------


# --- machopo ------------------------------------------------------------------


# --- the server -----------------------------------------------------------

class Server(common.Server):
    """Speedometer's files (or, for the tool's own tests, another `root`'s),
    with `scripts/bench/speedometer/` added to its pages: `errors.js` first in
    every page, `record.js` after it when Chrome records the inventory, and
    the runner's module after `main.mjs`: `inventory.mjs` (recording),
    `machopo.mjs` (machopo's runs of the inventory) or `measure.mjs` (measuring).
    Also serves machopo's probe page, and takes what pages post."""

    MODULES = {"record": "inventory.mjs", "inventory": "machopo.mjs", "measure": "measure.mjs"}

    def __init__(self, root: Path | None = None) -> None:
        super().__init__(root or SPEEDOMETER)
        self.mode = "record"
        # What the runner's module gives up after, ms.
        self.deadline = MACHOPO_SUITE_DEADLINE * 1000
        # Whether `measure.mjs` holds the runner's start until the page's
        # `__machopoStart()` (`--profile`, once the profile has started).
        self.hold = False
        self.probe: dict | None = None

    def get(self, path: str) -> tuple[bytes | None, str]:
        if path == SUMMARY:
            return super().get(path)
        if path.startswith("/__machopo__/"):
            name = path.removeprefix("/__machopo__/")
            if name == "probe.html":
                data = json.dumps(self.probe).replace("</", "<\\/")
                page = (
                    "<!DOCTYPE html><meta charset=utf-8><title>machopo's probe</title>"
                    f"<script>window.__machopoProbe = {data};</script>"
                    '<script src="/__machopo__/probe.js"></script>'
                )
                return page.encode(), "text/html; charset=utf-8"
            file = SCRIPTS / name
            if not file.is_file():
                return None, ""
            text = file.read_text(encoding="utf-8").replace("__DEADLINE__", str(self.deadline))
            text = text.replace("__HOLD__", "true" if self.hold else "false")
            return text.encode(), "text/javascript; charset=utf-8"
        file = self.file(path)
        if file is None:
            return None, ""
        data = file.read_bytes()
        if file.suffix == ".html":
            page = path + "index.html" if path.endswith("/") else path
            data = self.inject(page, data.decode("utf-8")).encode("utf-8")
        return data, content_type(file)

    def inject(self, path: str, html: str) -> str:
        first = '<script src="/__machopo__/errors.js"></script>'
        if self.mode == "record":
            first += '<script src="/__machopo__/record.js"></script>'
        head = re.search(r"<head[^>]*>", html, re.I)
        at = head.end() if head else 0
        html = html[:at] + first + html[at:]
        if path == "/index.html":
            main = '<script src="resources/main.mjs" type="module"></script>'
            if main not in html:
                raise SystemExit("index.html no longer loads resources/main.mjs as it did")
            module = self.MODULES[self.mode]
            html = html.replace(main, main + f'<script type="module" src="/__machopo__/{module}"></script>')
        return html


# --- the suites -----------------------------------------------------------

def suites() -> list[tuple[str, bool]]:
    """Every suite of `resources/tests.mjs`, in order, with whether it runs
    by default (it isn't `disabled: true`)."""
    text = (SPEEDOMETER / "resources" / "tests.mjs").read_text(encoding="utf-8")
    found = []
    for block in re.split(r"\nSuites\.push\(\{\n", text)[1:]:
        name = re.search(r'^\s*name: "([^"]+)"', block, re.M)
        if name:
            found.append((name.group(1), not re.search(r"^\s*disabled: true", block, re.M)))
    return found


def suite_url(server: Server, suite: str, iterations: int) -> str:
    return server.url(f"/index.html?startAutomatically&iterationCount={iterations}&suites={suite}")


# --- the runs -------------------------------------------------------------


def run_chrome_inventory(server: Server, chosen: list[str] | None) -> dict:
    """Every suite (or `chosen`) once in Chrome, recorded."""
    server.mode = "record"
    query = "startAutomatically&iterationCount=1&" + (f"suites={','.join(chosen)}" if chosen else "tags=all")
    url = server.url(f"/index.html?{query}")
    print(f"Chrome {CHROME_VERSION}: {url}")
    result = run_in_chrome(server, url, CHROME_TIMEOUT)
    if "error" in result:
        raise SystemExit(f"Chrome's recording: {result['error']}")
    return json.loads(result["body"])


# --- what is valid ----------------------------------------------------------

# Classes a page sets in an animation frame callback after a test's steps,
# which a test's summary has or not as that frame came before the runner
# ended the test or after: TipTap's `focus()` focuses the editor in one
# (ProseMirror's `ProseMirror-focused`), and Chrome sometimes ends the test
# first (the frame's callbacks of the suite's frame run after the runner's
# end), so the class comes with the next test's summary instead. The work
# is the same in the iteration, only split otherwise between its tests: not
# compared. A run that never had one where the other browser's did is noted
# (`timing_note`).
TIMING_CLASSES = {"ProseMirror-focused"}


def summary_difference(got: dict, want: dict) -> str | None:
    """`common.summary_difference`, without `TIMING_CLASSES`."""
    return common.summary_difference(got, want, TIMING_CLASSES)


def run_problems(run: dict, reference: dict | None) -> list[str]:
    """Why `run` (one suite's, as `measure.mjs` posts it) isn't valid:
    it didn't get to the end, something threw, or a test's summary differs
    from `reference`'s first iteration's (Chrome's, `None`: nothing to
    compare with). Empty when it is valid."""
    if "error" in run:
        return [run["error"]]
    problems = []
    if run["status"] != "done":
        where = f" in {run['current']}" if run.get("current") else ""
        problems.append(f"{run['status']}{where}")
    for error in run["errors"]:
        problems.append(f"{error['kind']} in {error['page']}: {first_line(error['message'])}")
        break
    if reference is None or "error" in reference or not reference["summaries"]:
        return problems
    first = reference["summaries"][0]
    for iteration, summaries in enumerate(run["summaries"]):
        for test, summary in summaries.items():
            want = first.get(test)
            difference = "Chrome has no summary" if want is None else summary_difference(summary, want)
            if difference:
                problems.append(f"iteration {iteration}, {test}: {difference}")
                return problems
    return problems


def timing_note(runs: dict[str, dict]) -> str | None:
    """A browser's run (`runs`, by browser's name) that never had one of the
    `TIMING_CLASSES` where another's did: its page did less (Chrome's,
    unfocused all along, never focused TipTap's editor). `None` if none."""
    had = {
        browser: {
            name
            for summaries in run.get("summaries", [])
            for summary in summaries.values()
            for name in summary["classes"]
            if name in TIMING_CLASSES
        }
        for browser, run in runs.items()
        if "error" not in run
    }
    for browser, names in had.items():
        for other, others in had.items():
            if missing := sorted(others - names):
                return f"{browser}'s page never had `{missing[0]}`, {other}'s did: it did less"
    return None


# --- the report -----------------------------------------------------------

def probe_list(chrome: dict) -> dict:
    """Everything Chrome saw used, for machopo's probe."""
    api, declarations, selectors, at_rules = set(), set(), set(), {}
    for record in [chrome["runner"], *(s["api"] for s in chrome["suites"].values())]:
        api.update(record)
    for suite in chrome["suites"].values():
        declarations.update(suite["declarations"])
        selectors.update(suite["selectors"])
        # A recording from before the texts were kept has none to try.
        for kind, text in suite.get("atRuleTexts", {}).items():
            at_rules.setdefault(kind, text)
    elements = {key for key in api if key.startswith("element:")}
    api -= elements
    # Only the platform's elements: a custom element's class is the page's
    # own, which the probe page doesn't define.
    elements = {key for key in elements if re.match(r"(HTML|SVG|MathML)", key.split(":")[2])}
    return {
        "api": sorted(api),
        "declarations": sorted(declarations),
        "selectors": sorted(selectors),
        "atRules": at_rules,
        "elements": sorted(elements),
    }


def write_report(chrome: dict, missing: dict, runs: dict, all_suites: list[tuple[str, bool]]) -> None:
    culprits = missing.get("culprits", {})
    lacking = {kind: set(values) for kind, values in missing.items()}
    lacking["elements"] = {entry.split(":", 1)[0] for entry in missing["elements"]}
    report = {
        "date": date.today().isoformat(),
        "machopo": git_head(),
        "chrome": CHROME_VERSION,
        "chrome_status": chrome["status"],
        "chrome_errors": chrome["errors"],
        "machopo_runs": runs,
        "missing": missing,
        "suites": {},
    }
    lines = [
        f"# Speedometer 3.1 in machopo: what the suites need ({report['date']})",
        "",
        f"machopo `{report['machopo']}`, Chrome for Testing {CHROME_VERSION} (what the suites use), "
        "`python3 scripts/bench.py speedometer --inventory`.",
        "",
        "## Each default suite in machopo",
        "",
        "| Suite | Result | Tests done | First exception |",
        "| --- | --- | --- | --- |",
    ]
    for name, default in all_suites:
        if not default or name not in runs:
            continue
        run = runs[name]
        error = run["first_error"].replace("|", "\\|")
        lines.append(f"| {name} | {run['status']} | {run['done']} | {error} |")
    lines += ["", "## What machopo lacks, by suite", ""]
    runner = sorted(k for k in chrome["runner"] if k in lacking["api"])
    if runner:
        lines += ["### The runner", "", f"- API ({len(runner)}): {', '.join(f'`{k}`' for k in runner)}", ""]
    for name, default in all_suites:
        suite = chrome["suites"].get(name)
        if not suite:
            continue
        api = {k: n for k, n in suite["api"].items() if k in lacking["api"]}
        elements = sorted(
            {k.split(":")[1] for k in suite["api"] if k.startswith("element:")}
            & {e.split(":")[0] for e in missing["elements"]}
        )
        declarations = [d for d in suite["declarations"] if d in lacking["declarations"]]
        properties: dict[str, int] = {}
        for declaration in declarations:
            prop = declaration.split(":", 1)[0]
            properties[prop] = properties.get(prop, 0) + 1
        selectors = [s for s in suite["selectors"] if s in lacking["selectors"]]
        failing = sorted({part for s in selectors for part in culprits.get(s, [s])})
        at_rules = sorted(k for k in suite["atRules"] if k in lacking.get("atRules", set()))
        report["suites"][name] = {
            "default": default,
            "api": api,
            "elements": elements,
            "declarations": declarations,
            "selectors": selectors,
            "at_rules": at_rules,
        }
        lines += [f"### {name}{'' if default else ' (not run by default)'}", ""]
        if api:
            listed = ", ".join(f"`{k}`" for k in sorted(api))
            lines += [f"- API ({len(api)}): {listed}"]
        if elements:
            lines += [f"- Elements without their interface: {', '.join(f'`{e}`' for e in elements)}"]
        if properties:
            listed = ", ".join(f"`{p}` ({n})" for p, n in sorted(properties.items(), key=lambda i: (-i[1], i[0])))
            lines += [f"- CSS declarations machopo drops ({len(declarations)}), by property: {listed}"]
        if selectors:
            lines += [f"- Selectors machopo can't parse ({len(selectors)}), for: {', '.join(f'`{p}`' for p in failing)}"]
        if at_rules:
            lines += [f"- At-rules machopo drops: {', '.join(f'`{a}`' for a in at_rules)}"]
        lines.append("")
    RESULTS.mkdir(parents=True, exist_ok=True)
    (RESULTS / "inventory.json").write_text(json.dumps(report, indent=1, sort_keys=True), encoding="utf-8")
    (RESULTS / "inventory.md").write_text("\n".join(lines), encoding="utf-8", newline="\n")


def summarize_run(result: dict) -> dict:
    """One suite's run in machopo for the inventory: its status, tests done,
    first exception."""
    data = parse_run(result)
    if "error" in data:
        return {"status": "no report", "done": 0, "first_error": first_line(data["error"]), "errors": []}
    errors = data.get("errors", [])
    first = errors[0]["message"] if errors else ""
    if data.get("current") and not first:
        first = f"stopped in {data['current']}"
    status = data["status"]
    # The runner got to the end, but something threw on the way.
    if status == "done" and errors:
        status = "done, with exceptions"
    return {
        "status": status,
        "done": len(data.get("completed", [])),
        "first_error": first_line(first),
        "errors": errors,
        "current": data.get("current"),
        "seconds": data.get("seconds"),
    }


# --- the comparison ---------------------------------------------------------

def schedule(names: list[str], browsers: list[str]) -> list[tuple[str, str]]:
    """The runs in order, `(suite, browser)`: the suites, each in every
    browser one after the other, so that the two runs of a suite are close
    in time."""
    return [(name, browser) for name in names for browser in browsers]


def suite_samples(run: dict, suite: str) -> list[float]:
    """The suite's total in each iteration of `run`."""
    return [iteration[suite]["total"] for iteration in run["iterations"]]


def test_samples(run: dict, suite: str, test: str, kind: str) -> list[float]:
    """A test's `Sync` or `Async` time in each iteration of `run`."""
    return [iteration[suite]["tests"][test]["tests"][kind] for iteration in run["iterations"]]


def suite_problem(reference: dict | None, runs: dict[str, dict]) -> str | None:
    """Why a suite has no numbers: the first problem of either browser's run
    (`runs`, by browser's name), each checked against `reference`
    (Chrome's); `None` if every run is valid."""
    for browser, run in runs.items():
        problems = run_problems(run, reference)
        if problems:
            return f"{browser}: {problems[0]}"
    return None


def compare(names: list[str], chrome: dict | None, machopo: dict | None, reference: dict | None) -> list[dict]:
    """Each suite's row: why it has no numbers (`problem`), or each
    browser's run. `chrome` and `machopo` are runs by suite (`None`: not
    measured); `reference` is Chrome's, to check machopo's against: a suite it
    lacks (Chrome's earlier run of other suites) has no numbers."""
    rows = []
    for name in names:
        if reference is not None and name not in reference:
            rows.append({"suite": name, "problem": "no run of Chrome's to check against", "runs": {}})
            continue
        measured = {browser: runs[name] for browser, runs in (("Chrome", chrome), ("machopo", machopo)) if runs}
        problem = suite_problem(reference[name] if reference else None, measured)
        note = None if problem else timing_note(measured)
        rows.append({"suite": name, "problem": problem, "runs": {} if problem else measured, "note": note})
    return rows


def write_measure_report(rows: list[dict], env: dict, iterations: int) -> Path:
    """`measure.md`: each suite's times in each browser, and each test's."""
    lines = [
        f"# Speedometer 3.1 in machopo and Chrome ({env['date']})",
        "",
        f"machopo `{env['machopo']}` (the `{env['profile']}` profile: ThinLTO, no PGO), Chrome for Testing {env['chrome']} "
        f"(ThinLTO and PGO); {env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')}), {env['cores']} cores, power: {env['power']}, "
        f"machopo's GPU: {env.get('gpu', 'unknown')}. "
        f"Each suite in a new browser, {iterations} iterations, the browsers one after the other. "
        "Median ms of the suite's total in each iteration (95% interval); the first iteration's alone.",
        "",
        "| Suite | machopo | Chrome | machopo / Chrome | First iteration: machopo, Chrome |",
        "| --- | --- | --- | --- | --- |",
    ]
    for row in rows:
        if row["problem"]:
            problem = row["problem"].replace("|", "\\|")
            lines.append(f"| {row['suite']} | not valid: {problem} | | | |")
            continue
        runs = row["runs"]
        cells = {
            browser: interval_text(suite_samples(runs[browser], row["suite"])) if browser in runs else "not run"
            for browser in ("machopo", "Chrome")
        }
        ratio = firsts = ""
        if len(runs) == 2:
            ratio = "{:.2f}".format(
                median(suite_samples(runs["machopo"], row["suite"])) / median(suite_samples(runs["Chrome"], row["suite"]))
            )
        firsts = ", ".join(
            f"{runs[b]['iterations'][0][row['suite']]['total']:.1f}" if b in runs else "not run"
            for b in ("machopo", "Chrome")
        )
        lines.append(f"| {row['suite']} | {cells['machopo']} | {cells['Chrome']} | {ratio} | {firsts} |")
    notes = [f"- {row['suite']}: {row['note']}" for row in rows if row.get("note")]
    if notes:
        lines += ["", *notes]
    lines += [
        "",
        "## Each test",
        "",
        "Median ms of its sync time (the test's steps) and async time (until the next frame is done).",
        "",
        "| Suite | Test | machopo sync | machopo async | Chrome sync | Chrome async |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for row in rows:
        if row["problem"]:
            continue
        runs = row["runs"]
        first = next(iter(runs.values()))["iterations"][0][row["suite"]]["tests"]
        for test in first:
            cells = [
                f"{median(test_samples(runs[b], row['suite'], test, kind)):.1f}" if b in runs else "not run"
                for b in ("machopo", "Chrome")
                for kind in ("Sync", "Async")
            ]
            lines.append(f"| {row['suite']} | {test} | {' | '.join(cells)} |")
    RESULTS.mkdir(parents=True, exist_ok=True)
    path = RESULTS / "measure.md"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return path


# --- traces and samples -----------------------------------------------------


# The phases `phases.md` sums each browser's slices into, in its order.
PHASES = ("script", "style", "box tree", "layout", "paint", "parse", "GC", "canvas")

# Each phase's slices, by name, in machopo's trace and in Chrome's, whose
# names differ: machopo's spans are named as Chrome's trace events where they
# do the same work, but Chrome shapes text in layout and machopo in building
# the box tree (`StyleEngine::RebuildLayoutTree`).
SCRIPT_SLICES = {"v8.callFunction", "v8.callAsConstructor", "FunctionCall", "RunMicrotasks", "EventDispatch",
                 "TimerFire", "FireAnimationFrame", "CustomElementReactionQueue::invokeReactions"}
PHASE_SLICES = {
    "machopo": {
        "style": {"Document::UpdateStyle"},
        "box tree": {"StyleEngine::RebuildLayoutTree"},
        "layout": {"Layout"},
        "paint": {"Paint", "PrePaint", "PaintArtifactCompositor::Update", "LayerTreeHost::DoUpdateLayers",
                  "paint::main_thread_scroll_hit_test_region"},
        "canvas": {"raster::draw_into", "canvas::flush_shown"},
    },
    "Chrome": {
        # `UpdateLayoutTree` is DevTools' around both.
        "style": {"Document::recalcStyle", "Document::updateStyle", "UpdateLayoutTree"},
        "box tree": {"Document::rebuildLayoutTree"},
        "layout": {"Layout", "LocalFrameView::performLayout"},
        "paint": {"Paint", "PrePaint", "PaintArtifactCompositor::Update", "LayerTreeHost::DoUpdateLayers",
                  "Blink.PrePaint.UpdateTime", "Blink.Paint.UpdateTime", "Blink.CompositingInputs.UpdateTime",
                  "LocalFrameView::RunPaintLifecyclePhase"},
        "canvas": set(),
    },
}


def slice_phase(browser: str, name: str) -> str | None:
    """The phase of `browser`'s slice `name` (`PHASES`), if any."""
    if name in SCRIPT_SLICES:
        return "script"
    if name.startswith("V8."):
        return "GC" if "GC" in name or "SCAVENGE" in name.upper() else "script"
    if name.startswith("CppGC.") or name.startswith("Gc."):
        # The DOM's heap (Oilpan in Chrome, machopo's `gc` in machopo), inside V8's
        # GC events or on their own (a scavenge of its young objects).
        return "GC"
    if name == "ParseHTML" or name.startswith("HTMLDocumentParser::"):
        return "parse"
    for phase, names in PHASE_SLICES[browser].items():
        if name in names:
            return phase
    if browser == "Chrome" and (name.startswith("InlineNode::") or name.startswith("Canvas")):
        return "layout" if name.startswith("InlineNode::") else "canvas"
    return None


def phase_times(path: Path, browser: str) -> tuple[collections.Counter, collections.Counter]:
    """The self time (ns) of each phase (`slice_phase`) in the windows the
    runner times, on the renderer's main thread of `browser`'s trace (the
    track with the runner's marks: Chrome has a `CrRendererMain` in each
    renderer process), and of the slices no phase takes, by name. Each
    stretch between two of the thread's events goes to the innermost slice
    open in it."""
    _, events = perfetto_track_events(path)
    marks: dict[int, list[tuple[int, str]]] = collections.defaultdict(list)
    slices: dict[int, list[tuple[int, int, int, str]]] = collections.defaultdict(list)
    for track, time, order, kind, name, categories in events:
        if kind == 3 and USER_TIMING in categories:
            marks[track].append((time, name))
        elif kind in (1, 2):
            slices[track].append((time, order, kind, name))
    phases, others = collections.Counter(), collections.Counter()
    if not marks:
        return phases, others
    track = max(marks, key=lambda t: len(marks[t]))
    windows = mark_windows(marks[track])
    stack: list[str] = []
    last = None
    first = 0  # The first window that may still hold what comes.
    for time, _, kind, name in sorted(slices[track], key=lambda event: (event[0], event[1])):
        if last is not None and stack:
            for start, end in windows[first:]:
                if start >= time:
                    break
                overlap = min(end, time) - max(start, last)
                if overlap > 0:
                    phase = slice_phase(browser, stack[-1])
                    if phase:
                        phases[phase] += overlap
                    else:
                        others[stack[-1]] += overlap
            while first < len(windows) and windows[first][1] <= time:
                first += 1
        if kind == 1:
            stack.append(name)
        elif stack:
            stack.pop()
        last = time
    return phases, others


def write_phases_report(times: dict[str, dict[str, tuple[collections.Counter, collections.Counter]]],
                        env: dict, iterations: int) -> Path:
    """`phases.md` (and `phases.json`): each phase's self time in the timed
    windows, ms an iteration, in each suite of `times` (by browser: its
    phases' and other slices' ns) and their totals, machopo's and Chrome's,
    with what machopo takes more; then the slices no phase takes, most first."""
    browsers = [b for b in ("machopo", "Chrome") if any(b in by_browser for by_browser in times.values())]
    ms = {suite: {b: {phase: ns / 1e6 / iterations for phase, ns in phases.items()}
                  for b, (phases, _) in by_browser.items()} for suite, by_browser in times.items()}
    totals = {b: collections.Counter() for b in browsers}
    for by_browser in ms.values():
        for b, phases in by_browser.items():
            totals[b].update(phases)
    both = len(browsers) == 2

    def cell(suite_ms: dict, phase: str) -> str:
        values = [suite_ms.get(b, {}).get(phase, 0.0) for b in browsers]
        if both:
            return f"{values[0]:.1f} / {values[1]:.1f} ({values[0] - values[1]:+.1f})"
        return f"{values[0]:.1f}"

    lines = [
        f"# Speedometer 3.1's phases in {' and '.join(browsers)} ({env['date']})",
        "",
        f"machopo `{env['machopo']}` (the `{env['profile']}` profile), Chrome for Testing {env['chrome']}; "
        f"{env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')}), power: {env['power']}. From the traces of `--trace` ({iterations} iterations): "
        "each phase's self time (the stretches in which one of its slices is the innermost open on the renderer's "
        "main thread) in the windows the runner times, ms an iteration"
        + (", machopo / Chrome (what machopo takes more). " if both else ". ")
        + "machopo shapes text in building the box tree, Chrome in layout.",
        "",
        "| Phase | " + " | ".join(browsers) + (" | machopo − Chrome |" if both else " |"),
        "| --- | " + " | ".join("---:" for _ in browsers) + (" | ---: |" if both else " |"),
    ]
    for phase in PHASES:
        values = [totals[b][phase] for b in browsers]
        lines.append(f"| {phase} | " + " | ".join(f"{v:.1f}" for v in values)
                     + (f" | {values[0] - values[1]:+.1f} |" if both else " |"))
    lines += ["", "| Suite | " + " | ".join(PHASES) + " |", "| --- | " + " | ".join("---:" for _ in PHASES) + " |"]
    for suite, suite_ms in times.items():
        lines.append(f"| {suite} | " + " | ".join(cell(ms[suite], phase) for phase in PHASES) + " |")
    for b in browsers:
        others = collections.Counter()
        for by_browser in times.values():
            if b in by_browser:
                others.update(by_browser[b][1])
        lines += ["", f"The slices of {b}'s that no phase takes, most first (ms an iteration):", ""]
        lines += [f"- `{name}`: {ns / 1e6 / iterations:.1f}" for name, ns in others.most_common(10)] or ["- none"]
    report = RESULTS / "phases.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (RESULTS / "phases.json").write_text(json.dumps(ms, indent=1) + "\n", encoding="utf-8", newline="\n")
    return report


def timed_windows(slices: list[tuple[int, str | None]], marks: list[tuple[int, str]]) -> tuple[list[tuple[int, int]], str]:
    """The windows Speedometer times, one a test, on the renderer's main
    thread, and what found them: by the runner's `marks` (`mark_windows`),
    or, in a trace with none (machopo before it wrote User Timing), by the
    runner's frames in its `slices` (`frame_windows`)."""
    windows = mark_windows(marks)
    if windows:
        return windows, "the runner's marks"
    return frame_windows(slices), "the runner's frames"


def mark_windows(marks: list[tuple[int, str]]) -> list[tuple[int, int]]:
    """The windows the runner times, by the marks it sets around each test
    (`_runTestAndRecordResults`): from `<suite>.<test>-start`, before the
    test runs, to the next `<suite>.<test>-async-end`, as its async time
    ends; the same span as the sync and async times it reports. The
    runner's other marks (a suite's, an iteration's) have no
    `-async-end`."""
    starts: dict[str, int] = {}
    windows = []
    for time, name in sorted(marks):
        if name.endswith("-async-end"):
            start = starts.pop(name.removesuffix("-async-end"), None)
            if start is not None:
                windows.append((start, time))
        elif name.endswith("-start"):
            starts[name.removesuffix("-start")] = time
    return windows


def frame_windows(slices: list[tuple[int, str | None]]) -> list[tuple[int, int]]:
    """The windows Speedometer times, on the renderer's main thread's
    `slices`, one a test, as close as its frames tell. Off where the page
    has a timer before the runner's (the window ends early) or asks for two
    callbacks in a frame itself (a window too many). The runner asks for two animation frame callbacks
    at once, so they run in the same main frame: the test runs in the first
    (its sync time), and the second sets a timer whose callback ends its
    async time, which runs once the frame is done. So a window is a main
    frame (`ProxyMain::BeginMainFrame`) with two or more callbacks
    (`FireAnimationFrame`), from the start of its first, to the end of the
    first timer (`TimerFire`) that starts after the frame ends, if no other
    frame starts before that timer. A frame with one callback (the page's
    own) and the time between frames aren't timed."""
    open_slices: list[list] = []
    frames, timers = [], []
    for time, name in slices:
        if name is not None:
            open_slices.append([name, time, None, 0])
            if name == "FireAnimationFrame":
                frame = next((s for s in reversed(open_slices[:-1]) if s[0] == "ProxyMain::BeginMainFrame"), None)
                if frame is not None:
                    frame[2] = time if frame[2] is None else frame[2]
                    frame[3] += 1
            continue
        name, start, first_callback, callbacks = open_slices.pop()
        if name == "ProxyMain::BeginMainFrame":
            frames.append((start, time, first_callback if callbacks >= 2 else None))
        elif name == "TimerFire":
            timers.append((start, time))
    frames.sort()
    timers.sort()
    timer_starts = [start for start, _ in timers]
    frame_starts = [start for start, _, _ in frames]
    windows: list[tuple[int, int]] = []
    for start, end, first_callback in frames:
        if first_callback is None:
            continue
        i = bisect.bisect_left(timer_starts, end)
        if i == len(timers):
            continue
        j = bisect.bisect_right(frame_starts, start)
        if j < len(frames) and frame_starts[j] < timer_starts[i]:
            continue
        windows.append((first_callback, timers[i][1]))
    return windows


PERF_SAMPLE = re.compile(
    r"^(?P<comm>\S.*?)\s+(?P<pid>\d+)/(?P<tid>\d+)\s+(?P<s>\d+)\.(?P<ns>\d{9}):\s*(?:(?P<event>[\w-]+)(?::\w+)?:\s*)?$"
)

# Each event's samples: a sample's time, its stack (leaf first) and, if
# it doesn't count one, how much it counts.
Samples = dict[str, list[tuple]]


def read_samples(samples: Path, thread: str) -> tuple[Samples, set[int]]:
    """The samples of `thread` in `samples`, as this platform's sampler
    took them (`perf_samples`, `samply_samples`), and the processes they
    were taken in."""
    return samply_samples(samples, thread) if sys.platform in ("darwin", "win32") else perf_samples(samples, thread)


def perf_samples(samples: Path, thread: str, tid: int | None = None,
                 monotonic: bool = False) -> tuple[Samples, set[int]]:
    """The samples of the thread named `thread` in `samples` (or whose tid
    is `tid`), by event (`cycles`, `instructions`): each one's time (ns on
    CLOCK_BOOTTIME, as machopo's trace's: CLOCK_MONOTONIC's plus the time
    suspended, which a run doesn't change; with `monotonic`, on
    CLOCK_MONOTONIC as `perf -k monotonic` takes them, as Chrome's trace's
    `base::TimeTicks` are on Linux) and stack, its leaf first; and the
    processes they were taken in."""
    suspended = 0 if monotonic else (time.clock_gettime_ns(time.CLOCK_BOOTTIME)
                                     - time.clock_gettime_ns(time.CLOCK_MONOTONIC))
    script = subprocess.run(
        ["perf", "script", "-i", str(samples), "-F", "comm,pid,tid,time,event,ip,sym", "--ns", "--no-inline"],
        capture_output=True, text=True, errors="replace",
    )
    return read_perf_script(script.stdout, thread, suspended, tid)


def read_perf_script(text: str, thread: str, offset: int, tid: int | None = None) -> tuple[Samples, set[int]]:
    """`perf_samples` of `perf script`'s `text` (each sample a line with its
    thread's name, `pid/tid`, time and event, then a line a frame; a
    sample with no event is a cycle's), each time moved by `offset` ns: the
    thread named `thread`, or with a `tid`, the thread whose tid it is."""
    taken: Samples = collections.defaultdict(list)
    pids: set[int] = set()
    stack: list[str] | None = None
    for line in text.splitlines() + [""]:
        if not line.strip():
            stack = None
            continue
        if stack is not None:
            stack.append(line.strip().split(None, 1)[-1] if " " in line.strip() else "[unknown]")
            continue
        sample = PERF_SAMPLE.match(line)
        if sample and (int(sample["tid"]) == tid if tid is not None else sample["comm"] == thread):
            stack = []
            pids.add(int(sample["pid"]))
            event = sample["event"] or "cycles"
            taken[event].append((int(sample["s"]) * 1_000_000_000 + int(sample["ns"]) + offset, stack))
        elif sample:
            stack = []  # another thread's, not kept
    return dict(taken), pids


def samply_samples(samples: Path, thread: str) -> tuple[Samples, set[int]]:
    """`read_samply_profile` of samply's profile `samples` and its symbols,
    each time on the trace's clock. On macOS that is CLOCK_UPTIME_RAW
    (Perfetto's there: `mach_absolute_time`): samply's times are the wall
    clock's, moved by the two clocks' difference now, which a run doesn't
    change (`caffeinate` keeps the Mac awake: asleep, the uptime stops). On
    Windows it is QueryPerformanceCounter, as ETW's; samply's times count
    from its kernel ETL's start (`etl_start_ns`) but are given from its
    `startTime`, when samply itself started, some hundreds of ms before or
    after, a different time each run. On Windows too, the frames samply
    left nameless in Windows's own DLLs are named after (`windows_names`)."""
    profile = json.loads(gzip.decompress(samples.read_bytes()))
    if sys.platform == "win32":
        offset = round(profile["meta"]["startTime"] * 1_000_000) - etl_start_ns(samply_etls(samples)[0])
    else:
        offset = time.time_ns() - time.clock_gettime_ns(time.CLOCK_UPTIME_RAW)
    symbols_file = samply_symbols(samples)
    symbols = json.loads(symbols_file.read_text(encoding="utf-8")) if symbols_file.exists() else {}
    found = windows_names(samples, profile, nameless_frames(profile, symbols, thread)) if sys.platform == "win32" \
        else {}
    return read_samply_profile(profile, symbols, thread, offset, found)


def nameless_frames(profile: dict, symbols: dict, thread: str) -> dict[int, set[int]]:
    """The addresses of the frames of the threads named `thread` in samply's
    `profile` that it has no name for (`frame_namer`: the ones it made up
    from the unwind table, `fun_<address>`, or only the library's), by
    library (its index); not JIT frames, whose library has no code id."""
    name_of = frame_namer(profile, symbols, {})
    nameless: dict[int, set[int]] = collections.defaultdict(set)
    for record in profile["threads"]:
        if record["name"] != thread:
            continue
        frames, funcs, resources = record["frameTable"], record["funcTable"], record["resourceTable"]
        for i, func in enumerate(frames["func"]):
            resource = funcs["resource"][func]
            if resource is None or resource < 0:
                continue
            library = resources["lib"][resource]
            name = name_of(record, i)
            if profile["libs"][library].get("codeId") and (name.startswith("fun_") or name.startswith("[")):
                nameless[library].add(frames["address"][i])
    return dict(nameless)


def windows_names(samples: Path, profile: dict, nameless: dict[int, set[int]]) -> dict[tuple[str, int], str]:
    """Names for the `nameless` frames of samply's `profile` (`samples`), by
    library code id and address. On Windows samply names a DLL's frames by
    the DLL itself when it is on disk (its exports; the rest, `fun_<address>`
    from its unwind table) and doesn't look its PDB up on a symbol server
    then: Windows's own (ntdll's heap, vcruntime140's `memcpy`, the kernel's)
    are found by `samply load` of a copy of the profile without those DLLs'
    paths, through Microsoft's symbol server (`symbolicate`). None if that
    fails: the frames keep what they had."""
    if not nameless:
        return {}
    hidden = samples.parent / "nowhere"
    libraries = [dict(library) for library in profile["libs"]]
    for index in nameless:
        libraries[index]["path"] = str(hidden / libraries[index]["name"])
        libraries[index]["debugPath"] = str(hidden / libraries[index]["debugName"])
    copy = samples.with_name(samples.name.removesuffix(".json.gz") + ".nameless.json")
    copy.write_text(json.dumps({**profile, "libs": libraries}), encoding="utf-8", newline="\n")
    order = sorted(nameless)
    frames = [(k, address) for k, index in enumerate(order) for address in sorted(nameless[index])]
    try:
        named = symbolicate(copy, [(libraries[index]["debugName"], libraries[index]["breakpadId"]) for index in order],
                            frames)
    except (OSError, ValueError, RuntimeError) as e:
        print(f"  Windows's DLLs not named: {e}")
        return {}
    finally:
        copy.unlink(missing_ok=True)
    return {(profile["libs"][order[k]]["codeId"], address): name
            for (k, address), name in zip(frames, named) if name}


# How long `samply load` may take to say where it serves, and to look the
# symbols up (downloading a PDB the first time).
SAMPLY_LOAD_START = 60
SAMPLY_LOAD_SYMBOLS = 300


def symbolicate(profile: Path, modules: list[tuple[str, str]], frames: list[tuple[int, int]]) -> list[str | None]:
    """The function at each of `frames` (a module of `modules`, each its
    debug name and id, and an address in it), as `samply load` of
    `profile` finds it with Microsoft's symbol server (the Firefox
    profiler's symbolication API, `/symbolicate/v5`, which the server
    `samply load` starts answers); `None` where it finds none."""
    load = subprocess.Popen(["samply", "load", "--no-open", "--windows-symbol-server", WINDOWS_SYMBOL_SERVER,
                             str(profile)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                            encoding="utf-8", errors="replace")
    lines: queue.Queue = queue.Queue()
    threading.Thread(target=lambda: [lines.put(line) for line in load.stdout], daemon=True).start()
    try:
        deadline = time.monotonic() + SAMPLY_LOAD_START
        server = None
        while server is None:
            try:
                line = lines.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                raise RuntimeError(f"samply load didn't start in {SAMPLY_LOAD_START} s") from None
            found = re.search(r"symbolServer=(http://[\w.]+:\d+/\w+)", urllib.parse.unquote(line))
            server = found[1] if found else None
        body = json.dumps({"memoryMap": [list(module) for module in modules],
                           "stacks": [[list(frame) for frame in frames]]}).encode()
        request = urllib.request.Request(f"{server}/symbolicate/v5", body, {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=SAMPLY_LOAD_SYMBOLS) as response:
            found_frames = json.load(response)["results"][0]["stacks"][0]
        return [frame.get("function") for frame in found_frames]
    finally:
        load.kill()
        load.wait()
        load.stdout.close()


# The kernel ETL's first event, after the 72-byte header of its buffer
# (WMI_BUFFER_HEADER), is the trace's header: a SYSTEM_TRACE_HEADER with
# its time (QPC's ticks) at byte 16, then the TRACE_LOGFILE_HEADER, whose
# PerfFreq follows its BootTime, after the logger's and the file's names
# (two pointers) and the TIME_ZONE_INFORMATION, aligned to 8.
ETL_START = 72 + 16
ETL_BOOT_TIME = (72 + 32 + 4 * 4 + 8 + 4 * 8 + 2 * 8 + 172 + 7) // 8 * 8
ETL_PERF_FREQ = ETL_BOOT_TIME + 8


def etl_start_ns(etl: Path) -> int:
    """When the ETL file `etl` (x64's) started, as QueryPerformanceCounter's
    ns: samply counts its samples' times from its kernel ETL's."""
    with open(etl, "rb") as file:
        header = file.read(ETL_PERF_FREQ + 8)
    start, frequency = (struct.unpack_from("<q", header, at)[0] for at in (ETL_START, ETL_PERF_FREQ))
    return start * 1_000_000_000 // frequency


def frame_namer(profile: dict, symbols: dict, found: dict[tuple[str, int], str]):
    """What names a frame (`name_of(thread's record, frame's index)`) of
    samply's `profile`: `found` (by its library's code id and its address),
    first; else `symbols` (samply's `--unstable-presymbolicate`: by each
    library's code id, its addresses' symbols); else the name the profile
    gives it (a JIT frame's, from V8's perf map, which samply reads as a
    library of its own), unless that is only its address: `[library]`."""
    libraries = {library["code_id"]: library for library in symbols.get("data", [])}
    known = {code_id: dict(map(tuple, library["known_addresses"])) for code_id, library in libraries.items()}
    names = symbols.get("string_table", [])

    def name_of(record: dict, i: int) -> str:
        frames, funcs, resources = record["frameTable"], record["funcTable"], record["resourceTable"]
        func = frames["func"][i]
        resource = funcs["resource"][func]
        given = record["stringArray"][funcs["name"][func]]
        if resource is None or resource < 0:
            return given
        library = profile["libs"][resources["lib"][resource]]
        code_id = library.get("codeId")
        named = found.get((code_id, frames["address"][i]))
        if named:
            return named
        entry = known.get(code_id, {}).get(frames["address"][i])
        if entry is not None:
            return names[libraries[code_id]["symbol_table"][entry]["symbol"]]
        return given if not given.startswith("0x") else f"[{library['debugName']}]"

    return name_of


def read_samply_profile(profile: dict, symbols: dict, thread: str, offset: int,
                        found: dict[tuple[str, int], str] | None = None) -> tuple[Samples, set[int]]:
    """The samples of the threads named `thread` in samply's `profile` (the
    Firefox profiler's format), as `cpu`'s: each one's time (ns of the wall
    clock less `offset`), stack (leaf first, each frame named by
    `frame_namer` with `symbols` and `found`) and the µs of CPU time the
    thread used since the one before (none: waiting, not kept); and their
    processes."""
    name_of = frame_namer(profile, symbols, found or {})
    start = profile["meta"]["startTime"]
    taken: Samples = collections.defaultdict(list)
    pids: set[int] = set()
    for record in profile["threads"]:
        if record["name"] != thread:
            continue
        pids.add(int(record["pid"]))
        stacks: dict[int, list[str]] = {}

        def stack_of(i: int) -> list[str]:
            chain = []
            while i is not None and i not in stacks:
                chain.append(i)
                i = record["stackTable"]["prefix"][i]
            tail = stacks[i] if i is not None else []
            for j in reversed(chain):
                tail = [name_of(record, record["stackTable"]["frame"][j]), *tail]
                stacks[j] = tail
            return tail

        table = record["samples"]
        cpu = table.get("threadCPUDelta") or [None] * len(table["time"])
        for at, stack, used in zip(table["time"], table["stack"], cpu):
            if stack is not None and used:
                taken["cpu"].append((round((start + at) * 1_000_000) - offset, stack_of(stack), used))
    return dict(taken), pids


def fold(samples: list[tuple], windows: list[tuple[int, int]]) -> collections.Counter:
    """The stacks of the `samples` taken in `windows`, folded (root first,
    `;` between frames, as flamegraph.pl and speedscope read them) and
    counted (by a sample's weight, if it has one)."""
    starts = [start for start, _ in windows]
    folded: collections.Counter = collections.Counter()
    for time, stack, *weight in samples:
        i = bisect.bisect_right(starts, time) - 1
        if i >= 0 and time < windows[i][1] and stack:
            folded[";".join(frame.replace(";", ":") for frame in reversed(stack))] += weight[0] if weight else 1
    return folded


# How many of the functions most samples are in (or under) `sample.md` lists.
TOP_FUNCTIONS = 25

Folded = dict[str, collections.Counter]


def folded_path(folder: Path, name: str, event: str) -> Path:
    """Where `--sample` writes suite `name`'s stacks of `event`: the time's
    (`cycles`, macOS's `cpu`) in `<suite>-machopo.folded`, another event's in
    `<suite>-machopo.<event>.folded`."""
    return folder / (f"{name}-machopo.folded" if event in TIME_EVENTS else f"{name}-machopo.{event}.folded")


# The events that count the time it takes, Linux's and macOS's; and what a
# report calls each event's samples.
TIME_EVENTS = ("cycles", "cpu")
EVENT_SAMPLES = {"cycles": "cycle samples", "instructions": "instruction samples", "cpu": "µs of CPU time"}


def time_event(folded: Folded) -> str:
    """Which of `folded`'s events counts the time: `cycles`, or macOS's
    `cpu`."""
    return next((event for event in TIME_EVENTS if event in folded), "cycles")


def self_counts(folded: collections.Counter) -> collections.Counter:
    """The samples each function is the leaf of."""
    leaves: collections.Counter = collections.Counter()
    for stack, count in folded.items():
        leaves[stack.split(";")[-1]] += count
    return leaves


def totals_text(folded: Folded) -> str:
    """How many samples of each event, and the instructions per cycle; or,
    of macOS's, the CPU time."""
    if time_event(folded) == "cpu":
        return f"{sum(folded['cpu'].values()) / 1000:.1f} ms of CPU time."
    cycles = sum(folded.get("cycles", {}).values())
    instructions = sum(folded.get("instructions", {}).values())
    ipc = f" (IPC {instructions / cycles:.2f})" if cycles and instructions else ""
    return f"{cycles} samples of {SAMPLE_PERIOD:,} cycles, {instructions} of as many instructions{ipc}."


def write_sample_report(rows: list[tuple[str, Folded, str]]) -> Path:
    """`sample.md`: for each suite, its samples of each event (and what
    found its timed windows, `timed_windows`), and the
    functions most of its time in its timed windows is in (self) and
    under (inclusive; those under which nearly every sample is, the
    thread's and the run loop's, left out), and most instruction samples
    are in (Linux)."""
    way = sampling() or LINUX_SAMPLING
    lines = [
        "# machopo's samples in Speedometer's timed windows",
        "",
        f"{date.today().isoformat()}, machopo {git_head()} (the `perf` profile, {way.unwound}), "
        f"{way.described}, with V8's `{way.v8}`; the renderer's main thread only.",
    ]
    for name, by_event, found_by in rows:
        folded = by_event.get(time_event(by_event), collections.Counter())
        total = sum(folded.values())
        lines += ["", f"## {name}", "", f"{totals_text(by_event)} The windows by {found_by}."]
        if not total:
            continue
        under: collections.Counter = collections.Counter()
        for stack, count in folded.items():
            for frame in set(stack.split(";")):
                under[frame] += count
        instructions = by_event.get("instructions", collections.Counter())
        tables = [("Self", self_counts(folded), total), ("Inclusive", under, total)]
        if instructions:
            tables.append(("Self, instructions", self_counts(instructions), sum(instructions.values())))
        for title, counter, of in tables:
            lines += ["", f"| {title} | % | function |", "|---:|---:|---|"]
            shown = [(f, c) for f, c in counter.most_common() if title != "Inclusive" or c < 0.95 * of]
            for function, count in shown[:TOP_FUNCTIONS]:
                lines.append(f"| {count} | {100 * count / of:.1f} | `{function[:200].replace('|', '/')}` |")
    report = RESULTS / "sample.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


# --- comparing two versions of machopo ------------------------------------------

# How many functions `compare.md` lists, each way.
COMPARED_FUNCTIONS = 12


def count_text(value: float) -> str:
    """A count as it is, however big (macOS's µs of CPU time run to
    millions, which `:g` would write in exponents)."""
    return f"{value:.1f}".removesuffix(".0")


def spread(values: list[int]) -> str:
    """A run's values: their median and range."""
    if not values:
        return "–"
    return f"{count_text(median(values))} ({min(values)}–{max(values)})"


def compare_suite(name: str, runs: dict[str, list[Folded]]) -> list[str]:
    """`compare.md`'s part for suite `name`: for each event, each side's
    runs (`against`, `current`) and their median, range and the medians'
    change; then the functions whose median instruction samples (self;
    on macOS, CPU time) changed most each way: instructions are the
    steadier, the same work however the CPU runs it."""
    lines = ["", f"## {name}", "", "| | against | current | change |", "|---|---:|---:|---:|"]
    seen = {event for side in runs.values() for run in side for event in run}
    timed = next((event for event in TIME_EVENTS if event in seen), "cycles")
    steady = "instructions" if "instructions" in seen else timed
    for event in (event for event in (*SAMPLE_EVENTS, "cpu") if event in seen):
        against, current = ([sum(run.get(event, {}).values()) for run in runs[side]] for side in ("against", "current"))
        if not against or not current:
            continue
        lines.append(f"| {event} | {spread(against)} | {spread(current)} | {change(median(against), median(current))} |")
    lines += ["", "Each run, in the order they ran: " + "; ".join(
        f"{side} {', '.join(str(sum(run.get(timed, {}).values())) for run in runs[side])}"
        for side in ("against", "current")) + f" ({timed})."]
    selfs = {side: [self_counts(run.get(steady, collections.Counter())) for run in runs[side]]
             for side in ("against", "current")}
    if not all(selfs.values()):
        return lines
    functions = set().union(*(counter for side in selfs.values() for counter in side))
    medians = {f: tuple(median([counter[f] for counter in selfs[side]]) for side in ("against", "current"))
               for f in functions}
    moved = sorted(medians, key=lambda f: medians[f][1] - medians[f][0])
    for title, chosen in (("Fewer", moved[:COMPARED_FUNCTIONS]), ("More", moved[::-1][:COMPARED_FUNCTIONS])):
        chosen = [f for f in chosen if (medians[f][1] - medians[f][0]) * (1 if title == "More" else -1) > 0]
        if not chosen:
            continue
        lines += ["", f"{title} {EVENT_SAMPLES[steady]} (self, the runs' medians):", "",
                  "| against | current | function |", "|---:|---:|---|"]
        for function in chosen:
            before, after = medians[function]
            lines.append(f"| {count_text(before)} | {count_text(after)} | `{function[:200].replace('|', '/')}` |")
    return lines


# What `peak_memory` counts on each platform, for the reports.
MEMORY_WORDS = {"darwin": "the most phys footprint", "linux": "the peak resident set, VmHWM",
                "win32": "the peak private bytes"}


def time_problem(sides: dict[str, list[tuple[dict, int | None]]]) -> str | None:
    """Why a suite has no times to compare: the first problem of a run of
    either version (`run_problems`), or a version with no run."""
    for side, timed in sides.items():
        if not timed:
            return f"{side}: not run"
        for run, _ in timed:
            problems = run_problems(run, None)
            if problems:
                return f"{side}: {problems[0]}"
    return None


def write_time_report(rev: str, runs: int, iterations: int,
                      by_suite: dict[str, dict[str, list[tuple[dict, int | None]]]], env: dict) -> Path:
    """`compare.md` of times: for each suite, each version's total in every
    iteration of its runs, their median and its 95% interval, and the
    medians' change; machopo's peak memory, the median of each version's runs,
    and its change; the geometric mean of the suites' changes; and each
    run's median, in the order they ran, for the drift."""
    lines = [
        "# machopo against another version, in Speedometer's times",
        "",
        f"{date.today().isoformat()}: {versions_text(rev)}; {runs} runs each, taking turns, {iterations} iterations "
        f"a run; {env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')}), power: {env['power']}, machopo's GPU: {env.get('gpu', 'unknown')}. Each suite's total in every iteration of a "
        "version's runs: their median in ms (95% interval); machopo's peak memory "
        f"({MEMORY_WORDS.get(sys.platform, 'not read here')}), the median of its runs.",
        "",
        "| Suite | against | current | change | memory, against | memory, current | change |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    ratios, memory_changes, each_run = [], [], []
    for name, sides in by_suite.items():
        problem = time_problem(sides)
        if problem:
            lines.append(f"| {name} | not valid: {problem} | | | | | |")
            continue
        totals = {side: [total for run, _ in timed for total in suite_samples(run, name)]
                  for side, timed in sides.items()}
        before, after = median(totals["against"]), median(totals["current"])
        ratios.append(after / before)
        memory = {side: [peak for _, peak in timed if peak] for side, timed in sides.items()}
        cells = [f"{median(memory[side]) / 2**20:.0f} MB" if memory[side] else "–" for side in ("against", "current")]
        moved = "–"
        if memory["against"] and memory["current"]:
            memory_changes.append(median(memory["current"]) / median(memory["against"]))
            moved = change(median(memory["against"]), median(memory["current"]))
        lines.append(f"| {name} | {interval_text(totals['against'])} | {interval_text(totals['current'])} | "
                     f"{change(before, after)} | {cells[0]} | {cells[1]} | {moved} |")
        each_run.append(f"| {name} | " + " | ".join(
            ", ".join(f"{median(suite_samples(run, name)):.1f}" for run, _ in sides[side])
            for side in ("against", "current")) + " |")
    if ratios:
        mean = math.exp(sum(math.log(r) for r in ratios) / len(ratios))
        lines += ["", f"The geometric mean of the {len(ratios)} valid suites' current / against: {mean:.3f} "
                      f"({100 * (mean - 1):+.1f}%)."]
    if memory_changes:
        lines.append(f"Peak memory: {100 * (min(memory_changes) - 1):+.1f}% to {100 * (max(memory_changes) - 1):+.1f}% "
                     "by suite.")
    lines += ["", "## Each run", "", "Each run's median, in the order they ran.", "",
              "| Suite | against | current |", "| --- | --- | --- |", *each_run]
    report = RESULTS / "compare.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


def write_compare_report(rev: str, runs: int, iterations: int, by_suite: dict[str, dict[str, list[Folded]]],
                         found_by: dict[str, set[str]]) -> Path:
    """`compare.md`: machopo at `rev` against the working tree's, suite by
    suite (`compare_suite`); what found each side's timed windows
    (`found_by`, by side): each trace by its own, so a version before machopo
    wrote User Timing is by the runner's frames, as close as they tell."""
    lines = [
        "# machopo against another version, in Speedometer's timed windows",
        "",
        f"{date.today().isoformat()}: {versions_text(rev)}; {runs} runs each, taking turns, {iterations} "
        f"iterations a run. {(sampling() or LINUX_SAMPLING).described}, "
        "the renderer's main thread only; a median and its range (lowest–highest).",
        "",
        "The timed windows: " + "; ".join(
            f"{side}'s by {' and '.join(sorted(found_by.get(side, set()))) or 'nothing'}"
            for side in ("against", "current")) + ".",
    ]
    for name, sides in by_suite.items():
        lines += compare_suite(name, sides)
    report = RESULTS / "compare.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


# --- the commands -----------------------------------------------------------

def cmd_inventory(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    server = Server()
    log_path = RESULTS / "machopo.log"
    RESULTS.mkdir(parents=True, exist_ok=True)
    try:
        cached = RESULTS / "chrome.json"
        if args.reuse_chrome and cached.exists():
            chrome = json.loads(cached.read_text(encoding="utf-8"))
        else:
            started = time.monotonic()
            chrome = run_chrome_inventory(server, chosen)
            print(f"Chrome: {chrome['status']}, {len(chrome['suites'])} suites, {time.monotonic() - started:.0f} s")
            cached.write_text(json.dumps(chrome), encoding="utf-8")
        for error in chrome["errors"]:
            print(f"  Chrome: {error['page']}: {first_line(error['message'])}")

        binary = machopo_binary()
        server.mode = "inventory"
        server.deadline = (MACHOPO_SUITE_DEADLINE - 10) * 1000
        server.probe = probe_list(chrome)
        machopo_suites = [name for name, default in all_suites if (name in chosen if chosen else default)]
        print(f"machopo: the probe, then {len(machopo_suites)} suites, one at a time")
        with open(log_path, "w", encoding="utf-8") as log:
            probe = run_in_machopo(server, binary, server.url("/__machopo__/probe.html"), MACHOPO_PROBE_DEADLINE, log)
            results = [
                run_in_machopo(server, binary, suite_url(server, name, 1), MACHOPO_SUITE_DEADLINE, log)
                for name in machopo_suites
            ]
    finally:
        server.close()
    missing = parse_run(probe)
    if "error" in missing:
        print(f"machopo's probe didn't report: {missing['error']}; its log: {log_path}")
        return 1
    runs = {name: summarize_run(result) for name, result in zip(machopo_suites, results)}
    for name, run in runs.items():
        print(f"  {name}: {run['status']}, {run['done']} tests; {run['first_error']}")
    write_report(chrome, missing, runs, all_suites)
    print(f"results: {RESULTS / 'inventory.md'}, {RESULTS / 'inventory.json'}")
    return 0


def cmd_measure(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    names = [name for name, default in all_suites if (name in chosen if chosen else default)]
    iterations = args.iterations
    browsers = ["machopo", "Chrome"] if args.browser is None else [{"machopo": "machopo", "chrome": "Chrome"}[args.browser]]
    chrome_file, machopo_file = RESULTS / "chrome-runs.json", RESULTS / "machopo-runs.json"
    reference = None
    if args.reuse_chrome or args.browser == "machopo":
        if chrome_file.exists():
            reference = json.loads(chrome_file.read_text(encoding="utf-8"))["suites"]
        else:
            print(f"no Chrome runs to check machopo's against ({chrome_file}): its summaries aren't compared")
        if args.reuse_chrome:
            browsers = [b for b in browsers if b != "Chrome"]
    env = environment(MACHOPO_PROFILE)
    if env["power"] == "battery":
        print("warning: on battery power, the times may not be comparable")
    binary = machopo_binary(MACHOPO_PROFILE) if "machopo" in browsers else None
    if binary:
        gpu = machopo_gpu(binary)
        env["gpu"] = describe_gpu(gpu)
        if problem := gpu_problem(gpu):
            print(f"warning: {problem}")
    results: dict[str, dict[str, dict]] = {b: {} for b in browsers}
    RESULTS.mkdir(parents=True, exist_ok=True)
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(iterations) * 1000
    timeout = measure_deadline(iterations) + START_TIME
    print(f"{', '.join(browsers)}: {len(names)} suites, {iterations} iterations")
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for name, browser in schedule(names, browsers):
                url = suite_url(server, name, iterations)
                started = time.monotonic()
                if browser == "machopo":
                    result = run_in_machopo(server, binary, url, timeout, log)
                else:
                    result = run_in_chrome(server, url, timeout)
                run = parse_run(result)
                results[browser][name] = run
                print(f"  {name}, {browser}: {run.get('status') or run['error']}, "
                      f"{time.monotonic() - started:.0f} s")
    finally:
        server.close()
    for browser, file in (("Chrome", chrome_file), ("machopo", machopo_file)):
        if browser in results:
            saved = {"environment": env, "iterations": iterations, "suites": results[browser]}
            file.write_text(json.dumps(saved), encoding="utf-8")
    if "Chrome" in results:
        reference = results["Chrome"]
    rows = compare(names, results.get("Chrome"), results.get("machopo"), reference)
    report = write_measure_report(rows, env, iterations)
    print(report.read_text(encoding="utf-8"))
    return 0


def cmd_trace(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    """Each suite once in each browser, recording a trace; the times aren't
    compared (recording slows both down)."""
    names = [name for name, default in all_suites if (name in chosen if chosen else default)]
    browsers = ["machopo", "Chrome"] if args.browser is None else [{"machopo": "machopo", "chrome": "Chrome"}[args.browser]]
    folder = RESULTS / "trace"
    folder.mkdir(parents=True, exist_ok=True)
    binary = machopo_binary(MACHOPO_PROFILE) if "machopo" in browsers else None
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(args.iterations) * 1000
    timeout = measure_deadline(args.iterations) + START_TIME
    traces = []
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for name, browser in schedule(names, browsers):
                url = suite_url(server, name, args.iterations)
                if browser == "machopo":
                    trace = folder / f"{name}-machopo.pftrace"
                    run = parse_run(run_in_machopo(server, binary, url, timeout, log, trace))
                else:
                    trace = folder / f"{name}-chrome.pftrace"
                    run = parse_run(run_in_chrome(server, url, timeout, trace))
                problems = run_problems(run, None)
                print(f"  {name}, {browser}: {problems[0] if problems else 'done'}; {trace}")
                traces.append(trace)
    finally:
        server.close()
    missing = [str(trace) for trace in traces if not trace.exists()]
    if missing:
        print(f"no trace written: {', '.join(missing)}")
        return 1
    times: dict[str, dict[str, tuple[collections.Counter, collections.Counter]]] = {}
    for name in names:
        for browser in browsers:
            trace = folder / f"{name}-{'machopo' if browser == 'machopo' else 'chrome'}.pftrace"
            times.setdefault(name, {})[browser] = phase_times(trace, browser)
    report = write_phases_report(times, environment(MACHOPO_PROFILE), args.iterations)
    print(report.read_text(encoding="utf-8"))
    print("open the traces in https://ui.perfetto.dev")
    return 0


def cmd_profile(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    """Each suite once in each browser, DevTools' `Profiler` sampling its
    script (`run_in_chrome`, `run_in_machopo` with `cpu_profile`); the profiles
    compared by JS function in the windows the runner timed, by its marks
    (`mark_windows`, `profiles.write_report`)."""
    names = [name for name, default in all_suites if (name in chosen if chosen else default)]
    folder = RESULTS / "profile"
    folder.mkdir(parents=True, exist_ok=True)
    binary = machopo_binary(MACHOPO_PROFILE)
    server = Server()
    server.mode = "measure"
    server.hold = True
    server.deadline = measure_deadline(args.iterations) * 1000
    timeout = measure_deadline(args.iterations) + START_TIME
    suites: dict[str, dict] = {}
    failed = False
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for name, browser in schedule(names, ["machopo", "Chrome"]):
                url = suite_url(server, name, args.iterations)
                path = folder / f"{name}-{'machopo' if browser == 'machopo' else 'chrome'}.cpuprofile"
                path.unlink(missing_ok=True)
                if browser == "machopo":
                    run = parse_run(run_in_machopo(server, binary, url, timeout, log, cpu_profile=path))
                else:
                    run = parse_run(run_in_chrome(server, url, timeout, cpu_profile=path))
                problems = run_problems(run, None)
                print(f"  {name}, {browser}: {problems[0] if problems else 'done'}; {path}")
                if problems or not path.exists():
                    failed = True
                    continue
                windows = mark_windows([(time, mark) for time, mark in run.get("marks", [])])
                if not windows:
                    print(f"  {name}, {browser}: no timed windows in the runner's marks")
                    failed = True
                    continue
                profile = profiles.load(path)
                if run.get("runnerStarted") is None or run["runnerStarted"] < profile["pageClock"]["end"]:
                    print(f"  {name}, {browser}: the runner didn't wait for the profile")
                    failed = True
                    continue
                try:
                    placed = profiles.on_profile(profile, windows)
                except ValueError as e:
                    print(f"  {name}, {browser}: the page's clock: {e}")
                    failed = True
                    continue
                suite = suites.setdefault(name, {"times": {}, "members": collections.Counter(), "uncertainty": {},
                                                 "whole": {}})
                by_function, members = profiles.attribute(profile, placed)
                suite[browser] = profiles.per_iteration(by_function, args.iterations)
                if browser == "machopo":
                    suite["members"] = profiles.per_iteration(members, args.iterations)
                suite["whole"][browser] = profiles.per_iteration(profiles.attribute(profile)[0], args.iterations)
                suite["uncertainty"][browser] = profiles.uncertainty(profile)
                suite["times"][browser] = median(suite_samples(run, name))
    finally:
        server.close()
    complete = {name: suite for name, suite in suites.items() if "machopo" in suite and "Chrome" in suite}
    env = environment(MACHOPO_PROFILE)
    report = profiles.write_report(RESULTS, complete, args.iterations, {
        "machopo": env["machopo"], "chrome": env["chrome"], "interval": PROFILE_INTERVAL,
        "system": f"{env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')})",
    })
    print(f"each browser's time by JS function: {report}")
    return 1 if failed else 0


def sample_suites(binary: Path, names: list[str], folder: Path,
                  iterations: int) -> tuple[list[tuple[str, Folded, str]], bool]:
    """Each suite of `names` once in machopo (`binary`) under the platform's
    sampler (`sampler`: `perf`, samply), with a
    trace whose timed windows pick the samples: the renderer's main
    thread's stacks in them, by event, folded into `folder`. Its rows for
    `sample.md` (with what found its windows), and whether a suite wrote no
    trace or samples."""
    folder.mkdir(parents=True, exist_ok=True)
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(iterations) * 1000
    timeout = measure_deadline(iterations) + START_TIME
    rows = []
    failed = False
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for name in names:
                trace = folder / f"{name}-machopo.pftrace"
                samples = folder / f"{name}-machopo{(sampling() or LINUX_SAMPLING).suffix}"
                url = suite_url(server, name, iterations)
                run = parse_run(run_in_machopo(server, binary, url, timeout, log, trace, samples))
                problems = run_problems(run, None)
                if not trace.exists() or not samples.exists():
                    print(f"  {name}: no {'trace' if not trace.exists() else 'samples'} written")
                    failed = True
                    continue
                windows, found_by = timed_windows(perfetto_slices(trace).get(MAIN_THREAD, []),
                                                  perfetto_marks(trace).get(MAIN_THREAD, []))
                taken, pids = read_samples(samples, MAIN_THREAD)
                if sys.platform == "win32":
                    # What samply's times count from, read just now.
                    for etl in samply_etls(samples):
                        etl.unlink(missing_ok=True)
                else:
                    # V8's names for its JS, read by `perf script` just now,
                    # and its log.
                    for pid in pids:
                        Path(f"/tmp/perf-{pid}.map").unlink(missing_ok=True)
                    v8_log(samples).unlink(missing_ok=True)
                by_event = {event: fold(taken.get(event, []), windows)
                            for event in (sampling() or LINUX_SAMPLING).events}
                for event, folded in by_event.items():
                    folded_path(folder, name, event).write_text(
                        "".join(f"{stack} {count}\n" for stack, count in folded.most_common()),
                        encoding="utf-8", newline="\n")
                rows.append((name, by_event, found_by))
                print(f"  {name}: {problems[0] if problems else 'done'}; {totals_text(by_event)} "
                      f"In {len(windows)} timed windows, by {found_by}; {folded_path(folder, name, time_event(by_event))}")
    finally:
        server.close()
    return rows, failed


def cmd_sample(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    """Each suite once in machopo, sampled by Linux's `perf` or samply (macOS,
    Windows; `sample_all`), under `sample/`, and the functions most samples
    are in, `sample.md`; with `--browser chrome`, in a Chromium instead
    (`cmd_sample_chrome`: Chrome for Testing has no symbols). With both
    browsers' samples there, and both's times from `bench.py speedometer`,
    the samples by category, `categories.md`. With `--against`, machopo at
    that commit and the working tree's take turns (`cmd_compare`)."""
    if getattr(args, "browser", None) == "chrome":
        return cmd_sample_chrome(args, all_suites, chosen)
    way = sampling()
    if way is None or shutil.which(way.tool) is None:
        print("--sample takes Linux's perf, or samply on macOS and Windows (cargo install --locked samply)")
        return 1
    if sys.platform == "win32" and xperf() is None:
        print("--sample on Windows takes xperf, which samply records ETW with: install the Windows Performance "
              "Toolkit (in the Windows ADK)")
        return 1
    names = [name for name, default in all_suites if (name in chosen if chosen else default)]
    if getattr(args, "against", None):
        return cmd_compare(args, names)
    [(rows, failed)] = sample_all([("", machopo_binary(MACHOPO_PROFILE), names, RESULTS / "sample", args.iterations)])
    print(f"the functions most samples are in: {write_sample_report(rows)}")
    write_categories_report()
    return 1 if failed else 0


# The Chromium `--sample --browser chrome` takes unless told another
# (`--chrome-binary`): Fedora's, whose symbols `dnf debuginfo-install
# chromium` puts where `perf` finds them.
CHROMIUM = Path("/usr/lib64/chromium-browser/chromium-browser")


def cmd_sample_chrome(args, all_suites: list[tuple[str, bool]], chosen: list[str] | None) -> int:
    """Each suite once in a Chromium with symbols: its time first, not
    sampled (`chromium-runs.json`, to see how its shares stand for Chrome
    for Testing's), then sampled by `perf` as machopo is (`sample_chrome_suites`),
    under `sample/`; then `categories.md` if machopo's samples and both's times
    are there. Linux only."""
    if not sys.platform.startswith("linux") or shutil.which("perf") is None:
        print("--sample --browser chrome takes Linux's perf")
        return 1
    binary = Path(getattr(args, "chrome_binary", None) or CHROMIUM)
    if not binary.exists():
        print(f"no Chromium at {binary}: install Fedora's (sudo dnf install chromium; sudo dnf debuginfo-install "
              "chromium), or give one with --chrome-binary")
        return 1
    if getattr(args, "against", None):
        print("--against compares two machopos: not with --browser chrome")
        return 1
    names = [name for name, default in all_suites if (name in chosen if chosen else default)]
    timed = time_chromium(binary, names, max(args.iterations, 10))
    rows, failed = sample_chrome_suites(binary, names, RESULTS / "sample", args.iterations)
    failed = failed or len(timed) < len(names)
    write_categories_report()
    return 1 if failed else 0


def time_chromium(binary: Path, names: list[str], iterations: int) -> dict:
    """Each suite once in the Chromium at `binary`, as `bench.py speedometer`
    times Chrome, its runs saved to `chromium-runs.json`: the valid ones."""
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(iterations) * 1000
    timeout = measure_deadline(iterations) + START_TIME
    runs = {}
    print(f"{binary}: {len(names)} suites, {iterations} iterations, not sampled")
    try:
        for name in names:
            run = parse_run(run_in_chrome(server, suite_url(server, name, iterations), timeout, binary=binary))
            print(f"  {name}: {run.get('status') or run['error']}")
            if "error" not in run:
                runs[name] = run
    finally:
        server.close()
    RESULTS.mkdir(parents=True, exist_ok=True)
    (RESULTS / "chromium-runs.json").write_text(
        json.dumps({"binary": str(binary), "iterations": iterations, "suites": runs}), encoding="utf-8")
    return runs


def sample_chrome_suites(binary: Path, names: list[str], folder: Path,
                         iterations: int) -> tuple[list[tuple[str, Folded, str]], bool]:
    """Each suite of `names` once in the Chromium at `binary` under `perf`
    as machopo is (`perf_sampler`, V8's perf map), folded into `folder` as
    `<suite>-chrome.folded`: the renderer's main thread's stacks in the
    runner's timed windows. Chromium runs without its sandbox, which keeps
    V8 from writing its perf map, and traces the runner's marks alone (the
    categories `run_in_chrome` traces by default slow it to half its
    speed); its main thread is the one the marks are on, by its tid (it has
    no name of its own on Linux), on CLOCK_MONOTONIC as its trace is."""
    folder.mkdir(parents=True, exist_ok=True)
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(iterations) * 1000
    timeout = measure_deadline(iterations) + START_TIME
    rows = []
    failed = False
    print(f"{binary}: {len(names)} suites, {iterations} iterations, sampled")
    try:
        for name in names:
            trace = folder / f"{name}-chrome.pftrace"
            samples = folder / f"{name}-chrome.perf.data"
            log = v8_log(samples)
            wrapper, _ = perf_sampler(samples)
            arguments = ("--no-sandbox", f"--js-flags={V8_SAMPLED_FLAGS.format(log)}")
            # Every V8 of the run writes a perf map (its other renderers'
            # too), not only the sampled thread's process.
            maps = set(Path("/tmp").glob("perf-*.map"))
            run = parse_run(run_in_chrome(server, suite_url(server, name, iterations), timeout, trace,
                                          categories=(USER_TIMING,), binary=binary, wrapper=wrapper,
                                          arguments=arguments))
            thread = perfetto_marked_thread(trace) if trace.exists() else None
            if thread is None or not samples.exists():
                print(f"  {name}: no {'samples' if thread else 'marks in the trace'}; {run.get('error', '')}")
                failed = True
                continue
            windows, found_by = timed_windows([], perfetto_marks(trace).get(MAIN_THREAD, []))
            taken, _ = perf_samples(samples, MAIN_THREAD, tid=thread[1], monotonic=True)
            for path in set(Path("/tmp").glob("perf-*.map")) - maps:
                path.unlink(missing_ok=True)
            log.unlink(missing_ok=True)
            by_event = {event: fold(taken.get(event, []), windows) for event in LINUX_SAMPLING.events}
            for event, folded in by_event.items():
                path = folder / (f"{name}-chrome.folded" if event in TIME_EVENTS else f"{name}-chrome.{event}.folded")
                path.write_text("".join(f"{stack} {count}\n" for stack, count in folded.most_common()),
                                encoding="utf-8", newline="\n")
            rows.append((name, by_event, found_by))
            print(f"  {name}: {run.get('status') or run.get('error')}; {totals_text(by_event)} "
                  f"In {len(windows)} timed windows, by {found_by}")
    finally:
        server.close()
    return rows, failed


def suite_times(path: Path) -> dict[str, float]:
    """Each valid suite's median time (ms) in a runs file of `bench.py
    speedometer` (`machopo-runs.json`, `chrome-runs.json`, `chromium-runs.json`)."""
    if not path.exists():
        return {}
    suites = json.loads(path.read_text(encoding="utf-8"))["suites"]
    return {name: median(suite_samples(run, name)) for name, run in suites.items()
            if "error" not in run and run.get("iterations")}


def write_categories_report() -> Path | None:
    """`categories.md` (`categories.report`) of the samples under `sample/`,
    if both browsers' are there and both's times (machopo's and Chrome for
    Testing's from `bench.py speedometer`, the sampled Chromium's from
    `--sample --browser chrome`); what is missing otherwise."""
    folder = RESULTS / "sample"
    times = {side: suite_times(RESULTS / f"{side}-runs.json") for side in ("machopo", "chrome", "chromium")}
    folded = {side: {path.name[: -len(f"-{side}.folded")]: path for path in folder.glob(f"*-{side}.folded")}
              for side in ("machopo", "chrome")} if folder.exists() else {"machopo": {}, "chrome": {}}
    missing = [what for what, there in (("machopo's samples (--sample)", folded["machopo"]),
                                        ("a Chromium's samples (--sample --browser chrome)", folded["chrome"]),
                                        ("machopo's and Chrome for Testing's times (bench.py speedometer)",
                                         times["machopo"] and times["chrome"])) if not there]
    if missing:
        print(f"no categories.md yet: it takes {', '.join(missing)}")
        return None
    machopo = {name: categories.Suite(categories.read_folded(path), times["machopo"][name])
           for name, path in folded["machopo"].items() if name in times["machopo"]}
    chrome = {name: categories.Suite(categories.read_folded(path), times["chrome"][name])
              for name, path in folded["chrome"].items() if name in times["chrome"]}
    ratio = {name: times["chromium"][name] / times["chrome"][name]
             for name in times["chromium"] if name in times["chrome"]}
    header = ["# machopo's and Chrome's samples by category", "",
              f"{date.today().isoformat()}, machopo {git_head()}; each suite's shares of its samples times its median "
              "time when not sampled (machopo's and Chrome for Testing's last `bench.py speedometer`), the Chromium's "
              "shares standing for Chrome for Testing's. The categories and how a sample is put in one: "
              "`scripts/bench/speedometer/categories.py`."]
    report = RESULTS / "categories.md"
    report.write_text(categories.report(machopo, chrome, ratio, header), encoding="utf-8", newline="\n")
    print(f"the samples by category: {report}")
    return report


# A sampling job: what it is (printed before it, if anything), machopo's
# binary, the suites, the folder, the iterations of each.
SampleJob = tuple[str, Path, list[str], Path, int]

# What a copy of this script running as Administrator takes from the one
# that started it (`sample_elevated`): its globals that the tests change.
ELEVATED_GLOBALS = ("RESULTS", "SPEEDOMETER")


def sample_all(jobs: list[SampleJob]) -> list[tuple[list[tuple[str, Folded, str]], bool]]:
    """Each job sampled in turn (`sample_suites`). On Windows ETW needs
    Administrator: unless this process is one, all of them in one copy of
    this script that is (`sample_elevated`), so that UAC asks once."""
    if sys.platform == "win32" and not is_admin():
        return sample_elevated(jobs)
    results = []
    for label, binary, names, folder, iterations in jobs:
        if label:
            print(f"{label}:")
        results.append(sample_suites(binary, names, folder, iterations))
    return results


def sample_elevated(jobs: list[SampleJob]) -> list[tuple[list[tuple[str, Folded, str]], bool]]:
    """`sample_all` of `jobs` in this script run as Administrator
    (`run_elevated`), which starts with an environment of its own: it is
    handed this one's, the jobs and `ELEVATED_GLOBALS` in `elevated.json`,
    prints into `elevated.log` and gives back the jobs' rows in
    `elevated-results.json`. machopo is built already: it runs no cargo."""
    RESULTS.mkdir(parents=True, exist_ok=True)
    state, log, results = (RESULTS / "elevated.json", RESULTS / "elevated.log", RESULTS / "elevated-results.json")
    results.unlink(missing_ok=True)
    state.write_text(json.dumps({
        "jobs": [[label, str(binary), names, str(folder), iterations]
                 for label, binary, names, folder, iterations in jobs],
        "env": dict(os.environ), "cwd": os.getcwd(), "log": str(log), "results": str(results),
        "globals": {name: str(globals()[name]) for name in ELEVATED_GLOBALS},
    }), encoding="utf-8", newline="\n")
    print("sampling as Administrator (ETW needs it): UAC asks once")
    status = run_elevated([str(Path(__file__).resolve()), "--elevated", str(state)], log)
    if not results.exists():
        raise SystemExit(f"sampling as Administrator gave nothing back (exit status {status})")
    return [([(name, {event: collections.Counter(stacks) for event, stacks in by_event.items()}, found_by)
              for name, by_event, found_by in rows], failed)
            for rows, failed in json.loads(results.read_text(encoding="utf-8"))]


def elevated_main(state: Path) -> int:
    """The copy of this script `sample_elevated` runs as Administrator:
    the jobs in `state`, sampled in the environment, folder and globals of
    the one that started it, printing into its log; their rows into its
    results."""
    given = json.loads(state.read_text(encoding="utf-8"))
    os.environ.clear()
    os.environ.update(given["env"])
    os.chdir(given["cwd"])
    for name, value in given["globals"].items():
        globals()[name] = Path(value)
    with open(given["log"], "a", encoding="utf-8", buffering=1) as log:
        sys.stdout = sys.stderr = log
        try:
            results = sample_all([(label, Path(binary), names, Path(folder), iterations)
                                  for label, binary, names, folder, iterations in given["jobs"]])
        except BaseException:
            traceback.print_exc()
            return 1
    Path(given["results"]).write_text(json.dumps(results), encoding="utf-8", newline="\n")
    return 0


def time_suites(binary: Path, names: list[str], iterations: int) -> dict[str, tuple[dict, int | None]]:
    """Each suite of `names` once in machopo (`binary`), `iterations` in a
    row: its run, as `measure.mjs` posts it, and machopo's peak memory."""
    RESULTS.mkdir(parents=True, exist_ok=True)
    server = Server()
    server.mode = "measure"
    server.deadline = measure_deadline(iterations) * 1000
    timeout = measure_deadline(iterations) + START_TIME
    timed = {}
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for name in names:
                started = time.monotonic()
                result = run_in_machopo(server, binary, suite_url(server, name, iterations), timeout, log)
                run = parse_run(result)
                timed[name] = (run, result.get("memory"))
                print(f"  {name}: {run.get('status') or run['error']}, {time.monotonic() - started:.0f} s")
    finally:
        server.close()
    return timed


def cmd_time_compare(args, names: list[str]) -> int:
    """machopo at `args.against` and the working tree's, timed in turn
    `args.runs` times each (a run: every suite, `args.iterations`
    iterations), the other version first, so that the machine's drift
    falls on both; `compare.md` (`write_time_report`)."""
    problems = against_problems(args.against)
    if problems:
        print("\n".join(problems))
        return 1
    env = environment(MACHOPO_PROFILE)
    if env["power"] == "battery":
        print("warning: on battery power, the times may not be comparable")
    binaries = against_binaries(args.against)
    gpu = machopo_gpu(binaries["current"])
    env["gpu"] = describe_gpu(gpu)
    if problem := gpu_problem(gpu):
        print(f"warning: {problem}")
    by_suite: dict[str, dict[str, list]] = {name: {"against": [], "current": []} for name in names}
    for run in range(1, args.runs + 1):
        for side in ("against", "current"):
            print(f"{side}, run {run} of {args.runs}:")
            for name, timed in time_suites(binaries[side], names, args.iterations).items():
                by_suite[name][side].append(timed)
    report = write_time_report(args.against, args.runs, args.iterations, by_suite, env)
    print(report.read_text(encoding="utf-8"))
    return 0


def cmd_compare(args, names: list[str]) -> int:
    """machopo at `args.against` and the working tree's (`against_binaries`),
    sampled in turn `args.runs` times each, the other version first;
    `compare.md`."""
    problems = against_problems(args.against)
    if problems:
        print("\n".join(problems))
        return 1
    binaries = against_binaries(args.against)
    by_suite: dict[str, dict[str, list[Folded]]] = {name: {"against": [], "current": []} for name in names}
    found_by: dict[str, set[str]] = {"against": set(), "current": set()}
    failed = False
    turns = [(side, run) for run in range(1, args.runs + 1) for side in ("against", "current")]
    sampled = sample_all([(f"{side}, run {run} of {args.runs}", binaries[side], names,
                           RESULTS / "compare" / f"{side}-{run}", args.iterations) for side, run in turns])
    for (side, _), (rows, side_failed) in zip(turns, sampled):
        failed |= side_failed
        for name, by_event, found in rows:
            by_suite[name][side].append(by_event)
            found_by[side].add(found)
    print(f"the comparison: {write_compare_report(args.against, args.runs, args.iterations, by_suite, found_by)}")
    return 1 if failed else 0


def cmd_speedometer(args) -> int:
    if not (SPEEDOMETER / "index.html").exists():
        print(f"no Speedometer at {SPEEDOMETER}: `python3 scripts/ci.py third-party`")
        return 1
    all_suites = suites()
    names = {name for name, _ in all_suites}
    chosen = args.suites.split(",") if args.suites else None
    for name in chosen or []:
        if name not in names:
            print(f"no suite {name}; the suites: {', '.join(sorted(names))}")
            return 2
    if args.inventory:
        return cmd_inventory(args, all_suites, chosen)
    if args.trace:
        return cmd_trace(args, all_suites, chosen)
    if getattr(args, "profile", False):
        return cmd_profile(args, all_suites, chosen)
    if args.sample:
        return cmd_sample(args, all_suites, chosen)
    if getattr(args, "against", None):
        return cmd_time_compare(args, [name for name, default in all_suites if (name in chosen if chosen else default)])
    return cmd_measure(args, all_suites, chosen)


if __name__ == "__main__":
    # The copy of this script that samples as Administrator on Windows
    # (`sample_elevated`); `bench.py` runs the rest.
    if sys.argv[1:2] == ["--elevated"] and len(sys.argv) == 3:
        sys.exit(elevated_main(Path(sys.argv[2])))
    sys.exit("run scripts/bench.py speedometer")
