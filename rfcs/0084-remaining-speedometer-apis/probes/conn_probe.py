import sys, tempfile, time, json, os
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
port = serve.start()
extra = sys.argv[1:]
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", *extra, "about:blank"])
try:
    for host in ["127.0.0.1", "localhost", "127.0.0.1"]:
        s = chrome.navigate(f"http://{host}:{port}/conn.html", front=True); time.sleep(0.3)
        r = chrome.call("Runtime.evaluate", {"expression": "run()", "returnByValue": True, "awaitPromise": True}, s)
        print(r["result"].get("value") or r)
    s = chrome.navigate("about:blank", front=True)
    r = chrome.call("Runtime.evaluate", {"expression": "[navigator.connection.effectiveType,navigator.connection.rtt,navigator.connection.downlink].join('/')", "returnByValue": True}, s)
    print("about:blank", r["result"].get("value"))
finally:
    chrome.close()
