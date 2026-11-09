"""Tests of `bench.py blink-perf` (`blink_perf.py` and `hook.js`), on
`scripts/bench/blink_perf/fake/`: a stand-in for Blink's `perf_tests/` (its
`runner.js` has the same calls the hook takes) and a few pages. Standard
library only. The tests that run pages use machopo's dev build and Chrome for
Testing (skipped if it isn't downloaded yet: `bench.py speedometer`
downloads it).

    python3 scripts/bench/blink_perf/blink_perf_test.py [-k NAME]
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
import urllib.request
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import blink_perf  # noqa: E402
import common  # noqa: E402

FAKE = Path(__file__).resolve().parent / "fake"


# A document's summary, as `summary.mjs` gives it.
SUMMARY = {"elements": {"p": 2}, "classes": {"a": 1}, "text": 5}


def ran(unit: str, *values: float, summary: dict = SUMMARY) -> dict:
    return {"unit": unit, "values": list(values), "log": [], "summary": summary}


def page_run(machopo: dict, chrome: dict, catches: bool = False) -> dict:
    return {"machopo": machopo, "Chrome": chrome, "catches": catches}


class PagesTests(unittest.TestCase):
    def test_a_folders_pages_leave_out_what_pages_load(self):
        self.assertEqual(
            blink_perf.pages(FAKE, ["dom"], []),
            ["dom/differs.html", "dom/rate.html", "dom/throws.html", "dom/time.html"],
        )

    def test_chosen_pages_follow_the_folders_once_each(self):
        self.assertEqual(
            blink_perf.pages(FAKE, ["other"], ["dom/time.html", "other/uncaught.html"]),
            ["other/catches.html", "other/internals.html", "other/no-summary.html", "other/self-reported.html",
             "other/uncaught.html", "dom/time.html"],
        )

    def test_a_folder_or_page_that_isnt_there_stops_it(self):
        with self.assertRaises(SystemExit):
            blink_perf.pages(FAKE, ["nothing"], [])
        with self.assertRaises(SystemExit):
            blink_perf.pages(FAKE, [], ["dom/nothing.html"])


    def test_the_list_is_a_page_a_line(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "pages.txt"
            path.write_text("# The pages.\ndom/time.html\n\ndom/rate.html  # checked\n", encoding="utf-8")
            self.assertEqual(blink_perf.listed(path), ["dom/time.html", "dom/rate.html"])
            self.assertEqual(blink_perf.listed(Path(folder) / "none.txt"), [])


class ScanTests(unittest.TestCase):
    def test_what_only_chromiums_harness_has_leaves_a_page_out(self):
        self.assertEqual(blink_perf.scan(FAKE, "other/internals.html"), {"excluded": "needs Chromium's `internals`"})
        self.assertEqual(
            blink_perf.scan(FAKE, "other/self-reported.html"),
            {"excluded": 'logs its own statistics without "Time:"'},
        )
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for code, why in (
                ("tracingCategories: 'blink'", "measured from Chrome's traces"),
                ("traceEventsToMeasure: ['x']", "measured from Chrome's traces"),
                ("testRunner.execCommand('x')", "needs Blink's `testRunner`"),
                ("if (window.GCController) {}", "needs Blink's `GCController`"),
                ("chrome.gpuBenchmarking.x()", "needs Chrome's `gpuBenchmarking`"),
                ("gc();", "calls `gc()`"),
                # What doesn't: the runner's `gc()`, its statistics with "Time:".
                ("PerfTestRunner.gc();", None),
                ("PerfTestRunner.logStatistics(v, 'ms', \"Time:\");", None),
            ):
                (root / "page.html").write_text(f"<script>{code}</script>", encoding="utf-8")
                self.assertEqual(blink_perf.scan(root, "page.html").get("excluded"), why, code)

    def test_the_scripts_a_page_loads_are_scanned_but_the_runners(self):
        self.assertEqual(blink_perf.scan(FAKE, "other/catches.html"), {"catches": True})
        # The runner catches what `run` throws: not the page.
        self.assertEqual(blink_perf.scan(FAKE, "dom/time.html"), {"catches": False})
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "lib.js").write_text("internals.x();", encoding="utf-8")
            (root / "page.html").write_text('<script src="./lib.js"></script>', encoding="utf-8")
            self.assertEqual(blink_perf.scan(root, "page.html"), {"excluded": "needs Chromium's `internals`"})
            (root / "page.html").write_text("<script>p.then(f).catch(g)</script>", encoding="utf-8")
            self.assertEqual(blink_perf.scan(root, "page.html"), {"catches": True})


class CpuTests(unittest.TestCase):
    """Both browsers on the fastest CPUs (`common.py`'s, Speedometer's too)."""

    def sysfs(self, frequencies: dict[int, str]) -> Path:
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        for cpu, frequency in frequencies.items():
            (Path(folder.name) / f"cpu{cpu}" / "cpufreq").mkdir(parents=True)
            (Path(folder.name) / f"cpu{cpu}" / "cpufreq" / "cpuinfo_max_freq").write_text(frequency + "\n")
        return Path(folder.name)

    def test_the_cpus_of_the_highest_maximum_frequency(self):
        # This laptop's: Zen 5 at 0-3 and 12-15 (5.16 GHz), Zen 5c the rest.
        sysfs = self.sysfs({cpu: "5157895" if cpu % 12 < 4 else "3289474" for cpu in range(24)})
        self.assertEqual(common.fastest_cpus(sysfs, list(range(24))), [0, 1, 2, 3, 12, 13, 14, 15])
        # Of those this process may use.
        self.assertEqual(common.fastest_cpus(sysfs, [4, 5, 13]), [13])
        # None when every one is as fast, or one's frequency isn't known.
        self.assertIsNone(common.fastest_cpus(sysfs, [4, 5, 6]))
        self.assertIsNone(common.fastest_cpus(self.sysfs({0: "5", 1: "x"}), [0, 1]))
        self.assertIsNone(common.fastest_cpus(sysfs, [0, 24]))

    def test_taskset_runs_both_browsers_on_them(self):
        self.assertEqual(common.cpu_list([0, 1, 2, 3, 12, 13, 14, 15]), "0-3,12-15")
        self.assertEqual(common.cpu_list([5, 1, 3, 2]), "1-3,5")
        started = []

        def popen(command, **options):
            started.append(command)
            raise OSError("not started")

        # The tool's own affinity isn't touched there.
        affinity = mock.Mock(side_effect=AssertionError("the affinity changed"))
        with mock.patch.object(common, "browser_cpus", lambda: [0, 1, 12]), \
                mock.patch.object(common.sys, "platform", "linux"), \
                mock.patch.object(common, "chrome_binary", lambda: Path("chrome")), \
                mock.patch.object(common, "process_affinity", affinity), \
                mock.patch.object(common.subprocess, "Popen", popen):
            self.assertEqual(common.on_browser_cpus(["machopo"]), ["taskset", "-c", "0-1,12", "machopo"])
            with self.assertRaises(OSError):
                common.MachopoPipe(Path("machopo"), None, wrapper=["perf", "record"])
            with self.assertRaises(OSError):
                common.ChromePipe(["--headless=new"])
        self.assertEqual(started[0][:6], ["taskset", "-c", "0-1,12", "perf", "record", "machopo"])
        self.assertEqual(started[1][:4], ["taskset", "-c", "0-1,12", "sh"])
        with mock.patch.object(common, "browser_cpus", lambda: None):
            self.assertEqual(common.on_browser_cpus(["machopo"]), ["machopo"])

    @staticmethod
    def cpu_sets(classes: dict[int, int], group: int = 0) -> bytes:
        """`GetSystemCpuSetInformation`'s entries: a CPU set (32 bytes) for
        each CPU and its efficiency class, after an entry of another type."""
        # Read as a CPU set, the other entry would be of group 0x0505.
        other = (32).to_bytes(4, "little") + (1).to_bytes(4, "little") + b"\x05" * 24
        return other + b"".join(
            common.CPU_SET.pack(32, common.CPU_SET_TYPE, 256 + cpu, group, cpu, cpu // 2, 0, 0, efficiency)
            + bytes(32 - common.CPU_SET.size)
            for cpu, efficiency in classes.items())

    def test_the_cpus_of_the_highest_efficiency_class(self):
        # This desktop's: an i9-13900K, its P-cores (class 1) at 0-15.
        info = self.cpu_sets({cpu: 1 if cpu < 16 else 0 for cpu in range(32)})
        self.assertEqual(common.efficiency_classes(info), {cpu: 1 if cpu < 16 else 0 for cpu in range(32)})
        self.assertEqual(common.fastest_windows_cpus(info, list(range(32))), list(range(16)))
        # Of those this process may use, and none when they're all alike.
        self.assertEqual(common.fastest_windows_cpus(info, [3, 20, 21]), [3])
        self.assertIsNone(common.fastest_windows_cpus(info, [16, 17]))
        self.assertIsNone(common.fastest_windows_cpus(self.cpu_sets({0: 0, 1: 0}), [0, 1]))
        # A CPU it doesn't know, and more than one group: not pinned.
        self.assertIsNone(common.fastest_windows_cpus(info, [0, 40]))
        self.assertIsNone(common.fastest_windows_cpus(info, [0, 16, 40]))
        self.assertIsNone(common.efficiency_classes(self.cpu_sets({0: 1}, group=1)))
        self.assertIsNone(common.fastest_windows_cpus(self.cpu_sets({0: 1, 1: 0}, group=1), [0, 1]))
        # Where the allowed CPUs come from when not given: the process's mask.
        with mock.patch.object(common, "process_affinity", lambda mask=None: 0b110000000000000000001):
            self.assertEqual(common.fastest_windows_cpus(info), [0])

    @unittest.skipUnless(sys.platform == "win32", "Windows's own calls")
    def test_this_machines_cpu_sets_and_affinity(self):
        classes = common.efficiency_classes(common.windows_cpu_sets())
        self.assertEqual(sorted(classes), list(range(os.cpu_count())))
        before = common.process_affinity()
        self.assertEqual(before, (1 << os.cpu_count()) - 1)
        try:
            self.assertEqual(common.process_affinity(0b1), before)
            self.assertEqual(common.process_affinity(), 0b1)
        finally:
            common.process_affinity(before)
        self.assertEqual(common.process_affinity(), before)

    def test_on_windows_both_browsers_start_with_the_tools_affinity_on_them(self):
        masks = [0xFFFFFFFF]
        started = []

        def process_affinity(mask=None):
            before = masks[-1]
            if mask is not None:
                masks.append(mask)
            return before

        def popen(command, **options):
            started.append((command, masks[-1]))
            return mock.Mock(stdin=io.BytesIO(), stdout=io.BytesIO())

        def failing(command, **options):
            started.append((command, masks[-1]))
            raise OSError("not started")

        with mock.patch.object(common, "browser_cpus", lambda: [0, 1, 12]), \
                mock.patch.object(common.sys, "platform", "win32"), \
                mock.patch.object(common, "process_affinity", process_affinity), \
                mock.patch.object(common.subprocess, "Popen", popen):
            self.assertEqual(common.on_browser_cpus(["machopo"]), ["machopo"])
            common.MachopoPipe(Path("machopo"), None, wrapper=["samply", "record"])
            self.assertEqual(masks[-1], 0xFFFFFFFF)
            with mock.patch.object(common.subprocess, "Popen", failing), self.assertRaises(OSError):
                common.MachopoPipe(Path("machopo"), None)
            self.assertEqual(masks[-1], 0xFFFFFFFF)
        self.assertEqual(started[0], (["samply", "record", "machopo", "--automation-pipe"], 0b1000000000011))
        self.assertEqual(started[1], (["machopo", "--automation-pipe"], 0b1000000000011))
        self.assertEqual(masks, [0xFFFFFFFF, 0b1000000000011, 0xFFFFFFFF, 0b1000000000011, 0xFFFFFFFF])
        if sys.platform == "win32":
            # Chrome's pipe takes handles, which need the real platform.
            masks[:] = [0xFFFFFFFF]
            with mock.patch.object(common, "browser_cpus", lambda: [0, 1, 12]), \
                    mock.patch.object(common, "process_affinity", process_affinity), \
                    mock.patch.object(common, "chrome_binary", lambda: Path("chrome")), \
                    mock.patch.object(common.subprocess, "Popen", failing), self.assertRaises(OSError):
                common.ChromePipe(["--headless=new"])
            self.assertEqual(started[2][1], 0b1000000000011)
            self.assertEqual(masks[-1], 0xFFFFFFFF)
        # Not pinned: the affinity isn't touched.
        masks[:] = [0xFFFFFFFF]
        with mock.patch.object(common, "browser_cpus", lambda: None), \
                mock.patch.object(common.sys, "platform", "win32"), \
                mock.patch.object(common, "process_affinity", process_affinity), \
                mock.patch.object(common.subprocess, "Popen", popen):
            common.MachopoPipe(Path("machopo"), None)
        self.assertEqual(masks, [0xFFFFFFFF])

    def test_on_windows_they_are_found_by_efficiency_class(self):
        with mock.patch.object(common.sys, "platform", "win32"), \
                mock.patch.object(common, "_browser_cpus", []), \
                mock.patch.object(common, "fastest_windows_cpus", lambda: [0, 1]), \
                mock.patch.object(common.shutil, "which", lambda name: None):
            self.assertEqual(common.browser_cpus(), [0, 1])
        with mock.patch.object(common.sys, "platform", "linux"), \
                mock.patch.object(common, "_browser_cpus", []), \
                mock.patch.object(common, "fastest_windows_cpus", lambda: [0, 1]), \
                mock.patch.object(common.shutil, "which", lambda name: None):
            self.assertIsNone(common.browser_cpus())


class ResultTests(unittest.TestCase):
    def test_what_a_browser_gave(self):
        posted = {"body": json.dumps({"unit": "runs/s", "values": [3, 4], "log": ["4 runs/s"], "summary": SUMMARY}).encode()}
        self.assertEqual(
            blink_perf.result(posted), {"unit": "runs/s", "values": [3, 4], "log": ["4 runs/s"], "summary": SUMMARY}
        )
        posted = {"body": json.dumps({"unit": "ms", "values": [3], "log": [], "summary_error": "no module"}).encode()}
        self.assertEqual(blink_perf.result(posted)["summary_error"], "no module")
        # No report, an error the page posted, nothing measured.
        self.assertEqual(blink_perf.result({"error": "no report in 3 s"}), {"error": "no report in 3 s"})
        posted = {"body": json.dumps({"error": "fatal: x", "log": ["FATAL: x"]}).encode()}
        self.assertEqual(blink_perf.result(posted), {"error": "fatal: x", "log": ["FATAL: x"]})
        posted = {"body": json.dumps({"unit": "ms", "values": [], "log": ["Time:", "values  ms", ""]}).encode()}
        self.assertEqual(blink_perf.result(posted)["error"], "the runner gave no values (its log: values  ms)")

    def test_how_many_times_slower_machopo_is(self):
        # Less is better in ms, more in runs/s.
        self.assertEqual(blink_perf.slowdown(ran("ms", 30, 10, 20), ran("ms", 5, 5, 4)), 4)
        self.assertEqual(blink_perf.slowdown(ran("runs/s", 10, 10), ran("runs/s", 40, 40)), 4)
        self.assertEqual(blink_perf.slowdown(ran("MB/s", 20), ran("MB/s", 10)), 0.5)
        # No direction, units that differ, or one browser without values.
        self.assertIsNone(blink_perf.slowdown(ran("percent", 20), ran("percent", 10)))
        self.assertIsNone(blink_perf.slowdown(ran("ms", 20), ran("runs/s", 10)))
        self.assertIsNone(blink_perf.slowdown({"error": "x"}, ran("ms", 10)))


class ValidTests(unittest.TestCase):
    def test_why_a_page_isnt_valid(self):
        self.assertEqual(blink_perf.problems(page_run(ran("ms", 2), ran("ms", 1))), [])
        self.assertEqual(
            blink_perf.problems(page_run({"error": "fatal: x\nat run"}, {"error": "no report in 3 s"})),
            ["machopo: fatal: x", "Chrome: no report in 3 s"],
        )
        self.assertEqual(
            blink_perf.problems(page_run(ran("ms", 2), ran("runs/s", 1))), ["machopo in ms, Chrome in runs/s"]
        )
        machopo = {"unit": "ms", "values": [2], "log": [], "summary_error": "no module"}
        self.assertEqual(blink_perf.problems(page_run(machopo, ran("ms", 1))), ["machopo: no summary (no module)"])
        machopo = ran("ms", 2, summary=dict(SUMMARY, elements={"p": 1}))
        self.assertEqual(
            blink_perf.problems(page_run(machopo, ran("ms", 1))), ["the documents differ: 1 `p` elements, Chrome 2"]
        )

    def test_what_to_check_on_a_valid_page(self):
        self.assertEqual(blink_perf.notes(page_run(ran("ms", 2), ran("ms", 1))), [])
        # machopo over 2 times faster; just 2 isn't.
        self.assertEqual(
            blink_perf.notes(page_run(ran("ms", 1), ran("ms", 2.1))), ["suspect: machopo over 2 times faster"]
        )
        self.assertEqual(blink_perf.notes(page_run(ran("ms", 1), ran("ms", 2))), [])
        self.assertEqual(
            blink_perf.notes(page_run(ran("ms", 2), ran("ms", 1), catches=True)), ["catches exceptions itself"]
        )


class ReportTests(unittest.TestCase):
    def test_folders_their_geometric_means_and_the_pages_not_valid(self):
        runs = {
            "dom/a.html": page_run(ran("ms", 8), ran("ms", 2)),
            "dom/b.html": page_run(ran("runs/s", 10), ran("runs/s", 10), catches=True),
            "css/c.html": page_run(ran("ms", 3), ran("ms", 1)),
            # Suspect: in its folder's table, not in the means.
            "css/fast.html": page_run(ran("ms", 1), ran("ms", 10)),
            "base64/d.html": page_run({"error": "fatal: atob is not defined\nat run"}, ran("runs/s", 5)),
            "dom/e.html": page_run(ran("ms", 3, summary=dict(SUMMARY, text=4)), ran("ms", 1)),
            "dom/f.html": {"excluded": "needs Chromium's `internals`"},
        }
        env = dict(common.environment("perf"), date="2026-10-20", blink_perf="8c07befe7a00")
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(blink_perf, "RESULTS", Path(folder)):
            lines = blink_perf.write_report(runs, env, {"dom/a.html", "dom/e.html", "dom/f.html"})
            lines = lines.read_text(encoding="utf-8").splitlines()
        self.assertIn("The valid pages: machopo slower 2.29 (geometric mean of 3); 4 valid of 7.", lines)
        self.assertIn("| `dom/a.html` | ms | 8 | 2 | 4 | |", lines)
        self.assertIn("| `dom/b.html` | runs/s | 10 | 10 | 1 | catches exceptions itself |", lines)
        self.assertIn("| geometric mean of 2 | | | | 2 | |", lines)
        self.assertIn("| `css/fast.html` | ms | 1 | 10 | 0.1 | suspect: machopo over 2 times faster |", lines)
        self.assertIn("| geometric mean of 1 | | | | 3 | |", lines)
        # The listed pages not valid first, then the others, with the first
        # line of why; the pages left out last. A folder with no valid page
        # has no table.
        listed = lines.index("## Listed, not valid")
        self.assertEqual(
            lines[listed + 2 : listed + 4],
            ["- `dom/e.html`: the documents differ: text 4 characters long, Chrome 5",
             "- `dom/f.html`: needs Chromium's `internals`"],
        )
        self.assertLess(listed, lines.index("## css"))
        invalid = lines.index("## Not valid")
        self.assertEqual(lines[invalid + 2], "- `base64/d.html`: machopo: fatal: atob is not defined")
        self.assertIn("- `dom/e.html`: the documents differ: text 4 characters long, Chrome 5", lines[invalid:])
        self.assertNotIn("at run", lines)
        self.assertEqual(lines[lines.index("## Left out") + 2], "- `dom/f.html`: needs Chromium's `internals`")
        self.assertNotIn("## base64", lines)


def gc_run(machopo: dict, chrome: dict, wasm: dict, **rest) -> dict:
    return dict({"machopo": machopo, "Chrome": chrome, "wasm": wasm, "catches": False}, **rest)


# The GC in a window, as `gc_bench.window_gc` gives it.
WINDOW_GC = {"window_ns": 2_000_000_000, "gc_ns": 50_000_000, "gc_by_kind_ns": {"marking": 30_000_000,
             "atomic": 20_000_000}, "pauses_ns": [1_000_000, 4_000_000, 45_000_000], "background_gc_ns": 7_000_000}


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.server = blink_perf.Server(FAKE)
        self.addCleanup(self.server.close)

    def get(self, path: str) -> tuple[bytes, str]:
        with urllib.request.urlopen(self.server.url(path)) as response:
            return response.read(), response.headers["Content-Type"]

    def test_the_runner_gets_the_summary_and_the_hook_after_it(self):
        body, kind = self.get("/resources/runner.js")
        runner = (FAKE / "resources" / "runner.js").read_bytes()
        self.assertTrue(body.startswith(runner))
        self.assertTrue(body.endswith((blink_perf.SCRIPTS / "hook.js").read_bytes()))
        added = body[len(runner) :].decode()
        self.assertIn("var __machopoSummarize = (function () {", added)
        self.assertIn("\nfunction summarize(doc) {", added)
        self.assertNotIn("export", added)
        self.assertTrue(kind.startswith("text/javascript"))
        # Other files as they are, with their types.
        self.assertEqual(self.get("/dom/time.html")[0], (FAKE / "dom" / "time.html").read_bytes())
        self.assertEqual(self.get("/other/empty.wasm"), ((FAKE / "other" / "empty.wasm").read_bytes(), "application/wasm"))
        # The summary the hook loads.
        body, kind = self.get(common.SUMMARY)
        self.assertIn(b"export function summarize(doc)", body)
        self.assertTrue(kind.startswith("text/javascript"))

class CommandTests(unittest.TestCase):
    """`bench.py blink-perf` without running pages: which it runs, and the
    list given to the report."""

    def run_command(self, list_text: str | None, dirs: str | None = None, pages: str | None = None,
                    areas: list[str] = ()):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        path = Path(folder.name) / "pages.txt"
        if list_text is not None:
            path.write_text(list_text, encoding="utf-8")
        ran, reported = [], []
        args = mock.Mock(dirs=dirs, pages=pages, timeout=5, areas=list(areas), against=None, gc=False)
        with mock.patch.object(blink_perf, "BLINK_PERF", FAKE), mock.patch.object(blink_perf, "PAGES", path), \
                mock.patch.object(blink_perf, "RESULTS", Path(folder.name)), \
                mock.patch.object(blink_perf, "run_pages", lambda names, *rest: ran.extend(names) or {}), \
                mock.patch.object(blink_perf, "write_report", lambda runs, env, on_list: reported.append(on_list)), \
                contextlib.redirect_stdout(io.StringIO()):
            code = blink_perf.cmd_blink_perf(args)
        return code, ran, reported

    def test_the_list_by_default(self):
        code, ran, reported = self.run_command("dom/time.html\n# a comment\nother/catches.html\n")
        self.assertEqual((code, ran), (0, ["dom/time.html", "other/catches.html"]))
        self.assertEqual(reported, [{"dom/time.html", "other/catches.html"}])

    def test_the_list_under_some_paths(self):
        text = "dom/time.html\ndom/rate.html\nother/catches.html\n"
        code, ran, reported = self.run_command(text, areas=["dom/"])
        self.assertEqual((code, ran), (0, ["dom/time.html", "dom/rate.html"]))
        self.assertEqual(reported, [{"dom/time.html", "dom/rate.html", "other/catches.html"}])
        # A page, a folder without its slash; not a folder's name's start.
        self.assertEqual(self.run_command(text, areas=["other", "dom/rate.html"])[1], ["dom/rate.html", "other/catches.html"])
        self.assertEqual(self.run_command(text, areas=["do"])[:2], (1, []))

    def test_folders_or_pages_instead_with_the_list_for_regressions(self):
        code, ran, reported = self.run_command("dom/time.html\n", dirs="other", pages="dom/rate.html")
        self.assertEqual(ran, ["other/catches.html", "other/internals.html", "other/no-summary.html",
                               "other/self-reported.html", "other/uncaught.html", "dom/rate.html"])
        self.assertEqual(reported, [{"dom/time.html"}])

    def test_against_another_version_the_same_pages(self):
        compared = []
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        path = Path(folder.name) / "pages.txt"
        path.write_text("dom/time.html\nother/catches.html\n", encoding="utf-8")
        args = mock.Mock(dirs=None, pages=None, timeout=5, areas=["dom/"], against="HEAD~1", runs=2, gc=False)
        with mock.patch.object(blink_perf, "BLINK_PERF", FAKE), mock.patch.object(blink_perf, "PAGES", path), \
                mock.patch.object(blink_perf, "cmd_against", lambda a, names, root: compared.append(names) or 0), \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(blink_perf.cmd_blink_perf(args), 0)
        self.assertEqual(compared, [["dom/time.html"]])

    def test_no_list_no_pages(self):
        self.assertEqual(self.run_command("# nothing yet\n")[:2], (1, []))
        self.assertEqual(self.run_command(None)[:2], (1, []))


def timed(unit: str, *values: float, summary: dict = SUMMARY) -> dict:
    return {"unit": unit, "values": list(values), "log": [], "summary": summary}


class AgainstTests(unittest.TestCase):
    """`--against REV`: machopo at another commit and the working tree's."""

    def test_why_a_page_isnt_compared(self):
        same = {"against": [timed("ms", 2), timed("ms", 3)], "current": [timed("ms", 1)]}
        self.assertIsNone(blink_perf.version_problem(same))
        self.assertEqual(blink_perf.version_problem({"against": [timed("ms", 2)], "current": []}), "current: not run")
        self.assertEqual(
            blink_perf.version_problem({"against": [{"error": "fatal: x\nat run"}], "current": [timed("ms", 1)]}),
            "against: fatal: x",
        )
        self.assertEqual(
            blink_perf.version_problem({"against": [timed("ms", 2)], "current": [timed("runs/s", 1)]}),
            "current: in runs/s, against in ms",
        )
        # The current version doing less than the other: against's first run is the reference.
        less = timed("ms", 1, summary=dict(SUMMARY, text=2))
        self.assertEqual(
            blink_perf.version_problem({"against": [timed("ms", 2)], "current": [timed("ms", 1), less]}),
            "current: the documents differ: text 2 characters long, against 5",
        )
        self.assertEqual(
            blink_perf.version_problem({"against": [timed("ms", 2), less], "current": [timed("ms", 1)]}),
            "against: the documents differ: text 2 characters long, against 5",
        )
        no_summary = {"unit": "ms", "values": [1], "log": []}
        self.assertEqual(
            blink_perf.version_problem({"against": [timed("ms", 2)], "current": [no_summary]}), "current: no summary"
        )

    def test_the_comparison(self):
        by_page = {
            "dom/a.html": {"against": [timed("ms", *[10] * 8)], "current": [timed("ms", *[20] * 8)]},
            "dom/b.html": {"against": [timed("runs/s", *[10] * 4), timed("runs/s", *[30] * 4)],
                           "current": [timed("runs/s", *[40] * 4), timed("runs/s", *[40] * 4)]},
            "dom/c.html": {"against": [timed("ms", 1)], "current": [{"error": "no report in 3 s"}]},
            "dom/d.html": {"excluded": "needs Chromium's `internals`"},
            # Too quick for the timer in one version: no ratio.
            "dom/e.html": {"against": [timed("ms", 0, 0, 1)], "current": [timed("ms", 1)]},
        }
        env = dict(common.environment("perf"), date="2026-10-20")
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(blink_perf, "RESULTS", Path(folder)), \
                mock.patch.object(common, "versions_text", return_value="`against` is HEAD~1"):
            lines = blink_perf.write_compare_report("HEAD~1", 2, by_page, env).read_text(encoding="utf-8").splitlines()
        self.assertIn("| `dom/a.html` | ms | 10.0 (10.0–10.0) | 20.0 (20.0–20.0) | +100.0% |", lines)
        self.assertIn("| `dom/b.html` | runs/s | 20.0 (10.0–30.0) | 40.0 (40.0–40.0) | +100.0% |", lines)
        # Times: a's doubled, b's halved (twice the runs a second).
        self.assertIn("The geometric mean of the 2 pages' times, current over against: 1.000 (+0.0%).", lines)
        self.assertEqual(lines[lines.index("## Not compared") + 2], "- `dom/c.html`: current: no report in 3 s")
        self.assertEqual(lines[lines.index("## Not compared") + 3], "- `dom/e.html`: against: a median of 0 ms")
        self.assertEqual(lines[lines.index("## Left out") + 2], "- `dom/d.html`: needs Chromium's `internals`")
        self.assertIn("| `dom/b.html` | 10, 30 | 40, 40 |", lines)

    def run_against(self, problems: list[str]) -> tuple[int, list, mock.Mock]:
        order = []

        def time_pages(binary, names, root, timeout):
            order.append(binary.name)
            return {"dom/time.html": timed("ms", 1, 2, 3), "other/internals.html": {"excluded": "x"}}

        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        args = mock.Mock(against="HEAD~1", runs=2, timeout=5)
        build = mock.Mock(return_value={"against": Path("against"), "current": Path("current")})
        with mock.patch.object(blink_perf, "RESULTS", Path(folder.name)), \
                mock.patch.object(common, "against_problems", return_value=problems), \
                mock.patch.object(common, "against_binaries", build), \
                mock.patch.object(common, "versions_text", return_value="`against` is HEAD~1"), \
                mock.patch.object(blink_perf, "time_pages", side_effect=time_pages), \
                contextlib.redirect_stdout(io.StringIO()):
            code = blink_perf.cmd_against(args, ["dom/time.html", "other/internals.html"], FAKE)
        self.compared = json.loads((Path(folder.name) / "compare.json").read_text()) if code == 0 else None
        return code, order, build

    def test_the_versions_take_turns_the_other_first(self):
        code, order, _ = self.run_against([])
        self.assertEqual((code, order), (0, ["against", "current", "against", "current"]))
        self.assertEqual(len(self.compared["pages"]["dom/time.html"]["current"]), 2)
        self.assertEqual(self.compared["pages"]["other/internals.html"], {"excluded": "x"})

    def test_a_version_pinning_otherwise_isnt_built(self):
        code, order, build = self.run_against(["HEAD~1 pins ../third_party otherwise"])
        self.assertEqual((code, order), (1, []))
        build.assert_not_called()


