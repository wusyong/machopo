#!/usr/bin/env python3
"""Runs machopo --remote-debugging-pipe --headless under strace, loads a page in its renderer
process, and says what the renderer process asked of the kernel.

    probe.py OUT_DIR [machopo's extra arguments...]
    probe.py --report OUT_DIR      (the report again, from OUT_DIR's trace)

`PROBE_URL` in the environment: the page to load in place of the built-in
one (a wasm document's `file:` URL, `about:crash`).
"""
import collections
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse

MACHOPO = "/home/wusyong/Desktop/m/machopo/target/debug/machopo"
PAGE = """<!DOCTYPE html><meta charset=utf-8>
<style>p{font-family:serif} .s{font-family:sans-serif} .m{font-family:monospace}
.x{font-family:system-ui} .b{font-weight:bold;font-style:italic}</style>
<p>Serif text</p><p class=s>Sans text</p><p class=m>Mono text</p>
<p class=x>中文字 日本語 한국어 😀 العربية עברית ไทย</p><p class="s b">Bold italic</p>
<input value=field><button>Button</button>
<canvas id=c width=100 height=40></canvas><iframe srcdoc="<p>child frame"></iframe>
<script>
let s = 0; for (let i = 0; i < 3e6; i++) s += i % 7;
const c = document.getElementById('c').getContext('2d');
c.font = '16px serif'; c.fillText('canvas', 10, 20);
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone + ' ' + new Date().toString();
const lang = navigator.language + ' ' + (1234.5).toLocaleString();
WebAssembly.instantiate(new Uint8Array([0,97,115,109,1,0,0,0]));
crypto.getRandomValues(new Uint8Array(8)); Math.random();
new Worker(URL.createObjectURL(new Blob(['postMessage(1)'])));
document.title = 'done ' + s + ' ' + zone + ' ' + lang;
</script>"""


def main():
    out = sys.argv[1]
    extra = sys.argv[2:]
    os.makedirs(out, exist_ok=True)
    trace = os.path.join(out, "trace.txt")
    # The pipe is file descriptors 3 (what machopo reads) and 4 (what it
    # writes), as Chrome's: `sh` moves the ends there, and strace hands
    # them on.
    to_child, to_browser = os.pipe()
    from_browser, from_child = os.pipe()
    child = subprocess.Popen(
        ["sh", "-c", f'exec "$0" "$@" 3<&{to_child} 4>&{from_child}',
         "strace", "-f", "-qq", "-s", "200", "-o", trace, MACHOPO,
         "--remote-debugging-pipe", "--headless", *extra],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, pass_fds=(to_child, from_child),
        stderr=open(os.path.join(out, "machopo.log"), "wb"))
    os.close(to_child)
    os.close(from_child)
    to_browser = os.fdopen(to_browser, "wb")
    buffered = b""
    ids = iter(range(1, 1000))

    def read():
        nonlocal buffered
        while b"\0" not in buffered:
            chunk = os.read(from_browser, 65536)
            if not chunk:
                raise SystemExit("machopo ended early")
            buffered += chunk
        message, buffered = buffered.split(b"\0", 1)
        return json.loads(message)

    def call(method, params=None, session=None):
        number = next(ids)
        command = {"id": number, "method": method, "params": params or {}}
        if session:
            command["sessionId"] = session
        to_browser.write(json.dumps(command).encode() + b"\0")
        to_browser.flush()
        while True:
            message = read()
            if message.get("id") == number:
                if "error" in message:
                    raise SystemExit(f"{method}: {message['error']}")
                return message.get("result", {})

    target = call("Target.getTargets")["targetInfos"][0]["targetId"]
    session = call("Target.attachToTarget", {"targetId": target, "flatten": True})["sessionId"]
    call("Page.enable", session=session)
    url = os.environ.get("PROBE_URL") or (
        "data:text/html;charset=utf-8," + urllib.parse.quote(PAGE))
    call("Page.navigate", {"url": url}, session=session)
    time.sleep(4)
    # A page that crashed its renderer answers none of these.
    try:
        title = call("Runtime.evaluate", {"expression": "document.title"}, session=session)
        print("the page's title:", title.get("result", {}).get("value"))
        call("Page.captureScreenshot", session=session)
    except SystemExit as failed:
        print("after the load:", failed)
    try:
        call("Browser.close")
    except SystemExit:
        pass
    to_browser.close()
    child.wait(timeout=60)
    report(trace, out)


LINE = re.compile(r"^(\d+)\s+(\w+)\((.*)$")


def joined(trace):
    """strace -f's lines, a call cut in two (`<unfinished ...>`, then
    `<... name resumed>`) put back as one."""
    pending = {}
    lines = []
    for line in open(trace, errors="replace").read().splitlines():
        resumed = re.match(r"^(\d+)\s+<\.\.\. (\w+) resumed>(.*)$", line)
        if resumed:
            pid, name, rest = resumed.groups()
            start = pending.pop((pid, name), f"{pid} {name}(")
            lines.append(start + rest)
        elif line.endswith("<unfinished ...>"):
            m = LINE.match(line)
            if m:
                pending[(m.group(1), m.group(2))] = line[: -len("<unfinished ...>")]
        else:
            lines.append(line)
    lines.extend(pending.values())
    return lines


