"""ns per call of a few loop bodies, timed by the page itself
(performance.now, the best of several rounds), in Chrome for Testing, the
distribution's Chromium and machopo."""
import subprocess, os, json, threading, http.server, sys, time, signal
S=os.path.dirname(os.path.abspath(__file__))  # needs `chrome/` beside it for the profiles, and machopo's `it` binary as `it-now`
CASES={"empty":"x = n;","createElement":"x = document.createElement('div');","div+span+append":"document.createElement('div').appendChild(document.createElement('span'));","append new div":"if (i % 1000000 === 0) root = document.createElement('div'); root.appendChild(document.createElement('div'));","createTextNode":"x = document.createTextNode('a');","nodeType":"x = n.nodeType;","getAttribute":"x = n.getAttribute('id');"}
N,ROUNDS=2_000_000,7
JS="""(function(){ const html = document.documentElement || document.appendChild(document.createElement('html')); const n = document.body || html.appendChild(document.createElement('body')); n.setAttribute('id','x'); let x; let root = document.createElement('div'); const out = {};
const cases = %s;
for (const name in cases) { const f = new Function('n','root','N', 'let x; for (let i = 0; i < N; i++) { ' + cases[name] + ' } return x;'); let best = Infinity; for (let r = 0; r < %d; r++) { const t = performance.now(); f(n, root, %d); best = Math.min(best, performance.now() - t); } out[name] = best * 1e6 / %d; }
return JSON.stringify(out); })()""" % (json.dumps(CASES), ROUNDS, N, N)
def chrome(binary, tag):
    got={}
    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body=("<!doctype html><html><body><script>fetch('/r',{method:'POST',body:%s});</script></body></html>" % JS).encode()
            self.send_response(200); self.send_header("Content-Type","text/html"); self.end_headers(); self.wfile.write(body)
        def do_POST(self):
            got["r"]=json.loads(self.rfile.read(int(self.headers["Content-Length"]))); self.send_response(200); self.end_headers()
        def log_message(self,*a): pass
    srv=http.server.HTTPServer(("127.0.0.1",0),H); threading.Thread(target=srv.serve_forever,daemon=True).start()
    p=subprocess.Popen([binary,"--headless=new","--disable-gpu","--no-first-run","--no-sandbox","--disable-extensions",f"--user-data-dir={S}/chrome/profile-t-{tag}",f"http://127.0.0.1:{srv.server_port}/"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
    t=time.time()
    while "r" not in got and time.time()-t<600: time.sleep(0.2)
    os.killpg(p.pid, signal.SIGKILL); srv.shutdown()
    return got.get("r")
def machopo():
    env={**os.environ,"MACHOPO_BENCH_WINDOW":"1","MACHOPO_BENCH_SCRIPT":JS}
    r=subprocess.run([f"{S}/it-now","--ignored","--exact","bench::bench_calls","--test-threads","1","--nocapture"],env=env,capture_output=True,text=True,timeout=900)
    o=r.stdout+r.stderr; l=[x for x in o.splitlines() if 'left:' in x][0]
    return json.loads(json.loads(l[l.index('"'):].strip()))
res={"CfT 152":chrome("/home/wusyong/Desktop/m/machopo/target/chrome-for-testing/152.0.7977.82/chrome-linux64/chrome","cft"),"Chromium 156":chrome("/usr/bin/chromium-browser","fed"),"machopo":machopo()}
print("%-18s"%""+"".join("%14s"%k for k in res)+"   machopo/CfT  machopo/Chromium")
for c in CASES:
    v=[res[k][c] if res[k] else float('nan') for k in res]
    print("%-18s"%c+"".join("%14.1f"%x for x in v)+"   %8.2f %12.2f"%(v[2]/v[0],v[2]/v[1]))
