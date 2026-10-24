"""Validate and summarize immutable Veritas/Python comparison run records."""

from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import statistics
import sys
from typing import Any

try:
    from .usage_evidence import delivery_tree_sha256, read_thread_usage_snapshot, sha256_file
except ImportError:  # Executed directly by the command-line interface.
    from usage_evidence import delivery_tree_sha256, read_thread_usage_snapshot, sha256_file


HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
LANGUAGES = {"veritas", "python"}
ORIGINS = {"independent", "known", "calibration"}
STATUSES = {"passed", "failed", "incomplete"}
CLASS_PAIR_COUNT = 6
MINIMUM_COMPLETIONS_PER_LANGUAGE = 5
MINIMUM_GUARDRAIL_PAIRS = 5
MINIMUM_FAVORABLE_PAIRS = 4
MINIMUM_MEDIAN_REDUCTION = 0.20


def _object(value: Any, label: str, errors: list[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        errors.append(f"{label} must be an object")
        return {}
    return value


def _required(obj: dict[str, Any], key: str, label: str, errors: list[str]) -> Any:
    if key not in obj:
        errors.append(f"{label}.{key} is required")
        return None
    return obj[key]


def _nonnegative_number(value: Any, label: str, errors: list[str], *, integer: bool = False) -> None:
    valid_type = isinstance(value, int) and not isinstance(value, bool) if integer else isinstance(value, (int, float)) and not isinstance(value, bool)
    if not valid_type or value < 0:
        errors.append(f"{label} must be a non-negative {'integer' if integer else 'number'}")


def _sha256(value: Any, label: str, errors: list[str]) -> None:
    if not isinstance(value, str) or not HEX_SHA256.fullmatch(value):
        errors.append(f"{label} must be a lowercase SHA-256 hex digest")


def _timestamp(value: Any, label: str, errors: list[str]) -> None:
    if not isinstance(value, str):
        errors.append(f"{label} must be an ISO-8601 timestamp")
        return
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None or parsed.utcoffset() is None:
            errors.append(f"{label} must include a timezone")
    except ValueError:
        errors.append(f"{label} must be an ISO-8601 timestamp")


def _evidence_path(root: Path, value: Any, label: str, errors: list[str], *, directory: bool = False) -> Path | None:
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{label} must be a non-empty relative path")
        return None
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        errors.append(f"{label} must stay inside the evidence root")
        return None
    base = root.resolve()
    resolved = (base / relative).resolve()
    try:
        resolved.relative_to(base)
    except ValueError:
        errors.append(f"{label} escapes the evidence root")
        return None
    if directory and not resolved.is_dir():
        errors.append(f"{label} does not exist as a directory")
        return None
    if not directory and not resolved.is_file():
        errors.append(f"{label} does not exist as a file")
        return None
    return resolved


def _verify_evidence_file(root: Path, path_value: Any, digest_value: Any, label: str, errors: list[str]) -> Path | None:
    _sha256(digest_value, f"{label}_sha256", errors)
    path = _evidence_path(root, path_value, f"{label}_path", errors)
    if path is not None and isinstance(digest_value, str) and HEX_SHA256.fullmatch(digest_value):
        try:
            actual = sha256_file(path)
        except OSError as exc:
            errors.append(f"{label}_path cannot be read: {exc}")
        else:
            if actual != digest_value:
                errors.append(f"{label}_sha256 does not match its evidence file")
    return path


def _validate_runner_evidence(
    *, record: dict[str, Any], context: dict[str, Any] | None, stage: str,
    stage_evidence: dict[str, Any], evidence_root: Path, errors: list[str]
) -> None:
    label = f"evidence.stages.{stage}.runner_report"
    report_path = _verify_evidence_file(
        evidence_root,
        stage_evidence.get("runner_report_path"),
        stage_evidence.get("runner_report_sha256"),
        label,
        errors,
    )
    if report_path is None:
        return
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"{label} is not readable JSON: {exc}")
        return
    if not isinstance(report, dict) or report.get("schema_version") != 1:
        errors.append(f"{label} must be a schema_version 1 object")
        return
    assignment = record.get("assignment", {})
    task = record.get("task", {})
    candidate = record.get("candidate", {})
    model = record.get("model", {})
    environment = record.get("environment", {})
    expected = {
        "pair_id": record.get("pair_id"),
        "stage": stage,
        "language": candidate.get("language"),
        "candidate_version": candidate.get("version"),
        "session_id": model.get("session_id"),
        "task_id": task.get("id"),
        "manifest_sha256": assignment.get("manifest_sha256"),
        "schedule_sha256": assignment.get("schedule_sha256"),
    }
    if isinstance(context, dict):
        schedule_manifest = context.get("schedule", {}).get("manifest_sha256")
        if assignment.get("manifest_sha256") != schedule_manifest:
            errors.append("assignment.manifest_sha256 does not match the frozen schedule")
    for key, value in expected.items():
        if report.get(key) != value:
            errors.append(f"{label} {key} does not match the run record")
    container = report.get("container")
    if not isinstance(container, dict) or container.get("docker_engine_id") != environment.get("host_id"):
        errors.append(f"{label} Docker Engine ID does not match environment.host_id")
    case_report = report.get("case_report")
    if not isinstance(case_report, dict) or case_report.get("task_id") != task.get("id") or case_report.get("stage") != stage:
        errors.append(f"{label} case report is not bound to the assigned task and stage")
    if not isinstance(container, dict) or container.get("delivery_source_project_mounted") is not False:
        errors.append(f"{label} does not establish source-free delivery")
    if isinstance(case_report, dict):
        delivery_report = case_report.get("delivery")
        if not isinstance(delivery_report, dict):
            errors.append(f"{label} has no delivery report")
        elif delivery_report.get("artifact_sha256") != report.get("delivery_artifact_sha256"):
            errors.append(f"{label} delivery hash is inconsistent within the runner report")
        source_report = case_report.get("source")
        source_passed = isinstance(source_report, dict) and source_report.get("passed") is True
        delivery_passed = isinstance(delivery_report, dict) and delivery_report.get("passed") is True
        report_passed = case_report.get("passed")
        if not isinstance(report_passed, bool) or report_passed != (source_passed and delivery_passed):
            errors.append(f"{label} overall success does not match its source and delivery results")
        stage_outcome = record.get("outcome", {}).get(stage, {})
        if isinstance(report_passed, bool) and report_passed != (stage_outcome.get("status") == "passed"):
            errors.append(f"{label} result success does not match the recorded stage status")

    delivery_relative = report.get("delivery_artifact_path")
    if not isinstance(delivery_relative, str) or Path(delivery_relative).is_absolute() or ".." in Path(delivery_relative).parts:
        errors.append(f"{label} delivery_artifact_path must be a safe report-relative path")
    else:
        delivery_path = (report_path.parent / delivery_relative).resolve()
        try:
            delivery_path.relative_to(evidence_root.resolve())
        except ValueError:
            errors.append(f"{label} delivery artifact escapes the evidence root")
        else:
            if not delivery_path.is_dir():
                errors.append(f"{label} delivery artifact directory is missing")
            elif isinstance(report.get("delivery_artifact_sha256"), str):
                try:
                    actual_delivery_sha = delivery_tree_sha256(delivery_path)
                except OSError as exc:
                    errors.append(f"{label} delivery artifact cannot be read: {exc}")
                else:
                    if actual_delivery_sha != report.get("delivery_artifact_sha256"):
                        errors.append(f"{label} delivery tree digest does not match the runner report")
            elif report.get("delivery_artifact_sha256") is None:
                if any(delivery_path.iterdir()):
                    errors.append(f"{label} has unreported files in an unaccepted delivery tree")
            elif report.get("delivery_artifact_sha256") is not None:
                errors.append(f"{label} delivery_artifact_sha256 is invalid")

    manifest_relative = report.get("delivery_manifest_path")
    manifest_digest = report.get("delivery_manifest_sha256")
    if manifest_relative is not None:
        if not isinstance(manifest_relative, str) or Path(manifest_relative).is_absolute() or ".." in Path(manifest_relative).parts:
            errors.append(f"{label} delivery_manifest_path must be a safe report-relative path")
        else:
            manifest_path = (report_path.parent / manifest_relative).resolve()
            try:
                manifest_path.relative_to(evidence_root.resolve())
            except ValueError:
                errors.append(f"{label} delivery manifest escapes the evidence root")
            else:
                if not manifest_path.is_file():
                    errors.append(f"{label} delivery manifest is missing")
                else:
                    try:
                        actual_manifest_sha = sha256_file(manifest_path)
                    except OSError as exc:
                        errors.append(f"{label} delivery manifest cannot be read: {exc}")
                    else:
                        if actual_manifest_sha != manifest_digest:
                            errors.append(f"{label} delivery manifest digest does not match the runner report")
    elif isinstance(case_report, dict) and case_report.get("passed") is True:
        errors.append(f"{label} passed delivery has no captured delivery manifest")


def _implementation_signal_eligibility(
    *,
    pairs: int,
    distinct_briefs: int,
    completion_counts: dict[str, int],
    guardrail_pairs: int,
    median_primary_reduction: float | None,
    favorable_pairs: int,
    veritas_only_severe_regression_pairs: list[str],
) -> dict[str, Any]:
    completion_floor_pass = all(
        completion_counts.get(language, 0) >= MINIMUM_COMPLETIONS_PER_LANGUAGE
        for language in LANGUAGES
    )
    completion_parity_pass = (
        completion_counts.get("veritas", 0) >= completion_counts.get("python", 0)
    )
    guardrail_floor_pass = guardrail_pairs >= MINIMUM_GUARDRAIL_PAIRS
    benefit_pass = (
        median_primary_reduction is not None
        and median_primary_reduction >= MINIMUM_MEDIAN_REDUCTION
        and favorable_pairs >= MINIMUM_FAVORABLE_PAIRS
    )
    stop_rule_triggered = bool(veritas_only_severe_regression_pairs)
    complete_design = pairs == CLASS_PAIR_COUNT and distinct_briefs == 3
    return {
        "minimum_completions_per_language": MINIMUM_COMPLETIONS_PER_LANGUAGE,
        "completion_floor_pass": completion_floor_pass,
        "completion_parity_pass": completion_parity_pass,
        "minimum_guardrail_pairs": MINIMUM_GUARDRAIL_PAIRS,
        "guardrail_floor_pass": guardrail_floor_pass,
        "minimum_median_primary_reduction": MINIMUM_MEDIAN_REDUCTION,
        "minimum_favorable_pairs": MINIMUM_FAVORABLE_PAIRS,
        "benefit_threshold_pass": benefit_pass,
        "stop_rule_triggered": stop_rule_triggered,
        "complete_design_pass": complete_design,
        "implementation_signal_eligible": (
            complete_design
            and completion_floor_pass
            and completion_parity_pass
            and guardrail_floor_pass
            and benefit_pass
            and not stop_rule_triggered
        ),
    }


def _validate_assignment(record: dict[str, Any], context: dict[str, Any] | None, errors: list[str]) -> None:
    assignment = _object(_required(record, "assignment", "record", errors), "assignment", errors)
    slot = _required(assignment, "session_slot", "assignment", errors)
    if not isinstance(slot, str) or not slot.strip():
        errors.append("assignment.session_slot must be a non-empty string")
    for key in ("schedule_sha256", "schedule_file_sha256", "prerun_records_sha256", "campaign_config_sha256", "manifest_sha256"):
        _sha256(_required(assignment, key, "assignment", errors), f"assignment.{key}", errors)
    primary_measure = _required(assignment, "primary_measure", "assignment", errors)
    if not isinstance(primary_measure, str) or primary_measure not in {"active_seconds", "human_interventions"}:
        errors.append("assignment.primary_measure must be active_seconds or human_interventions")
    if context is None:
        errors.append("frozen schedule and pre-run records are required to validate an independent assignment")
        return

    schedule = context["schedule"]
    pre_run = context["pre_run"]
    pair_id = record.get("pair_id")
    matches = [pair for pair in schedule.get("pairs", []) if pair.get("pair_id") == pair_id]
    if len(matches) != 1:
        errors.append("pair_id must identify exactly one assignment in the frozen schedule")
        return
    pair = matches[0]
    language = record.get("candidate", {}).get("language")
    sessions = [session for session in pair.get("sessions", []) if session.get("language") == language]
    if len(sessions) != 1 or sessions[0].get("session_slot") != slot:
        errors.append("assignment.session_slot does not match this language's frozen schedule slot")
    task = record.get("task", {})
    if any(task.get(key) != pair.get(schedule_key) for key, schedule_key in (("id", "task_id"), ("class", "task_class"), ("brief_sha256", "brief_sha256"))):
        errors.append("task identity does not match the frozen schedule assignment")
    if record.get("pair_order") != pair.get("pair_order"):
        errors.append("pair_order does not match the frozen schedule assignment")
    if assignment.get("schedule_sha256") != schedule.get("schedule_sha256"):
        errors.append("assignment.schedule_sha256 does not match the frozen schedule")
    if assignment.get("schedule_file_sha256") != context["schedule_file_sha256"]:
        errors.append("assignment.schedule_file_sha256 does not match the schedule file")
    if assignment.get("prerun_records_sha256") != context["prerun_records_sha256"]:
        errors.append("assignment.prerun_records_sha256 does not match the pre-run records")
    if assignment.get("primary_measure") != pre_run.get("primary_measure"):
        errors.append("assignment.primary_measure does not match the pre-run declaration")
    campaign = schedule.get("campaign", {})
    if assignment.get("campaign_config_sha256") != schedule.get("campaign_config_sha256"):
        errors.append("assignment.campaign_config_sha256 does not match the frozen schedule")
    if assignment.get("manifest_sha256") != schedule.get("manifest_sha256"):
        errors.append("assignment.manifest_sha256 does not match the frozen schedule")
    model = record.get("model", {})
    scheduled_model = campaign.get("assistant_model", {}) if isinstance(campaign, dict) else {}
    if any(model.get(key) != scheduled_model.get(key) for key in ("provider", "name", "version", "reasoning_effort")):
        errors.append("record model does not match the frozen campaign model")
    if record.get("budget") != campaign.get("build_budget"):
        errors.append("record budget does not match the frozen campaign budget")


def validate_record(
    record: Any, context: dict[str, Any] | None = None, evidence_root: Path | None = None
) -> tuple[bool, list[str], dict[str, Any]]:
    """Return validity, actionable field errors, and derived outcome facts."""
    errors: list[str] = []
    root = _object(record, "record", errors)
    if isinstance(root.get("schema_version"), bool) or root.get("schema_version") != 1:
        errors.append("schema_version must equal 1")
    for key in ("run_id", "pair_id"):
        value = _required(root, key, "record", errors)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{key} must be a non-empty string")

    task = _object(_required(root, "task", "record", errors), "task", errors)
    for key in ("id", "class"):
        value = _required(task, key, "task", errors)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"task.{key} must be a non-empty string")
    origin = _required(task, "origin", "task", errors)
    if not isinstance(origin, str) or origin not in ORIGINS:
        errors.append("task.origin must be independent, known, or calibration")
    _sha256(_required(task, "brief_sha256", "task", errors), "task.brief_sha256", errors)

    candidate = _object(_required(root, "candidate", "record", errors), "candidate", errors)
    language = _required(candidate, "language", "candidate", errors)
    if not isinstance(language, str) or language not in LANGUAGES:
        errors.append("candidate.language must be veritas or python")
    version = _required(candidate, "version", "candidate", errors)
    if not isinstance(version, str) or not version.strip():
        errors.append("candidate.version must be a non-empty string")

    model = _object(_required(root, "model", "record", errors), "model", errors)
    for key in ("provider", "name", "version", "session_id"):
        value = _required(model, key, "model", errors)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"model.{key} must be a non-empty string")
    if _required(model, "fresh_session", "model", errors) is not True:
        errors.append("model.fresh_session must be true for every comparison run")

    if task.get("origin") == "independent":
        _validate_assignment(root, context, errors)

    environment = _object(_required(root, "environment", "record", errors), "environment", errors)
    for key in ("os", "architecture", "cpu", "runtime_tooling", "network_mode", "host_id"):
        value = _required(environment, key, "environment", errors)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"environment.{key} must be a non-empty string")

    order = _required(root, "pair_order", "record", errors)
    if not isinstance(order, list) or len(order) != 2 or not all(isinstance(item, str) for item in order) or set(order) != LANGUAGES:
        errors.append("pair_order must list veritas and python exactly once")

    budget = _object(_required(root, "budget", "record", errors), "budget", errors)
    for key in ("active_minutes", "tool_calls"):
        _nonnegative_number(_required(budget, key, "budget", errors), f"budget.{key}", errors, integer=True)

    timestamps = _object(_required(root, "timestamps", "record", errors), "timestamps", errors)
    timestamp_values = {}
    for key in ("started_at", "ended_at"):
        timestamp_values[key] = _required(timestamps, key, "timestamps", errors)
        _timestamp(timestamp_values[key], f"timestamps.{key}", errors)
    try:
        started = datetime.fromisoformat(timestamp_values["started_at"].replace("Z", "+00:00"))
        ended = datetime.fromisoformat(timestamp_values["ended_at"].replace("Z", "+00:00"))
        if started.tzinfo is not None and ended.tzinfo is not None and ended < started:
            errors.append("timestamps.ended_at must be at or after timestamps.started_at")
    except (AttributeError, TypeError, ValueError):
        pass

    outcome = _object(_required(root, "outcome", "record", errors), "outcome", errors)
    completed = True
    for stage in ("initial", "follow_up"):
        stage_data = _object(_required(outcome, stage, "outcome", errors), f"outcome.{stage}", errors)
        status = _required(stage_data, "status", f"outcome.{stage}", errors)
        if not isinstance(status, str) or status not in STATUSES:
            errors.append(f"outcome.{stage}.status must be passed, failed, or incomplete")
        passed = _required(stage_data, "acceptance_passed", f"outcome.{stage}", errors)
        total = _required(stage_data, "acceptance_total", f"outcome.{stage}", errors)
        _nonnegative_number(passed, f"outcome.{stage}.acceptance_passed", errors, integer=True)
        _nonnegative_number(total, f"outcome.{stage}.acceptance_total", errors, integer=True)
        if isinstance(passed, int) and isinstance(total, int) and passed > total:
            errors.append(f"outcome.{stage}.acceptance_passed cannot exceed acceptance_total")
        valid_counts = isinstance(passed, int) and not isinstance(passed, bool) and isinstance(total, int) and not isinstance(total, bool)
        stage_passed = status == "passed" and valid_counts and passed == total and total > 0
        if status == "passed" and not stage_passed:
            errors.append(f"outcome.{stage} cannot be passed unless all non-empty acceptance cases passed")
        completed = completed and stage_passed
    for key in ("clean_workspace_run", "hidden_system_python_dependency", "severe_regression"):
        if not isinstance(_required(outcome, key, "outcome", errors), bool):
            errors.append(f"outcome.{key} must be boolean")

    costs = _object(_required(root, "costs", "record", errors), "costs", errors)
    for key in ("active_seconds", "tool_calls", "cold_setup_seconds", "warm_run_seconds", "human_interventions", "artifact_bytes", "dependency_bytes"):
        _nonnegative_number(_required(costs, key, "costs", errors), f"costs.{key}", errors, integer=True)
    if isinstance(costs.get("active_seconds"), int) and isinstance(budget.get("active_minutes"), int):
        if costs["active_seconds"] > budget["active_minutes"] * 60:
            errors.append("costs.active_seconds exceeds the frozen active-time budget")
    if isinstance(costs.get("tool_calls"), int) and isinstance(budget.get("tool_calls"), int):
        if costs["tool_calls"] > budget["tool_calls"]:
            errors.append("costs.tool_calls exceeds the frozen tool-call budget")

    commands = _required(root, "commands", "record", errors)
    if not isinstance(commands, list) or not commands or not all(isinstance(item, str) and item.strip() for item in commands):
        errors.append("commands must be a non-empty list of command strings")
    interventions = _required(root, "human_interventions", "record", errors)
    if not isinstance(interventions, list) or not all(isinstance(item, str) for item in interventions):
        errors.append("human_interventions must be a list of descriptions")
    if isinstance(costs.get("human_interventions"), int) and isinstance(interventions, list) and costs["human_interventions"] != len(interventions):
        errors.append("costs.human_interventions must equal the number of recorded intervention descriptions")

    evidence = _object(_required(root, "evidence", "record", errors), "evidence", errors)
    evidence_files = (
        ("prompt_path", "prompt_sha256"),
        ("test_inputs_path", "test_inputs_sha256"),
        ("transcript_path", "transcript_sha256"),
        ("output_path", "output_sha256"),
        ("usage_snapshot_path", "usage_snapshot_sha256"),
    )
    for path_key, digest_key in evidence_files:
        path_value = _required(evidence, path_key, "evidence", errors)
        if not isinstance(path_value, str) or not path_value.strip():
            errors.append(f"evidence.{path_key} must be a non-empty relative path")
        elif Path(path_value).is_absolute() or ".." in Path(path_value).parts:
            errors.append(f"evidence.{path_key} must stay inside the evidence root")
        _sha256(_required(evidence, digest_key, "evidence", errors), f"evidence.{digest_key}", errors)
    stages = _object(_required(evidence, "stages", "evidence", errors), "evidence.stages", errors)
    for stage in ("initial", "follow_up"):
        stage_evidence = _object(_required(stages, stage, "evidence.stages", errors), f"evidence.stages.{stage}", errors)
        _required(stage_evidence, "runner_report_path", f"evidence.stages.{stage}", errors)
        _sha256(_required(stage_evidence, "runner_report_sha256", f"evidence.stages.{stage}", errors), f"evidence.stages.{stage}.runner_report_sha256", errors)
    if evidence_root is not None:
        for path_key, digest_key in evidence_files:
            _verify_evidence_file(evidence_root, evidence.get(path_key), evidence.get(digest_key), f"evidence.{path_key}", errors)
        usage_path = _evidence_path(evidence_root, evidence.get("usage_snapshot_path"), "evidence.usage_snapshot_path", errors)
        if usage_path is not None:
            try:
                measured = read_thread_usage_snapshot(usage_path, model.get("session_id", ""))
            except ValueError as exc:
                errors.append(f"evidence.usage_snapshot_path: {exc}")
            else:
                if measured["active_seconds"] != costs.get("active_seconds"):
                    errors.append("costs.active_seconds does not match measured thread usage")
                if measured["tool_calls"] != costs.get("tool_calls"):
                    errors.append("costs.tool_calls does not match measured thread usage")
        for stage in ("initial", "follow_up"):
            stage_evidence = stages.get(stage)
            if isinstance(stage_evidence, dict):
                _validate_runner_evidence(
                    record=root,
                    context=context,
                    stage=stage,
                    stage_evidence=stage_evidence,
                    evidence_root=evidence_root,
                    errors=errors,
                )

    review = _object(_required(root, "review", "record", errors), "review", errors)
    for key in ("reviewer_id", "verdict", "notes"):
        value = _required(review, key, "review", errors)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"review.{key} must be a non-empty string")

    exclusion = _object(_required(root, "exclusion", "record", errors), "exclusion", errors)
    excluded = _required(exclusion, "excluded", "exclusion", errors)
    reason = _required(exclusion, "reason", "exclusion", errors)
    if not isinstance(excluded, bool):
        errors.append("exclusion.excluded must be boolean")
    elif excluded and (not isinstance(reason, str) or not reason.strip()):
        errors.append("exclusion.reason is required when a run is excluded")
    elif not excluded and reason is not None:
        errors.append("exclusion.reason must be null when a run is not excluded")

    return not errors, errors, {"primary_success": completed}


