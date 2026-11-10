"""What the benchmarks share: Chrome for Testing (downloaded under
`target/`), machopo's build, the CDP pipes that drive both browsers (Chrome's
`--remote-debugging-pipe`, machopo's `--automation-pipe`), a page in a new
browser (`run_in_chrome`, `run_in_machopo`), the server the pages come from and
post to, reading Perfetto traces, sampling with Linux's `perf` or samply
(macOS, Windows), running as Administrator on Windows, and the machine's
description for the reports.

Standard library only.
"""

from __future__ import annotations

import base64
import collections
import contextlib
import ctypes
import json
import math
import os
import platform
import queue
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import zipfile
from datetime import date
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

# This file is scripts/bench/common.py.
ROOT = Path(__file__).resolve().parents[2]
# `summary.mjs`, beside this file, and where pages load it from.
SUMMARY_FILE = Path(__file__).resolve().parent / "summary.mjs"
SUMMARY = "/__machopo__/summary.mjs"


# Chrome for Testing of the V8 milestone machopo links (V8 15.2: Chrome 152).
CHROME_VERSION = "152.0.7977.82"


CHROME_DIR = ROOT / "target" / "chrome-for-testing" / CHROME_VERSION


# What machopo is measured with: release with ThinLTO (Cargo.toml).
MACHOPO_PROFILE = "perf"


# Both browsers' windows, CSS px; Speedometer's frame is 800×600 in them.
WINDOW = (1200, 900)


def chrome_platform() -> str:
    machine = platform.machine().lower()
    if sys.platform.startswith("linux"):
        return "linux64"
    if sys.platform == "darwin":
        return "mac-arm64" if machine in ("arm64", "aarch64") else "mac-x64"
    if sys.platform == "win32":
        return "win64"
    raise SystemExit(f"no Chrome for Testing for {sys.platform} {machine}")


def chrome_path() -> Path:
    """Where Chrome for Testing `CHROME_VERSION`'s binary is, once
    downloaded."""
    plat = chrome_platform()
    base = CHROME_DIR / f"chrome-{plat}"
    return {
        "linux64": base / "chrome",
        "win64": base / "chrome.exe",
    }.get(plat, base / "Google Chrome for Testing.app" / "Contents" / "MacOS" / "Google Chrome for Testing")


def chrome_binary() -> Path:
    """Chrome for Testing `CHROME_VERSION`, downloaded once; on Windows, its
    folder readable by Chrome's sandbox (`let_app_containers_read`)."""
    binary = chrome_path()
    if not binary.exists():
        download_chrome()
    if sys.platform == "win32":
        let_app_containers_read(CHROME_DIR)
    return binary


def download_chrome() -> None:
    """Chrome for Testing `CHROME_VERSION`, unpacked into `CHROME_DIR`."""
    plat = chrome_platform()
    url = f"https://storage.googleapis.com/chrome-for-testing-public/{CHROME_VERSION}/{plat}/chrome-{plat}.zip"
    print(f"downloading Chrome for Testing {CHROME_VERSION} ({plat})")
    CHROME_DIR.mkdir(parents=True, exist_ok=True)
    archive = CHROME_DIR / f"chrome-{plat}.zip"
    with urllib.request.urlopen(url) as response, open(archive, "wb") as out:
        shutil.copyfileobj(response, out)
    with zipfile.ZipFile(archive) as zip_file:
        for info in zip_file.infolist():
            path = Path(zip_file.extract(info, CHROME_DIR))
            # zipfile drops the permissions: put back the executable bits.
            mode = info.external_attr >> 16
            if mode:
                path.chmod(mode & 0o777)
    archive.unlink()


# The groups of every app container (ALL APPLICATION PACKAGES and ALL
# RESTRICTED APPLICATION PACKAGES), by SID: `icacls` names them in the
# system's language.
APP_CONTAINERS = ("S-1-15-2-1", "S-1-15-2-2")


def let_app_containers_read(folder: Path) -> None:
    """`folder` and what's in it readable by the app containers (Windows),
    unless it already is. Chrome's sandbox runs its utility processes (the
    network service's) in app containers, which must read `chrome.exe` and
    its DLLs: `C:\\Program Files`, where Chrome is installed, lets them, a
    folder under the user's profile doesn't, and the page can't load."""
    if all(inherited_read(folder, sid) for sid in APP_CONTAINERS):
        return
    grants = [f"*{sid}:(OI)(CI)(RX)" for sid in APP_CONTAINERS]
    subprocess.run(["icacls", str(folder), "/grant", *grants, "/q"], check=True, stdout=subprocess.DEVNULL)


def inherited_read(folder: Path, sid: str) -> bool:
    """Whether `folder`'s DACL lets `sid` read and run it, and passes that on
    to the files and folders in it."""
    # The SDDL names some SIDs by an alias: ALL APPLICATION PACKAGES' is AC.
    names = {sid, {"S-1-15-2-1": "AC"}.get(sid, sid)}
    for flags, rights, trustee in re.findall(r"\(A;([A-Z]*);([^;]*);[^;]*;[^;]*;([^;)]*)\)", dacl(folder)):
        if trustee in names and "OI" in flags and "CI" in flags and "IO" not in flags:
            mask = int(rights, 16) if rights.startswith("0x") else (0x1F01FF if rights == "FA" else 0)
            if mask & 0x1200A9 == 0x1200A9:
                return True
    return False


def dacl(path: Path) -> str:
    """`path`'s DACL in SDDL (Windows)."""
    import ctypes
    from ctypes import wintypes

    advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    advapi32.GetNamedSecurityInfoW.argtypes = [
        wintypes.LPCWSTR, ctypes.c_int, wintypes.DWORD, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
        ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p),
    ]
    advapi32.GetNamedSecurityInfoW.restype = wintypes.DWORD
    advapi32.ConvertSecurityDescriptorToStringSecurityDescriptorW.argtypes = [
        ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(wintypes.LPWSTR), ctypes.c_void_p,
    ]
    kernel32.LocalFree.argtypes = [ctypes.c_void_p]
    se_file_object, dacl_security_information, sddl_revision_1 = 1, 4, 1
    descriptor = ctypes.c_void_p()
    error = advapi32.GetNamedSecurityInfoW(
        str(path), se_file_object, dacl_security_information, None, None, None, None, ctypes.byref(descriptor)
    )
    if error:
        raise ctypes.WinError(error)
    try:
        text = wintypes.LPWSTR()
        if not advapi32.ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor, sddl_revision_1, dacl_security_information, ctypes.byref(text), None
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            return text.value
        finally:
            kernel32.LocalFree(ctypes.cast(text, ctypes.c_void_p))
    finally:
        kernel32.LocalFree(descriptor)


def build_env(profile: str) -> dict:
    """The environment machopo is built with `profile` in: the `perf` profile
    keeps frame pointers on Linux and macOS, where Chrome and its V8 are
    built with them too (Chromium's `enable_frame_pointers`), so that the
    two run alike and a sampler unwinds machopo's stack through its JS."""
    env = dict(os.environ)
    if profile == "perf" and (sys.platform == "darwin" or sys.platform.startswith("linux")):
        env["RUSTFLAGS"] = f"{env.get('RUSTFLAGS', '')} -C force-frame-pointers=yes".strip()
    return env


def machopo_binary(profile: str = "dev", root: Path = ROOT, target: Path | None = None) -> Path:
    """machopo, built with `profile` from the tree at `root`, into `target`
    (by default the tree's `target/`)."""
    env = build_env(profile)
    if target is not None:
        env["CARGO_TARGET_DIR"] = str(target)
    done = subprocess.run(["cargo", "build", "-q", "--profile", profile, "-p", "shell", "--bin", "machopo",
                           *build_options(profile, target or root / "target")], cwd=root, env=env)
    if done.returncode != 0:
        raise SystemExit("building machopo failed")
    folder = "debug" if profile == "dev" else profile
    return (target or root / "target") / folder / ("machopo.exe" if sys.platform == "win32" else "machopo")


def build_options(profile: str, target: Path) -> list[str]:
    """What else `cargo build` takes for machopo with `profile` into `target`:
    on Windows, the `perf` profile's exe names its PDB by the whole path,
    by which samply finds it (rustc has the linker keep only the name,
    `/PDBALTPATH:%_PDB%`, and samply then names machopo's functions only by
    their addresses, from the exe's unwind table). Through `--config`,
    which adds to `.cargo/config.toml`'s flags: `RUSTFLAGS` would replace
    them."""
    if profile != "perf" or sys.platform != "win32":
        return []
    pdb = target / "perf" / "machopo.pdb"
    return ["--config", f"target.x86_64-pc-windows-msvc.rustflags=['-C', 'link-arg=/PDBALTPATH:{pdb}']"]


