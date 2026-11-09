"""Both browsers' samples by the kind of work they are in, and the
comparison (`categories.md`).

`bench.py speedometer --sample` folds machopo's samples, `--sample --browser
chrome` a Chromium's (`<suite>-machopo.folded`, `<suite>-chrome.folded`: the
renderer main thread's stacks in the runner's timed windows). Each sample
goes to the innermost frame of its stack that says what work it is:

- allocation, `memcpy` and the standard library's containers (Rust's
  `core`, `alloc`, `hashbrown`...; C++'s `std`, PartitionAlloc, WTF's
  strings and containers, `base`, `cc`) go to their caller;
- V8's own frames look outward: JS when a JS function calls them (JIT code,
  builtins, the runtime), "V8 API" when machopo or Blink does; V8's GC is GC;
- Skia's frames look outward: canvas under a canvas, else paint;
- machopo's by crate (`dom` and `dom_api` are DOM, `dom_js` and `bindings` are
  bindings, `layout::text` and parley text, the rest of `layout` and taffy
  box layout...), Blink's by class name (style before text: `FontBuilder` is
  style);
- the sampling's own (V8's perf map, its GC log, the trace) is left out.

A suite's share of each kind, times its time when not sampled (its median
in `machopo-runs.json`, `chrome-runs.json`: sampling slows each browser by its
own amount), is that kind's ms of an iteration. The Chromium sampled stands
for Chrome for Testing, which has no symbols on Linux: its shares, Chrome
for Testing's times.
"""

from __future__ import annotations

import collections
import math
import re
from pathlib import Path

CATEGORIES = ("JS", "DOM", "style", "text", "box layout", "V8 API", "HTML parse", "paint", "GC", "bindings",
              "canvas", "other")
# The sampling's own: left out of the shares.
SAMPLING = "sampling"

# What a frame is: a category, or one of these.
PASS, V8, SKIA = "pass", "v8", "skia"

Folded = collections.Counter


def bare(frame: str) -> str:
    """`frame` without what its `<...>` and `(...)` nests hold (template and
    call arguments): a type in them isn't the frame's name."""
    out, depth = [], 0
    for ch in frame:
        if ch in "<(":
            depth += 1
        elif ch in ">)":
            depth = max(depth - 1, 0)
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def cxx_name(frame: str) -> str:
    """A C++ frame's qualified name: no return type, template or call
    arguments, qualifiers or `[clone ...]`."""
    name = bare(re.sub(r"\s*\[clone [^\]]*\]", "", frame)).strip()
    name = re.sub(r"(\s+(const|volatile|&&|&|noexcept))+$", "", name)
    return name.rsplit(" ", 1)[-1]


# --- what both browsers' frames share ----------------------------------------

PASS_ANY = re.compile(
    r"^(\[unknown\]|__mem|__libc|memcpy|memmove|memset|memcmp|mi_|_mi_|malloc|free$|calloc|realloc|"
    r"__GI_|pthread_|__pthread|operator new|operator delete|__rust_|__rdl_|decltype|__strlen|strlen|syscall|"
    r"__lll|__futex|sched_)")
OWN = re.compile(r"(perfetto|protozero|absl)::|machopo_trace|CodeEventLogger|LinuxPerf\w*Logger|GCTracer::(Print|Output)|"
                 r"v8::base::(OS::FPrint|OS::VSNPrintF|SNPrintF)")
V8_GC = re.compile(r"v8::internal::(Heap|Scavenger\w*|MarkCompact\w*|MinorMarkSweep\w*|Sweeper|ConcurrentMarking|"
                   r"IncrementalMarking\w*|\w*MarkingVisitor\w*|GlobalHandles|TracedHandles|CppHeap|"
                   r"MemoryAllocator|PagedSpace|NewSpace|SemiSpace\w*|LocalHeap|Evacuat\w*|ArrayBufferSweeper|"
                   r"MarkingWorklist\w*|GCTracer|MemoryReducer)::")


def is_js(frame: str) -> bool:
    """V8's name for JS (its perf map): a function's (`JS:*name url`) or a
    regular expression's code."""
    return frame.startswith("JS:") or frame.startswith("RegExp")


