import http.server, sys, json
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def send(self, code, body, extra=None, chunked=False):
        b = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("X-Test", "a")
        self.send_header("X-Test", "b")
        for k, v in (extra or {}).items(): self.send_header(k, v)
        if chunked:
            self.send_header("Transfer-Encoding", "chunked"); self.end_headers()
            for i in range(0, len(b), 5):
                part = b[i:i+5]; self.wfile.write(b"%x\r\n" % len(part) + part + b"\r\n")
            self.wfile.write(b"0\r\n\r\n")
        else:
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        if self.path.startswith("/hello"): self.send(200, "안녕 " + self.path)
        elif self.path == "/chunk": self.send(200, "조각조각 나뉜 응답입니다", chunked=True)
        elif self.path == "/go": self.send(302, "", {"Location": "/hello?from=go"})
        elif self.path == "/rel/a": self.send(301, "", {"Location": "b"})
        elif self.path == "/rel/b": self.send(200, "상대 주소 도착")
        elif self.path == "/loop": self.send(302, "", {"Location": "/loop"})
        elif self.path == "/ua": self.send(200, self.headers.get("User-Agent","") + "|" + self.headers.get("X-Key",""))
        else: self.send(404, "없음")
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0)); body = self.rfile.read(n).decode()
        if self.path == "/see": self.send(303, "", {"Location": "/hello?after=post"})
        else: self.send(201, self.headers.get("Content-Type","") + "|" + body)
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
