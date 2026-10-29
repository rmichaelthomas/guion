from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import benchmark_jsonl_text_slice as benchmark


class JsonlTextSliceBenchmarkTests(unittest.TestCase):
    def test_variant_changes_only_date_extraction_in_frozen_application(self) -> None:
        original, variant = benchmark.application_sources()

        old = (
            "let characters = text_chars(at)\n"
            "                        let date = characters[0] + characters[1] + characters[2] + characters[3] + characters[4] + characters[5] + characters[6] + characters[7] + characters[8] + characters[9]"
        )
        new = "let date = text_slice(at, 0, 10)"

        self.assertEqual(original.count(old), 1)
        self.assertEqual(variant, original.replace(old, new, 1))

    def test_variant_retains_the_original_application_as_a_separate_arm(self) -> None:
        original, variant = benchmark.application_sources()

        self.assertIn("text_chars(at)", original)
        self.assertIn("text_slice(at, 0, 10)", variant)
        self.assertNotEqual(original, variant)


if __name__ == "__main__":
    unittest.main()
