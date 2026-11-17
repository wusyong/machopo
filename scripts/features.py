#!/usr/bin/env python3
"""Which web platform features machopo lacks, by how much of the web uses
them: public data per web-features ID, joined with what machopo has.

    python3 scripts/features.py            # writes target/features/priority.md and features.csv
    python3 scripts/features.py --refresh  # the same, downloading the public data again

The public data (kept in `target/features/cache/` once downloaded):

- web-features (https://github.com/web-platform-dx/web-features): the
  features, their Baseline status and the browser-compat-data keys of each.
- webstatus.dev: the share of Chrome's page loads that use each feature,
  where Chromium counts it.
- chromestatus.com: the same per CSS property and per counted API member,
  for the features webstatus.dev has no number for.

What machopo has:

- WPT: the tests the checkout's `WEB_FEATURES.yml` files map to each feature
  (`../third_party/wpt`, read and never written), against
  `tests/wpt/include.txt` and the expectations in `tests/wpt/metadata/`. A
  test that isn't included counts as not passing, and the list is picked file
  by file: a low share alone doesn't say a feature is missing.
- The source: the CSS properties `crates/css/src/properties.rs` parses, the
  interfaces and members `crates/dom_api/src/interfaces.rs` declares, and,
  for selectors, at-rules and elements, whether the name is a string literal
  in the sources (which can be wrong either way).

Standard library only. Exits non-zero on failure.
"""

from __future__ import annotations

import argparse
import csv
import datetime
import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from fnmatch import fnmatchcase
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WPT = ROOT.parent / "third_party" / "wpt"
OUT = ROOT / "target" / "features"
CACHE = OUT / "cache"

WEBSTATUS = "https://api.webstatus.dev/v1/features"
SOURCES = {
    "web-features.json": "https://unpkg.com/web-features/data.json",
    "css-popularity.json": "https://chromestatus.com/data/csspopularity",
    "feature-popularity.json": "https://chromestatus.com/data/featurepopularity",
}

# Old features with no web-features counter in Chromium, whose older counter
# is unambiguous by its name. Picked by hand.
COUNTERS_BY_HAND = {
    "fetch": "Fetch",
    "xhr": "XMLHttpRequestAsynchronous",
    "websockets": "WebSocket",
    "localstorage": "DOMStorageRead",
    "sessionstorage": "DOMStorageRead",
    "details": "DetailsElement",
}

# What the `selectors` crate parses and matches by itself: machopo's sources
# have no name for these to be found by.
SELECTORS_CRATE = {
    "not", "is", "where", "has", "root", "empty", "first-child", "last-child", "only-child", "nth-child",
    "nth-last-child", "first-of-type", "last-of-type", "only-of-type", "nth-of-type", "nth-last-of-type", "host",
    "slotted",
}

# --- the public data ----------------------------------------------------------


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "feature-list-script"})
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


def download(refresh: bool) -> None:
    """Puts each data set in the cache, unless it is there already."""
    CACHE.mkdir(parents=True, exist_ok=True)
    for name, url in SOURCES.items():
        path = CACHE / name
        if refresh or not path.exists():
            print(f"downloading {url}")
            path.write_bytes(fetch(url))
    path = CACHE / "webstatus.json"
    if refresh or not path.exists():
        print(f"downloading {WEBSTATUS}")
        features: list[dict] = []
        token = None
        while True:
            query = {"page_size": "100"} | ({"page_token": token} if token else {})
            page = json.loads(fetch(WEBSTATUS + "?" + urllib.parse.urlencode(query)))
            features.extend(page["data"])
            token = page.get("metadata", {}).get("next_page_token")
            if not token:
                break
        path.write_text(json.dumps(features), encoding="utf-8", newline="\n")


def load(name: str):
    return json.loads((CACHE / name).read_text(encoding="utf-8"))


def current(entries: list[dict]) -> dict[str, float]:
    """chromestatus's counters by name, without the ones Chromium stopped
    reporting: those keep the share of their last day."""
    newest = max(datetime.date.fromisoformat(e["date"]) for e in entries)
    since = (newest - datetime.timedelta(days=30)).isoformat()
    return {e["property_name"]: e["day_percentage"] for e in entries if e["date"] >= since}


# --- WPT's tests of each feature ----------------------------------------------

