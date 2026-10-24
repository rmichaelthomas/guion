"""Acceptance checks for the persistent feed inbox application."""

from __future__ import annotations

import json
import os
from pathlib import Path
from shutil import copytree, ignore_patterns, rmtree
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import subprocess
import tempfile
from threading import Lock
from time import sleep
import unittest


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PROJECT = ROOT / "evaluation/development/feed_inbox"
if not DEFAULT_PROJECT.is_dir():
    DEFAULT_PROJECT = ROOT / "examples/feed_inbox"
PROJECT = Path(os.environ.get("VERITAS_FEED_INBOX_PROJECT", DEFAULT_PROJECT))
VERITAS = Path(os.environ.get("VERITAS_BINARY", ROOT / "target/debug/veritas"))
UV = Path(os.environ.get("VERITAS_UV", "/private/tmp/veritas-foundation-rc35-verify/veritas-0.1.0-darwin-arm64/toolchains/uv"))
UV_CACHE = Path(os.environ.get("UV_CACHE_DIR", "/private/tmp/veritas-uv-interop-rc34"))
PYTHON_INSTALL = Path(os.environ.get("UV_PYTHON_INSTALL_DIR", "/private/tmp/veritas-python-cold-rc33"))
FEED = """<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel><title>Fixture</title><link>http://example.test/</link><description>fixture</description>
<item><title>A Refreshed Café</title><link>https://example.test/a</link><pubDate>Wed, 21 Oct 2026 07:28:00 GMT</pubDate></item>
<item><title>Fourth item</title><link>https://example.test/d</link></item>
</channel></rss>"""


class SlowFeedHandler(BaseHTTPRequestHandler):
    active = 0
    peak_active = 0
    stats_lock = Lock()

    def do_GET(self):
        with self.stats_lock:
            type(self).active += 1
            type(self).peak_active = max(type(self).peak_active, type(self).active)
        try:
            sleep(0.25)
            payload = FEED.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/rss+xml; charset=utf-8")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        finally:
            with self.stats_lock:
                type(self).active -= 1

    def log_message(self, format, *args):
        pass


