"""`test.py cdp`: machopo's answers to DevTools' domains (V8's, and the
renderer's `Page`, `Network`, `Log`, `DOM`, `CSS` and `Overlay`) against Chrome's. Each case
sends the same commands to machopo and to Chrome for Testing (the version
`bench.py` pins, whose V8 is machopo's), on the same pages, and keeps the
answers and the events it names; the values that differ from run to run
(object, script, context, exception, node and style sheet ids, times, the
target's id)
become placeholders, numbered as they first appear, so that which ones
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
    "/dom.html": (
        "<!DOCTYPE html><title>DOM</title><body>\n<div id=a class='x y'>text</div>\n<p></p>\n"
        "<template><b>t</b></template><iframe src='/child.html'></iframe>\n"
        "<svg><circle r=1></circle></svg></body>"
    ),
    "/listeners.html": (
        "<!DOCTYPE html><body><button id=b class='x y'>x</button><p>Hello there</p>\n<script>\n"
        "b.addEventListener('click', function f() {}, true);\n"
        "b.addEventListener('mouseover', { handleEvent() {} });\n"
        "document.body.addEventListener('keydown', () => {}, { passive: true, once: true });\n"
        "window.addEventListener('load', function w() {});\n</script>"
    ),
    "/boxes.html": (
        "<!DOCTYPE html><style>html, body { margin: 0 } "
        "#a { position: absolute; left: 10px; top: 20px; width: 100px; height: 50px; "
        "padding: 5px; border: 2px solid black; margin: 3px } "
        "#t { position: absolute; left: 200px; top: 200px; width: 100px; height: 100px; "
        "transform: rotate(90deg) }</style>"
        "<div id=a class='x y x'>a</div><div id=t></div><p style='margin-top: 400px'>x</p>"
        "<div id=s style='overflow: auto; height: 20px'><div style='height: 100px'></div></div>"
        "<div id=n style='overflow: auto; height: 200px'><div style='height: 100px'></div></div>"
    ),
    "/css.html": (
        "<!DOCTYPE html><style>html, body { margin: 0; font-family: Times } div { color: green } "
        ".a, p, div.a { color: blue; margin: 1px 2px } #x { color: red !important; padding: 3px; "
        "width: 100px; line-height: 20px } "
        "div::before { content: 'b' } span { color: gray }</style>"
        "<div id=x class=a style='color: pink; border-top: solid'>text <b>bold</b></div>"
        "<font id=f color=red>f</font><div id=e></div>"
    ),
    "/sheets.html": (
        "<!DOCTYPE html><link rel=stylesheet href='/style.css'>\n<style media=screen\n title=t>\n"
        ".a { color: red }\n</style><p class=a>p</p><div id=host></div>\n<script>\n"
        "const s = new CSSStyleSheet(); s.replaceSync('.c { color: blue }'); document.adoptedStyleSheets = [s];\n"
        "host.attachShadow({mode: 'open'}).innerHTML = '<style>.d { }</style>';\n</script>"
    ),
    "/source.html": (
        "<!DOCTYPE html><style>\n/* top */\n.a ,\n p.b > i { color: red !important; margin: 1px 2px; foo: bar; "
        "color: blu; /* gap: 1px; */ --x: 1 }\n@media (min-width: 1px) {\n  @supports (display: grid) {\n"
        "    .a { padding-left: 3px }\n  }\n}\n@media print { .a { top: 0 } }\n.a{background:var(--x)}\n"
        "</style><style media=\"screen\">.a { left: 0 }</style>\n"
        "<div class=a style='color: pink;  zz: 1; border-top: solid'>a</div><div id=e></div>"
    ),
    "/pseudo.html": (
        "<!DOCTYPE html><style>\nbody { color: red; margin: 0; font-family: Times }\n.p { font-size: 20px; padding: 1px }\n"
        ".p::selection { color: navy }\n.x::before { content: 'b'; color: blue }\n.x::after { content: 'a' }\n"
        "li::marker { color: green }\n.x::selection { color: pink }\n"
        "@keyframes k { from { opacity: 0 } to { opacity: 1 } }\n"
        ".x { animation: k 1000s linear; transition: left 1000s; left: 0; position: relative }\n"
        ".x.gone::after { content: none }\n#li.n::before { content: 'n' }\n"
        "</style><div class=p style='letter-spacing: 1px'><div class=x>x</div><ul><li id=li>l</li></ul></div>"
    ),
    "/force.html": (
        "<!DOCTYPE html><style>#x:hover { color: rgb(0, 128, 0) } .p:hover #x { background-color: red }"
        " #x:focus { margin-left: 3px } #x:focus-within { margin-top: 3px } .p:focus-within { margin-left: 4px }"
        " .p:has(#x:hover) { margin-right: 8px } #x:target { padding-left: 5px } #x:active { padding-top: 1px }"
        " #i:disabled { margin-left: 6px } #i:enabled { margin-top: 2px } #i:read-only { text-indent: 7px }"
        " #r:required { margin-left: 9px } #r:optional { margin-top: 9px } #c:checked { margin-left: 2px }"
        " details:open { margin-left: 3px }</style>"
        "<div class=p><div id=x tabindex=0>x</div><input id=i><input id=r><input id=c type=checkbox>"
        "<details><summary>s</summary>d</details></div>"
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
    "searchId",
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
    "styleSheetId",
}

# The keys whose values are a node's number in a session's `DOM`, and lists
# of them: one placeholder for each number, whatever the key (a node's
# `nodeId` is another's `parentId`); 0 (none) stays.
NODE_IDS = {"nodeId", "parentId", "parentNodeId", "previousNodeId", "hostId"}

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
    ("dom_objects", "[1].result.result.className"): "machopo has no HTMLDocument: an HTML document's object is a "
    "Document",
    ("overlay", "[6].result.highlight.elementInfo.accessibleName"): "machopo has no accessibility tree: the "
    "overlay's label has no accessibility info",
    ("overlay", "[6].result.highlight.elementInfo.accessibleRole"): "the same",
    ("overlay", "[6].result.highlight.elementInfo.isKeyboardFocusable"): "the same",
    ("overlay", "[6].result.highlight.elementInfo.layoutObjectName"): "Blink's layout object's class "
    "(LayoutBlockFlow...), which machopo's boxes have no names like",
    ("overlay", "[9].result.highlight.elementInfo.accessibleName"): "the same as [6]'s",
    ("overlay", "[9].result.highlight.elementInfo.accessibleRole"): "the same as [6]'s",
    ("overlay", "[9].result.highlight.elementInfo.isKeyboardFocusable"): "the same as [6]'s",
    ("overlay", "[9].result.highlight.elementInfo.layoutObjectName"): "the same as [6]'s",
    ("overlay", "[19].error.data"): "Chrome's message has where in its CBOR the field is missing",
    ("css_force", "[24].margin-left"): "Chrome counts an element's ancestors' forced `:focus-within` as it is forced "
    "and unforced, not as the element leaves its tree: they match it after the element went; machopo's don't",
    ("css_force", "[26]"): "Chrome's `DOM.disable` forgets the forced pseudo-classes without styling the elements "
    "again: a style worked out with them stays until something else restyles the element; machopo restyles it",
    ("css_force", "[28].error.data"): "Chrome's message has where in its CBOR the field is wrong",
    ("css_source", "[0][0].selectorList.selectors[0].specificity.components"): "Chrome 152 breaks a selector's "
    "specificity down by its simple selectors, which DevTools' specificity tooltip reads; machopo doesn't",
    **{
        ("css_source", f"{rule}.selectorList.selectors[{selector}].specificity.components"): "the same"
        for rule, selector in [
            ("[0][0]", 1), ("[0][1]", 0), ("[0][2]", 0), ("[0][3]", 0),
            ("[4][0]", 0), ("[4][0]", 1), ("[4][1]", 0), ("[4][2]", 0), ("[4][3]", 0),
        ]
    },
    ("css_source", "[0][1].media[0].mediaList[0]"): "machopo doesn't break media queries down (only the device "
    "toolbar's media query bar reads them, and machopo has no device emulation)",
    ("css_source", "[4][1].media[0].mediaList[0]"): "the same",
    ("css_source", "[4][4]."): "a rule of a sheet script edited: machopo's text of the sheet is its rules as "
    "CSSOM writes them, Chrome's the text it had with the new rule written into it",
    ("css_source", "[5][1]"): "Chrome tells an inline style changed twice (once for the attribute, once for the "
    "style), machopo once",
    ("css_pseudo", "[8]"): "an inline box's height differs from Chrome's (a `<span>` of 20px Times: machopo 20, "
    "Chrome 23)",
    ("css_pseudo", "[9].nodeHeight"): "the same",
    ("css_sheets", "[1][1].end"): "a `<style>` innerHTML made: Chrome's place is where the document's parser was "
    "as the script ran (after `</script>`); machopo's, none",
    ("css_sheets", "[1][1].start"): "the same",
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


# The events of `DOM` the `dom` case keeps.
DOM_EVENTS = (
    "DOM.setChildNodes",
    "DOM.childNodeInserted",
    "DOM.childNodeRemoved",
    "DOM.childNodeCountUpdated",
    "DOM.attributeModified",
    "DOM.attributeRemoved",
    "DOM.characterDataModified",
    "DOM.inlineStyleInvalidated",
    "DOM.shadowRootPushed",
    "DOM.documentUpdated",
)


def case_dom(run: Run, url: Callable[[str], str]) -> list:
    """A page's document as `DOM` sends it (two levels, the body's
    children asked for), script's changes as it tells them, and queries."""
    run.load(url("/dom.html"))
    transcript = [run.call("DOM.enable")]
    document = run.call("DOM.getDocument")
    transcript.append(document)
    root = document["result"]["root"]
    html = next(node for node in root["children"] if node["nodeName"] == "HTML")
    body = next(node for node in html["children"] if node["nodeName"] == "BODY")["nodeId"]
    transcript.append(run.call("DOM.requestChildNodes", {"nodeId": body}))
    transcript.append(run.events(DOM_EVENTS, settle=0.2))
    # One change a task: machopo tells a task's changes once it is through,
    # as the DOM is then, where Chrome tells each as it happens (a node put
    # in and taken out in one task, Chrome tells twice and machopo not at
    # all); `shell`'s `devtools_dom` tests the batches.
    for expression in [
        "document.body.append(document.createElement('span'), ' ')",
        "document.getElementById('a').setAttribute('title', 'hi')",
        "document.getElementById('a').removeAttribute('class')",
        "document.getElementById('a').firstChild.data = 'changed'",
        "document.querySelector('p').append('x')",
        "document.getElementById('a').style.color = 'red'",
        "window.root = document.querySelector('p').attachShadow({mode: 'open'})",
        "root.append('in')",
        "document.querySelector('span').remove()",
    ]:
        run.call("Runtime.evaluate", {"expression": expression})
        transcript.append(run.events(DOM_EVENTS, settle=0.3))
    found = run.call("DOM.querySelector", {"nodeId": root["nodeId"], "selector": "circle"})
    transcript.append(found)
    transcript.append(run.events(DOM_EVENTS, settle=0.2))
    circle = found["result"]["nodeId"]
    transcript.append(run.call("DOM.describeNode", {"nodeId": circle}))
    transcript.append(run.call("DOM.getAttributes", {"nodeId": circle}))
    transcript.append(run.call("DOM.getOuterHTML", {"nodeId": body}))
    transcript.append(run.call("DOM.querySelectorAll", {"nodeId": root["nodeId"], "selector": "div, b, p"}))
    transcript.append(run.call("DOM.collectClassNamesFromSubtree", {"nodeId": root["nodeId"]}))
    transcript.append(run.call("DOM.requestChildNodes", {"nodeId": body, "depth": 0}))
    transcript.append(run.call("DOM.querySelector", {"nodeId": 999999, "selector": "p"}))
    return transcript