RULE = re.compile(r"""^- (?:"([^"]+)"|'([^']+)'|([^:\s]+)):\s*(.*)$""")


def parse_rules(text: str) -> tuple[list[tuple[str, list[str]]], list[str]]:
    """A `WEB_FEATURES.yml` file's rules, `(pattern, feature IDs)` in order,
    and the lines that aren't one. The files hold a `rules:` list of one-key
    mappings to a flow list of IDs, which may go on over indented lines and
    may be a YAML anchor's (`&id001 [...]`, then `*id001`)."""
    rules: list[tuple[str, list[str]]] = []
    unparsed: list[str] = []
    anchors: dict[str, list[str]] = {}
    lines: list[str] = []
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#") or line.strip() == "rules:":
            continue
        if line[0].isspace() and lines:
            lines[-1] += " " + line.strip()
        else:
            lines.append(line.rstrip())
    for line in lines:
        m = RULE.match(line)
        value = re.sub(r"\s+#.*$", "", m.group(4)).strip() if m else ""
        anchor = None
        if value.startswith("&"):
            anchor, _, value = value[1:].partition(" ")
            value = value.strip()
        if value.startswith("*") and value[1:] in anchors:
            ids = anchors[value[1:]]
        elif value.startswith("[") and value.endswith("]"):
            ids = [i.strip() for i in value[1:-1].split(",") if i.strip()]
        else:
            unparsed.append(line)
            continue
        if anchor:
            anchors[anchor] = ids
        rules.append((m.group(1) or m.group(2) or m.group(3), ids))
    return rules, unparsed


# The kinds of test wptrunner runs by itself.
RUNNABLE = {"testharness", "reftest", "crashtest", "print-reftest"}
# At the checkout's root, what holds no test of the platform: WPT's own
# tools (and their tests), and what `npm` leaves.
SKIPPED_ROOTS = {"tools", "node_modules"}


def wpt_feature_map() -> dict[str, list[str]]:
    """Each feature's test files in the WPT checkout, as
    `tools/web_features/manifest.py` there maps them: a directory's
    `WEB_FEATURES.yml` replaces what it inherited, its `**` rule goes on
    into the directories below, its other rules match file names. Kept in
    the cache by the checkout's commit: reading every file takes a while."""
    rev = subprocess.run(
        ["git", "-C", str(WPT), "rev-parse", "HEAD"], check=True, capture_output=True, text=True
    ).stdout.strip()
    cached = CACHE / f"wpt-features-{rev}.json"
    if cached.exists():
        return json.loads(cached.read_text(encoding="utf-8"))
    print(f"mapping the tests in {WPT} to features (about a minute)")
    # WPT's own reading of what is a test, and of which kind. The checkout
    # is upstream's: nothing may be written into it, bytecode included.
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(WPT))
    from tools import localpaths  # noqa: F401  (puts WPT's vendored packages on the path)
    from tools.manifest.sourcefile import SourceFile

    result: dict[str, set[str]] = {}
    unparsed: list[str] = []

    def add(ids: list[str], files: list[str]) -> None:
        for feature in ids:
            result.setdefault(feature, set()).update(files)

    def walk(rel: str, inherited: list[str]) -> None:
        directory = WPT / rel
        names = sorted(os.listdir(directory))
        tests = []
        for name in names:
            if not (directory / name).is_file():
                continue
            source = SourceFile(str(WPT), os.path.join(rel, name), "/")
            if source.name_is_non_test:
                continue
            try:
                kind = source.type
            except ModuleNotFoundError:
                # test262's files need PyYAML to be read: the tests of the
                # JS language, which is V8's.
                continue
            if kind in RUNNABLE:
                tests.append(name)
        inherited = list(inherited)
        yml = directory / "WEB_FEATURES.yml"
        if yml.is_file():
            inherited = []
            rules, bad = parse_rules(yml.read_text(encoding="utf-8"))
            unparsed.extend(f"{rel}/WEB_FEATURES.yml: {line}" for line in bad)
            for pattern, ids in rules:
                if pattern == "**":
                    inherited.extend(ids)
                    add(ids, [f"{rel}/{name}" for name in tests])
                elif "*" in pattern:
                    add(ids, [f"{rel}/{name}" for name in tests if fnmatchcase(name, pattern)])
                elif pattern in tests:
                    add(ids, [f"{rel}/{pattern}"])
        else:
            add(inherited, [f"{rel}/{name}" for name in tests])
        for name in names:
            if not name.startswith(".") and (directory / name).is_dir():
                walk(f"{rel}/{name}", inherited)

    for top in sorted(os.listdir(WPT)):
        if not top.startswith(".") and top not in SKIPPED_ROOTS and (WPT / top).is_dir():
            walk(top, [])
    if unparsed:
        print(f"{len(unparsed)} rules not understood, left out:", *unparsed, sep="\n  ")
    mapping = {feature: sorted(files) for feature, files in sorted(result.items())}
    cached.write_text(json.dumps(mapping), encoding="utf-8", newline="\n")
    return mapping


