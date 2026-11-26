#!/usr/bin/env python3
"""Runs machopo --remote-debugging-pipe --headless on macOS, loads a page in
its renderer process, and says what the sandbox's log held of the renderer
process meanwhile.

    probe_macos.py OUT_DIR [machopo's extra arguments...]
    probe_macos.py --report OUT_DIR      (the report again, from OUT_DIR's log)

Two ways, by `PROBE_PROFILE` in the environment:

- A Seatbelt profile's file: the whole of machopo runs under it
  (`sandbox-exec -f`), its renderer process inheriting it. With
  `(allow (with report) default)` everything is let through and logged:
  what a renderer asks of the system when nothing is refused.
- Unset: machopo as it is, its renderer in its own sandbox, and the log is
  what that sandbox refused.

`PROBE_URL` in the environment: the page to load in place of the built-in
one (a wasm document's `file:` URL, `about:crash`).

The kernel's log leaves out a report it has made lately for the same
process, operation and target: the counts are of kinds, not of calls.
"""
import base64
import collections
import json
import os
import re
import select
import subprocess
import sys
import time
import urllib.parse

MACHOPO = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "..", "..", "target", "debug", "machopo")
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
    log_path = os.path.join(out, "sandbox.log")
    # The kernel's sandbox reports, as they come.
    log = subprocess.Popen(
        ["/usr/bin/log", "stream", "--style", "compact", "--predicate",
         'sender == "Sandbox" AND eventMessage CONTAINS "machopo("'],
        stdout=open(log_path, "wb"), stderr=subprocess.DEVNULL)
    time.sleep(2)
    command = [MACHOPO, "--remote-debugging-pipe", "--headless", *extra]
    profile = os.environ.get("PROBE_PROFILE")
    if profile:
        command = ["/usr/bin/sandbox-exec", "-f", profile, *command]
    # The pipe is file descriptors 3 (what machopo reads) and 4 (what it
    # writes), as Chrome's: `sh` moves the ends there.
    to_child, to_browser = os.pipe()
    from_browser, from_child = os.pipe()
    child = subprocess.Popen(
        ["sh", "-c", f'exec "$0" "$@" 3<&{to_child} 4>&{from_child}', *command],
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
            # A renderer that didn't start leaves the browser with nothing
            # to answer.
            if not select.select([from_browser], [], [], 20)[0]:
                raise SystemExit("machopo didn't answer")
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

    try:
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
            shot = call("Page.captureScreenshot", session=session)
            with open(os.path.join(out, "page.png"), "wb") as f:
                f.write(base64.b64decode(shot.get("data", "")))
        except SystemExit as failed:
            print("after the load:", failed)
        try:
            call("Browser.close")
        except SystemExit:
            pass
    except SystemExit as failed:
        print("machopo:", failed)
    to_browser.close()
    try:
        child.wait(timeout=10)
    except subprocess.TimeoutExpired:
        child.kill()
    time.sleep(2)
    log.terminate()
    log.wait()
    with open(os.path.join(out, "browser.pid"), "w") as f:
        f.write(str(child.pid))
    report(out)


# `Sandbox: machopo(123) allow file-read-data /path`, with a duplicate's
# count before the verdict at times.
LINE = re.compile(r"Sandbox: (?:\d+ duplicate reports? for )?machopo\((\d+)\) (allow|deny)\(?\d*\)? (\S+) ?(.*)$")


def report(out):
    browser = open(os.path.join(out, "browser.pid")).read().strip()
    kinds = collections.OrderedDict()
    for line in open(os.path.join(out, "sandbox.log"), errors="replace"):
        m = LINE.search(line.rstrip())
        if not m or m.group(1) == browser:
            continue
        _, verdict, operation, target = m.groups()
        kinds.setdefault((verdict, operation), collections.Counter())[target] += 1
    with open(os.path.join(out, "report.txt"), "w") as f:
        f.write("== operations ==\n")
        for (verdict, operation), targets in kinds.items():
            f.write(f"{sum(targets.values()):6} {verdict} {operation} ({len(targets)} targets)\n")
        for (verdict, operation), targets in kinds.items():
            f.write(f"\n== {verdict} {operation} ==\n")
            for target, count in sorted(targets.items()):
                f.write(f"{count:6} {target}\n")
    print("report:", os.path.join(out, "report.txt"))


if __name__ == "__main__":
    if sys.argv[1] == "--report":
        report(sys.argv[2])
    else:
        main()
