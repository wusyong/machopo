"""Runs WPT with wptrunner and the machopo product:

    python -m wptrunner_machopo.run --wpt WPT --out DIR --metadata DIR \\
        --machopo MACHOPO --machopodriver MACHOPODRIVER [--manifest FILE] [--include-file FILE] \\
        [--root URL_BASE TESTS METADATA ...] [--subsuite-file FILE] \\
        [--exclude PREFIX ...] [--test-types TYPE ...] [--processes N] \\
        [--timeout-multiplier N] [--this-chunk I --total-chunks N] \\
        [--machopo-arg ARG ...] [--update] [TEST ...]

`python3 scripts/test.py wpt` and `wdspec` run it in their venv. Not through
`wpt run`, which checks the system's hosts file for WPT's domains for every
product but its built-in ones: machopo resolves them itself. So it calls
wptrunner directly, as Chromium's blinkpy does. The logs go under `--out`,
the manifest to `--manifest`; Python mustn't write bytecode into the
checkout (`PYTHONDONTWRITEBYTECODE`, which `-I` would ignore).

The expected results are wptrunner's metadata (`.ini` files) under
`--metadata`. wptrunner reads a copy of them under `--out`: it keeps its
manifest's cache next to the metadata (`.cache/`, some 30 MB), which
doesn't belong in the repo. With `--update`, the run's results then go
into `--metadata` through wptrunner's own metadata update (`update.py`),
which keeps what it doesn't touch: conditions, `disabled`, comments.

`--root` adds a test root of our own beside WPT's, served at URL_BASE
(machopo's reftests at `/_machopo/`, as Servo's and Firefox's tests at
`/_mozilla/`), with its expectations; wptrunner reads the roots from a
config file this writes under `--out`.
"""

import argparse
import os
import shutil
import subprocess
import sys

# The most processes a run starts by default: each is wptrunner's worker, a
# machopodriver and a whole machopo (some 80 MB). Most of them wait, on a test's
# timeout or the next frame, so a run wants several per CPU; measured, 32
# were as fast with 8 cores as with 32 threads, and 4 cores topped out at
# about 16.
MAX_PROCESSES = 32


def default_processes():
    return min(MAX_PROCESSES, 4 * (os.cpu_count() or 1))


def wpt_commit(wpt):
    """The checkout's commit, which the manifest is built from."""
    out = subprocess.run(["git", "-C", wpt, "rev-parse", "HEAD"], capture_output=True, text=True)
    return out.stdout.strip() if out.returncode == 0 else None


def manifest_is_current(manifest, commit):
    """Whether the manifest was built from `commit`. Checking it against
    the whole checkout takes wptrunner some 10 s a run; the checkout only
    changes when its commit does, so a stamp next to the manifest says."""
    stamp = manifest + ".commit"
    if commit is None or not os.path.exists(manifest) or not os.path.exists(stamp):
        return False
    with open(stamp, encoding="utf-8") as f:
        return f.read().strip() == commit


def root_name(url_base):
    """A root's name in the config, and in its files' names: `/_machopo/` is
    `_machopo`."""
    return url_base.strip("/").replace("/", "-")


def root_manifest(args, url_base):
    return os.path.join(os.path.dirname(args.manifest), f"{root_name(url_base)}-MANIFEST.json")


def update_root_manifests(args):
    """Our roots' manifests, brought up to date: unlike the checkout, their
    tests change with the repo, but they are small (WPT's, checked every
    run, would take some 10 s)."""
    from manifest import manifest

    for url_base, tests, _ in args.root:
        path = root_manifest(args, url_base)
        cache = os.path.join(os.path.dirname(path), f"{root_name(url_base)}-cache")
        manifest.load_and_update(tests, path, url_base, update=True, cache_root=cache)


def write_config(args):
    """wptrunner's config with our roots, each with a copy of its
    expectations (wptrunner keeps a cache next to them)."""
    path = os.path.join(args.out, "config.ini")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for url_base, tests, metadata in args.root:
            name = root_name(url_base)
            copy = os.path.join(args.out, f"metadata-{name}")
            os.makedirs(copy, exist_ok=True)
            copy_metadata(metadata, copy)
            f.write(
                f"[manifest:{name}]\n"
                f"tests = {tests}\n"
                f"metadata = {copy}\n"
                f"manifest = {root_manifest(args, url_base)}\n"
                f"url_base = {url_base}\n\n"
            )
    return path