# Where Linux tells each CPU's maximum frequency.
SYSFS_CPUS = Path("/sys/devices/system/cpu")


def fastest_cpus(sysfs: Path = SYSFS_CPUS, allowed: list[int] | None = None) -> list[int] | None:
    """Of the CPUs this process may run on (`allowed`), those whose maximum
    frequency is the highest, on Linux: a hybrid CPU's big cores (AMD's
    Zen 5 beside Zen 5c, Intel's P-cores beside E-cores). `None` when that
    is all of them or the frequencies aren't known (Windows:
    `fastest_windows_cpus`; macOS places threads by their QoS, and can't be
    told to)."""
    if allowed is None:
        if not hasattr(os, "sched_getaffinity"):
            return None
        allowed = sorted(os.sched_getaffinity(0))
    frequencies = {}
    for cpu in allowed:
        try:
            frequencies[cpu] = int((sysfs / f"cpu{cpu}" / "cpufreq" / "cpuinfo_max_freq").read_text())
        except (OSError, ValueError):
            return None
    return fastest_of(frequencies, allowed)


def fastest_of(speeds: dict[int, int], allowed: list[int]) -> list[int] | None:
    """Of `allowed`, the CPUs whose speed is the highest; `None` when that
    is all of them, or one's speed isn't known."""
    if not allowed or any(cpu not in speeds for cpu in allowed):
        return None
    top = max(speeds[cpu] for cpu in allowed)
    fastest = [cpu for cpu in allowed if speeds[cpu] == top]
    return fastest if len(fastest) < len(allowed) else None


# An entry of `GetSystemCpuSetInformation` (SYSTEM_CPU_SET_INFORMATION):
# its size and type (0: a CPU set), then the set's id, group, logical
# processor's index in the group, core, last level cache, NUMA node and
# efficiency class.
CPU_SET = struct.Struct("<IIIHBBBBB")
CPU_SET_TYPE = 0


def efficiency_classes(info: bytes) -> dict[int, int] | None:
    """Each logical CPU's efficiency class in `GetSystemCpuSetInformation`'s
    `info`, by its index; `None` if the CPUs are in more than one group
    (over 64 of them), which one affinity mask can't hold."""
    classes = {}
    offset = 0
    while offset + 8 <= len(info):
        size, kind = struct.unpack_from("<II", info, offset)
        if size == 0:
            break
        if kind == CPU_SET_TYPE:
            _, _, _, group, index, _, _, _, efficiency = CPU_SET.unpack_from(info, offset)
            if group != 0:
                return None
            classes[index] = efficiency
        offset += size
    return classes


def windows_cpu_sets() -> bytes:
    """`GetSystemCpuSetInformation`'s entries for this machine's CPUs."""
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    needed = wintypes.ULONG()
    kernel32.GetSystemCpuSetInformation(None, 0, ctypes.byref(needed), None, 0)
    info = ctypes.create_string_buffer(needed.value)
    if not kernel32.GetSystemCpuSetInformation(info, needed.value, ctypes.byref(needed), None, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    return info.raw[:needed.value]


def fastest_windows_cpus(info: bytes | None = None, allowed: list[int] | None = None) -> list[int] | None:
    """Of the CPUs this process may run on (`allowed`), those of the highest
    efficiency class in `GetSystemCpuSetInformation`'s `info`, on Windows:
    a hybrid CPU's big cores (Intel's P-cores beside E-cores; a higher
    class is a faster core). `None` when that is all of them."""
    if allowed is None:
        mask = process_affinity()
        allowed = [cpu for cpu in range(mask.bit_length()) if mask >> cpu & 1]
    classes = efficiency_classes(windows_cpu_sets() if info is None else info)
    return fastest_of(classes, allowed) if classes is not None else None


def process_affinity(mask: int | None = None) -> int:
    """This process's affinity mask (Windows), and with `mask`, set to it
    (the mask is the one from before)."""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    kernel32.GetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t),
                                                ctypes.POINTER(ctypes.c_size_t)]
    kernel32.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    process = kernel32.GetCurrentProcess()
    current, system = ctypes.c_size_t(), ctypes.c_size_t()
    if not kernel32.GetProcessAffinityMask(process, ctypes.byref(current), ctypes.byref(system)):
        raise ctypes.WinError(ctypes.get_last_error())
    if mask is not None and not kernel32.SetProcessAffinityMask(process, mask):
        raise ctypes.WinError(ctypes.get_last_error())
    return current.value


def cpu_list(cpus: list[int]) -> str:
    """`cpus` as `taskset` takes them: `0-3,12-15`."""
    runs: list[list[int]] = []
    for cpu in sorted(cpus):
        if runs and cpu == runs[-1][1] + 1:
            runs[-1][1] = cpu
        else:
            runs.append([cpu, cpu])
    return ",".join(f"{a}-{b}" if a != b else str(a) for a, b in runs)


# The CPUs the browsers run on (`on_browser_cpus`), found once.
_browser_cpus: list[list[int] | None] = []


def browser_cpus() -> list[int] | None:
    """The fastest CPUs (`fastest_cpus` on Linux, if `taskset` can put the
    browsers on them; `fastest_windows_cpus` on Windows): on a hybrid CPU a
    browser lands on a big core or a small one for a whole page, which
    moves its numbers by half again (the small cores' clock), each browser
    apart, so their ratio too."""
    if not _browser_cpus:
        if sys.platform == "win32":
            _browser_cpus.append(fastest_windows_cpus())
        else:
            _browser_cpus.append(fastest_cpus() if shutil.which("taskset") else None)
    return _browser_cpus[0]


def on_browser_cpus(command: list[str]) -> list[str]:
    """`command` run on `browser_cpus` (by `taskset`, whose affinity the
    processes it starts inherit), if any; on Windows `command` itself,
    started in `browser_affinity`."""
    cpus = browser_cpus()
    return ["taskset", "-c", cpu_list(cpus), *command] if cpus and sys.platform != "win32" else command


@contextlib.contextmanager
def browser_affinity():
    """On Windows, this process on `browser_cpus` while a browser starts in
    it: a process takes its creator's affinity when it is created, and so
    do the ones it starts (Chrome's GPU process and renderers, which its
    broker creates; samply's machopo), and Windows has no `taskset` to start a
    command through. Set back after, so that the tool's own work (cargo's
    builds) runs on every CPU. Elsewhere it does nothing (`on_browser_cpus`)."""
    cpus = browser_cpus() if sys.platform == "win32" else None
    if not cpus:
        yield
        return
    before = process_affinity(sum(1 << cpu for cpu in cpus))
    try:
        yield
    finally:
        process_affinity(before)


