"""Validate and summarize blinded assistant language-choice records."""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from datetime import UTC, datetime
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Any

try:
    from .usage_evidence import read_thread_usage_snapshot
except ImportError:  # Executed directly by the command-line interface.
    from usage_evidence import read_thread_usage_snapshot


LANGUAGES = {"veritas", "python"}
CHOICES = LANGUAGES | {"either"}
ORIGINS = {"independent", "known", "calibration"}
EXPECTED_CLASSES = {
    "small_automation_cli",
    "concurrent_network_integration",
    "maintainable_application",
}
HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
RFC3339_UTC = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$")


def _parse_timestamp(value: Any, label: str, errors: list[str]) -> datetime | None:
    if not isinstance(value, str) or not RFC3339_UTC.fullmatch(value):
        errors.append(f"{label} must be an RFC3339 timestamp")
        return None
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)
    except ValueError:
        errors.append(f"{label} must be a valid RFC3339 timestamp")
        return None
    if parsed.tzinfo is None:
        errors.append(f"{label} must include a timezone")
        return None
    return parsed.astimezone(UTC)


def _object(value: Any, label: str, errors: list[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        errors.append(f"{label} must be an object")
        return {}
    return value


def _string(obj: dict[str, Any], key: str, label: str, errors: list[str]) -> Any:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{label}.{key} must be a non-empty string")
    return value


def validate_record(
    record: Any,
    schedule: dict[str, Any] | None = None,
    prerun_record: dict[str, Any] | None = None,
    prerun_records_sha256: str | None = None,
    evidence_root: Path | None = None,
) -> tuple[bool, list[str], dict[str, Any]]:
    """Validate one recommendation made before any implementation results are shown."""
    errors: list[str] = []
    root = _object(record, "record", errors)
    if isinstance(root.get("schema_version"), bool) or root.get("schema_version") != 1:
        errors.append("schema_version must equal 1")
    _string(root, "decision_id", "record", errors)
    decision_time = _parse_timestamp(root.get("recorded_at_utc"), "record.recorded_at_utc", errors)

    task = _object(root.get("task"), "task", errors)
    for key in ("id", "class"):
        _string(task, key, "task", errors)
    origin = _string(task, "origin", "task", errors)
    if not isinstance(origin, str) or origin not in ORIGINS:
        errors.append("task.origin must be independent, known, or calibration")
    brief_digest = _string(task, "brief_sha256", "task", errors)
    if not isinstance(brief_digest, str) or not HEX_SHA256.fullmatch(brief_digest):
        errors.append("task.brief_sha256 must be a lowercase SHA-256 digest")

    assistant = _object(root.get("assistant"), "assistant", errors)
    for key in ("provider", "name", "version", "reasoning_effort", "session_id"):
        _string(assistant, key, "assistant", errors)
    if assistant.get("fresh_session") is not True:
        errors.append("assistant.fresh_session must be true")

    budget = _object(root.get("budget"), "budget", errors)
    for key, minimum in (("active_minutes", 1), ("tool_calls", 0)):
        value = budget.get(key)
        if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
            errors.append(f"budget.{key} must be an integer of at least {minimum}")

    usage = _object(root.get("usage"), "usage", errors)
    for key in ("active_seconds", "tool_calls"):
        value = usage.get(key)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            errors.append(f"usage.{key} must be a non-negative integer")
    if isinstance(usage.get("active_seconds"), int) and isinstance(budget.get("active_minutes"), int):
        if usage["active_seconds"] > budget["active_minutes"] * 60:
            errors.append("actual usage.active_seconds exceeds the frozen choice budget")
    if isinstance(usage.get("tool_calls"), int) and isinstance(budget.get("tool_calls"), int):
        if usage["tool_calls"] > budget["tool_calls"]:
            errors.append("actual usage.tool_calls exceeds the frozen choice budget")

    evidence = _object(root.get("evidence"), "evidence", errors)
    evidence_pairs = (
        ("prompt_transcript_path", "prompt_transcript_sha256"),
        ("response_transcript_path", "response_transcript_sha256"),
        ("usage_snapshot_path", "usage_snapshot_sha256"),
    )
    for path_key, digest_key in evidence_pairs:
        path_value = _string(evidence, path_key, "evidence", errors)
        if isinstance(path_value, str):
            relative = Path(path_value)
            if relative.is_absolute() or ".." in relative.parts:
                errors.append(f"evidence.{path_key} must stay inside the evidence root")
        digest_value = _string(evidence, digest_key, "evidence", errors)
        if not isinstance(digest_value, str) or not HEX_SHA256.fullmatch(digest_value):
            errors.append(f"evidence.{digest_key} must be a lowercase SHA-256 digest")
    if evidence_root is not None:
        base = evidence_root.resolve()
        for path_key, digest_key in evidence_pairs:
            value = evidence.get(path_key)
            if not isinstance(value, str):
                continue
            path = (base / value).resolve()
            try:
                path.relative_to(base)
            except ValueError:
                errors.append(f"evidence.{path_key} escapes the evidence root")
                continue
            try:
                actual_digest = hashlib.sha256(path.read_bytes()).hexdigest()
            except OSError:
                errors.append(f"evidence.{path_key} does not exist")
                continue
            if actual_digest != evidence.get(digest_key):
                errors.append(f"evidence.{digest_key} does not match its transcript")
            if path_key == "usage_snapshot_path":
                try:
                    measured = read_thread_usage_snapshot(path, assistant.get("session_id", ""))
                except ValueError as exc:
                    errors.append(f"evidence.usage_snapshot_path: {exc}")
                else:
                    if measured["active_seconds"] != usage.get("active_seconds"):
                        errors.append("usage.active_seconds does not match measured thread usage")
                    if measured["tool_calls"] != usage.get("tool_calls"):
                        errors.append("usage.tool_calls does not match measured thread usage")

    presentation_order = root.get("presentation_order")
    if (
        not isinstance(presentation_order, list)
        or len(presentation_order) != 2
        or not all(isinstance(item, str) for item in presentation_order)
        or set(presentation_order) != LANGUAGES
    ):
        errors.append("presentation_order must list veritas and python exactly once")

    context = _object(root.get("context"), "context", errors)
    if context.get("mode") != "pre-build-recommendation":
        errors.append("context.mode must be pre-build-recommendation")
    for key in ("veritas_version", "python_version"):
        _string(context, key, "context", errors)
    documentation_digest = _string(context, "documentation_sha256", "context", errors)
    if not isinstance(documentation_digest, str) or not HEX_SHA256.fullmatch(documentation_digest):
        errors.append("context.documentation_sha256 must be a lowercase SHA-256 digest")
    schedule_digest = _string(context, "schedule_sha256", "context", errors)
    if not isinstance(schedule_digest, str) or not HEX_SHA256.fullmatch(schedule_digest):
        errors.append("context.schedule_sha256 must be a lowercase SHA-256 digest")
    schedule_file_digest = _string(context, "schedule_file_sha256", "context", errors)
    if not isinstance(schedule_file_digest, str) or not HEX_SHA256.fullmatch(schedule_file_digest):
        errors.append("context.schedule_file_sha256 must be a lowercase SHA-256 digest")
    _string(context, "schedule_entry_id", "context", errors)
    prerun_digest = _string(context, "prerun_records_sha256", "context", errors)
    if not isinstance(prerun_digest, str) or not HEX_SHA256.fullmatch(prerun_digest):
        errors.append("context.prerun_records_sha256 must be a lowercase SHA-256 digest")
    prerecord_time = _parse_timestamp(context.get("prerun_recorded_at_utc"), "context.prerun_recorded_at_utc", errors)
    if decision_time is not None and prerecord_time is not None and decision_time <= prerecord_time:
        errors.append("record.recorded_at_utc must be later than the pre-run record")
    if context.get("comparative_pair_results_shown") is not False:
        errors.append("context.comparative_pair_results_shown must be false")

    if prerun_record is not None:
        if context.get("prerun_records_sha256") != prerun_records_sha256:
            errors.append("context.prerun_records_sha256 does not match the pre-run records file")
        if context.get("prerun_recorded_at_utc") != prerun_record.get("recorded_at_utc"):
            errors.append("context.prerun_recorded_at_utc does not match the pre-run record")
        if context.get("schedule_sha256") != prerun_record.get("schedule_sha256"):
            errors.append("pre-run record schedule digest does not match the choice")
        if context.get("schedule_file_sha256") != prerun_record.get("schedule_file_sha256"):
            errors.append("schedule file digest does not match the pre-run record")
        if context.get("campaign_config_sha256") != prerun_record.get("campaign_config_sha256"):
            errors.append("campaign config digest does not match the pre-run record")

    if schedule is not None:
        schedule_payload = dict(schedule)
        schedule_digest_in_file = schedule_payload.pop("schedule_sha256", None)
        schedule_digest_calculated = hashlib.sha256(json.dumps(schedule_payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()
        if schedule.get("schema_version") != 2 or schedule.get("status") != "assigned-before-runs" or schedule_digest_in_file != schedule_digest_calculated:
            errors.append("pre-run schedule status or digest is invalid")
        campaign = schedule.get("campaign", {})
        model = campaign.get("assistant_model", {}) if isinstance(campaign, dict) else {}
        campaign_digest = schedule.get("campaign_config_sha256")
        if context.get("campaign_config_sha256") != campaign_digest:
            errors.append("campaign config digest does not match the pre-run schedule")
        if not isinstance(campaign_digest, str) or not HEX_SHA256.fullmatch(campaign_digest):
            errors.append("pre-run schedule campaign_config_sha256 is invalid")
        if any(assistant.get(key) != model.get(key) for key in ("provider", "name", "version", "reasoning_effort")):
            errors.append("assistant model does not match the frozen campaign model")
        if budget != campaign.get("choice_budget"):
            errors.append("choice budget does not match the frozen campaign budget")
        if prerun_record is not None and prerun_record.get("campaign_config_sha256") != campaign_digest:
            errors.append("pre-run record campaign config digest does not match the schedule")
        expected_entry = next((item for item in schedule.get("choices", []) if item.get("decision_id") == root.get("decision_id")), None)
        if expected_entry is None:
            errors.append("decision_id is absent from the pre-run schedule")
        else:
            if context.get("schedule_sha256") != schedule.get("schedule_sha256"):
                errors.append("context.schedule_sha256 does not match the pre-run schedule")
            if context.get("schedule_entry_id") != expected_entry.get("decision_id"):
                errors.append("context.schedule_entry_id does not match the assigned decision")
            if task.get("id") != expected_entry.get("task_id") or task.get("class") != expected_entry.get("task_class") or brief_digest != expected_entry.get("brief_sha256"):
                errors.append("task identity does not match the pre-run schedule")
            if presentation_order != expected_entry.get("presentation_order"):
                errors.append("presentation_order does not match the pre-run assignment")
            if documentation_digest != expected_entry.get("documentation_sha256") or documentation_digest != schedule.get("documentation_sha256"):
                errors.append("context.documentation_sha256 does not match the scheduled choice bundle")

    recommendation = _object(root.get("recommendation"), "recommendation", errors)
    choice = _string(recommendation, "choice", "recommendation", errors)
    if not isinstance(choice, str) or choice not in CHOICES:
        errors.append("recommendation.choice must be veritas, python, or either")
    _string(recommendation, "rationale", "recommendation", errors)
    conditions = recommendation.get("conditions")
    if not isinstance(conditions, list) or not all(isinstance(item, str) for item in conditions):
        errors.append("recommendation.conditions must be a list of strings")

    exclusion = _object(root.get("exclusion"), "exclusion", errors)
    excluded = exclusion.get("excluded")
    if not isinstance(excluded, bool):
        errors.append("exclusion.excluded must be boolean")
    reason = exclusion.get("reason")
    if excluded is True and (not isinstance(reason, str) or not reason.strip()):
        errors.append("exclusion.reason is required for an excluded decision")
    elif excluded is False and reason is not None:
        errors.append("exclusion.reason must be null for a non-excluded decision")

    return not errors, errors, {"recommends_veritas": choice == "veritas"}


def summarize_records(
    records: list[Any],
    schedule: dict[str, Any] | None = None,
    prerun_record: dict[str, Any] | None = None,
    prerun_records_sha256: str | None = None,
    evidence_root: Path | None = None,
) -> dict[str, Any]:
    """Summarize direct language choices without conflating them with build results."""
    invalid: list[dict[str, Any]] = []
    valid_records: list[dict[str, Any]] = []
    decision_ids: set[str] = set()
    session_ids: set[str] = set()
    cohort_model: tuple[str, ...] | None = None
    for index, record in enumerate(records):
        valid, errors, _ = validate_record(record, schedule, prerun_record, prerun_records_sha256, evidence_root)
        decision_id = record.get("decision_id") if isinstance(record, dict) else None
        if valid:
            model = tuple(record["assistant"][key] for key in ("provider", "name", "version", "reasoning_effort"))
            if cohort_model is not None and model != cohort_model:
                errors.append("assistant model differs from the frozen choice cohort model")
                valid = False
            elif cohort_model is None:
                cohort_model = model
        if valid:
            if decision_id in decision_ids:
                errors.append("decision_id is duplicated")
                valid = False
            session_id = record["assistant"]["session_id"]
            if session_id in session_ids:
                errors.append("assistant session_id is reused; each choice needs a fresh session")
                valid = False
        if not valid:
            invalid.append({"index": index, "decision_id": decision_id, "errors": errors})
            continue
        decision_ids.add(decision_id)
        session_ids.add(session_id)
        valid_records.append(record)

    independent: dict[str, list[dict[str, Any]]] = defaultdict(list)
    known_or_calibration = 0
    for record in valid_records:
        if record["task"]["origin"] != "independent":
            known_or_calibration += 1
        elif not record["exclusion"]["excluded"]:
            independent[record["task"]["class"]].append(record)

    classes: dict[str, Any] = {}
    for task_class, choices in sorted(independent.items()):
        per_brief = Counter(record["task"]["id"] for record in choices)
        counts = Counter(record["recommendation"]["choice"] for record in choices)
        classes[task_class] = {
            "decisions": len(choices),
            "distinct_briefs": len(per_brief),
            "decisions_per_brief": dict(sorted(per_brief.items())),
            "veritas_choices": counts["veritas"],
            "python_choices": counts["python"],
            "either_choices": counts["either"],
            "veritas_majority": counts["veritas"] > len(choices) / 2,
        }

    sample_complete = set(classes) == EXPECTED_CLASSES and all(
        result["decisions"] == 6
        and result["distinct_briefs"] == 3
        and all(count == 2 for count in result["decisions_per_brief"].values())
        for result in classes.values()
    )
    majority_classes = sum(result["veritas_majority"] for result in classes.values())
    return {
        "kind": "veritas_assistant_recommendation_summary",
        "schema_version": 1,
        "total_decisions": len(records),
        "valid_decisions": len(valid_records),
        "invalid_decisions": invalid,
        "known_or_calibration_decisions": known_or_calibration,
        "classes": classes,
        "sample_complete": sample_complete,
        "veritas_majority_classes": majority_classes,
        "direct_recommendation_signal": "veritas_majority_in_two_or_more_classes"
        if sample_complete and majority_classes >= 2
        else "insufficient_direct_recommendation_signal",
        "gate_note": "Direct choices are separate from matched build outcomes; both are required for a task-class recommendation.",
    }


def _load_records(directory: Path) -> tuple[list[Any], list[dict[str, Any]]]:
    records: list[Any] = []
    file_errors: list[dict[str, Any]] = []
    for path in sorted(directory.glob("*.json")):
        try:
            records.append(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError) as exc:
            file_errors.append({"file": str(path), "errors": [str(exc)]})
    return records, file_errors


def _load_prerun(path: Path, schedule: dict[str, Any], schedule_file_sha256: str) -> tuple[dict[str, Any], str]:
    raw = path.read_bytes()
    records = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    if len(records) != 1 or not isinstance(records[0], dict):
        raise ValueError("pre-run records file must contain exactly one record")
    record = records[0]
    if schedule.get("schema_version") != 2 or schedule.get("status") != "assigned-before-runs":
        raise ValueError("schedule is not the frozen campaign schedule")
    if record.get("status") != "recorded_before_formal_runs":
        raise ValueError("pre-run record has an invalid status")
    if record.get("schedule_sha256") != schedule.get("schedule_sha256"):
        raise ValueError("pre-run record does not bind the supplied schedule")
    if record.get("schedule_file_sha256") != schedule_file_sha256:
        raise ValueError("pre-run record does not bind the supplied schedule file")
    if record.get("campaign") != schedule.get("campaign") or record.get("campaign_config_sha256") != schedule.get("campaign_config_sha256"):
        raise ValueError("pre-run campaign does not match the frozen schedule")
    return record, hashlib.sha256(raw).hexdigest()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate_parser = commands.add_parser("validate", help="validate one decision record")
    validate_parser.add_argument("record", type=Path)
    validate_parser.add_argument("--schedule", type=Path, required=True)
    validate_parser.add_argument("--prerun-records", type=Path, required=True)
    validate_parser.add_argument("--evidence-root", type=Path, required=True)
    summarize_parser = commands.add_parser("summarize", help="summarize a directory of decision records")
    summarize_parser.add_argument("directory", type=Path)
    summarize_parser.add_argument("--schedule", type=Path, required=True)
    summarize_parser.add_argument("--prerun-records", type=Path, required=True)
    summarize_parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.command == "validate":
        try:
            record = json.loads(args.record.read_text(encoding="utf-8"))
            schedule = json.loads(args.schedule.read_text(encoding="utf-8"))
            prerun_record, prerun_digest = _load_prerun(args.prerun_records, schedule, hashlib.sha256(args.schedule.read_bytes()).hexdigest())
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print(json.dumps({"kind": "veritas_assistant_recommendation_validation", "status": "invalid", "errors": [str(exc)]}, sort_keys=True))
            return 1
        valid, errors, derived = validate_record(record, schedule, prerun_record, prerun_digest, args.evidence_root)
        print(json.dumps({"kind": "veritas_assistant_recommendation_validation", "status": "valid" if valid else "invalid", "errors": errors, "derived": derived}, sort_keys=True))
        return 0 if valid else 1
    if not args.directory.is_dir():
        print(json.dumps({"kind": "veritas_assistant_recommendation_summary", "status": "invalid", "errors": [f"not a directory: {args.directory}"]}, sort_keys=True))
        return 1
    try:
        schedule = json.loads(args.schedule.read_text(encoding="utf-8"))
        prerun_record, prerun_digest = _load_prerun(args.prerun_records, schedule, hashlib.sha256(args.schedule.read_bytes()).hexdigest())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(json.dumps({"kind": "veritas_assistant_recommendation_summary", "status": "invalid", "errors": [str(exc)]}, sort_keys=True))
        return 1
    records, file_errors = _load_records(args.directory)
    summary = summarize_records(records, schedule, prerun_record, prerun_digest, args.evidence_root)
    if file_errors:
        summary["invalid_decisions"].extend(file_errors)
        summary["direct_recommendation_signal"] = "insufficient_direct_recommendation_signal"
    print(json.dumps(summary, sort_keys=True))
    return 1 if summary["invalid_decisions"] else 0


if __name__ == "__main__":
    sys.exit(main())
