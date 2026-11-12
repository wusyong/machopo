import subprocess, sys, os, json, re
S=os.path.dirname(os.path.abspath(__file__)); js=open(f"{S}/chrome/bench.js").read()
def chrome():
    r=subprocess.run(["taskset","-c","2","chromium-browser","--headless=new","--disable-gpu","--no-first-run",f"--user-data-dir={S}/chrome/profile","--dump-dom",f"file://{S}/chrome/page.html"],capture_output=True,text=True,timeout=600)
    m=re.search(r'RESULT (\{.*?\})',r.stdout); return json.loads(m.group(1))
def machopo(b):
    env={**os.environ,"MACHOPO_BENCH_WINDOW":"1","MACHOPO_BENCH_SCRIPT":js+" run()"}
    r=subprocess.run(["taskset","-c","2",f"{S}/{b}","--ignored","--exact","bench::bench_calls","--test-threads","1","--nocapture"],env=env,capture_output=True,text=True,timeout=600)
    o=r.stdout+r.stderr; l=[x for x in o.splitlines() if 'left:' in x][0]
    return json.loads(json.loads(l[l.index('"'):].strip()))
def best(f,n=3):
    out=None
    for _ in range(n):
        r=f(); out=r if out is None else {k:min(out[k],r[k]) for k in r}
    return out
bins=sys.argv[1:]
c=best(chrome); ms={b:best(lambda b=b: machopo(b)) for b in bins}
print("%-28s %8s"%("", "Chrome")+"".join("%10s %5s"%(b[3:],"x") for b in bins))
for k in c:
    print("%-28s %8.1f"%(k,c[k])+"".join("%10.1f %5.2f"%(ms[b][k], ms[b][k]/c[k] if c[k] else 0) for b in bins))
