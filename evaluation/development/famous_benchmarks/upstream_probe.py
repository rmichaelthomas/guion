"""Run selected unmodified pyperformance workloads without its pyperf CLI."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time
import types


ROOT = Path(__file__).resolve().parents[3]
SOURCE_ROOT = (
    ROOT / "evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0"
)
SOURCE_COMMIT = "2ab96f98f98274d07ebfdfd66e4f021723179706"
EXPECTED_HASHES = {
    "bm_richards/run_benchmark.py": (
        "a4512668525331960c54043b5150a3fff92badaeaba850a941893ac69a1028d8"
    ),
    "bm_nbody/run_benchmark.py": (
        "d1385e816d7cfea361b7915e2cf70138cd6b84f40df8bd5152638851f7bcac2b"
    ),
}


def load_benchmark(module_name: str, source_path: Path):
    spec = importlib.util.spec_from_file_location(module_name, source_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load benchmark source: {source_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    richards_path = SOURCE_ROOT / "bm_richards.py"
    nbody_path = SOURCE_ROOT / "bm_nbody.py"
    paths = {
        "bm_richards/run_benchmark.py": richards_path,
        "bm_nbody/run_benchmark.py": nbody_path,
    }
    source_hashes = {
        name: hashlib.sha256(path.read_bytes()).hexdigest()
        for name, path in paths.items()
    }
    if source_hashes != EXPECTED_HASHES:
        raise RuntimeError(f"pinned pyperformance source hash mismatch: {source_hashes}")

    # pyperf supplies command-line and timing facilities. The benchmark source
    # functions themselves need only perf_counter, so avoid an extra dependency.
    sys.modules["pyperf"] = types.SimpleNamespace(perf_counter=time.perf_counter, Runner=None)

    richards = load_benchmark("pinned_pyperf_richards", richards_path)
    richards.taskWorkArea = richards.TaskWorkArea()
    if not richards.Richards().run(1):
        raise RuntimeError("official Richards canonical-count check failed")
    richards_result = {
        "iterations": 1,
        "hold_count": richards.taskWorkArea.holdCount,
        "packet_count": richards.taskWorkArea.qpktCount,
    }

    nbody = load_benchmark("pinned_pyperf_nbody", nbody_path)
    nbody.offset_momentum(nbody.BODIES[nbody.DEFAULT_REFERENCE])
    initial_energy = nbody.report_energy()
    nbody.advance(0.01, nbody.DEFAULT_ITERATIONS)
    final_energy = nbody.report_energy()
    nbody_result = {
        "iterations": nbody.DEFAULT_ITERATIONS,
        "reference": nbody.DEFAULT_REFERENCE,
        "energy_values": [initial_energy, final_energy],
    }

    print(
        json.dumps(
            {
                "pyperformance_version": "1.14.0",
                "source_commit": SOURCE_COMMIT,
                "python_version": sys.version,
                "harness": "unmodified benchmark workload functions; pyperf CLI wrapper omitted",
                "source_sha256": source_hashes,
                "richards": richards_result,
                "nbody": nbody_result,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
