"""Focused checks for the installed performance harness."""

import sys
import shutil
import tempfile
import unittest
from pathlib import Path

import runner


class PeakMemoryTests(unittest.TestCase):
    def test_records_peak_memory_for_a_successful_candidate(self) -> None:
        observed = runner.peak_memory([sys.executable, "-c", "pass"], Path.cwd())

        self.assertIsInstance(observed, int)
        self.assertGreater(observed, 0)


class GuionApplicationPreparationTests(unittest.TestCase):
    def test_converts_only_the_temporary_copy_of_the_frozen_application(self) -> None:
        frozen_source = runner.APP / "main.vr"
        original_source = frozen_source.read_bytes()
        with tempfile.TemporaryDirectory() as scratch:
            project = Path(scratch) / "jsonl-summary"
            shutil.copytree(runner.APP, project)

            runner.prepare_guion_application(project)

            self.assertEqual((project / "main.gu").read_bytes(), original_source)
            self.assertIn('entry = "main.gu"', (project / "guion.toml").read_text())
            self.assertFalse((project / "main.vr").exists())
            self.assertFalse((project / "veritas.toml").exists())
            self.assertFalse((project / "veritas.lock").exists())
        self.assertEqual(frozen_source.read_bytes(), original_source)


if __name__ == "__main__":
    unittest.main()
