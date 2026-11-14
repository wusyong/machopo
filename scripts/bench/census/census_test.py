#!/usr/bin/env python3
"""Tests of `bench.py census` (`census.py`): how a callback is named, the
pages' counts summed, and the report.

    python3 scripts/bench/census/census_test.py
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import census  # noqa: E402

HEADER = "entry\tinterface\tmember\tkind\tcalls\t" + "\t".join(census.MADE + census.WITH) + "\n"
MEMBER = "dom_js::templates::InfoMember<dom_api::interfaces::typed_paths::Element::M<3>>"


def row(entry: str, label: tuple[str, str, str], calls: int, made: list[int], with_: list[int]) -> str:
    return "\t".join([entry, *label, str(calls), *map(str, made), *map(str, with_)]) + "\n"


class CensusTests(unittest.TestCase):
    def test_a_member_is_named_by_its_label_and_another_callback_by_its_type(self):
        self.assertEqual(
            census.callback_name({"entry": MEMBER, "interface": "Element", "member": "id", "kind": "getter"}),
            ("InfoMember", "Element.id (getter)"))
        self.assertEqual(
            census.callback_name({"entry": "dom_js::templates::Indexed<dom_api::typed::NodeList::M<1>>",
                                  "interface": "", "member": "", "kind": ""}),
            ("Indexed", "Indexed<M<1>>"))
        self.assertEqual(
            census.callback_name({"entry": census.OUTSIDE, "interface": "", "member": "", "kind": ""}),
            (census.OUTSIDE, census.OUTSIDE))

    def test_the_pages_counts_are_summed_by_callback(self):
        with tempfile.TemporaryDirectory() as folder:
            a = row(MEMBER, ("Element", "id", "getter"), 10, [0, 0, 0, 10, 0, 2], [0, 0, 0, 10, 0, 2])
            b = row(MEMBER, ("Element", "id", "getter"), 5, [1, 0, 0, 5, 0, 0], [1, 0, 0, 5, 0, 0])
            Path(folder, "a.tsv").write_text(HEADER + a, encoding="utf-8")
            Path(folder, "b.tsv").write_text(HEADER + b, encoding="utf-8")
            sums = census.read(Path(folder))
        counts = sums[("InfoMember", "Element.id (getter)")]
        self.assertEqual((counts["calls"], counts["scopes"], counts["reads"], counts["roots"], counts["with_root"]),
                         (15, 1, 15, 2, 2))

    def test_the_report_gives_the_share_of_calls_and_ranks_the_callbacks(self):
        with tempfile.TemporaryDirectory() as folder:
            rows = (row(MEMBER, ("Element", "id", "getter"), 30, [0, 0, 0, 30, 0, 0], [0, 0, 0, 30, 0, 0])
                    + row(MEMBER, ("Element", "classList", "getter"), 10, [10, 0, 0, 30, 0, 10],
                          [10, 0, 0, 10, 0, 10]))
            Path(folder, "a.tsv").write_text(HEADER + rows, encoding="utf-8")
            text = census.report(census.read(Path(folder)), "speedometer", 1)
        self.assertIn("40 calls.", text)
        self.assertIn("| roots | 10 | 10 | 25.0% |", text)
        most_roots = text.split("## The most roots")[1].split("##")[0]
        self.assertIn("Element.classList (getter)", most_roots)
        # Only the first is listed, and one that made none never is.
        self.assertNotIn("Element.id", most_roots)
        self.assertEqual(text.split("## The most trampoline_scopes")[1].split("##")[0].count("InfoMember"), 0)


if __name__ == "__main__":
    unittest.main()