# --- machopo: which of those tests pass ---------------------------------------

# The statuses that aren't a pass, for a test or a subtest.
NOT_PASSING = {"FAIL", "ERROR", "TIMEOUT", "CRASH", "NOTRUN", "PRECONDITION_FAILED"}


def ini_status(text: str) -> str:
    """What a test file's `.ini` expects of it: `pass`, `partial` (some
    subtests or variants don't pass), `fail` (none does) or `disabled`. A
    known intermittent result (`[PASS, FAIL]`) counts as passing. The worker
    variants of a `.any.js` test are left out when it has a window's: they
    say nothing about the feature in a document."""
    sections: list[dict] = []
    section = None
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent == 0 and stripped.startswith("["):
            section = {"name": stripped[1:-1], "test": False, "subtest": False, "disabled": False}
            sections.append(section)
        elif section is None:
            continue
        elif indent == 2 and stripped.startswith("disabled:"):
            section["disabled"] = True
        elif stripped.startswith("expected:"):
            if stripped.split(":", 1)[1].strip() in NOT_PASSING:
                section["test" if indent == 2 else "subtest"] = True
    if not sections:
        return "pass"
    sections = [s for s in sections if "worker" not in s["name"]] or sections
    live = [s for s in sections if not s["disabled"]]
    if not live:
        return "disabled"
    if all(s["test"] for s in live):
        return "fail"
    if any(s["test"] or s["subtest"] for s in live) or len(live) < len(sections):
        return "partial"
    return "pass"


class Wpt:
    """Which WPT test files machopo runs, and what it expects of each."""

    def __init__(self) -> None:
        self.files: set[str] = set()
        dirs: list[str] = []
        for line in (ROOT / "tests/wpt/include.txt").read_text(encoding="utf-8").splitlines():
            line = line.split("#")[0].strip().lstrip("/")
            if line.endswith("/"):
                dirs.append(line)
            elif line:
                self.files.add(line)
        self.dirs = tuple(dirs)
        self.metadata = ROOT / "tests/wpt/metadata"
        self.disabled_dirs = tuple(
            ini.parent.relative_to(self.metadata).as_posix() + "/"
            for ini in self.metadata.rglob("__dir__.ini")
            if re.search(r"^\s*disabled:", ini.read_text(encoding="utf-8"), re.M)
        )

    def state(self, test: str) -> str:
        """`out` (not in `include.txt`), or what `ini_status` says."""
        if not (test in self.files or test.startswith(self.dirs)):
            return "out"
        if test.startswith(self.disabled_dirs):
            return "disabled"
        ini = self.metadata / (test + ".ini")
        return ini_status(ini.read_text(encoding="utf-8")) if ini.is_file() else "pass"


# --- machopo: what the source declares ----------------------------------------