class FeedInboxAcceptance(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="veritas-feed-inbox-")
        self.root = Path(self.temp.name)
        self.veritas_project = self.root / "veritas"
        self.python_project = self.root / "python"
        copytree(PROJECT, self.veritas_project, ignore=ignore_patterns(".venv", "__pycache__"))
        copytree(PROJECT, self.python_project, ignore=ignore_patterns(".venv", "__pycache__"))
        self.fixture = {"entries": [
            {"link": "https://example.test/a", "title": "Café Tokyo", "published": "2026-10-21", "read": False},
            {"link": "https://example.test/b", "title": "Already read", "published": "2026-10-20", "read": True},
            {"link": "https://example.test/c", "title": "Latest item", "published": "2026-10-22", "read": False},
        ]}
        payload = json.dumps(self.fixture, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        (self.veritas_project / "inbox.json").write_text(payload + "\n", encoding="utf-8")
        (self.python_project / "inbox.json").write_text(payload + "\n", encoding="utf-8")

    def tearDown(self):
        self.temp.cleanup()

    def run_veritas(self, *args, offline=True):
        environment = os.environ.copy()
        environment.update({
            "VERITAS_UV": str(UV),
            "UV_CACHE_DIR": str(UV_CACHE),
            "UV_PYTHON_INSTALL_DIR": str(PYTHON_INSTALL),
        })
        if offline:
            environment["UV_OFFLINE"] = "1"
        else:
            environment.pop("UV_OFFLINE", None)
            synced = subprocess.run(
                [str(VERITAS), "sync", str(self.veritas_project)],
                cwd=self.root,
                env=environment,
                text=True,
                capture_output=True,
                timeout=60,
            )
            self.assertEqual(synced.returncode, 0, synced.stderr)
            environment["UV_OFFLINE"] = "1"
        return subprocess.run(
            [str(VERITAS), "run", str(self.veritas_project), *args],
            cwd=self.root,
            env=environment,
            text=True,
            capture_output=True,
            timeout=60,
        )

    def run_python(self, *args, offline=True):
        interpreter = self.python_project / ".veritas/python-bridge/.venv/bin/python3"
        if not interpreter.exists():
            environment = os.environ.copy()
            environment.update({"VERITAS_UV": str(UV), "UV_CACHE_DIR": str(UV_CACHE), "UV_PYTHON_INSTALL_DIR": str(PYTHON_INSTALL)})
            if offline:
                environment["UV_OFFLINE"] = "1"
            else:
                environment.pop("UV_OFFLINE", None)
            synced = subprocess.run([str(VERITAS), "sync", str(self.python_project)], cwd=self.root, env=environment, text=True, capture_output=True, timeout=60)
            self.assertEqual(synced.returncode, 0, synced.stderr)
        return subprocess.run(
            [str(interpreter), str(self.python_project / "python_baseline.py"), *args],
            cwd=self.python_project,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def compare(self, args, expected_stdout, expected_status=0):
        veritas = self.run_veritas(*args)
        python = self.run_python(*args)
        self.assertEqual(veritas.returncode, expected_status, veritas.stderr)
        self.assertEqual(python.returncode, expected_status, python.stderr)
        self.assertEqual(veritas.stdout, expected_stdout)
        self.assertEqual(python.stdout, expected_stdout)
        self.assertEqual(veritas.stderr, python.stderr)
        self.assertEqual((self.veritas_project / "inbox.json").read_bytes(), (self.python_project / "inbox.json").read_bytes())
        return veritas

    def test_list_filter_summary_and_read_follow_up_match_python(self):
        self.compare(("list",), "[new] Café Tokyo — 2026-10-21 — https://example.test/a\n[read] Already read — 2026-10-20 — https://example.test/b\n[new] Latest item — 2026-10-22 — https://example.test/c\n")
        self.compare(("list", "unread"), "[new] Café Tokyo — 2026-10-21 — https://example.test/a\n[new] Latest item — 2026-10-22 — https://example.test/c\n")
        self.compare(("summary",), "Items:\n3\nUnread:\n2\nRead:\n1\n")
        self.compare(("read", "https://example.test/a"), "Marked read: https://example.test/a\n")
        self.compare(("summary",), "Items:\n3\nUnread:\n1\nRead:\n2\n")
        self.compare(("unread", "https://example.test/a"), "Marked unread: https://example.test/a\n")

    def test_unknown_link_and_bad_arguments_have_stable_results(self):
        self.compare(("read", "https://example.test/missing"), "No inbox item: https://example.test/missing\n", 1)
        self.compare(("unknown",), "Unknown command: unknown\n", 2)
        self.compare(("list", "extra"), "Usage: list [unread]\n", 2)

    def test_ungranted_refresh_host_reports_same_failure_as_python(self):
        self.compare(
            ("refresh", "http://localhost:1/not-contacted", "http://localhost:2/also-not-contacted"),
            "New items:\n0\nTotal items:\n3\nFeed errors:\nhttp://localhost:1/not-contacted — R_NETWORK_DENIED: network access to this host is not granted\nhttp://localhost:2/also-not-contacted — R_NETWORK_DENIED: network access to this host is not granted\n\n",
            1,
        )

    def test_refresh_fetches_multiple_feeds_concurrently_with_python_parity(self):
        for project in (self.veritas_project, self.python_project):
            state_path = project / "inbox.json"
            state = json.loads(state_path.read_text(encoding="utf-8"))
            state["entries"][0]["read"] = True
            state_path.write_text(json.dumps(state, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")

        server = ThreadingHTTPServer(("127.0.0.1", 0), SlowFeedHandler)
        server.daemon_threads = True
        server_thread = __import__("threading").Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        try:
            host, port = server.server_address
            urls = (f"http://{host}:{port}/feed-a.xml", f"http://{host}:{port}/feed-b.xml")
            expected = "New items:\n1\nTotal items:\n4\n"
            with SlowFeedHandler.stats_lock:
                SlowFeedHandler.peak_active = 0
            veritas = self.run_veritas("refresh", *urls, offline=False)
            veritas_peak = SlowFeedHandler.peak_active
            self.assertEqual(veritas.returncode, 0, veritas.stderr)
            self.assertEqual(veritas.stdout, expected)
            self.assertGreaterEqual(veritas_peak, 2, f"Veritas requests were serial (peak={veritas_peak})")

            with SlowFeedHandler.stats_lock:
                SlowFeedHandler.peak_active = 0
            python = self.run_python("refresh", *urls, offline=False)
            python_peak = SlowFeedHandler.peak_active
            self.assertEqual(python.returncode, 0, python.stderr)
            self.assertEqual(python.stdout, expected)
            self.assertGreaterEqual(python_peak, 2, f"Python baseline requests were serial (peak={python_peak})")
            self.assertEqual(veritas.stderr, python.stderr)
            self.assertEqual((self.veritas_project / "inbox.json").read_bytes(), (self.python_project / "inbox.json").read_bytes())
            state = json.loads((self.veritas_project / "inbox.json").read_text(encoding="utf-8"))
            by_link = {entry["link"]: entry for entry in state["entries"]}
            self.assertEqual(by_link["https://example.test/a"]["title"], "A Refreshed Café")
            self.assertTrue(by_link["https://example.test/a"]["read"])
            self.assertFalse(by_link["https://example.test/d"]["read"])
        finally:
            server.shutdown()
            server.server_close()
            server_thread.join(timeout=2)

    def test_feed_import_uses_managed_parser_and_preserves_read_state(self):
        for project in (self.veritas_project, self.python_project):
            (project / "feed.xml").write_text(FEED, encoding="utf-8")
            state_path = project / "inbox.json"
            state = json.loads(state_path.read_text(encoding="utf-8"))
            state["entries"][0]["read"] = True
            state_path.write_text(json.dumps(state, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
        veritas = self.run_veritas("import-feed", "feed.xml")
        python = self.run_python("import-feed", "feed.xml")
        expected = "New items:\n1\nTotal items:\n4\n"
        self.assertEqual(veritas.returncode, 0, veritas.stderr)
        self.assertEqual(python.returncode, 0, python.stderr)
        self.assertEqual(veritas.stdout, expected)
        self.assertEqual(python.stdout, expected)
        self.assertEqual(veritas.stderr, python.stderr)
        self.assertEqual((self.veritas_project / "inbox.json").read_bytes(), (self.python_project / "inbox.json").read_bytes())
        stored = json.loads((self.veritas_project / "inbox.json").read_text(encoding="utf-8"))
        by_link = {entry["link"]: entry for entry in stored["entries"]}
        self.assertEqual(by_link["https://example.test/a"]["title"], "A Refreshed Café")
        self.assertTrue(by_link["https://example.test/a"]["read"])
        self.assertFalse(by_link["https://example.test/d"]["read"])

    def test_built_artifact_runs_after_source_is_removed(self):
        for project in (self.veritas_project, self.python_project):
            (project / "feed.xml").write_text(FEED, encoding="utf-8")
        environment = os.environ.copy()
        environment.update({"VERITAS_UV": str(UV), "UV_CACHE_DIR": str(UV_CACHE), "UV_PYTHON_INSTALL_DIR": str(PYTHON_INSTALL), "UV_OFFLINE": "1"})
        built = subprocess.run([str(VERITAS), "build", str(self.veritas_project)], cwd=self.root, env=environment, text=True, capture_output=True, timeout=60)
        self.assertEqual(built.returncode, 0, built.stderr)
        self.assertIn("artifact format 30", built.stdout)
        (self.veritas_project / "main.vr").unlink()
        rmtree(self.veritas_project / "src")

        veritas = subprocess.run([str(VERITAS), "run", str(self.veritas_project / "main.vrb"), "import-feed", "feed.xml"], cwd=self.veritas_project, env=environment, text=True, capture_output=True, timeout=60)
        python = self.run_python("import-feed", "feed.xml")
        self.assertEqual(veritas.returncode, 0, veritas.stderr)
        self.assertEqual(python.returncode, 0, python.stderr)
        self.assertEqual(veritas.stdout, python.stdout)
        self.assertEqual(veritas.stderr, python.stderr)
        self.assertEqual((self.veritas_project / "inbox.json").read_bytes(), (self.python_project / "inbox.json").read_bytes())


if __name__ == "__main__":
    unittest.main()
