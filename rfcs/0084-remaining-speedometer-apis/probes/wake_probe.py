import sys, tempfile, time, json, os
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench")); sys.path.insert(0, HERE)
import common, serve
port = serve.start()
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "about:blank"])
try:
    s = chrome.navigate(f"http://127.0.0.1:{port}/wake.html", front=True); time.sleep(1)
    print(chrome.call("Runtime.evaluate", {"expression": "run()", "returnByValue": True, "awaitPromise": True}, s)["result"].get("value"))
    t = chrome.call("Target.createTarget", {"url": "about:blank"})["targetId"]
    s2 = chrome.call("Target.attachToTarget", {"targetId": t, "flatten": True})["sessionId"]
    chrome.call("Page.bringToFront", {}, s2); time.sleep(1)
    print(chrome.call("Runtime.evaluate", {"expression": "document.visibilityState+' '+hidden.toString().length", "returnByValue": True}, s)["result"].get("value"))
    print(chrome.call("Runtime.evaluate", {"expression": "hidden()", "returnByValue": True, "awaitPromise": True}, s)["result"].get("value"))
finally:
    chrome.close()
