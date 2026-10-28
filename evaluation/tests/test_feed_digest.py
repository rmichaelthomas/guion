"""Acceptance and parity checks for the feed-digest development workload.

Build the Guion CLI first. Set GUION_UV to the pinned uv executable. The
harness serves all feed data from a loopback-only HTTP server. Set
GUION_FEED_PACKAGE_RUNNER to exercise a relocated package instead of source.
"""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from shutil import copytree, ignore_patterns
import os
import subprocess
import tempfile
import threading
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PROJECT = ROOT / "evaluation/development/guion_feed_digest"
PROJECT = Path(os.environ.get("GUION_FEED_PROJECT", DEFAULT_PROJECT))
BASELINE = PROJECT / "python_baseline.py"
GUION = Path(os.environ.get("GUION_BINARY", ROOT / "target/debug/guion"))
PACKAGE_RUNNER = os.environ.get("GUION_FEED_PACKAGE_RUNNER")
SCRATCH = Path(os.environ.get("GUION_FEED_SCRATCH", ROOT / "target/perf-session"))


FEED_A = """<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel><title>Alpha</title><link>http://example.test/</link><description>fixture</description>
<item><title>Crème brûlée 東京</title><link>http://example.test/story#old</link><pubDate>Wed, 21 Oct 2026 07:28:00 GMT</pubDate></item>
<item><title>Second</title><link>http://example.test/second</link></item>
</channel></rss>"""
FEED_B = """<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel><title>Beta</title><link>http://example.test/</link><description>fixture</description>
<item><title>Duplicate title does not replace canonical first result</title><link>http://example.test/story</link></item>
<item><title>Third</title><link>http://example.test/third</link></item>
</channel></rss>"""


class Handler(BaseHTTPRequestHandler):
    changed = False
    active = 0
    peak = 0
    stats_lock = threading.Lock()

    def do_GET(self):
        if self.path == "/a.xml":
            body = FEED_A.replace("Crème brûlée 東京", "Crème brûlée 改訂") if self.changed else FEED_A
            self.send_response(200)
            self.send_header("Content-Type", "application/rss+xml; charset=utf-8")
            self.send_header("Content-Length", str(len(body.encode("utf-8"))))
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        elif self.path == "/b.xml":
            body = FEED_B.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path == "/bad.xml":
            body = b"not xml at all"
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path == "/missing.xml":
            body = b"not found"
            self.send_response(404)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path in {"/slow-a.xml", "/slow-b.xml"}:
            with self.stats_lock:
                type(self).active += 1
                type(self).peak = max(type(self).peak, type(self).active)
            try:
                time.sleep(0.2)
                body = FEED_A.encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            finally:
                with self.stats_lock:
                    type(self).active -= 1
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *_):
        pass


class FeedDigestAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if PACKAGE_RUNNER:
            if not Path(PACKAGE_RUNNER).is_file():
                raise RuntimeError(f"Guion package runner missing at {PACKAGE_RUNNER}")
        elif not GUION.is_file():
            raise RuntimeError(f"Guion binary missing at {GUION}; build the CLI first")
        SCRATCH.mkdir(parents=True, exist_ok=True)
        cls.work_dir = tempfile.TemporaryDirectory(prefix="guion-feed-acceptance-", dir=SCRATCH)
        cls.work = Path(cls.work_dir.name)
        cls.project = cls.work / "feed_digest"
        copytree(PROJECT, cls.project, ignore=ignore_patterns(".venv", "__pycache__", "digest.md"))
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.server.server_port}"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)
        cls.work_dir.cleanup()

    def run_guion(self, urls, cwd, project):
        if PACKAGE_RUNNER:
            command = [PACKAGE_RUNNER, *urls]
            working_dir = project
            environment = os.environ.copy()
            for name in (
                "GUION_UV",
                "GUION_PYTHON",
                "GUION_PYTHON_INSTALL_DIR",
                "GUION_PYTHON_CACHE_DIR",
                "UV_PYTHON_INSTALL_DIR",
                "UV_CACHE_DIR",
                "UV_OFFLINE",
            ):
                environment.pop(name, None)
        else:
            command = [str(GUION), "run", str(project), *urls]
            working_dir = cwd
            environment = os.environ.copy()
        return subprocess.run(
            command, cwd=working_dir, text=True, capture_output=True, timeout=30, env=environment
        )

    def run_python(self, urls, cwd, project):
        bridge = project / ".guion/python-bridge"
        if not bridge.is_dir():
            bridge = project / ".veritas/python-bridge"
        command = ["uv", "run", "--project", str(bridge), "--locked", "--offline", "python", str(project / "python_baseline.py"), str(project / "digest.md"), *urls]
        env = os.environ.copy()
        pinned_uv = env.get("GUION_UV")
        if pinned_uv:
            command[0] = pinned_uv
        return subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=30, env=env)

    def compare(self, urls):
        guion = self.run_guion(urls, self.work, self.project)
        self.assertEqual(guion.returncode, 0, guion.stderr)
        guion_digest = (self.project / "digest.md").read_text(encoding="utf-8")
        python = self.run_python(urls, self.work, self.project)
        self.assertEqual(python.returncode, 0, python.stderr)
        python_digest = (self.project / "digest.md").read_text(encoding="utf-8")
        self.assertEqual(guion_digest, python_digest)
        return guion_digest

    def test_dedup_unicode_sort_failure_and_python_parity(self):
        Handler.changed = False
        digest = self.compare([f"{self.base}/a.xml", f"{self.base}/b.xml", f"{self.base}/missing.xml"])
        self.assertIn("Crème brûlée 東京", digest)
        self.assertEqual(digest.count("http://example.test/story)"), 1)
        self.assertLess(digest.index("http://example.test/second"), digest.index("http://example.test/story"))
        self.assertIn("R_HTTP_STATUS: HTTP server returned status 404", digest)

    def test_reversed_input_is_deterministic(self):
        forward = self.compare([f"{self.base}/a.xml", f"{self.base}/b.xml"])
        reverse = self.compare([f"{self.base}/b.xml", f"{self.base}/a.xml"])
        self.assertEqual(forward, reverse)

    def test_changed_title_is_reflected_on_fresh_run(self):
        Handler.changed = False
        first = self.compare([f"{self.base}/a.xml", f"{self.base}/b.xml"])
        Handler.changed = True
        second = self.compare([f"{self.base}/a.xml", f"{self.base}/b.xml"])
        self.assertIn("Crème brûlée 改訂", second)
        self.assertNotEqual(first, second)

    def test_malformed_feed_contributes_no_entries(self):
        digest = self.compare([f"{self.base}/bad.xml", f"{self.base}/a.xml"])
        self.assertIn("Crème brûlée 東京", digest)
        self.assertNotIn("not xml at all", digest)

    def test_ungranted_hostname_is_denied(self):
        digest = self.compare(["http://localhost:1/not-contacted", f"{self.base}/a.xml"])
        self.assertIn("R_NETWORK_DENIED: network access to this host is not granted", digest)

    def test_guion_and_python_fetch_independent_feeds_concurrently(self):
        urls = [f"{self.base}/slow-a.xml", f"{self.base}/slow-b.xml"]
        Handler.peak = 0
        guion = self.run_guion(urls, self.work, self.project)
        self.assertEqual(guion.returncode, 0, guion.stderr)
        self.assertGreaterEqual(Handler.peak, 2, "Guion serialized the feed requests")
        guion_digest = (self.project / "digest.md").read_text(encoding="utf-8")

        Handler.peak = 0
        python = self.run_python(urls, self.work, self.project)
        self.assertEqual(python.returncode, 0, python.stderr)
        self.assertGreaterEqual(Handler.peak, 2, "Python serialized the feed requests")
        self.assertEqual((self.project / "digest.md").read_text(encoding="utf-8"), guion_digest)


if __name__ == "__main__":
    unittest.main()
