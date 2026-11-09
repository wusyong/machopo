"""Tests of how `test.py` waits for the GC tests (`matrix`, `fuzz`): a run
past its timeout that keeps using the CPU is slow, waited for up to a
limit; one that stops using it, or keeps busy past the limit, is hung and
killed. Real processes (small Python programs) stand in for cargo and its
test binaries; the debugger is a stand-in. Linux and macOS: on Windows a
run past its timeout is hung, as the tool can't read its CPU time.

    python3 scripts/gc_wait_test.py [-k NAME]
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

# `test` is the standard library's test package: loaded by its path.
_spec = importlib.util.spec_from_file_location("machopo_test_py", Path(__file__).resolve().parent / "test.py")
test_py = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = test_py
_spec.loader.exec_module(test_py)

# Small enough for the tests to take seconds, with an interval long enough
# for a busy loop to use `BUSY` of CPU in it on a loaded machine.
INTERVAL = 0.3
BUSY = 0.05
STACKS = "(the stand-in debugger's stacks)"


def start(code: str) -> subprocess.Popen:
    """`code` in a Python of its own, as `run_gc_tests` starts cargo."""
    return subprocess.Popen(
        [sys.executable, "-c", code],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )


def stop(proc: subprocess.Popen) -> None:
    """`proc` and what's under it ended, if a test left them running."""
    if proc.poll() is None:
        os.killpg(proc.pid, signal.SIGKILL)
    proc.communicate()


# Busy for `seconds`, then done.
BUSY_FOR = "import time\nend = time.monotonic() + {seconds}\nwhile time.monotonic() < end: pass\nprint('done')"


