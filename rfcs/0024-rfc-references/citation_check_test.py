"""Fixture tests and mutation tests for sanity.py's RFC citation check.

Usage: python rfcs/0024-rfc-references/citation_check_test.py scripts/sanity.py
Builds a throwaway git repo of fixtures, runs `rfc_citations` from the real
script and from mutated copies, and reports which mutants the fixtures catch.
"""
import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

SRC = Path(sys.argv[1]).read_text(encoding="utf-8")

# (path, line, cited?) -- one line per fixture file.
FIXTURES = [
    ("crates/a/README.md", "沒有 `position`（RFC 0013）。", True),
    ("crates/b/README.md", "see RFC-0021", True),
    ("crates/c/src/lib.rs", "// base64 (RFC 4648, with padding)", False),
    ("crates/d/README.md", "見 `rfcs/README.md`", True),
    ("crates/e/README.md", "見 `rfcs/0021-webdriver/README.md`", True),
    ("crates/f/README.md", "https://github.com/rust-lang/rfcs/blob/master/README.md", False),
    ("crates/g/README.md", "frozen in pre-rfcs", True),
    ("crates/h/src/lib.rs", "/// Only block layout in P1.", True),
    ("crates/i/src/lib.rs", "// since P3.1, the table", True),
    ("crates/j/src/lib.rs", "struct LayerTreeHost<P>; let GP3 = P5 + P10 + P1x;", False),
    ("crates/k/README.md", "see MVP.md and PROGRESS.md", True),
    ("docs/README.md", "流程見 `rfcs/README.md`、`rfcs/0000-template/`、`rfcs/pre-rfcs/` 和 `rfcs/`。", False),
    ("docs/x.md", "流程見 `rfcs/README.md`。", True),
    ("CLAUDE.md", "見 `rfcs/pre-rfcs/P3.md`", True),
    ("README.md", "See rfcs/README.md.", False),
    ("README.md.bak", "見 `rfcs/README.md`", True),
    ("rfcs/0001-x/README.md", "RFC 0013 和 P3.1", False),
    ("tests/wpt/baselines/x-expected.txt", 'expected "New P1"', False),
    ("tests/sites/x/index.html", "RFC 0013", False),
    ("scripts/sanity.py", "RFC 0013", False),
    ("scripts/other.py", "# RFC 0013", True),
]

MUTANTS = {
    "RFC number without leading zero": (r'r"\bRFC[ -]?0\d{3}\b"', r'r"\bRFC[ -]?\d{4}\b"'),
    "no `RFC-` / `RFC` spacing variants": (r'r"\bRFC[ -]?0\d{3}\b"', r'r"\bRFC 0\d{3}\b"'),
    "rfcs/ without lookbehind": (r'r"|(?<![\w/.-])rfcs/[\w./-]*"', r'r"|rfcs/[\w./-]*"'),
    "no rfcs/ path pattern": (r'r"|(?<![\w/.-])rfcs/[\w./-]*"', r'r"|(?!x)x"'),
    "no pre-rfcs pattern": (r'r"|pre-rfcs"', r'r"|(?!x)x"'),
    "no MVP file names": (r'r"|\b(?:MVP|PROGRESS)\.md\b"', r'r"|(?!x)x"'),
    "no stage codes": (r'r"|(?<![A-Za-z0-9_])P[0-4](?:\.\d+)?(?![A-Za-z0-9_])"', r'r"|(?!x)x"'),
    "stage codes without lookbehind": (r'r"|(?<![A-Za-z0-9_])P[0-4]', r'r"|P[0-4]'),
    "stage codes without lookahead": (r'(?:\.\d+)?(?![A-Za-z0-9_])"', r'(?:\.\d+)?"'),
    "stage codes P0-P9": (r'P[0-4](?:', r'P[0-9](?:'),
    "pointers allowed in any file": ("if rel in RFC_POINTERS_ALLOWED and token in RFC_POINTERS:", "if token in RFC_POINTERS:"),
    "anything allowed in pointer files": ("if rel in RFC_POINTERS_ALLOWED and token in RFC_POINTERS:", "if rel in RFC_POINTERS_ALLOWED:"),
    "files with NUL read": ('if "\\0" in text:', "if False:"),
    "undecodable files read": ('text = data.decode("utf-8")', 'text = data.decode("utf-8", "replace")'),
    "no trailing-dot strip": ('token = m.group(0).rstrip(".")', "token = m.group(0)"),
    "pointer files matched by suffix": ("if rel in RFC_POINTERS_ALLOWED and", "if rel.endswith(tuple(RFC_POINTERS_ALLOWED)) and"),
    "rfcs/ not excluded": ('RFC_CITATION_EXCLUDED = ("rfcs/", ', "RFC_CITATION_EXCLUDED = ("),
    "baselines not excluded": ('"tests/wpt/baselines/", ', ""),
    "sites not excluded": ('"tests/sites/", ', ""),
    "sanity.py not excluded": (', "scripts/sanity.py")', ")"),
}


def load(src: str, root: Path):
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False, encoding="utf-8") as f:
        f.write(src)
    spec = importlib.util.spec_from_file_location("sanity_mut", f.name)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.ROOT = root
    return mod


def run(src: str, root: Path) -> set[str]:
    return {p.split(":", 1)[0] for p in load(src, root).rfc_citations()}


with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    for rel, line, _ in FIXTURES:
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(line + "\n", encoding="utf-8", newline="\n")
    (root / "zero.dat").write_bytes(b"RFC 0013\0")
    (root / "latin1.txt").write_bytes(b"RFC 0013 \xff")
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    (root / "untracked.md").write_text("RFC 0013\n", encoding="utf-8")

    expected = {rel for rel, _, cited in FIXTURES if cited}
    got = run(SRC, root)
    ok = got == expected
    print("real script:", "ok" if ok else f"FAIL missing={expected - got} extra={got - expected}")
    for name, (a, b) in MUTANTS.items():
        assert SRC.count(a) == 1, f"{name}: pattern found {SRC.count(a)} times"
        got = run(SRC.replace(a, b), root)
        diff = (expected - got) | (got - expected)
        print(f"{'caught' if diff else 'MISSED'}: {name}" + (f" ({sorted(diff)})" if diff else ""))
