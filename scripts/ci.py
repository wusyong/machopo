#!/usr/bin/env python3
"""The tools around the build: V8, the projects in ../third_party, and the
wasm guests.

    python scripts/ci.py v8        # build V8 (debug and release) from our rusty_v8 fork, to work on the fork
    python scripts/ci.py third-party  # sync ../third_party (our forks, WPT) to the commits machopo pins
    python scripts/ci.py patches NAME # export a fork's commits since its upstream to patches/NAME
    python scripts/ci.py guests    # rebuild the wasm guests (C) with wasi-sdk and wit-bindgen

Everything runs against the V8 that `.cargo/config.toml` names: the prebuilt
one from our rusty_v8 fork's GitHub Release (`RUSTY_V8_MIRROR`), or, with
`RUSTY_V8_ARCHIVE` set in the environment, one `v8` built locally. A plain
build links the release one. The checks of the repo's rules (`unsafe`,
`deps`, `spans`, `boundary`, `tests`, `numerics`, `docs`) are in `sanity.py`; the test runs `cargo test`
leaves out (the GC's `matrix` and `fuzz`, `automation`, the reftests, the
scenarios and WPT) in `test.py`; the benchmarks (Speedometer, the tile
raster) in `bench.py`.

Standard library only. Every command exits non-zero on failure.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Vendored and forked upstream code lives next to the repo, in
# `../third_party`, each project its own repository.
THIRD_PARTY = ROOT.parent / "third_party"
# A fork's commits since its upstream (`upstream_rev`), one `git
# format-patch` file each, in `patches/<name>/`: machopo's history keeps
# each version of each change, and its reviews see them.
PATCHES = ROOT / "patches"


# --- v8 ---------------------------------------------------------------------

V8_ARTIFACTS = THIRD_PARTY / "rusty_v8_artifacts"
# The target directory V8 is built from source in, outside machopo's `target/`:
# `cargo clean` would take ninja's objects with it (a full rebuild, half an
# hour), and machopo's own builds copy the prebuilt over the library there.
V8_BUILD = THIRD_PARTY / "rusty_v8_build"


def v8_feature_suffix() -> str:
    """rusty_v8's artifact name for the features the workspace enables on
    `v8` (its `prebuilt_features_suffix`)."""
    line = next(
        (l for l in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines() if l.startswith("v8 =")),
        "",
    )
    suffix = ""
    if "v8_enable_pointer_compression" in line or "v8_enable_sandbox" in line:
        suffix += "_ptrcomp"
    if "v8_enable_sandbox" in line:
        suffix += "_sandbox"
    return suffix


def cmd_v8(args: argparse.Namespace) -> int:
    """Builds V8 from the rusty_v8 fork (`V8_FROM_SOURCE=1`) and copies the
    static library and generated binding to `../third_party/rusty_v8_artifacts`
    under the names rusty_v8 looks for; builds link them instead of the
    prebuilt while `RUSTY_V8_ARCHIVE` points there. Builds a debug V8 (with
    V8's `DCHECK`s, for `V8_FORCE_DEBUG=1`) and a release one (what every
    other build links), or only the one `--profile` names. Only for working
    on the fork before it is released; needs all of its submodules. Builds
    in `../third_party/rusty_v8_build`, so ninja rebuilds only what changed;
    with sccache on the `PATH`, V8 compiles through it, so a build directory
    removed since comes back mostly from its cache."""
    env = {**os.environ, "V8_FROM_SOURCE": "1", "CARGO_TARGET_DIR": str(V8_BUILD)}
    if not (env.get("SCCACHE") or env.get("CCACHE") or shutil.which("sccache") or shutil.which("ccache")):
        print(f"No sccache or ccache: if {V8_BUILD} is removed, V8 is built again from scratch (`brew install sccache`)")
    if "LIBCLANG_PATH" not in env and sys.platform == "darwin":
        # bindgen needs libclang 21.1+; Chromium doesn't ship it for macOS.
        prefix = subprocess.run(["brew", "--prefix", "llvm"], capture_output=True, text=True)
        if prefix.returncode != 0:
            print("libclang 21.1+ is needed: `brew install llvm`, or set LIBCLANG_PATH")
            return 1
        env["LIBCLANG_PATH"] = prefix.stdout.strip() + "/lib"
    target = next(
        l.split(": ", 1)[1]
        for l in subprocess.run(["rustc", "-vV"], capture_output=True, text=True).stdout.splitlines()
        if l.startswith("host: ")
    )
    lib = "rusty_v8.lib" if sys.platform == "win32" else "librusty_v8.a"
    profiles = ["debug", "release"] if args.profile == "both" else [args.profile]
    for profile in profiles:
        start = time.monotonic()
        # Cargo reruns the `v8` build script (which runs gn and ninja) only
        # when something it tracks changed; touching a file it watches makes
        # sure V8 is rebuilt from the fork's current sources.
        os.utime(THIRD_PARTY / "rusty_v8" / "src" / "binding.cc")
        cargo = ["cargo", "build", "-p", "bindings"] + (["--release"] if profile == "release" else [])
        build = subprocess.run(cargo, cwd=ROOT, env=env)
        if build.returncode != 0:
            return build.returncode
        gn_out = V8_BUILD / profile / "gn_out"
        # Built from source, V8's `is_debug` follows cargo's profile; named
        # as rusty_v8 looks the artifact up (`debug` only for
        # `V8_FORCE_DEBUG`). On Windows both are release (rustc can't link a
        # debug V8 there).
        v8_profile = "release" if sys.platform == "win32" else profile
        name = f"{v8_feature_suffix()}_{v8_profile}_{target}"
        lib_name = f"rusty_v8{name}.lib" if sys.platform == "win32" else f"librusty_v8{name}.a"
        V8_ARTIFACTS.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(gn_out / "obj" / lib, V8_ARTIFACTS / lib_name)
        shutil.copyfile(gn_out / "src_binding.rs", V8_ARTIFACTS / f"src_binding{name}.rs")
        print(
            f"{profile} V8 built in {time.monotonic() - start:.0f}s; "
            f"copied {lib_name} and src_binding{name}.rs to {V8_ARTIFACTS}"
        )
    print(f"Link it instead of the prebuilt with RUSTY_V8_ARCHIVE={V8_ARTIFACTS}")
    return 0


# --- third-party ------------------------------------------------------------


@dataclass
class Pin:
    """A project in `../third_party`: its repository, the commit or tag machopo
    builds against, the submodules it needs, and whether its files must be
    upstream's byte for byte (no line-ending conversion)."""

    repository: str
    ref: str
    submodules: list[str] = field(default_factory=list)
    byte_exact: bool = False
    # rusty_v8's release tag, whose prebuilt V8 machopo links, when `ref` is a
    # commit past it: then the commits since the tag may change only what
    # isn't in the prebuilt (`PAST_PREBUILT`).
    prebuilt_tag: str | None = None
    # A folder of the repository rather than a checkout of it: what gitiles
    # gives of it at `ref` as a tarball (`sync_archive`).
    archive: str | None = None
    # A fork's upstream and the upstream commit its branch starts from:
    # `ref` is that commit and the patches in `patches/<name>/`.
    upstream: str | None = None
    upstream_rev: str | None = None


