#!/usr/bin/env python3
"""Run one unmodified pinned pyperformance Richards workload iteration."""

from __future__ import annotations

import hashlib
import sys
import time
import types

from upstream_probe import EXPECTED_HASHES, SOURCE_ROOT, load_benchmark


def main() -> int:
    source = SOURCE_ROOT / "bm_richards.py"
    expected = EXPECTED_HASHES["bm_richards/run_benchmark.py"]
    actual = hashlib.sha256(source.read_bytes()).hexdigest()
    if actual != expected:
        raise RuntimeError(f"pinned Richards source hash mismatch: {actual}")
    sys.modules["pyperf"] = types.SimpleNamespace(perf_counter=time.perf_counter, Runner=None)
    richards = load_benchmark("pinned_pyperf_richards", source)
    richards.taskWorkArea = richards.TaskWorkArea()
    if not richards.Richards().run(1):
        raise RuntimeError("pinned Richards canonical-count check failed")
    print(richards.taskWorkArea.holdCount)
    print(richards.taskWorkArea.qpktCount)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