class CdpPipe:
    """A browser's `process`, driven with CDP's messages through a pipe,
    each followed by a NUL: Chrome's `--remote-debugging-pipe`, and machopo's
    `--automation-pipe`, which takes them Chrome's way (as machopodriver does).
    Answers go to the `call` that waits for them; events wait for `event`."""

    name = "the browser"

    def __init__(self, process: subprocess.Popen, to_browser, from_browser) -> None:
        self.process = process
        self.to_browser = to_browser
        self.from_browser = from_browser
        self.next_id = 0
        self.answers: queue.Queue[dict | None] = queue.Queue()
        self.events: queue.Queue[dict | None] = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self) -> None:
        buffer = b""
        while True:
            try:
                chunk = self.from_browser.read1(65536)
            except (OSError, ValueError):
                chunk = b""
            if not chunk:
                self.answers.put(None)
                self.events.put(None)
                return
            buffer += chunk
            while b"\0" in buffer:
                message, buffer = buffer.split(b"\0", 1)
                try:
                    message = json.loads(message)
                except ValueError:
                    continue
                (self.answers if "id" in message else self.events).put(message)

    def call(self, method: str, params: dict | None = None, session: str | None = None, timeout: float = 30) -> dict:
        """`method`'s result; `RuntimeError` on its error, or if the browser
        ends or doesn't answer in `timeout` seconds."""
        self.next_id += 1
        command = {"id": self.next_id, "method": method, "params": params or {}}
        if session:
            command["sessionId"] = session
        try:
            self.to_browser.write(json.dumps(command).encode() + b"\0")
            self.to_browser.flush()
        except (OSError, ValueError):
            raise RuntimeError(f"{self.name} has ended")
        deadline = time.monotonic() + timeout
        while True:
            try:
                message = self.answers.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty:
                raise RuntimeError(f"{self.name} didn't answer {method}")
            if message is None:
                self.answers.put(None)
                raise RuntimeError(f"{self.name} has ended")
            if message.get("id") != self.next_id:
                continue
            if "error" in message:
                raise RuntimeError(f"{method}: {message['error'].get('message')}")
            return message.get("result", {})

    def event(self, method: str, timeout: float = 30) -> dict:
        """The params of the next `method` event (the events before it
        dropped); `RuntimeError` if the browser ends or sends none in
        `timeout` seconds."""
        deadline = time.monotonic() + timeout
        while True:
            try:
                message = self.events.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty:
                raise RuntimeError(f"{self.name} sent no {method}")
            if message is None:
                self.events.put(None)
                raise RuntimeError(f"{self.name} has ended")
            if message.get("method") == method:
                return message.get("params", {})

    def navigate(self, url: str, front: bool = False) -> str:
        """Its tab navigated to `url` (brought to the front first if
        `front`); the session attached to the tab."""
        deadline = time.monotonic() + 10
        while True:
            pages = [info for info in self.call("Target.getTargets")["targetInfos"] if info.get("type") == "page"]
            if pages or time.monotonic() > deadline:
                break
            time.sleep(0.05)
        if not pages:
            raise RuntimeError(f"{self.name} has no tab")
        session = self.call("Target.attachToTarget", {"targetId": pages[0]["targetId"], "flatten": True})["sessionId"]
        if front:
            self.call("Page.bringToFront", {}, session)
        self.call("Page.navigate", {"url": url}, session)
        return session

    def evaluate(self, expression: str, session: str):
        """`expression`'s value in the session's page (as JSON);
        `RuntimeError` if it throws."""
        result = self.call("Runtime.evaluate", {"expression": expression, "returnByValue": True}, session)
        if "exceptionDetails" in result:
            raise RuntimeError(f"{expression}: {result['exceptionDetails'].get('text')}")
        return result["result"].get("value")

    def close(self, grace: float = 10) -> None:
        """Asks the browser to close, and waits up to `grace` seconds for it
        to end on its own (what it writes as it ends, such as `MACHOPO_TRACE`'s
        end, gets written), then ends it."""
        try:
            self.call("Browser.close", timeout=5)
        except RuntimeError:
            pass
        try:
            self.process.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            pass
        end(self.process)
        for stream in (self.to_browser, self.from_browser):
            try:
                stream.close()
            except OSError:
                pass


class MachopoPipe(CdpPipe):
    """`machopo --automation-pipe`, through its standard input and output; run
    by `wrapper` (a sampler), if any, which passes the pipe through."""

    name = "machopo"

    def __init__(self, binary: Path, log, env: dict | None = None, wrapper: list[str] | None = None) -> None:
        with browser_affinity():
            process = subprocess.Popen(
                on_browser_cpus([*(wrapper or []), str(binary), "--automation-pipe"]),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=log,
                env={**os.environ, **(env or {})},
            )
        super().__init__(process, process.stdin, process.stdout)

    def open(self, url: str) -> None:
        """Its window at `WINDOW`, and its tab navigated to `url`."""
        self.set_window_size()
        self.navigate(url)

    def set_window_size(self) -> None:
        """Its window at `WINDOW`, as Chrome's (`--window-size`)."""
        self.call("Browser.setWindowBounds", {"windowId": 1, "bounds": {"width": WINDOW[0], "height": WINDOW[1]}})


class ChromePipe(CdpPipe):
    """Chrome, run with `arguments`, through its `--remote-debugging-pipe`:
    on Windows the pipe's two handles are named on the command line
    (`--remote-debugging-io-pipes`, which Chrome ignores without the other
    switch); elsewhere Chrome reads file descriptor 3 and writes 4, which
    `sh` moves the pipe's ends to (`subprocess` keeps a descriptor's
    number)."""

    name = "Chrome"

    def __init__(self, arguments: list[str], binary: Path | None = None, wrapper: list[str] | None = None) -> None:
        """`binary`: another Chrome than Chrome for Testing (a Chromium);
        `wrapper`: what runs it (a sampler, ending in `--`)."""
        to_chrome, to_browser = os.pipe()
        from_browser, from_chrome = os.pipe()
        command = [*(wrapper or []), str(binary or chrome_binary()), "--remote-debugging-pipe", *arguments]
        if sys.platform == "win32":
            import msvcrt

            handles = [msvcrt.get_osfhandle(to_chrome), msvcrt.get_osfhandle(from_chrome)]
            for handle in handles:
                os.set_handle_inheritable(handle, True)
            command.insert(2, f"--remote-debugging-io-pipes={handles[0]},{handles[1]}")
            startup = subprocess.STARTUPINFO()
            # Chrome inherits the pipe's two handles and the standard ones
            # (which `subprocess` adds), no others.
            startup.lpAttributeList = {"handle_list": handles}
            options = {"startupinfo": startup, "close_fds": True}
        else:
            command = on_browser_cpus(["sh", "-c", f'exec "$0" "$@" 3<&{to_chrome} 4>&{from_chrome}', *command])
            options = {"pass_fds": (to_chrome, from_chrome)}
        try:
            with browser_affinity():
                process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **options)
        finally:
            os.close(to_chrome)
            os.close(from_chrome)
        super().__init__(process, os.fdopen(to_browser, "wb"), os.fdopen(from_browser, "rb"))

    def open(self, url: str) -> None:
        """Its tab brought to the front, navigated to `url`, and left: the
        page has focus and no session stays attached to it while it runs,
        as when Chrome opens the URL itself. Headless Chrome's first tab
        isn't the front one until asked: unfocused, an editor focused in a
        test doesn't show it (TipTap's `ProseMirror-focused`)."""
        session = self.navigate(url, front=True)
        self.call("Target.detachFromTarget", {"sessionId": session})


def end(process: subprocess.Popen, grace: float = 10) -> None:
    """`process`, ended: asked to, then killed after `grace` seconds (on
    Windows, `terminate()` kills it at once)."""
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=grace)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".json": "application/json",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".webp": "image/webp",
    ".ico": "image/x-icon",
    ".woff": "font/woff",
    ".woff2": "font/woff2",
    ".ttf": "font/ttf",
    ".wasm": "application/wasm",
    ".txt": "text/plain; charset=utf-8",
}


def content_type(file: Path) -> str:
    return TYPES.get(file.suffix.lower(), "application/octet-stream")


# --- comparing two versions of machopo ------------------------------------------

# Where `--against` checks the other version out: next to the repo, so its
# `../third_party` is the same checkouts.
AGAINST = ROOT.parent / "machopo-against"

# The files that pin `../third_party`, which both versions build from.
PIN_FILES = ("third_party.toml", ".cargo/config.toml")


def git_show(rev: str, path: str) -> str | None:
    """`path` as `rev` has it, or `None` if it can't be read."""
    done = subprocess.run(["git", "show", f"{rev}:{path}"], cwd=ROOT, capture_output=True, text=True)
    return done.stdout if done.returncode == 0 else None


def against_problems(rev: str) -> list[str]:
    """Why machopo at `rev` can't be built next to this one, if it can't: both
    build from the same `../third_party`, so `rev` must pin it alike."""
    problems = []
    for path in PIN_FILES:
        theirs = git_show(rev, path)
        if theirs is None:
            problems.append(f"{rev}: no {path} (is it a commit?)")
        elif theirs != (ROOT / path).read_text(encoding="utf-8"):
            problems.append(f"{rev} pins ../third_party otherwise ({path} differs): both versions build from the "
                            "same checkouts")
    return problems


def against_tree(rev: str) -> Path:
    """A worktree of the repo at `rev` (`AGAINST`, made the first time and
    moved after). `rev` is the repo's, resolved here first: in the
    worktree, `HEAD` (or a branch it has moved) would be the worktree's own,
    and it would stay where it was."""
    resolved = subprocess.run(["git", "rev-parse", "--verify", "-q", f"{rev}^{{commit}}"], cwd=ROOT,
                              capture_output=True, text=True)
    if resolved.returncode != 0:
        raise SystemExit(f"no commit {rev}")
    rev = resolved.stdout.strip()
    if (AGAINST / ".git").exists():
        done = subprocess.run(["git", "checkout", "-q", "--detach", rev], cwd=AGAINST)
    else:
        done = subprocess.run(["git", "worktree", "add", "-q", "--detach", str(AGAINST), rev], cwd=ROOT)
    if done.returncode != 0:
        raise SystemExit(f"checking {rev} out at {AGAINST} failed")
    return AGAINST


