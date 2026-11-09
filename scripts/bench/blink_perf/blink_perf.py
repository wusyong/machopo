"""`bench.py blink-perf`: Blink's perf tests (Chromium's
`third_party/blink/perf_tests`, at the commit `third_party.toml` pins, in
`../third_party/blink_perf`) in machopo and in Chrome for Testing, each page in
a new browser of each, machopo first. A page is a micro-benchmark of one
operation that `resources/runner.js` (`PerfTestRunner`) runs over and over;
the server adds `hook.js` after that script, which posts the runner's
values (or an error) and a summary of the document back. A page is valid
when both browsers gave values, nothing threw, and their summaries are the
same; pages that need what only Chromium's harness has are left out, not
run. By default the pages in `pages.txt`, those machopo runs validly, or
those of them under the paths given (an area: `dom/`). The
report (`target/bench/blink_perf/report.md`) has each valid page's median
in each browser and how many times slower machopo is, by folder, with the
geometric means; the listed pages that weren't valid (regressions), the
others that weren't and why, and those left out; `runs.json` keeps what
each browser posted. With `--against REV`, machopo at `REV` and the working
tree's instead, in turn (`compare.md`).

Standard library only.
"""

from __future__ import annotations

import json
import math
import re
import shutil
import sys
from datetime import date
from pathlib import Path

# What the benchmarks share, from `scripts/bench/common.py`; the GC's
# trace, as `bench.py gc` reads it.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import common  # noqa: E402
from common import ROOT  # noqa: E402


BLINK_PERF = ROOT.parent / "third_party" / "blink_perf"
# `hook.js`, beside this file.
SCRIPTS = Path(__file__).resolve().parent
RESULTS = ROOT / "target" / "bench" / "blink_perf"
# The runner every page loads, which the server adds `hook.js` to.
RUNNER = "/resources/runner.js"


# The units pages report: the runner's `ms` (`measureTime`) and `runs/s`
# (`measureRunsPerSecond`), and what pages that measure themselves give.
# How many times slower machopo is: machopo's over Chrome's where less is better,
# Chrome's over machopo's where more is; none for a unit that is neither
# (`percent`).
LESS_IS_BETTER = {"ms"}
MORE_IS_BETTER = {"runs/s", "MB/s"}

# The pages run by default: one a line (`#` starts a comment), each valid in
# both browsers and checked by a person when it was marked (`notes`).
PAGES = SCRIPTS / "pages.txt"

# machopo this many times "slower" or less is suspect: a page whose work machopo
# skips (`editing/textarea-dom` appends text to a `<textarea>` and clears
# it, Chrome updating its value and `:valid` each time) ends with the same
# document, so its summary doesn't tell.
SUSPICIOUS = 0.5

# What leaves a page out, unrun: it is measured from Chrome's traces (by
# Telemetry, none here) or needs what only Chromium's test harness gives.
# `PerfTestRunner.gc()` doesn't: without `GCController` it does nothing.
EXCLUDED = [
    (re.compile(r"tracingCategories|traceEventsToMeasure"), "measured from Chrome's traces"),
    (re.compile(r"\binternals\."), "needs Chromium's `internals`"),
    (re.compile(r"\btestRunner\."), "needs Blink's `testRunner`"),
    (re.compile(r"\bGCController\b"), "needs Blink's `GCController`"),
    (re.compile(r"\bgpuBenchmarking\b"), "needs Chrome's `gpuBenchmarking`"),
    (re.compile(r"(?<![.\w])gc\(\)"), "calls `gc()`"),
]
# A page that logs statistics itself without the "Time:" the hook takes
# (old WebKit pages, which measure two rounds): left out too.
LOGS_STATISTICS = re.compile(r"PerfTestRunner\.logStatistics\(")
TIME_TITLE = re.compile(r"""["']Time:["']""")
# A page that catches exceptions itself: an API machopo lacks may go unseen.
CATCHES = re.compile(r"\btry\s*\{|\.catch\(")
SCRIPT_SRC = re.compile(r"""<script[^>]*\bsrc=["']([^"']+)["']""", re.I)


