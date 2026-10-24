"""Generate a pre-run randomized assignment for the independent campaign."""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import secrets
import re


HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")


def validate_campaign(campaign: dict) -> None:
    if campaign.get("schema_version") != 1:
        raise ValueError("campaign schema_version must equal 1")
    model = campaign.get("assistant_model")
    if not isinstance(model, dict):
        raise ValueError("campaign assistant_model must be an object")
    for key in ("provider", "name", "version", "reasoning_effort"):
        if not isinstance(model.get(key), str) or not model[key].strip():
            raise ValueError(f"campaign assistant_model.{key} must be a non-empty string")
    for key in ("choice_budget", "build_budget"):
        budget = campaign.get(key)
        if not isinstance(budget, dict):
            raise ValueError(f"campaign {key} must be an object")
        minutes = budget.get("active_minutes")
        calls = budget.get("tool_calls")
        if isinstance(minutes, bool) or not isinstance(minutes, int) or minutes <= 0:
            raise ValueError(f"campaign {key}.active_minutes must be a positive integer")
        if isinstance(calls, bool) or not isinstance(calls, int) or calls < 0:
            raise ValueError(f"campaign {key}.tool_calls must be a non-negative integer")
    if campaign.get("primary_measure") not in {"active_seconds", "human_interventions"}:
        raise ValueError("campaign primary_measure must be active_seconds or human_interventions")


def create_schedule(
    manifest: dict,
    documentation_sha256: str,
    campaign: dict,
    campaign_config_sha256: str,
    created_at: str | None = None,
) -> dict:
    validate_campaign(campaign)
    if not isinstance(campaign_config_sha256, str) or not HEX_SHA256.fullmatch(campaign_config_sha256):
        raise ValueError("campaign_config_sha256 must be a lowercase SHA-256 digest")
    tasks = manifest.get("tasks")
    if not isinstance(tasks, list) or len(tasks) != 9:
        raise ValueError("expected exactly nine frozen task briefs")
    pairs = []
    choices = []
    for task in tasks:
        task_id = task["id"]
        task_class = task["class"]
        brief_digest = hashlib.sha256(Path(task["brief"]).read_bytes()).hexdigest()
        for pair_number in (1, 2):
            order = ["veritas", "python"]
            if secrets.choice((False, True)):
                order.reverse()
            pair_id = f"{task_id}-pair-{pair_number}"
            pairs.append({"pair_id": pair_id, "task_id": task_id, "task_class": task_class, "brief_sha256": brief_digest, "pair_order": order, "sessions": [{"language": language, "session_slot": f"{pair_id}-{language}"} for language in order]})
        for choice_number in (1, 2):
            order = ["veritas", "python"]
            if secrets.choice((False, True)):
                order.reverse()
            choices.append({"decision_id": f"{task_id}-choice-{choice_number}", "task_id": task_id, "task_class": task_class, "brief_sha256": brief_digest, "presentation_order": order, "documentation_sha256": documentation_sha256})
    result = {
        "schema_version": 2,
        "status": "assigned-before-runs",
        "created_at": created_at or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "documentation_sha256": documentation_sha256,
        "manifest_sha256": hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
        "campaign": campaign,
        "campaign_config_sha256": campaign_config_sha256,
        "pairs": pairs,
        "choices": choices,
    }
    canonical = json.dumps(result, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    result["schedule_sha256"] = hashlib.sha256(canonical).hexdigest()
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("product_bundle_manifest", type=Path)
    parser.add_argument("campaign_config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    bundle = json.loads(args.product_bundle_manifest.read_text(encoding="utf-8"))
    schedule = create_schedule(
        json.loads(args.manifest.read_text(encoding="utf-8")),
        bundle["bundle_sha256"],
        json.loads(args.campaign_config.read_text(encoding="utf-8")),
        hashlib.sha256(args.campaign_config.read_bytes()).hexdigest(),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(schedule, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"schedule_sha256": schedule["schedule_sha256"], "pairs": len(schedule["pairs"]), "choices": len(schedule["choices"]), "path": str(args.output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
