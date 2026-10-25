"""Render a choice prompt with verified product profiles in frozen random order."""

from __future__ import annotations

import hashlib
import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any


LANGUAGES = {"veritas", "python"}
HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")


def render_choice_prompt(
    choice: dict[str, Any],
    task: dict[str, Any],
    bundle: dict[str, Any],
    product_root: Path,
    campaign: dict[str, Any],
    schedule_sha256: str,
    schedule_file_sha256: str,
    prerun_records_sha256: str,
    prerun_record: dict[str, Any],
) -> str:
    order = choice.get("presentation_order")
    if not isinstance(order, list) or len(order) != 2 or set(order) != LANGUAGES:
        raise ValueError("choice presentation_order must include each language exactly once")
    if any(not isinstance(value, str) or not HEX_SHA256.fullmatch(value) for value in (schedule_sha256, schedule_file_sha256, prerun_records_sha256)):
        raise ValueError("schedule and pre-run digests must be lowercase SHA-256 values")
    model = campaign.get("assistant_model")
    budget = campaign.get("choice_budget")
    if not isinstance(model, dict) or not isinstance(budget, dict):
        raise ValueError("campaign must declare an assistant model and choice budget")
    if not isinstance(task.get("brief_text"), str) or not task["brief_text"].strip():
        raise ValueError("task brief_text must be non-empty")

    artifacts = bundle.get("artifacts")
    if not isinstance(artifacts, list):
        raise ValueError("product bundle artifacts must be a list")
    canonical = json.dumps(artifacts, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    bundle_sha256 = hashlib.sha256(canonical).hexdigest()
    if bundle.get("bundle_sha256") != bundle_sha256 or choice.get("documentation_sha256") != bundle_sha256:
        raise ValueError("product bundle digest does not match the scheduled choice")

    profiles: dict[str, list[tuple[str, str]]] = {language: [] for language in LANGUAGES}
    root = product_root.resolve()
    for artifact in artifacts:
        language = artifact.get("language")
        relative = Path(artifact.get("path", ""))
        if language not in LANGUAGES or relative.is_absolute() or ".." in relative.parts:
            raise ValueError("each product artifact needs a safe path and language")
        path = (root / relative).resolve()
        try:
            path.relative_to(root)
        except ValueError as error:
            raise ValueError("product artifact path escapes the bundle root") from error
        payload = path.read_bytes()
        digest = hashlib.sha256(payload).hexdigest()
        if digest != artifact.get("sha256"):
            raise ValueError(f"product artifact digest mismatch: {relative.as_posix()}")
        profiles[language].append((relative.as_posix(), payload.decode("utf-8")))
    if not profiles["veritas"] or not profiles["python"]:
        raise ValueError("the choice bundle must contain both Veritas and Python profiles")

    lines = [
        "You are advising a developer choosing a language for the attached task.",
        "Do not implement the task or infer results from code that has not been built.",
        "Use only the task brief and the two supplied product profiles. Do not browse or use tools.",
        "Choose exactly one: veritas, python, or either. Explain decisive tradeoffs and conditions that would change your recommendation.",
        "",
        f"Assignment ID: {choice['decision_id']}",
        f"Schedule SHA-256: {schedule_sha256}",
        f"Schedule file SHA-256: {schedule_file_sha256}",
        f"Pre-run records SHA-256: {prerun_records_sha256}",
        f"Pre-run recorded at UTC: {prerun_record['recorded_at_utc']}",
        f"Choice-safe product bundle SHA-256: {bundle_sha256}",
        f"Presentation order: {', '.join(order)}",
        f"Configured assistant: {model.get('provider')} {model.get('name')} ({model.get('version')}); reasoning effort {model.get('reasoning_effort')}",
        f"Budget: {budget.get('active_minutes')} active minutes and {budget.get('tool_calls')} tool calls.",
        "No implementation results, benchmarks, author calibration, or candidate runs have been shown.",
        "The recommendation concerns this task only; do not make a universal language-replacement claim.",
        "",
        "# Task brief",
        task["brief_text"].rstrip(),
        "",
    ]
    for rank, language in enumerate(order, start=1):
        display_name = "Veritas" if language == "veritas" else "Python"
        lines.append(f"# Option {rank}: {display_name}")
        for path, content in sorted(profiles[language]):
            lines.extend((f"## {path}", content.rstrip(), ""))
    return "\n".join(lines).rstrip() + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("decision_id")
    parser.add_argument("--schedule", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--bundle-manifest", type=Path, required=True)
    parser.add_argument("--bundle-root", type=Path, required=True)
    parser.add_argument("--prerun-records", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        schedule = json.loads(args.schedule.read_text(encoding="utf-8"))
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        bundle = json.loads(args.bundle_manifest.read_text(encoding="utf-8"))
        schedule_payload = dict(schedule)
        supplied_schedule_sha = schedule_payload.pop("schedule_sha256", None)
        actual_schedule_sha = hashlib.sha256(json.dumps(schedule_payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()
        if schedule.get("schema_version") != 2 or schedule.get("status") != "assigned-before-runs" or supplied_schedule_sha != actual_schedule_sha:
            raise ValueError("schedule status or digest is invalid")
        manifest_sha = hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        if manifest_sha != schedule.get("manifest_sha256"):
            raise ValueError("task manifest does not match the frozen schedule")
        choices = [item for item in schedule.get("choices", []) if item.get("decision_id") == args.decision_id]
        if len(choices) != 1:
            raise ValueError("decision ID must identify exactly one schedule entry")
        choice = choices[0]
        tasks = [task for task in manifest.get("tasks", []) if task.get("id") == choice.get("task_id")]
        if len(tasks) != 1:
            raise ValueError("scheduled task is missing or duplicated in the task manifest")
        task = dict(tasks[0])
        brief_path = Path(task["brief"])
        task["brief_text"] = brief_path.read_text(encoding="utf-8")
        if hashlib.sha256(task["brief_text"].encode("utf-8")).hexdigest() != choice.get("brief_sha256"):
            raise ValueError("task brief does not match the scheduled digest")
        raw_prerun = args.prerun_records.read_bytes()
        preruns = [json.loads(line) for line in raw_prerun.decode("utf-8").splitlines() if line.strip()]
        matching = [item for item in preruns if item.get("schedule_sha256") == supplied_schedule_sha]
        if len(matching) != 1:
            raise ValueError("pre-run file must contain exactly one record for this schedule")
        prompt = render_choice_prompt(
            choice,
            task,
            bundle,
            args.bundle_root,
            schedule["campaign"],
            supplied_schedule_sha,
            hashlib.sha256(args.schedule.read_bytes()).hexdigest(),
            hashlib.sha256(raw_prerun).hexdigest(),
            matching[0],
        )
        if args.output.exists():
            raise FileExistsError(f"refusing to overwrite choice prompt: {args.output}")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(prompt, encoding="utf-8")
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        return 1
    print(json.dumps({"decision_id": args.decision_id, "presentation_order": choice["presentation_order"], "prompt_sha256": hashlib.sha256(prompt.encode("utf-8")).hexdigest(), "path": str(args.output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
