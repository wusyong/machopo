#!/usr/bin/env python3
"""Test runs too slow, or too demanding of the machine, for `cargo test`.

    python scripts/test.py matrix      # GC tests under each pool size, and under stress
    python scripts/test.py fuzz [--iterations N] [--minutes M] [--seed S] [--max-flags N] [--out FILE]
                                       # random GC configurations (numfuzz-style)
    python scripts/test.py automation [FILTER ...]
                                       # every scenario in tests/scenarios/ in a real window (machopo --automation),
                                       # and the pipe's test with one (machopo --automation-pipe --window)
    python scripts/test.py scenarios [FILTER ...]
                                       # the scenarios without the platform's window (cargo test skips them)
    python scripts/test.py wpt [TEST ...] [--processes N] [--chunk I/N] [--profile NAME] [--no-build]
                                       # WPT's testharness.js tests, reftests and crashtests, against
                                       # machopodriver and machopo (wptrunner)
    python scripts/test.py wpt-check RAW_LOG ...
                                       # wpt's checks of the whole list over the logs of a run in chunks
    python scripts/test.py reftest [TEST ...] [--processes N] [--profile NAME] [--no-build]
                                       # our own reftests (wptrunner)
    python scripts/test.py wdspec [TEST ...] [--profile NAME] [--no-build]
                                       # WPT's WebDriver tests, against machopodriver and machopo (wptrunner)
    python scripts/test.py mutate SPEC [--only NAME ...] [--keep-going] | --restore
                                       # mutation testing: break the code as SPEC lists, check a test fails
    python scripts/test.py cdp [CASE ...]
                                       # machopo's answers to DevTools' JavaScript domains against Chrome's

`matrix` and `fuzz` link the debug V8 (V8's `DCHECK`s), in a target directory
of their own; `automation` needs a display and a GPU. `scenarios` is a
test of `cargo test` that skips itself unless `MACHOPO_SLOW_TESTS` is set,
which the command sets. `wpt`, `reftest` and `wdspec` run upstream's
wptrunner, in a venv of its own, against the debug build or another
profile's (`--profile ci`, as the nightly CI). Every command first syncs
../third_party to the commits machopo pins (`ci.py third-party`), and runs
nothing if it can't. The GC's own checks (loom, shuttle, its fuzzer,
Verus, Miri) are elise's, in ../third_party/elise/scripts/test.py.
The checks of the repo's rules are in `sanity.py`, the tools around the build
in `ci.py`. Each command's arguments are below, and in `test.py <command> -h`.

Standard library only. Every command exits non-zero on failure.
"""

from __future__ import annotations

import argparse
import inspect
import json
import os
import random
import re
import shutil
import signal
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The crates whose tests exercise the GC under V8's flags and V8's
# `DCHECK`s: `bindings` (V8's heap, the fork's slots, the platform the DOM's
# heap is driven by) and `dom_js` (JS holding the DOM's nodes through
# wrappers and listeners, the DOM's heap marked with V8's). The other
# crates' tests run in a plain `cargo test`.
GC_TEST_ARGS = [
    "test",
    *("-p", "bindings"),
    *("-p", "dom_js"),
    # Every test binary, past the first one that fails: else one failure
    # hides whatever the rest would have found under the same configuration.
    "--no-fail-fast",
]

# The debug V8, with V8's own `DCHECK`s: rusty_v8 links the release
# prebuilt unless `V8_FORCE_DEBUG` is set. Changing it reruns
# the `v8` build script and rebuilds everything above it, so the debug V8
# gets a target directory of its own and the two builds stay warm. The path
# is absolute: trybuild resolves a relative one from each crate.
DEBUG_V8_TARGET = ROOT / "target" / "v8-debug"
DEBUG_V8_ENV = {"V8_FORCE_DEBUG": "1", "CARGO_TARGET_DIR": str(DEBUG_V8_TARGET)}


# --- Running the GC tests -----------------------------------------------------


@dataclass
class Config:
    """One way to run the GC tests. Every field maps to an environment
    variable the tests read at run time, so changing it doesn't rebuild."""

    pool: int | None = None  # MACHOPO_V8_THREAD_POOL_SIZE; None = V8's default
    seed: int | None = None  # MACHOPO_TEST_SEED; None = each test's own default
    flags: list[str] = field(default_factory=list)  # MACHOPO_V8_FLAGS
    # The DOM's heap's own options (`dom_js` reads them): `stress`, `verify`,
    # `nursery=BYTES`. V8's flags reach the GC only through the engine that
    # drives it; these stress the heap itself.
    gc: list[str] = field(default_factory=list)  # MACHOPO_GC_OPTIONS
    # V8's own seed (`--random-seed`, `--fuzzer-random-seed`), which picks
    # where the random GC intervals fall. Left at 0, `--random-gc-interval`
    # draws from the system's randomness (a failure can't be replayed). Must
    # be nonzero: 0 means "unset" to V8.
    v8_seed: int | None = None

    def env(self) -> dict[str, str]:
        env = {}
        if self.pool is not None:
            env["MACHOPO_V8_THREAD_POOL_SIZE"] = str(self.pool)
        if self.seed is not None:
            env["MACHOPO_TEST_SEED"] = f"0x{self.seed:08x}"
        flags = list(self.flags)
        if self.v8_seed is not None:
            flags += [f"--random-seed={self.v8_seed}", f"--fuzzer-random-seed={self.v8_seed}"]
        if flags:
            env["MACHOPO_V8_FLAGS"] = " ".join(flags)
        if self.gc:
            env["MACHOPO_GC_OPTIONS"] = ",".join(self.gc)
        return env

    def describe(self) -> str:
        env = self.env()
        if not env:
            return "(V8's defaults)"
        return " ".join(f"{k}={v!r}" if " " in v else f"{k}={v}" for k, v in env.items())

    def repro(self) -> str:
        debug_v8 = f"V8_FORCE_DEBUG=1 CARGO_TARGET_DIR={DEBUG_V8_TARGET}"
        env = " ".join(f"{k}={v!r}" if " " in v else f"{k}={v}" for k, v in self.env().items())
        return " ".join(part for part in (debug_v8, env, "cargo", *GC_TEST_ARGS) if part)


@dataclass
class Result:
    ok: bool
    summary: str
    output: str
    # What failed, without what changes from one build or run to the next
    # (binaries' hashes, how a binary crashed): the same bug gives the same
    # signature under any configuration. Empty when `ok`.
    signature: str = ""
    # Every thread's stack, taken before a hung run was killed, or when a
    # slow one was first found still busy past its timeout.
    stacks: str = ""
    # Ran past its timeout, kept busy, and got to the end (`wait_for`).
    slow: bool = False


# A test binary that cargo ran and that failed: its path, its name without
# the hash, and how it exited.
CRASHED_BINARY = re.compile(r"process didn't exit successfully: `([^`]*?([\w-]+)-[0-9a-f]{16}(?:\.exe)?)` \(([^)]*)\)")

# Each test binary `build_gc_tests` built, and its package's directory,
# where cargo runs it.
TEST_BINARIES: dict[str, Path] = {}


