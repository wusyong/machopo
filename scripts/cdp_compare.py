"""`test.py cdp`: machopo's answers to DevTools' JavaScript domains against
Chrome's. Each case sends the same commands to machopo and to Chrome for
Testing (the version `bench.py` pins, whose V8 is machopo's), on the same pages,
and keeps the answers and the events it names; the values that differ from
run to run (object, script, context and exception ids, times, the target's
id) become placeholders, numbered as they first appear, so that which ones
are the same thing still counts. What differs is printed; the known
differences, each with its reason, are left out.

Standard library only (and `scripts/bench/common.py`).
"""

from __future__ import annotations

import json
import queue
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts" / "bench"))
import common  # noqa: E402

# The pages the cases load. Their scripts' code starts a line: machopo counts
# an inline script's columns from where its code starts, Chrome from the
# start of that line.
PAGES = {
    "/": "<!DOCTYPE html><title>Top</title><p>top",
    "/frames.html": (
        "<!DOCTYPE html><title>Frames</title><iframe src='/child.html'></iframe>"
        "<iframe src='data:text/html,<title>Data</title>'></iframe>"
    ),
    "/child.html": "<!DOCTYPE html><title>Child</title>child",
    "/add.html": (
        "<!DOCTYPE html>\n<script>\nfunction add(a, b) {\n  const sum = a + b;\n  return sum;\n}\n</script>"
    ),
    "/throws.html": (
        "<!DOCTYPE html>\n<script>\nthrow new TypeError('boom');</script>\n"
        "<script>\nwindow.p = Promise.reject(new Error('later'));</script>"
    ),
    "/console.html": (
        "<!DOCTYPE html>\n<script>\nconsole.log('a', 1, {b: 2}, [1, 2]);\n"
        "console.warn('w'); console.error(new Error('e'));\n</script>"
    ),
    "/life.html": (
        "<!DOCTYPE html>\n<title>Life</title>\n<link rel=stylesheet href='/style.css'>\n"
        "<img src='/missing.png'>\n<iframe src='/child.html'></iframe>"
    ),
}

# The other files the pages load, with their MIME types.
FILES = {
    "/style.css": ("p { color: green }", "text/css"),
}

# What Chrome loads of its own (headless Chrome asks for the favicon; machopo
# has none): left out of the `network` and `log` cases.
CHROMES_OWN = ("/favicon.ico",)

# Chrome's paint lifecycle events, which machopo doesn't tell (it has no paint
# timing yet): left out of the `lifecycle` case.
PAINT_EVENTS = {
    "firstPaint",
    "firstContentfulPaint",
    "firstImagePaint",
    "firstMeaningfulPaint",
    "firstMeaningfulPaintCandidate",
    "InteractiveTime",
}

# Keys whose values differ from run to run: placeholders, by key.
VOLATILE = {
    "objectId",
    "scriptId",
    "executionContextId",
    "uniqueId",
    "executionContextUniqueId",
    "exceptionId",
    "timestamp",
    "debuggerId",
    "sessionId",
    "callFrameId",
    "breakpointId",
    "loaderId",
    "hash",
}

# What machopo is known to do differently, by case and JSON path (a prefix):
# left out of the comparison, each with why.
KNOWN: dict[tuple[str, str], str] = {
    ("navigation", "[1]"): "Chrome clears the contexts twice, as the old document goes and as the new one's "
    "window is made; once is all DevTools needs",
    ("navigation", "[2]: machopo has none"): "the same",
    ("contexts", "[3].params.context.auxData.frameId"): "Chrome still has the initial about:blank's context of the "
    "frame that went to data: (Blink makes a new frame for the opaque origin); machopo's went with its document",
    ("contexts", "[4]: machopo has none"): "the same",
    ("page", "[3]."): "machopo keeps every document's markup; Chrome answers from its caches, which the test server's "
    "responses don't go in",
    ("debugger", "[2].params.hitBreakpoints[0]"): "Chrome's breakpoint ids start with br:, which the V8 source "
    "machopo builds doesn't put there; DevTools takes them as they are",
}


class Pages(common.Server):
    """`PAGES`, as HTML."""

    def get(self, path: str):
        if path in FILES:
            body, mime_type = FILES[path]
            return body.encode(), mime_type
        page = PAGES.get(path)
        if page is None:
            return None, ""
        return page.encode(), "text/html; charset=utf-8"