class Server(common.Server):
    """`perf_tests/` (or, for the tool's own tests, another `root`), with
    `summary.mjs`'s `summarize` (as `__machopoSummarize`) and `hook.js` after
    `resources/runner.js`. In the runner's script, not imported: an
    `import()` the page starts can settle long after it is done (in machopo,
    `dom/range-select-node` after 285 s; Chrome, 1 s)."""

    def __init__(self, root: Path | None = None) -> None:
        super().__init__(root or BLINK_PERF)

    def get(self, path: str) -> tuple[bytes | None, str]:
        body, content_type = super().get(path)
        if body is not None and path == RUNNER:
            summary = common.SUMMARY_FILE.read_text(encoding="utf-8").replace("export function", "function")
            body += f"\nvar __machopoSummarize = (function () {{\n{summary}\nreturn summarize;\n}})();\n".encode()
            body += (SCRIPTS / "hook.js").read_bytes()
        return body, content_type


def pages(root: Path, dirs: list[str], chosen: list[str]) -> list[str]:
    """The pages to run, as paths under `root`: each `.html` under the
    folders `dirs` (not their `resources/` and `test_data/`, which pages
    load), and the pages `chosen`, in that order, each once."""
    found = []
    for folder in dirs:
        base = root / folder
        if not base.is_dir():
            raise SystemExit(f"no folder {folder} in {root}")
        for page in sorted(base.rglob("*.html")):
            parts = page.relative_to(root).parts
            if "resources" in parts or "test_data" in parts:
                continue
            found.append(page.relative_to(root).as_posix())
    for page in chosen:
        if not (root / page).is_file():
            raise SystemExit(f"no page {page} in {root}")
        found.append(page)
    return list(dict.fromkeys(found))


def under(names: list[str], paths: list[str]) -> list[str]:
    """The pages of `names` under any of `paths` (folders or pages)."""
    return [name for name in names if any(name == p or name.startswith(p.rstrip("/") + "/") for p in paths)]


def listed(path: Path) -> list[str]:
    """The pages in the list `path`."""
    if not path.is_file():
        return []
    lines = (line.split("#", 1)[0].strip() for line in path.read_text(encoding="utf-8").splitlines())
    return [line for line in lines if line]


def sources(root: Path, page: str) -> list[str]:
    """The text of `page` and of the scripts it loads from `root` (not the
    runner, which the hook is added to)."""
    html = (root / page).read_text(encoding="utf-8", errors="replace")
    texts = [html]
    for src in SCRIPT_SRC.findall(html):
        if "://" in src:
            continue
        file = ((root / page).parent / src).resolve()
        if root.resolve() not in file.parents or not file.is_file() or file == (root / RUNNER[1:]).resolve():
            continue
        texts.append(file.read_text(encoding="utf-8", errors="replace"))
    return texts


def scan(root: Path, page: str) -> dict:
    """What `page`'s source says: why it is left out (`excluded`), or
    whether it catches exceptions itself (`catches`)."""
    texts = sources(root, page)
    for pattern, why in EXCLUDED:
        if any(pattern.search(text) for text in texts):
            return {"excluded": why}
    if any(LOGS_STATISTICS.search(text) and not TIME_TITLE.search(text) for text in texts):
        return {"excluded": 'logs its own statistics without "Time:"'}
    return {"catches": any(CATCHES.search(text) for text in texts)}