def build_gc_tests() -> bool:
    """Builds the GC tests with the debug V8 before any configuration runs,
    so that a configuration's timeout counts only its tests. The
    configurations change only environment variables the tests read at run
    time, so they don't rebuild."""
    env = {**os.environ, **DEBUG_V8_ENV}
    proc = subprocess.run(
        ["cargo", *GC_TEST_ARGS, "--no-run", "--message-format=json-render-diagnostics"],
        cwd=ROOT,
        env=env,
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    for line in proc.stdout.splitlines():
        message = json.loads(line) if line.startswith("{") else {}
        if message.get("reason") == "compiler-artifact" and message.get("executable"):
            TEST_BINARIES[message["executable"]] = Path(message["manifest_path"]).parent
    return proc.returncode == 0


def debug_crashes(config: Config, output: str, timeout: float) -> str:
    """Each test binary that crashed in `output`, run again on its own
    under gdb or lldb with the same configuration, and the stacks of every
    thread when it crashes again. V8 prints a stack trace when it aborts,
    but it can crash again while printing it, and its offsets mean nothing
    once the next build replaces the binary. A crash that doesn't come back
    on its own leaves only the debugger saying the binary exited."""
    debugger = shutil.which("gdb") or shutil.which("lldb")
    if not debugger:
        return ""
    env = {"RUST_BACKTRACE": "1", **os.environ, **DEBUG_V8_ENV, **config.env()}
    stacks = []
    for binary, name, why in CRASHED_BINARY.findall(output):
        if why == "exit status: 101" or binary not in TEST_BINARIES:
            continue
        package = TEST_BINARIES[binary]
        if Path(debugger).name == "gdb":
            command = [debugger, "-batch", "-nx", "-ex", "run", "-ex", "bt", "-ex", "thread apply all bt", "--args", binary]
        else:
            command = [debugger, "--batch", "-o", "run", "-k", "bt", "-k", "thread backtrace all", "--", binary]
        try:
            out = subprocess.run(
                command,
                cwd=package,
                env={**env, "CARGO_MANIFEST_DIR": str(package)},
                capture_output=True,
                text=True,
                errors="replace",
                timeout=timeout,
            )
            stacks.append(f"=== {name} under {Path(debugger).name} ===\n{out.stdout}{out.stderr}")
        except subprocess.TimeoutExpired:
            stacks.append(f"=== {name} under {Path(debugger).name}: timed out ===")
    return "\n".join(stacks)


def run_gc_tests(config: Config, timeout: float | None = None) -> Result:
    """Runs the GC tests under `config`. Past `timeout` seconds a run that
    has stopped using the CPU (a deadlock in the GC's threads, say), or that
    keeps busy past the limit, is taken to be hung: the stacks of every
    thread of the tests still running are taken, then they're killed. One
    that keeps busy and gets to the end is slow, not hung (`wait_for`)."""
    # A panic's backtrace, unless the caller's environment says otherwise:
    # a failure under a random configuration may not come back.
    env = {"RUST_BACKTRACE": "1", **os.environ, **DEBUG_V8_ENV, **config.env()}
    proc = subprocess.Popen(
        ["cargo", *GC_TEST_ARGS],
        cwd=ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        # Its own process group, so a hung run is killed with every test
        # binary under it.
        start_new_session=True,
    )
    waited = wait_for(proc, timeout)
    output = waited.output
    if waited.outcome == "hung":
        running = re.findall(r"^\s+Running .*\((.*)\)$", output, re.M)
        hung = re.sub(r"-[0-9a-f]{16}(\.exe)?$", "", Path(running[-1]).name) if running else "cargo"
        why = {True: "still busy, ", False: "idle, ", None: ""}[waited.busy]
        summary = f"hung in {hung}: {why}killed after {minutes(waited.took)}"
        return Result(False, summary, output, f"{hung} timed out", waited.stacks)
    # V8 only prints a warning for a flag it doesn't know and carries on, so
    # a typo would silently test nothing. Treat it as a failure.
    unknown = re.findall(r"unrecognized flag (\S+)", output)
    if unknown:
        return Result(False, f"V8 didn't recognize {', '.join(sorted(set(unknown)))}", output, "unrecognized flag")
    results = re.findall(r"^test result: (\w+)\. (\d+) passed; (\d+) failed", output, re.M)
    passed = sum(int(p) for _, p, _ in results)
    failed = sum(int(f) for _, _, f in results)
    if proc.returncode != 0:
        # Exit status 101 is libtest's for failed tests, counted above;
        # anything else is a crash (a signal, V8's abort).
        crashed = [f"{name} ({why})" for _, name, why in CRASHED_BINARY.findall(output) if why != "exit status: 101"]
        why = [f"{failed} failed"] if failed else []
        why += [f"crashed: {', '.join(crashed)}"] if crashed else []
        return Result(False, f"{passed} passed, {'; '.join(why or ['cargo failed'])}", output, failure_signature(output))
    summary = f"{passed} passed in {len(results)} test binaries"
    if waited.outcome == "slow":
        summary += f", slowly: past the timeout of {minutes(timeout)}, done in {minutes(waited.took)}"
        return Result(True, summary, output, stacks=waited.stacks, slow=True)
    return Result(True, summary, output)


# Past its timeout, a run whose processes keep using the CPU is taken to be
# slow rather than hung (a configuration under which the GC's debug checks
# turn quadratic, say) and waited for, checked every `PROGRESS_INTERVAL`
# seconds, up to `SLOW_LIMIT_FACTOR` times its timeout in all. A deadlock
# leaves every thread waiting, using no CPU; an endless loop keeps using it,
# hence the limit.
PROGRESS_INTERVAL = 30
SLOW_LIMIT_FACTOR = 3
# The CPU seconds the processes have to use in an interval to count as busy.
BUSY_CPU_SECONDS = 1.0
# Linux's processes, read by `cpu_usage` (elsewhere it runs `ps`).
PROC = Path("/proc")


@dataclass
class Waited:
    output: str
    # "done" (within the timeout), "slow" (done past it) or "hung" (killed).
    outcome: str
    took: float
    # Every thread's stack: a hung run's before it was killed, a slow one's
    # when it was first found busy past the timeout.
    stacks: str = ""
    # Whether a hung run was killed still busy (at the limit) or idle;
    # `None` when its CPU time can't be read.
    busy: bool | None = None


def wait_for(proc: subprocess.Popen, timeout: float | None) -> Waited:
    """`proc` (started in a session of its own) waited for up to `timeout`
    seconds; past it, for as long as it and the processes under it keep
    busy (`cpu_usage`), up to the limit; killed with them when they don't,
    or at the limit. Where their CPU time can't be read (Windows), killed
    at the timeout."""
    start = time.monotonic()
    try:
        output, _ = proc.communicate(timeout=timeout)
        return Waited(output, "done", time.monotonic() - start)
    except subprocess.TimeoutExpired:
        pass
    limit = start + SLOW_LIMIT_FACTOR * timeout
    stacks = ""
    usage = cpu_usage(proc.pid)
    busy = None if usage is None else False
    while usage is not None and time.monotonic() < limit:
        try:
            output, _ = proc.communicate(timeout=min(PROGRESS_INTERVAL, max(0.0, limit - time.monotonic())))
            return Waited(output, "slow", time.monotonic() - start, stacks)
        except subprocess.TimeoutExpired:
            pass
        now = cpu_usage(proc.pid)
        busy = now is not None and making_progress(usage, now)
        if not busy:
            break
        if not stacks:
            stacks = stopped_stacks(proc, then=signal.SIGCONT)
        usage = now
    output, stacks = killed_with_stacks(proc)
    return Waited(output, "hung", time.monotonic() - start, stacks, busy)


def making_progress(before: dict[int, float], after: dict[int, float]) -> bool:
    """Whether the processes under a run moved on between two looks at
    them (`cpu_usage`): one ended or a new one started (cargo going on to
    the next test binary), or they used `BUSY_CPU_SECONDS` of CPU."""
    if before.keys() != after.keys():
        return True
    return sum(after.values()) - sum(before.values()) >= BUSY_CPU_SECONDS


def stopped_stacks(proc: subprocess.Popen, then: int) -> str:
    """The stacks of every thread under `proc`, all stopped first, so the
    stacks are of one moment, and a test binary can't exit (or cargo start
    the next) while the debugger attaches to each in turn; then `then`
    sent to them (going on, or killed)."""
    os.killpg(proc.pid, signal.SIGSTOP)
    stacks = thread_stacks(proc.pid)
    os.killpg(proc.pid, then)
    return stacks


def killed_with_stacks(proc: subprocess.Popen) -> tuple[str, str]:
    """`proc`'s output once it and every process under it are killed, after
    their stacks are taken (`stopped_stacks`)."""
    if os.name == "nt":
        stacks = thread_stacks(proc.pid)
        proc.kill()
    else:
        stacks = stopped_stacks(proc, then=signal.SIGKILL)
    output, _ = proc.communicate()
    return output, stacks


def cpu_usage(pid: int) -> dict[int, float] | None:
    """The CPU seconds each of `pid` and the processes under it has used,
    by pid; `None` where it can't be read (Windows, or `pid` gone). Linux's
    `/proc` (clock ticks), else `ps` (whose `time` Linux gives only to the
    second)."""
    if os.name == "nt":
        return None
    parents: dict[int, int] = {}
    used: dict[int, float] = {}
    if (PROC / "self" / "stat").exists():
        ticks = os.sysconf("SC_CLK_TCK")
        for entry in PROC.iterdir():
            if not entry.name.isdigit():
                continue
            try:
                stat = (entry / "stat").read_text()
            except OSError:
                continue
            # After the command's name, which may hold spaces and
            # parentheses: state, ppid, ..., utime and stime (fields 14, 15).
            fields = stat[stat.rfind(")") + 2 :].split()
            parents[int(entry.name)] = int(fields[1])
            used[int(entry.name)] = (int(fields[11]) + int(fields[12])) / ticks
    else:
        out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,time="], capture_output=True, text=True).stdout
        for line in out.splitlines():
            child, parent, time_ = line.split()
            parents[int(child)] = int(parent)
            used[int(child)] = cpu_seconds(time_)
    if pid not in used:
        return None
    under, pending = {}, [pid]
    while pending:
        current = pending.pop()
        under[current] = used[current]
        pending += [child for child, parent in parents.items() if parent == current and child in used]
    return under


def cpu_seconds(text: str) -> float:
    """`ps`'s `time`: `[[dd-]hh:]mm:ss[.ss]`."""
    days, _, clock = text.rpartition("-")
    seconds = 0.0
    for part in clock.split(":"):
        seconds = seconds * 60 + float(part)
    return seconds + int(days or 0) * 86400


def crash_stack(debugged: str, frames: int = 40) -> list[str]:
    """The first frames of the thread that failed in `debug_crashes`'
    output: the one in V8's abort or a Rust panic if there is one (another
    thread may crash first while the isolate comes apart), else the one the
    debugger stopped in."""
    stacks: list[list[str]] = []
    for line in debugged.splitlines():
        frame = re.match(r"\s*\*?\s*(frame )?#(\d+)[: ]", line)
        if frame and (frame.group(2) == "0" or not stacks):
            stacks.append([])
        if frame:
            stacks[-1].append(line.strip())
    failed = [stack for stack in stacks if any(re.search(r"V8_Fatal|rust_panic|panic_fmt", f) for f in stack)]
    return (failed or stacks or [[]])[0][:frames]


def minutes(seconds: float) -> str:
    return f"{seconds / 60:.0f} min" if seconds >= 60 else f"{seconds:.0f}s"


def failure_signature(output: str) -> str:
    """What failed in a run's output: the failed tests and the binaries that
    crashed. Not how a binary crashed, nor V8's message: one bug can crash
    with a different signal, or before V8 has printed its check, from one
    run to the next."""
    parts = {f"{name} crashed" for _, name, why in CRASHED_BINARY.findall(output) if why != "exit status: 101"}
    parts |= {f"test {name}" for name in re.findall(r"^test (\S+) \.\.\. FAILED", output, re.M)}
    return "; ".join(sorted(parts)) or "cargo failed"


def thread_stacks(pid: int) -> str:
    """The stacks of every thread of each process under `pid` (cargo's test
    binaries), from gdb or lldb, whichever there is. Best effort: without
    either, or where the system doesn't let them attach, it says so."""
    debugger = shutil.which("gdb") or shutil.which("lldb")
    if os.name == "nt" or not debugger:
        return "(no thread stacks: neither gdb nor lldb found)"
    children, pending = [], [pid]
    while pending:
        out = subprocess.run(["pgrep", "-P", str(pending.pop())], capture_output=True, text=True).stdout
        found = [int(child) for child in out.split()]
        children += found
        pending += found
    stacks = []
    for child in children:
        if Path(debugger).name == "gdb":
            command = [debugger, "-p", str(child), "-batch", "-nx", "-ex", "thread apply all bt"]
        else:
            command = [debugger, "-p", str(child), "--batch", "-o", "thread backtrace all"]
        try:
            out = subprocess.run(command, capture_output=True, text=True, errors="replace", timeout=300)
            stacks.append(f"=== threads of process {child} ===\n{out.stdout}{out.stderr}")
        except subprocess.TimeoutExpired:
            stacks.append(f"=== threads of process {child}: {Path(debugger).name} timed out ===")
    return "\n".join(stacks) or "(no thread stacks: no process left under cargo)"


def sync_third_party() -> bool:
    """`ci.py third-party`: ../third_party at the commits machopo pins."""
    if subprocess.run([sys.executable, str(ROOT / "scripts" / "ci.py"), "third-party"], cwd=ROOT).returncode == 0:
        return True
    print("../third_party isn't at the commits machopo pins (above): nothing was run")
    return False


def revisions() -> dict[str, str]:
    """The commits of machopo and of the rusty_v8 fork the tests were built
    from, `+dirty` with uncommitted changes to tracked files: a failure
    recorded days ago replays only on the same code."""
    result = {}
    for name, path in (("machopo", ROOT), ("rusty_v8", ROOT.parent / "third_party" / "rusty_v8")):
        if not path.exists():
            result[name] = "missing"
            continue
        head = subprocess.run(["git", "rev-parse", "--short=12", "HEAD"], cwd=path, capture_output=True, text=True)
        dirty = subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=no"], cwd=path, capture_output=True, text=True
        )
        result[name] = head.stdout.strip() + ("+dirty" if dirty.stdout.strip() else "") or "unknown"
    return result


