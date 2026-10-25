from __future__ import annotations

import json
import os
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "work/native-package-analysis"
SOURCE = PROJECT / "column_analysis.vr"
VERITAS = ROOT / "target/release/veritas"
ROWS = [
    {"value": (index * 7919) % 50_003, "tag": f"é-{index % 13}", "department": f"dept-{index % 5}"}
    for index in range(100_000)
]
(PROJECT / "dataset.json").write_text(
    json.dumps({"rows": ROWS}, ensure_ascii=False, separators=(",", ":")),
    encoding="utf-8",
)

build = subprocess.run(
    [str(VERITAS), "build", str(SOURCE), "-o", str(PROJECT / "column_analysis.vrb")],
    capture_output=True,
    text=True,
)
if build.returncode:
    raise SystemExit(f"Veritas build failed:\n{build.stderr}")

samples = {"veritas": [], "python": []}
environment = os.environ.copy()
environment["PYTHONDONTWRITEBYTECODE"] = "1"
for pair in range(16):
    order = ("veritas", "python") if pair % 2 == 0 else ("python", "veritas")
    for language in order:
        report = PROJECT / f"column-{language}-report.json"
        report.unlink(missing_ok=True)
        if language == "veritas":
            command = [str(VERITAS), "run", "column_analysis.vrb", "dataset.json", report.name, "tag"]
        else:
            command = ["python3", "-B", "reference.py", "dataset.json", report.name, "tag"]
        started = time.perf_counter()
        result = subprocess.run(
            command,
            cwd=PROJECT,
            env=environment,
            capture_output=True,
            text=True,
            timeout=120,
        )
        samples[language].append(time.perf_counter() - started)
        if result.returncode:
            raise SystemExit(f"{language} execution failed:\n{result.stdout}\n{result.stderr}")
        if language == "veritas":
            veritas_stdout = result.stdout
        elif result.stdout != veritas_stdout:
            raise SystemExit(f"stdout differs: {result.stdout!r} != {veritas_stdout!r}")
    if (PROJECT / "column-veritas-report.json").read_bytes() != (PROJECT / "column-python-report.json").read_bytes():
        raise SystemExit(f"report bytes differ after pair {pair}")

summary = {
    "status": "author-run-json-columns-native-reduction-parity",
    "rows": len(ROWS),
    "distinct_categories": 13,
    "pairs": 16,
    "median_seconds": {language: statistics.median(values) for language, values in samples.items()},
    "samples_seconds": samples,
    "report_bytes_and_stdout_match": "passed for every pair",
    "measurement_scope": "source-tree CLI, includes launch and input/output, author-run only",
}
(ROOT / "work/native-package-analysis-column-timing.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
