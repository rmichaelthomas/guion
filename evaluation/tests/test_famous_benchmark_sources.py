"""Pinned official benchmark-source probes and their canonical outputs."""

import json
from pathlib import Path
import subprocess
import sys
import unittest

from evaluation.development.famous_benchmarks.runner import (
    load_pyperformance_source_report,
)


ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "evaluation/development/famous_benchmarks/upstream_probe.py"
RICHARDS_REFERENCE = ROOT / "evaluation/development/famous_benchmarks/richards_reference.py"
NATIVE_REPORT = ROOT / "evaluation/results/native-backend-probe-cranelift-0.135.0.json"


class FamousBenchmarkSourceTests(unittest.TestCase):
    def test_aggregate_report_loads_the_pinned_upstream_source_evidence(self):
        report = load_pyperformance_source_report()
        self.assertEqual(report["source_commit"], "2ab96f98f98274d07ebfdfd66e4f021723179706")
        self.assertEqual(report["richards"]["canonical_hold_count"], 9297)
        self.assertEqual(report["richards"]["canonical_packet_count"], 23246)
        self.assertNotIn("veritas_result", report["richards"])
        self.assertNotIn("veritas_to_python_ratio", report["nbody"])
        self.assertIn("python_median_seconds", report["nbody"])

    def test_pinned_pyperformance_sources_run_and_match_canonical_results(self):
        result = subprocess.run(
            [sys.executable, str(PROBE)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["pyperformance_version"], "1.14.0")
        self.assertEqual(
            report["source_commit"],
            "2ab96f98f98274d07ebfdfd66e4f021723179706",
        )
        self.assertEqual(report["richards"]["hold_count"], 9297)
        self.assertEqual(report["richards"]["packet_count"], 23246)
        self.assertEqual(report["nbody"]["iterations"], 20000)
        self.assertEqual(
            report["nbody"]["energy_values"],
            [-0.1690751638285245, -0.16908926275527172],
        )
        self.assertIn("bm_richards/run_benchmark.py", report["source_sha256"])
        self.assertIn("bm_nbody/run_benchmark.py", report["source_sha256"])

    def test_richards_reference_runs_the_pinned_workload_and_prints_canonical_counts(self):
        result = subprocess.run(
            [sys.executable, str(RICHARDS_REFERENCE)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "9297\n23246\n")

    def test_native_report_records_richards_parity_and_backend_scope(self):
        report = json.loads(NATIVE_REPORT.read_text(encoding="utf-8"))
        richards = report["richards_probe"]
        self.assertEqual(richards["correct_output"], ["9297", "23246"])
        self.assertEqual(
            richards["official_python_median_seconds"],
            richards["results"]["official_python_source"]["median_seconds"],
        )
        self.assertIn("complete Veritas Richards port", richards["native_backend_scope"])
        for execution in ("veritas_source", "veritas_artifact"):
            self.assertIn(execution, richards["native_backend_reports"])
            backend = richards["native_backend_reports"][execution]
            self.assertTrue(
                {
                    "main",
                    "route_packet",
                    "xor_d008",
                    "next_worker_count",
                }.issubset(backend["compiled_functions"]),
                backend,
            )
            self.assertEqual(backend["deoptimizations"], 0)

    def test_native_nbody_report_keeps_the_full_step_loop_in_native_code(self):
        report = json.loads(NATIVE_REPORT.read_text(encoding="utf-8"))
        nbody = report["nbody_probe"]
        for execution in ("veritas_source", "veritas_artifact"):
            backend = nbody["native_backend_reports"][execution]
            self.assertIn("advance", backend["compiled_functions"])
            self.assertIn("energy", backend["compiled_functions"])
            self.assertLessEqual(backend["calls"], 10, backend)


if __name__ == "__main__":
    unittest.main()