def summarize_records(
    records: list[Any], context: dict[str, Any] | None = None, evidence_root: Path | None = None
) -> dict[str, Any]:
    """Summarize all attempts while keeping known work out of independent counts."""
    valid_records = 0
    invalid_records = []
    known_or_calibration = 0
    independent: dict[str, list[dict[str, Any]]] = {}
    checked = [validate_record(record, context, evidence_root) for record in records]
    session_uses: dict[tuple[str, str], list[int]] = {}
    for index, record in enumerate(records):
        if not isinstance(record, dict):
            continue
        task = record.get("task")
        model = record.get("model")
        if not isinstance(task, dict) or task.get("origin") != "independent" or not isinstance(model, dict):
            continue
        task_id = task.get("id")
        session_id = model.get("session_id")
        if isinstance(task_id, str) and isinstance(session_id, str):
            session_uses.setdefault((task_id, session_id), []).append(index)
    reused_sessions = {
        index
        for indices in session_uses.values()
        if len(indices) > 1
        for index in indices
    }
    for index, record in enumerate(records):
        valid, errors, derived = checked[index]
        if index in reused_sessions:
            errors = [*errors, "model.session_id is reused for this task; every assigned run requires a fresh session"]
            valid = False
        if not valid:
            invalid_records.append({"index": index, "run_id": record.get("run_id") if isinstance(record, dict) else None, "errors": errors})
            continue
        valid_records += 1
        if record["task"]["origin"] != "independent":
            known_or_calibration += 1
            continue
        if record["exclusion"]["excluded"]:
            continue
        independent.setdefault(record["pair_id"], []).append({"record": record, "primary_success": derived["primary_success"]})

    complete_pairs: list[tuple[str, list[dict[str, Any]]]] = []
    incomplete_pairs: list[dict[str, Any]] = []
    for pair_id, members in sorted(independent.items()):
        languages = [item["record"]["candidate"]["language"] for item in members]
        if len(members) != 2 or sorted(languages) != ["python", "veritas"]:
            incomplete_pairs.append({"pair_id": pair_id, "run_count": len(members), "languages": languages})
            continue
        left, right = members[0]["record"], members[1]["record"]
        identity_keys = ("id", "class", "brief_sha256")
        same_task = all(left["task"][key] == right["task"][key] for key in identity_keys)
        same_model = all(left["model"][key] == right["model"][key] for key in ("provider", "name", "version"))
        same_order = left["pair_order"] == right["pair_order"]
        same_campaign = all(left["assignment"].get(key) == right["assignment"].get(key) for key in ("schedule_sha256", "schedule_file_sha256", "prerun_records_sha256", "campaign_config_sha256", "primary_measure"))
        fresh_contexts = left["model"]["session_id"] != right["model"]["session_id"]
        same_hardware = all(left["environment"][key] == right["environment"][key] for key in ("os", "architecture", "cpu", "network_mode", "host_id"))
        same_budget = left["budget"] == right["budget"]
        distinct_runs = left["run_id"] != right["run_id"]
        if not (same_task and same_model and same_order and same_campaign and fresh_contexts and same_hardware and same_budget and distinct_runs):
            incomplete_pairs.append({"pair_id": pair_id, "run_count": len(members), "error": "paired records disagree on task, model, randomized order, campaign assignment, fresh contexts, hardware or host ID, budget, or unique run identity"})
            continue
        complete_pairs.append((pair_id, members))

    by_class: dict[str, dict[str, Any]] = {}
    pair_reports = []
    for pair_id, members in complete_pairs:
        task = members[0]["record"]["task"]
        task_class = task["class"]
        by_language = {
            item["record"]["candidate"]["language"]: item
            for item in members
        }
        veritas = by_language["veritas"]
        python = by_language["python"]

        def guardrails_pass(item: dict[str, Any]) -> bool:
            outcome = item["record"]["outcome"]
            return (
                item["primary_success"]
                and outcome["clean_workspace_run"]
                and not outcome["hidden_system_python_dependency"]
                and not outcome["severe_regression"]
            )

        guardrail_pair = guardrails_pass(veritas) and guardrails_pass(python)
        veritas_only_severe = (
            veritas["record"]["outcome"]["severe_regression"]
            and not python["record"]["outcome"]["severe_regression"]
        )
        veritas_costs = veritas["record"]["costs"]
        python_costs = python["record"]["costs"]
        time_reduction = None
        if guardrail_pair and python_costs["active_seconds"] > 0:
            time_reduction = (
                python_costs["active_seconds"] - veritas_costs["active_seconds"]
            ) / python_costs["active_seconds"]
        intervention_reduction = None
        if guardrail_pair and python_costs["human_interventions"] > 0:
            intervention_reduction = (
                python_costs["human_interventions"] - veritas_costs["human_interventions"]
            ) / python_costs["human_interventions"]

        pair_reports.append({
            "pair_id": pair_id,
            "task_id": task["id"],
            "class": task_class,
            "guardrail_pair": guardrail_pair,
            "primary_success": {
                "veritas": veritas["primary_success"],
                "python": python["primary_success"],
            },
            "veritas_only_severe_regression": veritas_only_severe,
            "primary_measure": veritas["record"]["assignment"]["primary_measure"],
            "active_seconds": {
                "veritas": veritas_costs["active_seconds"],
                "python": python_costs["active_seconds"],
            },
            "human_interventions": {
                "veritas": veritas_costs["human_interventions"],
                "python": python_costs["human_interventions"],
            },
            "veritas_time_reduction": time_reduction,
            "veritas_intervention_reduction": intervention_reduction,
            "at_least_20_percent_time_reduction": time_reduction is not None and time_reduction >= 0.20,
            "at_least_20_percent_intervention_reduction": intervention_reduction is not None and intervention_reduction >= 0.20,
            "at_least_20_percent_primary_reduction": (time_reduction if veritas["record"]["assignment"]["primary_measure"] == "active_seconds" else intervention_reduction) is not None and (time_reduction if veritas["record"]["assignment"]["primary_measure"] == "active_seconds" else intervention_reduction) >= 0.20,
        })

        bucket = by_class.setdefault(task_class, {
            "pairs": 0,
            "brief_ids": set(),
            "paired_primary_successes": 0,
            "completion_counts": {"veritas": 0, "python": 0},
            "guardrail_pairs": 0,
            "veritas_only_severe_regression_pairs": [],
            "time_reductions": [],
            "intervention_reductions": [],
            "pairs_at_least_20_percent_faster": 0,
            "pairs_with_at_least_20_percent_fewer_interventions": 0,
            "primary_reductions": [],
            "pairs_at_least_20_percent_primary_reduction": 0,
        })
        bucket["pairs"] += 1
        bucket["brief_ids"].add(task["id"])
        for language, item in by_language.items():
            if item["primary_success"]:
                bucket["completion_counts"][language] += 1
        if veritas_only_severe:
            bucket["veritas_only_severe_regression_pairs"].append(pair_id)
        if all(item["primary_success"] for item in members):
            bucket["paired_primary_successes"] += 1
        if guardrail_pair:
            bucket["guardrail_pairs"] += 1
            if time_reduction is not None:
                bucket["time_reductions"].append(time_reduction)
            if intervention_reduction is not None:
                bucket["intervention_reductions"].append(intervention_reduction)
            primary_reduction = time_reduction if veritas["record"]["assignment"]["primary_measure"] == "active_seconds" else intervention_reduction
            if primary_reduction is not None:
                bucket["primary_reductions"].append(primary_reduction)
            if pair_reports[-1]["at_least_20_percent_time_reduction"]:
                bucket["pairs_at_least_20_percent_faster"] += 1
            if pair_reports[-1]["at_least_20_percent_intervention_reduction"]:
                bucket["pairs_with_at_least_20_percent_fewer_interventions"] += 1
            if pair_reports[-1]["at_least_20_percent_primary_reduction"]:
                bucket["pairs_at_least_20_percent_primary_reduction"] += 1
    class_report = {}
    for key, value in sorted(by_class.items()):
        median_primary_reduction = statistics.median(value["primary_reductions"]) if value["primary_reductions"] else None
        eligibility = _implementation_signal_eligibility(
            pairs=value["pairs"],
            distinct_briefs=len(value["brief_ids"]),
            completion_counts=value["completion_counts"],
            guardrail_pairs=value["guardrail_pairs"],
            median_primary_reduction=median_primary_reduction,
            favorable_pairs=value["pairs_at_least_20_percent_primary_reduction"],
            veritas_only_severe_regression_pairs=value["veritas_only_severe_regression_pairs"],
        )
        class_report[key] = {
            "pairs": value["pairs"],
            "distinct_briefs": len(value["brief_ids"]),
            "paired_primary_successes": value["paired_primary_successes"],
            "completion_counts": value["completion_counts"],
            "guardrail_pairs": value["guardrail_pairs"],
            "veritas_only_severe_regression_pairs": value["veritas_only_severe_regression_pairs"],
            "median_time_reduction": statistics.median(value["time_reductions"]) if value["time_reductions"] else None,
            "median_intervention_reduction": statistics.median(value["intervention_reductions"]) if value["intervention_reductions"] else None,
            "pairs_at_least_20_percent_faster": value["pairs_at_least_20_percent_faster"],
            "pairs_with_at_least_20_percent_fewer_interventions": value["pairs_with_at_least_20_percent_fewer_interventions"],
            "median_primary_reduction": median_primary_reduction,
            "pairs_at_least_20_percent_primary_reduction": value["pairs_at_least_20_percent_primary_reduction"],
            **eligibility,
        }
    
    adequately_sampled = len(class_report) >= 3 and all(
        value["pairs"] >= 6 and value["distinct_briefs"] >= 3
        for value in class_report.values()
    )
    return {
        "kind": "veritas_evaluation_ledger_summary",
        "schema_version": 1,
        "primary_measure": context["pre_run"]["primary_measure"] if context else None,
        "total_records": len(records),
        "valid_records": valid_records,
        "invalid_records": invalid_records,
        "known_or_calibration_records": known_or_calibration,
        "independent_pairs": len(complete_pairs),
        "incomplete_pairs": incomplete_pairs,
        "classes": class_report,
        "pairs": pair_reports,
        "recommendation_gate": "ready_for_blinded_review" if adequately_sampled and not invalid_records and not incomplete_pairs else "insufficient_independent_evidence",
        "gate_note": "This checks evidence completeness only; it never selects a recommended language.",
    }