class Source:
    """The names machopo's sources have: CSS properties, interfaces and
    their members, and every string literal that could be a name."""

    def __init__(self) -> None:
        text = (ROOT / "crates/css/src/properties.rs").read_text(encoding="utf-8")
        # The longhands' and the shorthands' `from_name`, each a match on the name.
        self.properties: set[str] = set()
        for function in re.finditer(r"fn from_name\(name: &str\).*?\n    \}", text, re.S):
            self.properties.update(re.findall(r'"(-?[a-z-]+)"', function.group(0)))
        self.interfaces: dict[str, set[str]] = {}
        members = None
        for line in (ROOT / "crates/dom_api/src/interfaces.rs").read_text(encoding="utf-8").splitlines():
            if m := re.match(r"\s*(?:partial\s+)?(?:interface|namespace|mixin)\s+(\w+)", line):
                members = self.interfaces.setdefault(m.group(1), set())
            elif members is None:
                continue
            elif m := re.match(r"\s*(?:static\s+)?(?:attribute|method|const)\s+(?:r#)?(\w+)", line):
                members.add(m.group(1))
            elif re.match(r"\s*constructor\(", line):
                members.add("<constructor>")
        self.globals = self.interfaces.get("Window", set()) | self.interfaces.get("WindowOrWorkerGlobalScope", set())
        self.literals: set[str] = set()
        self.css_literals = set(SELECTORS_CRATE)
        for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
            found = re.findall(r'"(-?[a-zA-Z][a-zA-Z0-9-]*)"', path.read_text(encoding="utf-8"))
            self.literals.update(found)
            if path.relative_to(ROOT / "crates").parts[0] in ("css", "style"):
                self.css_literals.update(found)

    def has(self, key: str) -> bool | None:
        """Whether machopo has what a browser-compat-data key names, or
        `None` for a key the sources can't answer for: a value of a
        property, a data type (`<length>` has no name to find), a header."""
        parts = key.split(".")
        if parts[0] == "css" and len(parts) == 3:
            if parts[1] == "properties":
                return parts[2] in self.properties
            if parts[1] in ("selectors", "at-rules"):
                return parts[2].replace("_", "-") in self.css_literals
        elif parts[0] == "html" and len(parts) == 3 and parts[1] == "elements":
            return parts[2] in self.literals
        elif parts[0] == "api" and len(parts) == 2:
            return parts[1] in self.interfaces or parts[1] in self.globals
        elif parts[0] == "api" and len(parts) == 3:
            members = self.interfaces.get(parts[1])
            if members is None:
                return False
            member = parts[2]
            if member == parts[1]:
                return "<constructor>" in members
            if member.endswith("_event"):
                return "on" + member[: -len("_event")] in members or member[: -len("_event")] in self.literals
            return member.removesuffix("_static") in members
        return None


# --- joining them -------------------------------------------------------------


def usage_of(feature: str, keys: list[str], webstatus: dict, css: dict[str, float], counters: dict[str, float]):
    """The share of Chrome's page loads that use the feature, and where the
    number is from: `webdx` (the feature's own counter), `css` (the highest
    of its properties'), `hand` (`COUNTERS_BY_HAND`), `api` (the highest of
    the counters Chromium generates for its members: only some members have
    one, so it reads low), or `(None, None)`."""
    usage = ((webstatus.get("usage") or {}).get("chrome") or {}).get("daily")
    if usage is not None:
        return usage, "webdx"
    shares = [css[k.split(".")[2]] for k in keys if k.startswith("css.properties.") and k.count(".") == 2
              and k.split(".")[2] in css]
    if shares:
        return max(shares), "css"
    if COUNTERS_BY_HAND.get(feature) in counters:
        return counters[COUNTERS_BY_HAND[feature]], "hand"
    shares = []
    for key in keys:
        parts = key.split(".")
        if parts[0] != "api" or len(parts) != 3:
            continue
        interface, member = parts[1], parts[2][0].upper() + parts[2][1:]
        names = [f"V8{interface}_Constructor"] if parts[2] == interface else [
            f"V8{interface}_{member}_{kind}" for kind in ("Method", "AttributeGetter", "AttributeSetter")]
        shares.extend(counters[name] for name in names if name in counters)
    return (max(shares), "api") if shares else (None, None)


def state_of(row: dict) -> str:
    """Where machopo is with a feature, from its WPT tests and the source:

    - `V8`, `wasm`: the language's own, the engine's to provide.
    - `no tests mapped`: WPT maps no test to it and the source has nothing
      to look for (image formats, HTTP headers).
    - `missing`: no test passes and nothing the source could have is there.
    - `unsure`: no test passes and the source has nothing to look for
      (units, color functions, media features): someone has to look.
    - `partial`: under nine tenths of what the source could have is there,
      or under four fifths of the tests that run pass.
    - `mostly there`: five tests or more run and four fifths pass.
    - `few tests`: the source has it, and fewer than five tests run.
    """
    if row["prefixes"] == ["javascript"] or row["groups"] == ["javascript"]:
        return "V8"
    if row["prefixes"] == ["webassembly"] or row["groups"] == ["webassembly"]:
        return "wasm"
    run = row["wpt_pass"] + row["wpt_partial"] + row["wpt_fail"]
    ok = row["wpt_pass"] + 0.5 * row["wpt_partial"]
    if row["wpt_total"] == 0 and row["source_checked"] == 0:
        return "no tests mapped"
    if ok == 0 and row["source_has"] == 0:
        return "missing" if row["source_checked"] else "unsure"
    if row["source_checked"] and row["source_has"] < 0.9 * row["source_checked"]:
        return "partial"
    if run >= 5:
        return "mostly there" if ok >= 0.8 * run else "partial"
    return "few tests" if run == 0 or ok >= 0.8 * run else "partial"


