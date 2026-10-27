"""Regression coverage for the lockfile auditor's reviewed scale boundary."""

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))

from evaluation.completion.performance.benchmark_lockfile_diagnostics import build_case  # noqa: E402
from evaluation.completion.compare_real_tasks import APPS  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


class LockfileScaleTests(unittest.TestCase):
    def test_8192_packages_produce_the_exact_sorted_diagnostic_report(self) -> None:
        configured_binary = os.environ.get("GUION_BINARY")
        binary = Path(configured_binary) if configured_binary else ROOT / "target/release/guion"
        if not binary.is_file():
            self.skipTest(f"release Guion binary is missing: {binary}")

        case = build_case(8192)
        scratch = ROOT / "target/perf-session/lockfile-scale-test"
        scratch.mkdir(parents=True, exist_ok=True)
        os.environ["TMPDIR"] = str(scratch)
        tempfile.tempdir = str(scratch)
        config = {
            "command": [str(binary.resolve()), "run", "--no-cache", "${candidate}", *case["command_args"]],
            "workspace_root_project": True,
            "env": {"GUION_FILE_ROOT": "${workspace}"},
        }
        outcome = run_case(case, config, APPS / "lockfile-auditor", timeout_seconds=90)
        self.assertTrue(
            outcome["passed"],
            json.dumps(
                {
                    "failures": outcome["failures"],
                    "stderr": outcome["stderr"],
                    "step_results": outcome["steps"],
                },
                indent=2,
            ),
        )


if __name__ == "__main__":
    unittest.main()
