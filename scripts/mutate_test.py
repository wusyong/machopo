"""Tests of `test.py mutate`'s originals: where the original of a file a
spec breaks waits while it is broken, in the repo or in a project next to
it (`../third_party/sunaba`), and that `--restore` puts each back where it
was, in a repo and its neighbours made for each test.

    python3 scripts/mutate_test.py [-k NAME]
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

_spec = importlib.util.spec_from_file_location("machopo_test_py", Path(__file__).resolve().parent / "test.py")
test_py = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = test_py
_spec.loader.exec_module(test_py)


class Originals(unittest.TestCase):
    """A repo (`m/machopo`) with a file of its own, and a project next to it
    (`m/third_party/sunaba`) with one of its own, in a directory of the
    test's: `ROOT` and the originals' directory point there."""

    def setUp(self) -> None:
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        top = Path(self.dir.name).resolve() / "m"
        self.root = top / "machopo"
        self.own = self.root / "crates" / "shell" / "src" / "lib.rs"
        self.next_door = top / "third_party" / "sunaba" / "src" / "linux.rs"
        for path, text in ((self.own, "fn own() {}\n"), (self.next_door, "fn next_door() {}\n")):
            path.parent.mkdir(parents=True)
            path.write_text(text, encoding="utf-8")
        originals = self.root / "target" / "mutate" / "originals"
        for name, value in (("ROOT", self.root), ("MUTATE_ORIGINALS", originals)):
            patcher = mock.patch.object(test_py, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.originals = originals

    def spec(self, file: str, check: str, to: str, seen: Path | None = None) -> Path:
        """A spec of one mutant of `file`, tested by a Python one-liner that
        fails when `check` is no longer in it; with `seen`, it first writes
        there whether the original waits where `saved_original` says."""
        spec = self.root / "mutants.toml"
        script = f"import sys; sys.exit({check!r} not in open({file!r}).read())"
        if seen is not None:
            waiting = str(test_py.saved_original(file))
            script = f"import os; open({str(seen)!r}, 'w').write(str(os.path.exists({waiting!r}))); " + script
        command = [sys.executable, "-c", script]
        spec.write_text(
            # A TOML array of basic strings is JSON's.
            f"test = {json.dumps(command)}\n"
            + f"[[mutant]]\nname = \"broken\"\nfile = \"{file}\"\nfrom = '''{check}'''\nto = '''{to}'''\n",
            encoding="utf-8",
        )
        return spec

    def test_a_file_of_the_repo_waits_by_its_path_in_it(self) -> None:
        saved = test_py.saved_original("crates/shell/src/lib.rs")
        self.assertEqual(saved, self.originals / "crates" / "shell" / "src" / "lib.rs")
        self.assertEqual(test_py.original_home(saved), self.own)

    def test_a_file_next_to_the_repo_waits_inside_the_originals_by_its_path_from_their_parent(self) -> None:
        saved = test_py.saved_original("../third_party/sunaba/src/linux.rs")
        self.assertEqual(saved, self.originals / "_outside" / "third_party" / "sunaba" / "src" / "linux.rs")
        self.assertTrue(saved.is_relative_to(self.originals))
        self.assertEqual(test_py.original_home(saved), self.next_door)

    def test_restore_puts_back_what_a_run_that_was_killed_left_broken_in_and_next_to_the_repo(self) -> None:
        # As a run leaves them when killed with a mutant written: the
        # original saved, the file broken.
        for file, path in (("crates/shell/src/lib.rs", self.own), ("../third_party/sunaba/src/linux.rs", self.next_door)):
            saved = test_py.saved_original(file)
            saved.parent.mkdir(parents=True, exist_ok=True)
            saved.write_bytes(path.read_bytes())
            path.write_text("broken\n", encoding="utf-8")
        restored = test_py.restore_originals()
        self.assertEqual(sorted(restored), ["../third_party/sunaba/src/linux.rs", "crates/shell/src/lib.rs"])
        self.assertEqual(self.own.read_text(encoding="utf-8"), "fn own() {}\n")
        self.assertEqual(self.next_door.read_text(encoding="utf-8"), "fn next_door() {}\n")
        self.assertFalse(self.originals.exists())

    def test_a_run_breaks_a_file_next_to_the_repo_and_puts_it_back(self) -> None:
        seen = self.root / "seen"
        spec = self.spec("../third_party/sunaba/src/linux.rs", "fn next_door", "fn elsewhere", seen)
        args = argparse.Namespace(restore=False, spec=str(spec), only=None, keep_going=False)
        with contextlib.redirect_stdout(io.StringIO()) as out:
            status = test_py.cmd_mutate(args)
        self.assertEqual(status, 0, out.getvalue())
        self.assertIn("caught: broken", out.getvalue())
        # While it was broken, its original waited inside the originals.
        self.assertEqual(seen.read_text(encoding="utf-8"), "True")
        self.assertEqual(self.next_door.read_text(encoding="utf-8"), "fn next_door() {}\n")
        self.assertFalse(self.originals.exists())

    def test_a_spec_naming_a_file_beyond_the_repos_neighbours_is_refused_before_anything_is_touched(self) -> None:
        outside = Path(self.dir.name).resolve() / "elsewhere.rs"
        outside.write_text("fn elsewhere() {}\n", encoding="utf-8")
        spec = self.spec("../../elsewhere.rs", "fn elsewhere", "fn broken")
        with self.assertRaisesRegex(ValueError, "outside the repo"):
            test_py.load_mutants(spec)
        self.assertEqual(outside.read_text(encoding="utf-8"), "fn elsewhere() {}\n")


if __name__ == "__main__":
    unittest.main()
