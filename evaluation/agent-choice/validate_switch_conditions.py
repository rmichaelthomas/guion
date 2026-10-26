#!/usr/bin/env python3
"""Validate the switch-condition ledger against its preserved sources and citations."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "evaluation/agent-choice/switch-conditions.json"
EXPECTED_COHORTS = {
    "recommendations": "evaluation/results/recommendations",
    "independent-v3-veritas-baseline": "evaluation/results/independent-v3/recommendations",
}
RATIONALE_OBJECTION_PATTERN = re.compile(
    r"\b(?:I would (?:reconsider|change|revisit|consider)|Veritas could fit if|"
    r"Choose (?:veritas|python|either) if|Reconsider if)\b",
    re.IGNORECASE,
)


def fail(message: str) -> None:
    raise ValueError(message)


def extract_rationale_objections(text: str) -> list[dict[str, int | str]]:
    """Return explicit reconsideration passages with stable character offsets."""
    objections = []
    for index, match in enumerate(RATIONALE_OBJECTION_PATTERN.finditer(text)):
        start = match.start()
        sentence_end = re.search(r"[.!?](?=\s+[A-Z*])", text[start:])
        end = start + sentence_end.end() if sentence_end else len(text)
        objections.append(
            {
                "index": index,
                "start_char": start,
                "end_char": end,
                "text": text[start:end].strip(),
            }
        )
    return objections


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
    expected_condition_refs: set[tuple[str, int]] = set()
    expected_rationale_refs: set[tuple[str, int]] = set()
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
        extracted_objections = extract_rationale_objections(record["rationale_text"])
        if extracted_objections != record.get("rationale_objections"):
            fail(f"rationale objection excerpts or offsets differ from source: {decision_id}")
        if recommendation.get("conditions", []) != record.get("conditions", []):
            fail(f"condition list omitted or changed: {decision_id}")
        expected_condition_refs.update((key, index) for index in range(len(record.get("conditions", []))))
        expected_rationale_refs.update((key, index) for index in range(len(extracted_objections)))
        cohort_paths[cohort].append(path)

    for entry in ledger.get("source_manifests", []):
        cohort = entry.get("cohort")
        expected = EXPECTED_COHORTS.get(cohort)
        if expected is None or entry.get("directory") != expected:
            fail(f"unexpected source cohort: {cohort}")
        paths = sorted(cohort_paths[cohort])
        if len(paths) != entry.get("record_count") or len(paths) != 18:
            fail(f"wrong source count for {cohort}")

    seen_condition_refs: list[tuple[str, int]] = []
    seen_rationale_refs: set[tuple[str, int]] = set()
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
        condition_refs = group.get("source_condition_refs")
        if not isinstance(condition_refs, list):
            fail(f"missing condition references for condition group {group.get('id')}")
        for reference in condition_refs:
            key = reference.get("source_record_key")
            index = reference.get("condition_index")
            if key not in record_keys or not isinstance(index, int) or isinstance(index, bool):
                fail(f"invalid source condition reference for group {group.get('id')}")
            if index < 0 or index >= len(record_keys[key].get("conditions", [])):
                fail(f"condition index is out of range for group {group.get('id')}: {key}#{index}")
            if record_keys[key]["brief_id"] not in group_briefs:
                fail(f"condition reference's brief is absent from group {group.get('id')}: {key}#{index}")
            if reference.get("status") != group.get("status"):
                fail(f"condition reference status differs from group {group.get('id')}: {key}#{index}")
            seen_condition_refs.append((key, index))
        condition_entries = frequency.get("condition_entries")
        rationale_refs = group.get("source_rationale_refs")
        if not isinstance(rationale_refs, list):
            fail(f"missing rationale-objection references for condition group {group.get('id')}")
        group_rationale_refs: set[tuple[str, int]] = set()
        for reference in rationale_refs:
            key = reference.get("source_record_key")
            index = reference.get("rationale_objection_index")
            if key not in record_keys or not isinstance(index, int) or isinstance(index, bool):
                fail(f"invalid rationale-objection reference for group {group.get('id')}")
            if index < 0 or index >= len(record_keys[key].get("rationale_objections", [])):
                fail(f"rationale-objection index is out of range for group {group.get('id')}: {key}#{index}")
            if record_keys[key]["brief_id"] not in group_briefs:
                fail(f"rationale-objection reference's brief is absent from group {group.get('id')}: {key}#{index}")
            if reference.get("status") != group.get("status"):
                fail(f"rationale-objection reference status differs from group {group.get('id')}: {key}#{index}")
            rationale_ref = (key, index)
            if rationale_ref in group_rationale_refs:
                fail(f"duplicate rationale-objection reference in group {group.get('id')}: {key}#{index}")
            group_rationale_refs.add(rationale_ref)
            seen_rationale_refs.add(rationale_ref)
        if (
            not isinstance(mentions, int)
            or mentions < 1
            or frequency.get("briefs") != len(group_briefs)
            or condition_entries != len(condition_refs)
            or frequency.get("rationale_objections") != len(group_rationale_refs)
        ):
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

    if len(seen_condition_refs) != len(set(seen_condition_refs)):
        fail("a source condition is mapped to more than one condition group")
    if set(seen_condition_refs) != expected_condition_refs:
        missing = sorted(expected_condition_refs - set(seen_condition_refs))
        extra = sorted(set(seen_condition_refs) - expected_condition_refs)
        fail(f"condition group coverage differs from source conditions; missing={missing}; extra={extra}")

    if not expected_rationale_refs <= seen_rationale_refs:
        missing = sorted(expected_rationale_refs - seen_rationale_refs)
        fail(f"rationale-objection group coverage is incomplete; missing={missing}")

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
