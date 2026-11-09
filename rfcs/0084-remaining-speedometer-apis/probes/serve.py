"""Serves this directory on 127.0.0.1:PORT; `/cors/x` adds
Access-Control-Allow-Origin: *, `/redirect/x` redirects to `/x`,
`/missing.css` is a 404."""
import http.server, threading, os, sys
ROOT = os.path.dirname(os.path.abspath(__file__))
class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=ROOT, **k)
    def log_message(self, *a): pass
    def end_headers(self):
        if getattr(self, "cors", False):
            self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()
    def do_GET(self):
        if self.path.startswith("/redirect/"):
            self.send_response(302); self.send_header("Location", "/" + self.path[len("/redirect/"):]); self.end_headers(); return
        if self.path.startswith("/cors/"):
            self.path = "/" + self.path[len("/cors/"):]
            self.cors = True
        super().do_GET()
    def guess_type(self, path):
        return "text/css" if path.endswith(".css") else super().guess_type(path)
def start():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server.server_address[1]
