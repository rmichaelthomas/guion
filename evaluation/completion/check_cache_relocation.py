#!/usr/bin/env python3
"""Verify the warmed source caches used by the delivery-mode comparison."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.compare_real_tasks import APP_IDS, APPS  # noqa: E402


def run(binary: Path, project: Path) -> dict:
    environment = os.environ.copy()
    environment["GUION_FILE_ROOT"] = str(project)
    result = subprocess.run([str(binary), "run", "--json", str(project)], env=environment,
                            capture_output=True, text=True, timeout=90)
    return json.loads(result.stdout)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session"
    scratch.mkdir(parents=True, exist_ok=True)
    results = {}
    with tempfile.TemporaryDirectory(prefix="cache-relocation-", dir=scratch) as tmp:
        for name in APP_IDS:
            first = Path(tmp) / name
            second = Path(tmp) / (name + "-relocated")
            shutil.copytree(APPS / name, first, ignore=shutil.ignore_patterns("dist", ".guion"))
            initial = run(binary, first).get("source_cache")
            shutil.copytree(first, second)
            relocated = run(binary, second).get("source_cache")
            results[name] = {"initial": initial, "relocated": relocated}
            if initial != "miss" or relocated != "hit":
                raise RuntimeError(f"{name} cache relocation failed: {results[name]}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"passed": True, "applications": results}, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": True, "applications": len(results)}))


if __name__ == "__main__":
    main()
