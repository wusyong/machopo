#!/usr/bin/env python3
"""Quick checks of the repo's rules, that read the sources and build nothing.

    python scripts/sanity.py unsafe    # `unsafe` stays in `bindings`, `ipc` and `gc`
    python scripts/sanity.py deps      # V8 stays in `bindings` and `dom_js` (and `gc_bench`)
    python scripts/sanity.py spans     # machopo's spans are `trace_event`'s, none made when the trace is off
    python scripts/sanity.py boundary  # the renderer stays behind the process boundary (docs/architecture.md)
    python scripts/sanity.py tests     # each crate's integration tests are one binary, `tests/it/`
    python scripts/sanity.py numerics  # integer arithmetic and casts say what overflow does
    python scripts/sanity.py docs      # the mechanical rules of docs/README.md

None takes arguments; each prints what breaks its rule, file and line, and
what to do. Each command's rules are below,
and in `sanity.py <command> -h`. The tools around the build are in `ci.py`,
the test runs `cargo test` leaves out in `test.py`.

Standard library only. Every command exits non-zero on failure.
"""

from __future__ import annotations

import argparse
import inspect
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# --- unsafe -----------------------------------------------------------------

# Where `#![allow(unsafe_code)]` may appear. The workspace denies
# `unsafe_code`; only `bindings` (V8 FFI) and `ipc` (shared memory mapped
# into two processes, and the Windows FFI) opt back in, at their crate roots,
# plus their integration tests. The DOM's GC heap is elise's, next to the
# repo, under its own rules.
UNSAFE_ALLOWED = [
    "crates/bindings/src/lib.rs",
    "crates/ipc/src/lib.rs",
    "crates/bindings/tests/",
    "crates/ipc/tests/",
]

ALLOW_UNSAFE = re.compile(r"#!?\[\s*(?:allow|expect)\s*\([^\]]*\bunsafe_code\b", re.S)


