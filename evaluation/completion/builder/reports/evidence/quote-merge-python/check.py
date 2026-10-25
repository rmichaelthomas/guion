"""Independent acceptance runner: local fixture traffic only."""
import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from http.server import ThreadingHTTPServer

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / "evidence"
spec = importlib.util.spec_from_file_location("fixture", ROOT / "fixtures/quote_server.py")
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def main():
    stage = sys.argv[1]
    program = Path(sys.argv[2])
    fixture_process = subprocess.Popen([sys.executable, str(ROOT / "fixtures/quote_server.py")],
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    fixture_port = int(fixture_process.stdout.readline())
    base = f"http://127.0.0.1:{fixture_port}"
    log = []
    checks = []
    active = peak = 0
    lock = threading.Lock()
    paired = threading.Event()

    class Instrumented(fixture.Handler):
        def do_GET(self):
            nonlocal active, peak
            if self.path == "/gate":
                with lock:
                    active += 1
                    peak = max(peak, active)
                    if active >= 2:
                        paired.set()
                paired.wait(2)
                time.sleep(0.08)
                try:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b'{"quotes":[]}')
                finally:
                    with lock:
                        active -= 1
            elif self.path == "/invalid-schema":
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b'{"quotes":[{"carrier":"Aster","service":"bad","price_cents":true,"days":1}]}')
            elif self.path == "/tie":
                body = {"quotes": [
                    {"carrier": "Zed", "service": "z", "price_cents": 900, "days": 1},
                    {"carrier": "Aster", "service": "alpha", "price_cents": 1100, "days": 2},
                    {"carrier": "Aster", "service": "alpha", "price_cents": 1100, "days": 1}]}
                self.send_response(200)
                self.end_headers()
                self.wfile.write(json.dumps(body).encode())
            else:
                super().do_GET()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Instrumented)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    other = f"http://127.0.0.1:{server.server_port}"

    def run(name, urls, options=(), expected_exit=0):
        feeds = EVIDENCE / f"{stage}-{name}-feeds.txt"
        feeds.write_text("\n".join(urls) + "\n", encoding="utf-8")
        command = [sys.executable, str(program), str(feeds), *options]
        started = time.perf_counter()
        result = subprocess.run(command, capture_output=True, text=True, timeout=8, cwd=program.parent)
        elapsed = time.perf_counter() - started
        entry = {"name": name, "command": command, "cwd": str(program.parent), "exit_code": result.returncode,
                 "stdout": result.stdout, "stderr": result.stderr, "elapsed_seconds": elapsed}
        log.append(entry)
        (EVIDENCE / f"{stage}-commands-results.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
        assert result.returncode == expected_exit, entry
        return json.loads(result.stdout) if expected_exit == 0 else None

    def check(name, condition):
        checks.append({"name": name, "passed": bool(condition)})
        assert condition, name

    try:
        urls = [base + path for path in ("/feed-a", "/feed-b", "/feed-c", "/slow-503", "/malformed")]
        baseline = run("baseline", urls)
        expected = [
            {"carrier": "Cedar", "service": "ground", "price_cents": 900, "days": 5},
            {"carrier": "Aster", "service": "air", "price_cents": 1100, "days": 2},
            {"carrier": "Birch", "service": "ground", "price_cents": 1900, "days": 3}]
        check("best quotes and ordering", baseline["quotes"] == expected)
        check("partial 503 and malformed isolation", baseline["successful_feeds"] == 3 and
              baseline["failed_feeds"] == [{"url": urls[3], "status": 503}, {"url": urls[4], "status": "malformed"}])
        repeated = run("repeat", urls)
        check("byte stable repeated output", log[0]["stdout"] == log[1]["stdout"])
        reverse = run("reversed", list(reversed(urls)))
        check("stable winner ordering independent of feed order", reverse["quotes"] == expected)
        malformed = run("schema", [base + "/feed-a", other + "/invalid-schema"])
        check("schema isolation", malformed["successful_feeds"] == 1 and malformed["failed_feeds"][0]["status"] == "malformed")
        timeout = run("timeout", [base + "/feed-a", base + "/slow-503"], ["--timeout", "0.03"])
        check("finite timeout reporting", timeout["successful_feeds"] == 1 and timeout["failed_feeds"][0]["status"] == "timeout")
        errors = run("errors", [base + "/missing", "file:///etc/hosts", "http://127.0.0.1:0/error"])
        check("HTTP and network error reporting", [x["status"] for x in errors["failed_feeds"]] == [404, "invalid_url", "network_error"])
        concurrency = run("concurrency", [other + "/gate"] * 6, ["--workers", "2"])
        check("observed concurrent requests exactly bounded at 2", paired.is_set() and peak == 2 and concurrency["successful_feeds"] == 6)
        tied = run("tie", [other + "/tie", base + "/feed-b"])
        check("deterministic ties", [q["carrier"] for q in tied["quotes"]] == ["Cedar", "Zed", "Aster"] and tied["quotes"][2]["service"] == "air")
        run("bad-workers", [], ["--workers", "1"], expected_exit=2)
        run("bad-timeout", [], ["--timeout", "nan"], expected_exit=2)
        check("invalid options fail clearly", True)
        empty = run("empty", [])
        check("empty feed file", empty == {"quotes": [], "successful_feeds": 0, "failed_feeds": []})
        if stage != "initial":
            filtered = run("max-days", urls, ["--max-days", "2"])
            check("followup filters quotes and preserves counts", filtered["quotes"] == [expected[1],
                  {"carrier": "Cedar", "service": "air", "price_cents": 1300, "days": 1},
                  {"carrier": "Birch", "service": "air", "price_cents": 2500, "days": 1}]
                  and filtered["successful_feeds"] == 3 and filtered["failed_feeds"] == baseline["failed_feeds"])
            zero = run("max-days-zero", urls, ["--max-days", "0"])
            check("zero day filter keeps feed counts", not zero["quotes"] and zero["successful_feeds"] == 3)
            run("negative-days", [], ["--max-days", "-1"], expected_exit=2)
            # Compare the initial version using exactly the same feeds and port.
            initial_program = ROOT / "initial/quote_merge.py"
            original = subprocess.run([sys.executable, str(initial_program), str(EVIDENCE / f"{stage}-baseline-feeds.txt")], capture_output=True, text=True, timeout=8)
            check("followup preserves default output byte for byte", original.returncode == 0 and original.stdout == log[0]["stdout"])
        summary = {"stage": stage, "checks": checks, "peak_concurrency": peak,
                   "measured_baseline_seconds": log[0]["elapsed_seconds"], "passed": True,
                   "utc": datetime.datetime.now(datetime.timezone.utc).isoformat()}
        (EVIDENCE / f"{stage}-checks.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print(json.dumps(summary, indent=2))
    finally:
        fixture_process.terminate()
        fixture_process.wait(timeout=3)
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