def case_dom_objects(run: Run, url: Callable[[str], str]) -> list:
    """Nodes and script's objects for them: the console's objects of
    nodes, an object's node, a node's object, `$0`, a node no document
    has; the listeners of a node, of those below it, of the window; the
    Elements panel's search."""
    run.load(url("/listeners.html"))
    run.call("DOM.enable")
    transcript = []

    def evaluate(expression: str, **more) -> dict:
        return run.call("Runtime.evaluate", {"expression": expression, "objectGroup": "g", **more})

    for expression in ["b", "document", "document.doctype", "document.body.lastChild.previousSibling", "({})"]:
        transcript.append(evaluate(expression))
    root = run.call("DOM.getDocument")["result"]["root"]
    button = evaluate("b")["result"]["result"]
    requested = run.call("DOM.requestNode", {"objectId": button["objectId"]})
    transcript.append(requested)
    transcript.append(run.events(DOM_EVENTS, settle=0.2))
    node = requested["result"]["nodeId"]
    transcript.append(run.call("DOM.resolveNode", {"nodeId": node, "objectGroup": "g"}))
    transcript.append(run.call("DOM.resolveNode", {}))
    transcript.append(run.call("DOM.requestNode", {"objectId": evaluate("({})")["result"]["result"]["objectId"]}))
    transcript.append(run.call("DOM.setInspectedNode", {"nodeId": node}))
    transcript.append(evaluate("$0 === b", includeCommandLineAPI=True, returnByValue=True))
    loose = evaluate("window.loose = document.createElement('section'); loose.append(document.createElement('i')); loose.firstChild")
    transcript.append(run.call("DOM.requestNode", {"objectId": loose["result"]["result"]["objectId"]}))
    transcript.append(run.events(DOM_EVENTS, settle=0.2))
    for expression, params in [("b", {}), ("document.body", {"depth": -1}), ("window", {})]:
        target = evaluate(expression)["result"]["result"]
        transcript.append(run.call("DOMDebugger.getEventListeners", {"objectId": target["objectId"], **params}))
    for query in ["hello", ".x", "<p>", "button"]:
        found = run.call("DOM.performSearch", {"query": query})
        transcript.append(found)
        search = found["result"]["searchId"]
        count = found["result"]["resultCount"]
        transcript.append(run.call("DOM.getSearchResults", {"searchId": search, "fromIndex": 0, "toIndex": count}))
        transcript.append(run.events(DOM_EVENTS, settle=0.2))
        transcript.append(run.call("DOM.discardSearchResults", {"searchId": search}))
    del root
    return transcript