def cmd_unsafe(args: argparse.Namespace) -> int:
    """`unsafe` stays in `bindings`, `ipc` and `gc`: the workspace denies it, every
    crate inherits the workspace lints, and only the allowed files opt out.
    The files that may allow `unsafe_code` are `UNSAFE_ALLOWED` in this
    script: those crates' roots and their integration tests."""
    problems = []

    workspace = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    lints = re.search(r"^\[workspace\.lints\.rust\]\n(.*?)(?=^\[|\Z)", workspace, re.M | re.S)
    if not lints or not re.search(r'^unsafe_code\s*=\s*"(deny|forbid)"', lints.group(1), re.M):
        problems.append('Cargo.toml: [workspace.lints.rust] must set unsafe_code = "deny"')

    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        text = manifest.read_text(encoding="utf-8")
        if not re.search(r"^\[lints\]\s*\nworkspace\s*=\s*true", text, re.M):
            rel = manifest.relative_to(ROOT).as_posix()
            problems.append(f"{rel}: must inherit the workspace lints ([lints] workspace = true)")

    for path in sorted((ROOT / "crates").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        if "/target/" in f"/{rel}":
            continue
        text = path.read_text(encoding="utf-8")
        for match in ALLOW_UNSAFE.finditer(text):
            if not any(rel == a or (a.endswith("/") and rel.startswith(a)) for a in UNSAFE_ALLOWED):
                line = text.count("\n", 0, match.start()) + 1
                problems.append(f"{rel}:{line}: allows unsafe_code outside bindings / ipc / gc")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("unsafe is confined to bindings and ipc")
    return 0


# --- deps -------------------------------------------------------------------

# The crates that may name V8: `bindings` (V8 itself), `dom_js` (the
# JS front end) and `gc_bench`, the GC benchmarks, which run their
# patterns on V8's heap too and are no part of `machopo`. Every other crate is
# engine-neutral, `dom_api` above all, so that another engine can
# share it.
V8_CRATES = {"bindings", "dom_js", "gc_bench"}
# Where an engine is picked (`ScriptEngineFactory`): these may depend on
# `dom_js` for `V8Engine`, in the sections given, and on nothing
# else of V8's. The renderer itself only sees `dom_api::ScriptEngine`.
ENGINE_PICKERS = {
    "shell": {"dependencies"},
    "browser": {"dev-dependencies"},
}
V8_DEPENDENCIES = {"v8", "bindings", "dom_js"}
# wasmtime stays in the DOM's wasm side (`dom_wasm`).
WASMTIME_CRATES = {"dom_wasm"}
WASMTIME_DEPENDENCIES = {"wasmtime", "wasmtime-wasi"}
DEPENDENCY_SECTION = re.compile(r"^\[(?:target\.[^\]]+\.)?((?:dev-|build-)?dependencies)\]\s*$")

def dependencies(manifest: Path) -> list[tuple[str, str]]:
    """What a Cargo.toml depends on: (name, section) for every dependency
    section (`dependencies`, `dev-dependencies`, `build-dependencies`)."""
    found, section = [], None
    for line in manifest.read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            m = DEPENDENCY_SECTION.match(line)
            section = m.group(1) if m else None
        elif section and (m := re.match(r"([A-Za-z0-9_-]+)\s*[=.]", line)):
            found.append((m.group(1), section))
    return found


def code_lines(path: Path):
    """`path`'s lines with `//` comments (doc comments included) removed."""
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        yield number, line.split("//", 1)[0]


def cmd_deps(args: argparse.Namespace) -> int:
    """V8 stays in `bindings` and `dom_js` (and `gc_bench`): other
    crates don't depend on V8. wasmtime likewise stays in `dom_wasm`.
    The crates that pick an engine may depend on `dom_js`
    (`ENGINE_PICKERS` in this script)."""
    problems = []
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        crate = manifest.parent.name
        if crate in V8_CRATES:
            continue
        picks = ENGINE_PICKERS.get(crate, set())
        for name, section in sorted(set(dependencies(manifest))):
            if name in WASMTIME_DEPENDENCIES and crate not in WASMTIME_CRATES:
                problems.append(f"crates/{crate}/Cargo.toml: `{name}` in [{section}], but wasmtime stays in {', '.join(sorted(WASMTIME_CRATES))}")
            if name not in V8_DEPENDENCIES or (name == "dom_js" and section in picks):
                continue
            problems.append(f"crates/{crate}/Cargo.toml: `{name}` in [{section}], but {crate} is engine-neutral")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("V8 is confined to bindings, dom_js and gc_bench")
    return 0


# --- spans --------------------------------------------------------------------

# `tracing`'s span macros (and `#[instrument]`), named in code: each builds,
# enters and drops an empty span when the trace doesn't take it.
TRACING_SPAN = re.compile(
    r"\btracing::(?:(?:trace|debug|info|warn|error)_span|span)!|\binstrument\b"
    r"|use\s+tracing::.*\b(?:trace|debug|info|warn|error)_span\b"
)
# The files that may name them, with why.
SPANS_ALLOWED = {
    "crates/trace_event/src/lib.rs": "its macros wrap them",
    "crates/shell/src/trace.rs": "the trace's own tests make tracing's spans, entered by hand too",
}


def cmd_spans(args: argparse.Namespace) -> int:
    """machopo's spans are `trace_event`'s (`trace_event!`, `info_event!`,
    `in_scope`): each made only when the trace takes its level, as
    Chromium's `TRACE_EVENT0` looks at its category's flag first.
    `tracing`'s span macros and `#[instrument]` build, enter and drop an
    empty span otherwise, on every call of the code they are in. Only the
    files of `SPANS_ALLOWED` in this script name them. Comments aren't
    read."""
    problems = []
    for path in sorted((ROOT / "crates").glob("*/**/*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        if relative in SPANS_ALLOWED or "/target/" in relative:
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            code = line.split("//", 1)[0]
            if TRACING_SPAN.search(code):
                problems.append(f"{relative}:{number}: tracing's span macro: trace_event::trace_event! (or info_event!, in_scope)")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("machopo's spans are trace_event's")
    return 0


# --- boundary -----------------------------------------------------------------

# The renderer runs as threads of the browser process (RFC 0004 is
# postponed), but keeps to the process boundary, as if it were a process of
# its own: docs/architecture.md, "process 的邊界", has the rules and the
# exceptions; these lists are its exceptions, each with why, and an entry
# goes once its exception is fixed.

# The crates of the renderer's side: what would run in a renderer process
# (`gfx` runs in both).
RENDERER_SIDE = {
    "gfx", "renderer", "compositor", "compositor_paint", "raster", "paint", "layout", "style_resolver", "style", "css",
    "html", "computed_style", "bindings",
    "dom", "dom_api", "dom_api_macros", "dom_js",
    "dom_wasm",
}
# What they must not depend on: the browser's side.
BROWSER_SIDE = {"browser", "net", "viz", "shell"}
# What their code must not name: Viz's whole queue (the renderer gets a
# `CompositorFrameSinkSender`, bound to its sink), and Viz's context lost
# observers (the browser tells the renderer, with `CompositorMsg::ContextLost`).
RENDERER_SIDE_FORBIDDEN = re.compile(r"\bVizMsg\b|\.add_context_lost_observer\(")

# In `protocol`, what doesn't cross a process: skipped fields, trait
# objects, weak references, closures and function pointers. Lines (trimmed)
# allowed, by file, with why.
PROTOCOL_NOT_SERIALIZABLE = re.compile(
    r"serde\(skip\)|\bArc<dyn\b|\bBox<dyn\b|\bRc<|\bWeak<|\bfn\(|\bimpl\s+Fn|\bdyn\s+Fn"
)
PROTOCOL_ALLOWED = {
    "crates/protocol/src/viz.rs": {
        # Exception 1: the shared image's backing, the GPU texture Viz made
        # for a tile (RFC 0004's step 1 makes it a mailbox table).
        "#[serde(skip)]": "the shared image's backing and `CopyOutputRequest`'s callback",
        "backing: Option<Arc<dyn Any + Send + Sync>>,": "exception 1",
        "pub fn new(mailbox: Mailbox, backing: Arc<dyn Any + Send + Sync>) -> Self {": "exception 1",
        "fn add_context_lost_observer(&self, observer: Weak<dyn ContextLostObserver>);":
            "exception 1's interface: only the browser's own UI registers (in the browser process)",
        # Between the browser and Viz, which stays in the browser process.
        "callback: Option<Arc<dyn Fn(Option<CopyOutputResult>) + Send + Sync>>,": "browser and Viz",
        "pub fn new(callback: impl Fn(Option<CopyOutputResult>) + Send + Sync + 'static) -> Self {":
            "browser and Viz",
        "send: Option<Arc<dyn Fn(BeginFrameArgs) + Send + Sync>>,": "browser and Viz",
        "pub fn new(send: impl Fn(BeginFrameArgs) + Send + Sync + 'static) -> Self {":
            "browser and Viz",
    },
}

# State shared by every renderer in the process: statics with interior
# mutability on the renderer's side. Allowed, by (file, name), with why.
MUTABLE_STATIC = re.compile(
    r"^\s*(?:pub(?:\([a-z]+\))?\s+)?static\s+(?:mut\s+)?([A-Z_][A-Z0-9_]*)\s*:\s*([^=]*)"
)
INTERIOR_MUTABILITY = re.compile(r"\b(?:Mutex|RwLock|LazyLock|OnceLock|OnceCell|Once|Cell|RefCell|Atomic\w+)\b")
STATICS_ALLOWED = {
    ("crates/gfx/src/lib.rs", "IMAGE_IDS"): "an id counter: ids only tell objects of one process apart",
    ("crates/paint/src/paint_controller.rs", "IDS"): "an id counter",
    ("crates/compositor/src/layer.rs", "IDS"): "an id counter",
    ("crates/dom/src/names.rs", "SERIALS"):
        "the threads' serials given so far: a serial only tells threads of one process apart",
    ("crates/dom/src/names.rs", "SERIAL"): "the thread's serial (thread-local)",
    ("crates/style/src/lib.rs", "USER_AGENT"):
        "the UA stylesheet's index, made once on a thread (thread-local: its names are numbered on that thread)",
    ("crates/bindings/src/trace.rs", "SCRIPT_DEPTH"): "how deep in script a thread is (thread-local)",
    ("crates/bindings/src/trace.rs", "RECORDING"):
        "whether the process's trace records: the trace is a process's (Perfetto's session, as V8's platform), so "
        "a renderer in a process of its own has its own flag, set as its own trace starts",
    ("crates/bindings/src/v8_text.rs", "ASCII_SCANS"): "tests' counters (thread-local)",
    ("crates/bindings/src/unified_heap.rs", "RECEIVER_UNWRAPS"): "tests' counters (thread-local)",
    ("crates/bindings/src/v8_text.rs", "BORROWED"):
        "the V8 strings lending their text on a thread, which V8's GC prologue on it checks (thread-local)",
    ("crates/css/src/rule_set.rs", "RULES_ADDED"): "tests' counters (thread-local)",
    ("crates/css/src/stylesheet.rs", "RULES_RESOLVED"): "tests' counters (thread-local)",
    ("crates/dom/src/names.rs", "TABLE"):
        "the names with no fixed number, as Blink's AtomicStringTable (thread-local: each renderer's main thread "
        "has its own)",
    ("crates/dom/src/string.rs", "VALUES"):
        "the interned attribute values, as Blink's AtomicStringTable (thread-local: each renderer's main thread "
        "has its own)",
    ("crates/style/src/query.rs", "CACHE"):
        "parsed selectors by their text, as Blink's SelectorQueryCache (thread-local: their names are numbered on "
        "the thread, and each renderer's main thread has its own)",
    ("crates/dom/src/ids.rs", "ID_WALKS"): "tests' counters (thread-local)",
    ("crates/dom/src/ids.rs", "TABLE_CHANGES"): "tests' counters (thread-local)",
    ("crates/dom/src/shadow.rs", "RETARGETS"): "tests' counters (thread-local)",
    ("crates/dom/src/form.rs", "FORM_WALKS"): "tests' counters (thread-local)",
    ("crates/dom/src/form.rs", "DISABLED_WALKS"): "tests' counters (thread-local)",
    ("crates/dom/src/collection.rs", "COLLECTION_STEPS"): "tests' counters (thread-local)",
    ("crates/dom/src/input.rs", "INPUT_TYPE_CHANGES"): "tests' counters (thread-local)",
    ("crates/style/src/query.rs", "FULL_MATCHES"): "tests' counters (thread-local)",
    ("crates/style/src/query.rs", "COMPOUND_ELEMENTS"): "tests' counters (thread-local)",
    ("crates/dom_api/src/frame.rs", "FRAME_TREE_WALKS"): "tests' counters (thread-local)",
    ("crates/dom_api/src/window.rs", "DOCUMENT_GETS"): "tests' counters (thread-local)",
    ("crates/dom_api/src/activation.rs", "CLICKING"):
        "the elements a synthetic click is being dispatched at, as Blink's nodes_dispatching_simulated_clicks "
        "(thread-local, by id: each renderer's main thread has its own)",
    ("crates/dom/src/style.rs", "PASSED_OVER"):
        "the changes the DOM didn't note for style, kept for style's check of them (the debug build's; "
        "thread-local, by document)",
    ("crates/html/src/fast_path.rs", "FAST_PARSES"): "tests' counters (thread-local)",
    ("crates/html/src/fast_path.rs", "DECODES"): "tests' counters (thread-local)",
    ("crates/raster/src/lib.rs", "DRAWS_INTO"): "tests' counters (thread-local)",
    ("crates/dom_js/src/templates.rs", "OVERLOADS"): "derived from the static declaration table",
    ("crates/dom_wasm/src/lib.rs", "STATE"):
        "thread-local: the DOM of each renderer's main thread's wasm engine, which installs it (a store's data must be `Send`)",
    ("crates/compositor/src/layer_tree_host_impl.rs", "TILE_PIXELS"):
        "thread-local: the buffer each thread that rasters tiles draws into",
    ("crates/bindings/src/platform.rs", "PLATFORM"): "V8's platform: one per process, as in Chromium",
    ("crates/bindings/src/script_state.rs", "WRAP_TAGS_CHECKED"): "a one-time check of V8's build",
}


def cmd_boundary(args: argparse.Namespace) -> int:
    """The renderer stays behind the process boundary: its crates don't
    depend on the browser's nor reach Viz but through their frame sink and
    messages, `protocol` has nothing that can't cross a process but the
    exceptions listed, and the renderer's side keeps no state shared by
    every renderer but those listed. The rules are in docs/architecture.md,
    in its section on the process boundary; the exceptions are `PROTOCOL_ALLOWED` and
    `STATICS_ALLOWED` in this script, each with why. An exception that is
    gone (fixed) fails too, until it is taken off its list."""
    problems = []
    for crate in sorted(RENDERER_SIDE):
        manifest = ROOT / "crates" / crate / "Cargo.toml"
        if not manifest.exists():
            problems.append(f"crates/{crate}: listed on the renderer's side, but there is no such crate")
            continue
        for name, section in sorted(set(dependencies(manifest))):
            if name in BROWSER_SIDE:
                problems.append(f"crates/{crate}/Cargo.toml: `{name}` in [{section}], but {crate} is on the renderer's side")
        for path in sorted((ROOT / "crates" / crate / "src").rglob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            for number, line in code_lines(path):
                if m := RENDERER_SIDE_FORBIDDEN.search(line):
                    problems.append(f"{rel}:{number}: `{m.group(0)}` on the renderer's side; see docs/architecture.md, \"process 的邊界\"")

    seen = set()
    for path in sorted((ROOT / "crates" / "protocol" / "src").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        allowed = PROTOCOL_ALLOWED.get(rel, {})
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            code = line.strip()
            if code.startswith("//") or not PROTOCOL_NOT_SERIALIZABLE.search(code):
                continue
            if code in allowed:
                seen.add((rel, code))
            else:
                problems.append(f"{rel}:{number}: `{code}` can't cross a process; see docs/architecture.md, \"process 的邊界\"")
    for rel, lines in PROTOCOL_ALLOWED.items():
        for code in lines:
            if (rel, code) not in seen:
                problems.append(f"{rel}: allowed `{code}` isn't there any more: take it off PROTOCOL_ALLOWED")

    found = set()
    for crate in sorted(RENDERER_SIDE):
        for path in sorted((ROOT / "crates" / crate / "src").rglob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            for number, line in code_lines(path):
                m = MUTABLE_STATIC.match(line)
                if not m or not INTERIOR_MUTABILITY.search(m.group(2)) and "mut " not in line:
                    continue
                key = (rel, m.group(1))
                found.add(key)
                if key not in STATICS_ALLOWED:
                    problems.append(f"{rel}:{number}: `static {m.group(1)}` is shared by every renderer in the process; see docs/architecture.md, \"process 的邊界\"")
    for key in sorted(set(STATICS_ALLOWED) - found):
        problems.append(f"{key[0]}: allowed `static {key[1]}` isn't there any more: take it off STATICS_ALLOWED")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("the renderer keeps to the process boundary (but for the listed exceptions)")
    return 0


# --- Tests --------------------------------------------------------------------

# Integration tests that are binaries of their own, and why. The others are
# modules of their crate's one test binary, `tests/it/`: cargo makes each
# file right under `tests/` a binary, and a change below links every one.
# The ones with `harness = false` in their crate's Cargo.toml (a `main` of
# their own) need no entry.
OWN_TEST_BINARIES = {
    "crates/bindings/tests/cppgc_traced_marking.rs": "V8 flags of its own (--stress-incremental-marking)",
    "crates/bindings/tests/isolate_teardown.rs": "V8 flags of its own (--stress-compaction)",
    "crates/bindings/tests/unified_heap_linear.rs": "V8 flags of its own (--ephemeron-fixpoint-iterations=0)",
    "crates/bindings/tests/unified_heap_marking.rs": "V8 flags of its own (--single-threaded-gc --stress-incremental-marking)",
    "crates/dom_js/tests/marking.rs": "V8 flags of its own (--stress-incremental-marking)",
}


def cmd_tests(args: argparse.Namespace) -> int:
    """Each crate's integration tests are one binary, `tests/it/` (each file
    a module of `tests/it/main.rs`), so that a change below links it once,
    not once per file. A file right under `tests/` is a binary of its own:
    only one with `harness = false` in the crate's Cargo.toml, or one in
    `OWN_TEST_BINARIES` in this script with its reason (V8 flags of its
    own, which V8 takes once per process; or not libtest, as trybuild). And a file in `tests/it/` that
    `main.rs` doesn't declare is never built, so its tests never run."""
    problems = []
    for tests in sorted((ROOT / "crates").glob("*/tests")):
        crate = tests.parent.name
        manifest = (tests.parent / "Cargo.toml").read_text(encoding="utf-8")
        harnessless = set(re.findall(r'\[\[test\]\]\s*\nname\s*=\s*"([^"]+)"\s*\nharness\s*=\s*false', manifest))
        for path in sorted(tests.glob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            if path.stem not in harnessless and rel not in OWN_TEST_BINARIES:
                problems.append(
                    f"{rel}: a test binary of its own: make it a module of crates/{crate}/tests/it/ "
                    "(or, if it can't share a process, list it in OWN_TEST_BINARIES with why)"
                )
        main = tests / "it" / "main.rs"
        if main.exists():
            declared = set(re.findall(r"^\s*(?:pub\s+)?mod\s+(\w+)\s*;", main.read_text(encoding="utf-8"), re.M))
            for path in sorted((tests / "it").glob("*.rs")):
                if path.stem != "main" and path.stem not in declared:
                    rel = path.relative_to(ROOT).as_posix()
                    problems.append(f"{rel}: not a module of tests/it/main.rs, so never built: add `mod {path.stem};`")
    for rel in sorted(OWN_TEST_BINARIES):
        if not (ROOT / rel).exists():
            problems.append(f"{rel}: in OWN_TEST_BINARIES but not there any more: take it off")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("integration tests are one binary per crate (but for the listed ones)")
    return 0


# --- Numerics -------------------------------------------------------------------

# The lints that make integer arithmetic and casts say what overflow does
# (the rules are in crates/base/README.md).
NUMERIC_LINTS = (
    "arithmetic_side_effects",
    "cast_possible_truncation",
    "cast_sign_loss",
    "cast_possible_wrap",
)
TESTS_REASON = "tests"

ATTRIBUTE_START = re.compile(r"#(!?)\[")
LINT_LEVEL = re.compile(r"\b(allow|expect)\s*\(")
REASON = re.compile(r'\breason\s*=\s*"((?:[^"\\]|\\.)*)"', re.S)


def rust_attributes(text: str):
    """Each attribute in `text` (comments left out): (line, inner, its text
    between the brackets)."""
    code = re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), text)
    for match in ATTRIBUTE_START.finditer(code):
        depth, i, in_string = 1, match.end(), False
        while i < len(code) and depth:
            c = code[i]
            if in_string:
                if c == "\\":
                    i += 1
                elif c == '"':
                    in_string = False
            elif c == '"':
                in_string = True
            elif c == "[":
                depth += 1
            elif c == "]":
                depth -= 1
            i += 1
        line = code.count("\n", 0, match.start()) + 1
        yield line, match.group(1) == "!", code[match.end() : i - 1]


def numeric_lints_in(body: str) -> set[str]:
    return {lint for lint in NUMERIC_LINTS if re.search(rf"\bclippy::{lint}\b", body)}


def crate_roots(crate: Path) -> list[Path]:
    """The library's and the binaries' roots."""
    roots = [crate / "src" / "lib.rs", crate / "src" / "main.rs"]
    roots += sorted((crate / "src" / "bin").glob("*.rs"))
    manifest = (crate / "Cargo.toml").read_text(encoding="utf-8")
    roots += [crate / p for p in re.findall(r'^path\s*=\s*"([^"]+\.rs)"', manifest, re.M)]
    return sorted({p for p in roots if p.exists() and "tests" not in p.relative_to(crate).parts})


def test_roots(crate: Path) -> list[Path]:
    """The integration tests' roots: each file right under `tests/`, and each
    `tests/<dir>/main.rs`."""
    tests = crate / "tests"
    return sorted(set(tests.glob("*.rs")) | set(tests.glob("*/main.rs")))


def numerics_problems(root: Path) -> list[str]:
    problems = []
    workspace = (root / "Cargo.toml").read_text(encoding="utf-8")
    lints = re.search(r"^\[workspace\.lints\.clippy\]\n(.*?)(?=^\[|\Z)", workspace, re.M | re.S)
    for lint in NUMERIC_LINTS:
        if not lints or not re.search(rf'^{lint}\s*=\s*"(warn|deny|forbid)"', lints.group(1), re.M):
            problems.append(f'Cargo.toml: [workspace.lints.clippy] must set {lint} = "warn"')

    for crate in sorted(p.parent for p in (root / "crates").glob("*/Cargo.toml")):
        roots = set(crate_roots(crate))
        tests = set(test_roots(crate))
        for path in sorted(crate.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if "/target/" in f"/{rel}":
                continue
            # What each root must allow: the lints for its tests, a crate's
            # own `cfg(test)` code or its integration tests.
            if path in roots:
                needed = "tests"
            elif path in tests:
                needed = "integration tests"
            else:
                needed = None
            found = False
            for line, inner, body in rust_attributes(path.read_text(encoding="utf-8")):
                lints = numeric_lints_in(body)
                if not lints:
                    continue
                level = LINT_LEVEL.search(body)
                reason = REASON.search(body)
                reason = reason.group(1) if reason else None
                where = f"{rel}:{line}"
                is_cfg_test = re.match(r"\s*cfg_attr\s*\(\s*test\s*,", body) is not None
                if level and level.group(1) == "expect" and not is_cfg_test:
                    if not reason or not reason.strip():
                        problems.append(f"{where}: #[expect] without a reason: say why it can't overflow")
                    continue
                fits = inner and reason == TESTS_REASON and (
                    (needed == "tests" and is_cfg_test) or (needed == "integration tests" and not is_cfg_test)
                )
                if not fits:
                    problems.append(
                        f"{where}: allows {', '.join(sorted(lints))}: write what overflow does "
                        '(crates/base/README.md), or #[expect(..., reason = "why it can\'t")]'
                    )
                    continue
                missing = set(NUMERIC_LINTS) - lints
                if missing:
                    problems.append(f"{where}: also allow {', '.join(sorted(missing))}")
                found = True
            if needed and not found:
                if needed == "tests":
                    want = f'#![cfg_attr(test, allow(<the lints>, reason = "{TESTS_REASON}"))]'
                else:
                    want = f'#![allow(<the lints>, reason = "{TESTS_REASON}")]'
                problems.append(f"{rel}: a {'crate' if path in roots else 'test'} root needs {want}")
    return problems


def cmd_numerics(args: argparse.Namespace) -> int:
    """Integer arithmetic and casts say what overflow does (the rules are in
    crates/base/README.md): the workspace turns on clippy's
    arithmetic_side_effects, cast_possible_truncation, cast_sign_loss and
    cast_possible_wrap. Allowing them takes one of two lines, each at a root
    of its own: a crate root's (a library's or a binary's)
    `#![cfg_attr(test, allow(..., reason = "tests"))]` (its `cfg(test)`
    code), and an integration test root's `#![allow(..., reason =
    "tests")]`. Each root has its line. Anywhere else an exception is
    `#[expect(lint, reason = "...")]` with why it can't overflow: it fails
    once nothing overflows there any more. No crate allows them whole."""
    problems = numerics_problems(ROOT)
    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("integer overflow is spelled out")
    return 0


# --- Docs ---------------------------------------------------------------------

# The documentation docs/README.md governs.
DOC_GLOBS = [
    "README.md",
    "CLAUDE.md",
    "docs/*.md",
    "rfcs/*.md",
    "rfcs/*/*.md",
    "crates/*/*.md",
    "tests/*/README.md",
    "resources/*/README.md",
]
# Vendored and forked upstream code lives next to the repo, in
# `../third_party` (each project its own repository); its `README.machopo.md`
# files follow the same rules.
THIRD_PARTY = ROOT.parent / "third_party"
# Built there by `ci.py v8`: never in a fresh checkout, so
# the docs may name it although it isn't there.
V8_ARTIFACTS = THIRD_PARTY / "rusty_v8_artifacts"
# Where test results may be counted (docs/README.md, "其他規則"; the
# rules themselves show an example).
COUNTS_ALLOWED = {"rfcs/pre-rfcs/PROGRESS.md", "tests/wpt/README.md", "docs/README.md"}
# ...and every RFC's progress.md (`rfcs/<rfc>/progress.md`).
TOP_DIRS = ("crates/", "tests/", "docs/", "rfcs/", "scripts/", "resources/", "../third_party/")

MONTH_DATE = re.compile(
    r"\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.? \d{1,2}, \d{4}\b"
)
TEST_COUNT = re.compile(
    r"\d+ 個(?:測試|reftest|WPT reftest)?(?:全部)?(?:通過|失敗)"
    r"|\d+ (?:tests? )?(?:passed|failed)\b"
    r"|(?:reftest|測試) ?共 ?\d+ 個"
)
# Code comments explain themselves instead of citing the docs; `rfcs/` and
# the MVP's documents are the RFC citation check's.
DOC_CITATION = re.compile(r"\bdocs/\S+")
# RFCs are cited only from `rfcs/`: an RFC is deferred, split, rescoped and
# rewritten when it's done, and every file that names it (or what it will
# do) goes stale with it. Elsewhere a file says what is and why. machopo's RFCs
# are numbered with a leading zero, IETF's (`RFC 4648`) aren't; `P0` to
# `P4` (`P3.1`) are the MVP's stages.
RFC_CITATION = re.compile(
    r"\bRFC[ -]?0\d{3}\b"
    r"|(?<![\w/.-])rfcs/[\w./-]*"
    r"|pre-rfcs"
    r"|\b(?:MVP|PROGRESS)\.md\b"
    r"|(?<![A-Za-z0-9_])P[0-4](?:\.\d+)?(?![A-Za-z0-9_])"
)
# The files about the process and where to find things may point at the
# RFC directory, never at one RFC.
RFC_POINTERS_ALLOWED = {"CLAUDE.md", "docs/README.md", "README.md"}
RFC_POINTERS = {"rfcs/", "rfcs/README.md", "rfcs/0000-template/", "rfcs/pre-rfcs/"}
# Kept as upstream wrote them (saved sites), and this script, which has to
# name the paths under `rfcs/` it reads.
RFC_CITATION_EXCLUDED = ("rfcs/", "tests/sites/", "scripts/sanity.py")
# wptrunner's expectations: their section headings are WPT's test and
# subtest names (`P3` can be a color space's), the rest is ours.
WPT_METADATA = "tests/wpt/metadata/"
CODE_SPAN = re.compile(r"`([^`\n]+)`")
CARGO_TEST = re.compile(r"cargo test( [^`]*)")
# `FILE.md` ...的「TITLE」, and 見「TITLE」 for a section of the same file.
FILE_SECTION = re.compile(r"`([\w./-]+\.md)`[^「」`。\n]{0,12}?的「([^」]+)」")
SAME_FILE_SECTION = re.compile(r"(?<![`\w])見「([^」]+)」")


def doc_files() -> list[Path]:
    files = {p for g in DOC_GLOBS for p in ROOT.glob(g)}
    files |= set(THIRD_PARTY.glob("*/README.machopo.md"))
    return sorted(p for p in files if "/target/" not in p.as_posix())


def titles(path: Path) -> set[str]:
    """The headings of a Markdown file, and its bold labels (`**Title**`),
    with any leading numbering (`1.3`, `6.`) dropped."""
    text = path.read_text(encoding="utf-8")
    out = set()
    for heading in re.findall(r"^#+\s+(.+?)\s*$", text, re.M):
        heading = heading.rstrip(" ✅")
        out.add(heading)
        out.add(re.sub(r"^[\d.]+\s*", "", heading))
    out.update(re.findall(r"\*\*([^*\n]+?)\*\*", text))
    return {t.strip().rstrip("。：:") for t in out}


def test_targets(crate: str) -> tuple[set[str], set[str]]:
    """A crate's integration tests and binaries, by name."""
    tests = {p.stem for p in (ROOT / "crates" / crate / "tests").glob("*.rs")}
    # A directory with a `main.rs` is a test too (cargo finds both).
    tests |= {p.parent.name for p in (ROOT / "crates" / crate / "tests").glob("*/main.rs")}
    manifest = (ROOT / "crates" / crate / "Cargo.toml").read_text(encoding="utf-8")
    bins = set(re.findall(r'\[\[bin\]\]\s*\nname\s*=\s*"([^"]+)"', manifest))
    tests |= set(re.findall(r'\[\[test\]\]\s*\nname\s*=\s*"([^"]+)"', manifest))
    if (ROOT / "crates" / crate / "src" / "main.rs").exists():
        bins.add(crate)
    return tests, bins


def rfc_citations() -> list[str]:
    """Where a tracked text file outside `rfcs/` cites an RFC or the MVP's
    documents and stages."""
    files = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, check=True
    ).stdout.decode("utf-8").split("\0")
    problems = []
    for rel in files:
        if not rel or rel.startswith(RFC_CITATION_EXCLUDED):
            continue
        try:
            data = (ROOT / rel).read_bytes()
            text = data.decode("utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if "\0" in text:
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if rel.startswith(WPT_METADATA) and line.lstrip().startswith("["):
                continue
            for m in RFC_CITATION.finditer(line):
                token = m.group(0).rstrip(".")
                if rel in RFC_POINTERS_ALLOWED and token in RFC_POINTERS:
                    continue
                problems.append(
                    f"{rel}:{number}: cites `{token}`: only `rfcs/` cites RFCs; say what is and why"
                    " (docs/README.md, 「每份文件放什麼」)"
                )
    return problems


def check_cargo_test(args: str) -> list[str]:
    problems = []
    words = args.split()
    crates = [words[i + 1] for i, w in enumerate(words[:-1]) if w == "-p"]
    for crate in crates:
        if not (ROOT / "crates" / crate / "Cargo.toml").exists():
            problems.append(f"no crate `{crate}`")
    if len(crates) != 1 or problems:
        return problems
    tests, bins = test_targets(crates[0])
    for flag, names, kind in (("--test", tests, "test"), ("--bin", bins, "binary")):
        for i, w in enumerate(words[:-1]):
            if w == flag and words[i + 1] not in names:
                problems.append(f"no {kind} `{words[i + 1]}` in `{crates[0]}`")
    # A filter on a crate's one test binary names its module first
    # (`--test it tree::`); a wrong one only runs no test, so check it.
    if "--test" in words and words[words.index("--test") + 1 :][:1] == ["it"]:
        it = ROOT / "crates" / crates[0] / "tests" / "it"
        for w in words:
            module = w.split("::")[0]
            if "::" in w and not ((it / f"{module}.rs").exists() or (it / module / "mod.rs").exists()):
                problems.append(f"no module `{module}` in `{crates[0]}`'s tests/it/")
    return problems


def path_exists(token: str, doc: Path) -> bool:
    token = re.sub(r":\d+(?:-\d+)?$", "", token).rstrip("/")
    bases = [ROOT, doc.parent]
    # A crate's README may name paths in the crate (`tests/common`).
    if doc.parent.parent.name == "crates":
        bases.append(doc.parent)
    for base in bases:
        candidate = base / token
        # Built here by `v8`: never in a fresh checkout.
        if candidate.resolve() == V8_ARTIFACTS.resolve() or V8_ARTIFACTS.resolve() in candidate.resolve().parents:
            return True
        if "*" in token:
            if list(base.glob(token)):
                return True
        elif candidate.exists():
            return True
    return False


# The old DOM's crates, deleted once the DOM that replaced them was the
# only one, and the crates renamed then (the DOM's took their names,
# `style` became `style_resolver`): an RFC written before that cites their
# files, sections and tests as they were, so those aren't checked in
# `rfcs/`.
GONE_CRATES = (
    "dom", "dom_api", "dom_macros", "dom_js", "dom_wasm", "style",
    "dom_next", "dom_api_next", "dom_api_next_macros", "dom_js_next", "dom_wasm_next", "style_next",
)
GONE_CRATE_PATH = re.compile(r"^(?:crates/)?(?:" + "|".join(GONE_CRATES) + r")/")
GONE_CRATE_TEST = re.compile(r"-p (?:" + "|".join(GONE_CRATES) + r")\b")


def cites_gone_crate(rel: str, cited: str) -> bool:
    """Whether `cited` (a path, or a `cargo test`'s arguments), in doc
    `rel`, is a deleted crate's that an RFC may cite as it was."""
    return rel.startswith("rfcs/") and bool(GONE_CRATE_PATH.match(cited) or GONE_CRATE_TEST.search(cited))


def is_historical(rel: str) -> bool:
    """A record of how things were: the MVP's frozen documents, every RFC's
    `progress.md` (the checks run, as they were run then) and a done RFC's
    other documents (its status ✅). The paths and test commands in it are
    as they were then, so they aren't checked: moving or renaming a test
    later doesn't send anyone back to rewrite history."""
    if rel.startswith("rfcs/pre-rfcs/"):
        return True
    parts = rel.split("/")
    if len(parts) != 3 or parts[0] != "rfcs":
        return False
    if parts[2] == "progress.md":
        return True
    readme = ROOT / "rfcs" / parts[1] / "README.md"
    if not readme.exists():
        return False
    status = re.search(r"^- 狀態：(\S+)", readme.read_text(encoding="utf-8"), re.M)
    return bool(status) and status.group(1) == "✅"


def cmd_docs(args: argparse.Namespace) -> int:
    """The mechanical parts of docs/README.md's rules: absolute dates as
    2026-09-30, test counts only in an RFC's progress.md, no `docs/` citations
    in code comments, and that the paths, test commands and
    sections the docs name exist (but for the paths and test commands of
    an RFC's progress.md, a done RFC and `rfcs/pre-rfcs/`, which record how things were). What a doc says about behaviour still
    needs reading. The docs are `DOC_GLOBS` in this script and every
    `README.machopo.md` in `../third_party`; code comments are read in every
    `.rs` under `crates/`.

    RFCs are cited only from `rfcs/`: no other tracked text file names an
    RFC's number (`RFC 0013`), a path under `rfcs/`, or the MVP's documents
    and stages (`P3.1`). `CLAUDE.md`, `docs/README.md` and the root
    `README.md` may point at `rfcs/`, `rfcs/README.md`, `rfcs/0000-template/`
    and `rfcs/pre-rfcs/`. The saved sites and this script
    are left out, and so are the test and subtest names that head the
    sections of wptrunner's expectations."""
    problems = []
    docs = doc_files()
    by_name: dict[str, list[Path]] = {}
    for doc in docs:
        by_name.setdefault(doc.name, []).append(doc)
    for doc in docs:
        rel = Path(os.path.relpath(doc, ROOT)).as_posix()
        text = doc.read_text(encoding="utf-8")
        historical = is_historical(rel)
        for number, line in enumerate(text.splitlines(), 1):
            where = f"{rel}:{number}"
            if m := MONTH_DATE.search(line):
                problems.append(f"{where}: date `{m.group(0)}`: write it as 2026-09-30")
            counts_allowed = rel in COUNTS_ALLOWED or re.fullmatch(r"rfcs/[^/]+/progress\.md", rel)
            if not counts_allowed and (m := TEST_COUNT.search(line)):
                problems.append(f"{where}: test count `{m.group(0)}`: counts go in the RFC's progress.md")
            for span in CODE_SPAN.findall(line):
                if historical:
                    break
                if cites_gone_crate(rel, span):
                    continue
                if span.startswith(TOP_DIRS) and " " not in span and not path_exists(span, doc):
                    problems.append(f"{where}: `{span}` doesn't exist")
                if m := CARGO_TEST.match(span):
                    problems += [f"{where}: `{span}`: {p}" for p in check_cargo_test(m.group(1))]
            for name, title in FILE_SECTION.findall(line):
                if cites_gone_crate(rel, name):
                    continue
                # From the root, or next to the citing doc (an RFC's
                # `README.md` and `details.md` cite each other).
                found = [base / name for base in (ROOT, doc.parent) if (base / name).exists()]
                targets = found or by_name.get(Path(name).name, [])
                if not targets:
                    problems.append(f"{where}: `{name}` isn't a doc")
                elif not any(title.strip() in titles(t) for t in targets):
                    problems.append(f"{where}: no section 「{title}」 in `{name}`")
            for title in SAME_FILE_SECTION.findall(line):
                if title.strip() not in titles(doc):
                    problems.append(f"{where}: no section 「{title}」 in this file")

    for path in sorted((ROOT / "crates").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        if "/target/" in f"/{rel}":
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            comment = re.search(r"//.*", line)
            if comment and (m := DOC_CITATION.search(comment.group(0))):
                problems.append(f"{rel}:{number}: comment cites `{m.group(0)}`: explain it in the comment")

    problems += rfc_citations()

    for problem in problems:
        print(problem)
    if problems:
        print(f"{len(problems)} problems")
        return 1
    print(f"{len(docs)} docs follow the mechanical rules")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True, metavar="command")

    def command(name: str, func, help: str) -> None:
        """A subcommand, described by its function's docstring in its `-h`."""
        sub.add_parser(
            name,
            help=help,
            description=inspect.cleandoc(func.__doc__),
            formatter_class=argparse.RawDescriptionHelpFormatter,
        ).set_defaults(func=func)

    command("unsafe", cmd_unsafe, "check that unsafe stays in bindings, ipc and gc")
    command("deps", cmd_deps, "check that V8 stays in bindings, dom_js and gc_bench")
    command("spans", cmd_spans, "check that machopo's spans are trace_event's, none made when the trace is off")
    command("boundary", cmd_boundary, "check that the renderer keeps to the process boundary (docs/architecture.md)")
    command("tests", cmd_tests, "check that each crate's integration tests are one binary, tests/it/")
    command("numerics", cmd_numerics, "check that integer arithmetic and casts say what overflow does")
    command("docs", cmd_docs, "dates, test counts, paths, test commands and sections in the docs")

    # Every command's own help, so that `sanity.py -h` explains each rule.
    parser.epilog = "\n".join(f"--- {name} ---\n\n{p.format_help()}" for name, p in sub.choices.items())

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