# What the fork's commits past its release tag may change: its Rust and its
# docs, which machopo builds or reads from the checkout, and its CI, which only
# builds the next release. Anything else (the C++ glue, `Cargo.toml`'s
# features, which pick the build of V8, the `v8` submodule) is in the
# prebuilt V8, so it needs a new release.
PAST_PREBUILT = re.compile(r"\.github/.*|.*\.(rs|md)")


def third_party_pins() -> dict[str, Pin]:
    """Each project in `../third_party`. The forks', elise's and WPT's from
    `third_party.toml`; rusty_v8's repository and release tag from
    `RUSTY_V8_MIRROR` and `RUSTY_V8_MIRROR_TAG` in `.cargo/config.toml`, so
    its source matches the prebuilt V8, and its commit from
    `third_party.toml` when one is past the tag (else the tag). Only
    rusty_v8's `v8` submodule: `bindings`' build script reads V8's sources;
    the V8 library itself is downloaded."""
    import tomllib

    pins = tomllib.loads((ROOT / "third_party.toml").read_text(encoding="utf-8"))
    rusty_v8 = pins.pop("rusty_v8", {})
    result = {
        name: Pin(
            pin["repository"],
            pin["rev"],
            byte_exact=pin.get("byte_exact", False),
            archive=pin.get("archive"),
            upstream=pin.get("upstream"),
            upstream_rev=pin.get("upstream_rev"),
        )
        for name, pin in pins.items()
    }
    config = (ROOT / ".cargo" / "config.toml").read_text(encoding="utf-8")
    mirror = re.search(r'^RUSTY_V8_MIRROR = "([^"]+)/releases/download"', config, re.M)
    tag = re.search(r'^RUSTY_V8_MIRROR_TAG = "([^"]+)"', config, re.M)
    if not mirror or not tag:
        raise SystemExit(".cargo/config.toml has no RUSTY_V8_MIRROR / RUSTY_V8_MIRROR_TAG")
    rev = rusty_v8.get("rev")
    result["rusty_v8"] = Pin(mirror.group(1), rev or tag.group(1), ["v8"], prebuilt_tag=tag.group(1) if rev else None)
    return result


