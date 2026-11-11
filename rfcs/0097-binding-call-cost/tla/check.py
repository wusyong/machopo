#!/usr/bin/env python3
"""Model-check this folder's Unified.tla with TLC: elise's `tla/unified`
model (its marking with V8's in one major GC) with V8 allowed to allocate
in an operation (a wrapper made, V8's scavenge, its marking's start and its
pause, all with the operation's raw pointers held), under the three rules
RFC 0097 gives; and with each rule taken out, which must give the
counterexample listed. `none` is the protocol as it is today, unchanged by
what the model adds. The runner is elise's `tla/unified/check.py`.

TLC comes from the TLA+ tools' release (pinned, checked against its
SHA-256, kept in machopo's target/tla-0097/); Java is the machine's.

    python3 rfcs/0097-binding-call-cost/tla/check.py [--mutation NAME]
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
# machopo's `target/` (the model is elise's `tla/unified`, changed).
OUT = HERE.parents[2] / "target" / "tla-0097"

TLC_VERSION = "1.7.4"
TLC_URL = f"https://github.com/tlaplus/tlaplus/releases/download/v{TLC_VERSION}/tla2tools.jar"
TLC_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"

# The model's sizes (the new heap's objects, JS objects, fields each). The
# protocol holds at (2, 2, 2) too (88.6 million states, 17 minutes on an
# Apple M4: too long for every mutation); (3, 2, 1) and (2, 3, 1) didn't
# finish in 15 minutes.
SIZES = [(2, 2, 1)]

SAFETY = ["TypeOK", "MarkingComplete", "NoDangling", "WrappedNotYoung", "ExpandosKept"]

# (mutation, invariants checked, the invariant TLC must find violated or
# None when it must hold, why).
CHECKS = [
    ("none", SAFETY, None, "the protocol as it is (the model unchanged by `snap`)"),
    ("no_black_allocation", SAFETY, "MarkingComplete", "as before"),
    ("inop", SAFETY, None, "V8 allocating in operations, with the rules"),
    ("inop_snapshot_at_once", SAFETY, "MarkingComplete",
     "the snapshot taken in the operation misses what only it holds"),
    ("inop_plain_finish", SAFETY, "MarkingComplete",
     "the pause in the operation frees what only it holds"),
    ("inop_no_wrap_barrier", SAFETY, "MarkingComplete",
     "a node wrapped in the operation, its black wrapper never scanned"),
    ("inop_scan_before_snapshot", SAFETY, "MarkingComplete",
     "an object scanned before the snapshot, written to with no barrier"),
]


def tlc_jar() -> Path:
    jar = OUT / f"tla2tools-{TLC_VERSION}.jar"
    if not jar.exists() or hashlib.sha256(jar.read_bytes()).hexdigest() != TLC_SHA256:
        OUT.mkdir(parents=True, exist_ok=True)
        print(f"downloading TLC {TLC_VERSION}", file=sys.stderr)
        with urllib.request.urlopen(TLC_URL) as response:
            data = response.read()
        digest = hashlib.sha256(data).hexdigest()
        if digest != TLC_SHA256:
            raise SystemExit(f"{TLC_URL}: SHA-256 {digest}, expected {TLC_SHA256}")
        jar.write_bytes(data)
    return jar


def java_binary(given: str | None) -> str:
    candidates = [given] if given else [
        shutil.which("java"),
        os.path.join(os.environ["JAVA_HOME"], "bin", "java") if os.environ.get("JAVA_HOME") else None,
        "/opt/homebrew/opt/openjdk/bin/java",
        "/usr/local/opt/openjdk/bin/java",
    ]
    for java in filter(None, candidates):
        # macOS has a /usr/bin/java that only says there is no Java.
        if os.path.exists(java) and subprocess.run([java, "-version"], capture_output=True).returncode == 0:
            return java
    raise SystemExit("no Java: install a JDK (for example `brew install openjdk`) or pass --java")


def config(size: tuple[int, int, int], mutation: str, invariants: list[str]) -> str:
    objects = ", ".join(f"o{i}" for i in range(1, size[0] + 1))
    js = ", ".join(f"j{i}" for i in range(1, size[1] + 1))
    return "\n".join([
        "SPECIFICATION Spec",
        "CONSTANTS",
        f"    Obj = {{{objects}}}",
        f"    Js = {{{js}}}",
        f"    NF = {size[2]}",
        "    Null = Null",
        "    None = None",
        f'    Mutation = "{mutation}"',
        "SYMMETRY Symmetry",
        *[f"INVARIANT {name}" for name in invariants],
        "",
    ])


def run(java: str, jar: Path, size: tuple[int, int, int], mutation: str,
        invariants: list[str]) -> dict:
    name = f"Unified-{'x'.join(map(str, size))}-{mutation}-{'-'.join(invariants)}"
    cfg = OUT / f"{name}.cfg"
    OUT.mkdir(parents=True, exist_ok=True)
    with open(cfg, "w", newline="\n") as f:
        f.write(config(size, mutation, invariants))
    # TLC looks for the config next to the spec, so both go in OUT.
    shutil.copyfile(HERE / "Unified.tla", OUT / "Unified.tla")
    start = time.monotonic()
    done = subprocess.run(
        [java, "-XX:+UseParallelGC", "-cp", str(jar), "tlc2.TLC",
         "-workers", "auto", "-deadlock", "-cleanup",
         "-metadir", str(OUT / "states" / name),
         "-config", cfg.name, "Unified.tla"],
        cwd=OUT, capture_output=True, text=True)
    seconds = time.monotonic() - start
    with open(OUT / f"{name}.out", "w", newline="\n") as f:
        f.write(done.stdout + done.stderr)
    violated = re.search(r"Invariant (\w+) is violated", done.stdout)
    # The last count is the total; the progress lines before it aren't.
    states = re.findall(r"([\d,]+) distinct states found", done.stdout)
    # TLC exits 0 when every invariant holds and 12 on a safety violation;
    # anything else is a broken model or setup.
    if done.returncode not in (0, 12) or (done.returncode == 12) != bool(violated):
        raise SystemExit(f"TLC failed on {name} (exit {done.returncode}): see {OUT / (name + '.out')}")
    return {
        "violated": violated.group(1) if violated else None,
        "states": states[-1] if states else "?",
        "seconds": seconds,
        "output": OUT / f"{name}.out",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--java", help="the java binary to run TLC with")
    parser.add_argument("--mutation", help="only this mutation's checks")
    args = parser.parse_args()

    java = java_binary(args.java)
    jar = tlc_jar()
    failed = []
    print("| Size | Mutation | Checked | Expected | TLC | Distinct states | s |")
    print("| --- | --- | --- | --- | --- | --- | --- |", flush=True)
    for mutation, invariants, expected, why in CHECKS:
        if args.mutation and mutation != args.mutation:
            continue
        for size in SIZES:
            want = expected.get(size) if isinstance(expected, dict) else expected
            result = run(java, jar, size, mutation, invariants)
            ok = result["violated"] == want
            print(f"| {'x'.join(map(str, size))} | {mutation} | {', '.join(invariants)} "
                  f"| {want or 'holds'} | {result['violated'] or 'holds'}{'' if ok else ' (WRONG)'} "
                  f"| {result['states']} | {result['seconds']:.1f} |", flush=True)
            if not ok:
                failed.append(f"{'x'.join(map(str, size))} {mutation}: expected {want or 'holds'} "
                              f"({why}), TLC: {result['violated'] or 'holds'}; see {result['output']}")
    if failed:
        raise SystemExit("\n".join(failed))


if __name__ == "__main__":
    main()
