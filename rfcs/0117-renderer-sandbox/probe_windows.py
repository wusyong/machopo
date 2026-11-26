#!/usr/bin/env python3
"""Runs machopo --remote-debugging-pipe --headless on Windows, loads a page
in its renderer process, and says what came of it: the page's title, its
screenshot's hash, and what the renderer process looked like from outside
(its token's integrity level and restrictions, the modules it loaded).

    probe_windows.py OUT_DIR [machopo's extra arguments...]

The probe's own runs were of a prototype build (not kept) that read from
the environment how to start its renderer: which token, integrity level,
app container, job and mitigations. With machopo as it is, this shows the
renderer in its sandbox, and with `--no-sandbox` outside.

Also in the environment: `PROBE_WINDOW` (a real window, not headless),
`PROBE_MACHOPO` (another executable), `PROBE_CWD` (where it runs),
`PROBE_WATCH` (`crashwatch_windows.py`'s path: started for this machopo).

`PROBE_URL` in the environment: the page to load in place of the built-in
one (a wasm document's `file:` URL, `about:crash`).
"""
import base64
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import msvcrt
import os
import queue
import subprocess
import sys
import threading
import time
import urllib.parse

MACHOPO = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "..", "..", "target", "debug", "machopo.exe")
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

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)
kernel32.OpenProcess.restype = wt.HANDLE
kernel32.CreateToolhelp32Snapshot.restype = wt.HANDLE


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wt.DWORD),
                ("cntThreads", wt.DWORD), ("th32ParentProcessID", wt.DWORD),
                ("pcPriClassBase", wt.LONG), ("dwFlags", wt.DWORD), ("szExeFile", wt.WCHAR * 260)]


def children(parent):
    snapshot = kernel32.CreateToolhelp32Snapshot(2, 0)
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    found = []
    more = kernel32.Process32FirstW(wt.HANDLE(snapshot), ctypes.byref(entry))
    while more:
        if entry.th32ParentProcessID == parent:
            found.append((entry.th32ProcessID, entry.cntThreads))
        more = kernel32.Process32NextW(wt.HANDLE(snapshot), ctypes.byref(entry))
    kernel32.CloseHandle(wt.HANDLE(snapshot))
    return found


def token_information(token, kind):
    size = wt.DWORD()
    advapi32.GetTokenInformation(token, kind, None, 0, ctypes.byref(size))
    buffer = ctypes.create_string_buffer(size.value)
    if not advapi32.GetTokenInformation(token, kind, buffer, size, ctypes.byref(size)):
        return None
    return buffer


def sid_text(sid):
    text = wt.LPWSTR()
    advapi32.ConvertSidToStringSidW(ctypes.c_void_p(sid), ctypes.byref(text))
    return text.value