# What a folder downloaded from an archive holds of the commit it is of.
ARCHIVE_REV = ".machopo-rev"
# gitiles' tarballs now and then come back cut short (seen on CI's
# runners, where another job's download of the same one was whole): an
# archive is downloaded again, a few seconds apart, before giving up.
ARCHIVE_ATTEMPTS = 3
ARCHIVE_RETRY_SECONDS = 5


def sync_archive(name: str, path: Path, pin: Pin) -> list[str]:
    """`path` as `pin.archive` (a folder of `pin.repository`) is at
    `pin.ref`: downloaded from gitiles' `+archive` when it is missing, or
    of another commit (its `ARCHIVE_REV`), into a folder of its own, then
    put in place of the old one; a download that fails or comes back cut
    short is tried again (`ARCHIVE_ATTEMPTS`). A folder without
    `ARCHIVE_REV` isn't one this made, so it is left as it is, with why.
    Why it couldn't, if it couldn't."""
    import http.client
    import io
    import urllib.error
    import zlib

    marker = path / ARCHIVE_REV
    # Named `.kun-rev` before the project was renamed: the same marker.
    if not marker.exists() and (path / ".kun-rev").exists():
        (path / ".kun-rev").rename(marker)
    if marker.is_file() and marker.read_text(encoding="utf-8").strip() == pin.ref:
        return []
    if path.exists() and not marker.is_file():
        return [f"{name}: {path} isn't a download of {pin.archive} ({ARCHIVE_REV} is missing): move it aside"]
    url = f"{pin.repository}/+archive/{pin.ref}/{pin.archive}.tar.gz"
    print(f"{name}: downloading {pin.archive} at {pin.ref[:12]} into {path}")
    fresh = path.with_name(path.name + ".new")
    for attempt in range(1, ARCHIVE_ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(url, timeout=300) as response:
                data = response.read()
            shutil.rmtree(fresh, ignore_errors=True)
            fresh.mkdir(parents=True)
            # Reading the whole archive is what tells a cut-short one:
            # gitiles sends no length to check the download against.
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
                tar.extractall(fresh, filter="data")
            break
        except urllib.error.HTTPError as error:
            # Asked for what isn't there (a wrong `rev` or folder):
            # asking again wouldn't change the answer.
            if error.code < 500:
                return [f"{name}: couldn't download {url}: {error}"]
            failure: Exception = error
        except (OSError, EOFError, tarfile.TarError, http.client.HTTPException, zlib.error) as error:
            failure = error
        shutil.rmtree(fresh, ignore_errors=True)
        if attempt == ARCHIVE_ATTEMPTS:
            return [f"{name}: couldn't download {url} in {attempt} tries: {failure}"]
        print(f"{name}: download {attempt} of {ARCHIVE_ATTEMPTS} failed ({failure}); trying again in {ARCHIVE_RETRY_SECONDS}s")
        time.sleep(ARCHIVE_RETRY_SECONDS)
    (fresh / ARCHIVE_REV).write_text(pin.ref + "\n", encoding="utf-8", newline="\n")
    if path.exists():
        shutil.rmtree(path)
    fresh.rename(path)
    return []


def git(cwd: Path, *args: str, check: bool = True) -> str:
    out = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and out.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} in {cwd}: {out.stderr.strip()}")
    return out.stdout.strip() if out.returncode == 0 else ""


