#!/usr/bin/env python3
"""Output-checked paired short-source and local HTTP benchmarks for the core sprint."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import sys
import threading
from collections import Counter
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402

HERE = Path(__file__).resolve().parent
EXPECTED = ["hello", "42", "65536"]
PAYLOAD = json.dumps(
    {"count": 42, "message": "hello", "payload": "a" * 65536},
    separators=(",", ":"),
).encode("utf-8")
TINY_SOURCE = 'fn main() -> Int:\n    print("ready")\n    return 0\n'
TINY_PYTHON = 'print("ready")\n'


class FixtureHandler(BaseHTTPRequestHandler):
    counts: Counter[str]
    counts_lock: threading.Lock

    def do_GET(self) -> None:
        if self.path != "/json":
            self.send_error(404)
            return
        with self.counts_lock:
            self.counts[self.path] += 1
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(PAYLOAD)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(PAYLOAD)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_revision() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()


def guion_command(binary: Path, source: Path) -> list[str]:
    return [
        str(binary.resolve()), "run", "--json", "--no-cache",
        "--allow-http-host", "127.0.0.1", str(source),
    ]


def measure_short_source(
    binary: Path, candidate_binary: Path | None, python_binary: Path, project: Path, trials: int
) -> dict[str, object]:
    project.mkdir(parents=True, exist_ok=True)
    source = project / "tiny.gu"
    python_source = project / "tiny.py"
    source.write_text(TINY_SOURCE, encoding="utf-8")
    python_source.write_text(TINY_PYTHON, encoding="utf-8")
    runner.write_manifest(project, "sprint-tiny-source", 100_000)
    commands = {"guion_baseline": guion_command(binary, source)}
    if candidate_binary is not None:
        commands["guion_candidate"] = guion_command(candidate_binary, source)
    commands["python_reference"] = [str(python_binary), str(python_source)]
    samples, metadata = runner.sample_candidates(
        "sprint-tiny-source", commands, project, ["ready"], trials=trials
    )
    ratios = [
        guion / python
        for guion, python in zip(
            samples["guion_baseline"]["samples_seconds"],
            samples["python_reference"]["samples_seconds"],
            strict=True,
        )
    ]
    comparisons: dict[str, object] = {"baseline_over_python": runner.paired_median_ci(ratios)}
    if "guion_candidate" in samples:
        candidate_ratios = [
            candidate / baseline_sample
            for candidate, baseline_sample in zip(
                samples["guion_candidate"]["samples_seconds"],
                samples["guion_baseline"]["samples_seconds"],
                strict=True,
            )
        ]
        comparisons["candidate_over_baseline"] = runner.paired_median_ci(candidate_ratios)
    return {
        "commands": commands,
        "expected_output": ["ready"],
        "samples": samples,
        "comparisons": comparisons,
        **metadata,
    }


def measure_http(
    baseline_binary: Path,
    candidate_binary: Path | None,
    python_binary: Path,
    project: Path,
    trials: int,
) -> dict[str, object]:
    project.mkdir(parents=True, exist_ok=True)
    counts: Counter[str] = Counter()
    lock = threading.Lock()
    FixtureHandler.counts = counts
    FixtureHandler.counts_lock = lock
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, name="sprint-http-fixture", daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}/json"
    legacy_source = project / "http_legacy.gu"
    legacy_source.write_text(
        (HERE / "http_request_legacy.gu").read_text(encoding="utf-8").replace("__URL__", url),
        encoding="utf-8",
    )
    reference = HERE / "http_reference.py"
    commands = {
        "guion_baseline": guion_command(baseline_binary, legacy_source),
        "python_reference": [str(python_binary), str(reference), url],
    }
    source_hashes = {"legacy": digest(legacy_source), "python_reference": digest(reference)}
    typed_source: Path | None = None
    if candidate_binary is not None:
        typed_source = project / "http_typed.gu"
        typed_source.write_text(
            (HERE / "http_response_json.gu").read_text(encoding="utf-8").replace("__URL__", url),
            encoding="utf-8",
        )
        commands["guion_candidate"] = guion_command(candidate_binary, typed_source)
        source_hashes["typed"] = digest(typed_source)
    runner.write_manifest(project, "sprint-http-local-json", 30_000_000)
    try:
        samples, metadata = runner.sample_candidates(
            "sprint-http-local-json", commands, project, EXPECTED, trials=trials
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    expected_count = (runner.WARMUPS + trials) * len(commands)
    actual_count = counts["/json"]
    if actual_count != expected_count:
        raise RuntimeError(f"fixture observed {actual_count} /json requests; expected {expected_count}")
    comparisons: dict[str, object] = {}
    python_samples = samples["python_reference"]["samples_seconds"]
    for lane in ("guion_baseline", "guion_candidate"):
        if lane not in samples:
            continue
        ratios = [
            guion / python
            for guion, python in zip(samples[lane]["samples_seconds"], python_samples, strict=True)
        ]
        comparisons[lane + "_over_python"] = runner.paired_median_ci(ratios)
    if "guion_candidate" in samples:
        ratios = [
            candidate / baseline
            for candidate, baseline in zip(
                samples["guion_candidate"]["samples_seconds"],
                samples["guion_baseline"]["samples_seconds"],
                strict=True,
            )
        ]
        comparisons["candidate_over_baseline"] = runner.paired_median_ci(ratios)
    return {
        "url_host_grant": "127.0.0.1",
        "fixture_url_path": "/json",
        "fixture_response_status": 200,
        "fixture_response_body_bytes": len(PAYLOAD),
        "fixture_response_body_sha256": hashlib.sha256(PAYLOAD).hexdigest(),
        "fixture_request_count": actual_count,
        "expected_fixture_request_count": expected_count,
        "expected_output": EXPECTED,
        "source_sha256": source_hashes,
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        **metadata,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-binary", required=True, type=Path)
    parser.add_argument("--candidate-binary", type=Path)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--scratch-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=15)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three paired trials are required")
    baseline = args.baseline_binary.resolve(strict=True)
    candidate = args.candidate_binary.resolve(strict=True) if args.candidate_binary else None
    python_binary = args.python.resolve(strict=True)
    scratch = args.scratch_dir.resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    project = scratch / "core-workloads"
    project.mkdir(parents=True, exist_ok=True)
    short = measure_short_source(baseline, candidate, python_binary, project / "short-source", args.trials)
    http = measure_http(baseline, candidate, python_binary, project / "http", args.trials)
    report = {
        "schema_version": 1,
        "kind": "guion-core-sprint-short-and-http-benchmarks",
        "status": "passed",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": git_revision(),
        "python": {
            "version": subprocess.run(
                [str(python_binary), "--version"], capture_output=True, text=True, check=True
            ).stdout.strip(),
            "executable": str(python_binary),
        },
        "platform": {"platform": platform.platform(), "architecture": platform.machine()},
        "measurement": {
            "trials_per_arm": args.trials,
            "warmups_per_arm": runner.WARMUPS,
            "trial_seed": runner.TRIAL_SEED,
            "bootstrap_seed": runner.BOOTSTRAP_SEED,
            "bootstrap_resamples": runner.BOOTSTRAP_SAMPLES,
            "order": "randomized per paired trial by the existing performance runner",
            "timing": "process startup through output oracle; fixture server startup excluded",
            "cache": "Guion source cache bypassed with --no-cache; OS cache uncontrolled",
        },
        "binary": {
            "baseline_sha256": digest(baseline),
            "candidate_sha256": digest(candidate) if candidate else None,
        },
        "workloads": {"tiny-source": short, "http-local-json": http},
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "output": str(args.output), "http_requests": http["fixture_request_count"]}))


if __name__ == "__main__":
    main()
