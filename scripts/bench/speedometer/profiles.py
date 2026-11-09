"""`bench.py speedometer --profile`'s reading of DevTools' `.cpuprofile`s
(`Profiler.stop`'s `profile`, machopo's and Chrome's alike) and its report:
where each browser's time went, by the page's JS functions.

A sample counts for the innermost function on its stack that has a URL
(the page's code): an API function (a DOM call), a getter or setter, a
function without one (a builtin, the runner's eval'd code), and Chrome's
Fast API calls, which are no nodes of their own, all count for the JS
function that called them, so that the two browsers compare alike. With no
such function on the stack, a sample is `(program)` (in a task, not in
script), `(idle)` (waiting for the next task), `(garbage collector)`, or
`(no JS function)`. machopo's API functions are also counted by name, with
the JS function that called them (`members`): Chrome's Fast API calls
have no nodes, so that table is machopo's only. JS without a URL (a string
handler's, or an older machopo's `glue.js`, which `--against` may run) is
looked through to the API function it called.

Only the windows the runner times count (its marks', on the page's
clock): the page's clock function (`common.PAGE_CLOCK`), its samples and
when it ran on that clock (`pageClock`), puts them on the profile's. The
rest of the run (loading, between iterations) is `(outside the timed
windows)`.
"""

from __future__ import annotations

import bisect
import collections
import json
from pathlib import Path
from urllib.parse import urlsplit

from common import PAGE_CLOCK

# What V8 names the samples that aren't in script.
PROGRAM = "(program)"
IDLE = "(idle)"
GC = "(garbage collector)"
ROOT = "(root)"
# A sample whose stack has no JS function with a URL, nor one of the above.
NO_FUNCTION = "(no JS function)"
# machopo's API functions without a name: its members on the shared path
# (overloads, variadic...), which share one callback, and so one node
# named as whichever of them V8 logged last (none, so far).
SHARED_PATH = "(machopo's shared path)"
# A sample's time out of the windows the runner times.
OUTSIDE = "(outside the timed windows)"
STATES = (PROGRAM, IDLE, GC)
# V8's nodes for a regular expression's code: not an API function.
REGEXP = "RegExp: "
# JS without a URL (an older machopo's `glue.js`) calling nothing native.
SCRIPT = "(script without a URL)"
# The rows that aren't a JS function's.
NOT_FUNCTIONS = (*STATES, NO_FUNCTION, OUTSIDE)

# How many rows each table of the report shows.
SUITE_ROWS = 10
TOTAL_ROWS = 30


def function_key(call_frame: dict) -> str | None:
    """A JS function by its name and place: `name path:line:column`
    (1-based), the URL's path only, so that both browsers' (the same
    server's) are the same; `None` for one without a URL."""
    url = call_frame.get("url")
    if not url:
        return None
    parts = urlsplit(url)
    path = parts.path + (f"?{parts.query}" if parts.query else "")
    name = call_frame.get("functionName") or "(anonymous)"
    return f"{name} {path}:{call_frame.get('lineNumber', 0) + 1}:{call_frame.get('columnNumber', 0) + 1}"


def attribute(profile: dict, windows: list[tuple[float, float]] | None = None
              ) -> tuple[collections.Counter, collections.Counter]:
    """The profile's time (ms) by JS function (`function_key`) and by
    state (`STATES`, `NO_FUNCTION`); and the API functions' (nodes without
    a URL under a JS function) by `(name, caller)`. A sample's time is its
    `timeDeltas` entry: the time since the one before it. With `windows`
    (on the profile's clock, `on_profile`), only the part of a sample's
    time in them counts; the rest is `OUTSIDE`."""
    nodes = {node["id"]: node for node in profile["nodes"]}
    parents: dict[int, int] = {}
    for node in profile["nodes"]:
        for child in node.get("children", []):
            parents[child] = node["id"]
    by_function: collections.Counter = collections.Counter()
    members: collections.Counter = collections.Counter()
    leaf_of: dict[int, tuple[str | None, str | None]] = {}
    time = profile["startTime"]
    for sample, delta in zip(profile["samples"], profile["timeDeltas"]):
        time += delta
        inside = delta if windows is None else overlap(windows, time - delta, time)
        if inside < delta:
            by_function[OUTSIDE] += (delta - inside) / 1000
        if not inside:
            continue
        if sample not in leaf_of:
            leaf_of[sample] = place(sample, nodes, parents)
        function, member = leaf_of[sample]
        by_function[function or NO_FUNCTION] += inside / 1000
        if member is not None and function is not None:
            members[(member, function)] += inside / 1000
    return by_function, members


def overlap(windows: list[tuple[float, float]], start: float, end: float) -> float:
    """How much of `start`..`end` is in `windows` (sorted, apart)."""
    total = 0.0
    i = bisect.bisect_left(windows, (end,))
    while i > 0 and windows[i - 1][1] > start:
        i -= 1
        total += max(0.0, min(end, windows[i][1]) - max(start, windows[i][0]))
    return total


