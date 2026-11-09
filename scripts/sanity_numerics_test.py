"""Tests of `sanity.py numerics`: which allows and expects of the integer
overflow lints it takes, and where, in small workspaces made for each test.

    python3 scripts/sanity_numerics_test.py [-k NAME]
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location("machopo_sanity_py", Path(__file__).resolve().parent / "sanity.py")
sanity = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = sanity
_spec.loader.exec_module(sanity)

LINTS = """
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,"""
WORKSPACE = """\
[workspace.lints.clippy]
undocumented_unsafe_blocks = "warn"
arithmetic_side_effects = "warn"
cast_possible_truncation = "warn"
cast_sign_loss = "warn"
cast_possible_wrap = "warn"
"""
TESTS_ROOT = f'#![cfg_attr(\n    test,\n    allow({LINTS}\n        reason = "tests"\n    )\n)]\n'
# What a crate allowed the lints with before it was converted: refused now.
WHOLE_CRATE = f'#![allow({LINTS}\n    reason = "not converted yet"\n)]\n'
INTEGRATION_ROOT = f'#![allow({LINTS}\n    reason = "tests"\n)]\n'


class Numerics(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.write("Cargo.toml", WORKSPACE)
        # A crate with a module and integration tests, and one with a binary.
        self.write("crates/done/Cargo.toml", '[package]\nname = "done"\n')
        self.write("crates/done/src/lib.rs", "//! Done.\n\n" + TESTS_ROOT + "\nmod util;\n")
        self.write(
            "crates/done/src/util.rs",
            """\
            #[expect(clippy::arithmetic_side_effects, reason = "i < len, so i + 1 <= len")]
            fn next(i: usize) -> usize {
                i + 1
            }
            """,
        )
        self.write("crates/done/tests/it/main.rs", INTEGRATION_ROOT + "\nmod more;\n")
        self.write("crates/done/tests/it/more.rs", "fn f() {}\n")
        self.write("crates/bin/Cargo.toml", '[package]\nname = "bin"\n[[bin]]\nname = "b"\npath = "src/main.rs"\n')
        self.write("crates/bin/src/lib.rs", TESTS_ROOT)
        self.write("crates/bin/src/main.rs", TESTS_ROOT + "fn main() {}\n")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, rel: str, text: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text), encoding="utf-8", newline="\n")

    def problems(self) -> list[str]:
        return sanity.numerics_problems(self.root)

    def assert_one(self, problems: list[str], where: str, says: str) -> None:
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(problems[0].startswith(where), problems)
        self.assertIn(says, problems[0])

    def test_the_two_lines_and_an_expect_with_its_reason(self):
        self.assertEqual(self.problems(), [])

    def test_the_lints_are_on(self):
        self.write("Cargo.toml", WORKSPACE.replace('cast_sign_loss = "warn"\n', ""))
        self.assert_one(self.problems(), "Cargo.toml", "cast_sign_loss")

    def test_an_allow_in_a_module(self):
        self.write(
            "crates/done/src/util.rs",
            '#[allow(clippy::cast_possible_truncation, reason = "fits")]\nfn f() {}\n',
        )
        self.assert_one(self.problems(), "crates/done/src/util.rs:1", "allows cast_possible_truncation")

    def test_an_allow_in_an_integration_test_module(self):
        self.write("crates/done/tests/it/more.rs", "#![allow(clippy::arithmetic_side_effects)]\n")
        self.assert_one(self.problems(), "crates/done/tests/it/more.rs:1", "allows arithmetic_side_effects")

    def test_an_expect_needs_a_reason(self):
        self.write("crates/done/src/util.rs", "#[expect(clippy::cast_sign_loss)]\nfn f() {}\n")
        self.assert_one(self.problems(), "crates/done/src/util.rs:1", "without a reason")
        self.write("crates/done/src/util.rs", '#[expect(clippy::cast_sign_loss, reason = " ")]\nfn f() {}\n')
        self.assert_one(self.problems(), "crates/done/src/util.rs:1", "without a reason")

    def test_no_crate_allows_them_whole(self):
        self.write("crates/done/src/lib.rs", WHOLE_CRATE + "mod util;\n")
        problems = self.problems()
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(problems[0].startswith("crates/done/src/lib.rs:1"), problems)
        self.assertIn("allows", problems[0])
        self.assertIn("crate root needs", problems[1])
        self.write("crates/done/src/lib.rs", TESTS_ROOT + "mod util;\n")
        self.write("crates/bin/src/main.rs", WHOLE_CRATE + "fn main() {}\n")
        problems = self.problems()
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(problems[0].startswith("crates/bin/src/main.rs:1"), problems)

    def test_each_root_has_its_line(self):
        self.write("crates/done/src/lib.rs", "mod util;\n")
        self.assert_one(self.problems(), "crates/done/src/lib.rs", 'reason = "tests"))]')
        self.write("crates/done/src/lib.rs", TESTS_ROOT + "mod util;\n")
        self.write("crates/done/tests/it/main.rs", "mod more;\n")
        self.assert_one(self.problems(), "crates/done/tests/it/main.rs", 'allow(<the lints>, reason = "tests")')
        self.write("crates/done/tests/it/main.rs", INTEGRATION_ROOT + "mod more;\n")
        self.write("crates/done/tests/own.rs", "fn f() {}\n")
        self.assert_one(self.problems(), "crates/done/tests/own.rs", "test root")

    def test_the_wrong_line_at_a_root(self):
        # The integration tests' line on a crate root: its `cfg(test)` would
        # not be all that the allow covers.
        self.write("crates/done/src/lib.rs", INTEGRATION_ROOT + "mod util;\n")
        problems = self.problems()
        self.assertEqual(len(problems), 2, problems)
        self.assertIn("allows", problems[0])
        self.assertIn("crate root needs", problems[1])

    def test_a_line_allows_all_four(self):
        self.write("crates/done/tests/it/main.rs", INTEGRATION_ROOT.replace("    clippy::cast_possible_wrap,\n", ""))
        self.assert_one(self.problems(), "crates/done/tests/it/main.rs:1", "also allow cast_possible_wrap")

    def test_comments_and_other_lints_are_not_attributes_of_its(self):
        self.write(
            "crates/done/src/util.rs",
            """\
            // #[allow(clippy::arithmetic_side_effects)]
            /// `#[allow(clippy::cast_sign_loss)]` in a doc comment.
            #[allow(clippy::too_many_arguments)]
            fn f() {}
            """,
        )
        self.assertEqual(self.problems(), [])


if __name__ == "__main__":
    unittest.main()
