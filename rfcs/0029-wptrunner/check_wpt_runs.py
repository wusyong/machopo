"""Checks of `python3 scripts/test.py wpt` that need a run, for the mutation
tests (`mutations-stage*.toml`). From the repo root:

    python3 rfcs/0029-wptrunner/check_wpt_runs.py CHECK

with CHECK one of update, stale, unrun, timeout, screenshots, kunnew,
kunupdate:

- `update`: `MACHOPO_UPDATE_EXPECTATIONS=1` over one test whose `.ini` has a
  wrong expectation, a missing one, a condition on another platform, a
  comment and a `bug`: the wrong one goes, the missing one comes, the rest
  stays.
- `stale`: an `.ini` for a test the checkout doesn't have fails the run
  before it starts.
- `unrun`: an `.ini` under an include line for a file no test comes from
  (a support file, here) fails a whole run after it.
- `timeout`: `MACHOPO_WPT_TIMEOUT_MULTIPLIER` reaches wptrunner: at 0.1, a test
  that takes some 8 s times out (wptrunner gives a test 5 s more than its
  timeout, 1 s here).
- `screenshots`: a reftest expected to fail leaves its `-actual.png`,
  `-expected.png` and `-diff.png`: the actual one with the pixels that
  differ from the expected one in red.
- `kunnew`: a reftest just added under `tests/reftests/` (in a
  directory: WPT's manifest takes the root's own files for support files)
  runs, as our roots' manifests are brought up to date every run.
- `kunupdate`: `MACHOPO_UPDATE_EXPECTATIONS=1 test.py reftest` updates our
  own reftests' expectations: a wrong one goes.

Each puts back what it changed in `tests/wpt/metadata/` and `tests/reftests/`. Exits non-zero
when the check fails.
"""

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
METADATA = ROOT / "tests" / "wpt" / "metadata"
OTHER_OS = "win" if sys.platform == "darwin" else "mac"


def wpt(*tests, env=None):
    cmd = [sys.executable, str(ROOT / "scripts" / "test.py"), "wpt", *tests]
    run = subprocess.run(cmd, cwd=ROOT, env={**os.environ, **(env or {})}, capture_output=True, text=True,
                         encoding="utf-8", errors="replace")
    return run.returncode, run.stdout + run.stderr


def check_update():
    ini = METADATA / "dom" / "nodes" / "rootNode.html.ini"
    original = ini.read_bytes() if ini.exists() else None
    try:
        ini.write_text(
            "# a comment the update keeps\n"
            "[rootNode.html]\n"
            "  bug: a bug the update keeps\n"
            # Passes: an expectation the update removes.
            "  [getRootNode() must return the context object when it does not have any parent]\n"
            "    expected: FAIL\n"
            # Passes here: a condition on another platform the update keeps.
            "  [getRootNode() must return the document when a node is in document]\n"
            "    expected:\n"
            f'      if os == "{OTHER_OS}": FAIL\n',
            encoding="utf-8",
            newline="\n",
        )
        status, out = wpt("dom/nodes/rootNode.html", env={"MACHOPO_UPDATE_EXPECTATIONS": "1"})
        text = ini.read_text(encoding="utf-8") if ini.exists() else ""
        wanted = {
            "the comment": "# a comment the update keeps" in text,
            "the bug": "bug: a bug the update keeps" in text,
            "the other platform's condition": f'if os == "{OTHER_OS}": FAIL' in text,
            "no wrong expectation": "does not have any parent" not in text,
            # Fails for the shadow-including root (no shadow DOM).
            "the missing expectation": "shadow-including root" in text,
        }
        failed = [name for name, ok in wanted.items() if not ok]
        if failed:
            print(f"update: {', '.join(failed)} wrong (exit {status}):\n{text}\n{out[-2000:]}")
            return 1
        return 0
    finally:
        if original is None:
            ini.unlink(missing_ok=True)
        else:
            ini.write_bytes(original)


def check_stale():
    ini = METADATA / "no-such-directory" / "no-such-test.html.ini"
    ini.parent.mkdir(parents=True, exist_ok=True)
    try:
        ini.write_text("[no-such-test.html]\n  expected: FAIL\n", encoding="utf-8", newline="\n")
        status, out = wpt("dom/nodes/rootNode.html")
        if status == 0 or "no-such-directory/no-such-test.html.ini" not in out:
            print(f"stale: the run didn't fail on the stale .ini (exit {status}):\n{out[-2000:]}")
            return 1
        return 0
    finally:
        ini.unlink(missing_ok=True)
        ini.parent.rmdir()


def check_unrun():
    ini = METADATA / "dom" / "nodes" / "attributes.js.ini"
    try:
        ini.write_text("[attributes.js]\n  expected: FAIL\n", encoding="utf-8", newline="\n")
        status, out = wpt()
        if status == 0 or "dom/nodes/attributes.js.ini" not in out:
            print(f"unrun: the run didn't fail on the .ini of a support file (exit {status}):\n{out[-2000:]}")
            return 1
        return 0
    finally:
        ini.unlink(missing_ok=True)


