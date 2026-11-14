"""A recording proxy for the SDK smoke test.

usage: python3 capture.py <listen_port> <upstream_base_url> <out_dir>

Forwards every request to the upstream rupta server unchanged and writes
each request to <out_dir>/<n>.json (method, path, query, headers) and
<n>.body (the body, decompressed when gzip or deflate encoded).
"""
import gzip
import http.server
import json
import os
import sys
import threading
import urllib.error
import urllib.request
import zlib

PORT, UPSTREAM, OUT = int(sys.argv[1]), sys.argv[2].rstrip("/"), sys.argv[3]
os.makedirs(OUT, exist_ok=True)
lock = threading.Lock()
counter = [0]
KEEP = {"content-type", "content-encoding", "x-sentry-auth", "user-agent", "origin"}


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def handle_any(self):
        if "chunked" in (self.headers.get("transfer-encoding") or "").lower():
            body = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    self.rfile.readline()
                    break
                body += self.rfile.read(size)
                self.rfile.readline()
        else:
            length = int(self.headers.get("content-length") or 0)
            body = self.rfile.read(length) if length else b""
        req = urllib.request.Request(UPSTREAM + self.path, data=body if self.command == "POST" else None, method=self.command)
        for k, v in self.headers.items():
            if k.lower() not in ("host", "content-length", "connection", "transfer-encoding"):
                req.add_header(k, v)
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                status, headers, data = r.status, r.getheaders(), r.read()
        except urllib.error.HTTPError as e:
            status, headers, data = e.code, e.headers.items(), e.read()
        if self.command == "POST":
            enc = (self.headers.get("content-encoding") or "").lower()
            plain = body
            try:
                if enc == "gzip":
                    plain = gzip.decompress(body)
                elif enc == "deflate":
                    plain = zlib.decompress(body)
            except Exception:
                pass
            with lock:
                counter[0] += 1
                n = counter[0]
            path, _, query = self.path.partition("?")
            meta = {
                "method": self.command,
                "path": path,
                "query": query,
                "status": status,
                "headers": {k.lower(): v for k, v in self.headers.items() if k.lower() in KEEP},
                "stored_encoding": enc if plain is body and enc else "",
            }
            with open(os.path.join(OUT, f"{n:03}.json"), "w") as f:
                json.dump(meta, f, indent=1)
            with open(os.path.join(OUT, f"{n:03}.body"), "wb") as f:
                f.write(plain)
        self.send_response(status)
        for k, v in headers:
            if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                self.send_header(k, v)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_POST = handle_any
    do_OPTIONS = handle_any


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
