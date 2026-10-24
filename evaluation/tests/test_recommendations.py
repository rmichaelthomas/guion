"""Checks for the direct assistant recommendation protocol."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from evaluation.protocol.recommendations import summarize_records, validate_record


RECORD = {
    "schema_version": 1,
    "decision_id": "choice-001",
    "recorded_at_utc": "2026-10-23T18:00:00Z",
    "task": {
        "id": "brief-automation-01",
        "class": "small_automation_cli",
        "origin": "independent",
        "brief_sha256": "a" * 64,
    },
    "assistant": {
        "provider": "OpenAI",
        "name": "gpt-6.1-sol",
        "version": "gpt-6.1-sol",
        "reasoning_effort": "medium",
        "session_id": "session-001",
        "fresh_session": True,
    },
    "budget": {"active_minutes": 5, "tool_calls": 0},
    "usage": {"active_seconds": 120, "tool_calls": 0},
    "presentation_order": ["veritas", "python"],
    "context": {
        "mode": "pre-build-recommendation",
        "veritas_version": "sprint-build",
        "python_version": "3.14.8 with uv",
        "documentation_sha256": "b" * 64,
        "schedule_sha256": "c" * 64,
        "schedule_file_sha256": "9" * 64,
        "schedule_entry_id": "brief-automation-01-choice-1",
        "prerun_records_sha256": "d" * 64,
        "prerun_recorded_at_utc": "2026-10-23T17:00:00Z",
        "comparative_pair_results_shown": False,
        "campaign_config_sha256": "f" * 64,
    },
    "recommendation": {
        "choice": "veritas",
        "rationale": "The task's packaging and typed API fit the supplied Veritas workflow.",
        "conditions": ["The target remains macOS ARM64."],
    },
    "evidence": {
        "prompt_transcript_path": "choice-001/prompt.txt",
        "prompt_transcript_sha256": "e" * 64,
        "response_transcript_path": "choice-001/response.txt",
        "response_transcript_sha256": "f" * 64,
        "usage_snapshot_path": "choice-001/thread-usage.json",
        "usage_snapshot_sha256": "a" * 64,
    },
    "exclusion": {"excluded": False, "reason": None},
}


class RecommendationProtocolTests(unittest.TestCase):
    def test_valid_prebuild_recommendation_record(self):
        valid, errors, derived = validate_record(RECORD)
        self.assertTrue(valid, errors)
        self.assertEqual(errors, [])
        self.assertTrue(derived["recommends_veritas"])

    def test_rejects_any_record_that_shows_comparative_pair_results(self):
        record = copy.deepcopy(RECORD)
        record["context"]["comparative_pair_results_shown"] = True
        valid, errors, _ = validate_record(record)
        self.assertFalse(valid)
        self.assertTrue(any("comparative_pair_results_shown" in error for error in errors))

    def test_record_must_match_the_pre_run_assignment_and_bundle(self):
        record = copy.deepcopy(RECORD)
        schedule = {
            "schema_version": 2,
            "status": "assigned-before-runs",
            "documentation_sha256": "b" * 64,
            "campaign_config_sha256": "f" * 64,
            "campaign": {
                "assistant_model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium"},
                "choice_budget": {"active_minutes": 5, "tool_calls": 0},
                "build_budget": {"active_minutes": 45, "tool_calls": 60},
                "primary_measure": "active_seconds",
            },
            "choices": [{
                "decision_id": "choice-001",
                "task_id": "brief-automation-01",
                "task_class": "small_automation_cli",
                "brief_sha256": "a" * 64,
                "presentation_order": ["veritas", "python"],
                "documentation_sha256": "b" * 64,
            }],
        }
        payload = json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()
        import hashlib
        schedule["schedule_sha256"] = hashlib.sha256(payload).hexdigest()
        record["context"]["schedule_sha256"] = schedule["schedule_sha256"]
        record["context"]["schedule_entry_id"] = "choice-001"
        valid, errors, _ = validate_record(record, schedule)
        self.assertTrue(valid, errors)
        record["presentation_order"] = ["python", "veritas"]
        valid, errors, _ = validate_record(record, schedule)
        self.assertFalse(valid)
        self.assertTrue(any("presentation_order" in error for error in errors))

    def test_choice_must_match_frozen_model_budget_and_capture_prompt_order(self):
        record = copy.deepcopy(RECORD)
        schedule = {
            "schema_version": 2,
            "status": "assigned-before-runs",
            "documentation_sha256": "b" * 64,
            "campaign_config_sha256": "f" * 64,
            "campaign": {
                "assistant_model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium"},
                "choice_budget": {"active_minutes": 5, "tool_calls": 0},
                "build_budget": {"active_minutes": 45, "tool_calls": 60},
                "primary_measure": "active_seconds",
            },
            "choices": [{
                "decision_id": "choice-001",
                "task_id": "brief-automation-01",
                "task_class": "small_automation_cli",
                "brief_sha256": "a" * 64,
                "presentation_order": ["veritas", "python"],
                "documentation_sha256": "b" * 64,
            }],
        }
        schedule["schedule_sha256"] = hashlib.sha256(json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        record["context"]["schedule_sha256"] = schedule["schedule_sha256"]
        record["context"]["schedule_entry_id"] = "choice-001"
        valid, errors, _ = validate_record(record, schedule)
        self.assertTrue(valid, errors)

        record["assistant"]["version"] = "other-model-version"
        valid, errors, _ = validate_record(record, schedule)
        self.assertFalse(valid)
        self.assertTrue(any("assistant model" in error for error in errors))

        record = copy.deepcopy(RECORD)
        record["context"]["schedule_sha256"] = schedule["schedule_sha256"]
        record["context"]["schedule_entry_id"] = "choice-001"
        record["budget"]["tool_calls"] = 1
        valid, errors, _ = validate_record(record, schedule)
        self.assertFalse(valid)
        self.assertTrue(any("budget" in error for error in errors))

    def test_actual_choice_usage_must_fit_the_five_minute_zero_tool_budget(self):
        record = copy.deepcopy(RECORD)
        record["usage"]["active_seconds"] = 301
        valid, errors, _ = validate_record(record)
        self.assertFalse(valid)
        self.assertTrue(any("actual usage.active_seconds" in error for error in errors))

        record = copy.deepcopy(RECORD)
        record["usage"]["tool_calls"] = 1
        valid, errors, _ = validate_record(record)
        self.assertFalse(valid)
        self.assertTrue(any("actual usage.tool_calls" in error for error in errors))

    def test_choice_must_bind_pre_run_record_and_occur_after_it(self):
        record = copy.deepcopy(RECORD)
        prerecord = {"recorded_at_utc": "2026-10-23T17:00:00Z", "schedule_sha256": "c" * 64, "schedule_file_sha256": "9" * 64, "campaign_config_sha256": "f" * 64}
        valid, errors, _ = validate_record(record, prerun_record=prerecord, prerun_records_sha256="d" * 64)
        self.assertTrue(valid, errors)
        record["context"]["prerun_recorded_at_utc"] = "2026-10-23T17:00:01Z"
        valid, errors, _ = validate_record(record, prerun_record=prerecord, prerun_records_sha256="d" * 64)
        self.assertFalse(valid)
        self.assertTrue(any("prerun_recorded_at_utc does not match" in error for error in errors))
        record = copy.deepcopy(RECORD)
        record["recorded_at_utc"] = "2026-10-23T16:59:59Z"
        valid, errors, _ = validate_record(record, prerun_record=prerecord, prerun_records_sha256="d" * 64)
        self.assertFalse(valid)
        self.assertTrue(any("later than the pre-run record" in error for error in errors))
        record = copy.deepcopy(RECORD)
        valid, errors, _ = validate_record(record, prerun_record=prerecord, prerun_records_sha256="e" * 64)
        self.assertFalse(valid)
        self.assertTrue(any("pre-run records file" in error for error in errors))

    def test_direct_choice_requires_well_formed_prerun_digest_and_timestamps(self):
        record = copy.deepcopy(RECORD)
        record["context"]["prerun_records_sha256"] = "not-a-sha256"
        valid, errors, _ = validate_record(record)
        self.assertFalse(valid)
        self.assertTrue(any("prerun_records_sha256" in error for error in errors))

        record = copy.deepcopy(RECORD)
        record["recorded_at_utc"] = "2026-10-23T18:00:00"
        record["context"]["prerun_recorded_at_utc"] = "not-a-time"
        valid, errors, _ = validate_record(record)
        self.assertFalse(valid)
        self.assertTrue(any("record.recorded_at_utc" in error for error in errors))
        self.assertTrue(any("context.prerun_recorded_at_utc" in error for error in errors))

    def test_choice_evidence_digests_are_checked_against_saved_transcripts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prompt_path = root / RECORD["evidence"]["prompt_transcript_path"]
            response_path = root / RECORD["evidence"]["response_transcript_path"]
            prompt_path.parent.mkdir(parents=True, exist_ok=True)
            response_path.parent.mkdir(parents=True, exist_ok=True)
            prompt_path.write_text("Exact prompt shown in assigned order\n", encoding="utf-8")
            response_path.write_text("veritas: reasoned choice\n", encoding="utf-8")
            usage_path = root / RECORD["evidence"]["usage_snapshot_path"]
            usage_path.write_text(json.dumps({
                "schemaVersion": 1,
                "thread": {"id": RECORD["assistant"]["session_id"]},
                "page": {"limit": 10, "nextCursor": None, "hasMore": False},
                "turns": [{"id": "turn-1", "status": "completed", "durationMs": 120000, "items": []}],
            }), encoding="utf-8")
            record = copy.deepcopy(RECORD)
            record["evidence"]["prompt_transcript_sha256"] = hashlib.sha256(prompt_path.read_bytes()).hexdigest()
            record["evidence"]["response_transcript_sha256"] = hashlib.sha256(response_path.read_bytes()).hexdigest()
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(usage_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, evidence_root=root)
            self.assertTrue(valid, errors)

            usage_snapshot = json.loads(usage_path.read_text(encoding="utf-8"))
            usage_snapshot["turns"][0]["durationMs"] = 301000
            usage_path.write_text(json.dumps(usage_snapshot), encoding="utf-8")
            record["usage"]["active_seconds"] = 301
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(usage_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, evidence_root=root)
            self.assertFalse(valid)
            self.assertTrue(any("actual usage.active_seconds" in error for error in errors))

            usage_snapshot["turns"][0]["durationMs"] = 120000
            usage_snapshot["turns"][0]["items"] = [{"type": "commandExecution", "id": "choice-tool-1"}]
            usage_path.write_text(json.dumps(usage_snapshot), encoding="utf-8")
            record["usage"]["active_seconds"] = 120
            record["usage"]["tool_calls"] = 1
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(usage_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, evidence_root=root)
            self.assertFalse(valid)
            self.assertTrue(any("actual usage.tool_calls" in error for error in errors))

            usage_snapshot["turns"][0]["items"] = []
            usage_path.write_text(json.dumps(usage_snapshot), encoding="utf-8")
            record["usage"]["tool_calls"] = 0
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(usage_path.read_bytes()).hexdigest()

            prompt_path.write_text("Different prompt order\n", encoding="utf-8")
            valid, errors, _ = validate_record(record, evidence_root=root)
            self.assertFalse(valid)
            self.assertTrue(any("does not match its transcript" in error for error in errors))

    def test_rejects_reused_assistant_session_across_decisions(self):
        second = copy.deepcopy(RECORD)
        second["decision_id"] = "choice-002"
        report = summarize_records([RECORD, second])
        self.assertEqual(report["valid_decisions"], 1)
        self.assertEqual(report["invalid_decisions"][0]["decision_id"], "choice-002")

    def test_summary_rejects_mixed_assistant_model_versions(self):
        second = copy.deepcopy(RECORD)
        second["decision_id"] = "choice-002"
        second["assistant"]["session_id"] = "session-002"
        second["assistant"]["version"] = "gpt-6.2-sol"
        report = summarize_records([RECORD, second])
        self.assertEqual(report["valid_decisions"], 1)
        self.assertEqual(report["invalid_decisions"][0]["decision_id"], "choice-002")
        self.assertTrue(any("model" in error for error in report["invalid_decisions"][0]["errors"]))

    def test_summary_requires_two_fresh_choices_per_brief_and_reports_class_majority(self):
        records = []
        for task_class, brief_prefix in [
            ("small_automation_cli", "cli"),
            ("concurrent_network_integration", "network"),
            ("maintainable_application", "application"),
        ]:
            for brief_index in range(3):
                for repetition in range(2):
                    record = copy.deepcopy(RECORD)
                    record["decision_id"] = f"{brief_prefix}-{brief_index}-{repetition}"
                    record["task"]["id"] = f"{brief_prefix}-{brief_index}"
                    record["task"]["class"] = task_class
                    record["task"]["brief_sha256"] = str(brief_index + 1) * 64
                    record["assistant"]["session_id"] = record["decision_id"]
                    record["presentation_order"] = ["python", "veritas"] if repetition else ["veritas", "python"]
                    record["recommendation"]["choice"] = "python" if brief_index == 0 else "veritas"
                    records.append(record)
        report = summarize_records(records)
        self.assertEqual(report["valid_decisions"], 18)
        self.assertTrue(report["sample_complete"])
        self.assertTrue(report["classes"]["small_automation_cli"]["veritas_majority"])
        self.assertEqual(report["classes"]["small_automation_cli"]["veritas_choices"], 4)

    def test_summary_rejects_an_unregistered_fourth_task_class(self):
        records = []
        for task_class, brief_prefix in [
            ("small_automation_cli", "cli"),
            ("concurrent_network_integration", "network"),
            ("maintainable_application", "application"),
        ]:
            for brief_index in range(3):
                for repetition in range(2):
                    record = copy.deepcopy(RECORD)
                    record["decision_id"] = f"{brief_prefix}-{brief_index}-{repetition}"
                    record["task"]["id"] = f"{brief_prefix}-{brief_index}"
                    record["task"]["class"] = task_class
                    record["task"]["brief_sha256"] = str(brief_index + 1) * 64
                    record["assistant"]["session_id"] = record["decision_id"]
                    records.append(record)
        for brief_index in range(3):
            for repetition in range(2):
                record = copy.deepcopy(RECORD)
                record["decision_id"] = f"extra-{brief_index}-{repetition}"
                record["task"]["id"] = f"extra-{brief_index}"
                record["task"]["class"] = "unregistered_class"
                record["task"]["brief_sha256"] = str(brief_index + 4) * 64
                record["assistant"]["session_id"] = record["decision_id"]
                records.append(record)

        report = summarize_records(records)
        self.assertFalse(report["sample_complete"])

    def test_cli_reports_invalid_recommendation_record(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "bad.json"
            schedule_path = Path(temporary) / "schedule.json"
            prerun_path = Path(temporary) / "prerun.jsonl"
            evidence_root = Path(temporary) / "evidence"
            evidence_root.mkdir()
            path.write_text(json.dumps({"schema_version": 1}), encoding="utf-8")
            schedule = {"schema_version": 1, "status": "assigned-before-runs", "documentation_sha256": "b" * 64, "choices": []}
            import hashlib
            schedule["schedule_sha256"] = hashlib.sha256(json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
            schedule_path.write_text(json.dumps(schedule), encoding="utf-8")
            prerun_path.write_text(json.dumps({"status": "recorded_before_formal_runs", "schedule_sha256": schedule["schedule_sha256"], "recorded_at_utc": "2026-01-01T00:00:00Z"}) + "\n", encoding="utf-8")
            result = subprocess.run(
                [sys.executable, "evaluation/protocol/recommendations.py", "validate", str(path), "--schedule", str(schedule_path), "--prerun-records", str(prerun_path), "--evidence-root", str(evidence_root)],
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(result.returncode, 1)
        report = json.loads(result.stdout)
        self.assertEqual(report["status"], "invalid")
        self.assertTrue(report["errors"])


if __name__ == "__main__":
    unittest.main()
