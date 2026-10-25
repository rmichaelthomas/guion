"""Focused checks for the installed performance harness."""

import sys
import unittest
from pathlib import Path

import runner


class PeakMemoryTests(unittest.TestCase):
    def test_records_peak_memory_for_a_successful_candidate(self) -> None:
        observed = runner.peak_memory([sys.executable, "-c", "pass"], Path.cwd())

        self.assertIsInstance(observed, int)
        self.assertGreater(observed, 0)


if __name__ == "__main__":
    unittest.main()
