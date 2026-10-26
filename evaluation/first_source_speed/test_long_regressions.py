import os
import subprocess
import sys
import unittest
from pathlib import Path

from evaluation.first_source_speed.long_regressions import long_workloads, verify_long_output


class LongRegressionDefinitionsTests(unittest.TestCase):
    def test_preserved_reference_workloads_have_fixed_outputs(self):
        workloads = {workload.name: workload for workload in long_workloads()}
        self.assertEqual(set(workloads), {"richards", "nbody", "mandelbrot"})
        self.assertEqual(workloads["richards"].expected_stdout, "9297\n23246\n")
        self.assertEqual(
            workloads["nbody"].expected_stdout,
            "-0.16907516382852447\n-0.16908926275527233\n",
        )
        self.assertEqual(workloads["mandelbrot"].expected_stdout, "155052\n1608\n")

    def test_nbody_output_uses_a_tight_float_tolerance(self):
        workload = next(item for item in long_workloads() if item.name == "nbody")
        verify_long_output(
            workload,
            0,
            "-0.16907516382852447\n-0.16908926275526803\n",
        )
        with self.assertRaisesRegex(RuntimeError, "output mismatch"):
            verify_long_output(workload, 0, "-0.1690751638285\n-0.168\n")

    def test_non_nbody_outputs_remain_exact(self):
        workload = next(item for item in long_workloads() if item.name == "richards")
        with self.assertRaisesRegex(RuntimeError, "output mismatch"):
            verify_long_output(workload, 0, "9297\n23245\n")

    def test_runner_can_be_invoked_as_a_file(self):
        script = Path(__file__).with_name("long_regressions.py")
        environment = dict(os.environ)
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--trials", result.stdout)


if __name__ == "__main__":
    unittest.main()