class RunTests(unittest.TestCase):
    """The fake pages in machopo (the dev build) and Chrome for Testing."""

    @classmethod
    def setUpClass(cls):
        if not common.chrome_path().exists():
            raise unittest.SkipTest("no Chrome for Testing yet: `bench.py speedometer` downloads it")
        results = tempfile.TemporaryDirectory()
        patch = mock.patch.object(blink_perf, "RESULTS", Path(results.name))
        patch.start()
        names = blink_perf.pages(FAKE, ["dom", "other"], [])
        cls.pages = names
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            cls.runs = blink_perf.run_pages(names, FAKE, "dev", 30)
        patch.stop()
        results.cleanup()

    def test_each_browser_gives_the_runners_values(self):
        for page, unit in (("dom/time.html", "ms"), ("dom/rate.html", "runs/s")):
            for browser in ("machopo", "Chrome"):
                got = self.runs[page][browser]
                self.assertEqual((got.get("unit"), len(got.get("values", []))), (unit, 4), (page, browser, got))
            self.assertIsNotNone(blink_perf.slowdown(self.runs[page]["machopo"], self.runs[page]["Chrome"]))
            # Both did the same: valid.
            self.assertEqual(blink_perf.problems(self.runs[page]), [], page)
            self.assertEqual(self.runs[page]["machopo"]["summary"]["elements"].get("pre"), 1, page)

    def test_a_page_that_does_more_in_chrome_isnt_valid(self):
        self.assertEqual(
            blink_perf.problems(self.runs["dom/differs.html"]), ["the documents differ: 0 `aside` elements, Chrome 1"]
        )

    def test_a_page_that_catches_is_valid_and_marked(self):
        run = self.runs["other/catches.html"]
        self.assertEqual(blink_perf.problems(run), [])
        self.assertIn("catches exceptions itself", blink_perf.notes(run))

    def test_a_summary_that_fails_leaves_the_values(self):
        run = self.runs["other/no-summary.html"]
        for browser in ("machopo", "Chrome"):
            self.assertEqual(len(run[browser].get("values", [])), 4, (browser, run[browser]))
        self.assertEqual(
            blink_perf.problems(run),
            ["machopo: no summary (Error: can't walk)", "Chrome: no summary (Error: can't walk)"],
        )

    def test_two_versions_of_machopo(self):
        # The dev build as both: its pages' values, a fatal error, a page left out.
        binary = common.machopo_binary("dev")
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        with mock.patch.object(blink_perf, "RESULTS", Path(folder.name)), contextlib.redirect_stdout(io.StringIO()):
            by_page = blink_perf.compare_versions({"against": binary, "current": binary},
                                                  ["dom/time.html", "dom/throws.html", "other/internals.html"],
                                                  FAKE, 1, 30)
        self.assertIsNone(blink_perf.version_problem(by_page["dom/time.html"]))
        self.assertEqual(len(by_page["dom/time.html"]["current"][0]["values"]), 4)
        self.assertTrue(blink_perf.version_problem(by_page["dom/throws.html"]).startswith("against: fatal: "))
        self.assertEqual(by_page["other/internals.html"], {"excluded": "needs Chromium's `internals`"})

    def test_a_page_left_out_isnt_run(self):
        self.assertEqual(self.runs["other/internals.html"], {"excluded": "needs Chromium's `internals`"})
        self.assertEqual(self.runs["other/self-reported.html"], {"excluded": 'logs its own statistics without "Time:"'})

    def test_a_fatal_error_is_what_a_browser_gave(self):
        for browser in ("machopo", "Chrome"):
            got = self.runs["dom/throws.html"][browser]
            self.assertTrue(got.get("error", "").startswith("fatal: Got an exception while running test.run"), got)
            self.assertNotIn("values", got)
            # With what the runner logged.
            self.assertTrue(any(line.startswith("FATAL: ") for line in got["log"]), got)

    def test_an_uncaught_exception_is_what_a_browser_gave(self):
        for browser in ("machopo", "Chrome"):
            got = self.runs["other/uncaught.html"][browser]
            self.assertIn("set up failed", got.get("error", ""), (browser, got))


if __name__ == "__main__":
    unittest.main()