def change(before: float, after: float) -> str:
    return f"{100 * (after - before) / before:+.1f}%" if before else "–"


def versions_text(rev: str) -> str:
    """What `compare.md` compares: machopo at `rev` and the working tree's."""
    resolved = subprocess.run(["git", "rev-parse", "--short=12", rev], cwd=ROOT, capture_output=True, text=True)
    dirty = subprocess.run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, capture_output=True,
                           text=True).stdout.strip()
    return (f"`against` is {rev} ({resolved.stdout.strip()}), `current` the working tree "
            f"({git_head()}{', with changes' if dirty else ''})")


def interval_text(values: list[float]) -> str:
    interval = median_interval(values)
    around = f" ({interval[0]:.1f}–{interval[1]:.1f})" if interval else ""
    return f"{median(values):.1f}{around}"


def against_binaries(rev: str) -> dict[str, Path]:
    """machopo at `rev` (built in a worktree, `against_tree`, into its own
    target directory) and the working tree's."""
    return {
        "against": machopo_binary(MACHOPO_PROFILE, against_tree(rev), ROOT / "target" / "against"),
        "current": machopo_binary(MACHOPO_PROFILE),
    }


# --- what is valid ----------------------------------------------------------

# A run of hex digits in a class name: what a page makes up anew each time
# (Observable Plot's `plot-7017677fbe27d8`), the same class to compare.
RANDOM_IN_CLASS = re.compile(r"[0-9a-f]{10,}")


def same_classes(classes: dict, ignored: set[str] = frozenset()) -> dict:
    """`classes`' counts, the made-up part of a name (`RANDOM_IN_CLASS`) as
    `*`, without the classes `ignored`."""
    merged: dict[str, int] = {}
    for name, count in classes.items():
        if name in ignored:
            continue
        name = RANDOM_IN_CLASS.sub("*", name)
        merged[name] = merged.get(name, 0) + count
    return merged


def summary_difference(got: dict, want: dict, ignored: set[str] = frozenset(), wanted_by: str = "Chrome") -> str | None:
    """How the summary `got` (`summary.mjs`'s, machopo's) differs from `want`
    (`wanted_by`'s), the first difference, not counting the classes
    `ignored`; or `None`."""
    got = dict(got, classes=same_classes(got["classes"], ignored))
    want = dict(want, classes=same_classes(want["classes"], ignored))
    for kind, noun in (("elements", "elements"), ("classes", "with the class")):
        for name in sorted(set(got[kind]) | set(want[kind])):
            have, should = got[kind].get(name, 0), want[kind].get(name, 0)
            if have != should:
                what = f"`{name}` {noun}" if kind == "elements" else f"elements {noun} `{name}`"
                return f"{have} {what}, {wanted_by} {should}"
    if got["text"] != want["text"]:
        return f"text {got['text']} characters long, {wanted_by} {want['text']}"
    return None


# --- the server -----------------------------------------------------------

class Server:
    """The files under `root` over HTTP on a port of its own, and what
    pages post (to any path). A benchmark's server derives from it to add
    to what it serves (`get`)."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.posted: queue.Queue[bytes] = queue.Queue()
        server = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args) -> None:
                pass

            def do_POST(self) -> None:
                length = int(self.headers.get("Content-Length", 0))
                server.posted.put(self.rfile.read(length))
                self.send_response(204)
                self.end_headers()

            def do_GET(self) -> None:
                path = self.path.split("?", 1)[0]
                body, content_type = server.get(path)
                if body is None:
                    self.send_response(404)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(body)

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def url(self, path: str) -> str:
        return f"http://127.0.0.1:{self.port}{path}"

    def close(self) -> None:
        self.httpd.shutdown()

    def get(self, path: str) -> tuple[bytes | None, str]:
        """The body and type of `path` (`None` if there is none): the file
        under `root`, `index.html` for a folder; `SUMMARY`."""
        if path == SUMMARY:
            return SUMMARY_FILE.read_bytes(), "text/javascript; charset=utf-8"
        file = self.file(path)
        if file is None:
            return None, ""
        return file.read_bytes(), content_type(file)

    def file(self, path: str) -> Path | None:
        """The file `path` names under `root`, if there is one (nothing
        outside it)."""
        if path.endswith("/"):
            path += "index.html"
        file = (self.root / path.lstrip("/")).resolve()
        if self.root.resolve() not in file.parents or not file.is_file():
            return None
        return file

    def take(self, timeout: float, process: subprocess.Popen) -> bytes | None:
        """What a page posts within `timeout` seconds; `None` if it doesn't,
        or `process` (its browser) ends first."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                return self.posted.get(timeout=0.5)
            except queue.Empty:
                if process.poll() is not None:
                    return None
        return None

    def forget(self) -> None:
        """Drops what earlier pages posted late."""
        while not self.posted.empty():
            self.posted.get_nowait()



# The trace category of `performance.mark()` and `measure()`, Blink's and
# machopo's: the runner's marks find the windows it times.
USER_TIMING = "blink.user_timing"


# What Chrome records with `--trace`: its top-level tasks, Blink, V8, cc and
# Viz, DevTools' performance panel's events (`FunctionCall`,
# `UpdateLayoutTree`, `Layout`, `Paint`), Oilpan's phases (`CppGC.*`, as
# machopo's DOM heap's), V8's heap's sizes and limits (`v8.memory`'s
# counters), and User Timing.
CHROME_TRACE_CATEGORIES = ("toplevel", "blink", "v8", "cc", "viz", "benchmark", "loading", "devtools.timeline",
                           "cppgc", "v8.memory", USER_TIMING)


# What a trace for the GC alone records: V8's and cppgc's events, V8's GC
# phases (as machopo's trace has them, to compare a pause's), V8's heap's
# sizes and limits (`v8.memory`: why it starts a full GC when it does), and
# the runner's User Timing. The others slow some pages down (blink_perf's
# innerHTML-setter to a third of its rate).
CHROME_GC_TRACE_CATEGORIES = ("v8", "cppgc", "disabled-by-default-v8.gc", "v8.memory", USER_TIMING)


# DevTools' `Profiler` samples every `PROFILE_INTERVAL` µs for `--profile`:
# often enough for the functions that matter, rarely enough that sampling
# slows each browser down about as much (about a tenth).
PROFILE_INTERVAL = 1000


# The function the page runs once the profile has started, for
# `PAGE_CLOCK_MS` ms: its samples put the page's clock on the profile's.
PAGE_CLOCK = "__machopoPageClock"
PAGE_CLOCK_MS = 10


def open_profiled(pipe: "CdpPipe", url: str, front: bool) -> tuple[str, dict]:
    """The pipe's tab navigated to `url`, a page that holds its start until
    its `__machopoStart()` (Speedometer's runner, with `measure.mjs` holding
    it); then DevTools' `Profiler` sampling its script every
    `PROFILE_INTERVAL` µs, once the page is ready (Chrome's renderer for
    the page may be another than `about:blank`'s, and the clock is the
    page's); then the page runs `PAGE_CLOCK` for `PAGE_CLOCK_MS` ms, and
    starts: the profile has the page's code from before it is compiled
    (V8 doesn't learn of code optimized on the stack before a profile
    starts). The session, kept attached, and when on the page's clock
    (`performance.now()`) `PAGE_CLOCK` ran (`pageClock`): a profile's
    start, nor DevTools' answer, says where on the page's clock it
    started."""
    session = pipe.navigate(url, front=front)
    wait_for(pipe, session, 'typeof globalThis.__machopoStart === "function"', url)
    pipe.call("Profiler.enable", {}, session)
    pipe.call("Profiler.setSamplingInterval", {"interval": PROFILE_INTERVAL}, session)
    pipe.call("Profiler.start", {}, session)
    start, end = pipe.evaluate(
        f"(function {PAGE_CLOCK}() {{ const start = performance.now(); let now = start; "
        f"while (now - start < {PAGE_CLOCK_MS}) now = performance.now(); return [start, now]; }})()", session)
    pipe.evaluate("void __machopoStart()", session)
    return session, {"start": start, "end": end}