def page_clock(profile: dict) -> tuple[float, float]:
    """Where the page's clock (ms) is on the profile's (µs): the earliest
    and the latest the page's 0 may be. Each sample in `PAGE_CLOCK` was
    taken while it ran, from `pageClock`'s `start` to its `end`.
    `ValueError` if none was, or they don't fit in that time."""
    nodes = {node["id"]: node for node in profile["nodes"]}
    parents = {child: node["id"] for node in profile["nodes"] for child in node.get("children", [])}
    clock = {node["id"] for node in profile["nodes"] if node["callFrame"].get("functionName") == PAGE_CLOCK}
    inside: dict[int, bool] = {}

    def in_clock(node_id: int) -> bool:
        if node_id not in inside:
            parent = parents.get(node_id)
            inside[node_id] = node_id in clock or (parent is not None and in_clock(parent))
        return inside[node_id]

    times = []
    time = profile["startTime"]
    for sample, delta in zip(profile["samples"], profile["timeDeltas"]):
        time += delta
        if sample in nodes and in_clock(sample):
            times.append(time)
    if not times:
        raise ValueError(f"no sample in {PAGE_CLOCK}")
    ran = profile["pageClock"]
    earliest, latest = max(times) - ran["end"] * 1000, min(times) - ran["start"] * 1000
    if earliest > latest:
        raise ValueError(f"{PAGE_CLOCK}'s samples are out of the time it ran")
    return earliest, latest


def on_profile(profile: dict, windows: list[tuple[float, float]]) -> list[tuple[float, float]]:
    """`windows` (ms on the page's clock) on the profile's clock (µs), by
    the middle of where `page_clock` puts the page's 0."""
    earliest, latest = page_clock(profile)
    zero = (earliest + latest) / 2
    return sorted((a * 1000 + zero, b * 1000 + zero) for a, b in windows)


def uncertainty(profile: dict) -> float:
    """How far off (ms, at most) `on_profile` may put a window."""
    earliest, latest = page_clock(profile)
    return (latest - earliest) / 2000


def place(node_id: int, nodes: dict[int, dict], parents: dict[int, int]) -> tuple[str | None, str | None]:
    """Where a sample at `node_id` counts: its innermost JS function with a
    URL (or one of `STATES`), and the API function it was in, if any: the
    native node (no line) right under that function, past JS without a
    URL (an older machopo's `glue.js`, which wrapped some members), or that JS if it
    called none; V8's regular expressions aren't API functions."""
    member = None
    script = None
    current: int | None = node_id
    while current is not None:
        call_frame = nodes[current]["callFrame"]
        key = function_key(call_frame)
        if key is not None:
            return key, member or script
        name = call_frame.get("functionName", "")
        if current == node_id and name in STATES:
            return name, None
        if name.startswith(REGEXP):
            pass
        elif call_frame.get("lineNumber", -1) < 0:
            member = name or SHARED_PATH
        else:
            script = f"{name or '(anonymous)'} {SCRIPT}"
        current = parents.get(current)
    return None, None


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def per_iteration(counter: collections.Counter, iterations: int) -> collections.Counter:
    return collections.Counter({key: value / iterations for key, value in counter.items()})


