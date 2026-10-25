from __future__ import annotations

import json
import os
from pathlib import Path
import statistics
import subprocess
import time
import tomllib

root = Path("/src")
package = root / "work/config-migrator-linux-pilot/moved"
config = package / "config.toml"
report = root / "work/config-migrator-linux-pilot/report.json"
data = json.loads(report.read_text())
baseline = [
    package / ".veritas/python-bridge/.venv/bin/python",
    "-B",
    root / "evaluation/pilot/author-self-review/config_migrator/python_main.py",
    "migrate",
    "--file",
    "config.toml",
]
veritas = [package / "run", "migrate", "--file", "config.toml"]
env = os.environ.copy()
env["PYTHONDONTWRITEBYTECODE"] = "1"
source = '# owner comment\nversion = 2\nname = "demo"\n[sync]\ntimeout_seconds = 45\n'
samples = {"veritas": [], "python": []}
for index in range(8):
    order = (("veritas", veritas), ("python", baseline)) if index % 2 == 0 else (("python", baseline), ("veritas", veritas))
    for name, command in order:
        config.write_text(source, encoding="utf-8")
        started = time.perf_counter()
        result = subprocess.run(command, cwd=package, env=env, capture_output=True, text=True)
        elapsed = time.perf_counter() - started
        if result.returncode != 0:
            raise SystemExit(f"{name} failed: {result.stderr}")
        parsed = tomllib.loads(config.read_text(encoding="utf-8"))
        assert parsed == {"version": 3, "name": "demo", "sync": {"timeout_seconds": 45, "parallelism": 4}}, parsed
        assert "# owner comment" in config.read_text(encoding="utf-8")
        samples[name].append(elapsed)
verify = subprocess.run([package / "veritas", "verify-package", package], cwd=package, env=env, capture_output=True, text=True)
if verify.returncode != 0:
    raise SystemExit(verify.stderr)
summary = {
    "status": "author-run-linux-warm-cache-timing",
    "target": data["target"],
    "package_directory_bytes": data["package_directory_bytes"],
    "pairs": 8,
    "veritas_median_seconds": statistics.median(samples["veritas"]),
    "python_median_seconds": statistics.median(samples["python"]),
    "ratio_veritas_over_python": statistics.median(samples["veritas"]) / statistics.median(samples["python"]),
    "samples_seconds": samples,
    "output_and_comment_checks": "passed after each invocation",
    "package_verification_after_runs": "passed",
    "environment": "Debian 12 ARM64 container, same bundled Python 3.14.8 and tomlkit 0.14.0, warm filesystem cache",
}
(root / "work/config-migrator-linux-timing.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
