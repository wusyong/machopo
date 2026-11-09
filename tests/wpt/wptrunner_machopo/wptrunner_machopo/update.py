"""Writes a run's results into the expected ones, with wptrunner's own
metadata update (what `wpt update-expectations` runs):

    python -m wptrunner_machopo.update --wpt WPT --metadata DIR --manifest FILE LOG

`run.py --update` runs it after the run, in a process of its own. Each
test's expectations become the results in LOG (wptrunner's raw log): a
failing subtest gets its status, a test that now passes whole loses its
`.ini`. What the update doesn't touch stays: conditions on other platforms
(only `os` is a property, and it isn't a full update, which would rebuild
every condition from this one log), `disabled`, comments and `bug`.
"""

import argparse
import os
import sys


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--wpt", required=True, help="the WPT checkout")
    parser.add_argument("--metadata", required=True, help="the expected results (.ini files) to update")
    parser.add_argument("--manifest", required=True, help="WPT's manifest")
    parser.add_argument(
        "--root", nargs=4, action="append", default=[], metavar=("URL_BASE", "TESTS", "METADATA", "MANIFEST"),
        help="a test root of our own, with its expectations to update",
    )
    parser.add_argument("log", help="the run's raw log")
    args = parser.parse_args(argv)

    sys.path[:0] = [args.wpt, os.path.join(args.wpt, "tools"), os.path.join(args.wpt, "tools", "wptrunner")]
    import localpaths  # noqa: F401  (WPT's third_party on sys.path)
    from mozlog import formatters, handlers
    from wptrunner import metadata
    from wptrunner.wptcommandline import TestRoot

    metadata.logger.add_handler(handlers.StreamHandler(sys.stdout, formatters.MachFormatter()))
    test_paths = {"/": TestRoot(args.wpt, args.metadata, args.manifest)}
    for url_base, tests, expectations, manifest in args.root:
        test_paths[url_base] = TestRoot(tests, expectations, manifest)
    metadata.update_expected(test_paths, [args.log], update_properties=(["os"], {}), full_update=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
