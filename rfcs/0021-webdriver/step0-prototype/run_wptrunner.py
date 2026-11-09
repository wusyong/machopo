"""Runs wptrunner with the machopo product, without `wpt run` (RFC 0021's step 0).

`wpt run` checks the system's hosts file for every product but its built-in
ones, so this calls wptrunner directly, as Chromium's blinkpy does. Run it
with the venv's Python and PYTHONDONTWRITEBYTECODE set (not `-I`, which
ignores it), so nothing is written into the checkout:

    PYTHONDONTWRITEBYTECODE=1 target/step0/venv/Scripts/python run_wptrunner.py <wpt> <out> [wptrunner args...]
"""

import os
import sys


def main():
    wpt_root = os.path.abspath(sys.argv[1])
    out = os.path.abspath(sys.argv[2])
    sys.path.insert(0, wpt_root)
    sys.path.insert(0, os.path.join(wpt_root, "tools"))
    import localpaths  # noqa: F401  (WPT's third_party on sys.path)

    sys.path.insert(0, os.path.join(wpt_root, "tools", "wptrunner"))
    from wptrunner import wptcommandline, wptrunner

    os.makedirs(os.path.join(out, "metadata"), exist_ok=True)
    parser = wptcommandline.create_parser()
    kwargs = vars(parser.parse_args([
        "--product", "machopo",
        "--tests", wpt_root,
        "--metadata", os.path.join(out, "metadata"),
        "--manifest", os.path.join(out, "MANIFEST.json"),
        "--manifest-update",
        "--no-pause-after-test",
        "--log-raw", os.path.join(out, "raw.log"),
    ] + sys.argv[3:]))
    wptcommandline.check_args(kwargs)
    with wptrunner.GlobalLogger(kwargs, {"mach": sys.stdout}):
        return wptrunner.start(**kwargs)


# Windows starts multiprocessing's workers (the manifest's) by importing
# this file again: only the parent runs.
if __name__ == "__main__":
    sys.exit(0 if main() == 0 else 1)
