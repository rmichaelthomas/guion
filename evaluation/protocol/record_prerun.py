#!/usr/bin/env python3
"""Append a timestamped record of the frozen evaluation and campaign inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from datetime import UTC, datetime
from pathlib import Path
import sys
from typing import Any


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def append_record(freeze_path: Path, schedule_path: Path, campaign_lock_path: Path, output_path: Path, primary_measure: str) -> dict[str, Any]:
    freeze = load(freeze_path)
    schedule = load(schedule_path)
    campaign = load(campaign_lock_path)
    if freeze.get("status") != "frozen_for_independent_evaluation":
        raise ValueError("freeze record is not approved")
    if schedule.get("schema_version") != 2 or schedule.get("status") != "assigned-before-runs":
        raise ValueError("schedule is not assigned for formal runs")
    scheduled_campaign = schedule.get("campaign")
    if not isinstance(scheduled_campaign, dict) or freeze.get("campaign") != scheduled_campaign:
        raise ValueError("schedule campaign does not match the freeze")
    campaign_digest = schedule.get("campaign_config_sha256")
    if not isinstance(campaign_digest, str) or not re.fullmatch(r"[0-9a-f]{64}", campaign_digest):
        raise ValueError("schedule campaign_config_sha256 is invalid")
    if freeze.get("campaign_config_sha256") != campaign_digest:
        raise ValueError("schedule campaign does not match the freeze")
    if scheduled_campaign.get("primary_measure") != primary_measure:
        raise ValueError("primary measure does not match the frozen campaign")
    if primary_measure not in {"active_seconds", "human_interventions"}:
        raise ValueError("primary measure must be active_seconds or human_interventions")
    supplied_schedule_digest = schedule.get("schedule_sha256")
    canonical_schedule = {key: value for key, value in schedule.items() if key != "schedule_sha256"}
    canonical_bytes = json.dumps(canonical_schedule, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    if not isinstance(supplied_schedule_digest, str) or hashlib.sha256(canonical_bytes).hexdigest() != supplied_schedule_digest:
        raise ValueError("schedule digest is missing or invalid")
    if schedule.get("manifest_sha256") != freeze.get("manifest_sha256"):
        raise ValueError("schedule manifest digest does not match the freeze")
    if schedule.get("documentation_sha256") != freeze.get("product_bundle_sha256"):
        raise ValueError("schedule product bundle digest does not match the freeze")
    if campaign.get("status") != "selected_before_independent_runs":
        raise ValueError("campaign inputs are not selected for independent runs")
    package = campaign.get("veritas_archive")
    if not isinstance(package, dict) or len(str(package.get("sha256", ""))) != 64:
        raise ValueError("campaign lock is missing the expected Veritas archive digest")

    record = {
        "schema_version": 1,
        "status": "recorded_before_formal_runs",
        "recorded_at_utc": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "freeze_record_sha256": digest(freeze_path),
        "schedule_path": str(schedule_path),
        "schedule_sha256": supplied_schedule_digest,
        "schedule_file_sha256": digest(schedule_path),
        "primary_measure": primary_measure,
        "manifest_sha256": schedule["manifest_sha256"],
        "product_bundle_sha256": schedule["documentation_sha256"],
        "campaign_lock_sha256": digest(campaign_lock_path),
        "campaign": scheduled_campaign,
        "campaign_config_sha256": campaign_digest,
        "veritas_archive_sha256": package["sha256"],
        "python_version": campaign["python_version"],
        "uv_version": campaign["uv_version"],
        "base_image": campaign["base_image"],
        "platform": campaign["platform"],
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists():
        for line in output_path.read_text(encoding="utf-8").splitlines():
            previous = json.loads(line)
            if previous.get("schedule_sha256") == record["schedule_sha256"]:
                raise ValueError("this schedule already has a pre-run record")
    payload = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    descriptor = os.open(output_path, os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
    try:
        os.write(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return record


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--freeze", type=Path, required=True)
    parser.add_argument("--schedule", type=Path, required=True)
    parser.add_argument("--campaign-lock", type=Path, required=True)
    parser.add_argument("--primary-measure", choices=("active_seconds", "human_interventions"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        record = append_record(args.freeze, args.schedule, args.campaign_lock, args.output, args.primary_measure)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        return 1
    print(json.dumps(record, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