def past_prebuilt_problems(name: str, path: Path, tag: str, commit: str) -> list[str]:
    """Why `commit` can't be built with `tag`'s prebuilt V8, if it can't:
    it doesn't descend from the tag, or changes since it what the prebuilt
    holds."""
    if not git(path, "rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}", check=False):
        return [f"{name}: the release tag {tag} is not in {path}; `git -C {path} fetch origin tag {tag}`"]
    ancestor = subprocess.run(["git", "merge-base", "--is-ancestor", tag, commit], cwd=path, capture_output=True)
    if ancestor.returncode != 0:
        return [f"{name}: {commit[:12]} doesn't descend from the release tag {tag}, whose prebuilt V8 machopo links"]
    changed = git(path, "diff", "--name-only", tag, commit).splitlines()
    beyond = [file for file in changed if not PAST_PREBUILT.fullmatch(file)]
    if beyond:
        return [
            f"{name}: {', '.join(beyond)} changed since {tag}, and the prebuilt V8 holds them: "
            "publish a new release and move RUSTY_V8_MIRROR_TAG to it (see the fork's README.machopo.md)"
        ]
    return []


def fetch_pin(path: Path, pin: Pin, shallow: bool) -> None:
    """Fetches the commit (or tag) `pin` names into the checkout at `path`,
    and rusty_v8's release tag with it. Shallowly into a shallow checkout
    (one this script cloned), whole into a whole one: a `--depth` would
    make a fork someone works on shallow."""
    ref = pin.ref
    # A tag is fetched as itself, so it resolves like in a full clone.
    target = ref if re.fullmatch(r"[0-9a-f]{40}", ref) else f"refs/tags/{ref}:refs/tags/{ref}"
    tag = pin.prebuilt_tag
    if not shallow:
        if tag:
            git(path, "fetch", "--quiet", "origin", f"refs/tags/{tag}:refs/tags/{tag}")
        git(path, "fetch", "--quiet", "origin", target)
    elif tag:
        # The tag, then the commits since it down to the tag's own
        # (`--shallow-exclude` alone stops short of it), so the checks
        # can tell the commit descends from the tag.
        git(path, "fetch", "--quiet", "--depth", "1", "origin", f"refs/tags/{tag}:refs/tags/{tag}")
        git(path, "fetch", "--quiet", f"--shallow-exclude={tag}", "origin", ref)
        git(path, "fetch", "--quiet", "--deepen=1", "origin", ref)
    else:
        git(path, "fetch", "--quiet", "--depth", "1", "origin", target)


def update_submodules(path: Path, pin: Pin) -> None:
    for submodule in pin.submodules:
        # V8 has paths past Windows' 260-character limit.
        git(path, "-c", "core.longpaths=true", "submodule", "update", "--init", "--depth", "1", submodule)


def clone_pin(path: Path, pin: Pin) -> None:
    """A shallow checkout of `pin` at `path`, which doesn't exist yet."""
    path.mkdir(parents=True)
    git(path, "init", "--quiet")
    # WPT has paths past Windows' 260-character limit too.
    git(path, "config", "core.longpaths", "true")
    if pin.byte_exact:
        git(path, "config", "core.autocrlf", "false")
    git(path, "remote", "add", "origin", pin.repository)
    fetch_pin(path, pin, shallow=True)
    git(path, "checkout", "--quiet", "--detach", pin.ref)
    update_submodules(path, pin)


def own_work(path: Path, head: str, want: str) -> str | None:
    """Why the checkout at `path` holds work of its own that switching it to
    `want` would leave, if it does: changes not committed (untracked files
    aside: a fork holds its builds' leftovers), or a HEAD `origin` doesn't
    have. A HEAD `want` descends from (an older pin) is `origin`'s, and so
    is one a remote-tracking branch holds; else `origin` is asked for it.
    (A remote-tracking branch alone won't do: this script's shallow clones
    fetch commits, not branches, so they have none.)"""
    changed = git(path, "status", "--porcelain", "--untracked-files=no", "--ignore-submodules=all")
    if changed:
        # `git` strips the output, the first line's leading space too.
        files = [line.split(maxsplit=1)[1] for line in changed.splitlines()]
        more = f" and {len(files) - 3} more" if len(files) > 3 else ""
        return f"changes not committed ({', '.join(files[:3])}{more})"
    if subprocess.run(["git", "merge-base", "--is-ancestor", head, want], cwd=path, capture_output=True).returncode == 0:
        return None
    if git(path, "branch", "-r", "--contains", head, check=False):
        return None
    if not origin_has(path, head):
        return f"commit {head[:12]}, which origin doesn't have (not pushed), or which origin couldn't be asked for"
    return None