def is_v8(frame: str) -> bool:
    """V8's own C++ (or its builtins); not a Rust frame that names one of
    rusty_v8's types (`<... as v8::support::CFnFrom<_>>`)."""
    if frame.startswith("<"):
        return False
    return frame.startswith("Builtins_") or "v8::internal::" in frame or re.search(r"(^|\s)v8::", frame) is not None


# --- machopo's (Rust) -------------------------------------------------------------

PASS_RUST = re.compile(
    r"^(<?(core|alloc|std|hashbrown|rustc_hash|indexmap|smallvec|arrayvec)::|<\[|<str>|<&|<\(|"
    r"<(u8|u16|u32|u64|usize|i32|i64|f32|f64|char|bool)\b|<[A-Z] as|<?gc::vector::)")
CRATES = {
    "dom": "DOM", "dom_api": "DOM",
    "dom_js": "bindings", "bindings": "bindings", "v8": "bindings",
    "gc": "GC",
    "style": "style", "style_resolver": "style", "css": "style", "selectors": "style", "computed_style": "style",
    "cssparser": "style", "animation": "style",
    "parley": "text", "parley_engine": "text", "harfrust": "text", "fontique": "text", "skrifa": "text",
    "read_fonts": "text", "swash": "text", "icu_segmenter": "text", "icu_properties": "text",
    "icu_normalizer": "text", "icu_collections": "text", "unicode_bidi": "text", "unicode_segmentation": "text",
    "unicode_linebreak": "text",
    "taffy": "box layout", "layout": "box layout",
    "html": "HTML parse", "html5ever": "HTML parse", "markup5ever": "HTML parse", "tendril": "HTML parse",
    "xml5ever": "HTML parse",
    "paint": "paint", "compositor_paint": "paint", "tiny_skia": "paint", "raster": "paint",
    "canvas": "canvas",
    "tracing": SAMPLING, "tracing_core": SAMPLING, "tracing_subscriber": SAMPLING, "trace_event": SAMPLING,
    "sharded_slab": SAMPLING,
}


def rust_kind(frame: str) -> str | None:
    """What a Rust frame is, by its crate (`None`: not Rust's)."""
    if frame.startswith('<unsafe extern "C" fn') and "v8::support::CFnFrom" in frame:
        return "bindings"  # rusty_v8's shell of a callback, `dom_js`'s member call inlined in it
    if PASS_RUST.search(frame):
        return PASS
    path = frame[1:] if frame.startswith("<") else frame
    crate = re.match(r"([A-Za-z_]\w*)::", path)
    if not crate:
        return None
    crate = crate.group(1)
    if crate == "layout" and re.match(r"<?layout::(text|font|shaping|line|bidi)", frame):
        return "text"
    if crate == "bindings" and re.match(r"<?bindings::(unified_heap|gc_platform)", frame):
        return "GC"
    if crate == "v8" and re.match(r"<?v8::(machopo|cppgc)", frame):
        return "GC" if "Marking" in frame or "cppgc" in frame else "bindings"
    return CRATES.get(crate, "other")


# --- Chrome's (Blink, C++) ----------------------------------------------------

PASS_CXX = re.compile(
    r"^(std::|partition_alloc::|base::|cc::|WTF::|absl::|gfx::|mojo::|"
    r"blink::(AtomicString\w*|String\w*|StringImpl|StringBuilder|StringView|HashTable\w*|HashMap|HashSet|Vector\w*|"
    r"\w*Hasher|Partitions|CaseMap|ParkableString\w*|CStringImpl|SegmentedString|\w*HashTraits|"
    r"LinkedHashSet\w*|Deque)(::|$))")
SKIA_NAME = re.compile(r"^(skcpu::|\w+::blit_|SafeRLE|Sk|sk_|SkSL|aaa_|RunBased|skcms|skia|skgpu|sktext|hsw::|"
                       r"portable::)")
