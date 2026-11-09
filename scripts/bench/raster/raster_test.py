#!/usr/bin/env python3
"""Tests of `bench.py raster` (`raster.py`): what it reads from a trace and
the compositor's log, its report, and a run of machopo (the dev profile,
headless) on a tall page of its own, scrolled and resized a few steps.

    python3 scripts/bench/raster/raster_test.py
"""

from __future__ import annotations

import argparse
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import raster  # noqa: E402
import speedometer  # noqa: E402


class ReadingTests(unittest.TestCase):
    """What the tool makes of a trace's slices and of the log."""

    def test_named_slices_with_their_spans(self):
        slices = [(0, "TileManager::PrepareTiles"), (1, "raster::playback"), (3, None), (4, "raster::playback"),
                  (6, None), (7, None), (8, "Other"), (9, None)]
        self.assertEqual(raster.spans(slices, {"TileManager::PrepareTiles", "raster::playback"}),
                         [("raster::playback", 1, 3), ("raster::playback", 4, 6), ("TileManager::PrepareTiles", 0, 7)])

    def test_a_phase_counts_what_began_in_it(self):
        found = [("A", 0, 2_000_000), ("A", 10_000_000, 13_500_000), ("A", 30_000_000, 31_000_000),
                 ("A", 40_000_000, 49_000_000), ("B", 10_000_000, 90_000_000)]
        # From 10 ms to 30 ms: the two As that begin in it (the last one at
        # its edge), not those before or after it, nor the B.
        self.assertEqual(raster.summary(found, "A", (10_000_000, 30_000_000)),
                         {"count": 2, "total_ms": 4.5, "max_ms": 3.5})
        self.assertEqual(raster.summary(found, "C", (0, 1)), {"count": 0, "total_ms": 0, "max_ms": 0})

    def test_the_logs_tiles_in_a_phase(self):
        log = "\n".join([
            "\x1b[2m2026-10-18T10:00:00.500000Z\x1b[0m \x1b[34mDEBUG\x1b[0m Compositor compositor::layer_tree_host_impl: "
            "prepared tiles rastered=180 solid=171 skipped=3 kept=2985",
            "2026-10-18T10:00:01.000000Z DEBUG Compositor compositor::layer_tree_host_impl: "
            "prepared tiles rastered=20 solid=5 skipped=40 kept=10",
            "2026-10-18T10:00:01.000000Z  INFO main browser: something else",
            "2026-10-18T10:00:03.000000Z DEBUG Compositor compositor::layer_tree_host_impl: "
            "prepared tiles rastered=7 solid=7 kept=0",
            # Before tiles were skipped, the log didn't say.
        ])
        start = 1792317600.0  # 2026-10-18T10:00:00Z
        self.assertEqual(raster.tiles_in_log(log, (start, start + 2)), {"rastered": 200, "solid": 176, "skipped": 43})
        self.assertEqual(raster.tiles_in_log(log, (start + 2.5, start + 4)),
                         {"rastered": 7, "solid": 7, "skipped": 0})

    def test_the_window_shrinks_and_grows_back(self):
        sizes = raster.resize_sizes()
        self.assertEqual(len(sizes), 2 * raster.RESIZE_STEPS)
        self.assertEqual((sizes[0], sizes[raster.RESIZE_STEPS - 1], sizes[-1]),
                         (raster.WINDOW, raster.SMALLEST, raster.WINDOW))

    @unittest.skipUnless(sys.platform == "darwin" or sys.platform.startswith("linux"), "macOS's or Linux's")
    def test_the_trace_clock_is_perfettos(self):
        clock = time.CLOCK_UPTIME_RAW if sys.platform == "darwin" else time.CLOCK_BOOTTIME
        before = time.clock_gettime_ns(clock)
        now = raster.trace_now()
        self.assertLessEqual(before, now)
        self.assertLess(now - before, 1_000_000_000)

    def test_the_report(self):
        results = {"page": {
            "scroll": {"prepare_tiles": {"count": 4, "total_ms": 10.0, "max_ms": 5.0},
                       "raster": {"count": 8, "total_ms": 8.0, "max_ms": 1.0},
                       "main_frames": {"count": 4, "total_ms": 2.0, "max_ms": 1.0},
                       "tiles": {"rastered": 8, "solid": 6, "skipped": 12}},
            "resize": {"prepare_tiles": {"count": 0, "total_ms": 0, "max_ms": 0},
                       "raster": {"count": 0, "total_ms": 0, "max_ms": 0},
                       "main_frames": {"count": 0, "total_ms": 0, "max_ms": 0},
                       "tiles": {"rastered": 0, "solid": 0, "skipped": 0}},
        }}
        env = {"os": "macOS", "cpu": "arm", "power": "AC"}
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(raster, "RESULTS", Path(folder)):
            text = raster.write_report(results, env, False, 2.0).read_text(encoding="utf-8")
            self.assertTrue((Path(folder) / "raster.json").exists())
            threaded = raster.write_report(results, env, False, 2.0, 3).read_text(encoding="utf-8")
        self.assertIn("headless, device scale factor 2, the default raster threads;", text)
        self.assertIn("headless, device scale factor 2, 3 raster threads;", threaded)
        self.assertIn("| page | scroll | 4 | 10.0 | 2.5, 5.0 | 2.0 | 8 | 6 (75%) | 12 |", text)
        self.assertIn("| page | resize | 0 | 0 | 0.0, 0 | 0 | 0 | 0 | 0 |", text)

    def test_an_unknown_page(self):
        args = argparse.Namespace(pages="nowhere", window=False, scale=2.0, raster_threads=None)
        with open(os.devnull, "w", encoding="utf-8") as quiet, mock.patch("sys.stdout", new=quiet):
            self.assertEqual(raster.cmd_raster(args), 2)