def origin_has(path: Path, commit: str) -> bool:
    """Whether the checkout's `origin` has `commit`: fetched into an empty
    repository, since one that has it answers from its own objects without
    asking; only the commit, not its tree (WPT's is large)."""
    url = git(path, "remote", "get-url", "origin")
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as empty:
        git(Path(empty), "init", "--quiet")
        fetch = ["git", "fetch", "--quiet", "--depth", "1", "--filter=tree:0", url, commit]
        return subprocess.run(fetch, cwd=empty, capture_output=True).returncode == 0


# `git format-patch`'s options, and the configuration that would change its
# output otherwise: the same commits give the same files on every machine
# and every time, so a patch that didn't change shows no diff. Not numbered
# (`[PATCH]`): a commit added doesn't change every subject. Full blob ids:
# abbreviated ones grow with the repository (a shallow checkout's and a
# whole one's differ). Binary files (fonts) are binary diffs, as
# `format-patch` gives them unasked.
FORMAT_PATCH = [
    "-c", "diff.noprefix=false",
    "-c", "diff.mnemonicPrefix=false",
    "-c", "diff.algorithm=myers",
    "-c", "diff.renames=true",
    "-c", "core.quotePath=true",
    "format-patch", "--quiet", "--zero-commit", "--no-signature", "--no-numbered",
    "--full-index", "--no-color",
]  # fmt: skip


def patch_files(directory: Path) -> list[Path]:
    """The patches in `directory`, in the order they apply."""
    return sorted(directory.glob("*.patch")) if directory.is_dir() else []


def has_commit(path: Path, commit: str) -> bool:
    return subprocess.run(["git", "cat-file", "-e", f"{commit}^{{commit}}"], cwd=path, capture_output=True).returncode == 0


def fetch_commit(path: Path, commit: str) -> None:
    """`commit` from `origin` into the checkout at `path`, shallowly into a
    shallow one. A fork on GitHub has its upstream's commits."""
    shallow = git(path, "rev-parse", "--is-shallow-repository") == "true"
    git(path, "fetch", "--quiet", *(["--depth", "1"] if shallow else []), "origin", commit)


def shown(directory: Path) -> str:
    return directory.relative_to(ROOT).as_posix() if directory.is_relative_to(ROOT) else str(directory)


def patched_tree(path: Path, base: str, patches: list[Path]) -> tuple[str | None, str | None]:
    """The tree `patches` make of `base`'s, in a scratch index of the
    checkout at `path` (its own index and files stay as they are), or the
    patch that doesn't apply and why."""
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as scratch:
        env = {**os.environ, "GIT_INDEX_FILE": str(Path(scratch) / "index")}

        def run(*args: str) -> subprocess.CompletedProcess[str]:
            return subprocess.run(["git", *args], cwd=path, env=env, capture_output=True, text=True)

        read = run("read-tree", base)
        if read.returncode != 0:
            return None, f"couldn't read {base[:12]}'s tree: {read.stderr.strip()}"
        for patch in patches:
            applied = run("apply", "--cached", "--whitespace=nowarn", str(patch))
            if applied.returncode != 0:
                return None, f"{patch.name} doesn't apply: {applied.stderr.strip()}"
        written = run("write-tree")
        if written.returncode != 0:
            return None, f"couldn't write the tree: {written.stderr.strip()}"
        return written.stdout.strip(), None


def patch_problems(name: str, path: Path, pin: Pin, directory: Path) -> list[str]:
    """Why the patches in `directory` on `pin.upstream_rev` don't give
    `pin.ref`'s tree, if they don't. The upstream commit is fetched from the
    fork's `origin` when the checkout hasn't it."""
    patches = patch_files(directory)
    if not pin.upstream_rev:
        if patches:
            return [f"{name}: {shown(directory)} has patches, but third_party.toml gives {name} no upstream_rev"]
        return []
    again = f"export them again from the checkout with `python scripts/ci.py patches {name}`"
    if not has_commit(path, pin.upstream_rev):
        try:
            fetch_commit(path, pin.upstream_rev)
        except SystemExit as error:
            return [f"{name}: couldn't fetch upstream_rev {pin.upstream_rev[:12]} into {path}: {error}"]
    tree, why = patched_tree(path, pin.upstream_rev, patches)
    if why:
        return [f"{name}: in {shown(directory)}, {why}; {again}"]
    want = git(path, "rev-parse", f"{pin.ref}^{{tree}}")
    if tree != want:
        changed = git(path, "diff-tree", "-r", "--name-only", tree, want).splitlines()
        more = f" and {len(changed) - 3} more" if len(changed) > 3 else ""
        return [
            f"{name}: upstream_rev {pin.upstream_rev[:12]} and the patches in {shown(directory)} don't give "
            f"{pin.ref[:12]}'s files ({', '.join(changed[:3])}{more} differ); {again}"
        ]
    return []