def wait_for(pipe: "CdpPipe", session: str, expression: str, url: str, timeout: float = 10) -> None:
    """Until `expression` is true in the tab's page (`url`'s): `Page.navigate`
    may answer before the navigation commits."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            if pipe.evaluate(expression, session) is True:
                return
        except RuntimeError:
            # Between the two documents.
            pass
        if time.monotonic() > deadline:
            raise RuntimeError(f"{pipe.name} didn't get {url} ready")
        time.sleep(0.002)


def finish_profile(pipe: "CdpPipe", session: str, path: Path, page_clock: dict) -> None:
    """The profile `open_profiled` started, stopped and written to `path`
    (DevTools' `.cpuprofile`), with when `PAGE_CLOCK` ran on the page's
    clock (`page_clock`, as `pageClock`, which DevTools ignores)."""
    profile = pipe.call("Profiler.stop", {}, session, timeout=120)["profile"]
    profile["pageClock"] = page_clock
    path.write_text(json.dumps(profile), encoding="utf-8")


def run_in_chrome(server: Server, url: str, timeout: float, trace: Path | None = None,
                  cpu_profile: Path | None = None,
                  categories: tuple[str, ...] = CHROME_TRACE_CATEGORIES, binary: Path | None = None,
                  wrapper: list[str] | None = None, arguments: tuple[str, ...] = ()) -> dict:
    """`url` in a new headless Chrome: what its page posted (`{"body"}`), or
    why there is nothing (`{"error"}`). With `trace`, Chrome records a trace
    of `categories` from before the page loads until it has posted (or the
    run gave up), written there (`save_trace`). With `cpu_profile`, DevTools' `Profiler`
    samples its script from the navigation until it has posted, written
    there (`open_profiled`). `binary`, `wrapper`: as `ChromePipe`'s;
    `arguments`, more of Chrome's."""
    profile = tempfile.mkdtemp(prefix="machopo-speedometer-chrome-")
    server.forget()
    chrome = ChromePipe([
        "--headless=new", f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
        "--disable-extensions", f"--window-size={WINDOW[0]},{WINDOW[1]}", *arguments, "about:blank",
    ], binary, wrapper)
    try:
        try:
            if trace:
                chrome.call("Tracing.start", {
                    "traceConfig": {"includedCategories": list(categories)},
                    "transferMode": "ReturnAsStream",
                    "streamFormat": "proto",
                })
            if cpu_profile:
                session, page_clock = open_profiled(chrome, url, front=True)
            else:
                chrome.open(url)
        except RuntimeError as e:
            return {"error": str(e)}
        body = server.take(timeout, chrome.process)
        if cpu_profile and body is not None:
            try:
                finish_profile(chrome, session, cpu_profile, page_clock)
            except RuntimeError as e:
                return {"error": f"the profile: {e}"}
        if trace:
            try:
                if save_trace(chrome, trace):
                    print(f"  Chrome's trace lost some events: {trace}")
            except RuntimeError as e:
                print(f"  Chrome's trace: {e}")
        if body is not None:
            return {"body": body}
        return {"error": f"Chrome ended (exit status {chrome.process.returncode})"
                if chrome.process.poll() is not None else f"no report in {timeout:.0f} s"}
    finally:
        chrome.close()
        shutil.rmtree(profile, ignore_errors=True)


def save_trace(chrome: CdpPipe, path: Path) -> bool:
    """Chrome's tracing ended and its trace (Perfetto's protobuf, as asked
    for) read back into `path`; whether Chrome's buffer ran out and lost
    some events."""
    chrome.call("Tracing.end")
    done = chrome.event("Tracing.tracingComplete", timeout=60)
    partial = path.with_name(path.name + ".part")
    with open(partial, "wb") as out:
        while True:
            chunk = chrome.call("IO.read", {"handle": done["stream"], "size": 1 << 20})
            data = chunk.get("data", "")
            out.write(base64.b64decode(data) if chunk.get("base64Encoded") else data.encode())
            if chunk.get("eof"):
                break
    chrome.call("IO.close", {"handle": done["stream"]})
    partial.replace(path)
    return bool(done.get("dataLossOccurred"))


def run_in_machopo(server: Server, binary: Path, url: str, timeout: float, log, trace: Path | None = None,
               samples: Path | None = None, cpu_profile: Path | None = None, env: dict | None = None) -> dict:
    """`url` in a new `machopo --automation-pipe`: what its page posted
    (`{"body"}`, and machopo's `peak_memory` by then as `memory` when nothing
    wraps it), or why there is nothing (`{"error"}`). With `trace`, machopo
    writes a trace there (`MACHOPO_TRACE`); with `samples`, `perf` samples it
    into that file (`sampler`); with `cpu_profile`, DevTools' `Profiler` as in
    Chrome (`run_in_chrome`); `env`, more of machopo's environment."""
    server.forget()
    env = {**(env or {}), **({"MACHOPO_TRACE": str(trace)} if trace else {})}
    wrapper = None
    if samples:
        wrapper, flags = sampler(samples)
        env["MACHOPO_V8_FLAGS"] = f"{os.environ.get('MACHOPO_V8_FLAGS', '')} {flags}".strip()
    machopo = MachopoPipe(binary, log, env or None, wrapper)
    try:
        try:
            if cpu_profile:
                machopo.set_window_size()
                session, page_clock = open_profiled(machopo, url, front=False)
            else:
                machopo.open(url)
        except RuntimeError as e:
            return {"error": str(e)}
        body = server.take(timeout, machopo.process)
        if cpu_profile and body is not None:
            try:
                finish_profile(machopo, session, cpu_profile, page_clock)
            except RuntimeError as e:
                return {"error": f"the profile: {e}"}
        if body is not None:
            # A wrapper's process is the sampler's, not machopo's.
            return {"body": body, "memory": None if wrapper else peak_memory(machopo.process.pid)}
        return {"error": f"machopo ended (exit status {machopo.process.returncode})" if machopo.process.poll() is not None
                else f"no report in {timeout:.0f} s"}
    finally:
        machopo.close()


def peak_memory(pid: int) -> int | None:
    """The most memory the process `pid` has had (bytes), as each platform
    counts the memory that is the process's own: macOS's phys footprint
    (Activity Monitor's Memory, and Chromium's private memory footprint
    there), Linux's peak resident set (`VmHWM`), Windows's peak private
    bytes (`PeakPagefileUsage`). `None` if it can't be read."""
    try:
        if sys.platform == "darwin":
            return macos_peak_footprint(pid)
        if sys.platform.startswith("linux"):
            return linux_peak_rss(Path(f"/proc/{pid}/status").read_text(encoding="utf-8"))
        if sys.platform == "win32":
            return windows_peak_private_bytes(pid)
    except OSError:
        return None
    return None


# `proc_pid_rusage`'s `rusage_info_v4` (<sys/resource.h>): after its uuid,
# 64-bit fields, `ri_lifetime_max_phys_footprint` the 29th.
RUSAGE_INFO_V4 = 4


LIFETIME_MAX_PHYS_FOOTPRINT = 28


def macos_peak_footprint(pid: int) -> int | None:
    """`pid`'s lifetime maximum phys footprint, from libproc."""
    class RusageInfoV4(ctypes.Structure):
        _fields_ = [("uuid", ctypes.c_uint8 * 16), ("fields", ctypes.c_uint64 * 40)]

    info = RusageInfoV4()
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
    if libproc.proc_pid_rusage(pid, RUSAGE_INFO_V4, ctypes.byref(info)) != 0:
        return None
    return info.fields[LIFETIME_MAX_PHYS_FOOTPRINT]


def linux_peak_rss(status: str) -> int | None:
    """The peak resident set in a `/proc/<pid>/status`'s text."""
    found = re.search(r"^VmHWM:\s+(\d+) kB$", status, re.M)
    return int(found[1]) * 1024 if found else None


def is_admin() -> bool:
    """Whether this process runs as Administrator (Windows)."""
    return bool(ctypes.WinDLL("shell32").IsUserAnAdmin())


def xperf() -> Path | None:
    """The Windows Performance Toolkit's xperf, which samply records ETW
    with, where samply looks for it: on `PATH`, else where the Windows
    Kits install it."""
    found = shutil.which("xperf")
    if found:
        return Path(found)
    kits = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    path = kits / "Windows Kits" / "10" / "Windows Performance Toolkit" / "xperf.exe"
    return path if path.exists() else None


# UAC's answer when the user declines (ERROR_CANCELLED).
UAC_DECLINED = 1223


def run_elevated(arguments: list[str], log: Path) -> int:
    """This Python running `arguments` as Administrator (Windows), through
    `ShellExecuteEx`'s `runas`: UAC asks the user, once. It starts with a
    new environment and a console of its own (hidden), so it is to write
    what it prints into `log`, which is printed here as it comes. Its exit
    status; 1 if it couldn't start (UAC declined)."""
    import codecs
    from ctypes import wintypes

    class ShellExecuteInfo(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.DWORD), ("fMask", wintypes.ULONG), ("hwnd", wintypes.HWND),
                    ("lpVerb", wintypes.LPCWSTR), ("lpFile", wintypes.LPCWSTR), ("lpParameters", wintypes.LPCWSTR),
                    ("lpDirectory", wintypes.LPCWSTR), ("nShow", ctypes.c_int), ("hInstApp", wintypes.HINSTANCE),
                    ("lpIDList", ctypes.c_void_p), ("lpClass", wintypes.LPCWSTR), ("hkeyClass", wintypes.HKEY),
                    ("dwHotKey", wintypes.DWORD), ("hIconOrMonitor", wintypes.HANDLE), ("hProcess", wintypes.HANDLE)]

    shell32 = ctypes.WinDLL("shell32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    shell32.ShellExecuteExW.argtypes = [ctypes.POINTER(ShellExecuteInfo)]
    kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_bytes(b"")
    info = ShellExecuteInfo(cbSize=ctypes.sizeof(ShellExecuteInfo), fMask=0x40,  # SEE_MASK_NOCLOSEPROCESS
                            lpVerb="runas", lpFile=sys.executable, lpParameters=subprocess.list2cmdline(arguments),
                            lpDirectory=str(ROOT), nShow=0)  # SW_HIDE
    if not shell32.ShellExecuteExW(ctypes.byref(info)):
        error = ctypes.get_last_error()
        print("UAC was declined" if error == UAC_DECLINED else f"running as Administrator failed: "
              f"{ctypes.WinError(error)}")
        return 1
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    try:
        with open(log, "rb") as out:
            while True:
                done = kernel32.WaitForSingleObject(info.hProcess, 200) == 0  # WAIT_OBJECT_0
                text = decoder.decode(out.read(), final=done)
                if text:
                    print(text, end="", flush=True)
                if done:
                    break
        status = wintypes.DWORD()
        kernel32.GetExitCodeProcess(info.hProcess, ctypes.byref(status))
        return status.value
    finally:
        kernel32.CloseHandle(info.hProcess)


def windows_peak_private_bytes(pid: int) -> int | None:
    """`pid`'s peak private bytes, from `GetProcessMemoryInfo`."""
    from ctypes import wintypes

    class ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                    *((name, ctypes.c_size_t) for name in (
                        "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
                        "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage",
                        "PeakPagefileUsage"))]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    process = kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not process:
        return None
    try:
        counters = ProcessMemoryCounters()
        counters.cb = ctypes.sizeof(counters)
        if not kernel32.K32GetProcessMemoryInfo(process, ctypes.byref(counters), counters.cb):
            return None
        return counters.PeakPagefileUsage
    finally:
        kernel32.CloseHandle(process)