def report(trace, out):
    lines = joined(trace)
    # The renderer process: what execve'd with --type=renderer, and the
    # threads it made (clone's and clone3's results).
    renderers = set()
    for line in lines:
        m = LINE.match(line)
        if m and m.group(2) == "execve" and "--type=renderer" in m.group(3):
            renderers.add(m.group(1))
    grew = True
    while grew:
        grew = False
        for line in lines:
            m = LINE.match(line)
            if m and m.group(1) in renderers and m.group(2) in ("clone", "clone3", "fork", "vfork"):
                made = re.search(r"=\s+(\d+)\s*$", line)
                if made and made.group(1) not in renderers:
                    renderers.add(made.group(1))
                    grew = True
    # Only from the renderer's own execve on (before it, the pid is the
    # browser's fork).
    started = set()
    calls = collections.Counter()
    paths = collections.OrderedDict()
    other = []
    for line in lines:
        m = LINE.match(line)
        if not m or m.group(1) not in renderers:
            continue
        pid, name, rest = m.groups()
        if name == "execve" and "--type=renderer" in rest:
            started.add(pid)
        if not started:
            continue
        calls[name] += 1
        if name in ("open", "openat", "openat2", "stat", "lstat", "newfstatat", "statx", "access",
                    "faccessat", "faccessat2", "readlink", "readlinkat", "execve", "unlink",
                    "unlinkat", "mkdir", "mkdirat", "rename", "renameat", "renameat2", "chdir",
                    "statfs", "getdents64", "inotify_add_watch", "utimensat", "chmod", "truncate"):
            found = re.search(r'"([^"]*)"', rest)
            flags = re.search(r"O_[A-Z_|]+", rest)
            failed = re.search(r"=\s+-1\s+(\w+)", rest)
            key = (found.group(1) if found else "?")
            entry = paths.setdefault(key, {"calls": set(), "flags": set(), "errors": set()})
            entry["calls"].add(name)
            if flags:
                entry["flags"].add(flags.group(0))
            if failed:
                entry["errors"].add(failed.group(1))
        if name in ("socket", "connect", "bind", "socketpair", "memfd_create", "prctl", "seccomp",
                    "ioctl", "shmget", "mq_open", "ptrace", "kill", "tgkill", "setrlimit",
                    "prlimit64", "sched_setaffinity", "sched_setscheduler", "setpriority",
                    "io_uring_setup", "perf_event_open", "bpf", "userfaultfd", "memfd_secret",
                    "pkey_alloc", "pkey_mprotect", "mlock", "madvise", "personality", "uname",
                    "sysinfo", "getrandom", "eventfd2", "epoll_create1", "timerfd_create",
                    "pipe2", "inotify_init1", "signalfd4", "capget", "getuid", "sched_getaffinity"):
            other.append(f"{name}({rest[:150]}")
    with open(os.path.join(out, "report.txt"), "w") as f:
        f.write(f"renderer pids and threads: {len(renderers)}\n\n== system calls ({len(calls)}) ==\n")
        for name, count in sorted(calls.items(), key=lambda item: -item[1]):
            f.write(f"{count:8} {name}\n")
        f.write(f"\n== paths ({len(paths)}) ==\n")
        for path, entry in paths.items():
            f.write(f"{path}  [{','.join(sorted(entry['calls']))}] "
                    f"{','.join(sorted(entry['flags']))} {','.join(sorted(entry['errors']))}\n")
        f.write("\n== other calls of interest (first of each shape) ==\n")
        seen = set()
        for line in other:
            shape = re.sub(r"\d+", "N", line)[:90]
            if shape not in seen:
                seen.add(shape)
                f.write(line + "\n")
        # mmap and mprotect with PROT_EXEC (the JITs).
        execs = collections.Counter()
        for line in lines:
            m = LINE.match(line)
            if m and m.group(1) in renderers and m.group(2) in ("mmap", "mprotect", "mremap"):
                prot = re.search(r"PROT_[A-Z_|]+", m.group(3))
                flags = re.search(r"MAP_[A-Z_|]+", m.group(3))
                if prot and "PROT_EXEC" in prot.group(0):
                    execs[(m.group(2), prot.group(0), flags.group(0) if flags else "")] += 1
        f.write("\n== executable mappings ==\n")
        for (name, prot, flags), count in execs.most_common():
            f.write(f"{count:6} {name} {prot} {flags}\n")
    print("report:", os.path.join(out, "report.txt"))


if __name__ == "__main__":
    if sys.argv[1] == "--report":
        report(os.path.join(sys.argv[2], "trace.txt"), sys.argv[2])
    else:
        main()
