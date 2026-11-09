#!/usr/bin/env python3
"""Builds the release V8 that stage 1c links: rusty_v8's checkout (at
../third_party/rusty_v8, as machopo pins it) with v8.patch (V8 itself: an
embedder heap marking with major GCs, include/v8-machopo.h) and glue.patch
(rusty_v8's C++: C functions for it) applied for the build and reverted
after, even on failure; patches already applied (by hand, while working on
them) are left so. Nothing is committed to the fork.

It builds as `ci.py v8 --profile release` does, in the same ninja
directory (../third_party/rusty_v8_build: only the patched files and those
including them are rebuilt; the next `ci.py v8` rebuilds them back), but
copies the library and binding into machopo's target/v8-0088/ instead of
../third_party/rusty_v8_artifacts, which every build of machopo on this machine
links. The 0088 branch links it (`.cargo/config.toml`'s RUSTY_V8_ARCHIVE).

`build.py save` writes the patches from the checkouts' changes instead.
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import ci  # noqa: E402

RUSTY_V8 = ci.THIRD_PARTY / "rusty_v8"
OUT = ROOT / "target" / "v8-0088"
# (checkout, patch, files the patch adds).
PATCHES = [
    (RUSTY_V8, HERE / "glue.patch", []),
    (RUSTY_V8 / "v8", HERE / "v8.patch", ["include/v8-machopo.h"]),
]


def git(path: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(path), *args], capture_output=True, text=True, check=check)


def applied(path: Path, patch: Path) -> bool:
    return git(path, "apply", "--check", "--reverse", str(patch), check=False).returncode == 0


def save() -> int:
    for path, patch, added in PATCHES:
        # New files show in the diff once added with intent to add.
        for file in added:
            git(path, "add", "--intent-to-add", file)
        diff = git(path, "diff", "--ignore-submodules=all", "--", ".", ":(exclude)gen").stdout
        for file in added:
            git(path, "reset", "--quiet", "--", file, check=False)
        with open(patch, "w", newline="\n") as f:
            f.write(diff)
        print(f"wrote {patch.name}: {len(diff.splitlines())} lines")
    return 0


def build() -> int:
    ours = []
    for path, patch, _ in PATCHES:
        if applied(path, patch):
            continue
        result = git(path, "apply", str(patch), check=False)
        if result.returncode != 0:
            print(f"{patch.name} doesn't apply to {path}:\n{result.stderr}")
            revert(ours)
            return 1
        ours.append((path, patch))
    try:
        env = {**os.environ, "V8_FROM_SOURCE": "1", "CARGO_TARGET_DIR": str(ci.V8_BUILD)}
        if "LIBCLANG_PATH" not in env and sys.platform == "darwin":
            prefix = subprocess.run(["brew", "--prefix", "llvm"], capture_output=True, text=True, check=True)
            env["LIBCLANG_PATH"] = prefix.stdout.strip() + "/lib"
        target = next(
            l.split(": ", 1)[1]
            for l in subprocess.run(["rustc", "-vV"], capture_output=True, text=True, cwd=ROOT).stdout.splitlines()
            if l.startswith("host: ")
        )
        start = time.monotonic()
        os.utime(RUSTY_V8 / "src" / "binding.cc")
        build = subprocess.run(["cargo", "build", "-p", "bindings", "--release"], cwd=ROOT, env=env)
        if build.returncode != 0:
            return build.returncode
        gn_out = ci.V8_BUILD / "release" / "gn_out"
        name = f"{ci.v8_feature_suffix()}_release_{target}"
        OUT.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(gn_out / "obj" / "librusty_v8.a", OUT / f"librusty_v8{name}.a")
        shutil.copyfile(gn_out / "src_binding.rs", OUT / f"src_binding{name}.rs")
        print(f"patched V8 built in {time.monotonic() - start:.0f}s; RUSTY_V8_ARCHIVE={OUT}")
        return 0
    finally:
        revert(ours)


def revert(ours: list[tuple[Path, Path]]) -> None:
    for path, patch in reversed(ours):
        if applied(path, patch):
            git(path, "apply", "--reverse", str(patch))
    # A new file was added with intent to add; the reverse patch deleted the
    # file but not the index entry.
    for path, patch, added in PATCHES:
        if (path, patch) in ours:
            for file in added:
                git(path, "reset", "--quiet", "--", file, check=False)


if __name__ == "__main__":
    sys.exit(save() if sys.argv[1:] == ["save"] else build())
