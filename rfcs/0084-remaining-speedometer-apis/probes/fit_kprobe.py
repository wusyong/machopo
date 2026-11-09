"""machopo (target/debug/machopo): `python3 fit_kprobe.py fit.html` prints report()."""
import os, sys, time
from pathlib import Path
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../../scripts/bench"))
sys.path.insert(0, HERE)
import common, serve
port = serve.start()
log = open(os.devnull, "w")
machopo = common.MachopoPipe(Path(sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "../../../target/debug/machopo")), log)
try:
    machopo.set_window_size()
    session = machopo.navigate(f"http://127.0.0.1:{port}/{sys.argv[1]}", front=False); time.sleep(1.5)
    r = machopo.call("Runtime.evaluate", {"expression": "report()", "returnByValue": True}, session)
    print(r["result"]["value"])
finally:
    common.end(machopo.process) if hasattr(machopo, "process") else machopo.close()