def export_patches(path: Path, base: str, head: str, directory: Path) -> list[Path]:
    """The commits from `base` to `head` in the checkout at `path`, one patch
    each, in `directory` in place of the ones there. A shallow checkout is
    deepened first: the commits between are needed."""
    if git(path, "rev-parse", "--is-shallow-repository") == "true":
        git(path, "fetch", "--quiet", "--unshallow", "origin", head)
    if subprocess.run(["git", "merge-base", "--is-ancestor", base, head], cwd=path, capture_output=True).returncode != 0:
        raise SystemExit(
            f"{head[:12]} doesn't descend from upstream_rev {base[:12]}: "
            "rebase the fork onto it, or move upstream_rev to the commit it is on"
        )
    for old in patch_files(directory):
        old.unlink()
    directory.mkdir(parents=True, exist_ok=True)
    git(path, *FORMAT_PATCH, "--output-directory", str(directory), f"{base}..{head}")
    return patch_files(directory)


def cmd_patches(args: argparse.Namespace) -> int:
    """Exports a fork's commits since its upstream (`upstream_rev`) to
    `patches/<name>/`, from its checkout, which must be at the commit machopo
    pins: move the pin first, so that the patches and the pin go in the same
    change. Then checks them, as `third-party` does."""
    name = args.name
    pin = third_party_pins().get(name)
    if not pin or not pin.upstream_rev:
        print(f"{name}: third_party.toml gives it no upstream_rev")
        return 1
    path = THIRD_PARTY / name
    want = git(path, "rev-parse", "--verify", "--quiet", f"{pin.ref}^{{commit}}", check=False)
    head = git(path, "rev-parse", "HEAD")
    if want != head:
        print(
            f"{name}: {path} is at {head[:12]}, but third_party.toml pins {pin.ref[:12]}: "
            "move the pin to the commit to export, or check that one out"
        )
        return 1
    if not has_commit(path, pin.upstream_rev):
        fetch_commit(path, pin.upstream_rev)
    directory = PATCHES / name
    files = export_patches(path, pin.upstream_rev, head, directory)
    problems = patch_problems(name, path, pin, directory)
    for problem in problems:
        print(problem)
    if not problems:
        print(f"{name}: {len(files)} patch{'' if len(files) == 1 else 'es'} in {shown(directory)}, which give {head[:12]}")
    return 1 if problems else 0