@dataclass
class Run:
    """A case's way to a browser: a session of its one tab."""

    pipe: common.CdpPipe
    session: str
    target: str
    # Events read while waiting for something else, for `events`.
    kept: list[dict] = field(default_factory=list)

    def _next_event(self, timeout: float) -> dict | None:
        """The next event: one kept, else one from the pipe. Raises
        `queue.Empty` after `timeout` seconds."""
        if self.kept:
            return self.kept.pop(0)
        return self.pipe.events.get(timeout=timeout)

    def call(self, method: str, params: dict | None = None) -> dict:
        """The whole answer (`result` or `error`)."""
        return self.answer(self.send(method, params))

    def send(self, method: str, params: dict | None = None) -> int:
        """Sends `method`, not waiting for its answer: its id."""
        self.pipe.next_id += 1
        command = {"id": self.pipe.next_id, "method": method, "params": params or {}, "sessionId": self.session}
        self.pipe.to_browser.write(json.dumps(command).encode() + b"\0")
        self.pipe.to_browser.flush()
        return self.pipe.next_id

    def answer(self, id: int) -> dict:
        """The answer to the command `id` (`result` or `error`)."""
        deadline = time.monotonic() + 30
        while True:
            message = self.pipe.answers.get(timeout=max(0.0, deadline - time.monotonic()))
            if message is None:
                raise RuntimeError(f"{self.pipe.name} has ended")
            if message.get("id") == id:
                return {key: value for key, value in message.items() if key in ("result", "error")}

    def events(self, methods: tuple[str, ...], settle: float = 0.5) -> list[dict]:
        """The events named `methods` that came so far, after `settle`
        seconds for the last ones, in order."""
        time.sleep(settle)
        found = []
        while True:
            try:
                message = self._next_event(timeout=0)
            except queue.Empty:
                return found
            if message is None:
                return found
            if message.get("method") in methods and message.get("sessionId") == self.session:
                found.append({"method": message["method"], "params": message.get("params", {})})

    def wait_for(self, method: str) -> dict:
        """The next event `method` (others kept for `events`)."""
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                message = self._next_event(timeout=1)
            except queue.Empty:
                continue
            if message is None:
                break
            if message.get("method") == method and message.get("sessionId") == self.session:
                return {"method": method, "params": message.get("params", {})}
        raise RuntimeError(f"{self.pipe.name} sent no {method}")

    def load(self, url: str) -> None:
        """Its tab at `url`, loaded: the navigation's own `load` (the
        lifecycle events come only with `Page` on, as it is for the load;
        turning them on tells the document shown's). The other events that
        came meanwhile are kept for `events`, but `Page`'s."""
        self.call("Page.enable")
        self.call("Page.setLifecycleEventsEnabled", {"enabled": True})
        loader = self.call("Page.navigate", {"url": url}).get("result", {}).get("loaderId")
        deadline = time.monotonic() + 30
        waiting = []
        while time.monotonic() < deadline:
            try:
                message = self.pipe.events.get(timeout=1)
            except queue.Empty:
                continue
            if message is None:
                break
            params = message.get("params", {})
            if (
                message.get("method") == "Page.lifecycleEvent"
                and params.get("name") == "load"
                and params.get("loaderId") == loader
            ):
                self.call("Page.disable")
                self.kept.extend(m for m in waiting if not m.get("method", "").startswith("Page."))
                return
            waiting.append(message)
        raise RuntimeError(f"{self.pipe.name} didn't load {url}")


def case_values(run: Run, url: Callable[[str], str]) -> list:
    """`Runtime.evaluate`'s RemoteObjects (with previews), and a value
    thrown."""
    run.load(url("/"))
    transcript = []
    for expression in [
        "1 + 1",
        "'s'",
        "undefined",
        "null",
        "10n",
        "Symbol('s')",
        "({a: 1, b: [1, 2], c: {d: 'e'}})",
        "[1, 'x', {y: 2}]",
        "new Map([[1, 2]])",
        "new Error('x')",
        "() => 1",
        "document.title",
    ]:
        transcript.append(
            run.call("Runtime.evaluate", {"expression": expression, "generatePreview": True})
        )
    transcript.append(run.call("Runtime.evaluate", {"expression": "throw new TypeError('t')"}))
    transcript.append(run.call("Runtime.evaluate", {"expression": "JSON.stringify([1, {a: 2}])", "returnByValue": True}))
    return transcript


