"""End-to-end checks for bounded API requests, pagination and restartable state."""

from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
from shutil import copytree, ignore_patterns, rmtree
import subprocess
import tempfile
from threading import Thread
import unittest
from urllib.parse import parse_qs, urlparse


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "evaluation/development/api_sync"
if not PROJECT.is_dir():
    PROJECT = ROOT / "examples/api_sync"
DEFAULT_VERITAS = ROOT / "veritas" if (ROOT / "veritas").is_file() else ROOT / "target/debug/veritas"
VERITAS = Path(os.environ.get("VERITAS_BINARY", DEFAULT_VERITAS))
TOKEN = "test-only-secret-value"


class ApiHandler(BaseHTTPRequestHandler):
    page_two_failures = 0
    page_two_failure_status = 503
    acknowledgement_failures = 0
    requests = []

    def do_GET(self):
        self._record()
        query = parse_qs(urlparse(self.path).query)
        page = int(query.get("page", ["1"])[0])
        if page == 1:
            self._reply(200, {"items": [{"id": "A", "value": "café"}], "next": 2})
        elif page == 2:
            if type(self).page_two_failures:
                type(self).page_two_failures -= 1
                self._reply(type(self).page_two_failure_status, {"error": "retry"})
            else:
                self._reply(200, {"items": [{"id": "B", "value": 42}], "next": None})
        else:
            self._reply(404, {"error": "unknown page"})

    def do_POST(self):
        self._record()
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length).decode("utf-8")
        type(self).requests[-1]["body"] = body
        if self.path == "/ack" and type(self).acknowledgement_failures:
            type(self).acknowledgement_failures -= 1
            self._reply(503, {"error": "retry"})
        else:
            self._reply(200, {"accepted": True})

    def _record(self):
        type(self).requests.append({
            "method": self.command,
            "path": self.path,
            "authorization": self.headers.get("Authorization"),
            "body": "",
        })

    def _reply(self, status, value):
        body = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        pass


