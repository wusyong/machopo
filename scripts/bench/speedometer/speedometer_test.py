"""Tests of `bench.py speedometer` (`speedometer.py` and the scripts it adds to
pages), on `scripts/bench/speedometer/fake/`: a stand-in for Speedometer's runner
that calls the same hooks around a few small suites. The browsers are the
real ones: machopo (built here, the dev profile) and Chrome for Testing (skipped
if it isn't downloaded yet: `bench.py speedometer` downloads it).

    python3 scripts/bench/speedometer/speedometer_test.py [-k NAME]
"""

from __future__ import annotations

import argparse
import collections
import gzip
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import speedometer  # noqa: E402
import categories  # noqa: E402
import common  # noqa: E402
import profiles  # noqa: E402

FAKE = Path(__file__).resolve().parent / "fake"


def setUpModule():
    # No test runs anything as Administrator: on Windows that would ask
    # the user through UAC, in the middle of the tests.
    def refused(arguments, log):
        raise AssertionError("a test asked to run as Administrator")

    patch = mock.patch.object(speedometer, "run_elevated", refused)
    patch.start()
    unittest.addModuleCleanup(patch.stop)

# A suite's deadline in the page (past `measure.mjs`'s STALL, 5 s), and the
# time its browser gets.
DEADLINE_MS = 8000
TIMEOUT = 30


def summary(elements: dict, classes: dict | None = None, text: int = 0) -> dict:
    return {"elements": elements, "classes": classes or {}, "text": text}


def run(summaries: list[dict], status: str = "done", errors: list | None = None, current=None) -> dict:
    return {"status": status, "current": current, "summaries": summaries, "errors": errors or [], "iterations": []}


class SummaryTests(unittest.TestCase):
    def test_the_first_difference_is_told(self):
        want = summary({"li": 3, "ul": 1}, {"item": 3}, 9)
        self.assertIsNone(speedometer.summary_difference(summary({"ul": 1, "li": 3}, {"item": 3}, 9), want))
        self.assertEqual(
            speedometer.summary_difference(summary({"ul": 1}, {"item": 3}, 9), want), "0 `li` elements, Chrome 3"
        )
        self.assertEqual(
            speedometer.summary_difference(summary({"li": 3, "ul": 1, "p": 1}, {"item": 3}, 9), want),
            "1 `p` elements, Chrome 0",
        )
        self.assertEqual(
            speedometer.summary_difference(summary({"li": 3, "ul": 1}, {"item": 2}, 9), want),
            "2 elements with the class `item`, Chrome 3",
        )
        self.assertEqual(
            speedometer.summary_difference(summary({"li": 3, "ul": 1}, {"item": 3}, 8), want),
            "text 8 characters long, Chrome 9",
        )

    def test_made_up_class_names_are_the_same_class(self):
        want = summary({}, {"plot-7017677fbe27d8": 1, "plot-a": 1, "x": 2})
        self.assertIsNone(
            speedometer.summary_difference(summary({}, {"plot-09d82426c279b": 1, "plot-a": 1, "x": 2}), want)
        )
        # Two made up differently are two of the same class.
        self.assertIsNone(
            speedometer.summary_difference(
                summary({}, {"plot-aaaaaaaaaa1": 1, "plot-bbbbbbbbbb2": 1}), summary({}, {"plot-cccccccccc3": 2})
            )
        )
        # Two of them are two, and a short run of hex digits is a name.
        self.assertEqual(
            speedometer.summary_difference(summary({}, {"plot-0123456789": 2, "plot-a": 1, "x": 2}), want),
            "2 elements with the class `plot-*`, Chrome 1",
        )
        self.assertEqual(
            speedometer.summary_difference(summary({}, {"plot-7017677fb": 1, "plot-a": 1, "x": 2}), want),
            "0 elements with the class `plot-*`, Chrome 1",
        )

    def test_classes_set_in_a_later_frame_are_not_compared(self):
        # TipTap's editor focused in a frame after the runner ended the test
        # in one iteration, before it in another: the same work.
        want = summary({"div": 1}, {"ProseMirror": 1, "ProseMirror-focused": 1})
        self.assertIsNone(speedometer.summary_difference(summary({"div": 1}, {"ProseMirror": 1}), want))
        self.assertEqual(
            speedometer.summary_difference(summary({"div": 1}, {}), want),
            "0 elements with the class `ProseMirror`, Chrome 1",
        )

    def test_a_run_that_never_had_a_timing_class_is_noted(self):
        focused = {"t/a": summary({}, {"ProseMirror-focused": 1})}
        unfocused = {"t/a": summary({}, {})}
        # Chrome's page focused in some iterations: nothing to note.
        self.assertIsNone(speedometer.timing_note({"Chrome": run([focused, unfocused]), "machopo": run([focused])}))
        self.assertEqual(
            speedometer.timing_note({"Chrome": run([unfocused, unfocused]), "machopo": run([focused])}),
            "Chrome's page never had `ProseMirror-focused`, machopo's did: it did less",
        )
        # Neither had it (a suite without an editor), or one browser's run alone.
        self.assertIsNone(speedometer.timing_note({"Chrome": run([unfocused]), "machopo": run([unfocused])}))
        self.assertIsNone(speedometer.timing_note({"machopo": run([focused])}))
        # The note goes with the suite's row, which keeps its numbers.
        rows = speedometer.compare(["t"], {"t": run([unfocused])}, {"t": run([focused])}, None)
        self.assertIsNone(rows[0]["problem"])
        self.assertEqual(rows[0]["note"], "Chrome's page never had `ProseMirror-focused`, machopo's did: it did less")

    def test_a_run_is_valid_when_it_ends_cleanly_with_chromes_documents(self):
        chrome = run([{"s/a": summary({"li": 3})}, {"s/a": summary({"li": 3})}])
        self.assertEqual(speedometer.run_problems(run([{"s/a": summary({"li": 3})}] * 3), chrome), [])
        # Nothing to compare with: only how it ended.
        self.assertEqual(speedometer.run_problems(run([{"s/a": summary({"li": 1})}]), None), [])
        self.assertEqual(speedometer.run_problems(run([{"s/a": summary({"li": 1})}]), {"error": "x"}), [])

    def test_what_makes_a_run_invalid(self):
        chrome = run([{"s/a": summary({"li": 3}), "s/b": summary({"li": 2})}])
        self.assertEqual(speedometer.run_problems({"error": "machopo ended"}, chrome), ["machopo ended"])
        self.assertEqual(
            speedometer.run_problems(run([], status="stalled", current="s/a"), chrome), ["stalled in s/a"]
        )
        self.assertEqual(speedometer.run_problems(run([], status="timeout"), chrome), ["timeout"])
        errors = [
            {"kind": "error", "page": "/s.html", "message": "Error: x\n    at f"},
            {"kind": "error", "page": "/s.html", "message": "Error: y"},
        ]
        self.assertEqual(
            speedometer.run_problems(run([{"s/a": summary({"li": 3})}], errors=errors), chrome),
            ["error in /s.html: Error: x"],
        )
        # A later iteration's, a later test's: the first difference.
        got = [
            {"s/a": summary({"li": 3}), "s/b": summary({"li": 2})},
            {"s/a": summary({"li": 3}), "s/b": summary({"li": 1})},
            {"s/a": summary({"li": 0}), "s/b": summary({"li": 1})},
        ]
        self.assertEqual(
            speedometer.run_problems(run(got), chrome), ["iteration 1, s/b: 1 `li` elements, Chrome 2"]
        )
        self.assertEqual(
            speedometer.run_problems(run([{"s/c": summary({})}]), chrome), ["iteration 0, s/c: Chrome has no summary"]
        )


def timed(totals: list[float], suite: str = "s") -> dict:
    """A run whose iterations' totals for `suite` are `totals`, each test
    `a` half sync and half async."""
    iterations = [{suite: {"tests": {"a": {"tests": {"Sync": t / 2, "Async": t / 2}, "total": t}}, "total": t}}
                  for t in totals]
    return dict(run([{f"{suite}/a": summary({"li": 1})}] * len(totals)), iterations=iterations)


# The steps of machopo's main frame, each in the span it is in, named as
# Chrome's trace has them where Chrome has the step, machopo's own otherwise.
# (The lifecycle's own spans are also where script forces a layout.)
FRAME_STEPS = {
    "WebFrameWidgetImpl::BeginMainFrame": "ProxyMain::BeginMainFrame",
    "PageAnimator::serviceScriptedAnimations": "WebFrameWidgetImpl::BeginMainFrame",
    "LocalFrameView::NotifyResizeObservers": "ProxyMain::BeginMainFrame",
    "WebFrameWidgetImpl::UpdateLifecycle": "ProxyMain::BeginMainFrame",
    "canvas::flush_shown": "WebFrameWidgetImpl::UpdateLifecycle",
    "PendingAnimations::Update": "ProxyMain::BeginMainFrame",
    "LocalFrameView::UpdateViewportIntersectionsForSubtree": "ProxyMain::BeginMainFrame",
    "LayerTreeHost::DoUpdateLayers": "ProxyMain::BeginMainFrame",
    "ProxyMain::BeginMainFrame::commit": "ProxyMain::BeginMainFrame",
    "MouseEventManager::RecomputeMouseHoverState": "ProxyMain::BeginMainFrame",
    "FrameView::take_canvas_textures": "LocalFrameView::UpdateLifecyclePhases",
    "paint::main_thread_scroll_hit_test_region": "LocalFrameView::UpdateLifecyclePhases",
    "PaintArtifactCompositor::Update": "LocalFrameView::UpdateLifecyclePhases",
    "FrameView::set_wheel_event_handler_region": "LocalFrameView::UpdateLifecyclePhases",
}

# The steps a frame takes only when it has something to do: the resize
# observers', with observations; the hover state's, after a layout, with
# the mouse over the page. In the trace, in their span, if they ran.
WHEN_NEEDED_STEPS = {"LocalFrameView::NotifyResizeObservers", "MouseEventManager::RecomputeMouseHoverState"}


def perfetto_threads(path: Path) -> dict[str, list[str | None]]:
    """The slices of a Perfetto trace by thread name: each thread's begins
    (the slice's name) and ends (`None`) in time order."""
    return {thread: [name for _, name in slices] for thread, slices in speedometer.perfetto_slices(path).items()}


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        results = tempfile.TemporaryDirectory()
        self.addCleanup(results.cleanup)
        patch = mock.patch.object(speedometer, "RESULTS", Path(results.name))
        patch.start()
        self.addCleanup(patch.stop)

    def test_the_browsers_take_turns_suite_by_suite(self):
        self.assertEqual(
            speedometer.schedule(["s1", "s2"], ["machopo", "Chrome"]),
            [("s1", "machopo"), ("s1", "Chrome"), ("s2", "machopo"), ("s2", "Chrome")],
        )
        self.assertEqual(speedometer.schedule(["s1", "s2"], ["machopo"]), [("s1", "machopo"), ("s2", "machopo")])

    def test_medians(self):
        self.assertEqual(speedometer.median([3, 1, 2]), 2)
        self.assertEqual(speedometer.median([4, 1, 3, 2]), 2.5)

    def test_the_medians_interval_takes_order_statistics(self):
        # 30 values: x(10) and x(21), 95.7%.
        self.assertEqual(speedometer.median_interval(list(range(30, 0, -1))), (10, 21))
        # 10 values (the default iterations): x(2) and x(9), 97.9%.
        self.assertEqual(speedometer.median_interval(list(range(10, 0, -1))), (2, 9))
        # 6 values: the smallest and the largest, 96.9%; 5 have none.
        self.assertEqual(speedometer.median_interval([6, 5, 4, 3, 2, 1]), (1, 6))
        self.assertIsNone(speedometer.median_interval([5, 4, 3, 2, 1]))
        # 9 values: x(2) and x(8), 96.1%; 99% takes x(1) and x(9).
        self.assertEqual(speedometer.median_interval(list(range(1, 10))), (2, 8))
        self.assertEqual(speedometer.median_interval(list(range(1, 10)), 0.99), (1, 9))

    def test_samples_are_every_iteration(self):
        self.assertEqual(speedometer.suite_samples(timed([9, 2, 3]), "s"), [9, 2, 3])
        self.assertEqual(speedometer.test_samples(timed([9, 2, 3]), "s", "a", "Async"), [4.5, 1, 1.5])

    def test_either_run_not_valid_leaves_the_suite_without_numbers(self):
        chrome, machopo = timed([2, 2]), timed([4, 4])
        rows = speedometer.compare(["s"], {"s": chrome}, {"s": dict(machopo, status="stalled")}, {"s": chrome})
        self.assertEqual(rows, [{"suite": "s", "problem": "machopo: stalled", "runs": {}, "note": None}])
        # Chrome's later iterations are checked against its first.
        changing = dict(chrome, summaries=[{"s/a": summary({"li": 1})}, {"s/a": summary({"li": 2})}])
        rows = speedometer.compare(["s"], {"s": changing}, {"s": machopo}, {"s": changing})
        self.assertEqual(rows[0]["problem"], "Chrome: iteration 1, s/a: 2 `li` elements, Chrome 1")
        rows = speedometer.compare(["s"], {"s": chrome}, {"s": machopo}, {"s": chrome})
        self.assertEqual((rows[0]["problem"], rows[0]["runs"]), (None, {"Chrome": chrome, "machopo": machopo}))
        # machopo alone, checked against Chrome's earlier run.
        other = dict(timed([2, 2]), summaries=[{"s/a": summary({"li": 2})}] * 2)
        rows = speedometer.compare(["s"], None, {"s": machopo}, {"s": other})
        self.assertEqual(rows[0]["problem"], "machopo: iteration 0, s/a: 1 `li` elements, Chrome 2")
        # A suite Chrome's earlier run didn't have: nothing to check machopo's by.
        rows = speedometer.compare(["s", "t"], None, {"s": machopo, "t": machopo}, {"s": chrome})
        self.assertEqual([row["problem"] for row in rows], [None, "no run of Chrome's to check against"])

    def test_the_report_has_each_suites_medians_and_each_tests(self):
        rows = [
            {
                "suite": "s",
                "problem": None,
                "runs": {"machopo": timed([30, 10, 10]), "Chrome": timed([12, 4, 5])},
                "note": "Chrome's page never had `x`",
            },
            {"suite": "t", "problem": "machopo: stalled", "runs": {}},
        ]
        env = speedometer.environment("perf")
        lines = speedometer.write_measure_report(rows, env, 3).read_text(encoding="utf-8").splitlines()
        self.assertIn("| s | 10.0 | 5.0 | 2.00 | 30.0, 12.0 |", lines)
        self.assertIn("- s: Chrome's page never had `x`", lines)
        self.assertIn("| t | not valid: machopo: stalled | | | |", lines)
        self.assertIn("| s | a | 5.0 | 5.0 | 2.5 | 2.5 |", lines)

    def test_the_environment(self):
        env = speedometer.environment("perf")
        self.assertEqual((env["profile"], env["chrome"]), ("perf", speedometer.CHROME_VERSION))
        self.assertIn(env["power"], ("AC", "battery", "unknown"))
        self.assertGreater(env["cores"], 0)
        self.assertTrue(env["os"] and env["cpu"])
        self.assertEqual(env["gpu"], "unknown")

    def test_machopo_on_the_cpu_adapter_is_a_problem(self):
        gpu = {"device": "AMD Radeon", "type": "DiscreteGpu", "backend": "Dx12"}
        self.assertIsNone(speedometer.gpu_problem(gpu))
        self.assertEqual(speedometer.describe_gpu(gpu), "AMD Radeon (DiscreteGpu, Dx12)")
        warp = {"device": "Microsoft Basic Render Driver", "type": "Cpu", "backend": "Dx12"}
        self.assertIn("CPU adapter", speedometer.gpu_problem(warp))
        self.assertIn("didn't tell", speedometer.gpu_problem(None))
        self.assertEqual(speedometer.describe_gpu(None), "unknown")
        # A machopo that doesn't start tells nothing.
        self.assertIsNone(speedometer.machopo_gpu(Path("no-such-machopo")))


