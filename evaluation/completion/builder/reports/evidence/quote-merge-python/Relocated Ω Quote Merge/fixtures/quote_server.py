#!/usr/bin/env python3
"""Deterministic fixture feeds for the Quote Merge builder task."""
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FEEDS = {
    "/feed-a": {"quotes": [{"carrier": "Aster", "service": "ground", "price_cents": 1220, "days": 4}, {"carrier": "Birch", "service": "air", "price_cents": 2500, "days": 1}]},
    "/feed-b": {"quotes": [{"carrier": "Aster", "service": "air", "price_cents": 1100, "days": 2}, {"carrier": "Cedar", "service": "ground", "price_cents": 900, "days": 5}]},
    "/feed-c": {"quotes": [{"carrier": "Birch", "service": "ground", "price_cents": 1900, "days": 3}, {"carrier": "Cedar", "service": "air", "price_cents": 1300, "days": 1}]},
}
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/slow-503":
            time.sleep(0.35)
            self.send_response(503)
            self.end_headers()
            return
        if self.path == "/malformed":
            body = b'{"quotes":[broken]}'
            self.send_response(200)
        elif self.path in FEEDS:
            body = json.dumps(FEEDS[self.path], sort_keys=True).encode()
            self.send_response(200)
        else:
            body = b'{}'
            self.send_response(404)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_args):
        pass
if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    print(server.server_address[1], flush=True)
    server.serve_forever()