def wptrunner_kwargs(args, update_manifest):
    from wptrunner import wptcommandline

    certs = os.path.join(args.wpt, "tools", "certs")
    argv = [
        "--product", "machopo",
        "--binary", args.machopo,
        "--webdriver-binary", args.machopodriver,
        "--tests", args.wpt,
        "--metadata", args.metadata_copy,
        "--manifest", args.manifest,
        "--manifest-update" if update_manifest else "--no-manifest-update",
        "--no-pause-after-test",
        # One test a group, the processes taking the next from one queue: by
        # default each process gets a fixed share of the tests (by their
        # ids' hashes), and those that drew the tests that time out finish
        # last, measured at 10 to 51 s of work for 16 processes.
        "--fully-parallel",
        # Else every test, a group of its own, would start a new browser.
        "--no-restart-on-new-group",
        "--processes", str(args.processes),
        # Every failing reftest's screenshots go into the log, expected
        # failures' too (by default only the unexpected): `screenshots.py`
        # writes them out.
        "--reftest-screenshot", "fail",
        "--timeout-multiplier", str(args.timeout_multiplier),
        "--ssl-type", "pregenerated",
        "--ca-cert-path", os.path.join(certs, "cacert.pem"),
        "--host-key-path", os.path.join(certs, "web-platform.test.key"),
        "--host-cert-path", os.path.join(certs, "web-platform.test.pem"),
        "--log-raw", os.path.join(args.out, "raw.log"),
        "--log-wptreport", os.path.join(args.out, "wptreport.json"),
        "--test-types", *args.test_types,
    ]
    # machopodriver's `--binary-arg`, each a word of its own.
    for arg in args.machopo_arg:
        argv += ["--webdriver-arg=--binary-arg", f"--webdriver-arg={arg}"]
    if args.include_file:
        argv += ["--include-file", args.include_file]
    if args.root:
        argv += ["--config", write_config(args)]
    if args.subsuite_file:
        argv += ["--subsuite-file", args.subsuite_file]
    for prefix in args.exclude:
        argv += ["--exclude", prefix]
    if args.total_chunks > 1:
        # By directory: a chunk's tests are whole directories, the same ones
        # every run, so a chunk's failures point at the directories it holds.
        argv += [
            "--this-chunk", str(args.this_chunk),
            "--total-chunks", str(args.total_chunks),
            "--chunk-type", "dir_hash",
        ]
    # The tests first: `--test-types` takes a list.
    kwargs = vars(wptcommandline.create_parser().parse_args(args.tests + argv))
    wptcommandline.check_args(kwargs)
    return kwargs


def copy_metadata(metadata, copy):
    """The `.ini` files of `metadata` into `copy`, which has no others;
    the cache wptrunner keeps there stays."""
    for root, dirs, files in os.walk(copy):
        dirs[:] = [d for d in dirs if d != ".cache"]
        for name in files:
            if name.endswith(".ini"):
                os.remove(os.path.join(root, name))
    for root, dirs, files in os.walk(metadata):
        for name in files:
            if name.endswith(".ini"):
                target = os.path.join(copy, os.path.relpath(os.path.join(root, name), metadata))
                os.makedirs(os.path.dirname(target), exist_ok=True)
                shutil.copyfile(os.path.join(root, name), target)