def case_overlay(run: Run, url: Callable[[str], str]) -> list:
    """Nodes' box models (absolutely positioned, transformed),
    content quads, the nodes at points, and the highlight objects Blink
    builds for the overlay; what `Overlay` answers without `DOM`, and to
    a mode it doesn't have."""
    run.load(url("/boxes.html"))
    transcript = [run.call("Overlay.enable")]
    run.call("DOM.enable")
    transcript.append(run.call("Overlay.enable"))
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]
    for selector in ["#s", "#n"]:
        node = run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]
        transcript.append(run.call("DOM.describeNode", {"nodeId": node}))
    for selector in ["#a", "#t", "head"]:
        node = run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]
        transcript.append(run.call("DOM.getBoxModel", {"nodeId": node}))
        transcript.append(run.call("DOM.getContentQuads", {"nodeId": node}))
        transcript.append(run.call("Overlay.getHighlightObjectForTest", {"nodeId": node}))
    # The viewports differ (Chrome's headless one is shorter): a point outside
    # both, none inside one only.
    for x, y in [(50, 50), (22, 33), (5, 5), (5, 2000)]:
        transcript.append(run.call("DOM.getNodeForLocation", {"x": x, "y": y}))
    transcript.append(run.events(DOM_EVENTS, settle=0.2))
    transcript.append(run.call("Overlay.setInspectMode", {"mode": "lasso"}))
    transcript.append(run.call("Overlay.highlightNode", {"nodeId": root}))
    transcript.append(run.call("Overlay.setShowGridOverlays", {"gridNodeHighlightConfigs": []}))
    return transcript