def _load_records(path: Path) -> tuple[list[Any], list[dict[str, Any]]]:
    records: list[Any] = []
    errors: list[dict[str, Any]] = []
    for file in sorted(path.glob("*.json")):
        try:
            records.append(json.loads(file.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError) as exc:
            errors.append({"file": str(file), "errors": [str(exc)]})
    return records, errors


def load_campaign_context(schedule_path: Path, prerun_records_path: Path) -> dict[str, Any]:
    schedule = json.loads(schedule_path.read_text(encoding="utf-8"))
    if not isinstance(schedule, dict) or schedule.get("schema_version") != 2 or schedule.get("status") != "assigned-before-runs":
        raise ValueError("schedule is not assigned for formal runs")
    canonical = {key: value for key, value in schedule.items() if key != "schedule_sha256"}
    expected_schedule_digest = hashlib.sha256(json.dumps(canonical, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()
    if schedule.get("schedule_sha256") != expected_schedule_digest:
        raise ValueError("schedule digest is missing or invalid")
    schedule_file_sha256 = hashlib.sha256(schedule_path.read_bytes()).hexdigest()
    lines = [json.loads(line) for line in prerun_records_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if any(not isinstance(item, dict) for item in lines):
        raise ValueError("each pre-run line must contain a JSON object")
    matching = [item for item in lines if item.get("schedule_sha256") == schedule["schedule_sha256"]]
    if len(matching) != 1:
        raise ValueError("pre-run records must contain exactly one declaration for this schedule")
    pre_run = matching[0]
    if pre_run.get("campaign_config_sha256") != schedule.get("campaign_config_sha256") or pre_run.get("campaign") != schedule.get("campaign"):
        raise ValueError("pre-run campaign does not match the frozen schedule")
    if pre_run.get("status") != "recorded_before_formal_runs":
        raise ValueError("pre-run declaration is not approved for formal runs")
    if pre_run.get("schedule_file_sha256") != schedule_file_sha256:
        raise ValueError("pre-run declaration does not match the schedule file")
    prerun_records_sha256 = hashlib.sha256(prerun_records_path.read_bytes()).hexdigest()
    if pre_run.get("primary_measure") not in {"active_seconds", "human_interventions"}:
        raise ValueError("pre-run declaration has no valid primary measure")
    return {"schedule": schedule, "pre_run": pre_run, "schedule_file_sha256": schedule_file_sha256, "prerun_records_sha256": prerun_records_sha256}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate", help="validate one immutable run record")
    validate_parser.add_argument("record", type=Path)
    validate_parser.add_argument("--schedule", type=Path, required=True)
    validate_parser.add_argument("--prerun-records", type=Path, required=True)
    validate_parser.add_argument("--evidence-root", type=Path, required=True)
    summarize_parser = subparsers.add_parser("summarize", help="summarize records in a directory")
    summarize_parser.add_argument("directory", type=Path)
    summarize_parser.add_argument("--schedule", type=Path, required=True)
    summarize_parser.add_argument("--prerun-records", type=Path, required=True)
    summarize_parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        context = load_campaign_context(args.schedule, args.prerun_records)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(json.dumps({"kind": "veritas_evaluation_validation_report", "status": "invalid", "errors": [str(exc)]}, sort_keys=True))
        return 1
    if args.command == "validate":
        try:
            record = json.loads(args.record.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            print(json.dumps({"kind": "veritas_evaluation_validation_report", "status": "invalid", "errors": [str(exc)]}, sort_keys=True))
            return 1
        valid, errors, derived = validate_record(record, context, args.evidence_root)
        print(json.dumps({"kind": "veritas_evaluation_validation_report", "status": "valid" if valid else "invalid", "errors": errors, "derived": derived}, sort_keys=True))
        return 0 if valid else 1
    if not args.directory.is_dir():
        print(json.dumps({"kind": "veritas_evaluation_ledger_summary", "status": "invalid", "errors": [f"not a directory: {args.directory}"]}, sort_keys=True))
        return 1
    records, file_errors = _load_records(args.directory)
    summary = summarize_records(records, context, args.evidence_root)
    if file_errors:
        summary["invalid_records"].extend(file_errors)
        summary["recommendation_gate"] = "insufficient_independent_evidence"
    print(json.dumps(summary, sort_keys=True))
    return 1 if summary["invalid_records"] else 0


if __name__ == "__main__":
    sys.exit(main())