def join() -> list[dict]:
    data = load("web-features.json")
    groups = data["groups"]
    webstatus = {f["feature_id"]: f for f in load("webstatus.json")}
    css = current(load("css-popularity.json"))
    counters = {name: share for name, share in current(load("feature-popularity.json")).items()
                if not name.startswith("OBSOLETE_")}
    tests_of = wpt_feature_map()
    wpt, source = Wpt(), Source()

    def top_groups(feature: dict) -> list[str]:
        named = feature.get("group") or []
        tops: list[str] = []
        for name in [named] if isinstance(named, str) else named:
            seen = set()
            while "parent" in groups.get(name, {}) and name not in seen:
                seen.add(name)
                name = groups[name]["parent"]
            if name not in tops:
                tops.append(name)
        return tops

    rows = []
    for feature_id, feature in data["features"].items():
        if feature.get("kind", "feature") != "feature":  # moved to, or split into, other IDs
            continue
        keys = feature.get("compat_features") or []
        usage, usage_from = usage_of(feature_id, keys, webstatus.get(feature_id, {}), css, counters)
        states = dict.fromkeys(("out", "pass", "partial", "fail", "disabled"), 0)
        tests = tests_of.get(feature_id, [])
        for test in tests:
            states[wpt.state(test)] += 1
        checks = [has for has in map(source.has, keys) if has is not None]
        row = {
            "id": feature_id,
            "name": feature["name"],
            "groups": top_groups(feature),
            "prefixes": sorted({key.split(".")[0] for key in keys}),
            "baseline": {"high": "widely", "low": "newly", False: "limited"}[feature["status"]["baseline"]],
            "usage": usage,
            "usage_from": usage_from,
            "wpt_total": len(tests),
            **{f"wpt_{name}": count for name, count in states.items()},
            "source_has": sum(checks),
            "source_checked": len(checks),
        }
        row["state"] = state_of(row)
        rows.append(row)
    return rows


# --- the report ---------------------------------------------------------------

USAGE_MARK = {"webdx": "", "css": " (c)", "api": " (a)", "hand": " (h)", None: ""}
TABLE = ("| # | Feature | Group | Baseline | Chrome usage | WPT: pass+partial / all | Source: has / checked |\n"
         "| --- | --- | --- | --- | --- | --- | --- |")


def table(rows: list[dict], limit: int | None = None) -> str:
    lines = [TABLE]
    for number, row in enumerate(rows[:limit], 1):
        usage = "-" if row["usage"] is None else f"{row['usage'] * 100:.1f}%{USAGE_MARK[row['usage_from']]}"
        wpt = f"{row['wpt_pass']}+{row['wpt_partial']} / {row['wpt_total']}" if row["wpt_total"] else "-"
        source = f"{row['source_has']} / {row['source_checked']}" if row["source_checked"] else "-"
        name = row["name"].replace("|", "\\|")
        lines.append(f"| {number} | {name} (`{row['id']}`) | {', '.join(row['groups']) or '-'} | {row['baseline']} "
                     f"| {usage} | {wpt} | {source} |")
    return "\n".join(lines)


def by_usage(rows: list[dict]) -> list[dict]:
    return sorted(rows, key=lambda row: (-(row["usage"] or 0), -row["wpt_total"], row["id"]))