TEXT_LIBRARY = re.compile(r"^(OT::|hb_|_hb_|icu_\d+::|u_|ubrk_|utext_|ucase_|FT_|ft_|AAT::|CFF::)")
BLINK = [(category, re.compile(rx)) for category, rx in [
    ("GC", r"^blink::(ThreadState|\w*Visitor|HeapAllocator|Persistent\w*|Member\w*|WeakMember\w*|ThreadHeap|"
           r"HeapObjectHeader)::|::Trace$"),
    ("bindings", r"^blink::(V8[A-Z]\w*|bindings|ScriptWrappable|ToV8\w*|NativeValueTraits\w*|V8PerIsolateData|"
                 r"V8PerContextData|DOMDataStore|DOMWrapperWorld|ScriptState|ExceptionState|WrapperTypeInfo|IDL\w*|"
                 r"V8ScriptRunner|V8Union\w*|ScriptValue|ScriptFunction\w*|ToV8Traits|CallbackFunctionBase|"
                 r"CallbackInterfaceBase)(::|$)|^blink::(\w*::)?v8_"),
    ("HTML parse", r"^blink::(HTML(Tokenizer|TreeBuilder\w*|DocumentParser\w*|ConstructionSite|StackItem|ElementStack|"
                   r"Token\w*|ParserScheduler|EntityParser|ParserScriptRunner|FormattingElementList|"
                   r"PreloadScanner|ParserIdioms|SrcsetParser|InputStream|ParserReentryPermit)|AtomicHTMLToken|"
                   r"DocumentParser|ScriptableDocumentParser|BackgroundHTML\w*|XMLDocumentParser|"
                   r"\w*FastPath\w*|TokenPreloadScanner|CSSPreloadScanner|DecodedDataDocumentParser)(::|$)|"
                   r"^blink::::\w*FastPath"),
    ("canvas", r"^blink::(\w*Canvas\w*|BaseRenderingContext2D|Path2D|CanvasPath|ImageData)(::|$)"),
    ("paint", r"^blink::(\w*Paint\w*|\w*Painter|PrePaint\w*|DisplayItem\w*|GraphicsContext\w*|\w*Raster\w*|"
              r"DrawingRecorder|HitTest\w*|\w*PropertyTree\w*|FragmentData\w*|ObjectPaintInvalidator|"
              r"\w*Composit\w*|PendingLayer)(::|$)"),
    ("style", r"^blink::(\w*Style\w*|\w*CSS\w*|\w*Selector\w*|RuleSet\w*|RuleFeature\w*|ElementRuleCollector|"
              r"MatchResult|MatchedProperties\w*|\w*Cascade\w*|FontBuilder|\w*Invalidat\w*|MediaQuer\w*|"
              r"css_longhand|css_shorthand|css_parsing_utils|\w*RuleData|StyleRecalc\w*|CounterStyle\w*|"
              r"Keyframe\w*|Animation\w*|\w*Interpolat\w*|PropertyRegistry|ComputedStyle\w*|"
              r"ContainerQuery\w*)(::|$)|"
              r"^blink::(Element|Node|Document|ContainerNode|Text|ShadowRoot)::(\w*Style\w*|RebuildLayoutTree\w*)$"),
    ("text", r"^blink::(\w*Inline\w*|\w*LineBreak\w*|Line\w*|\w*Shap\w*|\w*Font\w*|\w*Glyph\w*|TextRun\w*|"
             r"HarfBuzz\w*|Text(Fragment|Item|Offset|Shaping|Breaker|Autosizer|Metrics|Run|Iterator)\w*|\w*Bidi\w*|"
             r"ScriptRun\w*|LayoutText\w*|OffsetMapping\w*|Hyphenation\w*|\w*TextDecoration\w*|"
             r"TextBreakIterator|LazyLineBreakIterator)(::|$)"),
    ("box layout", r"^blink::(\w*Layout\w*|\w*Fragment\w*|ConstraintSpace\w*|BlockNode|\w*Grid\w*|\w*Flex\w*|"
                   r"\w*Table\w*|BreakToken\w*|BlockBreakToken|\w*Margin\w*|ExclusionSpace\w*|ScrollableArea|"
                   r"\w*ScrollAnchor\w*)(::|$)|"
                   r"^blink::(Element|Node|Document|ContainerNode|Text)::(\w*LayoutTree\w*|\w*LayoutObject\w*)$|"
                   r"^blink::::(Compute\w*(Size|Borders|Padding|Margins)\w*|Resolve\w*Length\w*)"),
    ("DOM", r"^blink::(\w*Node\w*|\w*Element\w*|Container\w*|Document\w*|Text|CharacterData|Attr|Attribute\w*|"
            r"QualifiedName|\w*Event\w*|\w*Listener\w*|Range\w*|\w*Selection\w*|\w*Mutation\w*|\w*Collection\w*|"
            r"TreeScope\w*|\w*Shadow\w*|\w*Slot\w*|CustomElement\w*|DOM\w*|ClassList|\w*Dataset\w*|NamedNodeMap|"
            r"\w*Form\w*|\w*Editing\w*|\w*Traversal\w*|\w*Observer\w*|SpaceSplitString\w*|\w*Comment|Template\w*|"
            r"\w*Window\w*|SelectionTemplate|EditingAlgorithm|\w*Script\w*|Frame\w*|\w*Focus\w*|\w*Input\w*|"
            r"\w*Option\w*|TreeOrderedMap|\w*NodeList\w*|Image\w*|\w*Anchor\w*|\w*Timer\w*|\w*Task\w*|"
            r"\w*Idle\w*|Location|History\w*|Performance\w*|Navigator\w*)(::|$)"),
]]


