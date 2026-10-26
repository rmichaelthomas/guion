#!/usr/bin/env python3
"""Validate the switch-condition ledger against its preserved sources and citations."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "evaluation/agent-choice/switch-conditions.json"
EXPECTED_COHORTS = {
    "recommendations": "evaluation/results/recommendations",
    "independent-v3-veritas-baseline": "evaluation/results/independent-v3/recommendations",
}


def fail(message: str) -> None:
    raise ValueError(message)


def main() -> int:
    ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
    decisions = ledger.get("source_decisions")
    groups = ledger.get("condition_groups")
    if ledger.get("schema_version") != 1 or not isinstance(decisions, list) or not isinstance(groups, list):
        fail("ledger schema or required arrays are invalid")
    if ledger.get("source_record_count") != len(decisions) or len(decisions) != 36:
        fail("expected 36 preserved recommendation decisions")

    record_keys: dict[str, dict] = {}
    brief_ids: set[str] = set()
    cohort_paths = {name: [] for name in EXPECTED_COHORTS}
    for record in decisions:
        decision_id = record.get("decision_id")
        cohort = record.get("cohort")
        if not isinstance(decision_id, str) or not decision_id:
            fail(f"missing decision id: {decision_id}")
        key = f"{cohort}:{decision_id}"
        if record.get("source_record_key") != key or key in record_keys:
            fail(f"missing or duplicate cohort-qualified source record key: {key}")
        record_keys[key] = record
        source = record.get("source_path")
        expected_directory = EXPECTED_COHORTS.get(cohort)
        if expected_directory is None or not isinstance(source, str) or not source.startswith(expected_directory + "/"):
            fail(f"source path does not match cohort: {decision_id}")
        path = (ROOT / source).resolve()
        if not path.is_file() or path.parent != (ROOT / expected_directory).resolve():
            fail(f"source recommendation is missing or outside its cohort: {source}")
        payload = path.read_bytes()
        if hashlib.sha256(payload).hexdigest() != record.get("source_sha256"):
            fail(f"source recommendation digest changed: {source}")
        raw = json.loads(payload)
        recommendation = raw.get("recommendation", {})
        brief_ids.add(record.get("brief_id"))
        if raw.get("decision_id") != decision_id or raw.get("task", {}).get("id") != record.get("brief_id"):
            fail(f"source identity mismatch: {decision_id}")
        if recommendation.get("choice") != record.get("choice"):
            fail(f"choice mismatch: {decision_id}")
        if recommendation.get("rationale", "") != record.get("rationale_text"):
            fail(f"rationale omitted or changed: {decision_id}")
        if recommendation.get("conditions", []) != record.get("conditions", []):
            fail(f"condition list omitted or changed: {decision_id}")
        cohort_paths[cohort].append(path)

    for entry in ledger.get("source_manifests", []):
        cohort = entry.get("cohort")
        expected = EXPECTED_COHORTS.get(cohort)
        if expected is None or entry.get("directory") != expected:
            fail(f"unexpected source cohort: {cohort}")
        paths = sorted(cohort_paths[cohort])
        if len(paths) != entry.get("record_count") or len(paths) != 18:
            fail(f"wrong source count for {cohort}")

    for group in groups:
        group_briefs = group.get("brief_ids")
        if not isinstance(group_briefs, list) or not group_briefs or len(set(group_briefs)) != len(group_briefs):
            fail(f"invalid brief list for condition group {group.get('id')}")
        if not set(group_briefs) <= brief_ids:
            fail(f"unknown briefs for condition group {group.get('id')}")
        if group.get("source_decision_ids") is not None:
            fail(f"ambiguous unqualified decision ids remain in condition group {group.get('id')}")
        if group_briefs != sorted(group_briefs):
            fail(f"brief list mismatch for condition group {group.get('id')}")
        frequency = group.get("frequency", {})
        mentions = frequency.get("topic_mentions")
        if not isinstance(mentions, int) or mentions < 1 or frequency.get("briefs") != len(group_briefs):
            fail(f"frequency mismatch for condition group {group.get('id')}")
        if group.get("status") not in ledger.get("status_values", []):
            fail(f"invalid status for condition group {group.get('id')}")
        evidence = group.get("evidence", [])
        if not isinstance(evidence, list) or not evidence:
            fail(f"missing evidence for condition group {group.get('id')}")
        for item in evidence:
            relative = item.get("path")
            reference = item.get("reference")
            if not isinstance(relative, str) or not isinstance(reference, str) or not reference:
                fail(f"invalid evidence item for condition group {group.get('id')}")
            path = (ROOT / relative).resolve()
            if not path.exists() or ROOT not in path.parents:
                fail(f"evidence file is missing or outside repository: {relative}")
            if path.is_dir():
                if item.get("kind") not in {"source-records", "briefs"} or not any(path.iterdir()):
                    fail(f"directory evidence is not a populated source set: {relative}")
                continue
            source = path.read_text(encoding="utf-8")
            if item.get("kind") in {"signature", "test"}:
                for symbol in reference.split("; "):
                    if symbol not in source:
                        fail(f"evidence citation does not exist: {relative}: {symbol}")
        if group.get("status") == "closed" and not any(item.get("kind") in {"signature", "test"} for item in evidence):
            fail(f"closed condition lacks a signature or test: {group.get('id')}")

    counts = [group["frequency"]["topic_mentions"] for group in groups]
    if counts != sorted(counts, reverse=True):
        fail("condition groups are not ordered by descending topic mention frequency")

    print(f"switch-condition ledger valid: {len(decisions)} source decisions, {len(groups)} condition groups")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"switch-condition ledger invalid: {error}", file=sys.stderr)
        raise SystemExit(1)