# The computed values `css` compares: machopo lists the properties it has,
# Chrome every one it has.
COMPUTED = (
    "color", "display", "width", "height", "margin-top", "margin-left", "padding-top",
    "border-top-style", "border-top-width", "font-size", "background-color", "position",
)


def css_style(style: dict) -> dict:
    """A `CSS.CSSStyle` as both have it: its longhands' values and
    priorities, its shorthands' (Chrome lists a rule's declarations as
    written too, from its text, which machopo doesn't keep)."""
    return {
        "longhands": {
            p["name"]: [p["value"], p.get("important", False)]
            for p in style["cssProperties"]
            if "longhandProperties" not in p
        },
        "shorthands": {e["name"]: [e["value"], e.get("important", False)] for e in style["shorthandEntries"]},
    }


def matched_styles(answer: dict) -> dict:
    """A `getMatchedStylesForNode` answer as both have it: its keys; its
    author rules' selectors, their specificity, those that match and the
    rules' declarations (`css_style`); the inline style's declarations;
    the presentational hints (neither has their text) and the layout
    parent; the first UA rule that sets `display: block`, in full (the UA
    sheets differ, but neither has their text)."""
    result = answer.get("result")
    if result is None:
        return answer
    rules = result["matchedCSSRules"]
    ua_block = next(
        (
            match["rule"]["style"]
            for match in rules
            if match["rule"]["origin"] == "user-agent"
            and any(p == {"name": "display", "value": "block"} for p in match["rule"]["style"]["cssProperties"])
        ),
        None,
    )
    return {
        "keys": sorted(result),
        "rules": [
            {
                "selectors": match["rule"]["selectorList"]["text"],
                "specificity": [
                    [s["specificity"]["a"], s["specificity"]["b"], s["specificity"]["c"]]
                    for s in match["rule"]["selectorList"]["selectors"]
                ],
                "matching": match["matchingSelectors"],
                "style": css_style(match["rule"]["style"]),
            }
            for match in rules
            if match["rule"]["origin"] == "regular"
        ],
        "inline": css_style(result["inlineStyle"]) if "inlineStyle" in result else None,
        "attributes": result.get("attributesStyle"),
        "parentLayoutNodeId": result.get("parentLayoutNodeId"),
        "ua display: block": ua_block,
    }


