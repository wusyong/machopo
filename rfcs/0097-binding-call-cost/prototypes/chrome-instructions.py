import subprocess, os, sys, re, json
S=os.path.dirname(os.path.abspath(__file__))
SETUP="const html = document.documentElement || document.appendChild(document.createElement('html')); const n = document.body || html.appendChild(document.createElement('body')); const c = document.createElement('p'); n.appendChild(c); n.setAttribute('id', 'x'); let x;"
CASES={"empty":"x = n;","nodeType":"x = n.nodeType;","parentNode":"x = c.parentNode;","firstChild":"x = n.firstChild;","getAttribute":"x = n.getAttribute('id');","id":"x = n.id;","tagName":"x = n.tagName;","hasAttribute":"x = n.hasAttribute('id');","setAttribute":"n.setAttribute('a', 'b');","contains":"x = n.contains(c);"}
def chrome(body, n):
    html=f"<!doctype html><html><body><script>(function () {{ {SETUP} for (let i = 0; i < {n}; i++) {{ {body} }} }})(); document.title='done';</script></body></html>"
    p=f"{S}/chrome/instr.html"; open(p,'w').write(html)
    best=None
    for _ in range(3):
        r=subprocess.run(["perf","stat","-x,","-e","instructions:u","--","chromium-browser","--headless=new","--disable-gpu","--no-first-run",f"--user-data-dir={S}/chrome/profile","--dump-dom",f"file://{p}"],capture_output=True,text=True,timeout=600)
        v=[float(l.split(',')[0]) for l in r.stderr.splitlines() if 'instructions' in l]
        best=v[0] if best is None else min(best,v[0])
    return best
def machopo(body, n):
    best=None
    for _ in range(3):
        env={**os.environ,"MACHOPO_BENCH_WINDOW":"1","MACHOPO_BENCH_SCRIPT":f"(function () {{ {SETUP} for (let i = 0; i < {n}; i++) {{ {body} }} }})(); 0"}
        r=subprocess.run(["perf","stat","-x,","-e","instructions:u","--",f"{S}/it-4kG","--ignored","--exact","bench::bench_calls","--test-threads","1"],env=env,capture_output=True,text=True)
        v=[float(l.split(',')[0]) for l in r.stderr.splitlines() if 'instructions' in l]
        best=v[0] if best is None else min(best,v[0])
    return best
A,B=20_000_000,60_000_000
res={}
for name,f in [("chrome",chrome),("machopo",machopo)]:
    per={}
    for c,body in CASES.items():
        lo=f(body,A); hi=f(body,B); per[c]=(hi-lo)/(B-A)
    res[name]={c:per[c]-per["empty"] for c in CASES if c!="empty"}; res[name]["empty loop"]=per["empty"]
    print(name, {k:round(v,1) for k,v in res[name].items()}, flush=True)
print("ratio", {k:round(res["machopo"][k]/res["chrome"][k],2) for k in res["chrome"] if res["chrome"][k]>1})
