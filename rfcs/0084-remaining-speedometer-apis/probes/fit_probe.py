"""Chrome 152: `python3 fit_probe.py fit.html` prints report(); the fit_flex*, fit_grid* pages need WPT's fonts/Ahem.ttf copied here."""
import os, sys, tempfile, time, json
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
port = serve.start()
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "about:blank"])
try:
    session = chrome.navigate(f"http://127.0.0.1:{port}/{sys.argv[1]}", front=True); time.sleep(1)
    r = chrome.call("Runtime.evaluate", {"expression": "report()", "returnByValue": True}, session)
    print(r["result"]["value"])
    r = chrome.call("Runtime.evaluate", {"expression": "navigator.userAgent", "returnByValue": True}, session)
    print(r["result"]["value"])
finally:
    chrome.close()
