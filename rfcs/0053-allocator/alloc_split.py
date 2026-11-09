#!/usr/bin/env python3
"""Split the libc allocation and copy samples in `ci.py speedometer --sample`'s
folded stacks by who called them.

    python3 alloc_split.py target/speedometer-results/sample [> alloc-split.md]

For each `<suite>-kun.folded` (cycles) and `<suite>-kun.instructions.folded`,
each sample whose leaf is in glibc's malloc family is put in one class by
walking up from the leaf past the allocator's own frames:

- rust:      through Rust's global allocator (`__rust_alloc`, `__rdl_*`,
             `alloc::alloc::*`, `alloc::raw_vec::*`): what `#[global_allocator]`
             replaces;
- binding:   a rusty_v8 `binding.cc` export (`cppgc__*`, `v8__*`): C++ `new`
             on Rust's behalf (e.g. `cppgc__Persistent__CONSTRUCT`);
- c++:       `operator new`/`delete`, Skia's `sk_malloc`, or a V8 / cppgc /
             Skia / libc++ frame;
- rust?:     no allocator shim seen (frame pointers may skip it) but the first
             caller looks like Rust;
- unknown:   the rest (JS frames, `[unknown]`).

Copies (`memcpy`, `memmove`, `memset` and their `__*_avx*` variants) are
counted apart: no allocator changes them.
"""

import collections
import re
import sys
from pathlib import Path

LIBC_ALLOC = re.compile(
    r"^(__libc_)?(malloc|free|cfree|realloc|calloc|posix_memalign|aligned_alloc|memalign|valloc)$"
    r"|^_int_|^_mid_memalign$|^malloc_consolidate$|^tcache|^unlink_chunk$|^sysmalloc|^__malloc_"
    r"|^arena_|^__default_morecore$|^__(s)?brk$|^(s)?brk$|^alloc_perturb$|^munmap_chunk$|^mremap_chunk$"
    r"|^__GI___libc_(malloc|free|realloc|calloc)$|^_int_free"
)
LIBC_COPY = re.compile(r"^(__)?(memcpy|memmove|memset|mempcpy|wmemset)(_.*)?$|^__mem(cpy|move|set)_")
RUST_SHIM = re.compile(
    r"^__rust_(alloc|dealloc|realloc|alloc_zeroed)$|^__rdl_|^__rg_|^alloc::alloc::|^<alloc::alloc::Global"
    r"|^alloc::raw_vec::|^std::sys::(pal::\w+::)?alloc|^std::alloc::|^<std::alloc::System"
)
CXX_SHIM = re.compile(r"^operator (new|delete)|^_Zn[wa]m|^_Zd[la]Pv|^sk_(malloc|free|realloc|calloc)|^std::__\w+::__libcpp_")
BINDING = re.compile(r"^(cppgc|v8)__\w+")
CXX_OWNER = re.compile(r"^(v8|cppgc|heap|std::__\w+|icu_?\d*|skia|SkSL|Sk\w+|Gr\w+|sk_\w+|Builtins_|v8_inspector)\b|^Sk[A-Z]|^Builtin")


def short(frame: str) -> str:
    return re.sub(r"::h[0-9a-f]{16}$", "", frame)[:160]


def classify(frames: list[str]) -> tuple[str, str]:
    """(class, the first frame above the allocator) of a stack, leaf first."""
    shim = None
    for frame in frames[1:]:
        if LIBC_ALLOC.search(frame):
            continue
        if RUST_SHIM.search(frame):
            shim = shim or "rust"
            continue
        if CXX_SHIM.search(frame):
            shim = shim or "c++"
            continue
        if BINDING.search(frame):
            return "binding", short(frame)
        if shim:
            return shim, short(frame)
        if CXX_OWNER.search(frame):
            return "c++", short(frame)
        if "::" in frame:
            return "rust?", short(frame)
        return "unknown", short(frame)
    return (shim or "unknown"), "(none)"


CLASSES = ["rust", "rust?", "binding", "c++", "unknown"]


def analyze(path: Path) -> dict:
    total = 0
    alloc = collections.Counter()
    copy = 0
    owners = {c: collections.Counter() for c in CLASSES}
    copy_owners = collections.Counter()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        stack, _, count = line.rpartition(" ")
        count = int(count)
        total += count
        frames = list(reversed(stack.split(";")))
        leaf = frames[0]
        if LIBC_ALLOC.search(leaf):
            cls, owner = classify(frames)
            alloc[cls] += count
            owners[cls][owner] += count
        elif LIBC_COPY.search(leaf):
            copy += count
            above = next((f for f in frames[1:] if not LIBC_COPY.search(f)), "(none)")
            copy_owners[short(above)] += count
    return {"total": total, "alloc": alloc, "copy": copy, "owners": owners, "copy_owners": copy_owners}


def pct(n: int, of: int) -> str:
    return f"{100 * n / of:.1f}" if of else "-"


def main() -> None:
    folder = Path(sys.argv[1])
    suites = sorted(p.name.removesuffix("-kun.folded") for p in folder.glob("*-kun.folded"))
    if not suites:
        sys.exit(f"no *-kun.folded in {folder}")
    out = ["# libc 的配置和複製，照呼叫者分", ""]
    for event, suffix in (("cycles", "-kun.folded"), ("instructions", "-kun.instructions.folded")):
        rows = [(s, analyze(folder / f"{s}{suffix}")) for s in suites if (folder / f"{s}{suffix}").exists()]
        if not rows:
            continue
        out += [f"## {event}（% 是佔那個 suite 計時區間的樣本）", "",
                "| suite | 樣本 | 配置 | rust | rust? | binding | c++ | unknown | 複製 |",
                "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
        whole = {"total": 0, "alloc": collections.Counter(), "copy": 0,
                 "owners": {c: collections.Counter() for c in CLASSES}, "copy_owners": collections.Counter()}
        for name, r in rows + [("合計", whole)]:
            if name != "合計":
                whole["total"] += r["total"]
                whole["alloc"] += r["alloc"]
                whole["copy"] += r["copy"]
                for c in CLASSES:
                    whole["owners"][c] += r["owners"][c]
                whole["copy_owners"] += r["copy_owners"]
            t = r["total"]
            a = r["alloc"]
            out.append(f"| {name} | {t} | {pct(sum(a.values()), t)} | "
                       + " | ".join(pct(a[c], t) for c in CLASSES) + f" | {pct(r['copy'], t)} |")
        if event == "cycles":
            out += [""]
            for c in CLASSES:
                top = whole["owners"][c].most_common(12)
                if top:
                    out += [f"### 合計：{c} 的配置，第一個呼叫者", "", "| 樣本 | 函式 |", "|---:|---|"]
                    out += [f"| {n} | `{f}` |" for f, n in top] + [""]
            out += ["### 合計：複製的呼叫者", "", "| 樣本 | 函式 |", "|---:|---|"]
            out += [f"| {n} | `{f}` |" for f, n in whole["copy_owners"].most_common(12)] + [""]
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