def result(posted: dict) -> dict:
    """What a browser's run of a page gave, as kept: the runner's `unit` and
    `values`, or why there are none (`error`); with the last lines the
    runner logged (`log`) and the summary of the document (`summary`, or
    why there is none: `summary_error`)."""
    if "error" in posted:
        return {"error": posted["error"]}
    body = common.parse_run(posted)
    if "error" in body:
        return body
    kept = {"log": body.get("log", [])}
    for key in ("summary", "summary_error"):
        if key in body:
            kept[key] = body[key]
    if body.get("error"):
        return dict(kept, error=body["error"])
    if not body.get("values"):
        last = next((line for line in reversed(kept["log"]) if line.strip()), None)
        return dict(kept, error="the runner gave no values" + (f" (its log: {last})" if last else ""))
    return dict(kept, unit=body.get("unit", "ms"), values=body["values"])


def slowdown(machopo: dict, chrome: dict) -> float | None:
    """How many times slower machopo is than Chrome on a page (above 1: slower),
    from their medians; `None` without both, with units that differ, or
    with one that tells no direction."""
    if "values" not in machopo or "values" not in chrome or machopo["unit"] != chrome["unit"]:
        return None
    a, b = common.median(machopo["values"]), common.median(chrome["values"])
    if a <= 0 or b <= 0:
        return None
    if machopo["unit"] in LESS_IS_BETTER:
        return a / b
    if machopo["unit"] in MORE_IS_BETTER:
        return b / a
    return None


def problems(run: dict) -> list[str]:
    """Why a page's run (`run_pages`'s) isn't valid: a browser gave no
    values, the units differ, or the summaries do. Empty when it is."""
    found = [f"{b}: {run[b]['error'].splitlines()[0]}" for b in ("machopo", "Chrome") if "error" in run.get(b, {})]
    if found:
        return found
    machopo, chrome = run["machopo"], run["Chrome"]
    if machopo["unit"] != chrome["unit"]:
        return [f"machopo in {machopo['unit']}, Chrome in {chrome['unit']}"]
    for b in ("machopo", "Chrome"):
        if "summary" not in run[b]:
            found.append(f"{b}: no summary ({run[b].get('summary_error', 'none posted')})")
    if found:
        return found
    difference = common.summary_difference(machopo["summary"], chrome["summary"])
    return [f"the documents differ: {difference}"] if difference else []


def suspect(run: dict) -> bool:
    s = slowdown(run["machopo"], run["Chrome"])
    return s is not None and s < SUSPICIOUS


def notes(run: dict) -> list[str]:
    """What to check by hand before a valid page goes on the list."""
    found = []
    if suspect(run):
        found.append("suspect: machopo over 2 times faster")
    if run.get("catches"):
        found.append("catches exceptions itself")
    return found


def geomean(values: list[float]) -> float | None:
    return math.exp(sum(math.log(v) for v in values) / len(values)) if values else None


def number(value: float) -> str:
    return f"{value:.3g}" if value < 100 else f"{value:.0f}"


