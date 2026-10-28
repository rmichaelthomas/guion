"""Focused checks for the current-format managed-package report helpers."""

from pathlib import Path
import unittest

from evaluation.completion.performance.verify_managed_package_analysis import (
    warmed_cache_report_value,
)


class WarmedCacheReportValueTests(unittest.TestCase):
    def test_repository_cache_is_reported_relative_to_the_repo(self):
        root = Path("/repo").resolve()
        cache = root / "target/perf-session/linux-aarch64-warmed"

        self.assertEqual(
            warmed_cache_report_value(cache, root),
            "target/perf-session/linux-aarch64-warmed",
        )

    def test_external_cache_is_reported_without_raising_or_disclosing_its_path(self):
        root = Path("/repo").resolve()
        cache = Path("/external/preloaded-cache").resolve()

        self.assertEqual(warmed_cache_report_value(cache, root), "outside repository")


if __name__ == "__main__":
    unittest.main()
