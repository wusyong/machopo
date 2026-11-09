"""IntersectionObserver v2 probes in Chrome: `iov_probe.py <page> chrome [case,case]`."""
import sys, tempfile, time, json, os
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench")); sys.path.insert(0, HERE)
import common, serve
page, browser = sys.argv[1], sys.argv[2]
portA = serve.start(); portB = serve.start()
url = f"http://127.0.0.1:{portA}/{page}?portB={portB}" + (("&only=" + sys.argv[3]) if len(sys.argv) > 3 else "")
b = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "--window-size=800,1500", "about:blank"])
try:
    s = b.navigate(url, front=True); time.sleep(0.5)
    r = b.call("Runtime.evaluate", {"expression": "run()", "returnByValue": True, "awaitPromise": True}, s)
    print(r["result"].get("value") or r)
finally:
    b.close()
