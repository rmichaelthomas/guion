import json
import hashlib
from pathlib import Path
import tempfile
import unittest

from evaluation.protocol.record_prerun import append_record

CAMPAIGN = {
    "schema_version": 1,
    "assistant_model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium"},
    "choice_budget": {"active_minutes": 5, "tool_calls": 0},
    "build_budget": {"active_minutes": 45, "tool_calls": 60},
    "primary_measure": "active_seconds",
}


class PreRunRecordTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.freeze = self.root / "freeze.json"
        self.schedule = self.root / "schedule.json"
        self.lock = self.root / "campaign-lock.json"
        self.output = self.root / "prerun.jsonl"
        self.freeze.write_text(json.dumps({
            "status": "frozen_for_independent_evaluation",
            "manifest_sha256": "a" * 64,
            "product_bundle_sha256": "b" * 64,
            "campaign": CAMPAIGN,
            "campaign_config_sha256": "f" * 64,
        }))
        schedule = {
            "schema_version": 2,
            "status": "assigned-before-runs",
            "manifest_sha256": "a" * 64,
            "documentation_sha256": "b" * 64,
            "campaign": CAMPAIGN,
            "campaign_config_sha256": "f" * 64,
            "pairs": [],
            "choices": [],
        }
        schedule["schedule_sha256"] = hashlib.sha256(json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        self.schedule.write_text(json.dumps(schedule))
        self.lock.write_text(json.dumps({
            "status": "selected_before_independent_runs",
            "platform": "linux/arm64",
            "python_version": "3.14.8",
            "uv_version": "0.13.4",
            "base_image": {"reference": "python:3.14.8-slim-bookworm", "sha256": "c" * 64},
            "veritas_archive": {"sha256": "d" * 64},
        }))

    def tearDown(self):
        self.temporary.cleanup()

    def test_appends_digest_bound_timestamped_record(self):
        record = append_record(self.freeze, self.schedule, self.lock, self.output, "active_seconds")
        self.assertEqual(record["status"], "recorded_before_formal_runs")
        self.assertEqual(record["veritas_archive_sha256"], "d" * 64)
        self.assertEqual(record["primary_measure"], "active_seconds")
        self.assertEqual(record["campaign"], CAMPAIGN)
        self.assertEqual(record["campaign_config_sha256"], "f" * 64)
        self.assertEqual(record["schedule_file_sha256"], hashlib.sha256(self.schedule.read_bytes()).hexdigest())
        self.assertEqual(len(self.output.read_text().splitlines()), 1)

    def test_rejects_schedule_with_changed_manifest(self):
        value = json.loads(self.schedule.read_text())
        value["manifest_sha256"] = "e" * 64
        canonical = {key: item for key, item in value.items() if key != "schedule_sha256"}
        value["schedule_sha256"] = hashlib.sha256(json.dumps(canonical, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        self.schedule.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "does not match the freeze"):
            append_record(self.freeze, self.schedule, self.lock, self.output, "active_seconds")
        self.assertFalse(self.output.exists())

    def test_rejects_schedule_with_changed_model_or_budget(self):
        value = json.loads(self.schedule.read_text())
        value["campaign"]["build_budget"]["tool_calls"] = 61
        canonical = {key: item for key, item in value.items() if key != "schedule_sha256"}
        value["schedule_sha256"] = hashlib.sha256(json.dumps(canonical, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        self.schedule.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "campaign does not match the freeze"):
            append_record(self.freeze, self.schedule, self.lock, self.output, "active_seconds")
        self.assertFalse(self.output.exists())

    def test_refuses_duplicate_schedule_record(self):
        append_record(self.freeze, self.schedule, self.lock, self.output, "active_seconds")
        with self.assertRaisesRegex(ValueError, "already has a pre-run record"):
            append_record(self.freeze, self.schedule, self.lock, self.output, "active_seconds")
        self.assertEqual(len(self.output.read_text().splitlines()), 1)

    def test_rejects_invalid_primary_measure_before_writing(self):
        with self.assertRaisesRegex(ValueError, "primary measure"):
            append_record(self.freeze, self.schedule, self.lock, self.output, "unregistered_metric")
        self.assertFalse(self.output.exists())
