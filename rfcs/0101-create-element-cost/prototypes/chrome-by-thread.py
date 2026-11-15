"""Cycles and instructions per createElement call by thread in a Chrome
binary: perf samples a run of the loop, less a run of the empty loop."""
import subprocess, os, re, sys, threading, http.server, time, signal
S=os.path.dirname(os.path.abspath(__file__)); N=20_000_000
def run(binary, tag, body, event, period, out):
    got=[]
    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            page=("<!doctype html><html><body><script>(function(){ const n = document.body; let x; for (let i = 0; i < %d; i++) { %s } })(); fetch('/r',{method:'POST',body:'done'});</script></body></html>" % (N, body)).encode()
            self.send_response(200); self.send_header("Content-Type","text/html"); self.end_headers(); self.wfile.write(page)
        def do_POST(self):
            got.append(1); self.send_response(200); self.end_headers()
        def log_message(self,*a): pass
    srv=http.server.HTTPServer(("127.0.0.1",0),H); threading.Thread(target=srv.serve_forever,daemon=True).start()
    p=subprocess.Popen(["perf","record","-q","-e",event,"-c",str(period),"-o",out,"--",binary,"--headless=new","--disable-gpu","--no-first-run","--no-sandbox","--disable-extensions",f"--user-data-dir={S}/chrome/profile-p-{tag}",f"http://127.0.0.1:{srv.server_port}/"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
    t=time.time()
    while not got and time.time()-t<600: time.sleep(0.1)
    os.killpg(p.pid, signal.SIGINT)
    try: p.wait(timeout=60)
    except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL)
    srv.shutdown()
    r=subprocess.run(["perf","report","-i",out,"--stdio","--sort","comm","-F","period,comm"],capture_output=True,text=True)
    d={}
    for l in r.stdout.splitlines():
        m=re.match(r'\s*(\d+)\s+(\S+)',l)
        if m: d[m.group(2)]=d.get(m.group(2),0)+int(m.group(1))
    return d
binary,tag=sys.argv[1],sys.argv[2]
for event,period in (("cycles:u",200003),("instructions:u",1000003)):
    c=run(binary,tag,"x = document.createElement('div');",event,period,f"{S}/bt2-{tag}-case.data")
    b=run(binary,tag,"x = n;",event,period,f"{S}/bt2-{tag}-base.data")
    diff={k:(c.get(k,0)-b.get(k,0))/N for k in set(c)|set(b)}
    print(tag,event,"total %.0f"%sum(diff.values()),{k:round(v) for k,v in sorted(diff.items(),key=lambda x:-x[1]) if abs(v)>=3},flush=True)