class InventoryReportTests(unittest.TestCase):
    """`probe_list` and `write_report` on made-up recordings."""

    def chrome(self) -> dict:
        suite = {
            "api": {"element:svg|circle:SVGCircleElement": 1, "element:my-list:MyList": 1},
            "declarations": {"color: red": 1},
            "selectors": {".a:hover::placeholder, .b": 1, ".c": 1},
            "atRules": {"CSSMediaRule": 2, "CSSKeyframesRule": 1},
            "atRuleTexts": {"CSSMediaRule": "@media print { }", "CSSKeyframesRule": "@keyframes k { }"},
        }
        return {"runner": {}, "suites": {"one": suite}, "status": "done", "errors": []}

    def test_the_probe_list_has_each_kind_of_at_rule_with_a_text(self):
        got = speedometer.probe_list(self.chrome())
        self.assertEqual(got["atRules"], {"CSSMediaRule": "@media print { }", "CSSKeyframesRule": "@keyframes k { }"})
        # Only the platform's elements.
        self.assertEqual(got["elements"], ["element:svg|circle:SVGCircleElement"])

    def test_the_report_lists_only_what_machopo_lacks(self):
        missing = {
            "api": [],
            "declarations": [],
            "selectors": [".a:hover::placeholder, .b"],
            "culprits": {".a:hover::placeholder, .b": ["::placeholder"]},
            "atRules": ["CSSKeyframesRule"],
            "elements": [],
        }
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(speedometer, "RESULTS", Path(folder)):
            speedometer.write_report(self.chrome(), missing, {}, [("one", True)])
            report = (Path(folder) / "inventory.md").read_text(encoding="utf-8")
            data = json.loads((Path(folder) / "inventory.json").read_text(encoding="utf-8"))
        self.assertIn("- Selectors machopo can't parse (1), for: `::placeholder`\n", report)
        self.assertNotIn(":hover", report)
        self.assertIn("- At-rules machopo drops: `CSSKeyframesRule`\n", report)
        self.assertNotIn("CSSMediaRule", report)
        self.assertEqual(data["suites"]["one"]["at_rules"], ["CSSKeyframesRule"])


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.server = speedometer.Server(FAKE)

    def tearDown(self):
        self.server.close()

    def page(self, path: str) -> str:
        body, _ = self.server.get(path)
        return body.decode()

    def test_the_runners_module_follows_main(self):
        main = '<script src="resources/main.mjs" type="module"></script>'
        for mode, module in (("measure", "measure.mjs"), ("inventory", "machopo.mjs"), ("record", "inventory.mjs")):
            self.server.mode = mode
            page = self.page("/index.html")
            self.assertIn(main + f'<script type="module" src="/__machopo__/{module}"></script>', page, mode)
            self.assertEqual(page.count("type=\"module\""), 2, mode)

    def test_every_page_collects_its_errors_first(self):
        self.server.mode = "measure"
        for path in ("/index.html", "/suites/list.html"):
            page = self.page(path)
            self.assertNotIn("record.js", page)
            self.assertLess(page.index("/__machopo__/errors.js"), page.index("<title>"), path)
        self.server.mode = "record"
        page = self.page("/suites/list.html")
        self.assertIn('<script src="/__machopo__/errors.js"></script><script src="/__machopo__/record.js"></script>', page)

    def test_the_deadline_is_written_into_the_modules(self):
        self.server.deadline = 1234
        for name in ("measure.mjs", "machopo.mjs"):
            self.assertIn("const DEADLINE = 1234;", self.page(f"/__machopo__/{name}"), name)

    def test_the_summary_measure_mjs_imports(self):
        self.assertIn("export function summarize(doc)", self.page(speedometer.SUMMARY))
        self.assertIn(f'import {{ summarize }} from "{speedometer.SUMMARY}";', self.page("/__machopo__/measure.mjs"))

    def test_only_files_under_its_root(self):
        self.assertEqual(self.server.get("/../speedometer_test.py"), (None, ""))
        self.assertEqual(self.server.get("/__machopo__/nothing.js"), (None, ""))
        self.assertEqual(self.server.get("/suites/")[0], None)


# A browser's end of the pipe: before each answer, an event and, for
# `Twice.answer`, an answer to no one; `Browser.close` answers and ends.
FAKE_BROWSER = r"""
import json, sys
def send(message):
    sys.stdout.buffer.write(json.dumps(message).encode() + b"\0")
    sys.stdout.buffer.flush()
buffer = b""
while True:
    chunk = sys.stdin.buffer.read1(65536)
    if not chunk:
        break
    buffer += chunk
    while b"\0" in buffer:
        line, buffer = buffer.split(b"\0", 1)
        command = json.loads(line)
        method = command["method"]
        send({"method": "Before." + method, "params": command["params"]})
        if method == "Twice.answer":
            send({"id": command["id"] - 1, "result": {"stale": True}})
        if method == "Fail.please":
            send({"id": command["id"], "error": {"message": "as asked"}})
        else:
            send({"id": command["id"], "result": {"echo": command["params"], "session": command.get("sessionId")}})
        if method == "Browser.close":
            sys.exit(0)
"""