def case_css(run: Run, url: Callable[[str], str]) -> list:
    """What Styles and Computed read of an element: its computed values,
    the rules it matches, its inline style and presentational hints, the
    fonts of its text, its computed style followed as script changes it,
    the overlay label's styles; what `CSS` answers without `DOM`, of a
    text node, of a node no session has."""
    run.load(url("/css.html"))
    transcript = [run.call("CSS.enable")]
    run.call("DOM.enable")
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]
    transcript.append(run.call("CSS.getComputedStyleForNode", {"nodeId": root}))
    transcript.append(run.call("CSS.enable"))

    def node(selector: str) -> int:
        return run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]

    x, font, empty = node("#x"), node("#f"), node("#e")
    described = run.call("DOM.describeNode", {"nodeId": x, "depth": 1})["result"]["node"]
    backend = described["children"][0]["backendNodeId"]
    pushed = run.call("DOM.pushNodesByBackendIdsToFrontend", {"backendNodeIds": [backend]})
    text = pushed["result"]["nodeIds"][0]
    for element in [x, font, empty]:
        transcript.append(matched_styles(run.call("CSS.getMatchedStylesForNode", {"nodeId": element})))
        inline = run.call("CSS.getInlineStylesForNode", {"nodeId": element})["result"]
        transcript.append({key: css_style(value) for key, value in inline.items()})
    for target in [x, text]:
        computed = run.call("CSS.getComputedStyleForNode", {"nodeId": target})
        values = {entry["name"]: entry["value"] for entry in computed["result"]["computedStyle"]}
        transcript.append({name: values.get(name) for name in COMPUTED})
        transcript.append(computed["result"].get("extraFields"))
    for target in [x, text, empty]:
        fonts = run.call("CSS.getPlatformFontsForNode", {"nodeId": target})
        # In no order (Chrome's is its hash table's): by glyphs.
        transcript.append(sorted(fonts["result"]["fonts"], key=lambda font: font["glyphCount"]))
    transcript.append(run.call("CSS.getMatchedStylesForNode", {"nodeId": text}))
    transcript.append(run.call("CSS.getMatchedStylesForNode", {"nodeId": 999999}))
    transcript.append(run.call("CSS.trackComputedStyleUpdatesForNode", {"nodeId": x}))
    run.call("Runtime.evaluate", {"expression": "x.style.paddingLeft = '7px'"})
    transcript.append(run.events(("CSS.computedStyleUpdated",), settle=0.5))
    run.call("Runtime.evaluate", {"expression": "document.body.append('-')"})
    transcript.append(run.events(("CSS.computedStyleUpdated",), settle=0.5))
    transcript.append(run.call("CSS.trackComputedStyleUpdatesForNode", {}))
    run.call("Runtime.evaluate", {"expression": "x.style.paddingLeft = '8px'"})
    transcript.append(run.events(("CSS.computedStyleUpdated",), settle=0.5))
    for target, color_format in [(x, "hex"), (empty, "hex"), (x, "rgb")]:
        highlight = run.call(
            "Overlay.getHighlightObjectForTest", {"nodeId": target, "includeStyle": True, "colorFormat": color_format}
        )
        transcript.append(highlight["result"]["highlight"]["elementInfo"].get("style"))
    run.events(DOM_EVENTS, settle=0.2)
    return transcript


