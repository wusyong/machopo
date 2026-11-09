import sys, tempfile, time, json, os
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
portA = serve.start(); portB = serve.start()
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "--window-size=800,800", "about:blank"])
def send(method, params, session):
    chrome.next_id += 1
    chrome.to_browser.write(json.dumps({"id": chrome.next_id, "method": method, "params": params, "sessionId": session}).encode() + b"\0"); chrome.to_browser.flush()
try:
    s = chrome.navigate(f"http://127.0.0.1:{portA}/iip_top3.html?portB={portB}", front=True); time.sleep(1.5)
    points = {"clip-path": (100, 100), "between-two-covers": (150, 350), "opacity": (100, 600)}
    for busy, name0 in [("a","clip-path"),("b","between-two-covers"),("c","opacity")]:
        for name, (x, y) in [(name0, points[name0])]:
            for kind in ["press"]:
                chrome.call("Runtime.evaluate", {"expression": f"window.__r=null;setTimeout(()=>runIn('{busy}',1200).then(r=>window.__r=r),50);1"}, s)
                time.sleep(0.4)
                if kind == "press":
                    send("Input.dispatchMouseEvent", {"type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1}, s)
                    send("Input.dispatchMouseEvent", {"type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1}, s)
                else:
                    send("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": x + 3, "y": y + 3}, s)
                time.sleep(1.6)
                r = chrome.call("Runtime.evaluate", {"expression": "JSON.stringify(window.__r)", "returnByValue": True}, s)
                print(f"busy={busy:5} at={name:22} {kind:5} -> {r['result'].get('value')}")
finally:
    chrome.close()