# How long a configuration may run before it counts as hung: this many
# times the baseline (the first configuration, V8's defaults), as stress
# flags slow the tests several times over, but at least `MIN_TIMEOUT`
# seconds, so a quick baseline doesn't cut short a slow configuration that is
# still making progress.
TIMEOUT_FACTOR = 10
MIN_TIMEOUT = 10 * 60


def timeout_after(baseline_seconds: float) -> float:
    return max(MIN_TIMEOUT, TIMEOUT_FACTOR * baseline_seconds)


def failure_excerpt(output: str, lines: int = 40) -> str:
    """The part of a failed run worth printing: panics, failed tests, and
    crashes, or else the tail."""
    keep = [
        line
        for line in output.splitlines()
        if re.search(r"panicked at|FAILED|STATUS_|exit code|^error|^# Fatal|^# Check failed|seed", line)
    ]
    return "\n".join((keep or output.splitlines())[-lines:])


# --- matrix -----------------------------------------------------------------


# The strongest stress the fuzzer has, on every matrix run rather than when
# the fuzzer happens to draw it: GCs at the JS heap's allocation counts, and
# marking started early and in many small steps.
MATRIX_STRESS_FLAGS = [
    "--random-gc-interval=500",
    "--stress-incremental-marking",
]
# Fixed, so a matrix failure replays exactly; the fuzzer varies it.
MATRIX_V8_SEED = 0x6B756E
# The DOM's heap stressed itself: a GC's work at every safepoint (a
# scavenge, in a heap V8 drives), the heap checked after each GC, and a
# nursery (the renderer's heap has none) for the scavenges to move.
MATRIX_GC_OPTIONS = ["stress", "verify", "nursery=1048576"]


def matrix_configs() -> list[Config]:
    """V8's default pool, a single worker, and eight workers, then the
    stress flags, then the DOM's heap's own stress."""
    return [Config(pool=pool) for pool in (None, 1, 8)] + [
        Config(flags=MATRIX_STRESS_FLAGS, v8_seed=MATRIX_V8_SEED),
        Config(gc=MATRIX_GC_OPTIONS),
    ]


def cmd_matrix(args: argparse.Namespace) -> int:
    """V8's default pool, a single worker, and eight workers, then the
    stress flags, then the DOM's heap's own stress (`matrix_configs`). This
    is the blocking GC job. The first configuration's time sets how long
    the others may take before they count as hung."""
    configs = matrix_configs()
    if not build_gc_tests():
        print("the GC tests didn't build")
        return 1
    failures = 0
    timeout = None
    for config in configs:
        start = time.monotonic()
        result = run_gc_tests(config, timeout)
        timeout = timeout or timeout_after(time.monotonic() - start)
        status = ("slow" if result.slow else "ok  ") if result.ok else "FAIL"
        print(f"{status} {config.describe():<60} {result.summary} ({time.monotonic() - start:.0f}s)")
        if not result.ok:
            failures += 1
            print(failure_excerpt(result.output))
            print(f"  reproduce: {config.repro()}")
    print(f"\n{len(configs) - failures}/{len(configs)} configurations passed")
    return 1 if failures else 0


# --- fuzz -------------------------------------------------------------------

# V8 GC flags worth mixing in, all recognized by the pinned V8. Flags in the
# same inner list contradict each other, so at most one of them is picked.
FUZZ_FLAGS: list[list[str]] = [
    ["--stress-incremental-marking"],
    ["--no-cppheap-incremental-marking"],  # atomic: the gc heap marks in the pause
    ["--no-incremental-marking"],
    ["--no-concurrent-marking"],
    ["--no-parallel-marking"],
    ["--single-threaded-gc"],
    ["--no-parallel-pause-for-gc-in-background"],
    ["--no-concurrent-sweeping"],
    ["--no-parallel-compaction"],
    ["--stress-compaction"],
    ["--stress-per-context-marking-worklist"],
    ["--no-memory-reducer"],
    ["--minor-ms", "--no-minor-ms"],
    # GCs at the JS heap's allocation counts. They need
    # `V8_ENABLE_ALLOCATION_TIMEOUT`, which our debug V8 has (gn's
    # `v8_enable_test_features` follows `is_debug`); the release one ignores
    # them.
    ["--random-gc-interval=500", "--gc-interval=200"],
    # Not `--verify-heap`: it checks only V8's own JS objects, which `dom`
    # never writes but through V8's API, and it verifies every new `Global`
    # by making handles, so it aborts where `dom` makes one with no
    # `HandleScope` open (which V8's API allows).
]


# The DOM's heap's options worth mixing in, each drawn on its own: its
# stress mode and verifier, and a nursery of a few sizes (in whole pages;
# none, the renderer's, half the time).
FUZZ_GC_NURSERIES = [256 << 10, 1 << 20, 4 << 20]


def random_gc_options(rng: random.Random) -> list[str]:
    options = [option for option in ("stress", "verify") if rng.random() < 0.3]
    if rng.random() < 0.5:
        options.append(f"nursery={rng.choice(FUZZ_GC_NURSERIES)}")
    return options


def random_config(rng: random.Random, max_flags: int) -> Config:
    groups = rng.sample(FUZZ_FLAGS, rng.randint(0, max_flags))
    config = Config(
        pool=rng.randint(1, 8),
        seed=rng.getrandbits(32),
        flags=sorted(rng.choice(group) for group in groups),
        gc=random_gc_options(rng),
    )
    # V8's flag is an `int`.
    config.v8_seed = rng.randint(1, 2**31 - 1)
    return config


def cmd_fuzz(args: argparse.Namespace) -> int:
    """Like V8's numfuzz: rerun the GC tests with a random test seed, thread
    pool size and set of GC flags, and record every
    configuration that fails. Not meant to block merges.

    First (after `ci.py third-party`, as every command here) the baseline, the tests under V8's defaults: if it fails, every
    configuration would fail the same way, so the fuzzer stops before it
    starts. Its time sets how long a configuration may take before it counts
    as hung (the stacks of its threads are recorded, then it's killed), if
    it has stopped using the CPU by then; one that keeps busy is waited for,
    up to a few times that, and is slow if it gets to the end: not a
    failure, but recorded with the failures, marked so (`run_gc_tests`). The
    fuzzer also stops when the same failure comes back in several
    configurations in a row, which a bug that depends on the configuration
    rarely does.

    Each failing configuration runs again (`--reruns`), and how often the
    same failure comes back is recorded; its whole output is kept in a
    directory named after `--out`, with the stacks of a crashed test binary
    run again under gdb or lldb, while the binaries are the ones that ran."""
    fuzz_seed = args.seed if args.seed is not None else random.getrandbits(32)
    print(f"fuzz seed: 0x{fuzz_seed:08x} (rerun this sequence with --seed 0x{fuzz_seed:08x})")
    revs = revisions()
    print(f"built from: {', '.join(f'{name} {rev}' for name, rev in revs.items())}", flush=True)
    rng = random.Random(fuzz_seed)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    logs = out.with_suffix("")

    if not build_gc_tests():
        print("the GC tests didn't build")
        return 1
    start = time.monotonic()
    baseline = run_gc_tests(Config(), args.timeout * 60 if args.timeout else None)
    took = time.monotonic() - start
    if not baseline.ok:
        print(f"baseline FAIL {baseline.summary}")
        print(failure_excerpt(baseline.output))
        print(f"  reproduce: {Config().repro()}")
        print("the tests fail without any configuration: fix that first, or every configuration fails the same way")
        return 1
    timeout = args.timeout * 60 if args.timeout else timeout_after(took)
    print(f"baseline ok   {baseline.summary} ({took:.0f}s); a configuration is hung after {minutes(timeout)}")
    deadline = time.monotonic() + args.minutes * 60 if args.minutes else None

    runs = failures = slow = 0
    first_seen: dict[str, int] = {}  # failure signature -> the run it first failed
    repeated = (None, 0)  # the last failure's signature and how many runs in a row had it
    while runs < args.iterations and (deadline is None or time.monotonic() < deadline):
        config = random_config(rng, args.max_flags)
        runs += 1
        start = time.monotonic()
        result = run_gc_tests(config, timeout)
        status = ("slow" if result.slow else "ok  ") if result.ok else "FAIL"
        print(f"[{runs}] {status} {config.describe()}  -> {result.summary} ({time.monotonic() - start:.0f}s)", flush=True)
        if result.ok:
            repeated = (None, 0)
            if result.slow:
                # Not a failure, but worth knowing which configurations get
                # this slow: they come close to looking hung.
                slow += 1
                with out.open("a", encoding="utf-8", newline="\n") as f:
                    record = {
                        "fuzz_seed": f"0x{fuzz_seed:08x}",
                        "run": runs,
                        "revisions": revs,
                        "env": config.env(),
                        "slow": True,
                        "summary": result.summary,
                        "reproduce": config.repro(),
                        "stacks": result.stacks,
                    }
                    f.write(json.dumps(record) + "\n")
            continue
        failures += 1
        first = first_seen.setdefault(result.signature, runs)
        if first == runs:
            print(failure_excerpt(result.output))
        else:
            print(f"  the same failure as [{first}]: {result.signature}")
        print(f"  reproduce: {config.repro()}")
        logs.mkdir(parents=True, exist_ok=True)
        log = logs / f"{fuzz_seed:08x}-{runs}.log"
        debugged = debug_crashes(config, result.output, timeout)
        log.write_text("\n".join(filter(None, [result.output, result.stacks, debugged])), encoding="utf-8", newline="\n")
        print(f"  output: {log}")
        again = [run_gc_tests(config, timeout).signature for _ in range(args.reruns)]
        reproduced = f"{again.count(result.signature)}/{len(again)}"
        if again:
            print(f"  failed the same way in {reproduced} reruns", flush=True)
        with out.open("a", encoding="utf-8", newline="\n") as f:
            record = {
                "fuzz_seed": f"0x{fuzz_seed:08x}",
                "run": runs,
                "revisions": revs,
                "env": config.env(),
                "summary": result.summary,
                "signature": result.signature,
                "reproduce": config.repro(),
                "excerpt": failure_excerpt(result.output),
                "crash_stack": crash_stack(debugged),
                "reproduced": reproduced,
                "other_failures": sorted({sig for sig in again if sig and sig != result.signature}),
                "output": str(log),
            }
            if result.stacks:
                record["stacks"] = result.stacks
            f.write(json.dumps(record) + "\n")
        signature, count = repeated
        repeated = (result.signature, count + 1 if signature == result.signature else 1)
        if repeated[1] >= args.stop_after:
            print(
                f"stopping: the same failure in {repeated[1]} configurations in a row, so it likely doesn't "
                "depend on the configuration; reproduce it with the baseline's environment"
            )
            break
    print(f"\n{runs - failures}/{runs} configurations passed" + (f", {slow} of them slowly" if slow else ""))
    if failures or slow:
        print(f"failing and slow configurations appended to {out}")
    return 1 if failures else 0


