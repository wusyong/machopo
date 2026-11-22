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
            "prepared tiles rastered=20 solid=5 skipped=40 kept=10 gpu=12 gpu_but_images=5 images=6 masks=1 "
            "filters=0 blurs=2 patterns=0 bitmap_glyphs=1",
            "2026-10-18T10:00:01.000000Z  INFO main browser: something else",
            "2026-10-18T10:00:03.000000Z DEBUG Compositor compositor::layer_tree_host_impl: "
            "prepared tiles rastered=7 solid=7 kept=0",
            # Before tiles were skipped, the log didn't say.
        ])
        start = 1792317600.0  # 2026-10-18T10:00:00Z
        none = dict.fromkeys(raster.COUNTS, 0)
        # What the log didn't say yet is 0.
        self.assertEqual(raster.tiles_in_log(log, (start, start + 2)),
                         {**none, "rastered": 200, "solid": 176, "skipped": 43, "gpu": 12, "gpu_but_images": 5,
                          "images": 6, "masks": 1, "blurs": 2, "bitmap_glyphs": 1})
        self.assertEqual(raster.tiles_in_log(log, (start + 2.5, start + 4)), {**none, "rastered": 7, "solid": 7})

    def test_the_logs_frames_on_their_way_to_the_screen(self):
        at = "2026-10-18T10:00:0"
        log = "\n".join([
            # The browser's UI's own, on its thread and its sink: not the tab's.
            f"{at}0.000000Z DEBUG main compositor::layer_tree_host_impl: frame submitted frame_token=1",
            f"{at}0.000100Z DEBUG VizCompositorThread viz::display: viz: frame arrived client_id=0 sink_id=1 "
            "frame_token=1",
            f"\x1b[2m{at}1.000000Z\x1b[0m DEBUG Compositor compositor::layer_tree_host_impl: frame submitted "
            "frame_token=1",
            f"{at}1.000500Z DEBUG VizCompositorThread viz::display: viz: frame arrived client_id=2 sink_id=1 "
            "frame_token=1",
            f"{at}1.001000Z DEBUG Compositor compositor::layer_tree_host_impl: frame submitted frame_token=2",
            f"{at}1.001200Z DEBUG VizCompositorThread viz::display: viz: frame arrived client_id=2 sink_id=1 "
            "frame_token=2",
            # One draw, of the browser's frame and the tab's second: the
            # first was never on the screen itself.
            f"{at}1.003000Z DEBUG VizCompositorThread viz::display: viz: frame drawn client_id=0 sink_id=1 "
            "frame_token=1",
            f"{at}1.003000Z DEBUG VizCompositorThread viz::display: viz: frame drawn client_id=2 sink_id=1 "
            "frame_token=2",
            # The same frame drawn again (the browser's own changed).
            f"{at}1.009000Z DEBUG VizCompositorThread viz::display: viz: frame drawn client_id=2 sink_id=1 "
            "frame_token=2",
            f"{at}2.000000Z DEBUG Compositor compositor::layer_tree_host_impl: frame submitted frame_token=3",
            f"{at}2.010000Z DEBUG VizCompositorThread viz::display: viz: frame drawn client_id=2 sink_id=1 "
            "frame_token=3",
            f"{at}3.000000Z  INFO main browser: a frame of something else",
        ])
        start = 1792317600.0  # 2026-10-18T10:00:00Z
        frames = raster.frames_in_log(log)
        self.assertEqual([token for _, token in frames["submitted"]], [1, 2, 3])
        self.assertEqual([token for _, token in frames["arrived"]], [1, 2])
        self.assertEqual([token for _, token in frames["drawn"]], [2, 2, 3])
        # The frames submitted in the second from 1 s: both took until the
        # draw at 1.003 (3 ms and 2 ms), and 0.5 ms and 0.2 ms to Viz.
        took = raster.frame_latency(frames, (start + 0.5, start + 1.5))
        self.assertEqual(took["to_screen"], {"count": 2, "median_ms": 3.0, "p95_ms": 3.0, "max_ms": 3.0})
        self.assertEqual(took["to_viz"], {"count": 2, "median_ms": 0.5, "p95_ms": 0.5, "max_ms": 0.5})
        # The third: 10 ms; it never said it arrived.
        took = raster.frame_latency(frames, (start + 1.5, start + 2.5))
        self.assertEqual(took["to_screen"], {"count": 1, "median_ms": 10.0, "p95_ms": 10.0, "max_ms": 10.0})
        self.assertEqual(took["to_viz"]["count"], 0)
        # A frame no draw followed counts for nothing.
        del frames["drawn"][-1]
        self.assertEqual(raster.frame_latency(frames, (start + 1.5, start + 2.5))["to_screen"]["count"], 0)
        self.assertEqual(raster.percentile([], 0.5), 0.0)
        self.assertEqual(raster.percentile([1.0, 2.0, 3.0, 4.0], 0.95), 4.0)

    def test_a_renderer_processs_traces_are_beside_the_browsers(self):
        with tempfile.TemporaryDirectory() as folder:
            trace = Path(folder) / "page.pftrace"
            for name in ("page.pftrace", "page.pftrace.412", "page.pftrace.97", "page.pftrace.old", "page.log",
                         "other.pftrace.5"):
                (Path(folder) / name).write_bytes(b"")
            self.assertEqual([path.name for path in raster.renderer_traces(trace)],
                             ["page.pftrace.412", "page.pftrace.97"])

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
                       "parts": {"raster::scene": {"count": 8, "total_ms": 5.0, "max_ms": 1.0},
                                 "raster::render": {"count": 8, "total_ms": 2.0, "max_ms": 1.0},
                                 "TileManager::ScanTile": {"count": 8, "total_ms": 0.5, "max_ms": 0.1},
                                 "TileManager::UploadTile": {"count": 2, "total_ms": 1.0, "max_ms": 0.6},
                                 "SharedImage::Write": {"count": 2, "total_ms": 0.6, "max_ms": 0.4},
                                 "SharedImage::Submit": {"count": 2, "total_ms": 0.3, "max_ms": 0.2}},
                       "browser": {"ChannelHost::OnMessage": {"count": 30, "total_ms": 1.5, "max_ms": 0.2},
                                   "SharedImage::Write": {"count": 2, "total_ms": 0.6, "max_ms": 0.4},
                                   "SharedImage::StageTexture": {"count": 1, "total_ms": 0.1, "max_ms": 0.1},
                                   "Display::DrawAndSwap": {"count": 4, "total_ms": 3.0, "max_ms": 1.0}},
                       "frames": {"to_viz": {"count": 4, "median_ms": 0.05, "p95_ms": 0.2, "max_ms": 0.2},
                                  "to_screen": {"count": 4, "median_ms": 1.2, "p95_ms": 2.5, "max_ms": 2.5}},
                       "tiles": {"rastered": 8, "solid": 6, "skipped": 12, "gpu": 4, "gpu_but_images": 2, "masks": 1,
                                 "blurs": 1}},
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
            named = Path(folder) / "another"
            named.mkdir()
            other = raster.write_report(results, env, False, 2.0, None, True, named, Path("/old/machopo"))
            self.assertEqual(other, named / "raster.md")
            other = other.read_text(encoding="utf-8")
        self.assertIn("the renderer in the browser's process, headless", text)
        self.assertIn("machopo at `/old/machopo`, a renderer process for the tab, headless", other)
        # The browser's side, and the frames' way to the screen; none of
        # either in a run from before they were told.
        self.assertIn("| page | scroll | 30, 1.5, 0.2 | 2, 0.6, 0.4 | 1, 0.1, 0.1 | 4, 3.0, 1.0 | 4 | 0.05, 0.2, 0.2 "
                      "| 1.2, 2.5, 2.5 |", text)
        self.assertIn("| page | resize | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 | 0, 0, 0 | 0 | 0, 0, 0 | 0, 0, 0 |", text)
        self.assertIn("headless, device scale factor 2, the default raster threads;", text)
        self.assertIn("headless, device scale factor 2, 3 raster threads;", threaded)
        self.assertIn("| page | scroll | 4 | 10.0 | 2.5, 5.0 | 2.0 | 8 | 6 (75%) | 12 |", text)
        self.assertIn("| page | resize | 0 | 0 | 0.0, 0 | 0 | 0 | 0 | 0 |", text)
        # The parts of the raster, of `PrepareTiles`; the tiles, of those rastered.
        self.assertIn("| page | scroll | 5.0 (50%) | 2.0 (20%) | 0.5 (5%) | 1.0 (10%) (0.6, 0.3) | 4 (50%) | 2 (25%) "
                      "| 1 (12%) | 0 (0%) | 1 (12%) | 0 (0%) | 0 (0%) |", text)
        # A run from before the parts were told has none.
        self.assertIn("| page | resize | 0 | 0 | 0 | 0 (0, 0) | 0 | 0 | 0 | 0 | 0 | 0 | 0 |", text)

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
            # Its frames were drawn, each after it was submitted, and Viz
            # put its tiles on the GPU; in the browser's process, no
            # channel's messages are handled.
            self.assertGreater(r["frames"]["to_screen"]["count"], 0, phase)
            self.assertGreater(r["frames"]["to_screen"]["median_ms"], 0, phase)
            self.assertGreaterEqual(r["frames"]["to_screen"]["median_ms"], r["frames"]["to_viz"]["median_ms"], phase)
            self.assertGreater(r["browser"]["Display::DrawAndSwap"]["count"], 0, phase)
            self.assertEqual(r["browser"]["ChannelHost::OnMessage"]["count"], 0, phase)
        # Resizing lays the page out again on the main thread.
        self.assertGreater(results["resize"]["main_frames"]["count"], 0)

    def test_a_renderer_in_a_process_of_its_own(self):
        page = "<!DOCTYPE html><body style='margin:0'>" + "".join(
            f"<p style='height:300px;background:#{i % 9}{i % 7}{i % 5}'>line {i}</p>" for i in range(20))
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(raster, "SCROLL_STEPS", 4), mock.patch.object(raster, "RESIZE_STEPS", 2):
            path = Path(folder) / "tall.html"
            path.write_text(page, encoding="utf-8")
            run = raster.run_page(self.binary, path.as_uri(), Path(folder), "tall", False, 2.0, None, True)
            # Its threads are in a trace of its own, read with the browser's.
            self.assertEqual(len(raster.renderer_traces(run["trace"])), 1)
            results = raster.analyze(run)
        for phase in ("scroll", "resize"):
            r = results[phase]
            self.assertGreater(r["prepare_tiles"]["count"], 0, phase)
            self.assertGreater(r["raster"]["count"], 0, phase)
            self.assertEqual(r["raster_threads"], 1, phase)
            self.assertGreater(r["frames"]["to_screen"]["count"], 0, phase)
            # Each of its messages is handled on the browser's IO thread,
            # and its tiles' pixels come in shared memory.
            self.assertGreater(r["browser"]["ChannelHost::OnMessage"]["count"], 0, phase)
            self.assertGreater(r["browser"]["SharedImage::StageTexture"]["count"], 0, phase)

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
