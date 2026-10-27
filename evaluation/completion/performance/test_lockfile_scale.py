"""Regression coverage for the lockfile auditor's reviewed scale boundary."""

import json
import os
import shutil
import sys
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))

from evaluation.completion.performance.benchmark_lockfile_diagnostics import build_case  # noqa: E402
from evaluation.completion.compare_real_tasks import APPS  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


class LockfileScaleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        scratch_parent = ROOT / "target/perf-session"
        scratch_parent.mkdir(parents=True, exist_ok=True)
        cls._scratch = tempfile.TemporaryDirectory(prefix="lockfile-scale-tests-", dir=scratch_parent)
        cls.scratch = Path(cls._scratch.name)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._scratch.cleanup()

    def run_scale_case(self, package_count: int) -> dict:
        configured_binary = os.environ.get("GUION_BINARY")
        binary = Path(configured_binary) if configured_binary else ROOT / "target/release/guion"
        case = build_case(package_count)
        config = {
            "command": [str(binary.resolve()), "run", "--no-cache", "${candidate}", *case["command_args"]],
            "workspace_root_project": True,
            "env": {"GUION_FILE_ROOT": "${workspace}"},
        }
        with patch.dict(os.environ, {"TMPDIR": str(self.scratch)}), patch.object(
            tempfile, "tempdir", str(self.scratch)
        ):
            return run_case(case, config, APPS / "lockfile-auditor", timeout_seconds=90)

    def test_16384_case_expected_rows_use_lexical_order(self) -> None:
        case = build_case(16384)
        rows = case["checks"][0]["value"]["violations"]
        expected = sorted(rows, key=lambda item: (item["name"], item["rule"]))
        self.assertEqual(rows, expected)

    def test_16384_packages_produce_the_exact_sorted_diagnostic_report(self) -> None:
        outcome = self.run_scale_case(16384)
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

    def test_8192_packages_produce_the_exact_sorted_diagnostic_report(self) -> None:
        configured_binary = os.environ.get("GUION_BINARY")
        binary = Path(configured_binary) if configured_binary else ROOT / "target/release/guion"
        if not binary.is_file():
            self.skipTest(f"release Guion binary is missing: {binary}")

        outcome = self.run_scale_case(8192)
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

    def test_repeated_versions_do_not_construct_a_placeholder_record_per_package(self) -> None:
        configured_binary = os.environ.get("GUION_BINARY")
        binary = Path(configured_binary) if configured_binary else ROOT / "target/release/guion"
        if not binary.is_file():
            self.skipTest(f"release Guion binary is missing: {binary}")

        app = APPS / "lockfile-auditor"
        scratch = self.scratch / "profile"
        scratch.mkdir()
        case = build_case(8192)
        with tempfile.TemporaryDirectory(dir=scratch) as directory:
            project = Path(directory)
            for filename in ("main.vr", "veritas.toml", "veritas.lock"):
                shutil.copy2(app / filename, project / filename)
            for filename, content in case["files"].items():
                (project / filename).write_text(content, encoding="utf-8")

            command = [
                str(binary.resolve()),
                "run",
                "--no-cache",
                "--json",
                str(project / "main.vr"),
                *case["command_args"],
            ]
            environment = os.environ.copy()
            environment["GUION_FILE_ROOT"] = str(project)
            environment["GUION_RUNTIME_PROFILE"] = "1"
            result = subprocess.run(command, cwd=project, env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

            envelope = json.loads(result.stdout)
            report = json.loads((project / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(report, case["checks"][0]["value"])
            profile = envelope["runtime_profile"]
            self.assertLess(
                profile["record_charges"]["events"],
                8192,
                "same-version package hits should not allocate one throwaway SemVer record each",
            )


if __name__ == "__main__":
    unittest.main()
