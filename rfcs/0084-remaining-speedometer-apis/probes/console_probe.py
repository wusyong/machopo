import os, sys, tempfile, time, json, queue
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
port = serve.start()
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "about:blank"])
try:
    session = chrome.navigate(f"http://127.0.0.1:{port}/blank.html", front=True); time.sleep(1)
    chrome.call("Log.enable", {}, session)
    chrome.call("Runtime.enable", {}, session)
    for expr in sys.argv[1:]:
        chrome.call("Runtime.evaluate", {"expression": expr}, session)
        time.sleep(0.3)
        print("==", expr)
        while True:
            try:
                m = chrome.events.get(timeout=0.3)
            except queue.Empty:
                break
            if m and m.get("method") == "Log.entryAdded":
                e = m["params"]["entry"]; print("  log", e["level"], e["source"], e["text"])
            elif m and m.get("method") == "Runtime.consoleAPICalled":
                print("  console", m["params"]["type"], [a.get("value") for a in m["params"]["args"]])
finally:
    chrome.close()