def case_properties(run: Run, url: Callable[[str], str]) -> list:
    """`Runtime.getProperties` and `callFunctionOn` on an object."""
    run.load(url("/"))
    made = run.call("Runtime.evaluate", {"expression": "({x: 1, get y() { return 2; }, z: [3]})"})
    object_id = made["result"]["result"]["objectId"]
    return [
        made,
        run.call("Runtime.getProperties", {"objectId": object_id, "ownProperties": True}),
        run.call(
            "Runtime.callFunctionOn",
            {"objectId": object_id, "functionDeclaration": "function () { return this.x + this.y; }", "returnByValue": True},
        ),
    ]


def case_console(run: Run, url: Callable[[str], str]) -> list:
    """`console`'s calls as DevTools gets them."""
    enabled = run.call("Runtime.enable")
    run.events(("Runtime.executionContextCreated",), settle=0.2)
    run.load(url("/console.html"))
    return [enabled, *run.events(("Runtime.consoleAPICalled",))]


def case_contexts(run: Run, url: Callable[[str], str]) -> list:
    """The frames' contexts, as `Runtime.enable` tells them."""
    run.load(url("/frames.html"))
    enabled = run.call("Runtime.enable")
    events = run.events(("Runtime.executionContextCreated",))
    events.sort(
        key=lambda event: (
            event["params"]["context"]["auxData"].get("frameId") != run.target,
            event["params"]["context"]["origin"],
        )
    )
    return [enabled, *events]


def case_exceptions(run: Run, url: Callable[[str], str]) -> list:
    """An uncaught exception, an unhandled rejection, and its handler
    later."""
    run.call("Runtime.enable")
    run.events(("Runtime.executionContextCreated",), settle=0.2)
    run.load(url("/throws.html"))
    thrown = run.events(("Runtime.exceptionThrown",))
    run.call("Runtime.evaluate", {"expression": "p.catch(() => {}); 0"})
    return [*thrown, *run.events(("Runtime.exceptionRevoked",))]


def case_navigation(run: Run, url: Callable[[str], str]) -> list:
    """The contexts as the tab goes from a page with a frame to another."""
    run.load(url("/frames.html"))
    run.call("Runtime.enable")
    run.events(("Runtime.executionContextCreated",))
    run.load(url("/"))
    return run.events(
        (
            "Runtime.executionContextCreated",
            "Runtime.executionContextDestroyed",
            "Runtime.executionContextsCleared",
        )
    )


def case_debugger(run: Run, url: Callable[[str], str]) -> list:
    """A breakpoint, where it pauses (call frames, scopes), a step, and the
    value on the frame."""
    run.load(url("/add.html"))
    enabled = run.call("Debugger.enable")
    run.events(("Debugger.scriptParsed",), settle=0.2)
    breakpoint = run.call("Debugger.setBreakpointByUrl", {"url": url("/add.html"), "lineNumber": 3})
    evaluated = run.send("Runtime.evaluate", {"expression": "add(1, 2)", "returnByValue": True})
    paused = run.wait_for("Debugger.paused")
    stepped = run.call("Debugger.stepOver")
    paused_again = run.wait_for("Debugger.paused")
    frame = paused_again["params"]["callFrames"][0]["callFrameId"]
    value = run.call("Debugger.evaluateOnCallFrame", {"callFrameId": frame, "expression": "sum", "returnByValue": True})
    resumed = run.call("Debugger.resume")
    return [enabled, breakpoint, paused, stepped, paused_again, value, resumed, run.answer(evaluated)]


def case_page(run: Run, url: Callable[[str], str]) -> list:
    """The frame tree and the main document's markup; the frames attached,
    navigated and detached as an iframe comes and goes."""
    run.load(url("/frames.html"))
    tree = run.call("Page.getResourceTree")
    # The child frames as Chrome's headless lists them after the load (no
    # URL, an opaque origin: in a process of their own, it seems) can't be
    # compared: the main frame and how many there are below it.
    main = tree.get("result", {}).get("frameTree", {})
    tree = {"frame": main.get("frame"), "children": len(main.get("childFrames", [])), "resources": main.get("resources")}
    target = run.target
    content_off = run.call("Page.getResourceContent", {"frameId": target, "url": url("/frames.html")})
    enabled = run.call("Page.enable")
    content = run.call("Page.getResourceContent", {"frameId": target, "url": url("/frames.html")})
    run.events(("Page.frameAttached", "Page.frameNavigated", "Page.frameDetached"), settle=0.2)
    run.call(
        "Runtime.evaluate",
        {"expression": f"const f = document.createElement('iframe'); f.src = '{url('/child.html')}'; "
         "document.body.append(f); 0"},
    )
    added = run.events(("Page.frameAttached", "Page.frameNavigated"), settle=0.5)
    run.call("Runtime.evaluate", {"expression": "f.remove(); 0"})
    removed = run.events(("Page.frameDetached",), settle=0.5)
    return [tree, content_off, enabled, content, *added, *removed]