def write_report(runs: dict[str, dict], env: dict, listed_pages: set[str] = frozenset()) -> Path:
    """`report.md`: the pages of the list that weren't valid; each folder's
    valid pages (median in each browser, how many times slower machopo is,
    what to check) and their geometric mean; the geometric mean of all the
    valid pages; the pages that weren't valid, with why; those left out."""
    valid = [page for page in runs if "excluded" not in runs[page] and not problems(runs[page])]
    trusted = [page for page in valid if not suspect(runs[page])]
    every = [slowdown(runs[page]["machopo"], runs[page]["Chrome"]) for page in trusted]
    every = [s for s in every if s]
    overall = geomean(every)
    lines = [
        f"# blink_perf in machopo and Chrome ({env['date']})",
        "",
        f"machopo `{env['machopo']}` (the `{env['profile']}` profile), Chrome for Testing {env['chrome']}; "
        f"{env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')}), power: {env['power']}. Blink's perf tests at "
        f"`{env['blink_perf']}`. Each page in a new browser of each, machopo first; the median of the "
        "runner's iterations (its first, a warm-up, not counted). \"machopo slower\" is machopo's median over Chrome's "
        "where less is better (ms), Chrome's over machopo's where more is (runs/s). A page is valid when both "
        "gave values and their documents' summaries are the same; the geometric means leave out the suspect ones.",
        "",
        f"The valid pages: machopo slower {number(overall)} (geometric mean of {len(every)}); "
        f"{len(valid)} valid of {len(runs)}."
        if overall else f"No page has a number to compare; {len(valid)} valid of {len(runs)}.",
    ]
    regressions = [page for page in runs if page in listed_pages and page not in valid]
    if regressions:
        lines += ["", "## Listed, not valid", ""]
        for page in regressions:
            why = runs[page].get("excluded") or "; ".join(problems(runs[page]))
            lines.append(f"- `{page}`: {why}")
    by_folder: dict[str, list[str]] = {}
    for page in valid:
        by_folder.setdefault(page.split("/")[0], []).append(page)
    for folder, names in sorted(by_folder.items()):
        lines += ["", f"## {folder}", "", "| Page | Unit | machopo | Chrome | machopo slower | Check |",
                  "| --- | --- | ---: | ---: | ---: | --- |"]
        slowdowns = []
        for page in names:
            machopo, chrome = runs[page]["machopo"], runs[page]["Chrome"]
            s = slowdown(machopo, chrome)
            if s and page in trusted:
                slowdowns.append(s)
            check = "; ".join(notes(runs[page]))
            lines.append(
                f"| `{page}` | {machopo['unit']} | {number(common.median(machopo['values']))} | "
                f"{number(common.median(chrome['values']))} | {number(s) if s else ''} |"
                + (f" {check} |" if check else " |")
            )
        if slowdowns:
            lines.append(f"| geometric mean of {len(slowdowns)} | | | | {number(geomean(slowdowns))} | |")
    invalid = [page for page in runs if "excluded" not in runs[page] and page not in valid]
    if invalid:
        lines += ["", "## Not valid", ""]
        lines += [f"- `{page}`: {'; '.join(problems(runs[page]))}" for page in invalid]
    excluded = [page for page in runs if "excluded" in runs[page]]
    if excluded:
        lines += ["", "## Left out", ""]
        lines += [f"- `{page}`: {runs[page]['excluded']}" for page in excluded]
    RESULTS.mkdir(parents=True, exist_ok=True)
    report = RESULTS / "report.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


def run_pages(names: list[str], root: Path, profile: str, timeout: float) -> dict[str, dict[str, dict]]:
    """Each page in machopo, then in Chrome, each in a new browser: what each
    gave (`result`), by page and browser, and whether the page catches
    exceptions (`catches`); a page `scan` leaves out isn't run (`excluded`)."""
    binary = common.machopo_binary(profile)
    server = Server(root)
    runs: dict[str, dict[str, dict]] = {}
    RESULTS.mkdir(parents=True, exist_ok=True)
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for page in names:
                found = scan(root, page)
                if "excluded" in found:
                    runs[page] = found
                    print(f"{page}: left out: {found['excluded']}", flush=True)
                    continue
                url = server.url("/" + page)
                runs[page] = {
                    "machopo": result(common.run_in_machopo(server, binary, url, timeout, log)),
                    "Chrome": result(common.run_in_chrome(server, url, timeout)),
                    "catches": found["catches"],
                }
                why = problems(runs[page])
                s = slowdown(runs[page]["machopo"], runs[page]["Chrome"])
                said = "; ".join(why) if why else f"machopo slower {number(s)}" if s else "valid"
                print(f"{page}: " + "; ".join([said] + ([] if why else notes(runs[page]))), flush=True)
    finally:
        server.close()
    return runs


# --- two versions of machopo -----------------------------------------------------

# The two versions `--against` compares, in the order each run takes them:
# the other first, so the machine's drift falls on both.
SIDES = ("against", "current")


