"""Checks that randomized language order changes only the rendered order."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from evaluation.protocol.render_choice_prompt import render_choice_prompt


class ChoicePromptTests(unittest.TestCase):
    def test_renders_the_same_verified_language_profiles_in_assigned_order(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            veritas_path = root / "veritas.md"
            python_path = root / "python.md"
            veritas_path.write_text("Veritas profile marker\n", encoding="utf-8")
            python_path.write_text("Python profile marker\n", encoding="utf-8")
            artifacts = [
                {"path": "veritas.md", "language": "veritas", "sha256": hashlib.sha256(veritas_path.read_bytes()).hexdigest()},
                {"path": "python.md", "language": "python", "sha256": hashlib.sha256(python_path.read_bytes()).hexdigest()},
            ]
            canonical = json.dumps(artifacts, sort_keys=True, separators=(",", ":")).encode()
            bundle = {"bundle_sha256": hashlib.sha256(canonical).hexdigest(), "artifacts": artifacts}
            campaign = {
                "assistant_model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium"},
                "choice_budget": {"active_minutes": 5, "tool_calls": 0},
                "build_budget": {"active_minutes": 45, "tool_calls": 60},
                "primary_measure": "active_seconds",
            }
            choice = {"decision_id": "task-choice-1", "task_id": "task-1", "task_class": "small_automation_cli", "brief_sha256": "a" * 64, "documentation_sha256": bundle["bundle_sha256"], "presentation_order": ["veritas", "python"]}
            task = {"id": "task-1", "class": "small_automation_cli", "brief_sha256": "a" * 64, "brief_text": "Build a small tool."}
            prerun = {"recorded_at_utc": "2026-10-23T17:00:00Z"}

            first = render_choice_prompt(choice, task, bundle, root, campaign, "b" * 64, "c" * 64, "d" * 64, prerun)
            reversed_choice = dict(choice, presentation_order=["python", "veritas"])
            second = render_choice_prompt(reversed_choice, task, bundle, root, campaign, "b" * 64, "c" * 64, "d" * 64, prerun)

        self.assertLess(first.index("Veritas profile marker"), first.index("Python profile marker"))
        self.assertLess(second.index("Python profile marker"), second.index("Veritas profile marker"))
        self.assertIn("Presentation order: veritas, python", first)
        self.assertIn("Presentation order: python, veritas", second)
        self.assertIn(bundle["bundle_sha256"], first)
        self.assertEqual(first.count("Veritas profile marker"), 1)
        self.assertEqual(first.count("Python profile marker"), 1)


if __name__ == "__main__":
    unittest.main()