def case_css_sheets(run: Run, url: Callable[[str], str]) -> list:
    """The sheets `CSS` tells of: a `<link>`'s, a `<style>`'s (its place in
    the markup), an adopted one's, a shadow root's; their texts; a
    `<style>`'s text changed and a `<link>` taken out; a rule's sheet."""
    run.load(url("/sheets.html"))
    run.call("DOM.enable")
    transcript = [run.call("CSS.enable")]
    # In an order of their own (the two send them in another).
    headers = sorted(
        (event["params"]["header"] for event in run.events(("CSS.styleSheetAdded",), settle=0.5)),
        key=lambda header: (header["isConstructed"], header["isInline"], header["title"], header["length"]),
    )
    transcript.append(headers)
    for header in headers:
        transcript.append(run.call("CSS.getStyleSheetText", {"styleSheetId": header["styleSheetId"]}))
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]
    p = run.call("DOM.querySelector", {"nodeId": root, "selector": "p"})["result"]["nodeId"]
    matched = run.call("CSS.getMatchedStylesForNode", {"nodeId": p})["result"]
    transcript.append([
        {"selector": rule["rule"]["selectorList"]["text"], "styleSheetId": rule["rule"].get("styleSheetId")}
        for rule in matched["matchedCSSRules"]
        if rule["rule"]["origin"] == "regular"
    ])
    sheet_events = ("CSS.styleSheetAdded", "CSS.styleSheetRemoved", "CSS.styleSheetChanged")
    for expression in [
        "document.querySelector('style').textContent = '.b { }'",
        "document.querySelector('link').remove()",
    ]:
        run.call("Runtime.evaluate", {"expression": expression})
        transcript.append(run.events(sheet_events, settle=0.5))
    transcript.append(run.call("CSS.getStyleSheetText", {"styleSheetId": "nothing"}))
    run.events(DOM_EVENTS, settle=0.2)
    return transcript


# What a `CSS.CSSRule` of Chrome's has that machopo's never does (none of
# these rules has any): left out of `css_source`.
RULE_KEYS_MACHOPO_HAS_NONE_OF = (
    "containerQueries", "layers", "navigations", "scopes", "startingStyles", "originTreeScopeNodeId",
)


def case_css_source(run: Run, url: Callable[[str], str]) -> list:
    """Where the rules an element matches are in their sheets' texts: the
    selectors, the block, each declaration as written (`!important`, ones
    that don't parse, one in a comment) then as parsed, the `@media` and
    `@supports` they are in (a sheet's own media too); an inline style's
    the same, as a sheet of its own, with its text; once script edits a
    sheet's rules and an inline style, what is told and the rules again."""
    run.load(url("/source.html"))
    run.call("DOM.enable")
    run.call("CSS.enable")
    run.events(("CSS.styleSheetAdded",), settle=0.5)
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]

    def node(selector: str) -> int:
        return run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]

    def rules(element: int) -> list:
        matched = run.call("CSS.getMatchedStylesForNode", {"nodeId": element})["result"]
        return [
            {key: value for key, value in rule["rule"].items() if key not in RULE_KEYS_MACHOPO_HAS_NONE_OF}
            for rule in matched["matchedCSSRules"]
            if rule["rule"]["origin"] == "regular"
        ] + [matched["inlineStyle"]]

    a, e = node(".a"), node("#e")
    transcript = [rules(a), rules(e)]
    inline = run.call("CSS.getInlineStylesForNode", {"nodeId": a})["result"]["inlineStyle"]
    transcript.append(run.call("CSS.getStyleSheetText", {"styleSheetId": inline["styleSheetId"]}))
    sheet_events = ("CSS.styleSheetAdded", "CSS.styleSheetRemoved", "CSS.styleSheetChanged")
    run.call("Runtime.evaluate", {"expression": "document.styleSheets[1].insertRule('.a { right: 0 }', 0)"})
    transcript.append(run.events(sheet_events, settle=0.5))
    transcript.append(rules(a)[:-1])
    run.call("Runtime.evaluate", {"expression": "document.querySelector('.a').style.color = 'blue'"})
    transcript.append(run.events(sheet_events, settle=0.5))
    transcript.append(rules(a)[-1])
    run.events(DOM_EVENTS, settle=0.2)
    return transcript


