#!/usr/bin/env python3
"""HTTP-only documentation fixture. Run with Python 3; stop with Control-C."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

PRODUCTS = [
    {"id": "prod_001", "name": "Desk lamp", "price": 49, "in_stock": True},
    {"id": "prod_002", "name": "Notebook", "price": 12, "in_stock": True},
]


class DemoAPI(BaseHTTPRequestHandler):
    def reply(self, status, value, headers=None):
        body = json.dumps(value, indent=2).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        for name, field in (headers or {}).items():
            self.send_header(name, field)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def read_json(self):
        """The JSON request body, or None after replying with an error."""
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 <= size <= 1_048_576:
                self.reply(413, {"error": "Demo payload limit is 1 MiB"})
                return None
            return json.loads(self.rfile.read(size))
        except (ValueError, UnicodeDecodeError):
            self.reply(400, {"error": "Send a JSON body"})
            return None

    def do_GET(self):
        if urlsplit(self.path).path != "/products":
            return self.reply(404, {"error": "Not found"})
        self.reply(200, {"products": PRODUCTS, "total": len(PRODUCTS), "currency": "USD"},
                   {"Accept-Query": "application/json"})

    do_HEAD = do_GET

    def do_POST(self):
        if urlsplit(self.path).path != "/echo":
            return self.reply(404, {"error": "Not found"})
        value = self.read_json()
        if value is not None:
            self.reply(200, {"ok": True, "body": value})

    def do_QUERY(self):
        """RFC 10008: filters /products with a JSON query such as {"max_price": 20}."""
        if urlsplit(self.path).path != "/products":
            return self.reply(404, {"error": "Not found"})
        media_type = self.headers.get("Content-Type", "").split(";")[0].strip().lower()
        if not media_type:
            return self.reply(400, {"error": "A QUERY needs a Content-Type"})
        if media_type != "application/json":
            return self.reply(415, {"error": "Send the query as application/json"},
                              {"Accept-Query": "application/json"})
        query = self.read_json()
        if query is None:
            return
        if not isinstance(query, dict):
            return self.reply(422, {"error": "Send a JSON object, for example {\"max_price\": 20}"})
        matches = [
            product for product in PRODUCTS
            if product["price"] <= query.get("max_price", product["price"])
            and query.get("in_stock", product["in_stock"]) == product["in_stock"]
            and str(query.get("name", "")).lower() in product["name"].lower()
        ]
        self.reply(200, {"products": matches, "total": len(matches), "currency": "USD"},
                   {"Accept-Query": "application/json"})


if __name__ == "__main__":
    with ThreadingHTTPServer(("127.0.0.1", 18990), DemoAPI) as server:
        print("Wirebolt demo API: http://127.0.0.1:18990/products", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
