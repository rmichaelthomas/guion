#!/usr/bin/env python3
"""Prepare immutable, choice-safe Guion v4 review materials without starting sessions."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "evaluation/results/independent-v4"
sys.path.insert(0, str(ROOT))
from evaluation.protocol.schedule import create_schedule

SOURCE_MANIFEST = ROOT / "evaluation/fixtures/independent-v1/manifest.json"
CELL_BUDGET = {"active_minutes": 5, "tool_calls": 0}
BUILD_BUDGET = {"active_minutes": 45, "tool_calls": 60}
EMPTY_BUNDLE_SHA256 = hashlib.sha256(b"[]").hexdigest()
MODELS = {
    "codex": {
        "provider": "OpenAI Codex",
        "name": "gpt-6.1-sol",
        "version": "gpt-6.1-sol",
        "reasoning_effort": "medium",
    },
    "claude": {
        "provider": "Anthropic Claude",
        "name": "claude-sonnet-5",
        "version": "claude-sonnet-5",
        "reasoning_effort": "provider-default",
    },
    "gemini": {
        "provider": "Google Gemini",
        "name": "gemini-4-argon",
        "version": "gemini-4-argon",
        "reasoning_effort": "provider-default",
    },
}
SOURCE_FILES = {
    "compatibility-and-migrations.md": "docs/compatibility-and-migrations.md",
    "setup-guide.md": "docs/getting-started.md",
    "language-spec.md": "language/spec.md",
    "builtin-signatures.json": "language/builtin-signatures.json",
    "host-api.md": "language/host-api.md",
    "guion-reference.md": "docs/agents/guion-reference.md",
}
PROFILE_README = """# Guion product profile

Guion is a private, pre-release general-purpose programming language. Programs use `.gu` source and compile to `.gub` artifacts for a Rust runtime. The core command does not require or launch Python. The current compiler and runtime are version 0.1.0 / artifact format 39; pre-1.0 source and package compatibility is not guaranteed.

The language includes typed functions and records, modules, immutable lists and maps, JSON, `Option` and `Result`, file and byte operations, CSV and timestamp parsing, JSONL streaming, SemVer parsing, Unicode operations, iCalendar parsing, TOML editing, SQLite transactions, HMAC-SHA256, byte-preserving HTTP, and bounded concurrent work. Capabilities such as file roots, exact HTTP hostnames, environment variables, and clock reads are granted by the caller. The language reference and host API in this bundle describe limits and unsupported surfaces.

Guion can create source-free packages on the targets documented in the compatibility sheet. Rust is needed to build the CLI from source; the core installed command does not need Python. Python interoperability is optional and requires a build with the bridge enabled.

The current evidence does not establish general performance superiority over Python. This choice packet contains no benchmark results or comparative implementation outcomes.
"""
PYTHON_PROFILE = """# Python product profile