def cxx_kind(frame: str) -> str | None:
    """What a C++ frame of Chrome's is (`None`: not one this knows)."""
    name = cxx_name(frame)
    if name.startswith("cppgc::"):
        return "GC"
    if PASS_CXX.search(name):
        return PASS
    if SKIA_NAME.search(name):
        return SKIA
    if TEXT_LIBRARY.search(name):
        return "text"
    if name.startswith("blink::"):
        return next((category for category, rx in BLINK if rx.search(name)), "other")
    if name.startswith(("content::", "viz::", "gpu::")):
        return "other"
    return None


def kind(frame: str) -> str | None:
    """What a frame is: a category, `PASS` (its caller's), `V8` or `SKIA`
    (what calls it says), `SAMPLING`, or `None` (no say)."""
    if is_js(frame):
        return "JS"
    if OWN.search(frame):
        return SAMPLING
    if PASS_ANY.search(frame):
        return PASS
    if is_v8(frame):
        return "GC" if V8_GC.search(bare(frame)) else V8
    found = cxx_kind(frame)
    return found if found is not None else rust_kind(frame)


def category(stack: list[str]) -> str:
    """The kind of work a sample is in: its stack root first, as folded."""
    in_v8 = in_skia = False
    for frame in reversed(stack):
        found = kind(frame)
        if found in (PASS, None):
            continue
        if found == V8:
            in_v8 = True
        elif found == SKIA:
            in_skia = True
        elif found == "JS":
            return "JS"
        elif in_skia:
            return "canvas" if found == "canvas" else "paint"
        elif in_v8 and found not in ("GC", SAMPLING):
            return "V8 API"
        else:
            return found
    return "paint" if in_skia else "V8 API" if in_v8 else "other"


def innermost(stack: list[str]) -> str:
    """The frame a sample is counted at: the innermost that says what it is
    (C++ by its qualified name)."""
    frame = next((f for f in reversed(stack) if kind(f) not in (PASS, None, V8, SKIA)), stack[-1])
    # C++'s come with their call arguments; Rust's (`<T as U>::f`) don't.
    return cxx_name(frame) if "(" in frame and not frame.startswith("<") else frame


def read_folded(path: Path) -> Folded:
    folded: Folded = collections.Counter()
    for line in path.read_text(encoding="utf-8").splitlines():
        stack, count = line.rsplit(" ", 1)
        folded[stack] += int(count)
    return folded


class Suite:
    """One browser's samples of a suite in ms of an iteration: each
    category's (`by_category`) and each frame's (`by_frame`, by category),
    from their shares of the samples (the sampling's own left out) times
    the suite's time when not sampled."""

    def __init__(self, folded: Folded, time_ms: float) -> None:
        counts: collections.Counter = collections.Counter()
        frames: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
        for stack, count in folded.items():
            frames_ = stack.split(";")
            found = category(frames_)
            counts[found] += count
            frames[found][innermost(frames_)] += count
        whole = sum(count for found, count in counts.items() if found != SAMPLING)
        scale = time_ms / whole if whole else 0.0
        self.by_category = {c: counts[c] * scale for c in CATEGORIES}
        self.by_frame = {c: collections.Counter({f: n * scale for f, n in frames[c].items()}) for c in CATEGORIES}
        self.sampling = counts[SAMPLING] / sum(counts.values()) if counts else 0.0
        self.time_ms = time_ms