def time_pages(binary: Path, names: list[str], root: Path, timeout: float) -> dict[str, dict]:
    """Each page once in machopo (`binary`), each in a new machopo: what it gave
    (`result`); a page `scan` leaves out isn't run (`excluded`)."""
    server = Server(root)
    timed: dict[str, dict] = {}
    RESULTS.mkdir(parents=True, exist_ok=True)
    try:
        with open(RESULTS / "machopo.log", "w", encoding="utf-8") as log:
            for page in names:
                found = scan(root, page)
                if "excluded" in found:
                    timed[page] = found
                    continue
                timed[page] = result(common.run_in_machopo(server, binary, server.url("/" + page), timeout, log))
                got = timed[page]
                print(f"  {page}: " + (got["error"].splitlines()[0] if "error" in got else
                                       f"{number(common.median(got['values']))} {got['unit']}"), flush=True)
    finally:
        server.close()
    return timed


def version_problem(sides: dict[str, list[dict]]) -> str | None:
    """Why a page has no times to compare: a run of either version without
    values, a unit or a document's summary other than `against`'s first
    run's (the newer version doing less work), or a version not run."""
    reference = None
    for side in SIDES:
        if not sides[side]:
            return f"{side}: not run"
        for got in sides[side]:
            if "error" in got:
                return f"{side}: {got['error'].splitlines()[0]}"
            if reference is None:
                reference = got
                continue
            if got["unit"] != reference["unit"]:
                return f"{side}: in {got['unit']}, against in {reference['unit']}"
            if "summary" not in got or "summary" not in reference:
                return f"{side}: no summary"
            difference = common.summary_difference(got["summary"], reference["summary"], wanted_by="against")
            if difference:
                return f"{side}: the documents differ: {difference}"
    return None


def write_compare_report(rev: str, runs: int, by_page: dict[str, dict], env: dict) -> Path:
    """`compare.md`: for each page, each version's values in all its runs,
    their median and 95% interval, and the medians' change; the geometric
    mean of the pages' times, current over against; the pages without
    times to compare, with why, and those left out; each run's median, in
    the order they ran (a version may land at another level each time).
    No page is called faster or slower: two builds of the same code moved
    a page by up to a third (where its code lies), each build steady over
    its runs, so the intervals (of one build's runs) don't tell."""
    lines = [
        "# machopo against another version, in Blink's perf tests",
        "",
        f"{env['date']}: {common.versions_text(rev)}; {runs} runs each, taking turns, each page in a new machopo; "
        f"{env['os']}, {env['cpu']} (the browsers on CPUs: {env.get('cpus', 'all')}), power: {env['power']}. "
        "Each page's values in all of a version's runs (the runner's iterations): their median (95% interval). "
        "Two builds of the same code can differ by up to a third on a page (where the code lies), however many runs: "
        "read the geometric mean, and a page's change only well beyond that.",
        "",
        "| Page | Unit | against | current | change |",
        "| --- | --- | --- | --- | --- |",
    ]
    times, invalid, excluded, each_run = [], [], [], []
    for page, sides in by_page.items():
        if "excluded" in sides:
            excluded.append(f"- `{page}`: {sides['excluded']}")
            continue
        problem = version_problem(sides)
        if problem:
            invalid.append(f"- `{page}`: {problem}")
            continue
        unit = sides["against"][0]["unit"]
        values = {side: [v for got in sides[side] for v in got["values"]] for side in SIDES}
        before, after = common.median(values["against"]), common.median(values["current"])
        # A page that takes no time to the timer's resolution has no ratio.
        zero = next(((side, median) for side, median in zip(SIDES, (before, after)) if median <= 0), None)
        if zero:
            invalid.append(f"- `{page}`: {zero[0]}: a median of {number(zero[1])} {unit}")
            continue
        if unit in LESS_IS_BETTER:
            times.append(after / before)
        elif unit in MORE_IS_BETTER:
            times.append(before / after)
        lines.append(f"| `{page}` | {unit} | {common.interval_text(values['against'])} | "
                     f"{common.interval_text(values['current'])} | {common.change(before, after)} |")
        each_run.append(f"| `{page}` | " + " | ".join(
            ", ".join(number(common.median(got["values"])) for got in sides[side]) for side in SIDES) + " |")
    mean = geomean(times)
    if mean:
        lines += ["", f"The geometric mean of the {len(times)} pages' times, current over against: {mean:.3f} "
                      f"({100 * (mean - 1):+.1f}%)."]
    if invalid:
        lines += ["", "## Not compared", "", *invalid]
    if excluded:
        lines += ["", "## Left out", "", *excluded]
    lines += ["", "## Each run", "", "Each run's median, in the order they ran.", "",
              "| Page | against | current |", "| --- | --- | --- |", *each_run]
    RESULTS.mkdir(parents=True, exist_ok=True)
    report = RESULTS / "compare.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return report


