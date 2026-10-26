import importlib.util
import random
import sys
import unittest
from pathlib import Path


RUNNER_PATH = Path(__file__).with_name("run.py")
SPEC = importlib.util.spec_from_file_location("first_source_speed_runner", RUNNER_PATH)
runner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runner
SPEC.loader.exec_module(runner)


class StatisticsTests(unittest.TestCase):
    def test_nearest_rank_p95(self):
        self.assertEqual(runner.nearest_rank([float(value) for value in range(1, 22)], 0.95), 20.0)

    def test_paired_ratios_use_corresponding_samples(self):
        self.assertEqual(runner.paired_ratios([2.0, 4.0, 6.0], [1.0, 2.0, 3.0]), [2.0, 2.0, 2.0])

    def test_bootstrap_interval_is_deterministic(self):
        first = runner.bootstrap_median_interval([2.0] * 21, resamples=500, seed=73)
        second = runner.bootstrap_median_interval([2.0] * 21, resamples=500, seed=73)
        self.assertEqual(first, second)
        self.assertEqual(first, (2.0, 2.0))

class CorrectnessAndPairingTests(unittest.TestCase):
    def test_output_mismatch_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "output mismatch"):
            runner.verify_output(return_code=0, stdout="wrong\n", expected_stdout="right\n")

    def test_pair_order_is_seeded_and_randomizes_both_orders(self):
        forward = runner.randomized_pair_order(random.Random(4))
        self.assertEqual(forward, runner.randomized_pair_order(random.Random(4)))
        orders = {
            runner.randomized_pair_order(random.Random(seed))
            for seed in range(20)
        }
        self.assertEqual(orders, {("guion", "python"), ("python", "guion")})


if __name__ == "__main__":
    unittest.main()