def case_css_pseudo(run: Run, url: Callable[[str], str]) -> list:
    """What Styles shows of an element beyond its own rules: its
    pseudo-elements' rules (those it can have: a list item's marker), its
    ancestors' rules and inline styles, and their highlight
    pseudo-elements' rules; the author's rules of each (the UA sheets
    differ). The pseudo-elements as nodes: an element's, a `::before`'s
    styles, Computed, box and label; those that come and go."""
    run.load(url("/pseudo.html"))
    run.call("DOM.enable")
    run.call("CSS.enable")
    run.events(("CSS.styleSheetAdded",), settle=0.5)
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]

    def node(selector: str) -> int:
        return run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]

    def rules(matches: list) -> list:
        # The author's (the UA sheets differ), and how many of the UA's
        # there are for a pseudo-element (whether it has any).
        return [
            {
                "selectors": match["rule"]["selectorList"]["text"],
                "matching": match["matchingSelectors"],
                "styleSheetId": match["rule"].get("styleSheetId"),
            }
            for match in matches
            if match["rule"]["origin"] == "regular"
        ]

    transcript = []
    for selector in [".x", "#li"]:
        matched = run.call("CSS.getMatchedStylesForNode", {"nodeId": node(selector)})["result"]
        transcript.append({
            "pseudoElements": [
                {"pseudoType": p["pseudoType"], "matches": rules(p["matches"])} for p in matched["pseudoElements"]
            ],
            "inherited": [
                {
                    "rules": rules(i["matchedCSSRules"]),
                    "inline": i.get("inlineStyle", {}).get("cssText"),
                }
                for i in matched["inherited"]
            ],
            "inheritedPseudoElements": [
                [{"pseudoType": p["pseudoType"], "matches": rules(p["matches"])} for p in i["pseudoElements"]]
                for i in matched["inheritedPseudoElements"]
            ],
        })
    # The pseudo-elements as nodes: an element's, a `::before`'s styles,
    # box and label; those that come and go.
    x, li = node(".x"), node("#li")
    described = run.call("DOM.describeNode", {"nodeId": x})["result"]["node"]
    transcript.append(described.get("pseudoElements"))
    transcript.append(run.call("DOM.describeNode", {"nodeId": li})["result"]["node"].get("pseudoElements"))
    before = described["pseudoElements"][0]["nodeId"]
    matched = run.call("CSS.getMatchedStylesForNode", {"nodeId": before})["result"]
    transcript.append({
        "keys": sorted(matched),
        "rules": rules(matched["matchedCSSRules"]),
        "inherited": [
            {"rules": rules(i["matchedCSSRules"]), "inline": i.get("inlineStyle", {}).get("cssText")}
            for i in matched["inherited"]
        ],
        "inheritedPseudoElements": [len(i["pseudoElements"]) for i in matched["inheritedPseudoElements"]],
        "parentLayoutNodeId": matched.get("parentLayoutNodeId") == x,
    })
    transcript.append(run.call("CSS.getInlineStylesForNode", {"nodeId": before}))
    computed = run.call("CSS.getComputedStyleForNode", {"nodeId": before})["result"]["computedStyle"]
    transcript.append([entry for entry in computed if entry["name"] in ("content", "color", "display")])
    transcript.append(run.call("DOM.describeNode", {"nodeId": before}))
    transcript.append(run.call("DOM.getBoxModel", {"nodeId": before}).get("result", {}).get("model", {}).get("height"))
    info = run.call("Overlay.getHighlightObjectForTest", {"nodeId": before})["result"]["highlight"]["elementInfo"]
    transcript.append({key: info.get(key) for key in ("tagName", "idValue", "className", "nodeHeight")})
    # The `@keyframes` its animations run, and its animations' values (by
    # name and property: the values move with the time).
    transcript.append(run.call("CSS.getMatchedStylesForNode", {"nodeId": x})["result"]["cssKeyframesRules"])
    # A transition started (two frames on: without, whether one ran
    # between the two commands decides whether it has).
    run.call("Runtime.evaluate", {
        "expression": "document.querySelector('.x').style.left = '5px';"
        " new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
        "awaitPromise": True,
    })
    animated = run.call("CSS.getAnimatedStylesForNode", {"nodeId": x})["result"]

    def animated_style(entry: dict) -> list:
        return [property["name"] for property in entry["cssProperties"]]

    def animated_styles(styles: dict) -> dict:
        return {
            "animations": [
                {"name": entry.get("name"), "properties": animated_style(entry["style"])}
                for entry in styles["animationStyles"]
            ],
            "transitions": animated_style(styles["transitionsStyle"]) if "transitionsStyle" in styles else None,
        }

    transcript.append({
        "own": animated_styles(animated),
        "inherited": [animated_styles(entry) for entry in animated["inherited"]],
    })
    pseudo_events = ("DOM.pseudoElementAdded", "DOM.pseudoElementRemoved")
    run.call("Runtime.evaluate", {"expression": "document.querySelector('.x').classList.add('gone')"})
    transcript.append(run.events(pseudo_events, settle=0.5))
    run.call("Runtime.evaluate", {"expression": "li.classList.add('n')"})
    transcript.append(run.events(pseudo_events, settle=0.5))
    run.events(DOM_EVENTS, settle=0.2)
    return transcript