def update(args):
    """The run's results into `--metadata`, through `update.py` in a process
    of its own: once a run ends, wptrunner has shut its logger down, and
    the metadata update logs through one of the same name. (Not through
    `wptrunner_kwargs` either: mozlog opens `--log-raw` as it parses the
    arguments, emptying the log.)"""
    cmd = [
        sys.executable, "-m", "wptrunner_machopo.update",
        "--wpt", args.wpt,
        "--metadata", args.metadata,
        "--manifest", args.manifest,
        os.path.join(args.out, "raw.log"),
    ]
    for url_base, tests, metadata in args.root:
        cmd += ["--root", url_base, tests, metadata, root_manifest(args, url_base)]
    return subprocess.run(cmd).returncode


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--wpt", required=True, help="the WPT checkout")
    parser.add_argument("--out", required=True, help="where the logs go")
    parser.add_argument("--metadata", required=True, help="the expected results (.ini files)")
    parser.add_argument("--manifest", help="WPT's manifest (default: MANIFEST.json under --out)")
    parser.add_argument("--machopo", required=True, help="the machopo binary")
    parser.add_argument("--machopodriver", required=True, help="the machopodriver binary")
    parser.add_argument("--include-file", help="the tests to run, one path per line")
    parser.add_argument(
        "--machopo-arg", action="append", default=[], help="an argument for machopo, once per argument"
    )
    parser.add_argument(
        "--root", nargs=3, action="append", default=[], metavar=("URL_BASE", "TESTS", "METADATA"),
        help="a test root of our own, served at URL_BASE, with its expectations",
    )
    parser.add_argument("--subsuite-file", help="wptrunner's subsuites (JSON): tests run with other browser arguments")
    parser.add_argument(
        "--exclude", action="append", default=[], metavar="PREFIX",
        help="tests not to run (but in a subsuite that names them)",
    )
    parser.add_argument("--test-types", nargs="+", default=["wdspec"], help="default: wdspec")
    parser.add_argument(
        "--processes", type=int, default=default_processes(),
        help=f"tests at a time (default: 4 per CPU, at most {MAX_PROCESSES}: {default_processes()} here)",
    )
    parser.add_argument("--timeout-multiplier", type=float, default=1, help="each test's timeout times this")
    parser.add_argument("--this-chunk", type=int, default=1, metavar="I", help="run the I-th chunk of the tests")
    parser.add_argument("--total-chunks", type=int, default=1, metavar="N", help="of N, by directory")
    parser.add_argument("--update", action="store_true", help="write the results as the expected ones")
    parser.add_argument("tests", nargs="*", help="only these tests (paths in the checkout)")
    args = parser.parse_args(argv)
    args.wpt, args.out, args.metadata = (os.path.abspath(p) for p in (args.wpt, args.out, args.metadata))
    args.manifest = os.path.abspath(args.manifest or os.path.join(args.out, "MANIFEST.json"))
    args.root = [(url_base, os.path.abspath(tests), os.path.abspath(metadata)) for url_base, tests, metadata in args.root]
    os.makedirs(args.out, exist_ok=True)
    args.metadata_copy = os.path.join(args.out, "metadata")
    # wptrunner wants it there, `.ini` files or none.
    os.makedirs(args.metadata_copy, exist_ok=True)
    copy_metadata(args.metadata, args.metadata_copy)

    commit = wpt_commit(args.wpt)
    update_manifest = not manifest_is_current(args.manifest, commit)

    sys.path[:0] = [args.wpt, os.path.join(args.wpt, "tools"), os.path.join(args.wpt, "tools", "wptrunner")]
    import localpaths  # noqa: F401  (WPT's third_party on sys.path)
    from wptrunner import wptrunner

    update_root_manifests(args)
    kwargs = wptrunner_kwargs(args, update_manifest)
    with wptrunner.GlobalLogger(kwargs, {"mach": sys.stdout}):
        status = wptrunner.start(**kwargs)
    if update_manifest and commit is not None and os.path.exists(args.manifest):
        with open(args.manifest + ".commit", "w", encoding="utf-8", newline="\n") as f:
            f.write(commit + "\n")
    from wptrunner_machopo import screenshots

    shots = os.path.join(args.out, "screenshots")
    if count := screenshots.write(os.path.join(args.out, "raw.log"), shots):
        print(f"the screenshots of {count} failing reftests are under {shots}")
    if args.update:
        return update(args)
    return status


# Windows starts multiprocessing's workers (the manifest's) by importing
# this module again: only the parent runs.
if __name__ == "__main__":
    sys.exit(0 if main() == 0 else 1)
