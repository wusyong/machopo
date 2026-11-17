"""Tests of how `features.py` reads what it joins: a `WEB_FEATURES.yml`
file's rules, a WPT test's expectations, and the state it gives a feature.

    python3 scripts/features_test.py [-k NAME]
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import features  # noqa: E402


class Rules(unittest.TestCase):
    def test_quoted_and_bare_patterns_in_order(self):
        rules, unparsed = features.parse_rules(
            'rules:\n- "*gap*": [flexbox-gap, flexbox]\n- "**": [flexbox]\n- font-size-*: [font-size]  # a comment\n'
        )
        self.assertEqual(
            rules, [("*gap*", ["flexbox-gap", "flexbox"]), ("**", ["flexbox"]), ("font-size-*", ["font-size"])]
        )
        self.assertEqual(unparsed, [])

    def test_a_list_over_two_lines(self):
        rules, unparsed = features.parse_rules("rules:\n- a.html: [margin,\n    width-height]\n- b.html: [margin]\n")
        self.assertEqual(rules, [("a.html", ["margin", "width-height"]), ("b.html", ["margin"])])
        self.assertEqual(unparsed, [])

    def test_an_anchor_and_its_aliases(self):
        rules, unparsed = features.parse_rules(
            "rules:\n- a.html: &id001 []\n- b.html: *id001\n- c.html: &id002 [x]\n- d.html: *id002\n"
        )
        self.assertEqual(rules, [("a.html", []), ("b.html", []), ("c.html", ["x"]), ("d.html", ["x"])])
        self.assertEqual(unparsed, [])

    def test_what_is_not_a_rule_is_reported(self):
        rules, unparsed = features.parse_rules("rules:\n- a.html:\n    ids: [x]\n- b.html: [y]\n")
        self.assertEqual(rules, [("b.html", ["y"])])
        self.assertEqual(len(unparsed), 1)


class Expectations(unittest.TestCase):
    def test_no_section_passes(self):
        self.assertEqual(features.ini_status("# only a comment\n"), "pass")

    def test_a_failing_test(self):
        self.assertEqual(features.ini_status("[a.html]\n  expected: FAIL\n"), "fail")

    def test_failing_subtests_are_partial(self):
        self.assertEqual(features.ini_status("[a.html]\n  [one]\n    expected: FAIL\n"), "partial")

    def test_a_known_intermittent_result_passes(self):
        self.assertEqual(features.ini_status("[a.html]\n  expected: [OK, TIMEOUT]\n  [one]\n    expected: [PASS, FAIL]\n"), "pass")

    def test_disabled(self):
        self.assertEqual(features.ini_status("[a.html]\n  disabled: no workers\n"), "disabled")

    def test_the_worker_variant_is_left_out(self):
        text = "[a.any.worker.html]\n  expected: ERROR\n\n[a.any.html]\n"
        self.assertEqual(features.ini_status(text), "pass")

    def test_only_a_worker_variant_counts(self):
        self.assertEqual(features.ini_status("[a.any.worker.html]\n  expected: ERROR\n"), "fail")

    def test_one_variant_of_two_failing_is_partial(self):
        text = "[a.html?x]\n  expected: ERROR\n\n[a.html?y]\n"
        self.assertEqual(features.ini_status(text), "partial")


class State(unittest.TestCase):
    def row(self, **fields) -> dict:
        row = {"prefixes": ["css"], "groups": ["css"], "wpt_total": 10, "wpt_pass": 0, "wpt_partial": 0,
               "wpt_fail": 0, "source_has": 0, "source_checked": 0}
        return row | fields

    def test_the_languages_own(self):
        self.assertEqual(features.state_of(self.row(prefixes=["javascript"])), "V8")
        self.assertEqual(features.state_of(self.row(prefixes=["webassembly"], groups=[])), "wasm")

    def test_nothing_to_go_by(self):
        self.assertEqual(features.state_of(self.row(wpt_total=0)), "no tests mapped")

    def test_missing_needs_something_to_look_for(self):
        self.assertEqual(features.state_of(self.row(source_checked=3)), "missing")
        self.assertEqual(features.state_of(self.row()), "unsure")

    def test_part_of_the_source(self):
        self.assertEqual(features.state_of(self.row(source_has=8, source_checked=10, wpt_pass=10)), "partial")
        self.assertEqual(features.state_of(self.row(source_has=9, source_checked=10, wpt_pass=10)), "mostly there")

    def test_by_the_tests_that_run(self):
        self.assertEqual(features.state_of(self.row(wpt_pass=4, wpt_fail=1)), "mostly there")
        self.assertEqual(features.state_of(self.row(wpt_pass=3, wpt_fail=2)), "partial")
        self.assertEqual(features.state_of(self.row(wpt_pass=3, wpt_partial=2, wpt_fail=0)), "mostly there")

    def test_few_tests(self):
        self.assertEqual(features.state_of(self.row(source_has=1, source_checked=1)), "few tests")
        self.assertEqual(features.state_of(self.row(wpt_pass=4)), "few tests")
        self.assertEqual(features.state_of(self.row(wpt_pass=1, wpt_fail=3)), "partial")


class Usage(unittest.TestCase):
    def test_the_features_own_counter_first(self):
        webstatus = {"usage": {"chrome": {"daily": 0.5}}}
        self.assertEqual(features.usage_of("x", ["css.properties.gap"], webstatus, {"gap": 0.9}, {}), (0.5, "webdx"))

    def test_the_highest_property(self):
        keys = ["css.properties.top", "css.properties.left", "css.properties.left.auto"]
        self.assertEqual(features.usage_of("x", keys, {"usage": {"chrome": {}}}, {"top": 0.2, "left": 0.3, "auto": 0.9}, {}),
                         (0.3, "css"))

    def test_a_counter_by_hand_then_the_members(self):
        counters = {"Fetch": 0.7, "V8Screen_Width_AttributeGetter": 0.4, "V8Screen_Constructor": 0.1}
        self.assertEqual(features.usage_of("fetch", ["api.fetch"], {}, {}, counters), (0.7, "hand"))
        self.assertEqual(features.usage_of("screen", ["api.Screen", "api.Screen.width"], {}, {}, counters), (0.4, "api"))
        self.assertEqual(features.usage_of("screen", ["api.Screen"], {}, {}, counters), (None, None))


if __name__ == "__main__":
    unittest.main()
