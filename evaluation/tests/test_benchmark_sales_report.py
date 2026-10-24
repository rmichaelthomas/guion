import unittest
import sys

from pathlib import Path

from evaluation.development.benchmark_sales_report import (
    percentile,
    runtime_commands,
    summarize,
)


class BenchmarkSummaryTests(unittest.TestCase):
    def test_reports_percentiles_and_spread_from_raw_samples(self):
        self.assertEqual(percentile([1, 2, 3, 4], 50), 2.5)
        self.assertEqual(
            summarize([0.1, 0.2, 0.3, 0.4]),
            {
                "samples_seconds": [0.1, 0.2, 0.3, 0.4],
                "min_seconds": 0.1,
                "median_seconds": 0.25,
                "p95_seconds": 0.385,
                "max_seconds": 0.4,
            },
        )

    def test_compares_source_execution_artifact_execution_and_python(self):
        commands = runtime_commands(
            Path("/tools/veritas"), Path("/tmp/project"), Path("/tmp/project/app.vrb")
        )
        self.assertEqual(
            commands["veritas_source"],
            ["/tools/veritas", "run", "/tmp/project/main.vr", "sales.json"],
        )
        self.assertEqual(
            commands["veritas_artifact"],
            ["/tools/veritas", "run", "/tmp/project/app.vrb", "sales.json"],
        )
        self.assertEqual(commands["python_standard_library"][0], sys.executable)


if __name__ == "__main__":
    unittest.main()
