"""Prototype (not merged): `bench.py speedometer --sample` on Windows with
samply, to see what its profile has. Run from an elevated terminal:

    python rfcs\\0066-windows-sampling\\prototype\\sample.py [SUITE] [ITERATIONS]

It runs `speedometer.sample_suites` with samply in place of `perf` and
keeps everything it recorded in target/bench/speedometer/sample/ (the
profile, its symbols, the ETL, the trace), plus `clocks.json`: the wall
clock less QueryPerformanceCounter before and after, for lining the
samples up with the trace afterwards."""

import ctypes
import gzip
import json
import struct
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts" / "bench" / "speedometer"))
import speedometer  # noqa: E402
import common  # noqa: E402

FLAGS = "--enable-etw-stack-walking --interpreted-frames-native-stack"
# Built beforehand with the PDB's absolute path in the exe, which samply
# needs to find it (rustc's /PDBALTPATH:%_PDB% leaves only its name):
#   cargo build --profile perf -p shell --bin machopo --config
#     "target.x86_64-pc-windows-msvc.rustflags=['-C','link-arg=/PDBALTPATH:<repo>\target\perf\machopo.pdb']"
BINARY = ROOT / "target" / "perf" / "machopo.exe"
WINDOWS = common.Sampling("samply", ("cpu",), ".samply.json.gz", "samply on Windows (prototype)")


def clocks() -> dict:
    return {"wall_ns": time.time_ns(), "qpc_ns": time.perf_counter_ns(),
            "wall_minus_qpc_ns": time.time_ns() - time.perf_counter_ns()}


def sampler(samples: Path):
    samples.unlink(missing_ok=True)
    common.samply_symbols(samples).unlink(missing_ok=True)
    return (["samply", "record", "--save-only", "--no-open", "--unstable-presymbolicate", "--browsers", "--keep-etl",
             "-r", str(common.SAMPLY_RATE), "-o", str(samples), "--"], FLAGS)


def etl_start_ns(etl: Path) -> int:
    """The QPC (as ns) of the kernel ETL's header event, which samply's
    sample times count from: the first event after the 72-byte buffer
    header, a SYSTEM_TRACE_HEADER, its timestamp at byte 16; the
    TRACE_LOGFILE_HEADER's PerfFreq after it."""
    data = etl.read_bytes()[:4096]
    raw = struct.unpack_from("<q", data, 72 + 16)[0]
    # BootTime, PerfFreq after the logger and file names' pointers and the
    # TIME_ZONE_INFORMATION, aligned to 8.
    boot = (72 + 32 + 4 * 4 + 8 + 4 * 8 + 2 * 8 + 172 + 7) // 8 * 8
    freq = struct.unpack_from("<q", data, boot + 8)[0]
    return raw * 1_000_000_000 // freq


def read_samples(samples: Path, thread: str):
    profile = json.loads(gzip.decompress(samples.read_bytes()))
    etl = samples.with_name(samples.name.removesuffix(".json.gz") + ".kernel.etl")
    offset = round(profile["meta"]["startTime"] * 1_000_000) - etl_start_ns(etl)
    print("  samply's startTime less the ETL's start, as QPC (ms):",
          (profile["meta"]["startTime"] * 1_000_000 - (time.time_ns() - time.perf_counter_ns()) - etl_start_ns(etl)) / 1e6)
    symbols_file = common.samply_symbols(samples)
    symbols = json.loads(symbols_file.read_text(encoding="utf-8")) if symbols_file.exists() else {}
    print("  threads:", sorted({t["name"] for t in profile["threads"]}))
    return speedometer.read_samply_profile(profile, symbols, thread, offset)


def main() -> int:
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("run this from an elevated terminal: ETW needs Administrator")
        return 1
    suite = sys.argv[1] if len(sys.argv) > 1 else "TodoMVC-jQuery"
    iterations = int(sys.argv[2]) if len(sys.argv) > 2 else 5
    common.sampler = sampler
    speedometer.sampling = lambda: WINDOWS
    speedometer.read_samples = read_samples
    folder = speedometer.RESULTS / "sample"
    before = clocks()
    rows, failed = speedometer.sample_suites(BINARY, [suite], folder, iterations)
    after = clocks()
    (folder / "clocks.json").write_text(json.dumps({"before": before, "after": after}, indent=2),
                                        encoding="utf-8", newline="\n")
    if rows:
        print(f"the functions most samples are in: {speedometer.write_sample_report(rows)}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
