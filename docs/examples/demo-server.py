#!/usr/bin/env python3
"""HTTP-only documentation fixture. Run with Python 3; stop with Control-C."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


class DemoAPI(BaseHTTPRequestHandler):
    def reply(self, status, value):
        body = json.dumps(value, indent=2).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_GET(self):
        if urlsplit(self.path).path != "/products":
            return self.reply(404, {"error": "Not found"})
        self.reply(200, {
            "products": [
                {"id": "prod_001", "name": "Desk lamp", "price": 49, "in_stock": True},
                {"id": "prod_002", "name": "Notebook", "price": 12, "in_stock": True},
            ],
            "total": 2,
            "currency": "USD",
        })

    do_HEAD = do_GET

    def do_POST(self):
        if urlsplit(self.path).path != "/echo":
            return self.reply(404, {"error": "Not found"})
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 <= size <= 1_048_576:
                return self.reply(413, {"error": "Demo payload limit is 1 MiB"})
            value = json.loads(self.rfile.read(size))
        except (ValueError, UnicodeDecodeError):
            return self.reply(400, {"error": "Send a JSON body"})
        self.reply(200, {"ok": True, "body": value})


if __name__ == "__main__":
    with ThreadingHTTPServer(("127.0.0.1", 18990), DemoAPI) as server:
        print("Wirebolt demo API: http://127.0.0.1:18990/products", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