def compare_versions(binaries: dict[str, Path], names: list[str], root: Path, runs: int,
                     timeout: float) -> dict[str, dict]:
    """The two versions (`binaries`, by side) in turn, `runs` times each
    (a run: every page once), the other first: by page, each side's
    results in the order they ran, or why it was left out."""
    by_page: dict[str, dict] = {}
    for run in range(1, runs + 1):
        for side in SIDES:
            print(f"{side}, run {run} of {runs}:", flush=True)
            for page, got in time_pages(binaries[side], names, root, timeout).items():
                if "excluded" in got:
                    by_page[page] = got
                else:
                    by_page.setdefault(page, {s: [] for s in SIDES})[side].append(got)
    return by_page


def cmd_against(args, names: list[str], root: Path) -> int:
    """machopo at `args.against` and the working tree's (`against_binaries`),
    timed in turn `args.runs` times each; `compare.md`."""
    problems_found = common.against_problems(args.against)
    if problems_found:
        print("\n".join(problems_found))
        return 1
    env = dict(common.environment(common.MACHOPO_PROFILE), date=date.today().isoformat())
    if env["power"] == "battery":
        print("warning: on battery power, the times may not be comparable")
    binaries = common.against_binaries(args.against)
    by_page = compare_versions(binaries, names, root, args.runs, args.timeout)
    (RESULTS / "compare.json").write_text(json.dumps({"environment": env, "pages": by_page}, indent=1) + "\n",
                                          encoding="utf-8", newline="\n")
    print(f"the comparison: {write_compare_report(args.against, args.runs, by_page, env)}")
    return 0


def cmd_blink_perf(args) -> int:
    root = BLINK_PERF
    if not (root / "resources" / "runner.js").is_file():
        print(f"no Blink perf tests at {root}: `python3 scripts/ci.py third-party`")
        return 1
    dirs = [d for d in (args.dirs or "").split(",") if d]
    chosen = [p for p in (args.pages or "").split(",") if p]
    on_list = listed(PAGES)
    if not dirs and not chosen:
        chosen = under(on_list, args.areas) if args.areas else on_list
        if not chosen:
            print(f"no pages in {PAGES}" + (f" under {' '.join(args.areas)}" if args.areas else "")
                  + ": --dirs or --pages")
            return 1
    names = pages(root, dirs, chosen)
    if args.against:
        print(f"machopo at {args.against} and the working tree's: {len(names)} pages, {args.runs} runs each")
        return cmd_against(args, names, root)
    print(f"machopo and Chrome: {len(names)} pages")
    runs = run_pages(names, root, common.MACHOPO_PROFILE, args.timeout)
    env = dict(common.environment(common.MACHOPO_PROFILE), date=date.today().isoformat(),
               blink_perf=(root / ".machopo-rev").read_text(encoding="utf-8").strip()[:12]
               if (root / ".machopo-rev").is_file() else "unknown")
    (RESULTS / "runs.json").write_text(json.dumps({"environment": env, "pages": runs}, indent=1) + "\n",
                                       encoding="utf-8", newline="\n")
    print(f"the report: {write_report(runs, env, set(on_list))}")
    return 0