def cmd_third_party(args: argparse.Namespace) -> int:
    """Syncs each project in `../third_party` to the commit machopo builds
    against (`third_party.toml` as it is in the working tree, committed or
    not), as `gclient sync` does: clones a missing one (shallow, at that
    commit), and switches one at another commit to it, fetching it first.
    A checkout that holds work of its own (changes not committed, or a
    commit its `origin` doesn't have) is left as it is, with why; push the
    work and move machopo's pin to it, or put it aside. Then checks that a
    rusty_v8 commit past the release tag changes only what the prebuilt V8
    doesn't hold (the fork's Rust and docs), unless `RUSTY_V8_ARCHIVE` is
    set: builds then link a V8 built locally from the checkout; and that a
    fork with an `upstream_rev` is that commit and its patches in
    `patches/<name>/` (`patch_problems`). Every `test.py` command runs this
    first: each builds machopo, which builds the forks by path."""
    problems = []
    for name, pin in third_party_pins().items():
        ref = pin.ref
        path = THIRD_PARTY / name
        if pin.archive:
            problems += sync_archive(name, path, pin)
            continue
        if not path.exists():
            print(f"{name}: cloning {pin.repository} at {ref} into {path}")
            clone_pin(path, pin)
        shallow = git(path, "rev-parse", "--is-shallow-repository") == "true"
        want = git(path, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}", check=False)
        if not want or (pin.prebuilt_tag and not git(path, "tag", "--list", pin.prebuilt_tag)):
            try:
                fetch_pin(path, pin, shallow)
            except SystemExit as error:
                problems.append(f"{name}: couldn't fetch {ref} into {path}: {error}")
                continue
            want = git(path, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}", check=False)
            if not want:
                problems.append(f"{name}: {ref} isn't in {pin.repository}")
                continue
        head = git(path, "rev-parse", "HEAD")
        if head != want:
            work = own_work(path, head, want)
            if work:
                problems.append(
                    f"{name}: left at {head[:12]} (machopo builds against {ref[:12]}): {path} holds {work}; "
                    "push it and move machopo's pin to it in third_party.toml, or put it aside"
                )
                continue
            print(f"{name}: switching {path} from {head[:12]} to {want[:12]}")
            git(path, "checkout", "--quiet", "--detach", want)
            update_submodules(path, pin)
        problems += patch_problems(name, path, pin, PATCHES / name)
        if want and pin.prebuilt_tag:
            if os.environ.get("RUSTY_V8_ARCHIVE"):
                # Builds link a V8 built locally (`ci.py v8`) instead of the
                # tag's prebuilt, so the commit may change what it holds.
                print(f"{name}: RUSTY_V8_ARCHIVE is set, so {want[:12]} may change what {pin.prebuilt_tag}'s prebuilt V8 holds")
            else:
                problems += past_prebuilt_problems(name, path, pin.prebuilt_tag, want)
        if pin.byte_exact and git(path, "config", "--get", "core.autocrlf", check=False) in ("true", "input"):
            problems.append(
                f"{name}: core.autocrlf converts line endings in {path}, but its files must be upstream's; "
                f"`git -C {path} config core.autocrlf false`, then `git -C {path} rm -rq --cached .` and `git -C {path} reset -q --hard`"
            )
        for submodule in pin.submodules:
            if not any((path / submodule).iterdir()):
                problems.append(
                    f"{name}: submodule {submodule} is empty; "
                    f"`git -C {path} submodule update --init --depth 1 {submodule}`"
                )
    for problem in problems:
        print(problem)
    if not problems:
        print("third_party matches third_party.toml, its patches and RUSTY_V8_MIRROR_TAG")
    return 1 if problems else 0


# --- guests -------------------------------------------------------------------

# The DOM's C guests (`dom_wasm`'s tests): each `<name>.c` there is
# built into `<name>.wasm` next to it, against `machopo:dom`'s WIT
# (`DOM_WIT`, the files of its package) and its world (`GUEST_WORLDS`: the
# tests' own, `wit/test.wit` there, the interface imported and `run`
# exported; or a page's script's), with wasi-sdk's clang into WASI 0.3 components
# (as a page's guests: their stores are async), checked in next to their source (the
# tests don't build them). At these versions only: the builds checked in
# depend on them.
GUESTS = ROOT / "crates" / "dom_wasm" / "tests" / "guests"
DOM_WIT = ROOT / "crates" / "dom_wasm" / "wit"
# Each guest's world, and the name wit-bindgen gives its files and
# functions (the world's own).
GUEST_WORLDS = {
    "dom": ("test", "test"),
    "page": ("machopo:dom/script@0.1.0", "script"),
    "todomvc": ("machopo:dom/script@0.1.0", "script"),
}
WASI_SDK_VERSION = "34.0"
WIT_BINDGEN_VERSION = "0.62.0"
# The same everywhere: the name section stays (traps show function names),
# DWARF doesn't; no checkout paths. The linker's flags apart, for what's
# only compiled (`-Werror` rejects them there).
CLANG_FLAGS = [
    "--target=wasm32-wasip3",
    "-O2",
    "-DNDEBUG",
    "-Wall",
    "-Werror",
    f"-ffile-prefix-map={ROOT}=/machopo",
]
CLANG_LINK_FLAGS = ["-Wl,--strip-debug"]


