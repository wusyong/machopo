import os, sys, tempfile, time, json, base64
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
port = serve.start()
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "about:blank"])
try:
    chrome.call("Tracing.start", {"traceConfig": {"includedCategories": ["blink.user_timing", "loading", "rail"]}, "transferMode": "ReturnAsStream", "streamFormat": "json"})
    session = chrome.navigate(f"http://127.0.0.1:{port}/nav.html", front=True); time.sleep(2)
    chrome.call("Runtime.evaluate", {"expression": "performance.mark('page-mark')"}, session)
    time.sleep(0.5)
    chrome.call("Tracing.end")
    done = chrome.event("Tracing.tracingComplete", timeout=60)
    data = b""
    while True:
        chunk = chrome.call("IO.read", {"handle": done["stream"], "size": 1 << 20})
        d = chunk.get("data", "")
        data += base64.b64decode(d) if chunk.get("base64Encoded") else d.encode()
        if chunk.get("eof"): break
    events = json.loads(data)["traceEvents"] if data.strip().startswith(b"{") else json.loads(data)
    for e in sorted(events, key=lambda e: e.get("ts", 0)):
        if "blink.user_timing" in e.get("cat", ""):
            print(e.get("cat"), e.get("ph"), e.get("name"), json.dumps(e.get("args", {}))[:150])
finally:
    chrome.close()
