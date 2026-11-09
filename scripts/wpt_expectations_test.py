"""Tests of how `test.py` reads WPT's expectations and results around a
wptrunner run: the known intermittent statuses an `.ini` lists (each file
with one has a comment), and the ones a run's raw log took; how many
tests run at once; the checks of a run in chunks, over every chunk's log.

    python3 scripts/wpt_expectations_test.py [-k NAME]
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

# `test` is the standard library's test package: loaded by its path.
_spec = importlib.util.spec_from_file_location("machopo_test_py", Path(__file__).resolve().parent / "test.py")
test_py = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = test_py
_spec.loader.exec_module(test_py)


class IntermittentResults(unittest.TestCase):
    def results(self, *entries: dict) -> list[str]:
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "raw.log"
            log.write_text("".join(json.dumps(entry) + "\n" for entry in entries), encoding="utf-8")
            return test_py.intermittent_results([log])

    def test_a_test_and_its_subtests_on_one_line(self):
        lines = self.results(
            {"action": "test_start", "test": "/a.html"},
            {"action": "test_status", "test": "/a.html", "subtest": "x", "status": "FAIL",
             "expected": "PASS", "known_intermittent": ["FAIL"]},
            {"action": "test_status", "test": "/a.html", "subtest": "y", "status": "FAIL",
             "expected": "PASS", "known_intermittent": ["FAIL"]},
            {"action": "test_end", "test": "/a.html", "status": "ERROR", "expected": "OK",
             "known_intermittent": ["ERROR"]},
        )
        self.assertEqual(lines, ["/a.html: ERROR (expected OK), 2 subtests"])

    def test_subtests_alone_and_one_subtest(self):
        lines = self.results(
            {"action": "test_status", "test": "/b.html", "subtest": "x", "status": "TIMEOUT",
             "expected": "PASS", "known_intermittent": ["FAIL", "TIMEOUT"]},
            {"action": "test_end", "test": "/b.html", "status": "OK"},
        )
        self.assertEqual(lines, ["/b.html: 1 subtest"])

    def test_expected_and_unexpected_results_arent_named(self):
        lines = self.results(
            # The first status listed: mozlog leaves `expected` out.
            {"action": "test_status", "test": "/c.html", "subtest": "x", "status": "PASS",
             "known_intermittent": ["FAIL"]},
            {"action": "test_end", "test": "/c.html", "status": "OK", "known_intermittent": ["ERROR"]},
            # Not in the list: unexpected, which wptrunner reports itself.
            {"action": "test_status", "test": "/d.html", "subtest": "x", "status": "FAIL",
             "expected": "PASS", "known_intermittent": ["TIMEOUT"]},
            {"action": "test_end", "test": "/d.html", "status": "ERROR", "expected": "OK"},
            {"action": "log", "level": "INFO", "message": "x"},
        )
        self.assertEqual(lines, [])

    def test_tests_in_order(self):
        end = {"action": "test_end", "status": "TIMEOUT", "expected": "OK", "known_intermittent": ["TIMEOUT"]}
        lines = self.results({**end, "test": "/z.html"}, {**end, "test": "/m.html"})
        self.assertEqual(lines, ["/m.html: TIMEOUT (expected OK)", "/z.html: TIMEOUT (expected OK)"])


class UncommentedIntermittents(unittest.TestCase):
    def bare(self, files: dict[str, str]) -> list[str]:
        with tempfile.TemporaryDirectory(dir=test_py.ROOT / "target") as tmp:
            metadata = Path(tmp) / "metadata"
            for name, text in files.items():
                (metadata / name).parent.mkdir(parents=True, exist_ok=True)
                (metadata / name).write_text(text, encoding="utf-8")
            root = test_py.TestRoot("/", Path(tmp) / "tests", metadata)
            prefix = (metadata.relative_to(test_py.ROOT).as_posix()) + "/"
            return [path.removeprefix(prefix) for path in test_py.uncommented_intermittents(root)]

    def test_a_list_needs_a_comment(self):
        bare = self.bare({
            "dir/a.html.ini": "[a.html]\n  expected: [OK, ERROR]\n",
            "dir/b.html.ini": "# Why.\n[b.html]\n  expected: [OK, ERROR]\n",
            "c.html.ini": "[c.html]\n  [x]\n    expected:\n      if os == \"win\": [PASS, FAIL]\n",
            "d.html.ini": "[d.html]\n  [x]\n    # Why, under the subtest.\n    expected: [PASS, FAIL]\n",
        })
        self.assertEqual(bare, ["c.html.ini", "dir/a.html.ini"])

    def test_one_status_and_bracketed_names_need_none(self):
        bare = self.bare({
            "e.html.ini": "[e.html]\n  [code points: [97,45\\]]\n    expected: FAIL\n",
            "f.html.ini": "[f.html]\n  expected:\n    if os == \"win\": FAIL\n",
            "__dir__.ini": "disabled: why\n",
        })
        self.assertEqual(bare, [])


class Chunks(unittest.TestCase):
    """A run of the include list in chunks: each chunk's log holds some of
    its tests, and only all of them together meet the list."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        tmp = Path(self.tmp.name)
        for path in ("a/x.html", "b/y.html", "b/z.any.js"):
            (tmp / "tests" / path).parent.mkdir(parents=True, exist_ok=True)
            (tmp / "tests" / path).write_text("", encoding="utf-8")
        # Under the repo, as the checks name an `.ini` by its path in it.
        self.metadata = Path(tempfile.mkdtemp(dir=test_py.ROOT / "target"))
        (self.metadata / "b").mkdir()
        (self.metadata / "b" / "y.html.ini").write_text("[y.html]\n  expected: FAIL\n", encoding="utf-8")
        self.root = test_py.TestRoot("/", tmp / "tests", self.metadata)
        self.include = ["a/", "b/y.html", "b/z.any.js  # a comment"]

    def tearDown(self):
        self.tmp.cleanup()
        test_py.shutil.rmtree(self.metadata)

    def log(self, name: str, *entries: dict) -> Path:
        path = Path(self.tmp.name) / name
        path.write_text("".join(json.dumps(entry) + "\n" for entry in entries), encoding="utf-8")
        return path

    def chunk(self, name: str, *tests: str, ended: bool = True) -> Path:
        entries = [{"action": "test_start", "test": test} for test in tests]
        return self.log(name, *entries, *([{"action": "suite_end"}] if ended else []))

    def test_the_chunks_together_meet_the_list(self):
        one = self.chunk("1.log", "/a/x.html")
        two = self.chunk("2.log", "/b/y.html", "/b/z.any.html", "/b/z.any.worker.html?v=1")
        self.assertEqual(test_py.unmatched_includes(self.include, [one, two], self.root), [])
        self.assertEqual(test_py.unrun_expectations(self.include, [one, two], self.root), [])

    def test_a_chunk_alone_leaves_the_others_lines(self):
        one = self.chunk("1.log", "/a/x.html")
        self.assertEqual(test_py.unmatched_includes(self.include, [one], self.root), ["b/y.html", "b/z.any.js"])
        unrun = test_py.unrun_expectations(self.include, [one], self.root)
        self.assertEqual([path.rsplit("/", 2)[-2:] for path in unrun], [["b", "y.html.ini"]])

    def test_a_cut_off_chunk_didnt_end(self):
        self.assertTrue(test_py.run_ended(self.chunk("1.log", "/a/x.html")))
        self.assertFalse(test_py.run_ended(self.chunk("2.log", "/b/y.html", ended=False)))

    def test_intermittent_results_of_every_chunk(self):
        flaky = {"status": "ERROR", "expected": "OK", "known_intermittent": ["ERROR"]}
        one = self.log("1.log", {"action": "test_end", "test": "/a/x.html", **flaky})
        two = self.log("2.log", {"action": "test_end", "test": "/b/y.html", **flaky})
        self.assertEqual(
            test_py.intermittent_results([one, two]),
            ["/a/x.html: ERROR (expected OK)", "/b/y.html: ERROR (expected OK)"],
        )

    def test_a_chunk_is_i_of_n(self):
        self.assertEqual(test_py.chunk_argument("2/4"), (2, 4))
        self.assertEqual(test_py.chunk_argument("4/4"), (4, 4))
        for bad in ("0/4", "5/4", "2", "a/b", "1/2/3"):
            with self.assertRaises(test_py.argparse.ArgumentTypeError, msg=bad):
                test_py.chunk_argument(bad)


class WptProcesses(unittest.TestCase):
    """How many tests run at a time for the GPU machopo draws with."""

    def test_a_gpu_keeps_wptrunners_default(self):
        gpu = {"device": "AMD Radeon", "type": "DiscreteGpu", "backend": "Dx12"}
        self.assertEqual(test_py.wpt_processes(gpu, 32), [])
        self.assertEqual(test_py.wpt_processes(None, 32), [])

    def test_the_cpu_adapter_takes_a_quarter_of_the_cpus(self):
        warp = {"device": "Microsoft Basic Render Driver", "type": "Cpu", "backend": "Dx12"}
        self.assertEqual(test_py.wpt_processes(warp, 32), ["--processes", "8"])
        self.assertEqual(test_py.wpt_processes(warp, 2), ["--processes", "1"])


if __name__ == "__main__":
    unittest.main()
