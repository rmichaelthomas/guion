#!/usr/bin/env python3
"""Measure output-checked AllHereHTML builds at three input sizes."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import resource
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "evaluation/completion/applications/allherehtml"
SCRATCH = ROOT / "target/perf-session"


def worker(binary: Path, project: Path, mode: str) -> None:
    command = [str(binary), "run"]
    if mode == "uncached":
        command.append("--no-cache")
    command.append("AllHereHTML.gu")
    started = time.perf_counter()
    result = subprocess.run(command, cwd=project, capture_output=True, text=True, timeout=180)
    elapsed = time.perf_counter() - started
    output = project / "guion-showcase.html"
    expected = project / "expected-guion-showcase.html"
    correct = result.returncode == 0 and output.is_file() and output.read_bytes() == expected.read_bytes()
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if sys.platform != "darwin":
        peak *= 1024
    print(json.dumps({"seconds": elapsed, "peak_rss_bytes": peak,
                      "exit_code": result.returncode, "output_correct": correct,
                      "stdout": result.stdout, "stderr": result.stderr}))
    if not correct:
        raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--mode", choices=("uncached", "cached"))
    parser.add_argument("--trials", type=int, default=5)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.worker:
        worker(binary, args.worker, args.mode)
        return
    if args.output is None or args.trials < 2:
        parser.error("--output and at least two trials are required")
    SCRATCH.mkdir(parents=True, exist_ok=True)
    rng = random.Random(20261026)
    result = {"schema_version": 1, "kind": "allherehtml-source-scaling", "trials": args.trials,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "host": sys.platform, "os_file_cache": "uncontrolled", "sizes": {}}
    with tempfile.TemporaryDirectory(prefix="allhere-bench-", dir=SCRATCH) as tmp:
        for scale in (1, 2, 4):
            project = Path(tmp) / str(scale)
            shutil.copytree(FIXTURE, project)
            template = project / "guion-site.template.html"
            expected = project / "expected-guion-showcase.html"
            source = template.read_text(encoding="utf-8")
            page = expected.read_text(encoding="utf-8")
            added = "x" * (len(source) * (scale - 1))
            extra = "" if scale == 1 else "<!--\n" + "\n".join(
                added[i:i + 80] for i in range(0, len(added), 80)
            ) + "\n-->"
            template.write_text(source.replace("</body>", extra + "</body>", 1), encoding="utf-8")
            expected.write_text(page.replace("</body>", extra + "</body>", 1), encoding="utf-8")
            manifest = project / "guion.toml"
            manifest.write_text(manifest.read_text().replace("instruction_limit = 20000000",
                                                            f"instruction_limit = {20000000 * scale}"))
            samples = {"uncached": [], "cached": []}
            schedule = [mode for mode in samples for _ in range(args.trials)]
            rng.shuffle(schedule)
            for index, mode in enumerate(["uncached", "cached", *schedule]):
                proc = subprocess.run([sys.executable, __file__, "--binary", str(binary), "--worker", str(project),
                                       "--mode", mode], capture_output=True, text=True, timeout=200)
                if proc.returncode:
                    raise RuntimeError(f"scale {scale} {mode} failed: {proc.stdout} {proc.stderr}")
                observation = json.loads(proc.stdout)
                if index >= 2:
                    samples[mode].append(observation)
            result["sizes"][str(scale)] = {
                "template_bytes": template.stat().st_size,
                "expected_output_sha256": hashlib.sha256(expected.read_bytes()).hexdigest(),
                "samples": samples,
                "median_seconds": {mode: statistics.median(x["seconds"] for x in values) for mode, values in samples.items()},
                "median_peak_rss_bytes": {mode: statistics.median(x["peak_rss_bytes"] for x in values) for mode, values in samples.items()},
            }
            print(json.dumps({"scale": scale, "median_seconds": result["sizes"][str(scale)]["median_seconds"]}), flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
