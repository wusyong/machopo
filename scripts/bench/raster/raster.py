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

A third table is the browser's side of the same frames, and how long a
frame takes to the screen: what the browser's IO thread spends on each of
a renderer process's messages (`ChannelHost::OnMessage`; there is none
with the renderer in the browser's process), what Viz spends putting
tiles on the GPU and drawing, and, from the log, each frame's time from
the compositor's `frame submitted` to Viz's `frame arrived` and `frame
drawn` of the same token. The renderer is a process of its own
(machopo's default), whose trace is a file of its own beside the
browser's; with `--single-process` it is threads of the browser's; `--binary` measures another build of machopo (an older
commit's), and `--name` keeps each run's results in a folder of its own.

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
# A frame on its way to the screen, in the log: submitted by the
# renderer's compositor (on its thread), then arrived at Viz and drawn,
# from the frame sink the browser bound that renderer to.
FRAME = re.compile(r"^(?P<time>\d{4}-\d\d-\d\dT[\d:.]+Z)\s+\w+\s+(?P<thread>\S+)\s.*"
                   r"frame (?P<what>submitted|arrived|drawn)(?P<fields>.*)$")
# The browser's own frame sink's client: its UI's frames, not a tab's.
BROWSER_CLIENT = 0
# The browser's threads, as the trace has them on Linux and macOS (names
# cut to 15 bytes; `thread_slices` cuts Windows's whole names to them).
VIZ_THREAD = "VizCompositorTh"
IO_THREAD = "Chrome_IOThread"
# What the browser's side does for a renderer's frames, each a span.
BROWSER_PARTS = {
    IO_THREAD: ("ChannelHost::OnMessage",),
    VIZ_THREAD: ("SharedImage::Write", "SharedImage::WriteTexture", "SharedImage::StageTexture",
                 "SharedImage::StageBuffer", "SharedImage::StageCopy", "SharedImage::StageUnmap",
                 "SharedImage::StageEncode", "SharedImage::CreateTexture", "SharedImage::BindGroups",
                 "SharedImage::Submit", "Display::DrawAndSwap"),
}


def page_url(kind: str, where: str, server: speedometer.Server) -> str:
    return (speedometer.ROOT / where).as_uri() if kind == "file" else server.url(where)


# The traces renderer processes wrote beside the browser's, as every tool
# reads them.
renderer_traces = speedometer.renderer_traces


def run_page(binary: Path, url: str, folder: Path, name: str, window: bool, scale: float,
             raster_threads: int | None = None, single_process: bool = False) -> dict:
    """`url` in machopo (with `--num-raster-threads raster_threads`, if given;
    the renderer threads of the browser's process if `single_process`),
    scrolled then resized: each phase's span, on the trace's clock and the
    wall clock (the log's), and where its trace and log are."""
    trace = folder / f"{name}.pftrace"
    log_path = folder / f"{name}.log"
    for old in [trace, *renderer_traces(trace)]:
        old.unlink(missing_ok=True)
    env = {**os.environ, "MACHOPO_TRACE": str(trace), "RUST_LOG": "info,compositor=debug,viz=debug"}
    arguments = [str(binary), "--automation-pipe", *(["--window"] if window else ["--scale", str(scale)]),
                 *(["--num-raster-threads", str(raster_threads)] if raster_threads else []),
                 *(["--single-process"] if single_process else [])]
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
        at = log_time(found["time"])
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


def log_time(text: str) -> float:
    return datetime.fromisoformat(text.replace("Z", "+00:00")).timestamp()


def frames_in_log(text: str) -> dict[str, list[tuple[float, int]]]:
    """The tab's frames the log tells of, each `(wall clock, token)`:
    submitted (the renderer's compositor's; the browser's UI's own are on
    another thread), and arrived at Viz and drawn (the sink that isn't the
    browser's own)."""
    found = {"submitted": [], "arrived": [], "drawn": []}
    for line in ANSI.sub("", text).splitlines():
        frame = FRAME.match(line)
        if not frame:
            continue
        fields = dict(COUNT.findall(frame["fields"]))
        if "frame_token" not in fields:
            continue
        if frame["what"] == "submitted":
            if frame["thread"] != "Compositor":
                continue
        elif int(fields.get("client_id", BROWSER_CLIENT)) == BROWSER_CLIENT:
            continue
        found[frame["what"]].append((log_time(frame["time"]), int(fields["frame_token"])))
    return found


def percentile(sorted_values: list[float], share: float) -> float:
    if not sorted_values:
        return 0.0
    return sorted_values[min(len(sorted_values) - 1, int(share * len(sorted_values)))]


def frame_latency(frames: dict[str, list[tuple[float, int]]], window: tuple[float, float]) -> dict:
    """Of the frames submitted in `window`: how long each took to arrive
    at Viz, and to be in what Viz drew (the first draw of that frame or a
    later one of its client's: a frame replaced before a draw was never on
    the screen itself). In ms."""
    arrived = {token: at for at, token in reversed(frames["arrived"])}
    drawn = frames["drawn"]
    to_viz, to_screen, at_draw = [], [], 0
    for at, token in frames["submitted"]:
        if not window[0] <= at <= window[1]:
            continue
        if token in arrived:
            to_viz.append((arrived[token] - at) * 1e3)
        while at_draw < len(drawn) and (drawn[at_draw][1] < token or drawn[at_draw][0] < at):
            at_draw += 1
        if at_draw < len(drawn):
            to_screen.append((drawn[at_draw][0] - at) * 1e3)
    to_viz.sort()
    to_screen.sort()

    def stats(values: list[float]) -> dict:
        return {"count": len(values), "median_ms": round(percentile(values, 0.5), 2),
                "p95_ms": round(percentile(values, 0.95), 2), "max_ms": round(max(values, default=0), 2)}

    return {"to_viz": stats(to_viz), "to_screen": stats(to_screen)}


def thread_slices(traces: list[Path]) -> dict[str, list[tuple[int, str | None]]]:
    """`perfetto_slices` of every process's trace together: a thread's
    name is one process's (the renderer's threads are in its own trace
    when it is a process of its own), cut to 15 bytes (`thread_name`)."""
    together: dict[str, list[tuple[int, str | None]]] = {}
    for trace in traces:
        for thread, slices in speedometer.perfetto_slices(trace).items():
            together.setdefault(thread_name(thread), []).extend(slices)
    return together


def thread_name(name: str) -> str:
    """A thread's `name` as the trace has it on Linux and macOS, cut to 15
    bytes: Windows keeps the whole of it (`VizCompositorThread`)."""
    return name.encode("utf-8")[:15].decode("utf-8", errors="ignore")


def analyze(run: dict) -> dict:
    """Each phase of a page's run: the compositor thread's `PrepareTiles`,
    the `raster::playback` calls (theirs and the raster threads') and how
    many threads made them, the renderer's main frames, the tiles the
    log counts, what the browser's side spent, and how long the frames
    took to the screen."""
    traces = [run["trace"], *renderer_traces(run["trace"])]
    slices = thread_slices(traces)
    compositor = spans(slices.get("Compositor", []), {"TileManager::PrepareTiles", "raster::playback"})
    rasters = {(trace, track): found for trace in traces for track, found in raster_spans(trace).items()}
    playbacks = [found for thread in rasters.values() for found in thread]
    main = spans(slices.get(speedometer.MAIN_THREAD, []), {"ProxyMain::BeginMainFrame"})
    browser = {thread: spans(slices.get(thread, []), set(names)) for thread, names in BROWSER_PARTS.items()}
    log = run["log"].read_text(encoding="utf-8", errors="replace")
    frames = frames_in_log(log)
    results = {}
    for name, phase in run["phases"].items():
        results[name] = {
            "browser": {part: summary(browser[thread], part, phase["trace"])
                        for thread, names in BROWSER_PARTS.items() for part in names},
            "frames": frame_latency(frames, phase["wall"]),
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
                 raster_threads: int | None = None, single_process: bool = False, folder: Path | None = None,
                 binary: Path | None = None) -> Path:
    """`raster.md`: for each page and phase, the content updates
    (`PrepareTiles`), the raster's total and each update's mean and
    longest, the main frames', the tiles rastered and how many were one
    color, and those not rastered."""
    # Not in the f-string: before Python 3.12 its expressions can't hold a
    # backslash.
    renderer = "the renderer in the browser's process" if single_process else "a renderer process for the tab"
    lines = [
        "# machopo's tile raster while scrolling and resizing",
        "",
        f"{date.today().isoformat()}, "
        f"{f'machopo at `{binary}`' if binary else f'machopo {speedometer.git_head()} (the `perf` profile)'}, "
        f"{renderer}, "
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
    lines += [
        "",
        "The browser's side (ms: count, total, longest), and each frame's time from the compositor's submit to "
        "Viz having it and to Viz having drawn it (ms: median, 95th percentile, longest):",
        "",
        "| Page | Phase | IO thread, a message | Viz: tiles to the GPU | of it staged from shared memory "
        "| Viz: draws | Frames | To Viz | To the screen |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for page, phases in results.items():
        for phase, r in phases.items():
            browser, frames = r.get("browser", {}), r.get("frames", {})

            def spent(name: str) -> str:
                found = browser.get(name, {})
                return f"{found.get('count', 0)}, {found.get('total_ms', 0)}, {found.get('max_ms', 0)}"

            def took(name: str) -> str:
                found = frames.get(name, {})
                return f"{found.get('median_ms', 0)}, {found.get('p95_ms', 0)}, {found.get('max_ms', 0)}"

            lines.append(f"| {page} | {phase} | {spent('ChannelHost::OnMessage')} | {spent('SharedImage::Write')} | "
                         f"{spent('SharedImage::StageTexture')} | {spent('Display::DrawAndSwap')} | "
                         f"{frames.get('to_screen', {}).get('count', 0)} | {took('to_viz')} | {took('to_screen')} |")
    folder = folder or RESULTS
    report = folder / "raster.md"
    report.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (folder / "raster.json").write_text(json.dumps(results, indent=1) + "\n", encoding="utf-8", newline="\n")
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
    folder = RESULTS / args.name if args.name else RESULTS
    folder.mkdir(parents=True, exist_ok=True)
    binary = Path(args.binary) if args.binary else speedometer.machopo_binary(speedometer.MACHOPO_PROFILE)
    server = speedometer.Server()
    results = {}
    try:
        for name in names:
            kind, where = PAGES[name]
            print(f"  {name}")
            results[name] = analyze(run_page(binary, page_url(kind, where, server), folder, name, args.window,
                                             args.scale, args.raster_threads, args.single_process))
    finally:
        server.close()
    report = write_report(results, env, args.window, args.scale, args.raster_threads, args.single_process, folder,
                          Path(args.binary) if args.binary else None)
    print(report.read_text(encoding="utf-8"))
    return 0
