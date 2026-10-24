"""Checks for the independent recommendation evaluation ledger."""

from __future__ import annotations

import json
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from evaluation.protocol.ledger import _implementation_signal_eligibility, summarize_records, validate_record
from evaluation.protocol.usage_evidence import delivery_tree_sha256


SCHEDULE = {
    "schema_version": 2,
    "status": "assigned-before-runs",
    "manifest_sha256": "a" * 64,
    "documentation_sha256": "b" * 64,
    "campaign_config_sha256": "f" * 64,
    "campaign": {
        "assistant_model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium"},
        "choice_budget": {"active_minutes": 5, "tool_calls": 0},
        "build_budget": {"active_minutes": 45, "tool_calls": 60},
        "primary_measure": "active_seconds",
    },
    "pairs": [{
        "pair_id": "pair-001",
        "task_id": "brief-automation-01",
        "task_class": "small_automation_cli",
        "brief_sha256": "a" * 64,
        "pair_order": ["veritas", "python"],
        "sessions": [
            {"language": "veritas", "session_slot": "pair-001-veritas"},
            {"language": "python", "session_slot": "pair-001-python"},
        ],
    }],
    "choices": [],
}
SCHEDULE["schedule_sha256"] = hashlib.sha256(json.dumps(SCHEDULE, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
SCHEDULE_FILE_SHA256 = "d" * 64
PRERUN_RECORDS_SHA256 = "e" * 64
CONTEXT = {
    "schedule": SCHEDULE,
    "schedule_file_sha256": SCHEDULE_FILE_SHA256,
    "prerun_records_sha256": PRERUN_RECORDS_SHA256,
    "pre_run": {"status": "recorded_before_formal_runs", "schedule_sha256": SCHEDULE["schedule_sha256"], "primary_measure": "active_seconds"},
}


RECORD = {
    "schema_version": 1,
    "run_id": "run-001-veritas",
    "pair_id": "pair-001",
    "assignment": {
        "session_slot": "pair-001-veritas",
        "schedule_sha256": SCHEDULE["schedule_sha256"],
        "schedule_file_sha256": SCHEDULE_FILE_SHA256,
        "prerun_records_sha256": PRERUN_RECORDS_SHA256,
        "campaign_config_sha256": "f" * 64,
        "manifest_sha256": SCHEDULE["manifest_sha256"],
        "primary_measure": "active_seconds",
    },
    "task": {
        "id": "brief-automation-01",
        "class": "small_automation_cli",
        "origin": "independent",
        "brief_sha256": "a" * 64,
    },
    "candidate": {"language": "veritas", "version": "RC66"},
    "model": {"provider": "OpenAI", "name": "gpt-6.1-sol", "version": "gpt-6.1-sol", "reasoning_effort": "medium", "session_id": "session-veritas-001", "fresh_session": True},
    "environment": {"os": "macOS", "architecture": "arm64", "cpu": "Apple M3", "runtime_tooling": "veritas RC66", "network_mode": "loopback-fixtures", "host_id": "fixture-engine"},
    "pair_order": ["veritas", "python"],
    "budget": {"active_minutes": 45, "tool_calls": 60},
    "timestamps": {"started_at": "2026-10-22T12:00:00Z", "ended_at": "2026-10-22T12:10:00Z"},
    "outcome": {
        "initial": {"status": "passed", "acceptance_passed": 4, "acceptance_total": 4},
        "follow_up": {"status": "passed", "acceptance_passed": 2, "acceptance_total": 2},
        "clean_workspace_run": True,
        "hidden_system_python_dependency": False,
        "severe_regression": False,
    },
    "costs": {
        "active_seconds": 360,
        "tool_calls": 3,
        "cold_setup_seconds": 120,
        "warm_run_seconds": 2,
        "human_interventions": 1,
        "artifact_bytes": 1000,
        "dependency_bytes": 2000,
    },
    "commands": ["veritas sync", "veritas test", "veritas run"],
    "human_interventions": ["Clarified the expected CSV column order."],
    "evidence": {
        "prompt_path": "run-001-veritas/prompt.txt",
        "prompt_sha256": "b" * 64,
        "test_inputs_path": "run-001-veritas/inputs.json",
        "transcript_path": "run-001-veritas/transcript.txt",
        "transcript_sha256": "a" * 64,
        "test_inputs_sha256": "c" * 64,
        "output_path": "run-001-veritas/output.json",
        "output_sha256": "d" * 64,
        "usage_snapshot_path": "run-001-veritas/thread-usage.json",
        "usage_snapshot_sha256": "e" * 64,
        "stages": {
            "initial": {"runner_report_path": "run-001-veritas/initial.json", "runner_report_sha256": "1" * 64},
            "follow_up": {"runner_report_path": "run-001-veritas/follow-up.json", "runner_report_sha256": "2" * 64},
        },
    },
    "review": {"reviewer_id": "reviewer-01", "verdict": "accepted", "notes": "All assertions reproduced."},
    "exclusion": {"excluded": False, "reason": None},
}


class ProtocolLedgerTests(unittest.TestCase):
    def test_valid_run_record_derives_primary_success(self):
        valid, errors, derived = validate_record(RECORD, CONTEXT)
        self.assertTrue(valid, errors)
        self.assertEqual(errors, [])
        self.assertTrue(derived["primary_success"])

    def test_failed_or_incomplete_attempt_is_retained_not_dropped(self):
        record = json.loads(json.dumps(RECORD))
        record["outcome"]["follow_up"]["status"] = "failed"
        record["outcome"]["follow_up"]["acceptance_passed"] = 1
        record["exclusion"] = {"excluded": True, "reason": "The task brief accidentally disclosed a fixture."}
        valid, errors, derived = validate_record(record, CONTEXT)
        self.assertTrue(valid, errors)
        self.assertFalse(derived["primary_success"])
        self.assertEqual(record["exclusion"]["reason"], "The task brief accidentally disclosed a fixture.")

    def test_validation_rejects_missing_follow_up_and_missing_failure_reason(self):
        record = json.loads(json.dumps(RECORD))
        del record["outcome"]["follow_up"]
        record["exclusion"] = {"excluded": True, "reason": None}
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("follow_up" in error for error in errors))
        self.assertTrue(any("exclusion.reason" in error for error in errors))

    def test_validation_reports_wrong_acceptance_field_types_without_crashing(self):
        record = json.loads(json.dumps(RECORD))
        record["outcome"]["initial"]["acceptance_passed"] = "all"
        record["outcome"]["initial"]["acceptance_total"] = "all"
        valid, errors, derived = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("acceptance_total" in error for error in errors))
        self.assertFalse(derived["primary_success"])

    def test_independent_run_must_match_its_scheduled_slot_and_predeclared_measure(self):
        record = json.loads(json.dumps(RECORD))
        record["assignment"]["session_slot"] = "unassigned-slot"
        record["assignment"]["primary_measure"] = "human_interventions"
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("session_slot" in error for error in errors))
        self.assertTrue(any("primary_measure" in error for error in errors))

    def test_independent_run_must_match_frozen_campaign_model_and_budget(self):
        record = json.loads(json.dumps(RECORD))
        record["model"]["version"] = "gpt-6.2-sol"
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("campaign model" in error for error in errors))

        record = json.loads(json.dumps(RECORD))
        record["budget"]["tool_calls"] = 61
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("campaign budget" in error for error in errors))

    def test_independent_run_cannot_be_validated_without_schedule_context(self):
        valid, errors, _ = validate_record(RECORD)
        self.assertFalse(valid)
        self.assertTrue(any("frozen schedule and pre-run records are required" in error for error in errors))

    def test_actual_usage_must_stay_within_frozen_caps(self):
        record = json.loads(json.dumps(RECORD))
        record["costs"]["active_seconds"] = 45 * 60 + 1
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("active-time budget" in error for error in errors))

        record = json.loads(json.dumps(RECORD))
        record["costs"]["tool_calls"] = 61
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("tool-call budget" in error for error in errors))

    def test_evidence_root_binds_usage_runner_reports_and_delivery_tree(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = json.loads(json.dumps(RECORD))
            evidence_dir = root / "run-001-veritas"
            evidence_dir.mkdir()

            snapshot = {
                "schemaVersion": 1,
                "thread": {"id": record["model"]["session_id"]},
                "page": {"limit": 10, "nextCursor": None, "hasMore": False},
                "turns": [{
                    "id": "turn-1", "status": "completed", "durationMs": 360000,
                    "items": [{"type": "commandExecution", "id": f"tool-{i}"} for i in range(3)],
                }],
            }
            snapshot_path = evidence_dir / "thread-usage.json"
            snapshot_path.write_text(json.dumps(snapshot), encoding="utf-8")
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(snapshot_path.read_bytes()).hexdigest()

            file_contents = {
                "prompt.txt": "prompt\n",
                "inputs.json": "{}\n",
                "transcript.txt": "conversation\n",
                "output.json": "{}\n",
            }
            digest_keys = {
                "prompt.txt": "prompt_sha256",
                "inputs.json": "test_inputs_sha256",
                "transcript.txt": "transcript_sha256",
                "output.json": "output_sha256",
            }
            path_keys = {
                "prompt.txt": "prompt_path",
                "inputs.json": "test_inputs_path",
                "transcript.txt": "transcript_path",
                "output.json": "output_path",
            }
            for name, content in file_contents.items():
                path = evidence_dir / name
                path.write_text(content, encoding="utf-8")
                record["evidence"][digest_keys[name]] = hashlib.sha256(path.read_bytes()).hexdigest()
                record["evidence"][path_keys[name]] = f"run-001-veritas/{name}"

            for stage in ("initial", "follow_up"):
                report_name = f"{stage}.json"
                delivery_name = report_name + ".delivery"
                delivery = evidence_dir / delivery_name
                delivery.mkdir()
                (delivery / "main.py").write_text("print('ok')\n", encoding="utf-8")
                tree_digest = delivery_tree_sha256(delivery)
                manifest_name = report_name + ".delivery-manifest.json"
                manifest = evidence_dir / manifest_name
                manifest.write_text('{"artifact_paths":["main.py"]}\n', encoding="utf-8")
                manifest_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
                report = {
                    "schema_version": 1,
                    "pair_id": record["pair_id"],
                    "stage": stage,
                    "language": record["candidate"]["language"],
                    "candidate_version": record["candidate"]["version"],
                    "session_id": record["model"]["session_id"],
                    "task_id": record["task"]["id"],
                    "manifest_sha256": record["assignment"]["manifest_sha256"],
                    "schedule_sha256": record["assignment"]["schedule_sha256"],
                    "delivery_artifact_path": delivery_name,
                    "delivery_artifact_sha256": tree_digest,
                    "delivery_manifest_path": manifest_name,
                    "delivery_manifest_sha256": manifest_digest,
                    "container": {"docker_engine_id": record["environment"]["host_id"], "delivery_source_project_mounted": False},
                    "case_report": {
                        "task_id": record["task"]["id"],
                        "stage": stage,
                        "passed": True,
                        "source": {"passed": True},
                        "delivery": {"passed": True, "artifact_sha256": tree_digest},
                    },
                }
                report_path = evidence_dir / report_name
                report_path.write_text(json.dumps(report), encoding="utf-8")
                record["evidence"]["stages"][stage] = {
                    "runner_report_path": f"run-001-veritas/{report_name}",
                    "runner_report_sha256": hashlib.sha256(report_path.read_bytes()).hexdigest(),
                }

            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertTrue(valid, errors)

            initial_report_path = evidence_dir / "initial.json"
            initial_report = json.loads(initial_report_path.read_text(encoding="utf-8"))
            initial_report["case_report"]["passed"] = False
            initial_report_path.write_text(json.dumps(initial_report), encoding="utf-8")
            record["evidence"]["stages"]["initial"]["runner_report_sha256"] = hashlib.sha256(initial_report_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertFalse(valid)
            self.assertTrue(any("result success does not match the recorded stage status" in error for error in errors))
            initial_report["case_report"]["passed"] = True
            initial_report_path.write_text(json.dumps(initial_report), encoding="utf-8")
            record["evidence"]["stages"]["initial"]["runner_report_sha256"] = hashlib.sha256(initial_report_path.read_bytes()).hexdigest()
            record["outcome"]["initial"]["status"] = "failed"
            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertFalse(valid)
            self.assertTrue(any("result success does not match the recorded stage status" in error for error in errors))
            record["outcome"]["initial"]["status"] = "passed"

            snapshot["turns"][0]["durationMs"] = (45 * 60 + 1) * 1000
            snapshot_path.write_text(json.dumps(snapshot), encoding="utf-8")
            record["costs"]["active_seconds"] = 45 * 60 + 1
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(snapshot_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertFalse(valid)
            self.assertTrue(any("active-time budget" in error for error in errors))

            snapshot["turns"][0]["durationMs"] = 360000
            snapshot["turns"][0]["items"] = [{"type": "commandExecution", "id": f"tool-{i}"} for i in range(61)]
            snapshot_path.write_text(json.dumps(snapshot), encoding="utf-8")
            record["costs"]["active_seconds"] = 360
            record["costs"]["tool_calls"] = 61
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(snapshot_path.read_bytes()).hexdigest()
            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertFalse(valid)
            self.assertTrue(any("tool-call budget" in error for error in errors))

            snapshot["turns"][0]["items"] = [{"type": "commandExecution", "id": f"tool-{i}"} for i in range(3)]
            snapshot_path.write_text(json.dumps(snapshot), encoding="utf-8")
            record["costs"]["tool_calls"] = 3
            record["evidence"]["usage_snapshot_sha256"] = hashlib.sha256(snapshot_path.read_bytes()).hexdigest()

            record["evidence"]["stages"]["initial"]["runner_report_sha256"] = "0" * 64
            valid, errors, _ = validate_record(record, CONTEXT, root)
            self.assertFalse(valid)
            self.assertTrue(any("runner_report_sha256 does not match" in error for error in errors))

    def test_summary_counts_only_complete_independent_pairs_but_reports_all_attempts(self):
        python_record = json.loads(json.dumps(RECORD))
        python_record["run_id"] = "run-001-python"
        python_record["candidate"] = {"language": "python", "version": "3.14.8 with uv"}
        python_record["pair_id"] = "pair-001"
        python_record["model"]["session_id"] = "session-python-001"
        python_record["costs"]["active_seconds"] = 600
        python_record["costs"]["human_interventions"] = 3
        python_record["human_interventions"] = ["Set up the venv.", "Fix an import.", "Repair invocation."]
        known = json.loads(json.dumps(RECORD))
        known["run_id"] = "run-known"
        known["pair_id"] = "pair-known"
        known["task"]["origin"] = "known"
        python_record["assignment"]["session_slot"] = "pair-001-python"
        report = summarize_records([RECORD, python_record, known], CONTEXT)
        self.assertEqual(report["valid_records"], 3)
        self.assertEqual(report["independent_pairs"], 1)
        self.assertEqual(report["known_or_calibration_records"], 1)
        self.assertEqual(report["classes"]["small_automation_cli"]["paired_primary_successes"], 1)
        self.assertEqual(report["classes"]["small_automation_cli"]["guardrail_pairs"], 1)
        self.assertAlmostEqual(report["classes"]["small_automation_cli"]["median_time_reduction"], 0.4)
        self.assertAlmostEqual(report["classes"]["small_automation_cli"]["median_intervention_reduction"], 2 / 3)
        self.assertTrue(report["pairs"][0]["at_least_20_percent_time_reduction"])
        self.assertTrue(report["pairs"][0]["at_least_20_percent_intervention_reduction"])
        self.assertEqual(report["primary_measure"], "active_seconds")
        self.assertAlmostEqual(report["classes"]["small_automation_cli"]["median_primary_reduction"], 0.4)
        self.assertEqual(report["recommendation_gate"], "insufficient_independent_evidence")

    def test_failed_guardrail_pair_does_not_count_toward_practical_benefit(self):
        python_record = json.loads(json.dumps(RECORD))
        python_record["run_id"] = "run-001-python"
        python_record["candidate"] = {"language": "python", "version": "3.14.8 with uv"}
        python_record["model"]["session_id"] = "session-python-001"
        record = json.loads(json.dumps(RECORD))
        record["outcome"]["hidden_system_python_dependency"] = True
        python_record["assignment"]["session_slot"] = "pair-001-python"
        report = summarize_records([record, python_record], CONTEXT)
        class_result = report["classes"]["small_automation_cli"]
        self.assertEqual(class_result["guardrail_pairs"], 0)
        self.assertIsNone(class_result["median_time_reduction"])
        self.assertIsNone(class_result["median_intervention_reduction"])
        self.assertFalse(class_result["completion_floor_pass"])
        self.assertFalse(class_result["guardrail_floor_pass"])
        self.assertFalse(class_result["implementation_signal_eligible"])

    def test_implementation_signal_requires_completion_and_guardrail_floors(self):
        base = {
            "pairs": 6,
            "distinct_briefs": 3,
            "completion_counts": {"veritas": 5, "python": 5},
            "guardrail_pairs": 5,
            "median_primary_reduction": 0.25,
            "favorable_pairs": 4,
            "veritas_only_severe_regression_pairs": [],
        }
        self.assertTrue(_implementation_signal_eligibility(**base)["implementation_signal_eligible"])
        low_completion = {**base, "completion_counts": {"veritas": 4, "python": 4}}
        result = _implementation_signal_eligibility(**low_completion)
        self.assertFalse(result["completion_floor_pass"])
        self.assertFalse(result["implementation_signal_eligible"])
        lower_veritas_completion = {**base, "completion_counts": {"veritas": 5, "python": 6}}
        result = _implementation_signal_eligibility(**lower_veritas_completion)
        self.assertTrue(result["completion_floor_pass"])
        self.assertFalse(result["completion_parity_pass"])
        self.assertFalse(result["implementation_signal_eligible"])
        low_guardrails = {**base, "guardrail_pairs": 4}
        result = _implementation_signal_eligibility(**low_guardrails)
        self.assertFalse(result["guardrail_floor_pass"])
        self.assertFalse(result["implementation_signal_eligible"])
        severe = {**base, "veritas_only_severe_regression_pairs": ["pair-004"]}
        result = _implementation_signal_eligibility(**severe)
        self.assertTrue(result["stop_rule_triggered"])
        self.assertFalse(result["implementation_signal_eligible"])

    def test_summary_rejects_pairs_reusing_the_same_fresh_session(self):
        python_record = json.loads(json.dumps(RECORD))
        python_record["run_id"] = "run-001-python"
        python_record["candidate"] = {"language": "python", "version": "3.14.8 with uv"}
        python_record["assignment"]["session_slot"] = "pair-001-python"
        report = summarize_records([RECORD, python_record], CONTEXT)
        self.assertEqual(report["independent_pairs"], 0)
        self.assertEqual(len(report["invalid_records"]), 2)
        self.assertTrue(all("session_id is reused" in item["errors"][-1] for item in report["invalid_records"]))

    def test_summary_rejects_session_reuse_across_pairs_for_the_same_task(self):
        schedule = json.loads(json.dumps(SCHEDULE))
        second_pair = json.loads(json.dumps(schedule["pairs"][0]))
        second_pair["pair_id"] = "pair-002"
        second_pair["pair_order"] = ["python", "veritas"]
        second_pair["sessions"] = [
            {"language": language, "session_slot": f"pair-002-{language}"}
            for language in second_pair["pair_order"]
        ]
        schedule["pairs"].append(second_pair)
        schedule.pop("schedule_sha256", None)
        schedule["schedule_sha256"] = hashlib.sha256(
            json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        context = json.loads(json.dumps(CONTEXT))
        context["schedule"] = schedule
        context["pre_run"]["schedule_sha256"] = schedule["schedule_sha256"]

        records = []
        session_ids = {
            ("pair-001", "veritas"): "session-veritas-001",
            ("pair-001", "python"): "session-python-001",
            ("pair-002", "veritas"): "session-veritas-001",
            ("pair-002", "python"): "session-python-002",
        }
        for pair in schedule["pairs"]:
            for language in ("veritas", "python"):
                record = json.loads(json.dumps(RECORD))
                record["run_id"] = f"run-{pair['pair_id']}-{language}"
                record["pair_id"] = pair["pair_id"]
                record["candidate"] = {"language": language, "version": "3.14.8 with uv" if language == "python" else "RC66"}
                record["model"]["session_id"] = session_ids[(pair["pair_id"], language)]
                record["pair_order"] = pair["pair_order"]
                record["assignment"].update({
                    "session_slot": f"{pair['pair_id']}-{language}",
                    "schedule_sha256": schedule["schedule_sha256"],
                })
                records.append(record)

        report = summarize_records(records, context)
        self.assertEqual(report["independent_pairs"], 0)
        self.assertEqual(len(report["invalid_records"]), 2)
        self.assertEqual(len(report["incomplete_pairs"]), 2)
        self.assertTrue(all("session_id is reused" in item["errors"][-1] for item in report["invalid_records"]))

    def test_summary_rejects_pairs_with_different_hardware_or_budgets(self):
        python_record = json.loads(json.dumps(RECORD))
        python_record["run_id"] = "run-001-python"
        python_record["candidate"] = {"language": "python", "version": "3.14.8 with uv"}
        python_record["model"]["session_id"] = "session-python-001"
        python_record["environment"]["cpu"] = "Different CPU"
        python_record["assignment"]["session_slot"] = "pair-001-python"
        report = summarize_records([RECORD, python_record], CONTEXT)
        self.assertEqual(report["independent_pairs"], 0)
        self.assertEqual(len(report["incomplete_pairs"]), 1)

    def test_summary_rejects_pairs_from_different_docker_engines(self):
        python_record = json.loads(json.dumps(RECORD))
        python_record["run_id"] = "run-001-python"
        python_record["candidate"] = {"language": "python", "version": "3.14.8 with uv"}
        python_record["model"]["session_id"] = "session-python-001"
        python_record["assignment"]["session_slot"] = "pair-001-python"
        python_record["environment"]["host_id"] = "other-engine"
        report = summarize_records([RECORD, python_record], CONTEXT)
        self.assertEqual(report["independent_pairs"], 0)
        self.assertEqual(len(report["incomplete_pairs"]), 1)

    def test_validation_requires_docker_engine_host_id(self):
        record = json.loads(json.dumps(RECORD))
        del record["environment"]["host_id"]
        valid, errors, _ = validate_record(record, CONTEXT)
        self.assertFalse(valid)
        self.assertTrue(any("environment.host_id" in error for error in errors))

    def test_cli_reports_invalid_record_with_nonzero_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "bad.json"
            path.write_text(json.dumps({"schema_version": 1}), encoding="utf-8")
            schedule_path = Path(temporary) / "schedule.json"
            schedule_path.write_text(json.dumps(SCHEDULE), encoding="utf-8")
            schedule_file_hash = hashlib.sha256(schedule_path.read_bytes()).hexdigest()
            pre_run_path = Path(temporary) / "prerun.jsonl"
            pre_run_path.write_text(json.dumps({"status":"recorded_before_formal_runs","schedule_sha256":SCHEDULE["schedule_sha256"],"schedule_file_sha256":schedule_file_hash,"primary_measure":"active_seconds"})+"\n", encoding="utf-8")
            evidence_root = Path(temporary) / "evidence"
            evidence_root.mkdir()
            result = subprocess.run(
                [sys.executable, "evaluation/protocol/ledger.py", "validate", str(path), "--schedule", str(schedule_path), "--prerun-records", str(pre_run_path), "--evidence-root", str(evidence_root)],
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