This campaign profile uses CPython 3.14.8 and the `python3` command. Python is a mature, general-purpose language with a broad standard library and package ecosystem. A task can use the standard library; any third-party package must be named in the task materials and available to the same evaluation environment. This is a language-choice profile, not a performance claim or a restriction on Python deployments outside this campaign.
"""


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def write_json(path: Path, value: object) -> bytes:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    path.write_bytes(payload)
    return payload


def copy_product_bundle() -> dict:
    bundle_root = OUT / "product-information"
    bundle_root.mkdir(parents=True, exist_ok=True)
    (bundle_root / "README.md").write_text(PROFILE_README, encoding="utf-8")
    (bundle_root / "guion-profile.md").write_text(
        "# Guion capability profile\n\n"
        "The reference in this bundle includes the current built-in and standard-library signatures, syntax, stable diagnostics, and five runnable examples. The host API describes permission grants, resource limits, deadlines, redirects, and cancellation behavior.\n\n"
        "Closed capabilities carried forward from the switch-condition audit include JSONL with recoverable typed JSON results; CSV and strict date/timestamp parsing; iCalendar and timezone parsing; SQLite transactions; comment-preserving TOML edits; bounded HTTP and concurrency; byte-preserving HTTP; HMAC-SHA256 verification; Unicode normalization and case-folding; SemVer parsing; and runtime effect/integrity reporting. These are product capabilities, not comparative results.\n\n"
        "Known limits include narrow pre-release platform support, task-specific package and ecosystem evidence, no recoverable conversion for managed Python bridge failures, no source-declared network-host requirements, and no crash-atomic transaction across multiple output files.\n",
        encoding="utf-8",
    )
    (bundle_root / "python-profile.md").write_text(PYTHON_PROFILE, encoding="utf-8")
    for name, source in SOURCE_FILES.items():
        source_text = (ROOT / source).read_text(encoding="utf-8")
        (bundle_root / name).write_text(source_text.rstrip() + "\n", encoding="utf-8")
    artifacts = []
    for path in sorted(item for item in bundle_root.iterdir() if item.is_file() and item.name != "bundle-manifest.json"):
        language = "python" if path.name == "python-profile.md" else "veritas"
        artifacts.append({"path": path.name, "sha256": digest(path.read_bytes()), "language": language})
    manifest = {
        "schema_version": 1,
        "bundle_id": "guion-vs-python-choice-safe-v4",
        "hash_algorithm": "sha256",
        "digest_definition": "SHA-256 of compact, sorted-key UTF-8 JSON for the artifacts array.",
        "artifacts": artifacts,
        "bundle_sha256": digest(canonical(artifacts)),
        "contains_benchmarks_or_comparative_outcomes": False,
        "machine_label_mapping": {"display": "Guion", "legacy_protocol_value": "veritas"},
    }
    write_json(bundle_root / "bundle-manifest.json", manifest)
    cold_manifest = {
        "schema_version": 1,
        "bundle_id": "guion-vs-python-cold-no-documents-v4",
        "hash_algorithm": "sha256",
        "digest_definition": "SHA-256 of compact, sorted-key UTF-8 JSON for the artifacts array.",
        "artifacts": [],
        "bundle_sha256": EMPTY_BUNDLE_SHA256,
        "contains_benchmarks_or_comparative_outcomes": False,
    }
    write_json(OUT / "cold-bundle-manifest.json", cold_manifest)
    return manifest


def create_manifest() -> dict:
    original = json.loads(SOURCE_MANIFEST.read_text(encoding="utf-8"))
    manifest = copy.deepcopy(original)
    manifest["kit_id"] = "guion-independent-workloads-v1"
    manifest["status"] = "prepared-for-independent-review"
    manifest["source_manifest"] = "evaluation/fixtures/independent-v1/manifest.json"
    manifest["source_manifest_sha256"] = digest(SOURCE_MANIFEST.read_bytes())
    for task in manifest["tasks"]:
        source = ROOT / task["brief"]
        original_text = source.read_text(encoding="utf-8")
        updated_text = re.sub(r"\bveritas\b", "Guion", original_text, flags=re.IGNORECASE)
        updated_text = updated_text.replace(
            "the existing Guion inventory-reconciliation example",
            "the existing inventory-reconciliation example",
        )
        updated_text = updated_text.replace(
            "Guion may use its managed Python bridge",
            "The non-Python candidate may use its managed Python bridge",
        )
        updated_text = updated_text.replace(
            "Guion may implement the feature directly",
            "The non-Python candidate may implement the feature directly",
        )
        updated_text = updated_text.replace(
            "Guion may use its normal dependency/interop path",
            "The non-Python candidate may use its normal dependency/interop path",
        )
        updated_text = "\n".join(line.rstrip() for line in updated_text.splitlines()) + "\n"
        normalized = OUT / "briefs" / source.name
        normalized.parent.mkdir(parents=True, exist_ok=True)
        normalized.write_text(updated_text, encoding="utf-8")
        task["source_brief"] = task["brief"]
        task["source_brief_sha256"] = digest(source.read_bytes())
        task["brief"] = str(normalized.relative_to(ROOT))
        task["brief_sha256"] = digest(updated_text.encode("utf-8"))
    return manifest


def model_config(model: dict, arm: str) -> dict:
    return {
        "schema_version": 1,
        "assistant_model": model,
        "choice_budget": CELL_BUDGET,
        "build_budget": BUILD_BUDGET,
        "primary_measure": "active_seconds",
        "arm": arm,
        "budget_accounting": {
            "active_seconds": "Sum terminal-turn durationMs from the saved provider session payload and round up to whole seconds.",
            "tool_calls": "Record tool use from the provider session payload; the choice allowance is zero.",
        },
    }


def load_profiles(bundle: dict) -> dict[str, list[tuple[str, str]]]:
    profiles: dict[str, list[tuple[str, str]]] = {"veritas": [], "python": []}
    base = OUT / "product-information"
    for artifact in bundle["artifacts"]:
        path = base / artifact["path"]
        profiles[artifact["language"]].append((artifact["path"], path.read_text(encoding="utf-8")))
    return profiles


def render_prompt(choice: dict, task: dict, campaign: dict, schedule: dict, schedule_file_sha: str, arm: str, profiles: dict) -> str:
    order = choice["presentation_order"]
    model = campaign["assistant_model"]
    lines = [
        "You are advising a developer choosing a language for the attached task.",
        "Do not implement the task, browse, use tools, or infer results from code that has not been built.",
        "Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.",
        "In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.",
        "Do not make a universal language-replacement claim.",
        "",
        f"Assignment ID: {choice['decision_id']}",
        f"Schedule SHA-256: {schedule['schedule_sha256']}",
        f"Schedule file SHA-256: {schedule_file_sha}",
        f"Choice-safe bundle SHA-256: {choice['documentation_sha256']}",
        f"Presentation order: {', '.join('Guion' if name == 'veritas' else 'Python' for name in order)}",
        f"Configured assistant: {model['provider']} {model['name']} ({model['version']}); reasoning effort {model['reasoning_effort']}",
        f"Budget: {CELL_BUDGET['active_minutes']} active minutes and {CELL_BUDGET['tool_calls']} tool calls.",
        "No implementation results, benchmarks, author calibration, or candidate runs have been shown.",
        "",
        "# Task brief",
        task["brief_text"].rstrip(),
        "",
    ]
    for rank, legacy_name in enumerate(order, start=1):
        display_name = "Guion" if legacy_name == "veritas" else "Python"
        lines.append(f"# Option {rank}: {display_name}")
        if arm == "cold":
            lines.append("No product profile or documentation is supplied for this option.\n")
        else:
            for path, content in sorted(profiles[legacy_name]):
                lines.extend((f"## {path}", content.rstrip(), ""))
    return "\n".join(lines).rstrip() + "\n"


def make_schedule(base: dict, config: dict, config_sha: str, bundle_sha: str) -> dict:
    schedule = copy.deepcopy(base)
    schedule["campaign"] = config
    schedule["campaign_config_sha256"] = config_sha
    schedule["documentation_sha256"] = bundle_sha
    for choice in schedule["choices"]:
        choice["documentation_sha256"] = bundle_sha
    schedule.pop("schedule_sha256", None)
    schedule["schedule_sha256"] = digest(canonical(schedule))
    return schedule


def prepare_cell_materials(manifest: dict, bundle: dict) -> list[dict]:
    config_root = OUT / "campaign-configs"
    schedule_root = OUT / "schedules"
    prompt_root = OUT / "prompts"
    for directory in (config_root, schedule_root, prompt_root):
        directory.mkdir(parents=True, exist_ok=True)
    base_config = model_config(MODELS["codex"], "product-information")
    base_config_bytes = canonical(base_config)
    common = create_schedule(
        manifest,
        bundle["bundle_sha256"],
        base_config,
        digest(base_config_bytes),
    )
    profiles = load_profiles(bundle)
    cells = []
    for provider_key, model in MODELS.items():
        for arm in ("product-information", "cold"):
            cell = f"{provider_key}-{arm}"
            config = model_config(model, arm)
            config_path = config_root / f"{cell}.json"
            config_bytes = write_json(config_path, config)
            bundle_sha = bundle["bundle_sha256"] if arm == "product-information" else EMPTY_BUNDLE_SHA256
            schedule = make_schedule(common, config, digest(config_bytes), bundle_sha)
            schedule_path = schedule_root / f"{cell}.json"
            schedule_bytes = write_json(schedule_path, schedule)
            choices = {item["decision_id"]: item for item in schedule["choices"]}
            tasks = {item["id"]: item for item in manifest["tasks"]}
            for choice in schedule["choices"]:
                task = copy.deepcopy(tasks[choice["task_id"]])
                task["brief_text"] = (ROOT / task["brief"]).read_text(encoding="utf-8")
                prompt = render_prompt(choice, task, config, schedule, digest(schedule_bytes), arm, profiles)
                output = prompt_root / cell / f"{choice['decision_id']}.md"
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text(prompt, encoding="utf-8")
            if set(choices) != {item["decision_id"] for item in common["choices"]}:
                raise ValueError(f"choice IDs differ across {cell}")
            cells.append({
                "cell_id": cell,
                "provider": model["provider"],
                "model": model["name"],
                "arm": arm,
                "campaign_config": str(config_path.relative_to(ROOT)),
                "campaign_config_sha256": digest(config_bytes),
                "schedule": str(schedule_path.relative_to(ROOT)),
                "schedule_file_sha256": digest(schedule_bytes),
                "schedule_sha256": schedule["schedule_sha256"],
                "choice_count": len(schedule["choices"]),
                "prompt_directory": str((prompt_root / cell).relative_to(ROOT)),
                "prompt_count": len(list((prompt_root / cell).glob("*.md"))),
            })
    return cells


def validate_assets(manifest: dict, bundle: dict, cells: list[dict]) -> None:
    if len(manifest["tasks"]) != 9 or len(cells) != 6:
        raise ValueError("expected nine briefs and six provider/context cells")
    expected_ids = None
    expected_orders = None
    expected_pairs = None
    for cell in cells:
        path = ROOT / cell["schedule"]
        schedule = json.loads(path.read_text(encoding="utf-8"))
        payload = dict(schedule)
        supplied = payload.pop("schedule_sha256")
        if digest(canonical(payload)) != supplied:
            raise ValueError(f"invalid schedule digest: {cell['cell_id']}")
        manifest_digest = hashlib.sha256(
            json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        if schedule.get("manifest_sha256") != manifest_digest:
            raise ValueError(f"task manifest digest differs: {cell['cell_id']}")
        ids = {entry["decision_id"] for entry in schedule["choices"]}
        if len(ids) != 18 or (expected_ids is not None and ids != expected_ids):
            raise ValueError(f"decision assignments differ: {cell['cell_id']}")
        expected_ids = ids
        orders = {entry["decision_id"]: entry["presentation_order"] for entry in schedule["choices"]}
        pairs = {entry["pair_id"]: entry["pair_order"] for entry in schedule["pairs"]}
        if expected_orders is not None and orders != expected_orders:
            raise ValueError(f"randomized choice orders differ: {cell['cell_id']}")
        if expected_pairs is not None and pairs != expected_pairs:
            raise ValueError(f"pair assignments differ: {cell['cell_id']}")
        expected_orders = orders
        expected_pairs = pairs
        for prompt_path in sorted((ROOT / cell["prompt_directory"]).glob("*.md")):
            prompt = prompt_path.read_text(encoding="utf-8")
            if "Option 1: Veritas" in prompt or "Option 2: Veritas" in prompt:
                raise ValueError(f"legacy product name leaked into prompt: {prompt_path}")
            task_section = prompt.split("# Task brief\n", 1)[1].split("\n# Option ", 1)[0]
            if "Guion" in task_section or "veritas" in task_section.lower():
                raise ValueError(f"candidate identity leaked into task brief: {prompt_path}")
            if cell["arm"] == "cold" and any(name in prompt for name in ("guion-reference.md", "builtin-signatures.json", "host-api.md")):
                raise ValueError(f"product documentation leaked into cold prompt: {prompt_path}")
    if bundle["bundle_sha256"] != digest(canonical(bundle["artifacts"])):
        raise ValueError("product bundle digest is invalid")


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    bundle = copy_product_bundle()
    manifest = create_manifest()
    manifest_path = OUT / "task-manifest.json"
    manifest_bytes = write_json(manifest_path, manifest)
    cells = prepare_cell_materials(manifest, bundle)
    validate_assets(manifest, bundle, cells)

    prompt_records = []
    for cell in cells:
        for path in sorted((ROOT / cell["prompt_directory"]).glob("*.md")):
            prompt_records.append({"path": str(path.relative_to(ROOT)), "sha256": digest(path.read_bytes())})
    prompt_inventory = {"status": "prepared-for-independent-review", "prompt_count": len(prompt_records), "prompts": prompt_records}
    prompt_inventory_path = OUT / "prompt-inventory.json"
    prompt_inventory_bytes = write_json(prompt_inventory_path, prompt_inventory)

    reasons = {
        "codex": "The Codex host can create fresh conversations, but the existing protocol requires a second evaluator to approve the final materials before any session. No approval record exists.",
        "claude": "Anthropic Claude cannot be launched from this Codex session; no substitute provider is used.",
        "gemini": "Google Gemini cannot be launched from this Codex session; no substitute provider is used.",
    }
    session_status = {
        "status": "not-started-awaiting-independent-review",
        "cells": [
            {
                "cell_id": cell["cell_id"],
                "decision_id": decision,
                "status": "not_started",
                "reason": reasons[cell["cell_id"].split("-", 1)[0]],
            }
            for cell in cells
            for decision in sorted(
                entry["decision_id"]
                for entry in json.loads((ROOT / cell["schedule"]).read_text(encoding="utf-8"))["choices"]
            )
        ],
    }
    status_bytes = write_json(OUT / "session-status.json", session_status)
    config_bytes = (OUT / "campaign-config.json").read_bytes()
    amendment_bytes = (OUT / "protocol-corrections.md").read_bytes()
    freeze = {
        "schema_version": 1,
        "status": "assets-frozen-for-independent-review",
        "campaign_id": "independent-v4-guion-choice",
        "frozen_at_utc": None,
        "source_v1_manifest_sha256": digest(SOURCE_MANIFEST.read_bytes()),
        "normalized_manifest_sha256": digest(manifest_bytes),
        "product_bundle_sha256": bundle["bundle_sha256"],
        "cold_documentation_sha256": EMPTY_BUNDLE_SHA256,
        "master_campaign_config_sha256": digest(config_bytes),
        "protocol_amendment_sha256": digest(amendment_bytes),
        "prompt_inventory_sha256": digest(prompt_inventory_bytes),
        "session_status_sha256": digest(status_bytes),
        "cells": cells,
        "review_status": "pending-second-evaluator-approval",
        "formal_prerun_records_created": False,
        "scored_sessions_started": False,
    }
    freeze_path = OUT / "freeze-record.json"
    freeze_bytes = write_json(freeze_path, freeze)
    freeze["freeze_record_sha256"] = digest(freeze_bytes)
    write_json(freeze_path, freeze)
    print(json.dumps({"status": freeze["status"], "cells": len(cells), "choices_per_cell": 18, "prompts": len(prompt_records), "freeze_sha256": digest(freeze_path.read_bytes()), "review_required_before_runs": True}, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, KeyError, ValueError, json.JSONDecodeError) as error:
        print(f"prepare review materials: {error}", file=sys.stderr)
        raise SystemExit(1)