class RunTests(unittest.TestCase):
    """machopo (the dev profile, headless) on a tall, narrow page, scrolled
    and resized a few steps."""

    @classmethod
    def setUpClass(cls):
        cls.binary = speedometer.machopo_binary()

    def test_a_page_scrolled_and_resized(self):
        page = "<!DOCTYPE html><body style='margin:0'>" + "".join(
            f"<p style='width:300px;height:300px;background:#{i % 9}{i % 7}{i % 5}'>line {i}</p>" for i in range(40))
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(raster, "SCROLL_STEPS", 8), mock.patch.object(raster, "RESIZE_STEPS", 3):
            path = Path(folder) / "tall.html"
            path.write_text(page, encoding="utf-8")
            # Two raster threads: they raster the tiles, from machopo's flag.
            run = raster.run_page(self.binary, path.as_uri(), Path(folder), "tall", False, 2.0, 2)
            self.assertTrue(run["trace"].exists())
            results = raster.analyze(run)
        self.assertEqual(set(results), {"scroll", "resize"})
        for phase in ("scroll", "resize"):
            # Each phase rastered tiles on the compositor thread, which the
            # log counts too; the backgrounds are one color a tile or not.
            r = results[phase]
            self.assertGreater(r["prepare_tiles"]["count"], 0, phase)
            self.assertGreater(r["raster"]["count"], 0, phase)
            self.assertEqual(r["raster_threads"], 2, phase)
            self.assertGreater(r["tiles"]["rastered"], 0, phase)
            self.assertLessEqual(r["tiles"]["solid"], r["tiles"]["rastered"], phase)
            # Right of the paragraphs, nothing draws: those tiles aren't
            # rastered.
            self.assertGreater(r["tiles"]["skipped"], 0, phase)
        # Resizing lays the page out again on the main thread.
        self.assertGreater(results["resize"]["main_frames"]["count"], 0)

    def test_tiles_are_rastered_on_the_compositor_thread_unless_asked(self):
        page = "<!DOCTYPE html><body style='margin:0'>" + "".join(
            f"<p style='height:300px;background:#{i % 9}{i % 7}{i % 5}'>line {i}</p>" for i in range(20))
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(raster, "SCROLL_STEPS", 4), mock.patch.object(raster, "RESIZE_STEPS", 2):
            path = Path(folder) / "tall.html"
            path.write_text(page, encoding="utf-8")
            results = raster.analyze(raster.run_page(self.binary, path.as_uri(), Path(folder), "tall", False, 2.0))
        for phase in ("scroll", "resize"):
            self.assertGreater(results[phase]["raster"]["count"], 0, phase)
            self.assertEqual(results[phase]["raster_threads"], 1, phase)


if __name__ == "__main__":
    unittest.main()