def report(rows: list[dict]) -> str:
    everywhere = [row for row in rows if row["baseline"] != "limited"]
    counts: dict[str, int] = {}
    for row in rows:
        counts[row["state"]] = counts.get(row["state"], 0) + 1

    def pick(source: list[dict], states: tuple[str, ...], with_usage: bool | None = None) -> list[dict]:
        return [row for row in source if row["state"] in states
                and (with_usage is None or (row["usage"] is not None) == with_usage)]

    missing = by_usage(pick(everywhere, ("missing",), True))
    partial = by_usage(pick(everywhere, ("partial",), True))
    uncounted = sorted(pick(everywhere, ("missing", "partial"), False),
                       key=lambda row: (row["state"] != "missing", -row["wpt_total"], row["id"]))
    few = by_usage(pick(everywhere, ("few tests",)))
    unsure = by_usage(pick(everywhere, ("unsure",)))
    limited = by_usage(pick([row for row in rows if row["baseline"] == "limited"], ("missing", "partial"), True))
    sections = [
        ("Missing, by usage", "In every major engine (Baseline widely or newly available), and not in machopo.",
         missing, None),
        ("Partly there, by usage", "In every major engine, and in machopo in part.", partial, None),
        ("Missing or partly there, with no usage number",
         "Mostly old, basic features Chromium never counted. The missing ones first, each group by its number of "
         "WPT tests, which is not usage.", uncounted, None),
        ("Few tests", "The source has them and fewer than five of their tests run: mostly done, and candidates "
         "for `tests/wpt/include.txt`.", few, None),
        ("Unsure", "No test of theirs runs and the source has nothing to look for. Some are there (a color "
         "syntax), some aren't: someone has to look.", unsure, None),
        ("In some engines only (Baseline limited), by usage", "The first 40 of those missing or partly there.",
         limited, 40),
    ]
    parts = [
        "# Web platform features, by what to do next",
        "Written by `python3 scripts/features.py`, whose `-h` says where the data is from. All of it, every "
        "feature, is in `features.csv` beside this file.",
        "## Reading it",
        "- **Chrome usage**: the share of Chrome's page loads that use the feature. Unmarked: the feature's own "
        "counter. `(c)`: the highest of its CSS properties'. `(a)`: the highest of its API members' that Chromium "
        "counts, which reads low. `(h)`: an older counter, matched by hand. `-`: no public number. High usage "
        "doesn't say a page breaks without it.\n"
        "- **WPT**: of the test files WPT maps to the feature, those machopo passes whole + in part / all. A test "
        "not in `tests/wpt/include.txt` counts as not passing, so a low share alone doesn't say it's missing.\n"
        "- **Source**: of the feature's CSS properties, selectors, at-rules, elements, interfaces and members, "
        "those found in machopo's sources / those that could be looked for. Properties and interfaces are read "
        "from their tables; selectors, at-rules and elements are looked for as strings, and can be wrong.",
        "## How many",
        "| State | Features |\n| --- | --- |\n"
        + "\n".join(f"| {state} | {count} |" for state, count in sorted(counts.items(), key=lambda item: -item[1])),
        "`missing`: no test passes and nothing the source could have is there. `partial`: under nine tenths of "
        "what the source could have is there, or under four fifths of the tests that run pass. `few tests`: the "
        "source has it, and fewer than five tests run. `mostly there`: five or more run and four fifths pass. "
        "`unsure`: no test passes and the source has nothing to look for. `no tests mapped`: neither WPT nor the "
        "source can say (image formats, HTTP headers). `V8`, `wasm`: the language's own, the engine's to provide.",
    ]
    for number, (title, note, section_rows, limit) in enumerate(sections, 1):
        parts += [f"## {number}. {title}", f"{len(section_rows)} features. {note}", table(section_rows, limit)]
    return "\n\n".join(parts) + "\n"


COLUMNS = ["id", "name", "groups", "baseline", "usage", "usage_from", "state", "wpt_total", "wpt_pass",
           "wpt_partial", "wpt_fail", "wpt_disabled", "wpt_out", "source_has", "source_checked"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--refresh", action="store_true",
                        help="download the public data again, instead of reading target/features/cache/")
    args = parser.parse_args()
    if not WPT.is_dir():
        print(f"no WPT checkout at {WPT}: `python3 scripts/ci.py third-party` puts it there", file=sys.stderr)
        return 1
    download(args.refresh)
    rows = join()
    (OUT / "priority.md").write_text(report(rows), encoding="utf-8", newline="\n")
    with open(OUT / "features.csv", "w", encoding="utf-8", newline="") as out:
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(COLUMNS)
        for row in by_usage(rows):
            writer.writerow([" ".join(row[column]) if column == "groups" else row[column] for column in COLUMNS])
    print(f"{len(rows)} features: {OUT / 'priority.md'}, {OUT / 'features.csv'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
