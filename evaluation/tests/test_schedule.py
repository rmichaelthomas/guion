"""Checks for pre-run randomized assignment generation."""
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from evaluation.protocol.schedule import create_schedule

CAMPAIGN = {
    "schema_version": 1,
    "assistant_model": {
        "provider": "OpenAI",
        "name": "gpt-6.1-sol",
        "version": "gpt-6.1-sol",
        "reasoning_effort": "medium",
    },
    "choice_budget": {"active_minutes": 5, "tool_calls": 0},
    "build_budget": {"active_minutes": 45, "tool_calls": 60},
    "primary_measure": "active_seconds",
}


class ScheduleTests(unittest.TestCase):
    def test_schedule_covers_all_pairs_and_choices_before_runs(self):
        manifest = json.loads((ROOT / "fixtures/independent-v1/manifest.json").read_text())
        schedule = create_schedule(manifest, "d" * 64, CAMPAIGN, "e" * 64, "2026-10-23T00:00:00Z")
        self.assertEqual(schedule["status"], "assigned-before-runs")
        self.assertEqual(schedule["campaign"], CAMPAIGN)
        self.assertEqual(schedule["campaign_config_sha256"], "e" * 64)
        self.assertEqual(len(schedule["pairs"]), 18)
        self.assertEqual(len(schedule["choices"]), 18)
        self.assertEqual(len({row["pair_id"] for row in schedule["pairs"]}), 18)
        self.assertEqual(len({row["decision_id"] for row in schedule["choices"]}), 18)
        for row in schedule["pairs"]:
            self.assertEqual(set(row["pair_order"]), {"veritas", "python"})
            self.assertEqual([entry["language"] for entry in row["sessions"]], row["pair_order"])
        for row in schedule["choices"]:
            self.assertEqual(set(row["presentation_order"]), {"veritas", "python"})
        without_digest = dict(schedule)
        digest = without_digest.pop("schedule_sha256")
        import hashlib
        canonical = json.dumps(without_digest, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(digest, hashlib.sha256(canonical).hexdigest())

    def test_schedule_rejects_missing_or_unbounded_campaign_limits(self):
        manifest = json.loads((ROOT / "fixtures/independent-v1/manifest.json").read_text())
        invalid = json.loads(json.dumps(CAMPAIGN))
        del invalid["assistant_model"]["version"]
        with self.assertRaisesRegex(ValueError, "assistant_model.version"):
            create_schedule(manifest, "d" * 64, invalid, "e" * 64, "2026-10-23T00:00:00Z")

        invalid = json.loads(json.dumps(CAMPAIGN))
        invalid["build_budget"]["active_minutes"] = 0
        with self.assertRaisesRegex(ValueError, "build_budget.active_minutes"):
            create_schedule(manifest, "d" * 64, invalid, "e" * 64, "2026-10-23T00:00:00Z")


if __name__ == "__main__":
    unittest.main()