def describe(pid, out):
    """What the process `pid` is, from outside."""
    lines = []
    process = kernel32.OpenProcess(0x0410, False, pid)  # QUERY_INFORMATION | VM_READ
    if not process:
        process = kernel32.OpenProcess(0x1000, False, pid)  # QUERY_LIMITED_INFORMATION
    if not process:
        return [f"  not opened: error {ctypes.get_last_error()}"]
    process = wt.HANDLE(process)
    token = wt.HANDLE()
    if advapi32.OpenProcessToken(process, 0x0008, ctypes.byref(token)):  # TOKEN_QUERY
        label = token_information(token, 25)  # TokenIntegrityLevel
        if label:
            lines.append("  integrity: " + sid_text(ctypes.c_void_p.from_buffer(label).value))
        restricted = token_information(token, 11)  # TokenRestrictedSids
        if restricted:
            count = wt.DWORD.from_buffer(restricted).value
            pointer = ctypes.sizeof(ctypes.c_void_p)
            sids = [sid_text(ctypes.c_void_p.from_buffer(restricted, pointer + i * 2 * pointer).value)
                    for i in range(count)]
            lines.append(f"  restricted to: {sids}")
        container = token_information(token, 29)  # TokenIsAppContainer
        if container:
            lines.append(f"  app container: {bool(wt.DWORD.from_buffer(container).value)}")
        kernel32.CloseHandle(token)
    else:
        lines.append(f"  token not opened: error {ctypes.get_last_error()}")
    modules = (wt.HMODULE * 1024)()
    needed = wt.DWORD()
    names = []
    if psapi.EnumProcessModulesEx(process, modules, ctypes.sizeof(modules), ctypes.byref(needed), 3):
        for module in modules[:needed.value // ctypes.sizeof(wt.HMODULE)]:
            name = ctypes.create_unicode_buffer(520)
            psapi.GetModuleFileNameExW(process, wt.HMODULE(module), name, 520)
            names.append(os.path.basename(name.value).lower())
        lines.append(f"  modules: {len(names)}")
    else:
        lines.append(f"  modules not listed: error {ctypes.get_last_error()}")
    with open(os.path.join(out, f"modules.{pid}.txt"), "w", newline="\n") as f:
        f.write("\n".join(sorted(names)) + "\n")
    kernel32.CloseHandle(process)
    return lines


def main():
    out = sys.argv[1]
    extra = sys.argv[2:]
    os.makedirs(out, exist_ok=True)
    to_child, to_browser = os.pipe()
    from_browser, from_child = os.pipe()
    handles = [msvcrt.get_osfhandle(to_child), msvcrt.get_osfhandle(from_child)]
    for handle in handles:
        os.set_handle_inheritable(handle, True)
    startup = subprocess.STARTUPINFO()
    startup.lpAttributeList = {"handle_list": handles}
    child = subprocess.Popen(
        [os.environ.get("PROBE_MACHOPO") or MACHOPO, "--remote-debugging-pipe", *([] if os.environ.get("PROBE_WINDOW") else ["--headless"]), *extra,
         f"--remote-debugging-io-pipes={handles[0]},{handles[1]}"],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, startupinfo=startup, close_fds=True,
        cwd=os.environ.get("PROBE_CWD"),
        stderr=open(os.path.join(out, "machopo.log"), "wb"))
    if os.environ.get("PROBE_WATCH"):
        subprocess.Popen([sys.executable, os.environ["PROBE_WATCH"], str(child.pid)])
    os.close(to_child)
    os.close(from_child)
    to_browser = os.fdopen(to_browser, "wb")
    messages = queue.Queue()

    def reader():
        buffered = b""
        while True:
            try:
                chunk = os.read(from_browser, 65536)
            except OSError:
                chunk = b""
            if not chunk:
                messages.put(None)
                return
            buffered += chunk
            while b"\0" in buffered:
                message, buffered = buffered.split(b"\0", 1)
                messages.put(json.loads(message))

    threading.Thread(target=reader, daemon=True).start()
    ids = iter(range(1, 1000))

    def call(method, params=None, session=None):
        number = next(ids)
        command = {"id": number, "method": method, "params": params or {}}
        if session:
            command["sessionId"] = session
        to_browser.write(json.dumps(command).encode() + b"\0")
        to_browser.flush()
        while True:
            try:
                # A renderer that didn't start leaves the browser with
                # nothing to answer.
                message = messages.get(timeout=20)
            except queue.Empty:
                raise SystemExit("machopo didn't answer")
            if message is None:
                raise SystemExit("machopo ended early")
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
        for pid, threads in children(child.pid):
            print(f"child {pid}: {threads} threads")
            print("\n".join(describe(pid, out)))
        # A page that crashed its renderer answers none of these.
        try:
            title = call("Runtime.evaluate", {"expression": "document.title"}, session=session)
            print("the page's title:", title.get("result", {}).get("value"))
            shot = base64.b64decode(call("Page.captureScreenshot", session=session).get("data", ""))
            with open(os.path.join(out, "page.png"), "wb") as f:
                f.write(shot)
            print("the screenshot:", hashlib.sha256(shot).hexdigest()[:16])
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
    log = open(os.path.join(out, "machopo.log"), errors="replace").read().strip()
    # Its log's ordinary lines aside.
    lines = [line[:300] for line in log.splitlines() if " INFO" not in line and "ICU4X" not in line]
    if lines:
        print("machopo's stderr:")
        for line in lines[:20]:
            print("  " + line)
    ended = [line.split("status")[-1][:60] for line in log.splitlines() if "a renderer process ended" in line]
    print("renderer ended:", ended)


if __name__ == "__main__":
    main()