class CdpPipeTests(unittest.TestCase):
    def setUp(self):
        process = subprocess.Popen([sys.executable, "-c", FAKE_BROWSER], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.pipe = speedometer.CdpPipe(process, process.stdin, process.stdout)
        self.addCleanup(self.pipe.close)

    def test_answers_go_to_their_calls_and_events_wait(self):
        self.assertEqual(self.pipe.call("One.call", {"n": 1}), {"echo": {"n": 1}, "session": None})
        self.assertEqual(self.pipe.call("Two.call", {"n": 2}, "s")["session"], "s")
        # An answer to an earlier call is no one's.
        self.assertEqual(self.pipe.call("Twice.answer"), {"echo": {}, "session": None})
        with self.assertRaisesRegex(RuntimeError, "^Fail.please: as asked$"):
            self.pipe.call("Fail.please")
        # The events before the one waited for are dropped.
        self.assertEqual(self.pipe.event("Before.Two.call"), {"n": 2})
        self.assertEqual(self.pipe.event("Before.Fail.please"), {})
        with self.assertRaisesRegex(RuntimeError, "sent no Before.One.call"):
            self.pipe.event("Before.One.call", timeout=0.5)

    def test_closing_lets_it_end_on_its_own(self):
        self.pipe.close(grace=10)
        self.assertEqual(self.pipe.process.returncode, 0)
        with self.assertRaisesRegex(RuntimeError, "has ended"):
            self.pipe.call("One.call")


class BrowserTests:
    """What each browser posts for the fake suites, through `load(url)`."""

    def setUp(self):
        self.server = speedometer.Server(FAKE)
        self.server.mode = "measure"
        self.server.deadline = DEADLINE_MS

    def tearDown(self):
        self.server.close()

    def measured(self, suite: str, iterations: int = 1) -> dict:
        return speedometer.parse_run(self.load(speedometer.suite_url(self.server, suite, iterations)))

    def test_the_page_has_focus(self):
        # As a user's tab has: an editor focused in a test shows it
        # (TipTap's `ProseMirror-focused`), which the summaries compare.
        self.assertEqual(json.loads(self.load(self.server.url("/focus.html"))["body"]), {"focus": True})

    def test_a_suite_is_measured_and_summarized(self):
        # What an earlier page posted late isn't this one's.
        self.server.posted.put(b'{"status": "an earlier page\'s"}')
        got = self.measured("list", 2)
        self.assertEqual(got["status"], "done")
        self.assertEqual(got["errors"], [])
        self.assertEqual(got["completed"], 4)
        self.assertEqual(len(got["iterations"]), 2)
        for iteration in got["iterations"]:
            self.assertEqual(list(iteration["list"]["tests"]), ["Adding3Items", "RemovingOne"])
            self.assertGreaterEqual(iteration["list"]["tests"]["Adding3Items"]["tests"]["Sync"], 0)
        self.assertEqual(len(got["summaries"]), 2)
        for summaries in got["summaries"]:
            added, removed = summaries["list/Adding3Items"], summaries["list/RemovingOne"]
            self.assertEqual((added["elements"]["li"], added["classes"]["item"]), (3, 3))
            self.assertEqual((removed["elements"]["li"], removed["classes"]["item"]), (2, 2))
            self.assertEqual(added["elements"]["ul"], 1)
            self.assertEqual(added["text"] - removed["text"], 3)
        self.assertEqual(speedometer.run_problems(got, got), [])

    def test_the_window_is_as_wide_in_both(self):
        got = self.measured("size")
        self.assertEqual(got["summaries"][0]["size/MeasuringTheWindow"]["elements"]["i"], speedometer.WINDOW[0] // 100)

    def test_a_different_document_each_iteration_is_invalid(self):
        got = self.measured("random", 3)
        self.assertEqual(got["status"], "done")
        problems = speedometer.run_problems(got, got)
        self.assertEqual(len(problems), 1)
        self.assertRegex(problems[0], r"^iteration [12], random/AddingSome: \d+ `p` elements, Chrome \d+$")

    def test_an_exception_makes_it_invalid(self):
        got = self.measured("throws")
        self.assertEqual(got["status"], "done")
        problems = speedometer.run_problems(got, None)
        self.assertEqual(len(problems), 1)
        self.assertRegex(problems[0], r"^error in /suites/throws\.html: .*thrown by the suite")

    def test_a_stall_after_an_exception_ends_it(self):
        got = self.measured("stalls")
        self.assertEqual((got["status"], got["current"]), ("stalled", "stalls/Stalling"))
        self.assertIn("thrown before the stall", got["errors"][0]["message"])

    def test_the_runner_giving_up_ends_it(self):
        got = self.measured("broken")
        self.assertEqual(got["status"], "error")
        self.assertEqual(got["errors"][0]["kind"], "handleError")
        self.assertIn("thrown in the test", got["errors"][0]["message"])

    def test_the_deadline_ends_it(self):
        got = self.measured("hangs")
        self.assertEqual((got["status"], got["current"]), ("timeout", "hangs/Hanging"))
        self.assertEqual(got["errors"], [])

    def test_a_page_that_reports_nothing(self):
        got = self.load(self.server.url("/plain.html"), timeout=3)
        self.assertEqual(got, {"error": "no report in 3 s"})


class ChromeTests(BrowserTests, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not speedometer.chrome_path().exists():
            raise unittest.SkipTest("no Chrome for Testing yet: `bench.py speedometer` downloads it")

    def load(self, url, timeout=TIMEOUT):
        return speedometer.run_in_chrome(self.server, url, timeout)

    def test_the_page_is_left_alone_and_chrome_ends_on_its_own(self):
        profile = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.addCleanup(profile.cleanup)
        chrome = speedometer.ChromePipe(["--headless=new", f"--user-data-dir={profile.name}", "about:blank"])
        chrome.open(self.server.url("/focus.html"))
        self.assertIsNotNone(self.server.take(TIMEOUT, chrome.process))
        # No session stays on the page, as when Chrome opens it itself.
        pages = [info for info in chrome.call("Target.getTargets")["targetInfos"] if info["type"] == "page"]
        self.assertEqual([(page["url"], page["attached"]) for page in pages], [(self.server.url("/focus.html"), False)])
        chrome.close()
        # Killed, it would have another status (1 on Windows, -15 elsewhere).
        self.assertEqual(chrome.process.returncode, 0)

    @unittest.skipUnless(sys.platform == "win32", "Windows' app containers")
    def test_chromes_sandbox_may_read_its_folder(self):
        self.assertTrue(all(speedometer.inherited_read(speedometer.CHROME_DIR, sid)
                            for sid in speedometer.APP_CONTAINERS))
        # A folder that doesn't let them gets the read passed on to what's in
        # it, and only once.
        folder = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, folder)
        (folder / "chrome.exe").write_bytes(b"")
        self.assertFalse(any(speedometer.inherited_read(folder, sid) for sid in speedometer.APP_CONTAINERS))
        speedometer.let_app_containers_read(folder)
        for sid in speedometer.APP_CONTAINERS:
            self.assertTrue(speedometer.inherited_read(folder, sid), sid)
        alias = {"S-1-15-2-1": "AC"}
        file = speedometer.dacl(folder / "chrome.exe")
        for sid in speedometer.APP_CONTAINERS:
            self.assertRegex(file, rf"\(A;ID;0x1200a9;;;{alias.get(sid, sid)}\)")
        with mock.patch("subprocess.run") as icacls:
            speedometer.let_app_containers_read(folder)
        icacls.assert_not_called()
        # The folder alone readable isn't enough: what's in it must be too.
        alone = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, alone)
        subprocess.run(["icacls", str(alone), "/grant", *(f"*{sid}:(RX)" for sid in speedometer.APP_CONTAINERS)],
                       check=True, stdout=subprocess.DEVNULL)
        self.assertFalse(any(speedometer.inherited_read(alone, sid) for sid in speedometer.APP_CONTAINERS))
        speedometer.let_app_containers_read(alone)
        self.assertTrue(all(speedometer.inherited_read(alone, sid) for sid in speedometer.APP_CONTAINERS))


    def test_the_recording_gives_each_suite_what_its_page_did_while_loading(self):
        # The runner loads a suite's page before its first test's hook: what
        # the page did by then is that suite's, not the one before's.
        self.server.mode = "record"
        url = self.server.url("/index.html?startAutomatically&iterationCount=1&suites=list,loading")
        suites = json.loads(self.load(url)["body"])["suites"]
        self.assertIn("new Blob", suites["loading"]["api"])
        self.assertNotIn("new Blob", suites["list"]["api"])
        self.assertIn(".loading", suites["loading"]["selectors"])
        self.assertNotIn(".loading", suites["list"]["selectors"])
        self.assertIn(".item", suites["list"]["selectors"])
        self.assertEqual((suites["list"]["tests"], suites["loading"]["tests"]), (2, 1))

    def test_the_recording_has_what_machopos_probe_tries(self):
        self.server.mode = "record"
        url = self.server.url("/index.html?startAutomatically&iterationCount=1&suites=css")
        suite = json.loads(self.load(url)["body"])["suites"]["css"]
        # The shorthand as written, not its longhands that read empty.
        self.assertIn("margin: var(--m) 2px", suite["declarations"])
        self.assertIn("color: red", suite["declarations"])
        self.assertEqual([d for d in suite["declarations"] if d.endswith(": ") or d.startswith("--")], [])
        # Each kind of at-rule with its top-level rule's text.
        texts = suite["atRuleTexts"]
        self.assertEqual(sorted(texts), ["CSSKeyframeRule", "CSSKeyframesRule", "CSSMediaRule", "nesting"])
        self.assertTrue(texts["CSSMediaRule"].startswith("@media"))
        self.assertTrue(texts["nesting"].startswith(".nest"))
        self.assertTrue(texts["CSSKeyframeRule"].startswith("@keyframes spin"))
        self.assertEqual(texts["CSSKeyframeRule"], texts["CSSKeyframesRule"])
        # An element made in a namespace, with it.
        self.assertIn("element:svg|circle:SVGCircleElement", suite["api"])
        self.assertIn("element:svg|rect:SVGRectElement", suite["api"])
        self.assertIn("element:|thing:Element", suite["api"])

    def test_open_shadow_roots_are_summarized(self):
        # machopo has no shadow DOM yet.
        got = self.measured("shadow")
        self.assertEqual(got["status"], "done")
        shadow = got["summaries"][0]["shadow/AddingAShadowRoot"]
        self.assertEqual((shadow["elements"]["span"], shadow["classes"]["item"]), (2, 2))
        self.assertEqual(shadow["elements"]["div"], 2)
        self.assertEqual(shadow["text"] - got["summaries"][0]["shadow/Nothing"]["text"], 3)


class MachopoTests(BrowserTests, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = speedometer.machopo_binary()
        cls.log = open(os.devnull, "w", encoding="utf-8")

    @classmethod
    def tearDownClass(cls):
        cls.log.close()

    def load(self, url, timeout=TIMEOUT):
        return speedometer.run_in_machopo(self.server, self.binary, url, timeout, self.log)

    def test_machopo_tells_its_gpu(self):
        # A GPU wherever there is one; the CPU adapter when asked for.
        gpu = speedometer.machopo_gpu(self.binary)
        self.assertIsNotNone(gpu)
        self.assertTrue(gpu["device"] and gpu["backend"])
        self.assertIn(gpu["type"], ("DiscreteGpu", "IntegratedGpu", "VirtualGpu", "Other", "Cpu"))
        before = os.environ.get("MACHOPO_GPU_ADAPTER")
        os.environ["MACHOPO_GPU_ADAPTER"] = "cpu"
        try:
            # Metal has no CPU adapter (Windows has WARP, Linux Mesa's), so
            # on macOS machopo draws with none and tells none: the GPU it would
            # tell otherwise shows within a moment of starting.
            cpu = speedometer.machopo_gpu(self.binary, timeout=5 if sys.platform == "darwin" else 10)
        finally:
            if before is None:
                del os.environ["MACHOPO_GPU_ADAPTER"]
            else:
                os.environ["MACHOPO_GPU_ADAPTER"] = before
        if sys.platform == "darwin":
            self.assertIsNone(cpu)
        else:
            self.assertEqual(cpu["type"], "Cpu")

    def test_machopos_peak_memory_with_its_run(self):
        result = self.load(speedometer.suite_url(self.server, "list", 1))
        self.assertEqual(speedometer.parse_run(result)["status"], "done")
        if sys.platform == "darwin" or sys.platform.startswith("linux"):
            self.assertGreater(result["memory"], 20 << 20)

    def test_the_windows_are_the_runners_times(self):
        # Each test's window by the runner's marks in machopo's trace is the
        # sync and async times it reports, give or take the 0.1 ms
        # `performance.now()` is coarsened to.
        with tempfile.TemporaryDirectory() as folder:
            trace = Path(folder) / "list-machopo.pftrace"
            got = speedometer.parse_run(speedometer.run_in_machopo(
                self.server, self.binary, speedometer.suite_url(self.server, "list", 2), TIMEOUT, self.log, trace))
            windows, found_by = speedometer.timed_windows(
                speedometer.perfetto_slices(trace)["CrRendererMain"], speedometer.perfetto_marks(trace)["CrRendererMain"])
        self.assertEqual(got["status"], "done")
        self.assertEqual(found_by, "the runner's marks")
        times = [test["tests"]["Sync"] + test["tests"]["Async"]
                 for iteration in got["iterations"] for test in iteration["list"]["tests"].values()]
        self.assertEqual(len(windows), len(times))
        for (start, end), time in zip(windows, times):
            self.assertAlmostEqual((end - start) / 1e6, time, delta=0.15)

    def test_the_probe_posts_what_machopo_lacks(self):
        self.server.probe = {
            "api": ["Document.createElement", "Document.notAMember"],
            "declarations": ["color: red", "margin: var(--m) 2px", "not-a-property: 1"],
            "selectors": [
                "div",
                "div::not-a-pseudo",
                ".a:hover:lang(ja)::not-a-pseudo, .b, .c:not-a-class",
                ".d::before:hover",
                ".e:is(.f, .g), .h:not-a-class",
                '[title="a,b"], [title="a\\",b"], .h:not-a-class',
            ],
            "atRules": {
                "CSSMediaRule": "@media (min-width: 1px) { .media { } }",
                "CSSKeyframeRule": "@keyframes spin { from { opacity: 0; } }",
                "CSSFontFeatureValuesRule": "@font-feature-values Font { @swash { fancy: 1; } }",
                "nesting": ".n { color: red; .m { color: blue; } }",
            },
            "elements": [
                "element:details:HTMLElement",
                "element:svg|circle:SVGCircleElement",
                "element:html|details:HTMLElement",
                "element:svg|circle:SVGNotACircle",
                "element:|thing:Element",
            ],
        }
        got = speedometer.parse_run(self.load(self.server.url("/__machopo__/probe.html")))
        self.assertEqual(got["api"], ["Document.notAMember"])
        self.assertEqual(got["declarations"], ["not-a-property: 1"])
        self.assertEqual(got["selectors"], self.server.probe["selectors"][1:])
        # What fails on its own, not what's written with it; a selector
        # whose parts each parse is its own culprit.
        self.assertEqual(
            got["culprits"],
            {
                "div::not-a-pseudo": ["::not-a-pseudo"],
                ".a:hover:lang(ja)::not-a-pseudo, .b, .c:not-a-class": ["::not-a-pseudo", ":not-a-class"],
                ".d::before:hover": [".d::before:hover"],
                ".e:is(.f, .g), .h:not-a-class": [":not-a-class"],
                '[title="a,b"], [title="a\\",b"], .h:not-a-class': [":not-a-class"],
            },
        )
        self.assertEqual(got["atRules"], ["CSSFontFeatureValuesRule", "nesting"])
        self.assertEqual(got["elements"], ["svg|circle: SVGNotACircle (machopo: SVGCircleElement)"])

    def test_the_inventorys_runs_post_how_far_they_got(self):
        self.server.mode = "inventory"
        got = speedometer.summarize_run(self.load(speedometer.suite_url(self.server, "list", 1)))
        self.assertEqual((got["status"], got["done"], got["first_error"]), ("done", 2, ""))
        got = speedometer.summarize_run(self.load(speedometer.suite_url(self.server, "stalls", 1)))
        self.assertEqual((got["status"], got["done"]), ("stalled", 0))
        self.assertIn("thrown before the stall", got["first_error"])

    def test_machopo_and_chrome_summarize_alike(self):
        if not speedometer.chrome_path().exists():
            self.skipTest("no Chrome for Testing yet")
        machopo = self.measured("list", 1)
        chrome = speedometer.parse_run(
            speedometer.run_in_chrome(self.server, speedometer.suite_url(self.server, "list", 1), TIMEOUT)
        )
        self.assertEqual(machopo["summaries"], chrome["summaries"])
        self.assertEqual(speedometer.run_problems(machopo, chrome), [])



class MeasureTests(unittest.TestCase):
    """`bench.py speedometer` without `--inventory`, on the fake suites: machopo (the
    dev profile, as built for `MachopoTests`) and Chrome in turn."""

    @classmethod
    def setUpClass(cls):
        if not speedometer.chrome_path().exists():
            raise unittest.SkipTest("no Chrome for Testing yet")

    def setUp(self):
        results = tempfile.TemporaryDirectory()
        self.addCleanup(results.cleanup)
        self.results = Path(results.name)
        for name, value in (("RESULTS", self.results), ("SPEEDOMETER", FAKE), ("MACHOPO_PROFILE", "dev")):
            patch = mock.patch.object(speedometer, name, value)
            patch.start()
            self.addCleanup(patch.stop)

    def measure(self, browser=None, reuse_chrome=False) -> list[str]:
        # Six samples, the fewest a 95% interval of the median takes.
        args = argparse.Namespace(browser=browser, iterations=6, reuse_chrome=reuse_chrome)
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            self.assertEqual(speedometer.cmd_measure(args, [("list", True), ("random", True)], None), 0)
        return (self.results / "measure.md").read_text(encoding="utf-8").splitlines()

    def test_both_browsers_in_turn(self):
        lines = self.measure()
        for browser in ("machopo", "chrome"):
            saved = json.loads((self.results / f"{browser}-runs.json").read_text(encoding="utf-8"))
            self.assertEqual(saved["iterations"], 6)
            self.assertEqual(sorted(saved["suites"]), ["list", "random"])
            self.assertEqual(len(saved["suites"]["list"]["iterations"]), 6)
            self.assertEqual(saved["environment"]["profile"], "dev")
        row = next(line for line in lines if line.startswith("| list |"))
        self.assertRegex(row, r"^\| list \| [\d.]+ \([\d.]+–[\d.]+\) \| [\d.]+ \([\d.]+–[\d.]+\) \| [\d.]+ \| [\d.]+, [\d.]+ \|$")
        self.assertTrue(any(line.startswith("| list | Adding3Items |") for line in lines))
        random = next(line for line in lines if line.startswith("| random |"))
        self.assertRegex(random, r"^\| random \| not valid: Chrome: iteration 1, random/AddingSome: ")

        # machopo alone, checked against Chrome's runs above.
        lines = self.measure(browser="machopo")
        row = next(line for line in lines if line.startswith("| list |"))
        self.assertRegex(row, r"^\| list \| [\d.]+ \([\d.]+–[\d.]+\) \| not run \|  \| [\d.]+, not run \|$")
        random = next(line for line in lines if line.startswith("| random |"))
        self.assertRegex(random, r"^\| random \| not valid: machopo: iteration 0, random/AddingSome: ")

    def test_profiles_of_both(self):
        args = argparse.Namespace(browser=None, iterations=2)
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            self.assertEqual(speedometer.cmd_profile(args, [("list", True)], None), 0)
        for side in ("machopo", "chrome"):
            profile = json.loads((self.results / "profile" / f"list-{side}.cpuprofile").read_text(encoding="utf-8"))
            self.assertEqual(len(profile["samples"]), len(profile["timeDeltas"]))
            # The suite's own code, in its iframe, by its URL.
            names = {node["callFrame"]["url"].rsplit("/", 1)[-1] for node in profile["nodes"]}
            self.assertTrue(any(name.endswith(".js") or name.endswith(".mjs") or name.endswith(".html")
                                for name in names), (side, names))
            # Where the page's clock function ran, and its samples.
            self.assertLess(profile["pageClock"]["start"], profile["pageClock"]["end"])
            self.assertLess(profiles.uncertainty(profile), 1.5)
        report = (self.results / "profile.md").read_text(encoding="utf-8")
        self.assertRegex(report, r"\| JS functions \| [\d.]+ \| [\d.]+ \| [-+][\d.]+ \|")
        # The timed windows, and the rest of the run.
        self.assertRegex(report, r"\| `\(outside the timed windows\)` \| [1-9][\d.]* \| [1-9][\d.]* \|")
        self.assertIn("### list", report)
        written = json.loads((self.results / "profile.json").read_text(encoding="utf-8"))
        for side in ("machopo", "Chrome"):
            timed = {key: value for key, value in written["list"][side].items() if key != profiles.OUTSIDE}
            self.assertGreater(sum(timed.values()), 0, side)
            # The whole run's: all of it by function, none out of the windows.
            whole = written["list"]["whole"][side]
            self.assertNotIn(profiles.OUTSIDE, whole)
            self.assertAlmostEqual(sum(whole.values()), sum(written["list"][side].values()), places=6)
            self.assertGreater(sum(whole.values()), sum(timed.values()), side)

    def test_traces_of_both(self):
        args = argparse.Namespace(browser=None, iterations=1)
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            self.assertEqual(speedometer.cmd_trace(args, [("frames", True)], None), 0)
        # And from them each phase's time, both browsers side by side.
        phases = (self.results / "phases.md").read_text(encoding="utf-8")
        self.assertIn("| Phase | machopo | Chrome | machopo − Chrome |", phases)
        self.assertRegex(phases, r"\n\| frames \| [\d.]+ / [\d.]+ \([-+][\d.]+\) \|")
        written = json.loads((self.results / "phases.json").read_text(encoding="utf-8"))
        self.assertGreater(written["frames"]["machopo"]["script"], 0)
        self.assertGreater(written["frames"]["Chrome"]["script"], 0)
        # machopo's: Perfetto's protobuf, its spans and V8's events.
        threads = perfetto_threads(self.results / "trace" / "frames-machopo.pftrace")
        self.assertIn("CrRendererMain", threads)
        main = threads["CrRendererMain"]
        # The windows the runner times, one a test, by its marks; each the
        # length of the test's two measures, which are slices on tracks of
        # their own.
        trace = self.results / "trace" / "frames-machopo.pftrace"
        slices = speedometer.perfetto_slices(trace)
        marks = speedometer.perfetto_marks(trace)["CrRendererMain"]
        windows, found_by = speedometer.timed_windows(slices["CrRendererMain"], marks)
        self.assertEqual(found_by, "the runner's marks")
        tests = ["TimerThenFrame", "Garbage", "Markup", "MarkupAndElements"]
        self.assertEqual([name for _, name in marks if name.endswith("-start") and "async" not in name],
                         [f"frames.{test}-start" for test in tests])
        measures = {}
        thread_names = set(speedometer.perfetto_track_events(trace)[0].values())
        for track, events in slices.items():
            if track in thread_names:
                continue
            (begin, name), (end, ended) = events
            self.assertIsNone(ended)
            measures[name] = end - begin
        self.assertEqual(sorted(measures), sorted(f"frames.{test}-{part}" for test in tests for part in ("sync", "async")))
        self.assertEqual(len(windows), len(tests))
        for (start, end), test in zip(windows, tests):
            self.assertAlmostEqual(end - start, measures[f"frames.{test}-sync"] + measures[f"frames.{test}-async"],
                                   delta=50_000)
        # Named as Chrome's trace events are, V8's by V8.
        for name in (
            "ThreadControllerImpl::RunTask", "EvaluateScript", "EventDispatch", "FunctionCall", "RunMicrotasks",
            "TimerFire", "PageAnimator::serviceScriptedAnimations", "FireAnimationFrame", "HitTest",
            "ProxyMain::BeginMainFrame", "LocalFrameView::UpdateLifecyclePhases",
            "LocalFrameView::RunStyleAndLayoutLifecyclePhases", "Document::UpdateStyle",
            "StyleEngine::RebuildLayoutTree", "Layout", "PrePaint", "Paint",
            "V8.DeserializeContext", "MinorGC", "V8.GC_SCAVENGER",
            "v8.callFunction", "ParseHTML", "CustomElementRegistry::define",
            "CustomElementReactionQueue::invokeReactions", "v8.callAsConstructor",
            "RenderThread::between_tasks", "RenderThread::run_platform_tasks",
            *(step for step in FRAME_STEPS if step not in WHEN_NEEDED_STEPS),
        ):
            self.assertIn(name, main)
        # A main frame, which the compositor posts, is in a task of its own;
        # the test's garbage is collected while it runs (the runner's frame:
        # a microtask of its rAF callback), V8's events on machopo's thread,
        # nested in machopo's spans.
        # A call is a `FunctionCall` only with no script around it (a call,
        # a classic script, a module, the microtasks after one, which Chrome
        # runs inside the call): a listener script's `click()` or
        # `dispatchEvent()` runs is a `v8.callFunction` alone.
        # Markup the test sets is parsed in its script; an upgrade
        # constructs in the element's reactions. Each step of a frame is in
        # the frame.
        script = {"FunctionCall", "EvaluateScript", "Evaluation::evaluate", "RunMicrotasks"}
        stack: list[str] = []
        frames = collections_in_tests = nested_calls = fragments = microtasks_in_frames = 0
        for i, name in enumerate(main):
            if name is None:
                stack.pop()
                continue
            if name == "ProxyMain::BeginMainFrame":
                frames += 1
                self.assertIn("ThreadControllerImpl::RunTask", stack)
            if name == "MinorGC" and "FireAnimationFrame" in stack:
                collections_in_tests += 1
            # The checkpoint after a callback (the runner's `then()` after
            # its test) is in the callback's span, not the task's end.
            if name == "RunMicrotasks" and "FireAnimationFrame" in stack:
                microtasks_in_frames += 1
            if name == "FunctionCall":
                self.assertEqual(stack[-1], "v8.callFunction")
                self.assertFalse(script & set(stack), stack)
            if name == "v8.callFunction" and stack[-1] == "EventDispatch" and main[i + 1] != "FunctionCall":
                nested_calls += 1
            if name == "ParseHTML" and script & set(stack):
                fragments += 1
            if name == "v8.callAsConstructor":
                self.assertEqual(stack[-1], "CustomElementReactionQueue::invokeReactions")
            if name in FRAME_STEPS:
                self.assertEqual(stack[-1], FRAME_STEPS[name], name)
            # The loop's work between tasks is of no task.
            if name == "RenderThread::between_tasks":
                self.assertEqual(stack, [])
            if name == "RenderThread::run_platform_tasks":
                self.assertEqual(stack, ["RenderThread::between_tasks"])
            stack.append(name)
        self.assertEqual(stack, [])
        self.assertGreater(frames, 0)
        self.assertGreater(collections_in_tests, 0)
        self.assertGreater(nested_calls, 0)
        self.assertGreater(fragments, 0)
        self.assertGreater(microtasks_in_frames, 0)
        # V8's helper threads have theirs.
        self.assertTrue(any(name and name.startswith("V8.GC_") for thread, names in threads.items()
                            if thread != "CrRendererMain" for name in names))
        # Chrome's: Perfetto's protobuf, its names interned as text (a
        # renderer for each page, so its threads aren't read as machopo's are).
        path = self.results / "trace" / "frames-chrome.pftrace"
        chrome = path.read_bytes()
        for name in (b"ThreadControllerImpl::RunTask", b"FunctionCall", b"TimerFire", b"FireAnimationFrame"):
            self.assertIn(name, chrome)
        # The runner's marks as User Timing's instants (their names are
        # elsewhere in it without: DevTools' events carry them).
        _, events = speedometer.perfetto_track_events(path)
        self.assertIn("frames.Garbage-start", {name for _, _, _, kind, name, categories in events
                                               if kind == 3 and speedometer.USER_TIMING in categories})
        self.assertFalse(path.with_name(path.name + ".part").exists())


class OpenProfiledTests(unittest.TestCase):
    """How `--profile` starts DevTools' `Profiler` in either browser."""

    class Pipe:
        name = "the browser"

        def __init__(self):
            self.calls = []
            # Not ready (`about:blank`, then the page before `measure.mjs`)
            # until asked a third time.
            self.ready = [True, False, False]

        def navigate(self, url, front):
            self.calls.append(("navigate", url))
            return "session"

        def call(self, method, params, session):
            self.calls.append((method, params, session))
            return {}

        def evaluate(self, expression, session):
            if "__machopoStart ===" in expression:
                self.calls.append(("ready?",))
                return self.ready.pop() if self.ready else True
            if expression == "void __machopoStart()":
                self.calls.append(("start the page",))
                return None
            self.calls.append(("clock", expression))
            return [12.5, 22.5]

    def test_after_the_navigation_every_interval_then_the_clock(self):
        pipe = self.Pipe()
        self.assertEqual(common.open_profiled(pipe, "http://127.0.0.1:1/index.html", front=False),
                         ("session", {"start": 12.5, "end": 22.5}))
        # The page's renderer is the one sampled, before the page starts and
        # its code compiles; the page's clock in the profile, before the
        # page's code.
        self.assertEqual(pipe.calls[:4], [("navigate", "http://127.0.0.1:1/index.html"),
                                          ("ready?",), ("ready?",), ("ready?",)])
        self.assertEqual(pipe.calls[4:7], [
            ("Profiler.enable", {}, "session"),
            ("Profiler.setSamplingInterval", {"interval": common.PROFILE_INTERVAL}, "session"),
            ("Profiler.start", {}, "session"),
        ])
        (kind, expression), start = pipe.calls[7:]
        self.assertEqual(kind, "clock")
        self.assertIn(f"function {common.PAGE_CLOCK}()", expression)
        self.assertIn(f"< {common.PAGE_CLOCK_MS})", expression)
        self.assertEqual(start, ("start the page",))


class ProfileReadingTests(unittest.TestCase):
    """What `--profile` makes of DevTools' `.cpuprofile`s."""

    PROFILE = {
        "nodes": [
            {"id": 1, "callFrame": {"functionName": "(root)", "url": ""}, "children": [2, 3, 4, 5, 9]},
            {"id": 2, "callFrame": {"functionName": "(program)", "url": ""}},
            {"id": 3, "callFrame": {"functionName": "(idle)", "url": ""}},
            {"id": 4, "callFrame": {"functionName": "(garbage collector)", "url": ""}},
            {"id": 5, "callFrame": {"functionName": "draw", "url": "http://127.0.0.1:41025/app.js?v=1",
                                    "lineNumber": 9, "columnNumber": 2}, "children": [6, 7, 10, 11, 12, 14]},
            # A DOM call, and a builtin it called.
            {"id": 6, "callFrame": {"functionName": "fill", "url": ""}, "children": [8]},
            {"id": 7, "callFrame": {"functionName": "", "url": "http://127.0.0.1:41025/app.js?v=1",
                                    "lineNumber": 0, "columnNumber": 0}},
            {"id": 8, "callFrame": {"functionName": "ArrayPrototypePush", "url": ""}},
            # Script without a URL, at the top: no JS function to count for.
            {"id": 9, "callFrame": {"functionName": "", "url": ""}},
            # machopo's shared path's callback: no name.
            {"id": 10, "callFrame": {"functionName": "", "url": "", "lineNumber": -1}},
            # A regular expression's code: not a member.
            {"id": 11, "callFrame": {"functionName": "RegExp: \\s+", "url": "", "lineNumber": -1}},
            # JS without a URL (an older machopo's `glue.js`), and the member it wraps.
            {"id": 12, "callFrame": {"functionName": "method", "url": "", "lineNumber": 377}, "children": [13]},
            {"id": 13, "callFrame": {"functionName": "scrollTo", "url": "", "lineNumber": -1}},
            {"id": 14, "callFrame": {"functionName": "rest", "url": "", "lineNumber": 248}},
        ],
        "samples": [2, 3, 3, 4, 5, 6, 8, 7, 9, 10, 11, 13, 14],
        "timeDeltas": [1000, 2000, 2000, 500, 1000, 3000, 1000, 1000, 250, 2000, 500, 1000, 250],
        "startTime": 5000,
    }

    def test_a_sample_counts_for_its_innermost_js_function(self):
        by_function, members = profiles.attribute(self.PROFILE)
        draw = "draw /app.js?v=1:10:3"
        self.assertEqual(by_function, collections.Counter({
            # Its own, its DOM calls' (through glue or not), the builtin's
            # and the regular expression's under one.
            draw: 8.75,
            "(anonymous) /app.js?v=1:1:1": 1.0,
            "(program)": 1.0, "(idle)": 4.0, "(garbage collector)": 0.5,
            "(no JS function)": 0.25,
        }))
        # The API function a sample was in, with its caller.
        self.assertEqual(members, collections.Counter({
            ("fill", draw): 4.0, ("(machopo's shared path)", draw): 2.0, ("scrollTo", draw): 1.0,
            ("rest (script without a URL)", draw): 0.25,
        }))

    def test_only_the_timed_windows(self):
        draw = "draw /app.js?v=1:10:3"
        # Samples at 6000, 8000... µs (`startTime` and the deltas), each the
        # time since the one before: a sample half in a window counts half,
        # and one across two windows (11500 to 14500), what's in each.
        by_function, members = profiles.attribute(self.PROFILE, [(11000, 12000), (13000, 15000), (16600, 16700)])
        self.assertEqual({key: round(value, 6) for key, value in by_function.items()}, {
            draw: 3.0, "(no JS function)": 0.1, "(outside the timed windows)": 12.4,
        })
        self.assertEqual(members, collections.Counter({("fill", draw): 2.5}))

    CLOCKED = {
        "nodes": [
            {"id": 1, "callFrame": {"functionName": "(root)", "url": ""}, "children": [2, 3]},
            {"id": 2, "callFrame": {"functionName": "(program)", "url": ""}},
            {"id": 3, "callFrame": {"functionName": common.PAGE_CLOCK, "url": ""}, "children": [4]},
            {"id": 4, "callFrame": {"functionName": "now", "url": ""}},
        ],
        "startTime": 1000,
        "samples": [2, 3, 3, 4, 2],
        "timeDeltas": [1000, 1000, 1000, 1000, 1000],
        # The page's clock function, from 10 to 13 ms on the page's clock.
        "pageClock": {"start": 10.0, "end": 13.0},
    }

    def test_the_page_clock_on_the_profile(self):
        # Its samples at 3000 to 5000 µs (the last in a call it made): the
        # page's 0 is from 5000 - 13000 to 3000 - 10000 µs.
        self.assertEqual(profiles.page_clock(self.CLOCKED), (-8000, -7000))
        self.assertEqual(profiles.on_profile(self.CLOCKED, [(21.0, 22.0), (20.0, 21.0)]),
                         [(12500, 13500), (13500, 14500)])
        self.assertEqual(profiles.uncertainty(self.CLOCKED), 0.5)

    def test_no_page_clock(self):
        unclocked = {**self.CLOCKED, "samples": [2, 2, 2, 2, 2]}
        with self.assertRaisesRegex(ValueError, "no sample in __machopoPageClock"):
            profiles.page_clock(unclocked)
        # Samples 2 ms apart in a function that ran for 0.5 ms.
        too_short = {**self.CLOCKED, "pageClock": {"start": 10.0, "end": 10.5}}
        with self.assertRaisesRegex(ValueError, "out of the time it ran"):
            profiles.page_clock(too_short)

    def test_per_iteration(self):
        self.assertEqual(profiles.per_iteration(collections.Counter({"f": 30.0}), 10), collections.Counter({"f": 3.0}))

    def test_the_functions_that_differ_most(self):
        machopo = collections.Counter({"a": 9.0, "b": 3.0, "c": 1.0, "d": 1.0, "same": 2.0, "(program)": 50.0})
        chrome = collections.Counter({"a": 1.0, "b": 1.0, "c": 2.0, "d": 6.0, "same": 2.0, "(program)": 1.0})
        # machopo's most extra time first, down to the most Chrome takes more
        # on; not the states, nor what's the same.
        self.assertEqual(profiles.suite_rows(machopo, chrome, 4),
                         [("a", 9.0, 1.0), ("b", 3.0, 1.0), ("c", 1.0, 2.0), ("d", 1.0, 6.0)])
        # Half as many of Chrome's: the ones it takes most more on.
        self.assertEqual(profiles.suite_rows(machopo, chrome, 2), [("a", 9.0, 1.0), ("b", 3.0, 1.0), ("d", 1.0, 6.0)])

    def test_the_report(self):
        machopo = collections.Counter({"draw /a.js:1:1": 9.0, "parse /a.js:2:1": 1.0, "(program)": 2.0, "(idle)": 5.0,
                                   "(outside the timed windows)": 40.0})
        chrome = collections.Counter({"draw /a.js:1:1": 1.0, "parse /a.js:2:1": 3.0, "(program)": 4.0, "(idle)": 6.0,
                                      "(outside the timed windows)": 50.0})
        suites = {"Charts": {"machopo": machopo, "Chrome": chrome, "times": {"machopo": 30.0, "Chrome": 20.0},
                             "uncertainty": {"machopo": 0.25, "Chrome": 0.4},
                             "whole": {"machopo": collections.Counter({"draw /a.js:1:1": 12.0}),
                                       "Chrome": collections.Counter({"draw /a.js:1:1": 2.0})},
                             "members": collections.Counter({("fill", "draw /a.js:1:1"): 7.0})}}
        with tempfile.TemporaryDirectory() as folder:
            report = profiles.write_report(Path(folder), suites, 10, {"machopo": "abc", "chrome": "152", "interval": 1000,
                                                                       "system": "Linux"}).read_text(encoding="utf-8")
            written = json.loads((Path(folder) / "profile.json").read_text(encoding="utf-8"))
        self.assertIn("| JS functions | 10.0 | 4.0 | +6.0 |", report)
        self.assertIn("| `(program)` | 2.0 | 4.0 | -2.0 |", report)
        # In the windows, and out of them: not a function's, nor compared.
        self.assertIn("| **Timed** | 17.0 | 14.0 | +3.0 |", report)
        self.assertIn("| `(outside the timed windows)` | 40.0 | 50.0 | -10.0 |", report)
        self.assertNotIn("| Charts | `(outside", report)
        # machopo's extra time first, then where Chrome takes more.
        self.assertIn("| Charts | `draw /a.js:1:1` | 9.0 | 1.0 | +8.0 |", report)
        section = report[report.index("### Charts"):]
        self.assertLess(section.index("`draw /a.js:1:1`"), section.index("`parse /a.js:2:1`"))
        self.assertIn("| `parse /a.js:2:1` | 1.0 | 3.0 | -2.0 |", section)
        self.assertIn("Profiled: machopo 30.0 ms, Chrome 20.0 ms", section)
        self.assertIn("The windows placed within 0.40 ms.", section)
        self.assertIn("| `fill` | 7.0 | `draw /a.js:1:1 (Charts)` 7.0 |", report)
        self.assertEqual(written["Charts"]["machopo"]["draw /a.js:1:1"], 9.0)
        self.assertEqual(written["Charts"]["whole"]["Chrome"]["draw /a.js:1:1"], 2.0)
        self.assertEqual(written["Charts"]["uncertainty"]["machopo"], 0.25)


class SampleReadingTests(unittest.TestCase):
    """What `--sample` makes of a trace and of `perf`'s samples."""

    def test_the_windows_by_the_runners_frames(self):
        def slice_(start, end, name):
            return [(start, name), (end, None)]

        def frame(start, end, *callbacks):
            return slice_(start, end, "ProxyMain::BeginMainFrame") + [
                event for a, b in callbacks for event in slice_(a, b, "FireAnimationFrame")
            ]
        slices = sorted(
            # The runner's two callbacks in one frame, then its timer: from
            # the first callback to the timer's end.
            frame(0, 10, (1, 3), (3, 4)) + slice_(12, 14, "TimerFire") + slice_(15, 16, "TimerFire")
            # The page's own callback alone, and its timer: not timed.
            + frame(20, 25, (21, 22)) + slice_(30, 31, "TimerFire")
            # Another frame starts before the timer after this one: the
            # timer is that frame's.
            + frame(40, 50, (41, 45), (45, 46)) + frame(55, 60, (56, 57), (57, 58)) + slice_(62, 63, "TimerFire")
            # No timer after it: no window.
            + frame(70, 75, (71, 72), (72, 73)),
            key=lambda event: (event[0], event[1] is not None),
        )
        self.assertEqual(speedometer.frame_windows(slices), [(1, 14), (56, 63)])

    def test_the_windows_by_the_runners_marks(self):
        marks = [
            # Out of order as given: in time order as read.
            ("S.B-async-end", 14), ("S.B-start", 10),
            ("suite-S-start", 0), ("S.A-start", 1), ("S.A-sync-end", 3), ("S.A-async-start", 3),
            ("S.A-async-end", 5),
            # A test started twice: from the later start.
            ("S.D-start", 40), ("S.D-start", 42), ("S.D-async-end", 45),
            # A start with no end, an end with no start, an end twice (the
            # second has no start left), the suite's end.
            ("S.C-start", 30), ("S.E-async-end", 50), ("S.A-async-end", 55), ("suite-S-end", 60),
        ]
        self.assertEqual(speedometer.mark_windows([(time, name) for name, time in marks]),
                         [(1, 5), (10, 14), (42, 45)])

    def test_marks_are_user_timings_instants(self):
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

        def event(time, kind, name, *categories):
            # On the sequence's thread, at an absolute time (CLOCK_BOOTTIME).
            return message((1, message((10, 1), (8, time), (58, 6),
                                       (11, message((9, kind), (23, name), *categories)))))

        trace = (
            message((1, message((10, 1), (60, message((1, 5), (4, message((5, b"CrRendererMain"))))),
                                (12, message((1, message((1, 1), (2, b"blink.user_timing"))),
                                             (1, message((1, 2), (2, b"v8"))))),
                                (59, message((11, message((11, 5))))))))
            # User Timing's instants, its category interned or as text.
            + event(100, 3, b"S.A-start", (3, 1)) + event(200, 3, b"S.A-async-end", (22, b"blink.user_timing"))
            # Another category's instants, and User Timing's slice: no marks.
            + event(110, 3, b"S.B-start", (3, 2)) + event(150, 3, b"S.B-async-end", (3, 2))
            + event(120, 1, b"S.C-start", (3, 1)) + event(130, 2, b"", (3, 1))
        )
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.pftrace"
            path.write_bytes(trace)
            self.assertEqual(speedometer.perfetto_marks(path), {"CrRendererMain": [(100, "S.A-start"),
                                                                                  (200, "S.A-async-end")]})

    def test_an_incremental_clocks_unit(self):
        # Chrome's slices count µs on the sequence's incremental clock; its
        # marks are ns of an absolute one.
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

        incremental = speedometer.INCREMENTAL_CLOCK
        def packet(*fields):
            return message((1, message((10, 1), *fields)))

        trace = (
            packet((6, message((1, message((1, incremental), (2, 1000), (4, 1000))))), (59, message((58, incremental))))
            + packet((8, 5), (11, message((9, 1), (23, b"Work"))))
            + packet((8, 3), (11, message((9, 2))))
            + packet((8, 1_000_004_000), (58, 6), (11, message((9, 3), (23, b"m"))))
        )
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.pftrace"
            path.write_bytes(trace)
            _, events = speedometer.perfetto_track_events(path)
        self.assertEqual([(time, kind, name) for _, time, _, kind, name, _ in events],
                         [(1_005_000, 1, "Work"), (1_008_000, 2, ""), (1_000_004_000, 3, "m")])

    def test_marks_first_then_frames(self):
        slices = [(0, "ProxyMain::BeginMainFrame"), (1, "FireAnimationFrame"), (2, None),
                  (2, "FireAnimationFrame"), (3, None), (4, None), (5, "TimerFire"), (6, None)]
        marks = [(10, "S.A-start"), (20, "S.A-async-end")]
        self.assertEqual(speedometer.timed_windows(slices, marks), ([(10, 20)], "the runner's marks"))
        # Marks that make no window (the page's own) leave it to the frames.
        self.assertEqual(speedometer.timed_windows(slices, [(10, "mine")]), ([(1, 6)], "the runner's frames"))

    def test_only_the_samples_in_the_windows_folded(self):
        samples = [(5, ["leaf", "root"]), (5, ["leaf", "root"]), (15, ["other", "root"]), (25, ["x;y", "root"]),
                   (30, ["late", "root"])]
        self.assertEqual(speedometer.fold(samples, [(0, 10), (20, 30)]), {"root;leaf": 2, "root;x:y": 1})

    def test_one_threads_samples(self):
        text = (
            "CrRendererMain  10/11  5.000000007: \n"
            "\t    7f01 leaf\n"
            "\t    7f02 JS:*run http://x/a.html:1:2\n"
            "\t    7f03 [unknown]\n"
            "\n"
            "VizCompositorTh  10/12  5.000000008: \n"
            "\t    7f04 viz\n"
            "\n"
            "CrRendererMain  20/21  6.000000000: \n"
            "\t    7f05 other\n"
        )
        self.assertEqual(
            speedometer.read_perf_script(text, "CrRendererMain", 100),
            ({"cycles": [(5_000_000_107, ["leaf", "JS:*run http://x/a.html:1:2", "[unknown]"]),
                         (6_000_000_100, ["other"])]},
             {10, 20}),
        )

    def test_samples_by_event(self):
        text = (
            "CrRendererMain  10/11  5.000000007:      cycles:u: \n"
            "\t    7f01 leaf\n"
            "\n"
            "CrRendererMain  10/11  5.000000009: instructions:u: \n"
            "\t    7f02 other\n"
            "\n"
            "VizCompositorTh  10/12  5.000000010: instructions:u: \n"
            "\t    7f03 viz\n"
        )
        self.assertEqual(
            speedometer.read_perf_script(text, "CrRendererMain", 0),
            ({"cycles": [(5_000_000_007, ["leaf"])], "instructions": [(5_000_000_009, ["other"])]}, {10}),
        )

    def test_cycles_and_instructions_one_sample_every_million(self):
        command, _ = speedometer.perf_sampler(Path(tempfile.gettempdir()) / "machopo-sampler-test.perf.data")
        self.assertEqual(command[command.index("-c") + 1], "1000000")
        events = [command[i + 1] for i, arg in enumerate(command) if arg == "-e"]
        self.assertEqual(events, ["cycles:u", "instructions:u"])
        # The user-space stacks alone: the kernel's aren't open to every
        # user (a `perf_event_paranoid` of 2).
        self.assertIn("--user-callchains", command)

    def test_each_platform_samples_its_way(self):
        for platform_name, way in (("linux", speedometer.LINUX_SAMPLING), ("darwin", speedometer.MACOS_SAMPLING),
                                   ("win32", speedometer.WINDOWS_SAMPLING), ("freebsd14", None)):
            with mock.patch.object(speedometer.sys, "platform", platform_name):
                self.assertIs(speedometer.sampling(), way)
        self.assertEqual((speedometer.LINUX_SAMPLING.tool, speedometer.LINUX_SAMPLING.events),
                         ("perf", ("cycles", "instructions")))
        self.assertEqual((speedometer.MACOS_SAMPLING.tool, speedometer.MACOS_SAMPLING.events), ("samply", ("cpu",)))
        self.assertEqual((speedometer.WINDOWS_SAMPLING.tool, speedometer.WINDOWS_SAMPLING.events,
                          speedometer.WINDOWS_SAMPLING.suffix), ("samply", ("cpu",), ".samply.json.gz"))
        samples = Path(tempfile.gettempdir()) / "S-machopo.samply.json.gz"
        for platform_name, tool in (("linux", "perf"), ("darwin", "samply"), ("win32", "samply")):
            with mock.patch.object(speedometer.sys, "platform", platform_name):
                self.assertEqual(speedometer.sampler(samples)[0][0], tool)

    def test_samply_on_windows_through_etw(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        samples = Path(folder.name) / "S-machopo.samply.json.gz"
        self.assertEqual(speedometer.samply_etls(samples),
                         (samples.with_name("S-machopo.samply.kernel.etl"), samples.with_name("S-machopo.samply.user.etl")))
        # A run's files from before are gone.
        for leftover in (samples, speedometer.samply_symbols(samples), *speedometer.samply_etls(samples)):
            leftover.write_bytes(b"old")
        with mock.patch.object(speedometer.sys, "platform", "win32"):
            command, flags = speedometer.samply_sampler(samples)
        self.assertEqual(list(Path(folder.name).iterdir()), [])
        self.assertEqual(command[:2], ["samply", "record"])
        self.assertEqual(command[-3:], ["-o", str(samples), "--"])
        self.assertEqual(command[command.index("-r") + 1], "1000")
        for option in ("--save-only", "--no-open", "--unstable-presymbolicate", "--browsers", "--keep-etl"):
            self.assertIn(option, command)
        # Windows's DLLs are named after (`windows_names`): samply doesn't
        # look them up on a symbol server while it records.
        self.assertNotIn("--windows-symbol-server", command)
        # V8 names its JS to ETW: no perf map, no log.
        self.assertEqual(flags, "--enable-etw-stack-walking --interpreted-frames-native-stack")

    @staticmethod
    def etl(start: int, frequency: int) -> bytes:
        """The start of a kernel ETL: its buffer's header, the trace's
        header event (`SYSTEM_TRACE_HEADER`) and `TRACE_LOGFILE_HEADER`."""
        header = bytearray(range(256)) * 2
        header[88:96] = start.to_bytes(8, "little")
        header[352:360] = (123).to_bytes(8, "little")  # BootTime
        header[360:368] = frequency.to_bytes(8, "little")
        return bytes(header)

    def test_samplys_times_on_windows_count_from_the_kernel_etl(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        etl = Path(folder.name) / "S.kernel.etl"
        # As in a trace of this desktop's: QPC at 10 MHz.
        etl.write_bytes(self.etl(5_011_197_876_670, 10_000_000))
        self.assertEqual(speedometer.etl_start_ns(etl), 501_119_787_667_000)
        etl.write_bytes(self.etl(7, 3))
        self.assertEqual(speedometer.etl_start_ns(etl), 2_333_333_333)
        # The samples' times: the ETL's start and their own, whatever
        # samply's `startTime` (when samply itself started).
        samples = Path(folder.name) / "S-machopo.samply.json.gz"
        speedometer.samply_etls(samples)[0].write_bytes(self.etl(5_000_000, 10_000_000))
        profile = {"meta": {"startTime": 1_792_503_871_043.5}, "libs": [{"debugName": "machopo", "codeId": "K"}],
                   "threads": [{"name": "CrRendererMain", "pid": 3, "stringArray": ["main"],
                                "funcTable": {"name": [0], "resource": [-1]}, "resourceTable": {"lib": []},
                                "frameTable": {"func": [0], "address": [0]},
                                "stackTable": {"prefix": [None], "frame": [0]},
                                "samples": {"time": [2.25, 10.5], "stack": [0, 0], "threadCPUDelta": [700, 900]}}]}
        samples.write_bytes(gzip.compress(json.dumps(profile).encode()))
        with mock.patch.object(speedometer.sys, "platform", "win32"):
            taken, pids = speedometer.read_samples(samples, "CrRendererMain")
        self.assertEqual(pids, {3})
        self.assertEqual([(stack, used) for _, stack, used in taken["cpu"]], [(["main"], 700), (["main"], 900)])
        # To within a µs: the wall clock's ms as a double don't hold every ns.
        for (time, _, _), expected in zip(taken["cpu"], (500_000_000 + 2_250_000, 500_000_000 + 10_500_000)):
            self.assertLess(abs(time - expected), 1000)

    def test_samply_with_the_symbols_beside(self):
        samples = Path(tempfile.gettempdir()) / "S-machopo.samply.json.gz"
        with mock.patch.object(speedometer.sys, "platform", "darwin"):
            command, flags = speedometer.samply_sampler(samples)
        self.assertEqual(command[:2], ["samply", "record"])
        self.assertEqual(command[-1], "--")
        self.assertEqual(command[command.index("-o") + 1], str(samples))
        self.assertEqual(command[command.index("-r") + 1], "1000")
        for option in ("--save-only", "--no-open", "--unstable-presymbolicate"):
            self.assertIn(option, command)
        self.assertIn("--perf-basic-prof", flags)
        self.assertIn(f"--logfile={samples.with_name('S-machopo.v8.log')}", flags)
        self.assertEqual(speedometer.samply_symbols(samples), samples.with_name("S-machopo.samply.json.syms.json"))
        self.assertEqual(speedometer.v8_log(samples.with_name("S-machopo.perf.data")), samples.with_name("S-machopo.v8.log"))

    def test_samplys_samples_are_named_and_counted_by_cpu_time(self):
        def thread(name, pid, samples):
            return {
                "name": name, "pid": pid,
                # 0 the root (machopo's, symbolized), 1 a JIT frame on it (at an
                # address machopo's symbols know: a JIT frame isn't the last
                # library's), 2 a system library's frame on 1, 3 machopo's frame
                # with no symbol, 4 a JIT frame on 0 that samply took from
                # V8's perf map, a library of its own.
                "stringArray": ["0x10", "JS:~run http://x/a.html:1:1", "0x20", "0x30", "JS:*go http://x/a.html:2:1"],
                "funcTable": {"name": [0, 1, 2, 3, 4], "resource": [2, -1, 1, 2, 0]},
                "resourceTable": {"lib": [2, 1, 0]},
                "frameTable": {"func": [0, 1, 2, 3, 4], "address": [16, 16, 32, 48, 7]},
                "stackTable": {"prefix": [None, 0, 1, 0, 0], "frame": [0, 1, 2, 3, 4]},
                "samples": samples,
            }
        profile = {
            "meta": {"startTime": 1000.5},
            "libs": [{"debugName": "machopo", "codeId": "K"}, {"debugName": "CoreText", "codeId": "C"},
                     {"debugName": "perf-7.map", "codeId": None}],
            "threads": [
                thread("CrRendererMain", 7, {"time": [2.25, 3.0, 4.0, 5.0, 6.0], "stack": [2, 1, None, 3, 4],
                                             "threadCPUDelta": [800, 0, 500, 300, 100]}),
                thread("Compositor", 7, {"time": [2.0], "stack": [0], "threadCPUDelta": [900]}),
            ],
        }
        symbols = {"string_table": ["main", "machopo::run"], "data": [
            {"code_id": "K", "symbol_table": [{"rva": 0, "size": 40, "symbol": 1}], "known_addresses": [[16, 0]]}]}
        taken, pids = speedometer.read_samply_profile(profile, symbols, "CrRendererMain", 1000)
        self.assertEqual(pids, {7})
        # Each time the wall clock's (ns) less the offset; a sample with no
        # CPU time, or no stack, isn't kept.
        self.assertEqual(taken, {"cpu": [
            (1_002_750_000 - 1000, ["[CoreText]", "JS:~run http://x/a.html:1:1", "machopo::run"], 800),
            (1_005_500_000 - 1000, ["[machopo]", "machopo::run"], 300),
            (1_006_500_000 - 1000, ["JS:*go http://x/a.html:2:1", "machopo::run"], 100),
        ]})
        # Each counts by its CPU time.
        self.assertEqual(speedometer.fold(taken["cpu"][:2], [(0, 2_000_000_000)]),
                         collections.Counter({"machopo::run;JS:~run http://x/a.html:1:1;[CoreText]": 800,
                                              "machopo::run;[machopo]": 300}))
        self.assertEqual(speedometer.totals_text({"cpu": collections.Counter({"a": 1100, "b": 400})}),
                         "1.5 ms of CPU time.")

    def test_each_events_stacks_have_a_file(self):
        folder = Path("s")
        self.assertEqual(speedometer.folded_path(folder, "A", "cycles"), folder / "A-machopo.folded")
        self.assertEqual(speedometer.folded_path(folder, "A", "instructions"), folder / "A-machopo.instructions.folded")

    def test_the_report_has_both_events(self):
        with tempfile.TemporaryDirectory() as results, mock.patch.object(speedometer, "RESULTS", Path(results)):
            report = speedometer.write_sample_report([("A", {
                "cycles": collections.Counter({"root;leaf": 3, "root;other": 1}),
                "instructions": collections.Counter({"root;leaf": 2, "root;busy": 6}),
            }, "the runner's frames")]).read_text(encoding="utf-8")
        self.assertIn("4 samples of 1,000,000 cycles, 8 of as many instructions (IPC 2.00). "
                      "The windows by the runner's frames.", report)
        self.assertIn("| Self, instructions | % | function |\n|---:|---:|---|\n| 6 | 75.0 | `busy` |", report)
        self.assertIn("| Self | % | function |\n|---:|---:|---|\n| 3 | 75.0 | `leaf` |", report)

    def test_machopo_is_built_from_a_tree_into_a_target(self):
        with mock.patch.object(speedometer.subprocess, "run") as run:
            run.return_value.returncode = 0
            binary = speedometer.machopo_binary("perf", Path("/w"), Path("/t"))
        self.assertEqual(run.call_args.kwargs["cwd"], Path("/w"))
        # As the platform writes the path (`\t` on Windows).
        self.assertEqual(run.call_args.kwargs["env"]["CARGO_TARGET_DIR"], str(Path("/t")))
        self.assertEqual(binary.parent, Path("/t/perf"))

    def test_the_perf_profile_keeps_frame_pointers(self):
        with mock.patch.dict(os.environ, {"RUSTFLAGS": "-C opt"}):
            perf = speedometer.build_env("perf")["RUSTFLAGS"]
            dev = speedometer.build_env("dev")["RUSTFLAGS"]
        if sys.platform == "darwin" or sys.platform.startswith("linux"):
            self.assertEqual(perf, "-C opt -C force-frame-pointers=yes")
        else:
            self.assertEqual(perf, "-C opt")
        self.assertEqual(dev, "-C opt")

    def test_machopo_is_built_so(self):
        with mock.patch.object(speedometer.subprocess, "run") as run:
            run.return_value.returncode = 0
            speedometer.machopo_binary("perf")
        self.assertEqual(run.call_args.kwargs["env"], speedometer.build_env("perf"))

    def test_on_windows_the_perf_profiles_exe_names_its_pdb_whole(self):
        pdb = Path("/t") / "perf" / "machopo.pdb"
        config = ["--config", f"target.x86_64-pc-windows-msvc.rustflags=['-C', 'link-arg=/PDBALTPATH:{pdb}']"]
        with mock.patch.object(speedometer.sys, "platform", "win32"):
            self.assertEqual(speedometer.build_options("perf", Path("/t")), config)
            self.assertEqual(speedometer.build_options("dev", Path("/t")), [])
            # Into a target of its own, `--against`'s, the PDB is that one's.
            with mock.patch.object(speedometer.subprocess, "run") as run:
                run.return_value.returncode = 0
                speedometer.machopo_binary("perf", Path("/w"), Path("/t"))
            self.assertEqual(run.call_args.args[0][-2:], config)
        with mock.patch.object(speedometer.sys, "platform", "linux"):
            self.assertEqual(speedometer.build_options("perf", Path("/t")), [])


class WindowsNamesTests(unittest.TestCase):
    """On Windows samply leaves Windows's own DLLs' frames nameless when the
    DLLs are on disk; they are named after, through `samply load`."""

    @staticmethod
    def profile() -> dict:
        def thread(name):
            return {
                "name": name, "pid": 4,
                # 0 machopo's, which samply named; 1 ntdll's, named by samply as
                # `fun_…`; 2 vcruntime's, only an address; 3 a JIT frame, its
                # library without a code id; 4 a frame of no library; 5 a JIT
                # frame of only an address.
                "stringArray": ["0x10", "0x20", "0x30", "JS:~f http://x/a.js:1:1", "0x99", "go", "0x77"],
                "funcTable": {"name": [0, 1, 2, 3, 5, 6], "resource": [0, 1, 2, 3, -1, 3]},
                "resourceTable": {"lib": [0, 1, 2, 3]},
                "frameTable": {"func": [0, 1, 2, 3, 4, 5], "address": [16, 32, 48, 7, 9, 77]},
                "stackTable": {"prefix": [None, 0, 1, 0, 0], "frame": [0, 1, 2, 3, 4]},
                "samples": {"time": [1.0, 2.0], "stack": [2, 3], "threadCPUDelta": [10, 20]},
            }
        return {"meta": {"startTime": 0.0},
                "libs": [{"name": "machopo.exe", "debugName": "machopo.pdb", "codeId": "K", "breakpadId": "KB",
                          "path": "C:\\machopo\\machopo.exe", "debugPath": "C:\\machopo\\machopo.pdb"},
                         {"name": "ntdll.dll", "debugName": "ntdll.pdb", "codeId": "N", "breakpadId": "NB",
                          "path": "C:\\Windows\\System32\\ntdll.dll", "debugPath": "ntdll.pdb"},
                         {"name": "vcruntime140.dll", "debugName": "vcruntime140.amd64.pdb", "codeId": "V",
                          "breakpadId": "VB", "path": "C:\\Windows\\System32\\vcruntime140.dll",
                          "debugPath": "vcruntime140.amd64.pdb"},
                         {"name": "JS JIT", "debugName": "JS JIT", "codeId": None, "breakpadId": "0",
                          "path": "JS JIT", "debugPath": "JS JIT"}],
                "threads": [thread("CrRendererMain"), thread("Compositor")]}

    SYMBOLS = {"string_table": ["machopo::main", "fun_20"], "data": [
        {"code_id": "K", "symbol_table": [{"rva": 0, "size": 20, "symbol": 0}], "known_addresses": [[16, 0]]},
        {"code_id": "N", "symbol_table": [{"rva": 32, "size": 8, "symbol": 1}], "known_addresses": [[32, 0]]}]}

    def test_the_nameless_frames_by_library(self):
        profile = self.profile()
        profile["threads"][1]["frameTable"]["address"] = [16, 33, 49, 7, 9, 78]
        # Of the thread's: not machopo's named one, the JIT's or the one of no
        # library; not the other thread's.
        self.assertEqual(speedometer.nameless_frames(profile, self.SYMBOLS, "CrRendererMain"), {1: {32}, 2: {48}})

    def test_what_is_found_names_a_frame_first(self):
        profile = self.profile()
        taken, _ = speedometer.read_samply_profile(profile, self.SYMBOLS, "CrRendererMain", 0)
        self.assertEqual([stack for _, stack, _ in taken["cpu"]],
                         [["[vcruntime140.amd64.pdb]", "fun_20", "machopo::main"], ["JS:~f http://x/a.js:1:1", "machopo::main"]])
        found = {("N", 32): "RtlpLowFragHeapAllocFromContext", ("V", 48): "memcpy", ("K", 16): "machopo::found"}
        taken, _ = speedometer.read_samply_profile(profile, self.SYMBOLS, "CrRendererMain", 0, found)
        self.assertEqual(taken["cpu"][0][1], ["memcpy", "RtlpLowFragHeapAllocFromContext", "machopo::found"])

    def test_windows_dlls_are_looked_up_without_their_paths(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        samples = Path(folder.name) / "S-machopo.samply.json.gz"
        profile = self.profile()
        asked = []

        def symbolicate(copy, modules, frames):
            given = json.loads(copy.read_text(encoding="utf-8"))
            asked.append((copy.name, [(lib["path"], lib["debugPath"]) for lib in given["libs"]], modules, frames,
                          len(given["threads"])))
            return ["RtlpLowFragHeapAllocFromContext", "RtlFreeHeap", None]

        with mock.patch.object(speedometer, "symbolicate", side_effect=symbolicate):
            found = speedometer.windows_names(samples, profile, {2: {48}, 1: {40, 32}})
        hidden = Path(folder.name) / "nowhere"
        self.assertEqual(asked, [("S-machopo.samply.nameless.json", [
            ("C:\\machopo\\machopo.exe", "C:\\machopo\\machopo.pdb"),
            (str(hidden / "ntdll.dll"), str(hidden / "ntdll.pdb")),
            (str(hidden / "vcruntime140.dll"), str(hidden / "vcruntime140.amd64.pdb")),
            ("JS JIT", "JS JIT")], [("ntdll.pdb", "NB"), ("vcruntime140.amd64.pdb", "VB")],
            [(0, 32), (0, 40), (1, 48)], 2)])
        self.assertEqual(found, {("N", 32): "RtlpLowFragHeapAllocFromContext", ("N", 40): "RtlFreeHeap"})
        # The copy is gone, and the profile as it was.
        self.assertEqual(list(Path(folder.name).iterdir()), [])
        self.assertEqual(profile, self.profile())
        # Nothing nameless: no lookup. A lookup that fails: nothing found.
        with mock.patch.object(speedometer, "symbolicate", side_effect=AssertionError("looked up")):
            self.assertEqual(speedometer.windows_names(samples, profile, {}), {})
        out = tempfile.TemporaryFile("w+", encoding="utf-8")
        self.addCleanup(out.close)
        with mock.patch.object(speedometer, "symbolicate", side_effect=RuntimeError("no server")), \
                mock.patch("sys.stdout", new=out):
            self.assertEqual(speedometer.windows_names(samples, profile, {1: {32}}), {})
        out.seek(0)
        self.assertIn("Windows's DLLs not named: no server", out.read())
        self.assertEqual(list(Path(folder.name).iterdir()), [])

    def test_on_windows_the_samples_frames_are_named_so(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        samples = Path(folder.name) / "S-machopo.samply.json.gz"
        samples.write_bytes(gzip.compress(json.dumps(self.profile()).encode()))
        speedometer.samply_symbols(samples).write_text(json.dumps(self.SYMBOLS), encoding="utf-8")
        speedometer.samply_etls(samples)[0].write_bytes(SampleReadingTests.etl(0, 10_000_000))
        looked_up = []

        def windows_names(given, profile, nameless):
            looked_up.append((given, nameless))
            return {("V", 48): "memcpy"}

        with mock.patch.object(speedometer.sys, "platform", "win32"), \
                mock.patch.object(speedometer, "windows_names", side_effect=windows_names):
            taken, _ = speedometer.read_samples(samples, "CrRendererMain")
        self.assertEqual(looked_up, [(samples, {1: {32}, 2: {48}})])
        self.assertEqual(taken["cpu"][0][1], ["memcpy", "fun_20", "machopo::main"])
        # Elsewhere samply names them itself.
        with mock.patch.object(speedometer.sys, "platform", "darwin"), \
                mock.patch.object(speedometer.time, "clock_gettime_ns", lambda clock: 0, create=True), \
                mock.patch.object(speedometer.time, "CLOCK_UPTIME_RAW", 8, create=True), \
                mock.patch.object(speedometer, "windows_names", side_effect=AssertionError("looked up")):
            speedometer.read_samples(samples, "CrRendererMain")

    @unittest.skipUnless(shutil.which("samply"), "no samply")
    def test_samply_load_answers_what_it_knows(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        profile = Path(folder.name) / "P.json"
        profile.write_text(json.dumps({"meta": {"version": 24, "startTime": 0, "interval": 1, "product": "machopo"},
                                       "libs": [], "threads": []}), encoding="utf-8")
        # A module it doesn't have: no name, quickly, and the server ended.
        started = []
        real = subprocess.Popen

        def popen(*args, **options):
            started.append(real(*args, **options))
            return started[-1]

        with mock.patch.object(speedometer.subprocess, "Popen", popen):
            self.assertEqual(speedometer.symbolicate(profile, [("machopo-none.pdb", "0" * 33)], [(0, 16), (0, 32)]),
                             [None, None])
        self.assertIsNotNone(started[0].poll())

    def test_a_samply_load_that_says_nothing_is_given_up(self):
        quiet = mock.Mock(stdout=io.StringIO("Loading profile\n"))
        with mock.patch.object(speedometer.subprocess, "Popen", return_value=quiet), \
                mock.patch.object(speedometer, "SAMPLY_LOAD_START", 0.2), \
                self.assertRaisesRegex(RuntimeError, "samply load didn't start"):
            speedometer.symbolicate(Path("P.json"), [], [])
        quiet.kill.assert_called_once()
        self.assertTrue(quiet.stdout.closed)


class ElevatedTests(unittest.TestCase):
    """On Windows the samples are taken as Administrator, in one copy of
    the script for all of a run's jobs."""

    def setUp(self):
        results = tempfile.TemporaryDirectory()
        self.addCleanup(results.cleanup)
        self.results = Path(results.name)
        patch = mock.patch.object(speedometer, "RESULTS", self.results)
        patch.start()
        self.addCleanup(patch.stop)
        self.jobs = [("against, run 1 of 1", Path("/a/machopo"), ["S", "T"], self.results / "a", 2),
                     ("", Path("/c/machopo"), ["S"], self.results / "c", 2)]
        self.sampled = []

    def sample(self, binary, names, folder, iterations):
        self.sampled.append((binary, names, folder, iterations))
        return [(name, {"cpu": collections.Counter({f"{binary.parent.name};{name}": 5})}, "the runner's marks")
                for name in names], binary.parent.name == "a"

    def test_elsewhere_or_as_administrator_each_job_in_turn_here(self):
        for platform_name, admin in (("linux", None), ("darwin", None), ("win32", True)):
            self.sampled = []
            with mock.patch.object(speedometer.sys, "platform", platform_name), \
                    mock.patch.object(speedometer, "is_admin", lambda: admin), \
                    mock.patch.object(speedometer, "sample_elevated", side_effect=AssertionError("elevated")), \
                    mock.patch.object(speedometer, "sample_suites", side_effect=self.sample), \
                    mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")):
                results = speedometer.sample_all(self.jobs)
            self.assertEqual(self.sampled, [(Path("/a/machopo"), ["S", "T"], self.results / "a", 2),
                                            (Path("/c/machopo"), ["S"], self.results / "c", 2)])
            self.assertEqual([failed for _, failed in results], [True, False])

    def test_on_windows_all_the_jobs_in_one_copy_as_administrator(self):
        started = []

        # The copy, run here: in an environment and a folder of its own,
        # with the module's globals as they were when it started.
        def run_elevated(arguments, log):
            started.append(arguments)
            self.assertEqual(arguments[0], str(Path(speedometer.__file__).resolve()))
            self.assertEqual(arguments[1], "--elevated")
            with mock.patch.dict(os.environ, {"MACHOPO_V8_FLAGS": "lost"}, clear=True), \
                    mock.patch.object(speedometer.os, "chdir") as chdir, \
                    mock.patch.object(speedometer, "RESULTS", Path("/elsewhere")), \
                    mock.patch.object(speedometer, "is_admin", lambda: True), \
                    mock.patch("sys.stdout"), mock.patch("sys.stderr"):
                status = speedometer.elevated_main(Path(arguments[2]))
                self.assertEqual(os.environ["MACHOPO_V8_FLAGS"], "--given")
                self.assertEqual(speedometer.RESULTS, self.results)
                chdir.assert_called_once_with(os.getcwd())
            return status

        with mock.patch.object(speedometer.sys, "platform", "win32"), \
                mock.patch.object(speedometer, "is_admin", lambda: False), \
                mock.patch.object(speedometer, "run_elevated", side_effect=run_elevated), \
                mock.patch.object(speedometer, "sample_suites", side_effect=self.sample), \
                mock.patch.dict(os.environ, {"MACHOPO_V8_FLAGS": "--given"}), \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")):
            results = speedometer.sample_all(self.jobs)
        self.assertEqual(len(started), 1)
        self.assertEqual(self.sampled, [(Path("/a/machopo"), ["S", "T"], self.results / "a", 2),
                                        (Path("/c/machopo"), ["S"], self.results / "c", 2)])
        # Its rows come back as they were taken.
        self.assertEqual(results, [
            ([("S", {"cpu": collections.Counter({"a;S": 5})}, "the runner's marks"),
              ("T", {"cpu": collections.Counter({"a;T": 5})}, "the runner's marks")], True),
            ([("S", {"cpu": collections.Counter({"c;S": 5})}, "the runner's marks")], False)])
        self.assertIsInstance(results[0][0][0][1]["cpu"], collections.Counter)
        # What it printed is in its log, the jobs' names first.
        self.assertIn("against, run 1 of 1:", (self.results / "elevated.log").read_text(encoding="utf-8"))

    def test_a_copy_that_fails_gives_nothing_back(self):
        def failing(binary, names, folder, iterations):
            raise RuntimeError("no machopo")

        def run_elevated(arguments, log):
            with mock.patch.object(speedometer.os, "chdir"), mock.patch.object(speedometer, "is_admin", lambda: True), \
                    mock.patch.dict(os.environ), mock.patch("sys.stdout"), mock.patch("sys.stderr"):
                return speedometer.elevated_main(Path(arguments[2]))

        with mock.patch.object(speedometer.sys, "platform", "win32"), \
                mock.patch.object(speedometer, "is_admin", lambda: False), \
                mock.patch.object(speedometer, "run_elevated", side_effect=run_elevated), \
                mock.patch.object(speedometer, "sample_suites", side_effect=failing), \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")), \
                self.assertRaisesRegex(SystemExit, r"gave nothing back \(exit status 1\)"):
            speedometer.sample_all(self.jobs)
        self.assertIn("RuntimeError: no machopo", (self.results / "elevated.log").read_text(encoding="utf-8"))
        # Declined, the same.
        with mock.patch.object(speedometer.sys, "platform", "win32"), \
                mock.patch.object(speedometer, "is_admin", lambda: False), \
                mock.patch.object(speedometer, "run_elevated", return_value=1), \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")), \
                self.assertRaises(SystemExit):
            speedometer.sample_all(self.jobs)

    def test_on_windows_sampling_takes_xperf(self):
        args = argparse.Namespace(iterations=1)
        out = tempfile.TemporaryFile("w+", encoding="utf-8")
        self.addCleanup(out.close)
        with mock.patch.object(speedometer.sys, "platform", "win32"), \
                mock.patch.object(speedometer.shutil, "which", lambda name: f"/bin/{name}" if name == "samply" else None), \
                mock.patch.object(speedometer, "xperf", lambda: None), \
                mock.patch.object(speedometer, "machopo_binary", side_effect=AssertionError("built")), \
                mock.patch("sys.stdout", new=out):
            self.assertEqual(speedometer.cmd_sample(args, [("S", True)], None), 1)
        out.seek(0)
        self.assertIn("Windows Performance Toolkit", out.read())

    def test_the_report_tells_how_windows_samples(self):
        with mock.patch.object(speedometer, "sampling", lambda: speedometer.WINDOWS_SAMPLING):
            report = speedometer.write_sample_report([("A", {"cpu": collections.Counter({"r;l": 1500})},
                                                       "the runner's marks")]).read_text(encoding="utf-8")
        self.assertIn("(the `perf` profile, unwind tables), `samply record` at 1,000 Hz (ETW)", report)
        self.assertIn("the kernel's frames included, with V8's `--enable-etw-stack-walking`;", report)
        self.assertIn("1.5 ms of CPU time.", report)
        with mock.patch.object(speedometer, "sampling", lambda: speedometer.MACOS_SAMPLING):
            report = speedometer.write_sample_report([("A", {"cpu": collections.Counter({"r;l": 1500})},
                                                       "the runner's marks")]).read_text(encoding="utf-8")
        self.assertIn("(the `perf` profile, frame pointers), `samply record` at 1,000 Hz (frame pointers)", report)
        self.assertIn("with V8's `--perf-basic-prof`;", report)


class PhasesTests(unittest.TestCase):
    """`--trace`'s phases: each phase's self time in the timed windows."""

    @staticmethod
    def trace(*threads) -> bytes:
        """A trace of `threads`, each `(uuid, name, events)`, an event
        `(time, kind, name)` (kind 1 begins, 2 ends, 3 is a User Timing
        mark), on its own sequence."""
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

        out = b""
        for sequence, (uuid, name, events) in enumerate(threads, 1):
            out += message((1, message((10, sequence), (60, message((1, uuid), (4, message((5, name.encode()))))),
                                       (59, message((11, message((11, uuid)))))
                                       )))
            for time, kind, event_name in events:
                categories = [(22, b"blink.user_timing")] if kind == 3 else []
                out += message((1, message((10, sequence), (8, time), (58, 6),
                                           (11, message((9, kind), (23, event_name.encode()), *categories)))))
        return out

    def test_each_phase_takes_its_slices_self_time_in_the_windows(self):
        ms = 1_000_000
        main = [
            (100 * ms, 3, "S.A-start"), (200 * ms, 3, "S.A-async-end"),
            # Script, with style forced in it: 30 ms of script, 10 of style.
            (110 * ms, 1, "v8.callFunction"), (120 * ms, 1, "Document::recalcStyle"), (130 * ms, 2, ""),
            (150 * ms, 2, ""),
            # Layout from before the window into it: 5 ms in it.
            (95 * ms, 1, "LocalFrameView::performLayout"), (105 * ms, 2, ""),
            # Something no phase takes, and GC.
            (160 * ms, 1, "Mystery"), (164 * ms, 2, ""), (170 * ms, 1, "V8.GC_SCAVENGER_SCAVENGE"), (172 * ms, 2, ""),
            # Script past the window's end: 5 ms in it.
            (195 * ms, 1, "FunctionCall"), (205 * ms, 2, ""),
            # After the window: nothing.
            (210 * ms, 1, "v8.callFunction"), (230 * ms, 2, ""),
        ]
        # Another renderer process's main thread, with fewer of the
        # runner's marks: not read.
        other = [(100 * ms, 3, "S.B-start"), (110 * ms, 1, "Document::recalcStyle"), (190 * ms, 2, "")]
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.pftrace"
            path.write_bytes(self.trace((5, "CrRendererMain", other), (7, "CrRendererMain", main)))
            phases, others = speedometer.phase_times(path, "Chrome")
        self.assertEqual(dict(phases), {"script": 35 * ms, "style": 10 * ms, "layout": 5 * ms, "GC": 2 * ms})
        self.assertEqual(dict(others), {"Mystery": 4 * ms})

    def test_each_browser_names_its_phases(self):
        self.assertEqual(speedometer.slice_phase("machopo", "Document::UpdateStyle"), "style")
        self.assertEqual(speedometer.slice_phase("Chrome", "Document::UpdateStyle"), None)
        self.assertEqual(speedometer.slice_phase("machopo", "StyleEngine::RebuildLayoutTree"), "box tree")
        self.assertEqual(speedometer.slice_phase("Chrome", "Document::rebuildLayoutTree"), "box tree")
        self.assertEqual(speedometer.slice_phase("Chrome", "InlineNode::ShapeTextIncludingFirstLine"), "layout")
        self.assertEqual(speedometer.slice_phase("machopo", "InlineNode::ShapeTextIncludingFirstLine"), None)
        self.assertEqual(speedometer.slice_phase("machopo", "V8.GC_MC_INCREMENTAL"), "GC")
        self.assertEqual(speedometer.slice_phase("machopo", "Gc.Scavenge"), "GC")
        self.assertEqual(speedometer.slice_phase("Chrome", "CppGC.IncrementalSweep"), "GC")
        self.assertEqual(speedometer.slice_phase("machopo", "V8.DeoptimizeCode"), "script")
        self.assertEqual(speedometer.slice_phase("machopo", "HTMLDocumentParser::PumpTokenizer"), "parse")
        self.assertEqual(speedometer.slice_phase("Chrome", "CanvasRenderingContext2D::fillRect"), "canvas")

    def test_the_report(self):
        ms = 1_000_000
        times = {
            "S": {"machopo": (collections.Counter({"style": 30 * ms, "script": 60 * ms}), collections.Counter({"X": 3 * ms})),
                  "Chrome": (collections.Counter({"style": 6 * ms, "script": 30 * ms}), collections.Counter())},
            "T": {"machopo": (collections.Counter({"style": 3 * ms}), collections.Counter({"X": 3 * ms, "Y": 9 * ms})),
                  "Chrome": (collections.Counter({"style": 3 * ms}), collections.Counter())},
        }
        env = {"date": "2026-10-19", "machopo": "abc", "profile": "perf", "chrome": "152", "os": "macOS",
               "cpu": "M4", "power": "AC"}
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(speedometer, "RESULTS", Path(folder)):
            text = speedometer.write_phases_report(times, env, 3).read_text(encoding="utf-8")
            written = json.loads((Path(folder) / "phases.json").read_text(encoding="utf-8"))
            only_machopo = {suite: {"machopo": by_browser["machopo"]} for suite, by_browser in times.items()}
            machopo_text = speedometer.write_phases_report(only_machopo, env, 3).read_text(encoding="utf-8")
        # Totals, ms an iteration, machopo's more.
        self.assertIn("| style | 11.0 | 3.0 | +8.0 |", text)
        self.assertIn("| script | 20.0 | 10.0 | +10.0 |", text)
        self.assertIn("| S | 20.0 / 10.0 (+10.0) | 10.0 / 2.0 (+8.0) | 0.0 / 0.0 (+0.0) |", text)
        self.assertIn("- `Y`: 3.0\n- `X`: 2.0", text)
        self.assertEqual(written["T"]["Chrome"], {"style": 1.0})
        self.assertIn("| style | 11.0 |", machopo_text)
        self.assertNotIn("Chrome's that no phase", machopo_text)


class CompareTests(unittest.TestCase):
    """`--sample --against REV`: machopo at another commit and the working
    tree's, sampled in turn."""

    def test_another_version_must_pin_third_party_alike(self):
        same = {path: (speedometer.ROOT / path).read_text(encoding="utf-8") for path in speedometer.PIN_FILES}
        with mock.patch.object(common, "git_show", side_effect=lambda rev, path: same[path]):
            self.assertEqual(speedometer.against_problems("HEAD~1"), [])
        other = dict(same, **{"third_party.toml": same["third_party.toml"] + "# moved\n"})
        with mock.patch.object(common, "git_show", side_effect=lambda rev, path: other[path]):
            self.assertEqual(len(speedometer.against_problems("HEAD~1")), 1)
        with mock.patch.object(common, "git_show", return_value=None):
            self.assertEqual(len(speedometer.against_problems("nothing")), 2)

    def test_each_suites_medians_ranges_and_changes(self):
        def run(cycles, instructions, leaves):
            return {"cycles": collections.Counter({"r;x": cycles}),
                    "instructions": collections.Counter({f"r;{f}": n for f, n in leaves.items()}) or
                    collections.Counter({"r;x": instructions})}
        runs = {
            "against": [run(100, 0, {"a": 50, "b": 10}), run(120, 0, {"a": 54, "b": 10}),
                        run(110, 0, {"a": 52, "b": 12})],
            "current": [run(90, 0, {"a": 40, "c": 9}), run(95, 0, {"a": 42, "c": 9}),
                        run(99, 0, {"a": 41, "c": 8})],
        }
        text = "\n".join(speedometer.compare_suite("S", runs))
        self.assertIn("| cycles | 110 (100–120) | 95 (90–99) | -13.6% |", text)
        self.assertIn("| instructions | 64 (60–64) | 49 (49–51) | -23.4% |", text)
        self.assertNotIn("| cpu |", text)
        self.assertIn("against 100, 120, 110; current 90, 95, 99 (cycles)", text)
        fewer = text[text.index("Fewer"):text.index("More")]
        self.assertIn("| 52 | 41 | `a` |\n| 10 | 0 | `b` |", fewer)
        self.assertIn("| 0 | 9 | `c` |", text[text.index("More"):])

    def test_macoss_cpu_time_compared(self):
        def run(cpu):
            return {"cpu": collections.Counter({f"r;{f}": n for f, n in cpu.items()})}
        runs = {"against": [run({"a": 500, "b": 100}), run({"a": 520, "b": 90}), run({"a": 510, "b": 110})],
                "current": [run({"a": 400}), run({"a": 410}), run({"a": 405})]}
        # Millions of µs written out, not in exponents.
        big = {"against": [run({"a": 2_773_970})], "current": [run({"a": 2_500_170})]}
        big_text = "\n".join(speedometer.compare_suite("S", big))
        self.assertIn("| cpu | 2773970 (2773970–2773970) | 2500170 (2500170–2500170) | -9.9% |", big_text)
        self.assertIn("| 2773970 | 2500170 | `a` |", big_text)
        text = "\n".join(speedometer.compare_suite("S", runs))
        self.assertIn("| cpu | 610 (600–620) | 405 (400–410) | -33.6% |", text)
        self.assertNotIn("| cycles |", text)
        self.assertIn("against 600, 610, 620; current 400, 410, 405 (cpu)", text)
        self.assertIn("Fewer µs of CPU time (self, the runs' medians):", text)
        self.assertIn("| 510 | 405 | `a` |", text)

    def test_the_versions_take_turns(self):
        order = []

        # The other version from before machopo wrote User Timing: each side's
        # windows found its own way, which the report tells.
        def sample(binary, names, folder, iterations):
            order.append((binary.name, folder.name))
            found_by = "the runner's frames" if binary.name == "against" else "the runner's marks"
            return [("S", {"cycles": collections.Counter({"r": 1}),
                           "instructions": collections.Counter({"r": 2})}, found_by)], False

        args = argparse.Namespace(against="HEAD~1", runs=2, iterations=1)
        with tempfile.TemporaryDirectory() as results, \
                mock.patch.object(speedometer, "RESULTS", Path(results)), \
                mock.patch.object(speedometer, "against_problems", return_value=[]), \
                mock.patch.object(common, "against_tree", return_value=Path("/w")), \
                mock.patch.object(common, "machopo_binary",
                                  side_effect=lambda profile, root=None, target=None: Path(
                                      "against" if target else "current")), \
                mock.patch.object(speedometer, "sample_suites", side_effect=sample), \
                mock.patch.object(speedometer, "is_admin", lambda: True), \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")):
            self.assertEqual(speedometer.cmd_compare(args, ["S"]), 0)
            report = (Path(results) / "compare.md").read_text(encoding="utf-8")
        self.assertEqual(order, [("against", "against-1"), ("current", "current-1"),
                                 ("against", "against-2"), ("current", "current-2")])
        self.assertIn("against 1, 1; current 1, 1 (cycles)", report)
        self.assertIn("The timed windows: against's by the runner's frames; current's by the runner's marks.", report)

    def test_a_version_pinning_otherwise_isnt_built(self):
        args = argparse.Namespace(against="HEAD~1", runs=1, iterations=1)
        with mock.patch.object(speedometer, "against_problems", return_value=["differs"]), \
                mock.patch.object(speedometer, "machopo_binary") as build, \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")):
            self.assertEqual(speedometer.cmd_compare(args, ["S"]), 1)
        build.assert_not_called()

    def test_without_sample_the_versions_are_timed_in_turn(self):
        order = []

        def time_suites(binary, names, iterations):
            order.append((binary.name, tuple(names), iterations))
            run = {"status": "done", "errors": [], "summaries": [], "iterations": [{"list": {"total": 5.0}}]}
            return {name: (run, 1 << 20) for name in names}

        args = argparse.Namespace(suites="list", against="HEAD~1", sample=False, inventory=False, trace=False,
                                  runs=2, iterations=4)
        with tempfile.TemporaryDirectory() as results, \
                mock.patch.object(speedometer, "RESULTS", Path(results)), \
                mock.patch.object(speedometer, "SPEEDOMETER", FAKE), \
                mock.patch.object(speedometer, "suites", return_value=[("list", True), ("css", True)]), \
                mock.patch.object(speedometer, "against_problems", return_value=[]), \
                mock.patch.object(speedometer, "against_binaries",
                                  return_value={"against": Path("against"), "current": Path("current")}), \
                mock.patch.object(speedometer, "time_suites", side_effect=time_suites), \
                mock.patch("sys.stdout", new=open(os.devnull, "w", encoding="utf-8")):
            self.assertEqual(speedometer.cmd_speedometer(args), 0)
            report = (Path(results) / "compare.md").read_text(encoding="utf-8")
        self.assertEqual(order, [("against", ("list",), 4), ("current", ("list",), 4)] * 2)
        self.assertIn("# machopo against another version, in Speedometer's times", report)
        self.assertIn("| list | 5.0 | 5.0 | +0.0% | 1 MB | 1 MB | +0.0% |", report)

    def test_times_and_memory_compared(self):
        def run(totals, status="done"):
            return {"status": status, "errors": [], "summaries": [],
                    "iterations": [{"A": {"total": t}, "B": {"total": t}, "C": {"total": t}} for t in totals]}
        mib = 1 << 20
        by_suite = {
            # Every iteration of every run: the median of all of them, not
            # of the first run's (11).
            "A": {"against": [(run([10, 11, 12]), 100 * mib), (run([13, 14, 15]), 100 * mib),
                              (run([13, 14, 15]), 100 * mib)],
                  "current": [(run([9, 10, 11]), 110 * mib), (run([9, 10, 11]), 108 * mib),
                              (run([9, 10, 11]), None)]},
            "B": {"against": [(run([5]), mib)], "current": [(run([5], "stalled"), mib)]},
            "C": {"against": [(run([10]), mib)], "current": [(run([20]), mib)]},
        }
        env = {"os": "macOS", "cpu": "arm", "power": "AC"}
        with tempfile.TemporaryDirectory() as results, mock.patch.object(speedometer, "RESULTS", Path(results)):
            text = speedometer.write_time_report("HEAD", 3, 3, by_suite, env).read_text(encoding="utf-8")
        row = next(line for line in text.splitlines() if line.startswith("| A | "))
        # Every iteration of the runs; the memory the median of the runs
        # that read it.
        self.assertRegex(row, r"^\| A \| 13\.0 \([\d.]+–[\d.]+\) \| 10\.0 \([\d.]+–[\d.]+\) \| -23\.1% \| "
                              r"100 MB \| 109 MB \| \+9\.0% \|$")
        self.assertIn("| B | not valid: current: stalled | | | | | |", text)
        # sqrt(10/13 × 20/10), not their mean (1.385).
        self.assertIn("The geometric mean of the 2 valid suites' current / against: 1.240 (+24.0%).", text)
        self.assertIn("Peak memory: +0.0% to +9.0% by suite.", text)
        self.assertIn("| A | 11.0, 14.0, 14.0 | 10.0, 10.0, 10.0 |", text)


class MemoryTests(unittest.TestCase):
    """`peak_memory`: the most a process has had of its own memory."""

    @unittest.skipUnless(sys.platform == "darwin" or sys.platform.startswith("linux"), "macOS's or Linux's")
    def test_another_processs_peak_stays(self):
        # A new process (as machopo is) touches 64 MB and gives it back; its
        # peak keeps them.
        child = subprocess.Popen(
            [sys.executable, "-c", "import mmap, time\n"
             "m = mmap.mmap(-1, 64 << 20)\n"
             "for i in range(0, len(m), 4096): m[i] = 1\n"
             "m.close()\n"
             "print('ready', flush=True)\n"
             "time.sleep(60)\n"],
            stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(child.stdout.readline().strip(), "ready")
            self.assertGreaterEqual(speedometer.peak_memory(child.pid), 64 << 20)
        finally:
            child.kill()
            child.wait()

    def test_linuxs_peak_resident_set(self):
        self.assertEqual(speedometer.linux_peak_rss("Name:\tmachopo\nVmPeak:\t 9 kB\nVmHWM:\t    2048 kB\nVmRSS:\t 1 kB\n"),
                         2048 * 1024)
        self.assertIsNone(speedometer.linux_peak_rss("Name:\tmachopo\n"))

    def test_no_process_no_memory(self):
        self.assertIsNone(speedometer.peak_memory(2**22 + 12345))


class SampleTests(unittest.TestCase):
    """`bench.py speedometer --sample` on the fake `frames` suite: machopo (the dev
    profile; on Windows the `perf` one, whose exe names its PDB so that
    samply finds it) under Linux's `perf` or samply (macOS; Windows, only
    when the tests run as Administrator: else UAC would ask)."""

    @classmethod
    def setUpClass(cls):
        way = speedometer.sampling()
        if way is None or shutil.which(way.tool) is None:
            raise unittest.SkipTest("no Linux perf or samply")
        if sys.platform == "win32" and (speedometer.xperf() is None or not speedometer.is_admin()):
            raise unittest.SkipTest("on Windows, sampling takes xperf and Administrator")

    def setUp(self):
        results = tempfile.TemporaryDirectory()
        self.addCleanup(results.cleanup)
        self.results = Path(results.name)
        profile = "perf" if sys.platform == "win32" else "dev"
        for name, value in (("RESULTS", self.results), ("SPEEDOMETER", FAKE), ("MACHOPO_PROFILE", profile)):
            patch = mock.patch.object(speedometer, name, value)
            patch.start()
            self.addCleanup(patch.stop)

    def test_the_stacks_in_the_timed_windows(self):
        args = argparse.Namespace(iterations=1)
        maps = set(Path("/tmp").glob("perf-*.map"))
        logs = set(speedometer.ROOT.glob("isolate-*.log"))
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            self.assertEqual(speedometer.cmd_sample(args, [("frames", True)], None), 0)
        folded = (self.results / "sample" / "frames-machopo.folded").read_text(encoding="utf-8").splitlines()
        stacks = [line.rsplit(" ", 1)[0].split(";") for line in folded]
        self.assertTrue(stacks)
        # V8 names the test's JS function, and the stack goes on through it
        # into machopo's call of it, the runner's animation frame callback's (on
        # Linux the dev profile has no frame pointers: from machopo's own code,
        # the stack ends before it gets to the JS; arm64 macOS always keeps
        # them).
        self.assertTrue(any(any("makeGarbage" in frame for frame in stack)
                            and any("dom_js::listeners::call_animation_frame" in frame for frame in stack)
                            for stack in stacks))
        # The trace makes no log records: html5ever's for each token would
        # be made only to be dropped.
        self.assertFalse(any("tracing_log" in frame or "to_escaped_string" in frame
                             for stack in stacks for frame in stack))
        report = (self.results / "sample.md").read_text(encoding="utf-8")
        self.assertIn("## frames", report)
        self.assertIn("The windows by the runner's marks.", report)
        if sys.platform in ("darwin", "win32"):
            self.assertRegex(report, r"\d+\.\d ms of CPU time\.")
            files = ["frames-machopo.folded", "frames-machopo.pftrace", "frames-machopo.samply.json.gz",
                     "frames-machopo.samply.json.syms.json"]
        else:
            self.assertRegex(report, r"\d+ samples of 1,000,000 cycles, \d+ of as many instructions \(IPC")
            self.assertTrue((self.results / "sample" / "frames-machopo.instructions.folded").read_text(encoding="utf-8"))
            files = ["frames-machopo.folded", "frames-machopo.instructions.folded", "frames-machopo.perf.data",
                     "frames-machopo.pftrace"]
        # V8's names for the JS are gone with the run, and it left no log.
        self.assertEqual(set(Path("/tmp").glob("perf-*.map")) - maps, set())
        self.assertEqual(set(speedometer.ROOT.glob("isolate-*.log")) - logs, set())
        self.assertEqual(sorted(path.name for path in (self.results / "sample").iterdir()), files)


class CategoryTests(unittest.TestCase):
    """How `categories` puts a sample in a kind of work, and what it makes
    of both browsers' samples."""

    def kind(self, *stack):
        # As folded: the root first.
        return categories.category(list(stack))

    def test_machopos_frames_by_crate(self):
        self.assertEqual(self.kind("main", "<dom::Cx>::link"), "DOM")
        self.assertEqual(self.kind("main", "<style::Walk>::cascade"), "style")
        self.assertEqual(self.kind("main", "<layout::text::InlineContent>::new"), "text")
        self.assertEqual(self.kind("main", "parley::shape::shape_text::<u32>"), "text")
        self.assertEqual(self.kind("main", "<layout::BoxTree>::lay_out_box"), "box layout")
        self.assertEqual(self.kind("main", "html::fast_path::scan"), "HTML parse")
        self.assertEqual(self.kind("main", "<elise::heap::young::Scavenging>::evacuate"), "GC")
        self.assertEqual(self.kind("main", "<renderer::frame_view::FrameView>::paint"), "other")
        # rusty_v8's shell of a callback, `dom_js`'s member call inlined in it.
        self.assertEqual(self.kind("Builtins_CallApiCallbackGeneric",
                                   '<unsafe extern "C" fn(_) -> _ as v8::support::CFnFrom<_>>::mapping::c_fn'),
                         "bindings")
        # A Rust frame that names one of rusty_v8's traits isn't V8's own.
        self.assertEqual(self.kind("<bindings::unified_heap::Marking as v8::machopo::EmbedderMarking>::advance"), "GC")

    def test_allocation_and_containers_go_to_their_caller(self):
        self.assertEqual(self.kind("main", "<dom::Cx>::class_names", "<alloc::vec::Vec<u8>>::push",
                                   "mi_malloc_aligned"), "DOM")
        self.assertEqual(self.kind("main", "html::fast_path::decode", "__memmove_avx512_unaligned_erms"),
                         "HTML parse")
        # `gc`'s allocation is GC's, as cppgc's is in Chrome; its containers
        # are their caller's.
        self.assertEqual(self.kind("main", "<dom::Cx>::node", "<elise::heap::Inner>::allocate"), "GC")
        self.assertEqual(self.kind("main", "<dom::Cx>::node", "<elise::vector::HeapVector<u32>>::push"), "DOM")

    def test_v8s_own_frames_look_outward(self):
        # JS outside them: JS (its builtins, its runtime).
        self.assertEqual(self.kind("JS:*run http://x/a.js:1:2", "Builtins_LoadIC_Megamorphic"), "JS")
        self.assertEqual(self.kind("RegExp.< src: 'a' flags: 'i'>"), "JS")
        # machopo's code (or V8's API) outside them: V8 API.
        self.assertEqual(self.kind("<dom_js::front::V8Front>::finish", "v8::internal::Factory::NewStringFromUtf8"),
                         "V8 API")
        # V8's GC is GC, whoever calls it.
        self.assertEqual(self.kind("JS:*run http://x/a.js:1:2", "v8::internal::Heap::CollectGarbage(v8::internal::"
                                   "AllocationSpace)", "v8::internal::Scavenger::Process(v8::JobDelegate*)"), "GC")
        # A type in the arguments isn't the name: not V8's GC.
        self.assertEqual(self.kind("JS:~f http://x/a.js:1:2", "v8::internal::Factory::NewJSObject(v8::internal::"
                                   "AllocationType, v8::internal::Heap::AllocationOrigin)"), "JS")

    def test_the_samplings_own_is_left_out(self):
        for stack in (["main", "<tracing_subscriber::registry::sharded::Registry>::get"],
                      ["main", "perfetto::internal::TrackEventInternal::WriteEvent(perfetto::TraceWriterBase*)"],
                      ["JS:*f http://x/a.js:1:2", "v8::internal::CodeEventLogger::CodeCreateEvent()",
                       "v8::base::OS::FPrint(_IO_FILE*, char const*, ...)"]):
            self.assertEqual(self.kind(*stack), categories.SAMPLING)
        folded = collections.Counter({"main;<dom::Cx>::link": 3, "main;perfetto::TraceWriterImpl::NewTracePacket()": 1})
        suite = categories.Suite(folded, 30.0)
        # The DOM's 3 of the 3 samples kept: the suite's whole 30 ms.
        self.assertEqual(suite.by_category["DOM"], 30.0)
        self.assertEqual(suite.sampling, 0.25)

    def test_blinks_frames_by_class(self):
        self.assertEqual(self.kind("blink::HTMLTokenizer::NextTokenImpl(blink::SegmentedString&)"), "HTML parse")
        self.assertEqual(self.kind("bool blink::ElementRuleCollector::CollectMatchingRulesInternal<false>"
                                   "(blink::MatchRequest const&)"), "style")
        self.assertEqual(self.kind("blink::Element::RecalcStyle(blink::StyleRecalcChange)"), "style")
        # Style before text: the font builder is style's.
        self.assertEqual(self.kind("blink::FontBuilder::CreateFont(blink::FontDescription&)"), "style")
        self.assertEqual(self.kind("blink::ShapeResultView::ComputeInkBounds() const"), "text")
        self.assertEqual(self.kind("OT::Layout::GPOS_impl::PairPosFormat1_3<OT::Layout::SmallTypes>::apply"
                                   "(OT::hb_ot_apply_context_t*) const"), "text")
        self.assertEqual(self.kind("blink::BlockLayoutAlgorithm::Layout()"), "box layout")
        self.assertEqual(self.kind("blink::Element::InsertedInto(blink::ContainerNode&)"), "DOM")
        self.assertEqual(self.kind("blink::V8Node::ParentNodeAttributeGetterCallback(v8::FunctionCallbackInfo"
                                   "<v8::Value> const&)"), "bindings")
        self.assertEqual(self.kind("cppgc::internal::PageBackend::TryAllocateNormalPageMemory()"), "GC")
        # WTF's strings and PartitionAlloc are their caller's.
        self.assertEqual(self.kind("blink::Element::ClassAttributeChanged(blink::AtomicString const&)",
                                   "blink::AtomicStringTable::Add(blink::StringImpl*)",
                                   "void* partition_alloc::PartitionRoot::Alloc<(partition_alloc::internal::"
                                   "AllocFlags)16>(unsigned long)"), "DOM")
        # Skia's frames: a canvas's under a canvas, else paint's.
        self.assertEqual(self.kind("blink::BaseRenderingContext2D::fill(blink::V8CanvasFillRule const&)",
                                   "SkCanvas::drawPath(SkPath const&, SkPaint const&)", "aaa_fill_path"), "canvas")
        self.assertEqual(self.kind("blink::BoxPainter::Paint(blink::PaintInfo const&)",
                                   "SkCanvas::drawRect(SkRect const&, SkPaint const&)"), "paint")

    def test_a_cxx_frames_name(self):
        self.assertEqual(categories.cxx_name("bool blink::ElementRuleCollector::CollectMatchingRulesInternal<false>"
                                             "(blink::MatchRequest const&)"),
                         "blink::ElementRuleCollector::CollectMatchingRulesInternal")
        self.assertEqual(categories.cxx_name("blink::Foo::Bar(int) const [clone .cfi]"), "blink::Foo::Bar")
        self.assertEqual(categories.cxx_name("blink::(anonymous namespace)::ComputeBlockSize(int)"),
                         "blink::::ComputeBlockSize")

    def test_the_comparison(self):
        def suite(time_ms, **samples):
            return categories.Suite(collections.Counter({f"main;{frame}": n for frame, n in samples.items()}),
                                    time_ms)
        machopo = {"A": suite(30, **{"<dom::Cx>::link": 2, "<style::Walk>::cascade": 1}),
               "B": suite(10, **{"<dom::Cx>::link": 1})}
        chrome = {"A": suite(10, **{"blink::Element::InsertedInto()": 1, "blink::Element::RecalcStyle()": 1}),
                  "B": suite(10, **{"blink::Element::InsertedInto()": 1}),
                  "C": suite(5, **{"blink::Element::InsertedInto()": 1})}
        # A: machopo's DOM 20, style 10; Chrome's 5 and 5. Catching up in the DOM
        # alone: A (30 - 15) / 10, B 10 / 10.
        self.assertAlmostEqual(categories.caught_up(machopo, {k: chrome[k] for k in machopo}, ("DOM",)), 1.5 ** 0.5)
        text = categories.report(machopo, chrome, {"A": 1.25, "B": 1.0}, ["# t"])
        self.assertIn("2 suites; the geometric mean of machopo / Chrome: 1.732.", text)
        self.assertIn("| DOM | 30.0 | 15.0 | +15.0 | 2.00 | 1.225 |", text)
        # The widest gaps of each, those machopo is behind in.
        self.assertIn("| A | 30.0 | 10.0 | 3.00 | DOM +15.0, style +5.0 |", text)
        self.assertIn("| B | 10.0 | 10.0 | 1.00 |  |", text)
        self.assertIn("A 1.25, B 1.00; geometric mean 1.118.", text)
        self.assertIn("- machopo: `<dom::Cx>::link` 30.0", text)

    def test_a_threads_samples_by_its_tid(self):
        # Chromium's renderer main thread has the binary's name on Linux.
        text = (
            "chromium-browse  10/10  5.000000007:      cycles:u: \n"
            "\t    7f01 blink::Node::parentNode()\n"
            "\n"
            "chromium-browse  20/20  5.000000008:      cycles:u: \n"
            "\t    7f02 browser\n"
        )
        self.assertEqual(speedometer.read_perf_script(text, speedometer.MAIN_THREAD, 0, tid=10),
                         ({"cycles": [(5_000_000_007, ["blink::Node::parentNode()"])]}, {10}))

    def test_the_marked_threads_ids(self):
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

        def thread(track, pid, tid, name):
            return message((1, message((10, track), (60, message((1, track), (4, message((1, pid), (2, tid),
                                                                                           (5, name))))))))

        def mark(track, time):
            return message((1, message((10, track), (8, time), (58, 6),
                                       (11, message((9, 3), (23, b"S.A-start"), (22, b"blink.user_timing"),
                                                    (11, track))))))

        # Two renderers' main threads (the blank page's and the suite's):
        # the one with the marks.
        trace = thread(5, 100, 100, b"CrRendererMain") + thread(7, 200, 200, b"CrRendererMain") + mark(7, 10) + mark(7, 20)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.pftrace"
            path.write_bytes(trace)
            self.assertEqual(common.perfetto_marked_thread(path), (200, 200))
            path.write_bytes(thread(5, 100, 100, b"CrRendererMain"))
            self.assertIsNone(common.perfetto_marked_thread(path))

    def test_the_report_waits_for_both_browsers(self):
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(speedometer, "RESULTS", Path(folder)), \
                mock.patch("sys.stdout", new=io.StringIO()) as out:
            self.assertIsNone(speedometer.write_categories_report())
            self.assertIn("machopo's samples (--sample), a Chromium's samples (--sample --browser chrome), machopo's and "
                          "Chrome for Testing's times", out.getvalue())
            sample = Path(folder) / "sample"
            sample.mkdir()
            (sample / "S-machopo.folded").write_text("main;<dom::Cx>::link 3\n", encoding="utf-8")
            (sample / "S-chrome.folded").write_text("blink::Element::InsertedInto() 1\n", encoding="utf-8")
            iteration = {"S": {"total": 20.0}}
            for side, total in (("machopo", 30.0), ("chrome", 10.0), ("chromium", 12.0)):
                runs = {"suites": {"S": {"iterations": [{"S": {"total": total}}] * 3}}}
                (Path(folder) / f"{side}-runs.json").write_text(json.dumps(runs), encoding="utf-8")
            del iteration
            report = speedometer.write_categories_report()
            text = report.read_text(encoding="utf-8")
        self.assertIn("| DOM | 30.0 | 10.0 | +20.0 | 3.00 |", text)
        self.assertIn("S 1.20", text)


class AgainstTreeTests(unittest.TestCase):
    """`--against REV`'s worktree is at the repo's `REV`."""

    def test_head_is_the_repos(self):
        calls = []

        def run(command, cwd=None, **kwargs):
            calls.append((command, cwd))
            if command[:2] == ["git", "rev-parse"]:
                return subprocess.CompletedProcess(command, 0, stdout="abc123\n")
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as folder:
            worktree = Path(folder)
            (worktree / ".git").write_text("gitdir: x\n", encoding="utf-8")
            with mock.patch.object(common, "AGAINST", worktree), mock.patch.object(common.subprocess, "run", run):
                self.assertEqual(common.against_tree("HEAD"), worktree)
        # Resolved in the repo, then checked out in the worktree by commit:
        # the worktree's own HEAD is where it was last.
        self.assertEqual(calls, [(["git", "rev-parse", "--verify", "-q", "HEAD^{commit}"], common.ROOT),
                                 (["git", "checkout", "-q", "--detach", "abc123"], worktree)])


class ChromeSampleTests(unittest.TestCase):
    """`bench.py speedometer --sample --browser chrome` on the fake `frames`
    suite: Fedora's Chromium (skipped if it isn't installed) under `perf`."""

    @classmethod
    def setUpClass(cls):
        if not sys.platform.startswith("linux") or shutil.which("perf") is None:
            raise unittest.SkipTest("no Linux perf")
        if not speedometer.CHROMIUM.exists():
            raise unittest.SkipTest(f"no Chromium at {speedometer.CHROMIUM}")

    def test_the_stacks_in_the_timed_windows(self):
        with tempfile.TemporaryDirectory() as folder:
            results = Path(folder)
            maps = set(Path("/tmp").glob("perf-*.map"))
            args = argparse.Namespace(iterations=1, browser="chrome", chrome_binary=None, against=None)
            runs = mock.Mock(wraps=speedometer.run_in_chrome)
            with mock.patch.object(speedometer, "RESULTS", results), mock.patch.object(speedometer, "SPEEDOMETER", FAKE), \
                    mock.patch.object(speedometer, "run_in_chrome", runs), \
                    open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
                self.assertEqual(speedometer.cmd_sample(args, [("frames", True)], None), 0)
            # The sampled run traces the runner's marks alone: the categories
            # traced by default slow Chromium to half its speed.
            sampled = [call for call in runs.call_args_list if call.kwargs.get("wrapper")]
            self.assertEqual([call.kwargs["categories"] for call in sampled], [(speedometer.USER_TIMING,)])
            folded = (results / "sample" / "frames-chrome.folded").read_text(encoding="utf-8").splitlines()
            stacks = [line.rsplit(" ", 1)[0].split(";") for line in folded]
            # Blink's frames by their names (the debuginfo), and V8's name for
            # the test's JS function.
            self.assertTrue(any(frame.startswith("blink::") for stack in stacks for frame in stack))
            self.assertTrue(any("makeGarbage" in frame for stack in stacks for frame in stack))
            # Its time not sampled, first.
            runs = json.loads((results / "chromium-runs.json").read_text(encoding="utf-8"))
            self.assertIn("frames", runs["suites"])
            # V8's names for the JS are gone with the run, and its log.
            self.assertEqual(set(Path("/tmp").glob("perf-*.map")) - maps, set())
            self.assertEqual(sorted(path.name for path in (results / "sample").iterdir()),
                             ["frames-chrome.folded", "frames-chrome.instructions.folded", "frames-chrome.perf.data",
                              "frames-chrome.pftrace"])


if __name__ == "__main__":
    unittest.main()