def geomean(values: list[float]) -> float:
    return math.exp(sum(math.log(v) for v in values) / len(values)) if values else float("nan")


def caught_up(machopo: dict[str, Suite], chrome: dict[str, Suite], categories: tuple[str, ...]) -> float:
    """The geometric mean of each suite's machopo / Chrome if machopo took Chrome's
    time in `categories`."""
    return geomean([(machopo[s].time_ms - sum(machopo[s].by_category[c] - chrome[s].by_category[c] for c in categories))
                    / chrome[s].time_ms for s in machopo])


def report(machopo: dict[str, Suite], chrome: dict[str, Suite], chromium_ratio: dict[str, float], header: list[str],
           top: int = 8) -> str:
    """`categories.md`: each category's ms of an iteration over the suites
    both browsers have, what catching up in it alone would make the
    geometric mean, each suite's three widest gaps, the categories' top
    frames, and how the Chromium sampled compares with Chrome for Testing
    (`chromium_ratio`: its time over Chrome for Testing's)."""
    names = sorted(set(machopo) & set(chrome))
    machopo = {s: machopo[s] for s in names}
    chrome = {s: chrome[s] for s in names}
    lines = [*header, "",
             f"{len(names)} suites; the geometric mean of machopo / Chrome: {caught_up(machopo, chrome, ()):.3f}.", "",
             "## By category", "",
             "ms of an iteration, the suites summed; the last column is the geometric mean if machopo caught up in "
             "that category alone.", "",
             "| category | machopo | Chrome | gap | machopo / Chrome | caught up |", "| --- | ---: | ---: | ---: | ---: | ---: |"]
    rows = []
    for c in CATEGORIES:
        k = sum(machopo[s].by_category[c] for s in names)
        ch = sum(chrome[s].by_category[c] for s in names)
        rows.append((caught_up(machopo, chrome, (c,)), c, k, ch))
    for caught, c, k, ch in sorted(rows):
        ratio = f"{k / ch:.2f}" if ch else "–"
        lines.append(f"| {c} | {k:.1f} | {ch:.1f} | {k - ch:+.1f} | {ratio} | {caught:.3f} |")
    k_all = sum(machopo[s].time_ms for s in names)
    c_all = sum(chrome[s].time_ms for s in names)
    lines += [f"| all | {k_all:.1f} | {c_all:.1f} | {k_all - c_all:+.1f} | {k_all / c_all:.2f} | |", "",
              "## By suite", "", "| suite | machopo | Chrome | machopo / Chrome | the widest gaps |",
              "| --- | ---: | ---: | ---: | --- |"]
    for s in sorted(names, key=lambda s: chrome[s].time_ms - machopo[s].time_ms):
        gaps = [(g, c) for g, c in sorted(((machopo[s].by_category[c] - chrome[s].by_category[c], c)
                                           for c in CATEGORIES), reverse=True)[:3] if g >= 0.05]
        lines.append(f"| {s} | {machopo[s].time_ms:.1f} | {chrome[s].time_ms:.1f} | "
                     f"{machopo[s].time_ms / chrome[s].time_ms:.2f} | " + ", ".join(f"{c} {g:+.1f}" for g, c in gaps) + " |")
    lines += ["", "## The Chromium sampled and Chrome for Testing", "",
              "Its time over Chrome for Testing's, each suite (the shares stand for Chrome for Testing's the less "
              "alike they are): " + ", ".join(f"{s} {chromium_ratio[s]:.2f}" for s in names if s in chromium_ratio)
              + f"; geometric mean {geomean([chromium_ratio[s] for s in names if s in chromium_ratio]):.3f}.",
              "", "## Top frames", "", "ms of an iteration, the suites summed, at the innermost frame that says "
              "what the work is."]
    for c in CATEGORIES:
        lines += ["", f"### {c}", ""]
        for side, suites in (("machopo", machopo), ("Chrome", chrome)):
            frames: collections.Counter = collections.Counter()
            for s in names:
                frames.update(suites[s].by_frame[c])
            items = [f"`{name[:100]}` {ms:.1f}" for name, ms in frames.most_common(top) if ms >= 0.05]
            lines.append(f"- {side}: " + ("; ".join(items) if items else "none"))
    return "\n".join(lines) + "\n"
