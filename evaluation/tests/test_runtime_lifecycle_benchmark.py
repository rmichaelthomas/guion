import unittest

from evaluation.development.benchmark_runtime_lifecycle import (
    parse_process_table,
    percentile,
    process_tree_memory,
    summarize_samples,
)


class RuntimeLifecycleBenchmarkTests(unittest.TestCase):
    def test_summarizes_per_invocation_latency_and_throughput(self):
        summary = summarize_samples([10_000_000, 20_000_000, 30_000_000, 40_000_000], 0.1)
        self.assertEqual(summary["sample_count"], 4)
        self.assertEqual(summary["p50_ms"], 25.0)
        self.assertEqual(summary["p95_ms"], 38.5)
        self.assertEqual(summary["p99_ms"], 39.7)
        self.assertEqual(summary["max_ms"], 40.0)
        self.assertEqual(summary["throughput_calls_per_second"], 40.0)

    def test_interpolates_percentiles_and_rejects_empty_samples(self):
        self.assertEqual(percentile([1, 2, 3, 4], 50), 2.5)
        with self.assertRaises(ValueError):
            summarize_samples([], 0.1)

    def test_process_tree_memory_includes_nested_children_and_excludes_other_processes(self):
        process_table = """100 1 10\n101 100 20\n102 101 30\n200 1 99\n"""
        snapshot = process_tree_memory(parse_process_table(process_table), 100)
        self.assertEqual(snapshot["process_ids"], [100, 101, 102])
        self.assertEqual(snapshot["root_rss_bytes"], 10 * 1024)
        self.assertEqual(snapshot["descendants_rss_bytes"], 50 * 1024)
        self.assertEqual(snapshot["tree_rss_bytes"], 60 * 1024)

    def test_process_table_parser_skips_malformed_rows(self):
        self.assertEqual(parse_process_table("header\n100 1 10\nbad row\n"), [(100, 1, 10)])

    def test_process_tree_snapshot_is_unavailable_when_root_exited(self):
        self.assertIsNone(process_tree_memory([(101, 1, 20)], 100))


if __name__ == "__main__":
    unittest.main()
