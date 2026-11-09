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
    s = chrome.navigate(f"http://127.0.0.1:{portA}/iip_top5.html?portB={portB}", front=True); time.sleep(1.5)
    points = {"a": (150, 50), "rounded": (150, 200), "clippath": (150, 350), "b": (150, 500)}
    for busy, at, label in [("a","a","iframe border-radius"),("top","rounded","top in rounded div"),("top","clippath","top in clip-path div"),("b","b","iframe in rounded div")]:
        for name, (x, y) in [(at, points[at])]:
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
                print(f"{label:20} -> {r['result'].get('value')}")
finally:
    chrome.close()