# --- automation ---------------------------------------------------------------

SCENARIOS = ROOT / "tests" / "scenarios"
AUTOMATION_RESULTS = ROOT / "target" / "automation-results"


def cmd_automation(args: argparse.Namespace) -> int:
    """Each scenario in its own `machopo --automation` process (a window can be
    opened once per process), with the system's keyboard shortcuts; then the
    pipe's test that drives the window. Needs a display and a GPU."""
    build = subprocess.run(["cargo", "build", "-p", "shell", "--bin", "machopo"], cwd=ROOT)
    if build.returncode != 0:
        return build.returncode
    # The display stays on while this runs (macOS): once it sleeps, the
    # window is occluded, Viz draws nothing, and a capture waits for a frame
    # that never comes. `caffeinate -i` alone keeps only the system awake.
    if sys.platform == "darwin":
        subprocess.Popen(["caffeinate", "-d", "-i", "-w", str(os.getpid())])
    machopo = ROOT / "target" / "debug" / ("machopo.exe" if os.name == "nt" else "machopo")
    # The list of those known to fail, not a scenario.
    scenarios = sorted(s for s in SCENARIOS.glob("*.txt") if s.stem != "known-failures")
    known = known_failing_scenarios()
    if args.filter:
        scenarios = [s for s in scenarios if any(f in s.stem for f in args.filter)]
    failures = 0
    for scenario in scenarios:
        out = AUTOMATION_RESULTS / scenario.stem
        shutil.rmtree(out, ignore_errors=True)
        start = time.monotonic()
        result = subprocess.run(
            [str(machopo), "--automation", str(scenario), "--out", str(out)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        took = f"({time.monotonic() - start:.0f}s)"
        if result.returncode == 0:
            if scenario.stem in known:
                failures += 1
                print(f"PASS {scenario.stem} {took}: known-failures.txt names it, take it off")
            else:
                print(f"ok   {scenario.stem} {took}")
            continue
        if scenario.stem in known:
            print(f"known failure {scenario.stem} {took}")
            continue
        failures += 1
        print(f"FAIL {scenario.stem} {took}")
        # The failure is the last error line; the log has the rest.
        lines = re.sub(r"\x1b\[[0-9;]*m", "", result.stdout + result.stderr).splitlines()
        errors = [line for line in lines if " ERROR " in line or line.startswith("machopo:")]
        print("  " + (errors[-1].strip() if errors else f"exit status {result.returncode}"))
    print(f"\n{len(scenarios) - failures}/{len(scenarios)} scenarios passed; screenshots in {AUTOMATION_RESULTS}")
    # `machopo --automation-pipe --window`: the pipe's test that needs the window.
    if not args.filter or any(f in "pipe" for f in args.filter):
        start = time.monotonic()
        pipe = subprocess.run(
            ["cargo", "test", "-p", "shell", "--test", "it", "--", "--exact", WINDOW_PIPE_TEST],
            cwd=ROOT,
            env={**os.environ, "MACHOPO_WINDOW_TESTS": "1"},
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        took = f"({time.monotonic() - start:.0f}s)"
        if pipe.returncode == 0 and "1 passed" in pipe.stdout:
            print(f"ok   pipe --window {took}")
        else:
            failures += 1
            print(f"FAIL pipe --window {took}\n{pipe.stdout}{pipe.stderr}")
    return 1 if failures else 0


def known_failing_scenarios() -> set[str]:
    """The scenarios `tests/scenarios/known-failures.txt` names: one a
    line, `#` comments."""
    text = (SCENARIOS / "known-failures.txt").read_text(encoding="utf-8")
    return {name for line in text.splitlines() if (name := line.split("#", 1)[0].strip())}


# The test in crates/shell/tests/it/automation_pipe.rs that drives the real
# window, which runs only with `MACHOPO_WINDOW_TESTS`.
WINDOW_PIPE_TEST = "automation_pipe::the_real_window_is_driven_the_same"


# --- scenarios ---------------------------------------------------------------

# The part of `cargo test` that skips itself without `MACHOPO_SLOW_TESTS` (the
# scenarios): it would take most of a `cargo test --workspace`, so it runs
# here, when what it covers changes.
SLOW_TESTS_ENV = "MACHOPO_SLOW_TESTS"
SKIPPED = f"skipped without {SLOW_TESTS_ENV}"


def run_slow_test(target: str, filters: list[str], env: dict[str, str] | None = None) -> int:
    """`cargo test -p shell --test <target>` with `MACHOPO_SLOW_TESTS` set
    (and `env`), passing its output through. Fails if the test still says
    it skipped, so a run that tested nothing can't pass."""
    cmd = ["cargo", "test", "-p", "shell", "--test", target]
    if filters:
        cmd += ["--", *filters]
    proc = subprocess.Popen(
        cmd,
        cwd=ROOT,
        env={**os.environ, SLOW_TESTS_ENV: "1", **(env or {})},
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    skipped = False
    for line in proc.stdout:
        print(line, end="", flush=True)
        skipped |= SKIPPED in line
    status = proc.wait()
    if skipped:
        print(f"{target} skipped although {SLOW_TESTS_ENV} was set")
        return 1
    return status


def cmd_scenarios(args: argparse.Namespace) -> int:
    """The scenarios under `tests/scenarios/`, in a browser window without
    the platform's window (shell's `scenarios` test), four at a time: those
    that press keys with macOS's keyboard shortcuts and with the others',
    the rest once. In real time: about as long as the longest one.
    Screenshots go to `target/scenario-results/`. Those
    `tests/scenarios/known-failures.txt` names may fail; one of them that
    passes fails the run (to be taken off)."""
    return run_slow_test("scenarios", args.filter, None)


# --- WPT through wptrunner --------------------------------------------------

WPT = ROOT.parent / "third_party" / "wpt"
WPTRUNNER = ROOT / "target" / "wptrunner"
EXE = ".exe" if sys.platform == "win32" else ""
# What wptrunner needs (for `wpt`, `reftest` and `wdspec`): its own packages, pytest, and the
# manifest's (PyYAML, for test262's metadata).
WPTRUNNER_REQUIREMENTS = [
    WPT / "tools" / "wptrunner" / "requirements.txt",
    WPT / "tools" / "requirements_pytest.txt",
    WPT / "tools" / "wpt" / "requirements.txt",
]
PRODUCT = ROOT / "tests" / "wpt" / "wptrunner_machopo"


def wptrunner_python() -> Path | None:
    """The venv's Python, with wptrunner's requirements and machopo's product
    installed: the venv is made the first time (which downloads the
    packages), and brought up to date when the requirements or the
    product change. The product is installed from a copy under
    `target/`, so the build leaves nothing in the repo."""
    venv = WPTRUNNER / "venv"
    python = venv / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    if not python.exists():
        print(f"making {venv}", flush=True)
        if subprocess.run([sys.executable, "-m", "venv", str(venv)]).returncode != 0:
            return None
    requirements = "".join(path.read_text(encoding="utf-8") for path in WPTRUNNER_REQUIREMENTS)
    stamp = venv / "machopo-requirements.txt"
    if not stamp.exists() or stamp.read_text(encoding="utf-8") != requirements:
        cmd = [str(python), "-m", "pip", "install", "--quiet"]
        for path in WPTRUNNER_REQUIREMENTS:
            cmd += ["-r", str(path)]
        if subprocess.run(cmd).returncode != 0:
            return None
        stamp.write_text(requirements, encoding="utf-8", newline="\n")
    product = "".join(
        f"{path.relative_to(PRODUCT)}\n{path.read_text(encoding='utf-8')}"
        for path in sorted(PRODUCT.rglob("*"))
        if path.is_file() and "__pycache__" not in path.parts
    )
    product_stamp = venv / "machopo-product.txt"
    if not product_stamp.exists() or product_stamp.read_text(encoding="utf-8") != product:
        copy = WPTRUNNER / "product"
        shutil.rmtree(copy, ignore_errors=True)
        shutil.copytree(PRODUCT, copy, ignore=shutil.ignore_patterns("__pycache__"))
        cmd = [str(python), "-m", "pip", "install", "--quiet", "--no-deps", "--force-reinstall", str(copy)]
        if subprocess.run(cmd).returncode != 0:
            return None
        product_stamp.write_text(product, encoding="utf-8", newline="\n")
    return python


def checkout_untouched() -> bool:
    """Whether the WPT checkout is as it was: nothing written into it,
    ignored files (bytecode) included."""
    status = subprocess.run(
        ["git", "-C", str(WPT), "status", "--porcelain", "--ignored"],
        capture_output=True,
        text=True,
    ).stdout
    if status:
        print(f"the run wrote into {WPT}:\n{status}")
    return not status


@dataclass
class TestRoot:
    """A tree of tests wptrunner runs: WPT's checkout at `/`, or one of our
    own beside it (machopo's reftests at `/_machopo/`, as Servo's and Firefox's at
    `/_mozilla/`), with its expectations (`.ini` files, at the tests'
    paths)."""

    url_base: str
    tests: Path
    metadata: Path

    def url(self, path: str) -> str:
        """`path` (under the root) as a URL path."""
        return self.url_base + path.lstrip("/")


WPT_ROOT = TestRoot("/", WPT, ROOT / "tests" / "wpt" / "metadata")
MACHOPO_REFTESTS = TestRoot("/_machopo/", ROOT / "tests" / "reftests", ROOT / "tests" / "reftests" / "metadata")


def wptrunner_include(path: str, root: TestRoot) -> str:
    """A line of our include lists (a path under the root) as wptrunner's
    include takes it. It reads each as a path from the current directory
    first, and takes every test in a file or a directory found there, by
    the manifest; what isn't found it matches against the tests' URLs. Our
    lists name files, and a `.any.js` or `.window.js` file isn't a URL
    (`x.any.js` runs as `x.any.html`, `x.any.worker.html`...), so each
    becomes its path; one that isn't there stays a URL."""
    path = path.lstrip("/")
    local = root.tests / path
    return str(local) if path and local.exists() else root.url(path)


def stale_wpt_entries(include: list[str], root: TestRoot) -> list[str]:
    """What no longer fits the root's tests, which wptrunner passes over in
    silence: an include line naming no file or directory there, an `.ini`
    for a test file that isn't there (a `__dir__.ini`, for a directory).
    Each is a test removed or renamed (upstream, for WPT), or a typo."""
    stale = []
    for line in include:
        path = line.split("#", 1)[0].strip().lstrip("/")
        if path and not (root.tests / path).exists():
            stale.append(f"include line `{path}`: not in {root.tests}")
    if root.metadata.exists():
        for ini in sorted(root.metadata.rglob("*.ini")):
            rel = ini.relative_to(root.metadata)
            test = rel.parent if ini.name == "__dir__.ini" else rel.with_suffix("")
            if not (root.tests / test).exists():
                stale.append(f"{ini.relative_to(ROOT).as_posix()}: no {test.as_posix()} in {root.tests}")
    return stale


def started_tests(raw_logs: list[Path]) -> set[str]:
    """The ids of the tests that started in wptrunner's raw logs (each test,
    `disabled` ones too, starts; a log a chunk of the run), without their
    variants' queries."""
    ids = set()
    for raw_log in raw_logs:
        with open(raw_log, encoding="utf-8") as log:
            for line in log:
                entry = json.loads(line)
                if entry["action"] == "test_start":
                    ids.add(entry["test"].split("?", 1)[0])
    return ids


def unmatched_includes(include: list[str], raw_logs: list[Path], root: TestRoot) -> list[str]:
    """The include lines that no test of the run came from: wptrunner
    takes a line it can't place as a URL prefix that matches nothing, and
    the run passes without the tests. A test's id is its URL: a
    directory's tests are under it, a `.any.js` file's are `x.any.html`,
    `x.any.worker.html` and so on, a `.window.js` file's `x.window.html`,
    and any test's variants add a query."""
    ids = started_tests(raw_logs)
    unmatched = []
    for line in include:
        path = line.split("#", 1)[0].strip().lstrip("/")
        if not path:
            # The whole of a root of ours (`""`); a blank line of WPT's list.
            if root is not WPT_ROOT and not any(test.startswith(root.url_base) for test in ids):
                unmatched.append(root.url_base)
            continue
        url = root.url(path)
        if path.endswith("/") or (root.tests / path).is_dir():
            found = any(test.startswith(url.rstrip("/") + "/") for test in ids)
        elif path.endswith(".any.js"):
            found = any(test.startswith(url.removesuffix("js")) for test in ids)
        elif path.endswith((".window.js", ".worker.js")):
            found = f"{url.removesuffix('.js')}.html" in ids
        else:
            found = url in ids
        if not found:
            unmatched.append(path)
    return unmatched


def test_file(test_id: str, root: TestRoot) -> str | None:
    """The file under the root a test's id (its URL) came from, or None for
    another root's test: a `.any.js` file's globals are `x.any.html`,
    `x.any.worker.html` and so on, a `.window.js` file's is
    `x.window.html`, a `.worker.js` file's `x.worker.html`; a variant adds
    a query."""
    path = test_id.split("?", 1)[0]
    if root.url_base != "/":
        if not path.startswith(root.url_base):
            return None
        path = path[len(root.url_base) :]
    path = path.lstrip("/")
    if ".any." in path:
        return path[: path.index(".any.")] + ".any.js"
    for kind in ("window", "worker"):
        if path.endswith(f".{kind}.html"):
            return path.removesuffix(".html") + ".js"
    return path


def unrun_expectations(include: list[str], raw_logs: list[Path], root: TestRoot) -> list[str]:
    """The root's `.ini` files under the include lines whose test file no
    test of the run came from: a kind of test that didn't run (wptrunner
    runs only the types it's given), or one wptrunner no longer finds.
    `__dir__.ini` files are left out."""
    files = {test_file(test, root) for test in started_tests(raw_logs)}
    scope = [line.split("#", 1)[0].strip().lstrip("/") for line in include]
    # `""` is the whole of a root of ours; in WPT's lists, a blank line.
    scope = [path for path in scope if path or root is not WPT_ROOT]
    unrun = []
    if not root.metadata.exists():
        return unrun
    for ini in sorted(root.metadata.rglob("*.ini")):
        if ini.name == "__dir__.ini":
            continue
        test = ini.relative_to(root.metadata).with_suffix("").as_posix()
        in_scope = any(path == "" or test == path or test.startswith(path.rstrip("/") + "/") for path in scope)
        if in_scope and test not in files:
            unrun.append(ini.relative_to(ROOT).as_posix())
    return unrun


# A value of an `.ini` that lists statuses (`expected: [OK, ERROR]`, or a
# condition's: `if os == "win": [PASS, FAIL]`): the first is the one
# expected, the others known intermittent ones. A section heading (a test's
# or a subtest's name) starts with `[` itself.
STATUS_LIST = re.compile(r"^\s*(?:expected|if .*):\s*\[")


def uncommented_intermittents(root: TestRoot) -> list[str]:
    """The root's `.ini` files that list known intermittent statuses
    without a `#` comment: a test marked unstable says what was found of
    why (or where that is looked into), so that the list doesn't become a
    way to stop looking."""
    bare = []
    if not root.metadata.exists():
        return bare
    for ini in sorted(root.metadata.rglob("*.ini")):
        lines = ini.read_text(encoding="utf-8").splitlines()
        if any(STATUS_LIST.match(line) for line in lines) and not any(line.lstrip().startswith("#") for line in lines):
            bare.append(ini.relative_to(ROOT).as_posix())
    return bare


def intermittent_results(raw_logs: list[Path]) -> list[str]:
    """The tests of the run (its chunks' logs) that took one of their known
    intermittent statuses, not the first one their expectations list:
    wptrunner counts those as expected and says nothing of them. A line a
    test: its own status if it was one, and how many of its subtests' were."""
    harness: dict[str, str] = {}
    subtests: dict[str, int] = {}
    for raw_log in raw_logs:
        with open(raw_log, encoding="utf-8") as log:
            for line in log:
                entry = json.loads(line)
                if entry["action"] not in ("test_status", "test_end"):
                    continue
                # The list holds the other statuses, not the first: a result
                # in it is one of those, and mozlog keeps its `expected`.
                if entry["status"] not in (entry.get("known_intermittent") or []):
                    continue
                if entry["action"] == "test_end":
                    harness[entry["test"]] = f"{entry['status']} (expected {entry['expected']})"
                else:
                    subtests[entry["test"]] = subtests.get(entry["test"], 0) + 1
    lines = []
    for test in sorted(harness.keys() | subtests.keys()):
        parts = [harness[test]] if test in harness else []
        if count := subtests.get(test):
            parts.append(f"{count} subtest{'s' if count > 1 else ''}")
        lines.append(f"{test}: {', '.join(parts)}")
    return lines




def wpt_processes(gpu: dict | None, cpus: int) -> list[str]:
    """wptrunner's `--processes` for machopo drawing with `gpu` (as
    `speedometer.machopo_gpu` tells) on `cpus` CPUs: its own default with a
    GPU; on the CPU adapter (no GPU, or `MACHOPO_GPU_ADAPTER=cpu`) a quarter of
    the CPUs, at least 1, as every process then draws on the CPU too (32
    at once took a frame from 23 ms to 466)."""
    if gpu is None or gpu["type"] != "Cpu":
        return []
    return ["--processes", str(max(1, cpus // 4))]


def processes_for_gpu(machopo: Path) -> list[str]:
    """`wpt_processes` for `machopo` on this machine, saying which GPU it
    draws with, and warning when that's the CPU adapter."""
    sys.path.insert(0, str(ROOT / "scripts" / "bench" / "speedometer"))
    import speedometer

    gpu = speedometer.machopo_gpu(machopo)
    print(f"machopo draws with: {speedometer.describe_gpu(gpu)}")
    processes = wpt_processes(gpu, os.cpu_count() or 1)
    if processes:
        print(
            "warning: machopo draws on the CPU adapter, not a GPU (none, or MACHOPO_GPU_ADAPTER=cpu): "
            f"{processes[1]} tests at a time, and they are much slower"
        )
    return processes


@dataclass
class Build:
    """Which `machopo` and `machopodriver` the wptrunner commands run: those
    of a cargo profile (`dev`, or `ci`, release with the debug build's
    checks, as the nightly CI runs them), built first unless `build` is off
    (binaries built elsewhere, copied into the profile's directory)."""

    profile: str = "dev"
    build: bool = True

    def binaries(self) -> Path:
        # Cargo's directory for a profile is its name, but `dev`'s.
        return ROOT / "target" / ("debug" if self.profile == "dev" else self.profile)

    @staticmethod
    def of(args: argparse.Namespace) -> Build:
        return Build(args.profile, not args.no_build)


def run_wptrunner(
    out: str,
    test_types: list[str],
    include: list[str] | None,
    tests: list[str],
    extra: list[str],
    build: Build,
    root: TestRoot = WPT_ROOT,
    chunk: tuple[int, int] | None = None,
) -> int:
    """`python -m wptrunner_machopo.run` in the venv, against the `machopo` and
    `machopodriver` of `build`'s profile, built first unless it says not to:
    the tests of `root` (WPT's, or one of ours
    beside it), its expectations, the manifests shared by every command
    under `target/wptrunner/`, the logs and the wptreport under
    `target/<out>/`. `include` is the lines of an include list (paths under
    the root, `#` comments; `""` is all of it); `tests`, paths given on the
    command line, run instead. machopo and machopodriver log only warnings
    (`RUST_LOG`, unless set): wptrunner prints what its browser writes.
    `MACHOPO_UPDATE_EXPECTATIONS` writes the results as the expectations,
    `MACHOPO_WPT_TIMEOUT_MULTIPLIER` gives each test that many times as long.
    Fails if anything was written into the checkout; first if an include
    line or an expectation names a test that isn't there, or an `.ini`
    lists known intermittent statuses without a comment; and after a run
    of the include list, if a line brought no test or an expectation's test
    didn't run. Names the tests that took a known intermittent status.
    `chunk`, (I, N), runs the I-th of N chunks of the tests (wptrunner's,
    by directory): then the checks of the whole list are left to
    `wpt-check`, over every chunk's log."""
    if stale := stale_wpt_entries(include or [], root):
        print(f"these no longer fit {root.tests} (see tests/wpt/README.md, Updating):")
        for entry in stale:
            print(f"  {entry}")
        return 1
    if bare := uncommented_intermittents(root):
        print("these list known intermittent statuses without a `#` comment on why (see tests/wpt/README.md):")
        for path in bare:
            print(f"  {path}")
        return 1
    binaries = build.binaries()
    if build.build:
        cargo = ["cargo", "build", "--profile", build.profile]
        cargo += ["-p", "shell", "--bin", "machopo", "-p", "driver", "--bin", "machopodriver"]
        if subprocess.run(cargo, cwd=ROOT).returncode != 0:
            return 1
    else:
        missing = [name for name in ("machopo", "machopodriver") if not (binaries / f"{name}{EXE}").is_file()]
        if missing:
            print(f"--no-build, but {binaries} has no {' or '.join(missing)}")
            return 1
    python = wptrunner_python()
    if python is None:
        return 1
    out_dir = ROOT / "target" / out
    out_dir.mkdir(parents=True, exist_ok=True)
    if "--processes" not in extra:
        extra = [*extra, *processes_for_gpu(binaries / f"machopo{EXE}")]
    cmd = [
        str(python),
        "-m",
        "wptrunner_machopo.run",
        "--wpt", str(WPT),
        "--out", str(out_dir),
        "--metadata", str(WPT_ROOT.metadata),
        "--manifest", str(WPTRUNNER / "MANIFEST.json"),
        "--machopo", str(binaries / f"machopo{EXE}"),
        "--machopodriver", str(binaries / f"machopodriver{EXE}"),
        *extra,
    ]
    if root is not WPT_ROOT:
        root.metadata.mkdir(parents=True, exist_ok=True)
        cmd += ["--root", root.url_base, str(root.tests), str(root.metadata)]
    if tests:
        cmd += [wptrunner_include(test, root) for test in tests]
    elif include is not None:
        include_file = out_dir / "include.txt"
        paths = [line.split("#", 1)[0].strip() for line in include]
        include_file.write_text(
            "".join(f"{wptrunner_include(path, root)}\n" for path in paths if path or root is not WPT_ROOT),
            encoding="utf-8",
            newline="\n",
        )
        cmd += ["--include-file", str(include_file)]
    if chunk:
        cmd += ["--this-chunk", str(chunk[0]), "--total-chunks", str(chunk[1])]
    # The tests first: `--test-types` takes a list.
    cmd += ["--test-types", *test_types]
    if os.environ.get("MACHOPO_UPDATE_EXPECTATIONS"):
        cmd.append("--update")
    if multiplier := os.environ.get("MACHOPO_WPT_TIMEOUT_MULTIPLIER"):
        cmd += ["--timeout-multiplier", multiplier]
    env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
    env.setdefault("RUST_LOG", "warn")
    status = subprocess.run(cmd, cwd=WPTRUNNER, env=env).returncode
    if not checkout_untouched():
        return 1
    raw_logs = [out_dir / "raw.log"] if (out_dir / "raw.log").exists() else []
    print_intermittent_results(raw_logs)
    if include is not None and not tests:
        if chunk:
            print(f"chunk {chunk[0]} of {chunk[1]}: `wpt-check` checks the include list against every chunk's log")
        elif include_list_unmet(include, raw_logs, root):
            return 1
    return status


def print_intermittent_results(raw_logs: list[Path]) -> None:
    if intermittent := intermittent_results(raw_logs):
        print("these took a known intermittent status, which their expectations list:")
        for line in intermittent:
            print(f"  {line}")


def include_list_unmet(include: list[str], raw_logs: list[Path], root: TestRoot) -> bool:
    """Whether a run of the include list (its chunks' logs) left a line
    without a test, or an expectation's test unrun, saying which."""
    if unmatched := unmatched_includes(include, raw_logs, root):
        print("no test ran for these include lines:")
        for path in unmatched:
            print(f"  {path}")
        return True
    if unrun := unrun_expectations(include, raw_logs, root):
        print("these expectations are for tests that didn't run:")
        for path in unrun:
            print(f"  {path}")
        return True
    return False


def include_list(name: str) -> list[str]:
    return (ROOT / "tests" / "wpt" / name).read_text(encoding="utf-8").splitlines()


def listed_paths(tests: list[str], include: list[str], name: str) -> list[str]:
    """The TEST paths given, narrowed to what the include list `name` runs:
    its lines under a path given, or the path itself when it is under one
    of its lines. A directory holds tests the list leaves out (they need
    what machopo lacks, and have no expectations), so running it whole reports
    them all as unexpected. What the list doesn't reach is named, and
    `--all` runs it anyway."""

    def under(path: str, base: str) -> bool:
        path, base = path.strip("/"), base.strip("/")
        return path == base or path.startswith(base + "/")

    lines = [line.split("#", 1)[0].strip() for line in include]
    lines = [line for line in lines if line]
    run: list[str] = []
    for test in tests:
        found = [line for line in lines if under(line, test)]
        if not found and any(under(test, line) for line in lines):
            found = [test]
        if not found:
            print(f"{test}: not in tests/wpt/{name} (--all runs it anyway)")
        run += [path for path in found if path not in run]
    return run


def cmd_wpt(args: argparse.Namespace) -> int:
    """The WPT tests `tests/wpt/include.txt` names (testharness.js tests,
    reftests and crashtests), or those of them under the TEST paths given
    (`--all`: the paths whole, to try tests out for the list), run by upstream's
    wptrunner against machopodriver and machopo, compared with their expectations
    in `tests/wpt/metadata/` (wptrunner's `.ini` files: a test without one
    passes whole; a skipped one is `disabled`, with the reason; a list,
    `[OK, ERROR]`, is the expected status and known intermittent ones,
    named at the end when they come). A failing
    reftest's screenshots go to `target/wpt-results/screenshots/`
    (`-actual.png`, `-expected.png`, `-diff.png`). The first
    run makes a venv under `target/wptrunner/` (downloading wptrunner's
    packages) and builds WPT's manifest there (about a minute, again
    when the checkout's commit changes). The logs and the wptreport go to
    `target/wpt-results/`. wptserve's ports are fixed: one run at a time,
    and not with `wdspec` or `reftest`. Set `MACHOPO_UPDATE_EXPECTATIONS=1` to
    write the results as the expected ones, `MACHOPO_WPT_TIMEOUT_MULTIPLIER=N`
    to give each test N times as long."""
    extra = ["--processes", str(args.processes)] if args.processes else []
    include = include_list("include.txt")
    tests = args.tests
    if tests and not args.all:
        tests = listed_paths(tests, include, "include.txt")
        if not tests:
            return 1
    if args.chunk and tests:
        print("--chunk is for a run of the whole list, not of TEST paths")
        return 1
    return run_wptrunner(
        "wpt-results", ["testharness", "reftest", "crashtest"], include, tests, extra, Build.of(args), chunk=args.chunk
    )


def cmd_wpt_check(args: argparse.Namespace) -> int:
    """The checks `wpt` makes after a run of `tests/wpt/include.txt`, over
    the raw logs of a run in chunks (`wpt --chunk I/N`, each chunk's
    `target/wpt-results/raw.log`), which skips them: every log ended (a
    chunk cut off by a cancel or a timeout leaves a log without its end),
    every include line brought a test, every expectation's test ran. Names
    the tests that took a known intermittent status, as `wpt` does."""
    unended = [str(log) for log in args.raw_logs if not run_ended(log)]
    if unended:
        print("these logs are of runs that didn't end:")
        for log in unended:
            print(f"  {log}")
    print_intermittent_results(args.raw_logs)
    unmet = include_list_unmet(include_list("include.txt"), args.raw_logs, WPT_ROOT)
    return 1 if unended or unmet else 0


def run_ended(raw_log: Path) -> bool:
    """Whether wptrunner's raw log has the run's end (`suite_end`)."""
    with open(raw_log, encoding="utf-8") as log:
        return any(json.loads(line)["action"] == "suite_end" for line in log if line.strip())


def chunk_argument(value: str) -> tuple[int, int]:
    """`I/N`: the I-th of N chunks, from 1."""
    try:
        this, total = (int(part) for part in value.split("/"))
    except ValueError:
        raise argparse.ArgumentTypeError(f"{value!r} isn't I/N") from None
    if not 1 <= this <= total:
        raise argparse.ArgumentTypeError(f"{value!r}: I must be from 1 to N")
    return this, total


# Our reftests that don't run in the default subsuite: those at another scale
# run in subsuites of their own (`subsuites.json`). Handed over as paths, as the include's lines: wptrunner
# expands a path to its tests, and a test's own rule wins over its
# directory's, so a URL prefix wouldn't keep out a test a path names.
REFTEST_EXCLUDES = ["hidpi/"]


def cmd_reftest(args: argparse.Namespace) -> int:
    """Our own reftests, under `tests/reftests/`, run by upstream's
    wptrunner as WPT's are, at `/_machopo/` beside WPT (as Servo's and
    Firefox's own tests at `/_mozilla/`), compared with their expectations
    in `tests/reftests/metadata/`; or the TEST paths given (under
    `tests/reftests/`). Those of `hidpi/` run in the subsuites of
    `tests/reftests/subsuites.json`, at their scale. A failing one's
    screenshots go to `target/reftest-results/screenshots/`. WPT's reftests
    run with `wpt`; not at the same time (wptserve's ports are fixed). Set
    `MACHOPO_UPDATE_EXPECTATIONS=1` to write the results as the expected ones."""
    extra = ["--processes", str(args.processes)] if args.processes else []
    extra += ["--subsuite-file", str(MACHOPO_REFTESTS.tests / "subsuites.json")]
    for path in REFTEST_EXCLUDES:
        extra += ["--exclude", wptrunner_include(path, MACHOPO_REFTESTS)]
    return run_wptrunner(
        "reftest-results", ["reftest"], [""], args.tests, extra, Build.of(args), root=MACHOPO_REFTESTS
    )


def cmd_wdspec(args: argparse.Namespace) -> int:
    """WPT's WebDriver tests (`webdriver/tests/classic/`, pytest) that
    `tests/wpt/wdspec-include.txt` names, or those of them under the TEST
    paths given (`--all`: the paths whole), run by
    upstream's wptrunner against machopodriver and machopo, compared with their
    expectations in `tests/wpt/metadata/` (wptrunner's `.ini`
    files: a test without one passes whole). The first run makes a venv
    under `target/wptrunner/` (downloading wptrunner's and pytest's
    packages) and builds WPT's manifest (about a minute); machopo's product
    (`tests/wpt/wptrunner_machopo/`) is installed into it. The logs and the
    wptreport go to `target/wdspec-results/`. wptserve's ports are fixed,
    as `wpt`'s: not at the same time. Set `MACHOPO_UPDATE_EXPECTATIONS=1` to
    write the results as the expected ones."""
    include = include_list("wdspec-include.txt")
    tests = args.tests
    if tests and not args.all:
        tests = listed_paths(tests, include, "wdspec-include.txt")
        if not tests:
            return 1
    return run_wptrunner("wdspec-results", ["wdspec"], include, tests, [], Build.of(args))


# --- mutate -----------------------------------------------------------------

# Where a run keeps the files it mutates, as they were, until it has put them
# back: what `--restore` restores after a run that was killed.
MUTATE_DIR = ROOT / "target" / "mutate"
MUTATE_ORIGINALS = MUTATE_DIR / "originals"


@dataclass
class Mutant:
    name: str
    file: str
    old: str
    new: str
    test: list[str]


def load_mutants(spec: Path) -> tuple[list[Mutant], float]:
    """The mutants a spec file lists, each with its test command, and the
    seconds a run may take. Fails (before anything is touched) on a spec
    that doesn't say what to do, or a `from` that isn't in its file exactly
    once."""
    import tomllib

    data = tomllib.loads(spec.read_text(encoding="utf-8"))
    default_test = data.get("test")
    mutants = []
    for i, m in enumerate(data.get("mutant", [])):
        name = m.get("name", f"#{i + 1}")
        test = m.get("test", default_test)
        fields = {"file": m.get("file"), "from": m.get("from"), "to": m.get("to"), "test": test}
        missing = [key for key, value in fields.items() if value is None]
        if missing:
            raise ValueError(f"{spec}: mutant {name!r} has no {', '.join(missing)}")
        if not (isinstance(test, list) and test and all(isinstance(a, str) for a in test)):
            raise ValueError(f"{spec}: mutant {name!r}: `test` is a command as a list of strings")
        path = ROOT / m["file"]
        text = path.read_text(encoding="utf-8")
        count = text.count(m["from"])
        if count != 1:
            raise ValueError(f"{spec}: mutant {name!r}: its `from` is in {m['file']} {count} times, not once")
        if m["from"] == m["to"]:
            raise ValueError(f"{spec}: mutant {name!r}: `to` is the same as `from`")
        mutants.append(Mutant(name, m["file"], *same_lines(text, m["from"], m["to"]), test))
    if not mutants:
        raise ValueError(f"{spec}: no [[mutant]]")
    return mutants, float(data.get("timeout", 600))


def same_lines(text: str, old: str, new: str) -> tuple[str, str]:
    """`old` and `new` (in `text`), `new` with the line breaks it lacks of
    `old`'s, so that the lines after it keep their numbers: rustc's
    incremental build compiles again every function whose lines moved, as
    their line tables (debug info, panic locations) change, and a file
    whose generic functions every member of the declaration table
    instantiates (`dom_js`'s `templates.rs`) takes some three minutes
    more for a line added or removed than for a line changed (2026-11-04).
    The breaks go where they change no code: after `new` if `old` ends its
    line, before it if `old` starts one; else after the rest of the line
    `old` ends in, which both then take."""
    missing = old.count("\n") - new.count("\n")
    if missing <= 0:
        return old, new
    at = text.index(old)
    end = at + len(old)
    if old.endswith("\n") or text.startswith("\n", end) or end == len(text):
        return old, new + "\n" * missing
    if at == 0 or text[at - 1] == "\n":
        return old, "\n" * missing + new
    line_end = text.find("\n", end)
    rest = text[end:] if line_end == -1 else text[end:line_end]
    return old + rest, new + rest + "\n" * missing


def run_command(command: list[str], timeout: float) -> tuple[str, int | None, float]:
    """Runs `command` from the repo root: its output, its exit status
    (`None`: still running after `timeout` seconds, and killed with every
    process under it), and how long it took."""
    started = time.time()
    proc = subprocess.Popen(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        start_new_session=os.name != "nt",
    )
    try:
        output, _ = proc.communicate(timeout=timeout)
        return output, proc.returncode, time.time() - started
    except subprocess.TimeoutExpired:
        if os.name == "nt":
            # The test binaries cargo started, too.
            subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True)
        else:
            os.killpg(proc.pid, signal.SIGKILL)
        try:
            output, _ = proc.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            # A process that left the tree still holds the output open.
            output = "(its output is lost: a process it started still holds it open)"
        return output, None, time.time() - started


# cargo's report of a crate that doesn't compile (rustc's errors come first).
BUILD_FAILED = re.compile(r"^error: could not compile `", re.M)
# Verus's report of a proof that fails, which cargo reports as a crate that
# doesn't compile: for a mutant of proved code, that is the check catching it.
PROOF_FAILED = re.compile(r"^verification results:: \d+ verified, [1-9]\d* errors", re.M)


def outcome(output: str, status: int | None) -> str:
    """What a mutant's test run says: `caught` (it failed), `missed` (it
    passed), `hung` (killed past the timeout: caught, if slowly) or `build
    failed` (the mutant doesn't compile, which a mutant should). A Verus
    proof that fails, which cargo reports as a crate that doesn't compile,
    is `caught`."""
    if status is None:
        return "hung"
    if status == 0:
        return "missed"
    if PROOF_FAILED.search(output):
        return "caught"
    return "build failed" if BUILD_FAILED.search(output) else "caught"


def write_back(path: Path, data: bytes) -> None:
    """Writes `data` to `path` with a modification time of now: cargo
    rebuilds what it compiled from the mutant, though the bytes are older.
    A Python file's cached bytecode goes too: Python checks it by the
    source's size and whole seconds of its time, and a mutant as long as
    the original, put back within the second its test ran, would match."""
    path.write_bytes(data)
    now = time.time()
    os.utime(path, (now, now))
    if path.suffix == ".py":
        for cached in (path.parent / "__pycache__").glob(f"{path.stem}.*.pyc"):
            cached.unlink(missing_ok=True)


def restore_originals() -> list[str]:
    """Puts back the files an interrupted run left mutated; the ones it
    put back."""
    restored = []
    for saved in sorted(p for p in MUTATE_ORIGINALS.rglob("*") if p.is_file()):
        rel = saved.relative_to(MUTATE_ORIGINALS)
        write_back(ROOT / rel, saved.read_bytes())
        restored.append(rel.as_posix())
    shutil.rmtree(MUTATE_ORIGINALS)
    return restored


def cmd_cdp(args: argparse.Namespace) -> int:
    """Sends the same DevTools commands to machopo and to Chrome for Testing
    (the version `bench.py` pins, downloaded the first time), on the same
    pages, and prints where their answers and events differ: the values that
    differ from run to run (ids, times) are compared as placeholders, and
    the differences known, each with its reason, are left out
    (`scripts/cdp_compare.py`, where the cases are). Builds `machopo` first.
    Not in CI: it needs Chrome. Non-zero if anything else differs."""
    if subprocess.run(["cargo", "build", "-p", "shell", "--bin", "machopo"], cwd=ROOT).returncode != 0:
        return 1
    sys.path.insert(0, str(ROOT / "scripts"))
    import cdp_compare

    unknown = [case for case in args.cases if case not in cdp_compare.CASES]
    if unknown:
        print(f"no such case: {', '.join(unknown)} (the cases: {', '.join(cdp_compare.CASES)})")
        return 2
    log_path = ROOT / "target" / "cdp-compare.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with open(log_path, "w") as log:
        result = cdp_compare.compare(ROOT / "target" / "debug" / "machopo", args.cases, log)
    print(f"machopo's log: {log_path}")
    return result


def cmd_mutate(args: argparse.Namespace) -> int:
    """Mutation testing: breaks the code one way at a time, as a spec file
    lists, and checks that a test fails each time. A spec is TOML:

        # The command each mutant is tested with (from the repo root), unless
        # it names its own; and the seconds a run may take (default 600).
        test = ["cargo", "test", "-q", "-p", "shell", "--test", "it", "script::"]
        timeout = 300

        [[mutant]]
        name = "no time limit"
        file = "crates/shell/src/headless.rs"
        from = '''reftest_waiting = Some((0, Instant::now() + reftest_wait_timeout));'''
        to = '''reftest_waiting = None;'''
        # test = [...]  # this mutant's own command

    Each `from` must be in its file exactly once; `'''` strings take code
    as it is, backslashes and all. A `to` of fewer lines gets empty lines
    for the rest, so that the lines after it keep their numbers (a line
    moved makes rustc's incremental build compile its function again). First every test command runs on the
    code as it is, and must pass: else every mutant would look caught.
    Then each mutant is written, tested and put back (with a new
    modification time, so cargo rebuilds). A mutant is caught when its test
    fails, hung when it runs past the timeout (killed; caught, slowly),
    and a build failure when it doesn't compile (fix the mutant). The run
    stops at the first one missed, as it should be looked at before the
    rest, unless `--keep-going`.

    The files are broken while it runs: don't stage or commit meanwhile.
    Their originals wait in `target/mutate/originals/` until put back; a
    run that was killed leaves them there, and the next refuses to start
    until `--restore` puts them back. Exits non-zero if a mutant was missed,
    didn't build, or the code as it is fails its tests."""
    if args.restore:
        restored = restore_originals() if MUTATE_ORIGINALS.exists() else []
        print("restored: " + (", ".join(restored) if restored else "nothing to restore"))
        return 0
    if args.spec is None:
        print("mutate: a spec file, or --restore")
        return 1
    if MUTATE_ORIGINALS.exists():
        print(
            f"mutate: an earlier run didn't finish: its originals are in {MUTATE_ORIGINALS}. "
            "Check them against your edits, then `test.py mutate --restore` puts them back."
        )
        return 1
    try:
        mutants, timeout = load_mutants(Path(args.spec))
    except (OSError, ValueError) as e:
        print(f"mutate: {e}")
        return 1
    if args.only:
        mutants = [m for m in mutants if any(f in m.name for f in args.only)]
        if not mutants:
            print("mutate: no mutant's name contains any of " + ", ".join(args.only))
            return 1

    commands = list(dict.fromkeys(tuple(m.test) for m in mutants))
    print(f"{len(mutants)} mutants, {len(commands)} test commands. The files are broken while it runs: don't stage or commit.")
    for command in commands:
        output, status, took = run_command(list(command), timeout)
        if status != 0:
            why = "runs past the timeout of" if status is None else "fails"
            print(f"\nthe code as it is {why} `{' '.join(command)}` ({took:.0f} s):\n" + failure_excerpt(output))
            return 1
        print(f"baseline passes: {' '.join(command)} ({took:.0f} s)", flush=True)

    results = []
    try:
        for mutant in mutants:
            path = ROOT / mutant.file
            original = path.read_bytes()
            saved = MUTATE_ORIGINALS / mutant.file
            saved.parent.mkdir(parents=True, exist_ok=True)
            saved.write_bytes(original)
            try:
                text = original.decode("utf-8")
                write_back(path, text.replace(mutant.old, mutant.new).encode("utf-8"))
                output, status, took = run_command(mutant.test, timeout)
            finally:
                write_back(path, original)
                saved.unlink(missing_ok=True)
            result = outcome(output, status)
            results.append((mutant, result))
            print(f"{result}: {mutant.name} ({took:.0f} s)", flush=True)
            if result == "build failed":
                print(failure_excerpt(output, lines=15))
            if result == "missed" and not args.keep_going:
                print("stopped at the first mutant missed (--keep-going runs the rest)")
                break
    finally:
        # Only what is empty: an original that couldn't be put back stays
        # for `--restore`.
        for directory in sorted(MUTATE_ORIGINALS.rglob("*"), reverse=True):
            if directory.is_dir() and not any(directory.iterdir()):
                directory.rmdir()
        if MUTATE_ORIGINALS.exists() and not any(MUTATE_ORIGINALS.iterdir()):
            MUTATE_ORIGINALS.rmdir()

    counts = {r: sum(1 for _, x in results if x == r) for r in ("caught", "hung", "missed", "build failed")}
    print(f"\n{len(results)} of {len(mutants)} run: " + ", ".join(f"{n} {r}" for r, n in counts.items() if n))
    for mutant, result in results:
        if result in ("missed", "build failed"):
            print(f"  {result}: {mutant.name} ({mutant.file})")
    return 0 if counts["missed"] == 0 and counts["build failed"] == 0 and len(results) == len(mutants) else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True, metavar="command")

    def command(name: str, func, help: str) -> argparse.ArgumentParser:
        """A subcommand, described by its function's docstring in its `-h`."""
        parser = sub.add_parser(
            name,
            help=help,
            description=inspect.cleandoc(func.__doc__),
            formatter_class=argparse.RawDescriptionHelpFormatter,
        )
        parser.set_defaults(func=func)
        return parser

    def build_arguments(parser: argparse.ArgumentParser) -> None:
        """The wptrunner commands' choice of machopo and machopodriver."""
        parser.add_argument(
            "--profile",
            default="dev",
            metavar="NAME",
            help="the cargo profile to build and run them with (default: dev; the nightly CI's: ci)",
        )
        parser.add_argument(
            "--no-build",
            action="store_true",
            help="run those already in the profile's target directory (built elsewhere), without cargo",
        )

    command("matrix", cmd_matrix, "GC tests under each pool size, then under fixed stress flags")

    fuzz = command("fuzz", cmd_fuzz, "GC tests under random configurations (numfuzz-style)")
    fuzz.add_argument(
        "--iterations", type=int, default=20, metavar="N", help="configurations to try (default 20)"
    )
    fuzz.add_argument(
        "--minutes",
        type=float,
        metavar="M",
        help="stop after this many minutes, even with iterations left (default: no limit)",
    )
    def seed(text: str) -> int:
        """An integer in decimal or `0x` hex; argparse names it in errors."""
        return int(text, 0)

    fuzz.add_argument(
        "--seed",
        type=seed,
        metavar="S",
        help="the fuzzer's seed (decimal or 0x...), to replay the sequence of configurations "
        "a run printed at its start (default: a random one)",
    )
    fuzz.add_argument(
        "--max-flags", type=int, default=3, metavar="N", help="most V8 GC flags per configuration (default 3)"
    )
    fuzz.add_argument(
        "--timeout",
        type=float,
        metavar="M",
        help="minutes a configuration may run before it counts as hung, and is killed after its "
        f"threads' stacks are recorded (default: {TIMEOUT_FACTOR} times the baseline, at least "
        f"{MIN_TIMEOUT // 60}); one still using the CPU then is waited for, up to "
        f"{SLOW_LIMIT_FACTOR} times that, and counts as slow, not failed, if it gets to the end",
    )
    fuzz.add_argument(
        "--stop-after",
        type=int,
        default=3,
        metavar="N",
        help="stop when the same failure comes back in N configurations in a row (default 3)",
    )
    fuzz.add_argument(
        "--reruns",
        type=int,
        default=2,
        metavar="N",
        help="run each failing configuration N more times, to record how often it fails the same way (default 2)",
    )
    fuzz.add_argument(
        "--out",
        default=str(ROOT / "target" / "gc-fuzz-failures.jsonl"),
        metavar="FILE",
        help="where failing configurations are appended, one JSON object per line, with the "
        "environment that reproduces each, the commits of machopo and rusty_v8 they were built from, "
        "and a hung one's stacks, and the slow ones, marked \"slow\", with their stacks past the "
        "timeout; each failing one's whole output goes in a directory of the same name without "
        "the extension (default: target/gc-fuzz-failures.jsonl)",
    )

    automation = command(
        "automation",
        cmd_automation,
        "run every scenario in tests/scenarios/ in a real window (machopo --automation), and the pipe's "
        "test with one (machopo --automation-pipe --window)",
    )
    automation.add_argument(
        "filter",
        nargs="*",
        metavar="FILTER",
        help="run only the scenarios whose file name contains one of these, and the pipe's test "
        "if one is in `pipe` (default: all of them)",
    )

    scenarios = command(
        "scenarios", cmd_scenarios, "the scenarios in a browser window without the platform's (skipped by cargo test)"
    )
    scenarios.add_argument(
        "filter",
        nargs="*",
        metavar="FILTER",
        help="run only the scenarios whose file name contains one of these (default: all of them)",
    )

    wpt = command("wpt", cmd_wpt, "WPT's testharness.js tests, reftests and crashtests (wptrunner)")
    wpt.add_argument(
        "tests",
        nargs="*",
        metavar="TEST",
        help="run only these, paths in the checkout like dom/nodes/ (without the leading / under Git "
        "Bash, which rewrites it; default: those tests/wpt/include.txt names): those of the list under "
        "them, or a path under one of the list's",
    )
    wpt.add_argument(
        "--all",
        action="store_true",
        help="run the TEST paths whole, tests the list leaves out too (trying tests out for the list)",
    )
    wpt.add_argument(
        "--processes",
        type=int,
        metavar="N",
        help="tests at a time (default: 4 per CPU, at most 32)",
    )
    wpt.add_argument(
        "--chunk",
        type=chunk_argument,
        metavar="I/N",
        help="run the I-th of N chunks of the list (by directory), leaving the checks of the whole list to wpt-check",
    )
    build_arguments(wpt)

    wpt_check = command("wpt-check", cmd_wpt_check, "the checks of a wpt run of the whole list, over its chunks' logs")
    wpt_check.add_argument("raw_logs", nargs="+", type=Path, metavar="RAW_LOG", help="each chunk's raw.log")

    reftest = command("reftest", cmd_reftest, "our own reftests (wptrunner)")
    reftest.add_argument(
        "tests",
        nargs="*",
        metavar="TEST",
        help="run only these, paths under tests/reftests/ like flex/ (default: all of them)",
    )
    reftest.add_argument(
        "--processes",
        type=int,
        metavar="N",
        help="tests at a time (default: 4 per CPU, at most 32)",
    )
    build_arguments(reftest)

    wdspec = command("wdspec", cmd_wdspec, "WPT's WebDriver tests, against machopodriver and machopo (wptrunner)")
    wdspec.add_argument(
        "tests",
        nargs="*",
        metavar="TEST",
        help="run only these, paths in the checkout like webdriver/tests/classic/status "
        "(default: those tests/wpt/wdspec-include.txt names): those of the list under them, or a "
        "path under one of the list's",
    )
    wdspec.add_argument(
        "--all",
        action="store_true",
        help="run the TEST paths whole, tests the list leaves out too (trying tests out for the list)",
    )
    build_arguments(wdspec)


    cdp = command("cdp", cmd_cdp, "machopo's answers to DevTools' JavaScript domains against Chrome's")
    cdp.add_argument("cases", nargs="*", metavar="CASE", help="run only these cases (default: all)")

    mutate = command("mutate", cmd_mutate, "mutation testing: break the code as a spec file lists, check a test fails")
    mutate.add_argument("spec", nargs="?", metavar="SPEC", help="the TOML file that lists the mutants")
    mutate.add_argument("--only", nargs="+", metavar="NAME", help="run only the mutants whose name contains one of these")
    mutate.add_argument("--keep-going", action="store_true", help="run every mutant, past those missed")
    mutate.add_argument(
        "--restore", action="store_true", help="put back the files a run that was killed left broken, and do nothing else"
    )

    # Every command's own help, so that `test.py -h` explains every argument.
    parser.epilog = "\n".join(f"--- {name} ---\n\n{p.format_help()}" for name, p in sub.choices.items())

    args = parser.parse_args()
    # Every command builds machopo, which builds ../third_party's forks by path:
    # on other commits than machopo's pins, the failures would be of other code.
    # Putting back a killed mutation run's files builds nothing.
    builds_forks = not (args.command == "mutate" and args.restore)
    if builds_forks and not sync_third_party():
        return 1
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