def parse_run(result: dict) -> dict:
    """A run's result, as posted, or `{"error"}`."""
    if "error" in result:
        return {"error": result["error"]}
    try:
        return json.loads(result["body"])
    except ValueError as e:
        return {"error": f"what the page posted isn't JSON: {e}"}


def git_head() -> str:
    return subprocess.run(
        ["git", "rev-parse", "--short=12", "HEAD"], cwd=ROOT, capture_output=True, text=True
    ).stdout.strip()


def first_line(message: str) -> str:
    return message.strip().splitlines()[0][:200] if message.strip() else ""


def median(values: list[float]) -> float:
    ordered = sorted(values)
    middle = len(ordered) // 2
    return ordered[middle] if len(ordered) % 2 else (ordered[middle - 1] + ordered[middle]) / 2


def median_interval(values: list[float], confidence: float = 0.95) -> tuple[float, float] | None:
    """A `confidence` interval of the median that assumes no distribution:
    the order statistics x(l) and x(n + 1 - l), with l the largest for which
    fewer than l of n values fall below the median with probability (a
    binomial's, n and 1/2) at most half of 1 - `confidence`. `None` when
    there are too few values for one."""
    ordered = sorted(values)
    n = len(ordered)
    tail = (1 - confidence) / 2
    below, low = 0.0, 0
    for k in range(n + 1):
        below += math.comb(n, k) / 2**n
        if below > tail:
            break
        low = k + 1
    if low == 0:
        return None
    return ordered[low - 1], ordered[n - low]


def machopo_gpu(binary: Path, timeout: float = 10) -> dict | None:
    """The GPU adapter `binary` draws with, as its `SystemInfo.getInfo`
    tells (Chrome's method): `{"device", "type", "backend"}`, `type` being
    wgpu's (`Cpu` when it draws on the CPU, with no GPU or with
    `MACHOPO_GPU_ADAPTER=cpu`). `None` if it can't tell, or doesn't start."""
    try:
        machopo = MachopoPipe(binary, subprocess.DEVNULL)
    except OSError:
        return None
    try:
        # The adapter is known once Viz has made its renderer, just after
        # machopo starts.
        deadline = time.monotonic() + timeout
        while True:
            gpu = machopo.call("SystemInfo.getInfo").get("gpu", {})
            if gpu.get("devices"):
                aux = gpu.get("auxAttributes", {})
                return {
                    "device": gpu["devices"][0].get("deviceString", ""),
                    "type": aux.get("adapterType", ""),
                    "backend": aux.get("backend", ""),
                }
            if time.monotonic() > deadline:
                return None
            time.sleep(0.05)
    except RuntimeError:
        return None
    finally:
        machopo.close()


def describe_gpu(gpu: dict | None) -> str:
    return f"{gpu['device']} ({gpu['type']}, {gpu['backend']})" if gpu else "unknown"


def gpu_problem(gpu: dict | None) -> str | None:
    """Why machopo's times may not compare with Chrome's for its `gpu`: Chrome
    composites on the GPU."""
    if gpu is None:
        return "machopo didn't tell which GPU it draws with"
    if gpu["type"] == "Cpu":
        return f"machopo draws on the CPU adapter ({gpu['device']}), not a GPU: its times don't compare with Chrome's"
    return None


def environment(profile: str) -> dict:
    """What the numbers were measured on."""
    cpu, power = platform.processor() or platform.machine(), "unknown"
    system = platform.platform()
    if sys.platform == "darwin":
        version = subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip()
        system = f"macOS {version} ({platform.machine()})"
        cpu = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True
        ).stdout.strip() or cpu
        battery = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True).stdout
        power = "AC" if "AC Power" in battery else "battery" if "Battery Power" in battery else power
    elif sys.platform.startswith("linux"):
        cpuinfo = Path("/proc/cpuinfo")
        if cpuinfo.exists():
            model = re.search(r"^model name\s*:\s*(.+)$", cpuinfo.read_text(), re.M)
            cpu = model.group(1) if model else cpu
        mains = [p for p in Path("/sys/class/power_supply").glob("*") if (p / "type").exists()
                 and (p / "type").read_text().strip() == "Mains"]
        if mains:
            power = "AC" if any((p / "online").read_text().strip() == "1" for p in mains) else "battery"
    return {
        "date": date.today().isoformat(),
        "machopo": git_head(),
        "profile": profile,
        "chrome": CHROME_VERSION,
        "os": system,
        "cpu": cpu,
        "cores": os.cpu_count(),
        # The CPUs the browsers ran on: `all`, or the fastest.
        "cpus": cpu_list(browser_cpus()) if browser_cpus() else "all",
        "power": power,
        "gpu": "unknown",
    }


# The renderer's main thread, as both browsers name it.
MAIN_THREAD = "CrRendererMain"


def protobuf_fields(data: bytes):
    """A protobuf message's fields in order, `(number, value)`: an int for a
    varint or a fixed-size field, bytes for a length-delimited one."""
    i = 0
    while i < len(data):
        key, i = varint(data, i)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, i = varint(data, i)
        elif wire == 1:
            value, i = int.from_bytes(data[i:i + 8], "little"), i + 8
        elif wire == 2:
            length, i = varint(data, i)
            value, i = data[i:i + length], i + length
        elif wire == 5:
            value, i = int.from_bytes(data[i:i + 4], "little"), i + 4
        else:
            raise ValueError(f"wire type {wire}")
        yield number, value