def case_css_force(run: Run, url: Callable[[str], str]) -> list:
    """Styles' `:hov`: the pseudo-classes forced on an element, in its
    rules and computed values, for script; its ancestors' `:focus-within`
    and `:has()`, not their `:hover`; the form states and their opposites;
    each call in place of the one before; unknown names passed over; those
    of an element that left its tree, or of a session that turned `CSS` or
    `DOM` off, gone; the errors."""
    run.load(url("/force.html"))
    run.call("DOM.enable")
    run.call("CSS.enable")
    run.events(("CSS.styleSheetAdded",), settle=0.5)
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]

    def node(selector: str) -> int:
        return run.call("DOM.querySelector", {"nodeId": root, "selector": selector})["result"]["nodeId"]

    def force(node: int, classes) -> dict:
        return run.call("CSS.forcePseudoState", {"nodeId": node, "forcedPseudoClasses": classes})

    names = ("color", "background-color", "margin-left", "margin-top", "margin-right", "padding-left",
             "padding-top", "text-indent")

    def computed(node: int) -> dict:
        style = run.call("CSS.getComputedStyleForNode", {"nodeId": node})["result"]["computedStyle"]
        return {entry["name"]: entry["value"] for entry in style if entry["name"] in names}

    def script(expression: str):
        return run.call("Runtime.evaluate", {"expression": expression, "returnByValue": True})["result"]["result"].get("value")

    x, p = node("#x"), node(".p")
    transcript = [force(x, ["hover", "focus", "target", "nope", "link", "default"]), computed(x), computed(p)]
    matched = run.call("CSS.getMatchedStylesForNode", {"nodeId": x})["result"]["matchedCSSRules"]
    transcript.append([
        (match["rule"]["selectorList"]["text"], match["matchingSelectors"])
        for match in matched if match["rule"]["origin"] == "regular"
    ])
    transcript.append(script("[x.matches(':hover'), x.matches(':focus-within'), getComputedStyle(x).color]"))
    transcript += [force(x, ["active", "focus-within", "focus-visible"]), computed(x), computed(p)]
    transcript += [force(x, []), computed(x), computed(p)]
    for selector, classes in [("#i", ["disabled"]), ("#i", ["enabled", "read-write"]), ("#r", ["required"]),
                              ("#r", ["optional"]), ("#c", ["checked"]), ("details", ["open"])]:
        transcript += [force(node(selector), classes), computed(node(selector))]
    # Gone with the element's leaving its tree.
    force(x, ["hover", "focus"])
    script("document.body.append(x)")
    x = node("#x")
    transcript += [computed(x), computed(p)]
    force(x, ["hover"])
    run.call("CSS.disable")
    run.call("CSS.enable")
    run.events(("CSS.styleSheetAdded",), settle=0.5)
    transcript.append(computed(x))
    force(x, ["hover"])
    run.call("DOM.disable")
    transcript.append(script("getComputedStyle(x).color"))
    run.call("DOM.enable")
    root = run.call("DOM.getDocument")["result"]["root"]["nodeId"]
    transcript.append(force(root, ["hover"]))
    transcript.append(force(node("#x"), "hover"))
    run.call("CSS.disable")
    transcript.append(force(node("#x"), ["hover"]))
    run.events(DOM_EVENTS, settle=0.2)
    return transcript


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
    "dom": case_dom,
    "dom_objects": case_dom_objects,
    "overlay": case_overlay,
    "css": case_css,
    "css_sheets": case_css_sheets,
    "css_source": case_css_source,
    "css_pseudo": case_css_pseudo,
    "css_force": case_css_force,
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
            elif key in NODE_IDS and isinstance(item, int):
                out[key] = item if item == 0 else placeholder("nodeId", item)
            elif key == "nodeIds" and isinstance(item, list):
                out[key] = [id if id == 0 else placeholder("nodeId", id) for id in item]
            elif key in ("backendNodeId", "ownerNode") and isinstance(item, int):
                out[key] = placeholder("backendNodeId", item)
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