def case_lifecycle(run: Run, url: Callable[[str], str]) -> list:
    """Each frame's lifecycle, in order, as a page with an iframe loads
    (the main frame's, then the iframe's), and what turning the events on
    again tells of the document shown."""
    run.call("Page.enable")
    run.call("Page.setLifecycleEventsEnabled", {"enabled": True})
    run.events(("Page.lifecycleEvent",), settle=0.2)
    navigated = run.call("Page.navigate", {"url": url("/life.html")})
    events = run.events(
        ("Page.lifecycleEvent", "Page.frameStartedLoading", "Page.frameStoppedLoading"),
        settle=2.0,
    )

    def by_frame(events: list[dict]) -> tuple[list[str], list[list[str]]]:
        frames: dict[str, list[str]] = {}
        for event in events:
            params = event["params"]
            name = params.get("name") or event["method"].removeprefix("Page.")
            if name not in PAINT_EVENTS:
                frames.setdefault(params["frameId"], []).append(name)
        main = frames.pop(run.target, [])
        return main, sorted(frames.values())

    main, children = by_frame(events)
    run.call("Page.setLifecycleEventsEnabled", {"enabled": True})
    again, _ = by_frame(run.events(("Page.lifecycleEvent",), settle=0.2))
    return [navigated, main, children, again]


def case_network(run: Run, url: Callable[[str], str]) -> list:
    """The requests of a page with a stylesheet, an image that isn't there
    and an iframe: their types, who asked, their responses, and the events
    of each, in order."""
    run.call("Network.enable")
    run.call("Page.navigate", {"url": url("/life.html")})
    events = run.events(
        (
            "Network.requestWillBeSent",
            "Network.responseReceived",
            "Network.dataReceived",
            "Network.loadingFinished",
            "Network.loadingFailed",
        ),
        settle=2.0,
    )
    requests: dict[str, dict] = {}
    for event in events:
        params = event["params"]
        request = requests.setdefault(params["requestId"], {"events": []})
        name = event["method"].removeprefix("Network.")
        if request["events"][-1:] != [name]:
            request["events"].append(name)
        if name == "requestWillBeSent":
            initiator = params.get("initiator", {})
            request.update(
                url=params["request"]["url"],
                method=params["request"]["method"],
                type=params.get("type"),
                documentURL=params.get("documentURL"),
                initiator={key: initiator.get(key) for key in ("type", "url", "lineNumber")},
            )
        elif name == "responseReceived":
            response = params["response"]
            request["response"] = {
                key: response.get(key) for key in ("status", "statusText", "mimeType", "protocol")
            }
    return sorted(
        (request for request in requests.values() if not request.get("url", "").endswith(CHROMES_OWN)),
        key=lambda request: request.get("url", ""),
    )


def case_log(run: Run, url: Callable[[str], str]) -> list:
    """A page's failed load as `Log` tells it, turned on after the load."""
    run.load(url("/life.html"))
    enabled = run.call("Log.enable")
    entries = run.events(("Log.entryAdded",), settle=0.5)
    return [
        enabled,
        [
            {key: entry["params"]["entry"].get(key) for key in ("source", "level", "text", "url")}
            for entry in entries
            if not entry["params"]["entry"].get("url", "").endswith(CHROMES_OWN)
        ],
    ]


CASES: dict[str, Callable[[Run, Callable[[str], str]], list]] = {
    "values": case_values,
    "properties": case_properties,
    "console": case_console,
    "contexts": case_contexts,
    "exceptions": case_exceptions,
    "navigation": case_navigation,
    "debugger": case_debugger,
    "page": case_page,
    "lifecycle": case_lifecycle,
    "network": case_network,
    "log": case_log,
}