@unittest.skipIf(os.name == "nt", "the tool reads no CPU time on Windows")
class WaitFor(unittest.TestCase):
    def setUp(self) -> None:
        self.stacks_taken = 0
        for name, value in (
            ("PROGRESS_INTERVAL", INTERVAL),
            ("BUSY_CPU_SECONDS", BUSY),
            ("thread_stacks", self.thread_stacks),
        ):
            patcher = mock.patch.object(test_py, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def thread_stacks(self, pid: int) -> str:
        # The processes are stopped while their stacks are taken (where
        # `/proc` tells: the signal arrives a moment after it's sent).
        stat = Path(f"/proc/{pid}/stat")
        if stat.exists():
            deadline = time.monotonic() + 2
            while stat.read_text().rsplit(")", 1)[1].split()[0] != "T":
                self.assertLess(time.monotonic(), deadline, "not stopped while its stacks are taken")
                time.sleep(0.01)
        self.stacks_taken += 1
        return STACKS

    def wait(self, code: str, timeout: float, limit_factor: float = 10) -> tuple[test_py.Waited, subprocess.Popen]:
        proc = start(code)
        self.addCleanup(lambda: stop(proc))
        with mock.patch.object(test_py, "SLOW_LIMIT_FACTOR", limit_factor):
            return test_py.wait_for(proc, timeout), proc

    def test_done_within_the_timeout(self) -> None:
        waited, _ = self.wait("print('done')", timeout=10)
        self.assertEqual((waited.outcome, waited.output.strip(), waited.stacks), ("done", "done", ""))
        self.assertEqual(self.stacks_taken, 0)

    def test_busy_past_the_timeout_and_done_is_slow(self) -> None:
        waited, proc = self.wait(BUSY_FOR.format(seconds=2), timeout=0.5)
        self.assertEqual((waited.outcome, waited.output.strip()), ("slow", "done"))
        self.assertGreaterEqual(waited.took, 2)
        self.assertEqual(proc.returncode, 0)
        # Its stacks once, when it was first found busy past the timeout.
        self.assertEqual((waited.stacks, self.stacks_taken), (STACKS, 1))

    def test_idle_past_the_timeout_is_hung(self) -> None:
        waited, proc = self.wait("import time\nprint('started', flush=True)\ntime.sleep(60)", timeout=0.5)
        self.assertEqual((waited.outcome, waited.busy, waited.stacks), ("hung", False, STACKS))
        self.assertEqual(waited.output.strip(), "started")
        # Killed after one interval past the timeout, not at the limit.
        self.assertLess(waited.took, 0.5 + 3 * INTERVAL)
        self.assertEqual(proc.returncode, -signal.SIGKILL)
        self.assertEqual(self.stacks_taken, 1)

    def test_busy_past_the_limit_is_hung(self) -> None:
        waited, proc = self.wait("while True: pass", timeout=0.5, limit_factor=4)
        self.assertEqual((waited.outcome, waited.busy, waited.stacks), ("hung", True, STACKS))
        self.assertGreaterEqual(waited.took, 2)
        self.assertLess(waited.took, 2 + 3 * INTERVAL)
        self.assertEqual(proc.returncode, -signal.SIGKILL)
        # Once past the timeout, and once before it was killed.
        self.assertEqual(self.stacks_taken, 2)

    def test_a_slow_run_is_left_going_after_its_stacks(self) -> None:
        # Stopped while its stacks are taken, then continued: it gets to the
        # end on its own.
        waited, proc = self.wait(BUSY_FOR.format(seconds=1.5), timeout=0.3)
        self.assertEqual(waited.outcome, "slow")
        self.assertEqual(proc.returncode, 0)

    def test_without_cpu_time_past_the_timeout_is_hung(self) -> None:
        with mock.patch.object(test_py, "cpu_usage", lambda pid: None):
            waited, proc = self.wait(BUSY_FOR.format(seconds=30), timeout=0.5)
        self.assertEqual((waited.outcome, waited.busy, waited.stacks), ("hung", None, STACKS))
        self.assertLess(waited.took, 0.5 + 2 * INTERVAL)
        self.assertEqual(proc.returncode, -signal.SIGKILL)


@unittest.skipIf(os.name == "nt", "the tool reads no CPU time on Windows")
class CpuUsage(unittest.TestCase):
    PARENT = (
        "import subprocess, sys, time\n"
        "child = subprocess.Popen([sys.executable, '-c', 'import time\\nend = time.monotonic() + 30\\n"
        "while time.monotonic() < end: pass'])\n"
        "print(child.pid, flush=True)\n"
        "child.wait()"
    )

    def usage_under_a_parent(self, apart: float) -> tuple[subprocess.Popen, int, dict, dict]:
        """A parent that only waits for its busy child: their usage twice,
        `apart` seconds apart."""
        proc = start(self.PARENT)
        self.addCleanup(lambda: stop(proc))
        child = int(proc.stdout.readline())
        before = test_py.cpu_usage(proc.pid)
        time.sleep(apart)
        return proc, child, before, test_py.cpu_usage(proc.pid)

    def check(self, apart: float = 0.5) -> None:
        proc, child, before, after = self.usage_under_a_parent(apart)
        self.assertEqual(set(before), {proc.pid, child})
        self.assertEqual(set(after), {proc.pid, child})
        # The child's time is counted, which the parent alone doesn't use;
        # in seconds: one thread uses no more than the time that passed.
        self.assertGreater(after[child] - before[child], 0.2)
        self.assertLess(after[child] - before[child], apart + 1.1)
        self.assertLess(after[proc.pid] - before[proc.pid], 0.2)

    def test_counts_the_processes_under_it(self) -> None:
        self.check()

    def test_through_ps(self) -> None:
        # Where there is no `/proc` (macOS), or here as if there were none.
        # Linux's `ps` gives the time only to the second.
        with mock.patch.object(test_py, "PROC", Path("/nonexistent-proc")):
            self.check(apart=1.5)

    def test_a_process_gone_has_none(self) -> None:
        proc = start("pass")
        proc.communicate()
        self.assertIsNone(test_py.cpu_usage(proc.pid))

    def test_ps_time(self) -> None:
        self.assertEqual(test_py.cpu_seconds("00:00:07"), 7)
        self.assertEqual(test_py.cpu_seconds("01:02:03"), 3723)
        self.assertEqual(test_py.cpu_seconds("2-00:00:01"), 2 * 86400 + 1)
        # macOS: minutes and seconds to the hundredth.
        self.assertAlmostEqual(test_py.cpu_seconds("0:01.50"), 1.5)
        self.assertAlmostEqual(test_py.cpu_seconds("12:34.56"), 754.56)


class MakingProgress(unittest.TestCase):
    def test_busy_enough(self) -> None:
        with mock.patch.object(test_py, "BUSY_CPU_SECONDS", 1.0):
            self.assertTrue(test_py.making_progress({1: 5.0, 2: 1.0}, {1: 5.5, 2: 1.5}))
            self.assertFalse(test_py.making_progress({1: 5.0, 2: 1.0}, {1: 5.4, 2: 1.5}))

    def test_a_process_ended_or_started(self) -> None:
        # cargo going on to the next test binary, which hasn't used much yet.
        self.assertTrue(test_py.making_progress({1: 5.0, 2: 9.0}, {1: 5.0, 3: 0.0}))
        self.assertTrue(test_py.making_progress({1: 5.0}, {1: 5.0, 3: 0.0}))
        self.assertTrue(test_py.making_progress({1: 5.0, 2: 9.0}, {1: 5.0}))


@unittest.skipIf(os.name == "nt", "the tool reads no CPU time on Windows")
class RunGcTests(unittest.TestCase):
    """`run_gc_tests` with a Python program in cargo's place, which prints
    libtest's lines: what it makes of each outcome."""

    def run_gc_tests(self, code: str, timeout: float, limit_factor: float = 10) -> test_py.Result:
        real = subprocess.Popen

        def popen(command, **kwargs):
            if command[0] == "cargo":
                command = [sys.executable, "-c", code]
            return real(command, **kwargs)

        with (
            mock.patch.object(subprocess, "Popen", popen),
            mock.patch.object(test_py, "PROGRESS_INTERVAL", INTERVAL),
            mock.patch.object(test_py, "BUSY_CPU_SECONDS", BUSY),
            mock.patch.object(test_py, "SLOW_LIMIT_FACTOR", limit_factor),
            mock.patch.object(test_py, "thread_stacks", lambda pid: STACKS),
        ):
            return test_py.run_gc_tests(test_py.Config(), timeout)

    PASSED = "print('     Running tests/it/main.rs (target/debug/deps/it-0123456789abcdef)')\n" \
        "print('test result: ok. 3 passed; 0 failed; 0 ignored')"

    def test_in_time(self) -> None:
        result = self.run_gc_tests(self.PASSED, timeout=10)
        self.assertEqual((result.ok, result.slow, result.stacks), (True, False, ""))
        self.assertEqual(result.summary, "3 passed in 1 test binaries")

    def test_slow(self) -> None:
        busy = "import time\nend = time.monotonic() + 1.5\nwhile time.monotonic() < end: pass\n"
        result = self.run_gc_tests(busy + self.PASSED, timeout=0.5)
        self.assertEqual((result.ok, result.slow, result.stacks), (True, True, STACKS))
        self.assertRegex(result.summary, r"^3 passed in 1 test binaries, slowly: past the timeout of 0s, done in \ds$")

    def test_slow_and_failed_is_a_failure(self) -> None:
        busy = "import time, sys\nend = time.monotonic() + 1.5\nwhile time.monotonic() < end: pass\n"
        failed = "print('test x ... FAILED')\nprint('test result: FAILED. 2 passed; 1 failed; 0 ignored')\nsys.exit(101)"
        result = self.run_gc_tests(busy + failed, timeout=0.5)
        self.assertEqual((result.ok, result.slow, result.signature), (False, False, "test x"))

    def test_hung_idle(self) -> None:
        idle = "print('     Running tests/it/main.rs (target/debug/deps/it-0123456789abcdef)', flush=True)\n" \
            "import time\ntime.sleep(60)"
        result = self.run_gc_tests(idle, timeout=0.5)
        self.assertEqual((result.ok, result.slow, result.signature, result.stacks), (False, False, "it timed out", STACKS))
        self.assertRegex(result.summary, r"^hung in it: idle, killed after \ds$")

    def test_hung_busy(self) -> None:
        busy = "print('     Running tests/it/main.rs (target/debug/deps/it-0123456789abcdef)', flush=True)\n" \
            "while True: pass"
        result = self.run_gc_tests(busy, timeout=0.5, limit_factor=4)
        self.assertEqual((result.ok, result.signature), (False, "it timed out"))
        self.assertRegex(result.summary, r"^hung in it: still busy, killed after \ds$")

    def test_hung_without_cpu_time(self) -> None:
        with mock.patch.object(test_py, "cpu_usage", lambda pid: None):
            result = self.run_gc_tests("import time\ntime.sleep(60)", timeout=0.5)
        self.assertRegex(result.summary, r"^hung in cargo: killed after \ds$")


def gc_result(summary: str, ok: bool = True, slow: bool = False) -> test_py.Result:
    return test_py.Result(ok, summary, "", "" if ok else "test x", STACKS if slow else "", slow)


class Commands(unittest.TestCase):
    """`fuzz` and `matrix` with `run_gc_tests` stood in for: what they make
    of a slow run."""

    def run_command(self, command, results: list[test_py.Result], **args) -> tuple[int, str, list[dict]]:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "failures.jsonl"
            namespace = argparse.Namespace(
                seed=1, out=str(out), timeout=1, minutes=None, iterations=len(results) - 1,
                max_flags=1, reruns=0, stop_after=3, **args,
            )
            printed = io.StringIO()
            with (
                mock.patch.object(test_py, "build_gc_tests", lambda: True),
                mock.patch.object(test_py, "run_gc_tests", mock.Mock(side_effect=results)),
                mock.patch.object(test_py, "revisions", lambda: {"machopo": "abc"}),
                mock.patch.object(test_py, "debug_crashes", lambda *a: ""),
                contextlib.redirect_stdout(printed),
            ):
                status = command(namespace)
            records = [json.loads(line) for line in out.read_text().splitlines()] if out.exists() else []
        return status, printed.getvalue(), records

    def test_fuzz_records_a_slow_run_and_passes(self) -> None:
        status, printed, records = self.run_command(
            test_py.cmd_fuzz, [gc_result("baseline"), gc_result("1 passed, slowly", slow=True), gc_result("ok")]
        )
        self.assertEqual(status, 0)
        self.assertIn("[1] slow ", printed)
        self.assertIn("[2] ok ", printed)
        self.assertIn("2/2 configurations passed, 1 of them slowly", printed)
        self.assertEqual(len(records), 1)
        self.assertEqual(
            {k: records[0][k] for k in ("run", "slow", "summary", "stacks")},
            {"run": 1, "slow": True, "summary": "1 passed, slowly", "stacks": STACKS},
        )
        self.assertIn("env", records[0])
        self.assertIn("reproduce", records[0])

    def test_fuzz_still_fails_on_a_failure(self) -> None:
        status, printed, records = self.run_command(
            test_py.cmd_fuzz,
            [gc_result("baseline"), gc_result("1 passed, slowly", slow=True), gc_result("1 failed", ok=False)],
        )
        self.assertEqual(status, 1)
        self.assertEqual([record.get("slow", False) for record in records], [True, False])
        self.assertIn("1/2 configurations passed, 1 of them slowly", printed)

    def test_fuzz_without_slow_runs_says_nothing_of_them(self) -> None:
        status, printed, records = self.run_command(test_py.cmd_fuzz, [gc_result("baseline"), gc_result("ok")])
        self.assertEqual((status, records), (0, []))
        self.assertNotIn("slow", printed)

    def test_matrix_prints_a_slow_run_and_passes(self) -> None:
        configs = len(test_py.matrix_configs())
        results = [gc_result("ok")] * configs
        results[3] = gc_result("1 passed, slowly", slow=True)
        printed = io.StringIO()
        with (
            mock.patch.object(test_py, "build_gc_tests", lambda: True),
            mock.patch.object(test_py, "run_gc_tests", mock.Mock(side_effect=results)),
            contextlib.redirect_stdout(printed),
        ):
            status = test_py.cmd_matrix(argparse.Namespace())
        self.assertEqual(status, 0)
        lines = [line for line in printed.getvalue().splitlines() if line.startswith(("ok", "slow", "FAIL"))]
        self.assertEqual([line.split()[0] for line in lines], ["ok"] * 3 + ["slow"] + ["ok"] * (configs - 4))


if __name__ == "__main__":
    unittest.main()