def varint(data: bytes, i: int) -> tuple[int, int]:
    value = shift = 0
    while True:
        byte = data[i]
        i += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            return value, i


# The clock Perfetto's SDK writes each sequence's times on, as deltas from
# a snapshot of CLOCK_BOOTTIME (the trace's clock).
INCREMENTAL_CLOCK = 64


def perfetto_track_packets(path: Path, threads: dict[int, str] | None = None, lost: list[str] | None = None,
                           counter_tracks: dict[int, str] | None = None):
    """Each track event of a Perfetto trace, as Perfetto's SDK writes them,
    `(track, time, sequence, entries, name, categories, interned)`: its
    time in ns (CLOCK_BOOTTIME, or what the event says; an incremental
    clock's in its unit, which is µs in Chrome's), the packet's sequence,
    the `TrackEvent`'s fields in order, its name (an end's empty), its
    categories, and its sequence's interned debug annotation names by
    iid. Fills `threads` with the threads' names by their tracks,
    `counter_tracks` with the counters' names by theirs (`static_name`
    or `name`), and `lost` with the threads (or `sequence N`) whose writer
    dropped packets, once the trace is read: a packet after the first of
    its sequence says the one before it was dropped (the first says so of
    every sequence)."""
    sequences: dict[int, dict] = {}
    threads = {} if threads is None else threads
    for number, packet in protobuf_fields(path.read_bytes()):
        if number != 1:
            continue
        fields: dict[int, object] = {}
        for key, value in protobuf_fields(packet):
            fields.setdefault(key, value)
        first = fields.get(10) not in sequences
        state = sequences.setdefault(
            fields.get(10), {"names": {}, "categories": {}, "annotations": {}, "track": None, "clock": None,
                             "time": 0, "unit": 1, "dropped": False})
        if fields.get(42) and not first:  # previous_packet_dropped
            state["dropped"] = True
        if fields.get(13, 0) & 1:  # SEQ_INCREMENTAL_STATE_CLEARED
            state["names"] = {}
            state["categories"] = {}
            state["annotations"] = {}
        if 59 in fields:  # trace_packet_defaults
            defaults = dict(protobuf_fields(fields[59]))
            state["clock"] = defaults.get(58)
            if 11 in defaults:
                state["track"] = dict(protobuf_fields(defaults[11])).get(11)
        if 6 in fields:  # clock_snapshot
            for key, clock in protobuf_fields(fields[6]):
                clock = dict(protobuf_fields(clock)) if key == 1 else {}
                if clock.get(1) == INCREMENTAL_CLOCK:
                    # In its own unit: Chrome's counts µs (unit_multiplier_ns).
                    state["unit"] = clock.get(4, 1)
                    state["time"] = clock.get(2, 0) * state["unit"]
        if 12 in fields:  # interned_data
            for key, entry in protobuf_fields(fields[12]):
                if key in (1, 2, 3):  # event_categories, event_names, debug_annotation_names
                    entry = dict(protobuf_fields(entry))
                    table = {1: "categories", 2: "names", 3: "annotations"}[key]
                    state[table][entry.get(1)] = entry.get(2, b"").decode()
        if 60 in fields:  # track_descriptor
            track = dict(protobuf_fields(fields[60]))
            if 4 in track:  # a thread's
                threads[track.get(1)] = dict(protobuf_fields(track[4])).get(5, b"").decode()
            if 8 in track and counter_tracks is not None:  # a counter's
                name = track.get(10, track.get(2, b""))  # static_name, name
                counter_tracks[track.get(1)] = name.decode() if isinstance(name, bytes) else ""
        if 11 in fields:  # track_event
            entries = list(protobuf_fields(fields[11]))
            event = dict(entries)
            time = fields.get(8, 0)
            if fields.get(58, state["clock"]) == INCREMENTAL_CLOCK:
                state["time"] += time * state["unit"]
                time = state["time"]
            name = event[23].decode() if 23 in event else state["names"].get(event.get(10), "")
            categories = [state["categories"].get(value) if key == 3 else value.decode()
                          for key, value in entries if key in (3, 22)]  # category_iids, categories
            yield (event.get(11, state["track"]), time, fields.get(10), entries, name, categories,
                   state["annotations"])
    if lost is not None:
        lost += [threads.get(state["track"], f"sequence {sequence}")
                 for sequence, state in sequences.items() if state["dropped"]]


def perfetto_track_events(path: Path, lost: list[str] | None = None) -> tuple[dict[int, str], list[tuple]]:
    """The track events of a Perfetto trace (`perfetto_track_packets`'):
    the threads' names by their tracks, and each event as `(track, time,
    order, kind, name, categories)`, its kind `TrackEvent.type` (1: begin,
    2: end, 3: instant, 4: counter). With `lost`, the threads whose writer
    dropped packets, added to it."""
    threads: dict[int, str] = {}
    events = []
    for track, time, _, entries, name, categories, _ in perfetto_track_packets(path, threads, lost):
        events.append((track, time, len(events), dict(entries).get(9), name, categories))
    return threads, events


def counter_value(event: dict) -> float:
    """A counter event's value (`TrackEvent`'s fields): `counter_value`,
    or `double_counter_value` (a fixed64)."""
    if 30 in event:
        return float(event[30])
    return struct.unpack("<d", event.get(44, 0).to_bytes(8, "little"))[0]


def debug_annotation(data: bytes, interned: dict[int, str]) -> tuple[str, object]:
    """A `DebugAnnotation`'s name (`name`, or interned as `name_iid`) and
    value: a dict of its `dict_entries`, a list of its `array_values`, or
    the scalar it holds (a string decoded, a double as a float)."""
    fields = list(protobuf_fields(data))
    by_number = dict(fields)
    name = by_number[10].decode() if 10 in by_number else interned.get(by_number.get(1), "")
    if 11 in by_number:
        return name, dict(debug_annotation(value, interned) for number, value in fields if number == 11)
    if 12 in by_number:
        return name, [debug_annotation(value, interned)[1] for number, value in fields if number == 12]
    if 6 in by_number:  # string_value
        return name, by_number[6].decode(errors="replace")
    if 5 in by_number:  # double_value
        return name, struct.unpack("<d", by_number[5].to_bytes(8, "little"))[0]
    for number in (2, 3, 4, 7):  # bool, uint, int, pointer
        if number in by_number:
            return name, by_number[number]
    return name, None


def perfetto_slices(path: Path) -> dict[str, list[tuple[int, str | None]]]:
    """The slices of a Perfetto trace of track events, by thread name:
    each thread's begins (the slice's name) and ends (`None`) in time
    order, from every sequence (V8's and machopo's events on one thread are
    two), each with its time (ns on CLOCK_BOOTTIME). A slice on a track of
    its own (a User Timing measure) is under its track's uuid."""
    threads, events = perfetto_track_events(path)
    slices: dict[str, list[tuple[int, str | None]]] = {}
    for track, time, _, kind, name, _ in sorted(events, key=lambda event: (event[0] or 0, event[1], event[2])):
        if kind in (1, 2):
            slices.setdefault(threads.get(track, str(track)), []).append((time, name if kind == 1 else None))
    return slices


def perfetto_thread_tracks(path: Path) -> dict[int, tuple[int, int]]:
    """The `(pid, tid)` of each thread's track of a Perfetto trace, by the
    track's uuid, as its thread descriptor says (a track of its own, such
    as a User Timing measure's, has none)."""
    descriptors: dict[int, tuple[int, int]] = {}
    for number, packet in protobuf_fields(path.read_bytes()):
        if number != 1:
            continue
        fields: dict[int, object] = {}
        for key, value in protobuf_fields(packet):
            fields.setdefault(key, value)
        if 60 in fields:  # track_descriptor
            track = dict(protobuf_fields(fields[60]))
            if 4 in track:  # a thread's: pid, tid
                thread = dict(protobuf_fields(track[4]))
                descriptors[track.get(1)] = (thread.get(1, 0), thread.get(2, 0))
    return descriptors