def normalize(value, target: str, names: dict[tuple[str, str], str] | None = None):
    """`value` with what differs from run to run made placeholders: the
    `VOLATILE` keys' values (`<objectId 1>`, numbered as they first appear,
    keys in order), and the target's id (`<target>`); other frame ids
    `<frameId n>` (a `Page.Frame`'s `id` and `parentId`, and `parentFrameId`,
    too). A context's `id` is an `executionContextId`."""
    if names is None:
        names = {}

    def placeholder(key: str, raw) -> str:
        name = (key, json.dumps(raw))
        if name not in names:
            count = sum(1 for known in names if known[0] == key) + 1
            names[name] = f"<{key} {count}>"
        return names[name]

    if isinstance(value, dict):
        out = {}
        for key in sorted(value):
            item = value[key]
            if key == "context" and isinstance(item, dict) and "id" in item:
                context = normalize({k: v for k, v in item.items() if k != "id"}, target, names)
                out[key] = {"id": placeholder("executionContextId", item["id"]), **context}
            elif key == "frame" and isinstance(item, dict) and "id" in item:
                frame = normalize({k: v for k, v in item.items() if k != "id"}, target, names)
                out[key] = {"id": normalize({"frameId": item["id"]}, target, names)["frameId"], **frame}
            elif key in ("frameId", "parentId", "parentFrameId") and item == target:
                out[key] = "<target>"
            elif key in ("frameId", "parentId", "parentFrameId"):
                out[key] = placeholder("frameId", item)
            elif key in VOLATILE:
                out[key] = placeholder(key, item)
            else:
                out[key] = normalize(item, target, names)
        return out
    if isinstance(value, list):
        return [normalize(item, target, names) for item in value]
    return value


def differences(machopo, chrome, path: str = "") -> list[str]:
    """Where `machopo` and `chrome` differ, as JSON paths with both values."""
    if isinstance(machopo, dict) and isinstance(chrome, dict):
        found = []
        for key in sorted(set(machopo) | set(chrome)):
            at = f"{path}.{key}"
            if key not in machopo:
                found.append(f"{at}: machopo has none; Chrome: {json.dumps(chrome[key])[:200]}")
            elif key not in chrome:
                found.append(f"{at}: Chrome has none; machopo: {json.dumps(machopo[key])[:200]}")
            else:
                found.extend(differences(machopo[key], chrome[key], at))
        return found
    if isinstance(machopo, list) and isinstance(chrome, list):
        found = []
        for index in range(max(len(machopo), len(chrome))):
            at = f"{path}[{index}]"
            if index >= len(machopo):
                found.append(f"{at}: machopo has none; Chrome: {json.dumps(chrome[index])[:200]}")
            elif index >= len(chrome):
                found.append(f"{at}: Chrome has none; machopo: {json.dumps(machopo[index])[:200]}")
            else:
                found.extend(differences(machopo[index], chrome[index], at))
        return found
    if machopo != chrome:
        return [f"{path}: machopo {json.dumps(machopo)[:200]}; Chrome {json.dumps(chrome)[:200]}"]
    return []


def known(case: str, difference: str) -> str | None:
    """Why `difference` (in `case`) is known, if it is."""
    for (known_case, prefix), why in KNOWN.items():
        if known_case == case and difference.startswith(prefix):
            return why
    return None


def run_case(case: str, make: Callable[[], common.CdpPipe], url: Callable[[str], str]) -> list:
    """`case` on a browser `make` starts: its normalized transcript."""
    pipe = make()
    try:
        targets = [info for info in pipe.call("Target.getTargets")["targetInfos"] if info.get("type") == "page"]
        target = targets[0]["targetId"]
        session = pipe.call("Target.attachToTarget", {"targetId": target, "flatten": True})["sessionId"]
        run = Run(pipe, session, target)
        return normalize(CASES[case](run, url), target)
    finally:
        pipe.close()


def compare(machopo: Path, cases: list[str], log) -> int:
    """Runs `cases` (all if empty) on `machopo` and on Chrome; prints what
    differs. Non-zero if anything does, but what is known."""
    server = Pages(ROOT)
    chrome = common.chrome_binary()
    print(f"Chrome for Testing {common.CHROME_VERSION} ({chrome})")
    failed = 0
    for case in cases or list(CASES):
        machopo_transcript = run_case(case, lambda: common.MachopoPipe(machopo, log), server.url)
        chrome_transcript = run_case(
            case, lambda: common.ChromePipe(["--headless", "--no-first-run", "about:blank"]), server.url
        )
        found = differences(machopo_transcript, chrome_transcript)
        unknown = [difference for difference in found if known(case, difference) is None]
        print(f"{case}: {'same' if not found else f'{len(found)} differences'}"
              + (f" ({len(found) - len(unknown)} known)" if len(unknown) != len(found) else ""))
        for difference in found:
            why = known(case, difference)
            print(f"  {'known: ' if why else ''}{difference}" + (f"  [{why}]" if why else ""))
        if unknown:
            failed += 1
    return 1 if failed else 0
