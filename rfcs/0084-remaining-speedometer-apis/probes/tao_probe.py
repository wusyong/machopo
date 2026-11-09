import os, sys, tempfile, time, threading, json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
import common
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        u = urlparse(self.path); q = parse_qs(u.query)
        if u.path == '/r':
            self.send_response(302); self.send_header('Location', q['to'][0])
            if 'tao' in q: self.send_header('Timing-Allow-Origin', q['tao'][0])
            self.end_headers(); return
        body = b''
        if u.path == '/page':
            body = b"<script>onload=()=>setTimeout(()=>{const e=performance.getEntriesByType('navigation')[0];parent.postMessage(e.redirectCount+' '+(e.redirectStart>0),'*')},0)</script>"
        elif u.path == '/top':
            body = TOP.encode()
        self.send_response(200); self.send_header('Content-Type','text/html'); self.end_headers(); self.wfile.write(body)
a = ThreadingHTTPServer(('127.0.0.1', 0), H); b = ThreadingHTTPServer(('127.0.0.1', 0), H)
A = f"http://127.0.0.1:{a.server_port}"; B = f"http://localhost:{b.server_port}"
for s in (a, b): threading.Thread(target=s.serve_forever, daemon=True).start()
from urllib.parse import quote
page = A + "/page"
cases = {
  'same': f"{A}/r?to={quote(page)}",
  'cross-no-tao': f"{A}/r?to={quote(B + '/r?to=' + quote(page))}",
  'cross-tao-star': f"{A}/r?to={quote(B + '/r?tao=*&to=' + quote(page))}",
  'cross-tao-dest': f"{A}/r?to={quote(B + '/r?tao=' + quote(A) + '&to=' + quote(page))}",
  'cross-first-tao-star': f"{B}/r?tao=*&to={quote(page)}",
}
TOP = "<script>const out={};const keys=" + json.dumps(list(cases)) + ";const urls=" + json.dumps(list(cases.values())) + """;
let i=0;function next(){if(i>=keys.length){window.done=JSON.stringify(out);return;}const f=document.createElement('iframe');onmessage=e=>{out[keys[i]]=e.data;f.remove();i++;next();};f.src=urls[i];document.body.appendChild(f);}
onload=next;</script><body></body>"""
chrome = common.ChromePipe(["--headless=new", f"--user-data-dir={tempfile.mkdtemp()}", "--no-first-run", "about:blank"])
try:
    session = chrome.navigate(A + "/top", front=True); time.sleep(3)
    r = chrome.call("Runtime.evaluate", {"expression": "window.done", "returnByValue": True}, session)
    print(r["result"].get("value"))
    # The main frame too.
    for k in ['cross-tao-star', 'cross-no-tao']:
        session = chrome.navigate(cases[k].replace('/page','/page'), front=True); time.sleep(1.5)
        r = chrome.call("Runtime.evaluate", {"expression": "(()=>{const e=performance.getEntriesByType('navigation')[0];return e.redirectCount+' '+(e.redirectStart>0)})()", "returnByValue": True}, session)
        print('main', k, r["result"].get("value"))
finally:
    chrome.close()