def suite_rows(machopo: collections.Counter, chrome: collections.Counter, rows: int) -> list[tuple[str, float, float]]:
    """The JS functions (not the states) that differ most: machopo's extra
    time first, then the most Chrome takes more on, `(name, machopo, Chrome)`."""
    functions = [key for key in set(machopo) | set(chrome) if key not in NOT_FUNCTIONS]
    by_difference = sorted(functions, key=lambda key: machopo[key] - chrome[key], reverse=True)
    more = [key for key in by_difference if machopo[key] - chrome[key] > 0][:rows]
    less = [key for key in reversed(by_difference) if machopo[key] - chrome[key] < 0][: max(1, rows // 2)]
    return [(key, machopo[key], chrome[key]) for key in more + list(reversed(less))]


def ms(value: float) -> str:
    return f"{value:.1f}"


def difference(machopo: float, chrome: float) -> str:
    return f"{machopo - chrome:+.1f}"


def summary_rows(machopo: collections.Counter, chrome: collections.Counter) -> list[str]:
    """The table of where each browser's time went: in JS functions, in
    each state, and out of the timed windows."""
    lines = ["| | machopo | Chrome | machopo − Chrome |", "| --- | ---: | ---: | ---: |"]
    js = {side: sum(v for k, v in counter.items() if k not in NOT_FUNCTIONS)
          for side, counter in (("machopo", machopo), ("Chrome", chrome))}
    lines.append(f"| JS functions | {ms(js['machopo'])} | {ms(js['Chrome'])} | {difference(js['machopo'], js['Chrome'])} |")
    for state in (*STATES, NO_FUNCTION):
        lines.append(f"| `{state}` | {ms(machopo[state])} | {ms(chrome[state])} | {difference(machopo[state], chrome[state])} |")
    timed = {side: sum(v for k, v in counter.items() if k != OUTSIDE) for side, counter in (("machopo", machopo), ("Chrome", chrome))}
    lines.append(f"| **Timed** | {ms(timed['machopo'])} | {ms(timed['Chrome'])} | {difference(timed['machopo'], timed['Chrome'])} |")
    lines.append(f"| `{OUTSIDE}` | {ms(machopo[OUTSIDE])} | {ms(chrome[OUTSIDE])} | {difference(machopo[OUTSIDE], chrome[OUTSIDE])} |")
    return lines


def write_report(results: Path, suites: dict[str, dict], iterations: int, env: dict) -> Path:
    """`profile.md` (and `profile.json`) in `results`: for each suite, its
    `machopo` and `Chrome` profiles' time in the timed windows by JS function
    and state, and out of them (ms an iteration); the run's median time
    an iteration (`times`); how far off the windows may be
    (`uncertainty`, ms); machopo's DOM members; and every suite's together.
    `profile.json` has the whole run's time too (`whole`)."""
    totals = {"machopo": collections.Counter(), "Chrome": collections.Counter()}
    every_function: list[tuple[str, str, float, float]] = []
    member_totals: collections.Counter = collections.Counter()
    callers: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    sections = []
    for name, suite in suites.items():
        machopo, chrome = suite["machopo"], suite["Chrome"]
        lines = [f"### {name}", ""]
        times = suite.get("times", {})
        if times:
            lines.append(f"Profiled: machopo {ms(times.get('machopo', 0))} ms, Chrome {ms(times.get('Chrome', 0))} ms an "
                         "iteration (the run's median).")
        off = suite.get("uncertainty", {})
        if off:
            lines.append(f"The windows placed within {max(off.values()):.2f} ms.")
        if times or off:
            lines.append("")
        lines += summary_rows(machopo, chrome)
        lines += ["", "| Function | machopo | Chrome | machopo − Chrome |", "| --- | ---: | ---: | ---: |"]
        for key, k, c in suite_rows(machopo, chrome, SUITE_ROWS):
            lines.append(f"| `{key}` | {ms(k)} | {ms(c)} | {difference(k, c)} |")
        sections.append("\n".join(lines))
        totals["machopo"].update(machopo)
        totals["Chrome"].update(chrome)
        for key in set(machopo) | set(chrome):
            if key not in NOT_FUNCTIONS:
                every_function.append((name, key, machopo[key], chrome[key]))
        for (member, caller), value in suite["members"].items():
            member_totals[member] += value
            callers[member][f"{caller} ({name})"] += value

    lines = [
        "# Speedometer 3.1 by JS function in machopo and Chrome",
        "",
        f"machopo `{env.get('machopo', '')}`, Chrome for Testing {env.get('chrome', '')}; {env.get('system', '')}. "
        f"DevTools' `Profiler` in each, sampling every {env.get('interval', '')} µs: {iterations} iterations, ms an "
        "iteration in the windows the runner times (each test's sync and async time, by its marks); the rest of the "
        f"run (loading, between iterations) is `{OUTSIDE}`. A function's time is its own and its calls' "
        "into the browser (DOM calls, getters and setters, Chrome's Fast API calls). `(program)` is in a task out of "
        "script, `(idle)` waiting for the next one.",
        "",
        "## Every suite",
        "",
    ]
    lines += summary_rows(totals["machopo"], totals["Chrome"])
    lines += ["", "The functions machopo takes most more on:", "",
              "| Suite | Function | machopo | Chrome | machopo − Chrome |", "| --- | --- | ---: | ---: | ---: |"]
    for suite, key, k, c in sorted(every_function, key=lambda row: row[3] - row[2])[:TOTAL_ROWS]:
        lines.append(f"| {suite} | `{key}` | {ms(k)} | {ms(c)} | {difference(k, c)} |")
    lines += ["", "## machopo's DOM members", "",
              "machopo's only, not compared: Chrome's Fast API calls have no nodes of their own. A member's time, and "
              "the JS functions that called it most. machopo's members on its shared path (overloads such as canvas's "
              f"`fill` and `stroke`, variadic ones...) share one callback, and so one node: `{SHARED_PATH}`, or "
              "named as any one of them, a member or not (`bytes`, `Blob`...). Through JS without a URL (an older machopo's "
              f"`glue.js`), the member it called; `{SCRIPT}` for time in that JS itself.", "",
              "| Member | machopo | Called from |", "| --- | ---: | --- |"]
    for member, value in member_totals.most_common(TOTAL_ROWS):
        top = ", ".join(f"`{caller}` {ms(v)}" for caller, v in callers[member].most_common(3))
        lines.append(f"| `{member}` | {ms(value)} | {top} |")
    lines += ["", "## Each suite", "", "\n\n".join(sections), ""]
    report = results / "profile.md"
    report.write_text("\n".join(lines), encoding="utf-8")
    (results / "profile.json").write_text(json.dumps({
        name: {
            side: dict(suite[side]) for side in ("machopo", "Chrome")
        } | {"members": {f"{m} <- {c}": v for (m, c), v in suite["members"].items()}, "times": suite.get("times", {}),
             "uncertainty": suite.get("uncertainty", {}),
             "whole": {side: dict(counter) for side, counter in suite.get("whole", {}).items()}}
        for name, suite in suites.items()
    }, indent=1), encoding="utf-8")
    return report
