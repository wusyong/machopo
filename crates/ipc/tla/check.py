#!/usr/bin/env python3
"""Model-check ipc's protocols with TLC: Channel.tla (one end of a channel:
its queue's limit, its end told once), Regions.tla (regions made for a
peer, written, read and given back) and Pipe.tla (a data pipe's credit).
Each is checked as it is, and with each part of it taken out (its
`Mutation`): the first must hold, and each mutation must give the
counterexample listed below.

TLC comes from the TLA+ tools' release (pinned, checked against its
SHA-256, kept in target/tla/); Java is the machine's (`java` on PATH,
$JAVA_HOME, or Homebrew's openjdk; --java to name one).
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
ROOT = HERE.parents[2]
OUT = ROOT / "target" / "tla"

TLC_VERSION = "1.7.4"
TLC_URL = f"https://github.com/tlaplus/tlaplus/releases/download/v{TLC_VERSION}/tla2tools.jar"
TLC_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"

CHANNEL = ["TypeOK", "QueueBound", "ToldOnce", "ToldFirstReason", "ReasonStays", "NoLateDelivery",
           "NothingWaitsWhenOver", "NoLateSend"]
REGIONS = ["TypeOK", "ReadsWhatWasSent", "ToldBeforeNamed", "NoFalseBad", "OnePeerEver", "TableIsOwn"]
PIPE = ["TypeOK", "WindowBound", "NoFalseBad", "Accounted", "Conserved", "NotStuck", "CancelHeard"]

# Each model's sizes (its constants but `Mutation`), and its checks:
# (mutation, extra constants, invariants checked, the invariant TLC must
# find violated or None when all must hold, why).
MODELS = {
    "Channel": {
        "sizes": [{"Limit": "6", "Sizes": "{1, 2, 3}", "MaxSends": "8"}],
        "checks": [
            ("none", {}, CHANNEL, None, "the channel's end"),
            ("no_limit", {}, ["QueueBound"], "QueueBound", "a peer that doesn't read: the queue grows past its limit"),
            ("reason_overwritten", {}, ["ReasonStays"], "ReasonStays", "a later reason takes the first one's place"),
            ("reason_overwritten", {}, ["ToldFirstReason"], "ToldFirstReason",
             "and it is the later one the IO thread is told"),
            ("told_each_time", {}, ["ToldOnce"], "ToldOnce", "the IO thread is told again each time it comes by"),
            ("deliver_after_over", {}, ["NoLateDelivery"], "NoLateDelivery",
             "a message handed up after the IO thread was told the channel is over"),
            ("queue_kept_when_over", {}, ["NothingWaitsWhenOver"], "NothingWaitsWhenOver",
             "messages (and their handles) left waiting on a channel nothing will write"),
            ("send_after_over", {}, ["NoLateSend"], "NoLateSend", "a send taken on a channel that is over"),
        ],
    },
    "Regions": {
        "sizes": [
            {"Peers": "{p1, p2}", "Regions": "{r1, r2}", "MaxData": "3"},
            {"Peers": "{p1, p2}", "Regions": "{r1, r2, r3}", "MaxData": "4"},
        ],
        "checks": [
            ("none", {}, REGIONS, None, "the regions' protocol"),
            ("reuse_without_release", {}, ["ReadsWhatWasSent"], "ReadsWhatWasSent",
             "a region written again before the peer read the first piece"),
            ("release_on_arrival", {}, ["ReadsWhatWasSent"], "ReadsWhatWasSent",
             "given back when the message arrived, written again before the peer got to it"),
            ("use_before_grant", {}, ["ToldBeforeNamed"], "ToldBeforeNamed",
             "a message names a region its peer is told of only after"),
            ("release_twice", {}, ["NoFalseBad"], "NoFalseBad", "the second giving back finds nothing out"),
            ("regrant_after_peer_gone", {}, ["OnePeerEver"], "OnePeerEver",
             "a gone peer's region, what it held still in it, made another's"),
            ("grant_to_two", {}, ["OnePeerEver"], "OnePeerEver", "one region in two peers' hands at once"),
        ],
    },
    "Pipe": {
        "sizes": [
            {"Capacity": "1", "Total": "3", "MaxChunk": "2"},
            {"Capacity": "2", "Total": "5", "MaxChunk": "2"},
            {"Capacity": "3", "Total": "6", "MaxChunk": "2"},
            {"Capacity": "4", "Total": "6", "MaxChunk": "3"},
            {"Capacity": "6", "Total": "10", "MaxChunk": "3"},
        ],
        "checks": [
            ("none", {"Hostile": "FALSE"}, PIPE, None, "the pipe's protocol"),
            ("none", {"Hostile": "TRUE"}, ["TypeOK", "WindowBound"], None,
             "whatever the reader's end sends, the writer's count stays in the capacity"),
            ("no_window", {"Hostile": "FALSE"}, ["NoFalseBad"], "NoFalseBad",
             "a writer that sends past its room: the reader finds more than the pipe holds"),
            ("threshold_over_capacity", {"Hostile": "FALSE"}, ["NotStuck"], "NotStuck",
             "a reader that waits for more than the pipe holds before it reports: the writer never has room again"),
            ("no_report_reset", {"Hostile": "FALSE"}, ["NoFalseBad"], "NoFalseBad",
             "bytes reported twice: the writer is told of more than it sent"),
            ("no_overread_check", {"Hostile": "TRUE"}, ["WindowBound"], "WindowBound",
             "a report of more than was sent gives room the reader doesn't have"),
            ("stale_report_is_bad", {"Hostile": "FALSE"}, ["NoFalseBad"], "NoFalseBad",
             "a report that crossed the pipe's end closes the channel"),
            ("stale_data_is_bad", {"Hostile": "FALSE"}, ["NoFalseBad"], "NoFalseBad",
             "bytes that crossed the reader's cancel close the channel"),
            ("cancel_keeps_writing", {"Hostile": "FALSE"}, ["CancelHeard"], "CancelHeard",
             "a writer that goes on after its reader cancelled"),
        ],
    },
}


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


def model_values(constants: dict[str, str]) -> list[str]:
    """The names in a constant's set that aren't numbers: model values, each
    declared as itself."""
    names = []
    for value in constants.values():
        if value.startswith("{"):
            names += [name for name in re.findall(r"[A-Za-z_]\w*", value)]
    return names


def config(constants: dict[str, str], mutation: str, invariants: list[str]) -> str:
    return "\n".join([
        "SPECIFICATION Spec",
        "CONSTANTS",
        *[f"    {name} = {value}" for name, value in constants.items()],
        *[f"    {name} = {name}" for name in model_values(constants)],
        f'    Mutation = "{mutation}"',
        *[f"INVARIANT {name}" for name in invariants],
        "",
    ])


def run(java: str, jar: Path, model: str, constants: dict[str, str], mutation: str,
        invariants: list[str]) -> dict:
    size = "x".join(re.sub(r"\W+", "", value) for value in constants.values())
    name = f"{model}-{size}-{mutation}-{'-'.join(invariants)}"[:200]
    cfg = OUT / f"{name}.cfg"
    OUT.mkdir(parents=True, exist_ok=True)
    with open(cfg, "w", newline="\n") as f:
        f.write(config(constants, mutation, invariants))
    # TLC looks for the config next to the spec, so both go in OUT.
    shutil.copyfile(HERE / f"{model}.tla", OUT / f"{model}.tla")
    start = time.monotonic()
    done = subprocess.run(
        [java, "-XX:+UseParallelGC", "-cp", str(jar), "tlc2.TLC",
         "-workers", "auto", "-deadlock", "-cleanup",
         "-metadir", str(OUT / "states" / name),
         "-config", cfg.name, f"{model}.tla"],
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
    parser.add_argument("--model", choices=sorted(MODELS), help="only this model")
    parser.add_argument("--mutation", help="only this mutation's checks")
    args = parser.parse_args()

    java = java_binary(args.java)
    jar = tlc_jar()
    failed = []
    print("| Model | Size | Mutation | Checked | Expected | TLC | Distinct states | s |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- |", flush=True)
    for model, spec in MODELS.items():
        if args.model and model != args.model:
            continue
        for mutation, extra, invariants, expected, why in spec["checks"]:
            if args.mutation and mutation != args.mutation:
                continue
            for size in spec["sizes"]:
                constants = {**size, **extra}
                label = ", ".join(f"{name} {value}" for name, value in constants.items())
                result = run(java, jar, model, constants, mutation, invariants)
                ok = result["violated"] == expected
                print(f"| {model} | {label} | {mutation} | {', '.join(invariants)} "
                      f"| {expected or 'holds'} | {result['violated'] or 'holds'}{'' if ok else ' (WRONG)'} "
                      f"| {result['states']} | {result['seconds']:.1f} |", flush=True)
                if not ok:
                    failed.append(f"{model} ({label}) {mutation}: expected {expected or 'holds'} ({why}), "
                                  f"TLC: {result['violated'] or 'holds'}; see {result['output']}")
    if failed:
        raise SystemExit("\n".join(failed))


if __name__ == "__main__":
    main()