def wasi_sdk_clang() -> Path | str:
    """wasi-sdk's clang (`WASI_SDK_PATH`, else `../third_party/wasi-sdk-<version>`),
    or why it can't be used."""
    sdk = Path(os.environ.get("WASI_SDK_PATH") or THIRD_PARTY / f"wasi-sdk-{WASI_SDK_VERSION}")
    version_file = sdk / "VERSION"
    if not version_file.exists():
        return f"no wasi-sdk at {sdk} (set WASI_SDK_PATH)"
    version = version_file.read_text().split()[0]
    if version != WASI_SDK_VERSION:
        return f"{sdk} is wasi-sdk {version}, not {WASI_SDK_VERSION}"
    return sdk / "bin" / ("clang.exe" if sys.platform == "win32" else "clang")


def wit_bindgen() -> Path | str:
    """wit-bindgen (`WIT_BINDGEN`, or on PATH), or why it can't be used."""
    found = os.environ.get("WIT_BINDGEN") or shutil.which("wit-bindgen")
    if not found:
        return "wit-bindgen isn't on PATH and WIT_BINDGEN isn't set"
    out = subprocess.run([found, "--version"], capture_output=True, text=True).stdout.split()
    if out[1:2] != [WIT_BINDGEN_VERSION]:
        return f"wit-bindgen is {' '.join(out[1:2]) or 'unknown'}, not {WIT_BINDGEN_VERSION}"
    return Path(found)


def cmd_guests(_args: argparse.Namespace) -> int:
    """Rebuilds every `tests/guests/<name>.c` of `dom_wasm` into
    `<name>.wasm`."""
    clang, bindgen = wasi_sdk_clang(), wit_bindgen()
    for tool in (clang, bindgen):
        if isinstance(tool, str):
            print(tool)
            return 1
    built = build_guests(clang, bindgen)
    if built is None:
        return 1
    for module in built:
        print(f"built {module.relative_to(ROOT)}")
    return 0


def build_guests(clang: Path, bindgen: Path) -> list[Path] | None:
    """Builds the guests (`GUESTS`); what it built, or none if
    it failed."""
    out = ROOT / "target" / "guests" / "dom"
    shutil.rmtree(out, ignore_errors=True)
    wit = out / "wit"
    (wit / "deps").mkdir(parents=True)
    shutil.copyfile(GUESTS / "wit" / "test.wit", wit / "test.wit")
    shutil.copytree(DOM_WIT, wit / "deps" / "machopo-dom")
    # clang runs wasm-opt when it's on PATH: only wasi-sdk's tools, so the
    # build doesn't depend on what else is installed.
    env = {**os.environ, "PATH": str(clang.parent)}
    built = []
    for source in sorted(GUESTS.glob("*.c")):
        world, name = GUEST_WORLDS[source.stem]
        gen = out / "gen" / source.stem
        cmd = [str(bindgen), "c", str(wit), "--world", world, "--out-dir", str(gen)]
        print(" ".join(cmd), flush=True)
        if subprocess.run(cmd, cwd=ROOT).returncode != 0:
            return None
        module = source.with_suffix(".wasm")
        cmd = [
            str(clang),
            *CLANG_FLAGS,
            *CLANG_LINK_FLAGS,
            f"-I{gen}",
            str(source),
            str(gen / f"{name}.c"),
            str(gen / f"{name}_component_type.o"),
            "-o",
            str(module),
        ]
        print(" ".join(cmd), flush=True)
        if subprocess.run(cmd, cwd=ROOT, env=env).returncode != 0:
            return None
        built.append(module)
    return built


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    v8 = sub.add_parser("v8", help="build V8 from the rusty_v8 fork, to link with RUSTY_V8_ARCHIVE")
    v8.add_argument(
        "--profile",
        choices=["both", "debug", "release"],
        default="both",
        help="which V8 to build: debug (for V8_FORCE_DEBUG=1), release (every other build), or both (default)",
    )
    v8.set_defaults(func=cmd_v8)
    third_party = sub.add_parser(
        "third-party",
        help="sync ../third_party to the commits machopo pins (our forks, WPT, Speedometer), leaving work of its own",
    )
    third_party.set_defaults(func=cmd_third_party)
    patches = sub.add_parser(
        "patches",
        help="export a fork's commits since its upstream_rev to patches/NAME, from its checkout at the pinned commit",
    )
    patches.add_argument("name", help="the fork, as third_party.toml names it (taffy, parley)")
    patches.set_defaults(func=cmd_patches)
    guests = sub.add_parser("guests", help="rebuild dom_wasm's test guests (C) with wasi-sdk and wit-bindgen")
    guests.set_defaults(func=cmd_guests)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
