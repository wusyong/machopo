#!/usr/bin/env python3
"""Tests of `bench.py calls` (`calls.py`): what it reads from `perf stat`,
the counts per call, what each engine's test is told, the cases the guest
knows, and its report.

    python3 scripts/bench/calls/calls_test.py
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import calls  # noqa: E402


class CallsTests(unittest.TestCase):
    def test_perf_stats_counts(self):
        text = (
            "\n"
            "123456789,,instructions:u,1000,100.00,,\n"
            "45678,,cycles:u,1000,100.00,,\n"
            "1,,context-switches,1000,100.00,,\n"
        )
        self.assertEqual(calls.parse_perf_stat(text), {"instructions": 123456789.0, "cycles": 45678.0})

    def test_the_store_forwarding_event_is_counted_as_stlf(self):
        text = (
            "100,,instructions:u,1000,100.00,,\n"
            "50,,cycles:u,1000,100.00,,\n"
            "7,,ls_bad_status2.stli_other:u,1000,100.00,,\n"
        )
        self.assertEqual(calls.parse_perf_stat(text, "ls_bad_status2.stli_other"),
                         {"instructions": 100.0, "cycles": 50.0, "stlf": 7.0})
        # Without it asked for, another event's line is left out.
        self.assertEqual(calls.parse_perf_stat(text), {"instructions": 100.0, "cycles": 50.0})

    def test_the_store_forwarding_event_is_the_cpus(self):
        amd = "processor\t: 0\nvendor_id\t: AuthenticAMD\n"
        intel = "vendor_id\t: GenuineIntel\n"
        self.assertEqual(calls.stlf_event(amd, "  ls_bad_status2.stli_other\n"), "ls_bad_status2.stli_other")
        self.assertEqual(calls.stlf_event(intel, "  ld_blocks.store_forward\n"), "ld_blocks.store_forward")
        # Not in `perf list`, or a vendor with none: no event.
        self.assertIsNone(calls.stlf_event(amd, ""))
        self.assertIsNone(calls.stlf_event("vendor_id\t: Other\n", "anything"))

    def test_an_uncounted_event_stops(self):
        with self.assertRaises(SystemExit):
            calls.parse_perf_stat("<not counted>,,instructions:u,0,0.00,,\n")

    def test_the_empty_loop_is_taken_away_per_call(self):
        counts = {
            "empty": [{"instructions": 100.0, "cycles": 50.0}, {"instructions": 120.0, "cycles": 60.0},
                      {"instructions": 110.0, "cycles": 70.0}],
            "nodeType": [{"instructions": 2110.0, "cycles": 1060.0}, {"instructions": 2130.0, "cycles": 1000.0},
                         {"instructions": 9000.0, "cycles": 1080.0}],
        }
        # The medians (110, 60; 2130, 1060), less the empty loop's, over n.
        self.assertEqual(calls.per_call(counts, 10), {"nodeType": {"instructions": 202.0, "cycles": 100.0}})

    def test_conflicts_are_per_call_when_every_run_counted_them(self):
        run = lambda i, c, s: {"instructions": i, "cycles": c, "stlf": s}
        counts = {"empty": [run(100.0, 50.0, 3.0)] * 3, "nodeType": [run(300.0, 70.0, 23.0)] * 3}
        self.assertEqual(calls.per_call(counts, 10)["nodeType"], {"instructions": 20.0, "cycles": 2.0, "stlf": 2.0})
        counts["nodeType"][1] = {"instructions": 300.0, "cycles": 70.0}
        self.assertNotIn("stlf", calls.per_call(counts, 10)["nodeType"])

    def test_each_engine_is_told_the_loop(self):
        # The page's, on a document of the DOM's, in a block.
        js = calls.environment("js", "firstChild", 5)["MACHOPO_BENCH_SCRIPT"]
        self.assertTrue(js.startswith("{ " + calls.JS_SETUP))
        self.assertIn("for (let i = 0; i < 5; i++) { x = n.firstChild; } } 0", js)
        self.assertIn("const Event = machopoDom.Event;", calls.JS_SETUP)
        self.assertTrue(calls.JS_SETUP.endswith(calls.EVENTS_SETUP))
        dispatch = calls.environment("js", "dispatchEvent", 5)["MACHOPO_BENCH_SCRIPT"]
        self.assertIn("{ t.dispatchEvent(ev); }", dispatch)

    def test_the_report(self):
        results = {"js": {"nodeType": {"instructions": 209.4, "cycles": 53.2, "stlf": 2.04}},
                   "wasm": {"nodeType": {"instructions": 2184.0, "cycles": 649.0}}}
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(calls, "RESULTS", Path(folder)):
            report = calls.write_report(results, 1000, 3, "perf", [0]).read_text(encoding="utf-8")
            self.assertTrue((Path(folder) / "calls.json").exists())
        self.assertIn("| Member | js | wasm |", report)
        self.assertIn("| `nodeType` | 209 (53) [2.04] | 2184 (649) |", report)
        # A member measured for one engine only has the other's cell empty.
        self.assertIn("| `firstChild` |  |  |", report)


if __name__ == "__main__":
    unittest.main()
