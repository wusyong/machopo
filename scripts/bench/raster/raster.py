#!/usr/bin/env python3
"""`bench.py raster`: how long machopo rasters its tiles while a page scrolls and
while its window is resized.

machopo runs a few pages (`PAGES`) one after the other: `machopo --automation-pipe`
(headless, or a real window with `--window`), with `MACHOPO_TRACE` and the
compositor's debug log. On each page, once it has loaded, the tool scrolls
it (`SCROLL_STEPS` wheel events down) and then resizes the window (from
`WINDOW` down to `SMALLEST` and back, `RESIZE_STEPS` sizes each way). For
each phase it reads from the trace the compositor thread's
`TileManager::PrepareTiles` (the raster: machopo rasters a tile before the
frame that draws it, so a slow raster makes a long frame, not a missing
tile) and the `raster::playback` calls in it (on the raster threads,
`CompositorTileWorker1` and on, or the compositor thread itself), and the
renderer's main frames; and from the log, how many of the tiles rastered turned out to be
one color, and how many weren't rastered (nothing draws in them). The results go to `target/bench/raster/raster.md` (and
`raster.json`), the traces and logs next to them.

A second table says where a raster's time goes (`PARTS`: building the
scene, rendering its pixels, reading them for one color, and putting them on
the GPU, the write and the submit apart), and how many of the tiles rastered
a renderer that fills on the GPU could have drawn, with what kept the
others from it (the log's counts).

The raster on the compositor thread doesn't depend on which of Viz's
renderers draws the frames, so headless (no window shows up) measures it
the same, at the device scale factor `--scale` gives (default 2, a
high-DPI screen's: the raster's cost goes with the pixels; a real window
takes its screen's); frame intervals aren't reported: the wheel's events are answered
once the renderer has handled each, so frames come as fast as the events,
not as fast as machopo could draw them.

Standard library only (with `speedometer.py`'s pipe, server and trace
reader).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from datetime import date, datetime
from pathlib import Path

# `speedometer.py`'s pipe, server and trace reading, from its folder beside
# this one.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "speedometer"))
import speedometer  # noqa: E402

RESULTS = speedometer.ROOT / "target" / "bench" / "raster"

# The pages: a long document with images, a short one with a stylesheet,
# and a modern site (Speedometer's NewsSite, served by `speedometer.Server`).
PAGES = {
    "gutenberg": ("file", "tests/sites/gutenberg-28885/pg28885-images.html"),
    "sqlite": ("file", "tests/sites/sqlite/about.html"),
    "newssite-next": ("speedometer", "/resources/newssite/news-next/dist/index.html"),
}

WINDOW = (1280, 800)
SMALLEST = (680, 500)
SCROLL_STEPS = 120
SCROLL_DELTA = 100
RESIZE_STEPS = 41
# Between two events: about a frame.
PAUSE = 0.016

def trace_now() -> int:
    """Now, in ns on the trace's clock (Perfetto's): CLOCK_UPTIME_RAW on
    macOS, CLOCK_BOOTTIME on Linux; elsewhere the monotonic clock
    (Windows': QueryPerformanceCounter, as Perfetto's there)."""
    if sys.platform == "darwin":
        return time.clock_gettime_ns(time.CLOCK_UPTIME_RAW)
    if sys.platform.startswith("linux"):
        return time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    return time.monotonic_ns()


PREPARED = re.compile(r"^(?P<time>\d{4}-\d\d-\d\dT[\d:.]+Z)\s.*prepared tiles (?P<counts>.*)$")
COUNT = re.compile(r"(\w+)=(\d+)")
# What the log counts of the tiles a `PrepareTiles` rastered.
COUNTS = ("rastered", "solid", "skipped", "gpu", "gpu_but_images", "images", "masks", "filters", "blurs", "patterns",
          "bitmap_glyphs")
# The parts of a tile's raster, each a span on the thread that rasters it:
# the playback (the scene built, then rendered), its pixels read for one
# color, and put on the GPU (Viz's write and submit, in the upload).
PARTS = ("raster::playback", "raster::scene", "raster::render", "TileManager::ScanTile", "TileManager::UploadTile",
         "SharedImage::Write", "SharedImage::CreateTexture", "SharedImage::WriteTexture", "SharedImage::BindGroups",
         "SharedImage::Submit")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def page_url(kind: str, where: str, server: speedometer.Server) -> str:
    return (speedometer.ROOT / where).as_uri() if kind == "file" else server.url(where)


def run_page(binary: Path, url: str, folder: Path, name: str, window: bool, scale: float,
             raster_threads: int | None = None) -> dict:
    """`url` in machopo (with `--num-raster-threads raster_threads`, if given),
    scrolled then resized: each phase's span, on the trace's clock and the
    wall clock (the log's), and where its trace and log are."""
    trace = folder / f"{name}.pftrace"
    log_path = folder / f"{name}.log"
    trace.unlink(missing_ok=True)
    env = {**os.environ, "MACHOPO_TRACE": str(trace), "RUST_LOG": "info,compositor=debug"}
    arguments = [str(binary), "--automation-pipe", *(["--window"] if window else ["--scale", str(scale)]),
                 *(["--num-raster-threads", str(raster_threads)] if raster_threads else [])]
    phases = {}
    with open(log_path, "w", encoding="utf-8") as log:
        process = subprocess.Popen(arguments, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env)
        machopo = speedometer.CdpPipe(process, process.stdin, process.stdout)
        machopo.name = "machopo"
        try:
            bounds = {"width": WINDOW[0], "height": WINDOW[1]}
            machopo.call("Browser.setWindowBounds", {"windowId": 1, "bounds": bounds})
            session = machopo.navigate(url)
            machopo.call("Machopo.waitForLoad", {}, timeout=60)
            time.sleep(2)

            def phase(label: str, steps) -> None:
                start = (trace_now(), time.time())
                for step in steps:
                    step()
                    time.sleep(PAUSE)
                time.sleep(0.5)
                phases[label] = {"trace": (start[0], trace_now()), "wall": (start[1], time.time())}

            wheel = {"type": "mouseWheel", "x": WINDOW[0] // 2, "y": WINDOW[1] // 2, "deltaX": 0,
                     "deltaY": SCROLL_DELTA}
            phase("scroll", [lambda: machopo.call("Input.dispatchMouseEvent", wheel, session)] * SCROLL_STEPS)
            time.sleep(1)
            phase("resize", [lambda size=size: machopo.call("Browser.setWindowBounds", {
                "windowId": 1, "bounds": {"width": size[0], "height": size[1]}}) for size in resize_sizes()])
        finally:
            machopo.close()
    return {"trace": trace, "log": log_path, "phases": phases}


def resize_sizes() -> list[tuple[int, int]]:
    """The window's sizes, `WINDOW` to `SMALLEST` in `RESIZE_STEPS`, and
    back."""
    down = [(round(WINDOW[0] + (SMALLEST[0] - WINDOW[0]) * i / (RESIZE_STEPS - 1)),
             round(WINDOW[1] + (SMALLEST[1] - WINDOW[1]) * i / (RESIZE_STEPS - 1))) for i in range(RESIZE_STEPS)]
    return down + down[::-1]


def spans(slices: list[tuple[int, str | None]], names: set[str]) -> list[tuple[str, int, int]]:
    """The slices named `names` among a thread's begins and ends
    (`perfetto_slices`'), each `(name, start, end)`."""
    found, stack = [], []
    for time_, name in slices:
        if name is not None:
            stack.append((name, time_))
        elif stack:
            begun, start = stack.pop()
            if begun in names:
                found.append((begun, start, time_))
    return found


def summary(found: list[tuple[str, int, int]], name: str, window: tuple[int, int]) -> dict:
    """How many of `name` began in `window`, their total and longest (ms)."""
    lengths = [(end - start) / 1e6 for begun, start, end in found if begun == name and window[0] <= start <= window[1]]
    return {"count": len(lengths), "total_ms": round(sum(lengths), 1), "max_ms": round(max(lengths, default=0), 1)}


def tiles_in_log(text: str, window: tuple[float, float]) -> dict:
    """The tiles the compositor's debug log says it rastered in `window`
    (wall clock, seconds), how many of them were one color, and how many
    it didn't raster (nothing draws in them)."""
    counts = dict.fromkeys(COUNTS, 0)
    for line in ANSI.sub("", text).splitlines():
        found = PREPARED.match(line)
        if not found:
            continue
        at = datetime.fromisoformat(found["time"].replace("Z", "+00:00")).timestamp()
        if window[0] <= at <= window[1]:
            for name, count in COUNT.findall(found["counts"]):
                if name in counts:
                    counts[name] += int(count)
    return counts


# The raster threads' names, `CompositorTileWorker1` and on, as the trace
# has them: cut to 15 bytes, the platforms' limit for a thread's name.
RASTER_THREAD = "CompositorTileW"


def raster_spans(trace: Path) -> dict[int, list[tuple[str, int, int]]]:
    """The `raster::playback` calls of each thread that rasters (the raster
    threads and the compositor thread), by its track: the raster threads
    share a name."""
    threads, events = speedometer.perfetto_track_events(trace)
    by_track: dict[int, list[tuple[int, str | None]]] = {}
    for track, time, _, kind, name, _ in sorted(events, key=lambda event: (event[0] or 0, event[1], event[2])):
        thread = threads.get(track, "")
        if kind in (1, 2) and (thread.startswith(RASTER_THREAD) or thread == "Compositor"):
            by_track.setdefault(track, []).append((time, name if kind == 1 else None))
    return {track: spans(found, set(PARTS)) for track, found in by_track.items()}


def analyze(run: dict) -> dict:
    """Each phase of a page's run: the compositor thread's `PrepareTiles`,
    the `raster::playback` calls (theirs and the raster threads') and how
    many threads made them, the renderer's main frames, and the tiles the
    log counts."""
    slices = speedometer.perfetto_slices(run["trace"])
    compositor = spans(slices.get("Compositor", []), {"TileManager::PrepareTiles", "raster::playback"})
    rasters = raster_spans(run["trace"])
    playbacks = [found for thread in rasters.values() for found in thread]
    main = spans(slices.get(speedometer.MAIN_THREAD, []), {"ProxyMain::BeginMainFrame"})
    log = run["log"].read_text(encoding="utf-8", errors="replace")
    results = {}
    for name, phase in run["phases"].items():
        results[name] = {
            "prepare_tiles": summary(compositor, "TileManager::PrepareTiles", phase["trace"]),
            "raster": summary(playbacks, "raster::playback", phase["trace"]),
            "parts": {part: summary(playbacks, part, phase["trace"]) for part in PARTS},
            "raster_threads": sum(1 for thread in rasters.values()
                                  if summary(thread, "raster::playback", phase["trace"])["count"]),
            "main_frames": summary(main, "ProxyMain::BeginMainFrame", phase["trace"]),
            "tiles": tiles_in_log(log, phase["wall"]),
        }
    return results


def write_report(results: dict[str, dict], env: dict, window: bool, scale: float,
                 raster_threads: int | None = None) -> Path:
    """`raster.md`: for each page and phase, the content updates
    (`PrepareTiles`), the raster's total and each update's mean and
    longest, the main frames', the tiles rastered and how many were one
    color, and those not rastered."""
    lines = [
        "# machopo's tile raster while scrolling and resizing",
        "",
        f"{date.today().isoformat()}, machopo {speedometer.git_head()} (the `perf` profile), "
        f"{'a real window' if window else f'headless, device scale factor {scale:g}'}, "
        f"{f'{raster_threads} raster threads' if raster_threads else 'the default raster threads'}; "
        f"{env['os']}, {env['cpu']}, power: {env['power']}. "
        f"Scroll: {SCROLL_STEPS} wheel events of {SCROLL_DELTA} px; resize: {WINDOW[0]} × {WINDOW[1]} to "
        f"{SMALLEST[0]} × {SMALLEST[1]} and back, {RESIZE_STEPS} sizes each way. Times in ms; a content update is "
        "one `TileManager::PrepareTiles` on the compositor thread.",
        "",
        "| Page | Phase | Updates | Raster (`PrepareTiles`) | Mean, longest | Main frames | Tiles rastered | One color "
        "| Not rastered |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for page, phases in results.items():
        for phase, r in phases.items():
            prepare, main, tiles = r["prepare_tiles"], r["main_frames"], r["tiles"]
            mean = prepare["total_ms"] / prepare["count"] if prepare["count"] else 0
            share = f"{tiles['solid']} ({100 * tiles['solid'] / tiles['rastered']:.0f}%)" if tiles["rastered"] else "0"
            lines.append(f"| {page} | {phase} | {prepare['count']} | {prepare['total_ms']} | {mean:.1f}, "
                         f"{prepare['max_ms']} | {main['total_ms']} | {tiles['rastered']} | {share} | "
                         f"{tiles['skipped']} |")
    lines += [
        "",
        "Where the raster's time goes (ms, and of `PrepareTiles`), and the tiles a renderer that fills on the GPU "
        "could have drawn (of those rastered; `+ images`: those only their images kept from it):",
        "",
        "| Page | Phase | Scene | Render | Read for one color | Upload (write, submit) | GPU could | + images "
        "| Masks | Filters | Blurs | Patterns | Bitmap glyphs |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for page, phases in results.items():
        for phase, r in phases.items():
            total, parts, tiles = r["prepare_tiles"]["total_ms"], r.get("parts", {}), r["tiles"]

            def part(name: str) -> str:
                ms = parts.get(name, {}).get("total_ms", 0)
                return f"{ms} ({100 * ms / total:.0f}%)" if total else f"{ms}"

            def share(name: str) -> str:
                count = tiles.get(name, 0)
                return f"{count} ({100 * count / tiles['rastered']:.0f}%)" if tiles["rastered"] else "0"

            write = parts.get("SharedImage::Write", {}).get("total_ms", 0)
            submit = parts.get("SharedImage::Submit", {}).get("total_ms", 0)
            lines.append(f"| {page} | {phase} | {part('raster::scene')} | {part('raster::render')} | "
                         f"{part('TileManager::ScanTile')} | {part('TileManager::UploadTile')} ({write}, {submit}) | "
                         f"{share('gpu')} | {share('gpu_but_images')} | {share('masks')} | {share('filters')} | "
                         f"{share('blurs')} | {share('patterns')} | {share('bitmap_glyphs')} |")
    report = RESULTS / "raster.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (RESULTS / "raster.json").write_text(json.dumps(results, indent=1) + "\n", encoding="utf-8", newline="\n")
    return report


def cmd_raster(args) -> int:
    names = args.pages.split(",") if args.pages else list(PAGES)
    for name in names:
        if name not in PAGES:
            print(f"no page {name}; the pages: {', '.join(PAGES)}")
            return 2
    env = speedometer.environment(speedometer.MACHOPO_PROFILE)
    if env["power"] == "battery":
        print("warning: on battery power, the times may not be comparable")
    RESULTS.mkdir(parents=True, exist_ok=True)
    binary = speedometer.machopo_binary(speedometer.MACHOPO_PROFILE)
    server = speedometer.Server()
    results = {}
    try:
        for name in names:
            kind, where = PAGES[name]
            print(f"  {name}")
            results[name] = analyze(run_page(binary, page_url(kind, where, server), RESULTS, name, args.window,
                                             args.scale, args.raster_threads))
    finally:
        server.close()
    report = write_report(results, env, args.window, args.scale, args.raster_threads)
    print(report.read_text(encoding="utf-8"))
    return 0