def perfetto_marked_thread(path: Path) -> tuple[int, int] | None:
    """The `(pid, tid)` of the thread the most User Timing marks of a
    Perfetto trace are on, as its thread descriptor says (`None` without
    marks): on Linux Chromium leaves each process's main thread unnamed
    (naming it would rename the process), so `perf` calls a renderer's main
    thread by the binary's name, and its tid picks its samples."""
    descriptors = perfetto_thread_tracks(path)
    _, events = perfetto_track_events(path)
    marked = collections.Counter(track for track, _, _, kind, _, categories in events
                                 if kind == 3 and USER_TIMING in categories)
    return next((descriptors[track] for track, _ in marked.most_common() if track in descriptors), None)


def perfetto_marks(path: Path) -> dict[str, list[tuple[int, str]]]:
    """The User Timing marks of a Perfetto trace (`performance.mark()`,
    instants of `blink.user_timing`), by thread name, each `(time, name)`
    in time order."""
    threads, events = perfetto_track_events(path)
    marks: dict[str, list[tuple[int, str]]] = {}
    for track, time, _, kind, name, categories in sorted(events, key=lambda event: (event[1], event[2])):
        if kind == 3 and USER_TIMING in categories:
            marks.setdefault(threads.get(track, str(track)), []).append((time, name))
    return marks


# What `--sample` counts, each event's samples one every `SAMPLE_PERIOD` of
# it: so a count is so many million cycles or instructions, whatever the
# clock speed. Cycles are the time it takes; instructions, the work done,
# nearly the same however the build lays out the code or the CPU runs,
# but blind to waiting on memory: the two side by side.
SAMPLE_EVENTS = ("cycles", "instructions")


SAMPLE_PERIOD = 1_000_000


# macOS has no `perf`: samply takes the samples, the thread's stack this
# often (Hz), and each counts as the CPU time the thread used since the one
# before it (µs; `cpu`), so that a thread waiting counts nothing, as
# `cycles:u` doesn't on Linux.
SAMPLY_RATE = 1000


class Sampling:
    """How `--sample` samples machopo on a platform: the program it takes, the
    events its samples count (the first is the time it takes), the samples'
    file for a suite, and the reports' words for it: how it samples, how
    the stacks are unwound, and the V8 flag that names the JS."""

    def __init__(self, tool: str, events: tuple[str, ...], suffix: str, described: str, unwound: str,
                 v8: str) -> None:
        self.tool, self.events, self.suffix, self.described = tool, events, suffix, described
        self.unwound, self.v8 = unwound, v8


LINUX_SAMPLING = Sampling(
    "perf", SAMPLE_EVENTS, ".perf.data",
    f"`perf record --call-graph fp` of {' and '.join(f'`{e}:u`' for e in SAMPLE_EVENTS)}, one sample every "
    f"{SAMPLE_PERIOD:,}", "frame pointers", "--perf-basic-prof")


MACOS_SAMPLING = Sampling(
    "samply", ("cpu",), ".samply.json.gz",
    f"`samply record` at {SAMPLY_RATE:,} Hz (frame pointers), each sample counting the µs of CPU time the thread "
    "used since the one before it (`cpu`)", "frame pointers", "--perf-basic-prof")


# Windows has no `perf` either: samply takes the samples through ETW (with
# the Windows Performance Toolkit's xperf), whose stack walk goes by the
# binaries' unwind tables, the kernel's frames too (Linux's `cycles:u`
# leaves them out); the time as on macOS.
WINDOWS_SAMPLING = Sampling(
    "samply", ("cpu",), ".samply.json.gz",
    f"`samply record` at {SAMPLY_RATE:,} Hz (ETW), each sample counting the µs of CPU time the thread used since "
    "the one before it (`cpu`), the kernel's frames included", "unwind tables", "--enable-etw-stack-walking")


def sampling() -> Sampling | None:
    """This platform's way to sample machopo, or none."""
    if sys.platform.startswith("linux"):
        return LINUX_SAMPLING
    if sys.platform == "darwin":
        return MACOS_SAMPLING
    if sys.platform == "win32":
        return WINDOWS_SAMPLING
    return None


def sampler(samples: Path) -> tuple[list[str], str]:
    """What runs machopo to sample it into `samples`, and the V8 flags for it:
    `perf_sampler` on Linux, `samply_sampler` on macOS and Windows."""
    return samply_sampler(samples) if sys.platform in ("darwin", "win32") else perf_sampler(samples)


V8_SAMPLED_FLAGS = "--perf-basic-prof --interpreted-frames-native-stack --no-logfile-per-isolate --logfile={}"

# On Windows V8 tells ETW where each JS function's code is and its name
# (its `V8.js` provider, which samply's `--browsers` turns on); there's no
# perf map there.
V8_ETW_FLAGS = "--enable-etw-stack-walking --interpreted-frames-native-stack"

# Where Windows's own DLLs' symbols are (ntdll's, vcruntime140's, the
# kernel's): Microsoft's symbol server, each PDB downloaded once into
# samply's cache.
WINDOWS_SYMBOL_SERVER = "https://msdl.microsoft.com/download/symbols"


def perf_sampler(samples: Path) -> tuple[list[str], str]:
    """`perf record` of machopo into `samples` (its user-space cycles and
    instructions, `SAMPLE_EVENTS`, one sample every `SAMPLE_PERIOD`; the
    stack unwound by frame pointers, V8's and the `perf` profile's, its
    user-space frames alone: without `--user-callchains`, `--call-graph`
    asks for the kernel's too, which a `perf_event_paranoid` of 2 refuses,
    and the events open none; each sample's time on CLOCK_MONOTONIC: a hardware event's can't be on
    CLOCK_BOOTTIME, the trace's), and the V8 flags that name JS functions
    for it: V8 writes `/tmp/perf-<pid>.map`, and an interpreted function
    has a frame of its own. The log V8 writes along (by default a file for
    each isolate, where machopo runs) goes to `v8_log(samples)`; it is of no
    use here. (`--logfile=/dev/null` hangs machopo as it starts, V8 15.2.)"""
    samples.unlink(missing_ok=True)
    events = [arg for event in SAMPLE_EVENTS for arg in ("-e", f"{event}:u")]
    return (["perf", "record", "-q", *events, "-c", str(SAMPLE_PERIOD), "-k", "monotonic", "--call-graph", "fp",
             "--user-callchains", "-o", str(samples), "--"],
            V8_SAMPLED_FLAGS.format(v8_log(samples)))


def samply_sampler(samples: Path) -> tuple[list[str], str]:
    """`samply record` of machopo into `samples` (its threads' stacks
    `SAMPLY_RATE` times a second), with the symbols of its frames beside it
    (`samply_symbols`: the profile itself has only addresses, which
    samply's UI looks up), and the V8 flags that name JS functions for it.
    On macOS the stacks are unwound by frame pointers, which arm64 macOS
    always keeps, and V8's flags are as for `perf_sampler`: samply reads
    `/tmp/perf-<pid>.map` too. On Windows ETW unwinds them, V8 names its
    functions to ETW (`V8_ETW_FLAGS`, `--browsers`), and samply keeps its
    ETL files (`samply_etls`), whose start its samples' times count from.
    samply passes machopo's standard input and output through."""
    samples.unlink(missing_ok=True)
    samply_symbols(samples).unlink(missing_ok=True)
    options = ["--save-only", "--no-open", "--unstable-presymbolicate", "-r", str(SAMPLY_RATE)]
    if sys.platform != "win32":
        return (["samply", "record", *options, "-o", str(samples), "--"], V8_SAMPLED_FLAGS.format(v8_log(samples)))
    for etl in samply_etls(samples):
        etl.unlink(missing_ok=True)
    return (["samply", "record", *options, "--browsers", "--keep-etl", "-o", str(samples), "--"], V8_ETW_FLAGS)


def samply_etls(samples: Path) -> tuple[Path, Path]:
    """The ETL files samply keeps beside its profile `samples` on Windows
    (`--keep-etl`): the kernel's session's (the samples) and the user
    one's (V8's events)."""
    name = samples.name.removesuffix(".json.gz")
    return samples.with_name(name + ".kernel.etl"), samples.with_name(name + ".user.etl")


def samply_symbols(samples: Path) -> Path:
    """The symbols samply writes beside its profile `samples`."""
    return samples.with_name(samples.name.removesuffix(".gz") + ".syms.json")


def v8_log(samples: Path) -> Path:
    """Where V8 writes its log while machopo is sampled into `samples`."""
    name = samples.name
    for suffix in (LINUX_SAMPLING.suffix, MACOS_SAMPLING.suffix):
        name = name.removesuffix(suffix)
    return samples.with_name(name + ".v8.log")