def check_timeout():
    status, out = wpt("html/semantics/scripting-1/the-script-element/async_007.htm",
                      env={"MACHOPO_WPT_TIMEOUT_MULTIPLIER": "0.1"})
    if status == 0 or "TIMEOUT" not in out:
        print(f"timeout: the test didn't time out at 0.1 times (exit {status}):\n{out[-2000:]}")
        return 1
    return 0


def check_screenshots():
    # Expected to fail, with differing pixels that aren't red already.
    test = "css/css-flexbox/align-baseline.html"
    status, out = wpt(test)
    base = ROOT / "target" / "wpt-results" / "screenshots" / test
    shots = {kind: Path(f"{base}-{kind}.png") for kind in ("actual", "expected", "diff")}
    missing = [kind for kind, path in shots.items() if not path.exists()]
    if status != 0 or missing:
        print(f"screenshots: missing {missing} (exit {status}):\n{out[-2000:]}")
        return 1
    # Pillow is the venv's (wptrunner's), not this Python's.
    venv = ROOT / "target" / "wptrunner" / "venv"
    python = venv / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    compare = """
import sys
from PIL import Image
actual, expected, diff = (Image.open(p).convert("RGB") for p in sys.argv[1:])
red, differing = (255, 0, 0), 0
for a, e, d in zip(actual.getdata(), expected.getdata(), diff.getdata()):
    differing += a != e
    if d != (red if a != e else a):
        sys.exit("the diff isn't the actual one with the differing pixels in red")
if not differing:
    sys.exit("the actual and expected ones are the same")
"""
    run = subprocess.run([str(python), "-c", compare, *map(str, shots.values())], capture_output=True, text=True)
    if run.returncode != 0:
        print(f"screenshots: {run.stderr.strip()}")
        return 1
    return 0


MACHOPO_TESTS = ROOT / "tests" / "reftests"


def reftest(*tests, env=None):
    cmd = [sys.executable, str(ROOT / "scripts" / "test.py"), "reftest", *tests]
    run = subprocess.run(cmd, cwd=ROOT, env={**os.environ, **(env or {})}, capture_output=True, text=True,
                         encoding="utf-8", errors="replace")
    return run.returncode, run.stdout + run.stderr


def started(raw_log):
    import json

    with open(raw_log, encoding="utf-8") as log:
        return {json.loads(line).get("test") for line in log if '"test_start"' in line}


def check_kun_new():
    test = MACHOPO_TESTS / "block" / "zz-check-new.html"
    ref = MACHOPO_TESTS / "block" / "zz-check-new-ref.html"
    try:
        ref.write_text("<!DOCTYPE html><div style='width:100px;height:100px;background:green'></div>\n",
                       encoding="utf-8", newline="\n")
        test.write_text("<!DOCTYPE html><link rel=match href=zz-check-new-ref.html>"
                        "<div style='width:100px;height:100px;background:green'></div>\n",
                        encoding="utf-8", newline="\n")
        status, out = reftest("block/zz-check-new.html")
        ran = started(ROOT / "target" / "reftest-results" / "raw.log")
        if status != 0 or "/_kun/block/zz-check-new.html" not in ran:
            print(f"kunnew: the new reftest didn't run (exit {status}):\n{out[-2000:]}")
            return 1
        return 0
    finally:
        test.unlink(missing_ok=True)
        ref.unlink(missing_ok=True)


def check_kun_update():
    ini = MACHOPO_TESTS / "metadata" / "flex" / "f1-row.html.ini"
    try:
        ini.parent.mkdir(parents=True, exist_ok=True)
        # Passes: an expectation the update removes, and the file with it.
        ini.write_text("[f1-row.html]\n  expected: FAIL\n", encoding="utf-8", newline="\n")
        status, out = reftest("flex/f1-row.html", env={"MACHOPO_UPDATE_EXPECTATIONS": "1"})
        if ini.exists():
            print(f"kunupdate: the wrong expectation stayed (exit {status}):\n{ini.read_text()}\n{out[-2000:]}")
            return 1
        return 0
    finally:
        ini.unlink(missing_ok=True)
        for directory in (ini.parent, ini.parent.parent):
            if directory.exists() and not any(directory.iterdir()):
                directory.rmdir()


if __name__ == "__main__":
    checks = {
        "update": check_update,
        "stale": check_stale,
        "unrun": check_unrun,
        "timeout": check_timeout,
        "screenshots": check_screenshots,
        "kunnew": check_kun_new,
        "kunupdate": check_kun_update,
    }
    if len(sys.argv) != 2 or sys.argv[1] not in checks:
        sys.exit(f"usage: {sys.argv[0]} {'|'.join(checks)}")
    sys.exit(checks[sys.argv[1]]())