class ApiSyncAcceptance(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="veritas-api-sync-")
        self.root = Path(self.temp.name)
        self.veritas_project = self._copy("veritas")
        self.python_project = self._copy("python")

    def tearDown(self):
        self.temp.cleanup()

    def _copy(self, name):
        target = self.root / name / "api_sync"
        target.parent.mkdir(parents=True)
        copytree(PROJECT, target, ignore=ignore_patterns("__pycache__", ".venv"))
        return target

    def _server(self, page_two_failures=0, acknowledgement_failures=0, page_two_failure_status=503):
        ApiHandler.page_two_failures = page_two_failures
        ApiHandler.page_two_failure_status = page_two_failure_status
        ApiHandler.acknowledgement_failures = acknowledgement_failures
        ApiHandler.requests = []
        server = ThreadingHTTPServer(("127.0.0.1", 0), ApiHandler)
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        return server, thread, f"http://127.0.0.1:{server.server_port}"

    def _run_veritas(self, base_url, start_page=None):
        environment = os.environ.copy()
        environment["VERITAS_API_TOKEN"] = TOKEN
        arguments = [str(VERITAS), "run", str(self.veritas_project), base_url]
        if start_page is not None:
            arguments.append(start_page)
        return subprocess.run(
            arguments,
            cwd=self.root,
            env=environment,
            text=True,
            capture_output=True,
            timeout=30,
        )
    def _run_python(self, base_url, start_page=None):
        environment = os.environ.copy()
        environment["VERITAS_API_TOKEN"] = TOKEN
        arguments = ["python3", str(self.python_project / "reference.py"), base_url]
        if start_page is not None:
            arguments.append(start_page)
        return subprocess.run(
            arguments,
            cwd=self.python_project,
            env=environment,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def _assert_pair(self, veritas, python):
        self.assertEqual(veritas.returncode, python.returncode, veritas.stderr)
        self.assertEqual(veritas.stdout, python.stdout)
        self.assertEqual(veritas.stderr, python.stderr)
        self.assertNotIn(TOKEN, veritas.stdout + veritas.stderr)
        self.assertNotIn(TOKEN, python.stdout + python.stderr)
        state_v = self.veritas_project / "sync-state.json"
        state_py = self.python_project / "sync-state.json"
        self.assertEqual(state_v.exists(), state_py.exists())
        if state_v.exists():
            self.assertEqual(state_v.read_bytes(), state_py.read_bytes())

    def test_paginates_retries_transient_status_and_resumes_without_refetching(self):
        server, thread, base_url = self._server()
        try:
            first = self._run_veritas(base_url)
            reference = self._run_python(base_url)
            self._assert_pair(first, reference)
            self.assertEqual(first.returncode, 0, first.stderr)
            state = json.loads((self.veritas_project / "sync-state.json").read_text())
            self.assertEqual(state, {
                "next_page": None,
                "records": {"A": {"id": "A", "value": "café"}, "B": {"id": "B", "value": 42}},
            })
            self.assertEqual(sum(item["path"].startswith("/records?page=2") for item in ApiHandler.requests), 2)
            ack_requests = [item for item in ApiHandler.requests if item["path"] == "/ack"]
            self.assertEqual(len(ack_requests), 2)
            self.assertTrue(all(item["method"] == "POST" for item in ack_requests))
            self.assertTrue(all(item["authorization"] == f"Bearer {TOKEN}" for item in ApiHandler.requests))
            self.assertEqual(json.loads(ack_requests[0]["body"]), {"records_seen": 2})

            # A second process starts from the persisted terminal cursor and only retries the idempotent ack.
            before = len(ApiHandler.requests)
            second = self._run_veritas(base_url)
            second_reference = self._run_python(base_url)
            self._assert_pair(second, second_reference)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertTrue(all(item["path"] == "/ack" for item in ApiHandler.requests[before:]))
        finally:
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()

    def test_operator_can_start_at_a_selected_page(self):
        server, thread, base_url = self._server()
        try:
            result = self._run_veritas(base_url, "2")
            reference = self._run_python(base_url, "2")
            self._assert_pair(result, reference)
            self.assertEqual(result.returncode, 0, result.stderr)
            state = json.loads((self.veritas_project / "sync-state.json").read_text())
            self.assertEqual(state, {
                "next_page": None,
                "records": {"B": {"id": "B", "value": 42}},
            })
            self.assertFalse(any(item["path"].startswith("/records?page=1") for item in ApiHandler.requests))
            self.assertEqual(sum(item["path"].startswith("/records?page=2") for item in ApiHandler.requests), 2)
        finally:
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()

    def test_permanent_page_failure_keeps_completed_page_for_later_resume(self):
        server, thread, base_url = self._server(page_two_failures=1, page_two_failure_status=400)
        try:
            first = self._run_veritas(base_url)
            self.assertEqual(first.returncode, 1)
            state = json.loads((self.veritas_project / "sync-state.json").read_text())
            self.assertEqual(state["next_page"], 2)
            self.assertEqual(list(state["records"]), ["A"])
            self.assertEqual(state, {
                "next_page": 2,
                "records": {"A": {"id": "A", "value": "café"}},
            })
            (self.python_project / "sync-state.json").write_text(
                json.dumps(state, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
                encoding="utf-8",
            )
            second = self._run_veritas(base_url, "1")
            second_reference = self._run_python(base_url, "1")
            self._assert_pair(second, second_reference)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(sum(item["path"].startswith("/records?page=1") for item in ApiHandler.requests), 1)
            state = json.loads((self.veritas_project / "sync-state.json").read_text())
            self.assertEqual(set(state["records"]), {"A", "B"})
        finally:
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()

    def test_transient_server_failures_are_retried_with_runtime_auth_and_post_body(self):
        server, thread, base_url = self._server(page_two_failures=1, acknowledgement_failures=1)
        try:
            result = self._run_veritas(base_url)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn(TOKEN, result.stdout + result.stderr)
            self.assertEqual(sum(item["path"] == "/records?page=2" for item in ApiHandler.requests), 2)
            ack = [item for item in ApiHandler.requests if item["path"] == "/ack"]
            self.assertEqual(len(ack), 2)
            self.assertTrue(all(item["authorization"] == f"Bearer {TOKEN}" for item in ApiHandler.requests))
            self.assertEqual(json.loads(ack[0]["body"]), {"records_seen": 2})
        finally:
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()

    def test_current_artifact_runs_after_source_removal(self):
        server, thread, base_url = self._server()
        try:
            built = subprocess.run(
                [str(VERITAS), "build", str(self.veritas_project)],
                cwd=self.root,
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            (self.veritas_project / "main.vr").unlink()
            rmtree(self.veritas_project / "src")
            run = subprocess.run(
                [str(VERITAS), "run", str(self.veritas_project / "main.vrb"), base_url],
                cwd=self.root,
                env={**os.environ, "VERITAS_API_TOKEN": TOKEN},
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertEqual(json.loads((self.veritas_project / "sync-state.json").read_text())["next_page"], None)
        finally:
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()


if __name__ == "__main__":
    unittest.main()
