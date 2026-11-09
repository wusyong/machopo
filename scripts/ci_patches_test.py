"""Tests of `ci.py`'s fork patches: `patches` exports a fork's commits since
its upstream (`upstream_rev`), and `third-party` checks that those patches
on that commit give the pinned commit's files, in small repositories made
for each test (an upstream, a fork of it, and a checkout of the fork).

    python3 scripts/ci_patches_test.py [-k NAME]
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

_spec = importlib.util.spec_from_file_location("machopo_ci_py", Path(__file__).resolve().parent / "ci.py")
ci = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = ci
_spec.loader.exec_module(ci)

# Whatever the machine's configuration (Windows' core.autocrlf=true among
# it): the repositories hold the bytes the tests write.
CONFIG = ["-c", "core.autocrlf=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]


def git(cwd: Path, *args: str) -> str:
    out = subprocess.run(["git", *CONFIG, *args], cwd=cwd, capture_output=True, text=True)
    if out.returncode != 0:
        raise AssertionError(f"git {' '.join(args)}: {out.stderr}")
    return out.stdout.strip()


def commit(repo: Path, files: dict[str, bytes], message: str) -> str:
    for name, data in files.items():
        (repo / name).parent.mkdir(parents=True, exist_ok=True)
        (repo / name).write_bytes(data)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)
    return git(repo, "rev-parse", "HEAD")


def pin(rev: str, upstream_rev: str | None) -> ci.Pin:
    return ci.Pin("unused", rev, upstream_rev=upstream_rev)


class Patches(unittest.TestCase):
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        # The fork: upstream's commits, then the fork's own. A file with
        # CRLF line endings and a binary one: both must survive the patches.
        self.fork = self.root / "fork"
        self.fork.mkdir()
        git(self.fork, "init", "-q", "-b", "main")
        git(self.fork, "config", "core.autocrlf", "false")
        commit(self.fork, {"src/lib.rs": b"fn a() {}\n", "NOTES.txt": b"one\r\ntwo\r\n"}, "Upstream's first")
        self.base = commit(self.fork, {"src/lib.rs": b"fn a() {}\nfn b() {}\n"}, "Upstream's second")
        git(self.fork, "switch", "-q", "-c", "machopo")
        commit(self.fork, {"src/lib.rs": b"fn a() {}\nfn b() { /* machopo: */ }\n"}, "Change b")
        commit(self.fork, {"NOTES.txt": b"one\r\ntwo\r\nthree\r\n", "font.bin": bytes(range(256))}, "Add a font")
        self.head = commit(self.fork, {"README.machopo.md": b"# The fork\n"}, "Add README.machopo.md")
        self.patches = self.root / "patches"

    def problems(self, path: Path, rev: str, upstream_rev: str | None = None) -> list[str]:
        return ci.patch_problems("fork", path, pin(rev, upstream_rev or self.base), self.patches)

    def test_exported_patches_give_the_fork(self) -> None:
        files = ci.export_patches(self.fork, self.base, self.head, self.patches)
        self.assertEqual(
            [file.name for file in files],
            ["0001-Change-b.patch", "0002-Add-a-font.patch", "0003-Add-README.machopo.md.patch"],
        )
        first = files[0].read_text(encoding="utf-8")
        self.assertTrue(first.startswith("From 0000000000000000000000000000000000000000 "), first)
        self.assertIn("Subject: [PATCH] Change b", first)
        self.assertNotIn("\n-- \n", first)  # no signature
        self.assertIn(b"\r\n", files[1].read_bytes())  # the CRLF file's lines, as they are
        self.assertIn(b"GIT binary patch", files[1].read_bytes())
        # Whole blob ids, the same in a shallow checkout and a whole one.
        self.assertRegex(first, re.compile(r"^index [0-9a-f]{40}\.\.[0-9a-f]{40} ", re.M))
        self.assertEqual(self.problems(self.fork, self.head), [])

    def test_exporting_again_writes_the_same_bytes(self) -> None:
        first = [file.read_bytes() for file in ci.export_patches(self.fork, self.base, self.head, self.patches)]
        again = [file.read_bytes() for file in ci.export_patches(self.fork, self.base, self.head, self.patches)]
        self.assertEqual(first, again)

    def test_exporting_replaces_the_old_patches(self) -> None:
        self.patches.mkdir()
        (self.patches / "0009-Gone.patch").write_text("old", encoding="utf-8")
        files = ci.export_patches(self.fork, self.base, self.head, self.patches)
        self.assertEqual(len(files), 3)
        self.assertFalse((self.patches / "0009-Gone.patch").exists())

    def test_a_commit_not_exported_is_told(self) -> None:
        ci.export_patches(self.fork, self.base, self.head, self.patches)
        newer = commit(self.fork, {"src/lib.rs": b"fn a() {}\nfn b() {}\nfn c() {}\n"}, "Add c")
        problems = self.problems(self.fork, newer)
        self.assertEqual(len(problems), 1)
        self.assertIn("src/lib.rs", problems[0])
        self.assertIn("ci.py patches fork", problems[0])

    def test_a_patch_changed_by_hand_is_told(self) -> None:
        files = ci.export_patches(self.fork, self.base, self.head, self.patches)
        files[2].unlink()
        problems = self.problems(self.fork, self.head)
        self.assertEqual(len(problems), 1)
        self.assertIn("README.machopo.md", problems[0])

    def test_a_patch_that_doesnt_apply_is_named(self) -> None:
        files = ci.export_patches(self.fork, self.base, self.head, self.patches)
        files[0].write_bytes(files[0].read_bytes().replace(b"+fn b() { /* machopo: */ }", b"+fn b() { /* other */ }"))
        files[1].unlink()
        (self.patches / "0004-Broken.patch").write_bytes(files[0].read_bytes())
        problems = self.problems(self.fork, self.head)
        self.assertEqual(len(problems), 1)
        self.assertIn("0004-Broken.patch doesn't apply", problems[0])

    def test_the_checks_index_and_files_stay_as_they_are(self) -> None:
        ci.export_patches(self.fork, self.base, self.head, self.patches)
        (self.fork / "src" / "lib.rs").write_bytes(b"staged\n")
        git(self.fork, "add", "src/lib.rs")
        (self.fork / "src" / "lib.rs").write_bytes(b"work of its own\n")
        self.assertEqual(self.problems(self.fork, self.head), [])
        self.assertEqual((self.fork / "src" / "lib.rs").read_bytes(), b"work of its own\n")
        self.assertEqual(git(self.fork, "status", "--porcelain"), "MM src/lib.rs")
        self.assertEqual(git(self.fork, "show", ":src/lib.rs"), "staged")

    def test_no_patches_is_upstream_itself(self) -> None:
        self.assertEqual(self.problems(self.fork, self.base), [])
        self.assertEqual(len(self.problems(self.fork, self.head)), 1)

    def test_patches_without_an_upstream_rev_are_told(self) -> None:
        ci.export_patches(self.fork, self.base, self.head, self.patches)
        problems = ci.patch_problems("fork", self.fork, pin(self.head, None), self.patches)
        self.assertEqual(len(problems), 1)
        self.assertIn("no upstream_rev", problems[0])
        self.assertEqual(ci.patch_problems("fork", self.fork, pin(self.head, None), self.root / "none"), [])

    def test_a_shallow_checkout_fetches_upstream_rev(self) -> None:
        ci.export_patches(self.fork, self.base, self.head, self.patches)
        checkout = self.root / "checkout"
        git(self.root, "clone", "-q", "--depth", "1", "--branch", "machopo", self.fork.as_uri(), str(checkout))
        self.assertFalse(ci.has_commit(checkout, self.base))
        self.assertEqual(self.problems(checkout, self.head), [])
        self.assertTrue(ci.has_commit(checkout, self.base))
        self.assertEqual(git(checkout, "rev-parse", "--is-shallow-repository"), "true")
        # Only the commit, not upstream's history before it.
        self.assertFalse(ci.has_commit(checkout, f"{self.base}^"))

    def test_a_shallow_checkout_is_deepened_to_export(self) -> None:
        checkout = self.root / "checkout"
        git(self.root, "clone", "-q", "--depth", "1", "--branch", "machopo", self.fork.as_uri(), str(checkout))
        ci.fetch_commit(checkout, self.base)
        self.assertEqual(len(ci.export_patches(checkout, self.base, self.head, self.patches)), 3)
        self.assertEqual(self.problems(self.fork, self.head), [])

    def test_a_fork_not_on_upstream_rev_isnt_exported(self) -> None:
        git(self.fork, "switch", "-q", "--orphan", "elsewhere")
        other = commit(self.fork, {"other.txt": b"other\n"}, "Elsewhere")
        with self.assertRaises(SystemExit) as raised:
            ci.export_patches(self.fork, self.base, other, self.patches)
        self.assertIn("doesn't descend from upstream_rev", str(raised.exception))
        self.assertFalse(self.patches.exists())


class Commands(unittest.TestCase):
    """`ci.py patches` and `ci.py third-party` on a fork in `third_party/`,
    as `third_party.toml` would pin it."""

    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.addCleanup(scratch.cleanup)
        root = Path(scratch.name)
        self.third_party = root / "third_party"
        self.fork = self.third_party / "fork"
        self.fork.mkdir(parents=True)
        git(self.fork, "init", "-q", "-b", "main")
        git(self.fork, "config", "core.autocrlf", "false")
        self.base = commit(self.fork, {"src/lib.rs": b"fn a() {}\n"}, "Upstream's")
        self.head = commit(self.fork, {"src/lib.rs": b"fn a() { /* machopo: */ }\n"}, "Change a")
        self.patches = root / "patches"
        for name, value in {"THIRD_PARTY": self.third_party, "PATCHES": self.patches}.items():
            patcher = mock.patch.object(ci, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def pins(self, rev: str) -> mock._patch:
        return mock.patch.object(ci, "third_party_pins", return_value={"fork": pin(rev, self.base)})

    def run_command(self, command, **args) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = command(argparse.Namespace(**args))
        return code, out.getvalue()

    def test_patches_exports_at_the_pinned_commit(self) -> None:
        with self.pins(self.head):
            code, out = self.run_command(ci.cmd_patches, name="fork")
        self.assertEqual(code, 0, out)
        self.assertEqual([file.name for file in ci.patch_files(self.patches / "fork")], ["0001-Change-a.patch"])
        self.assertIn("1 patch in", out)

    def test_patches_wants_the_checkout_at_the_pin(self) -> None:
        with self.pins(self.base):
            code, out = self.run_command(ci.cmd_patches, name="fork")
        self.assertEqual(code, 1)
        self.assertIn("move the pin", out)
        self.assertFalse((self.patches / "fork").exists())

    def test_third_party_checks_the_patches(self) -> None:
        with self.pins(self.head):
            self.assertEqual(self.run_command(ci.cmd_patches, name="fork")[0], 0)
            code, out = self.run_command(ci.cmd_third_party)
        self.assertEqual(code, 0, out)
        newer = commit(self.fork, {"src/lib.rs": b"fn a() { /* newer */ }\n"}, "Change a again")
        with self.pins(newer):
            code, out = self.run_command(ci.cmd_third_party)
        self.assertEqual(code, 1)
        self.assertIn("ci.py patches fork", out)


if __name__ == "__main__":
    unittest.main()
