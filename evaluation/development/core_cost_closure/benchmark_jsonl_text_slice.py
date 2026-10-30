#!/usr/bin/env python3
"""Compare the frozen JSONL app with an otherwise identical text_slice variant."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402

APP_SOURCE = ROOT / "evaluation/completion/applications/jsonl-summary/main.vr"
VARIANT_SOURCE = ROOT / "evaluation/development/core_cost_closure/fixtures/jsonl-summary-text-slice.vr"
GENERATOR = runner.GENERATOR
REFERENCE = runner.DATA_REFERENCE
OLD_DATE_EXTRACTION = (
    "let characters = text_chars(at)\n"
    "                        let date = characters[0] + characters[1] + characters[2] + characters[3] + characters[4] + characters[5] + characters[6] + characters[7] + characters[8] + characters[9]"
)
NEW_DATE_EXTRACTION = "let date = text_slice(at, 0, 10)"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def application_sources() -> tuple[str, str]:
    """Return frozen source and the separately labeled, single-change variant."""
    original = APP_SOURCE.read_text(encoding="utf-8")
    variant = VARIANT_SOURCE.read_text(encoding="utf-8")
    if original.count(OLD_DATE_EXTRACTION) != 1:
        raise ValueError("frozen JSONL application no longer has the expected date extraction")
    expected_variant = original.replace(OLD_DATE_EXTRACTION, NEW_DATE_EXTRACTION, 1)
    if variant != expected_variant:
        raise ValueError("text_slice fixture must differ only at the date extraction")
    return original, variant


def paired_ratio(samples: dict[str, dict[str, Any]], numerator: str, denominator: str) -> dict[str, Any]:
    ratios = [
        left / right
        for left, right in zip(
            samples[numerator]["samples_seconds"],
            samples[denominator]["samples_seconds"],
            strict=True,
        )
    ]
    return runner.paired_median_ci(ratios)


def measure_rows(
    binary: Path,
    python_binary: Path,
    scratch: Path,
    rows: int,
    trials: int,
) -> dict[str, Any]:
    original_text, variant_text = application_sources()
    project = scratch / f"jsonl-text-slice-{rows}"
    if project.exists():
        shutil.rmtree(project)
    project.mkdir(parents=True)
    runner.write_manifest(project, f"jsonl-text-slice-{rows}", 1_000_000_000)

    original = project / "main.gu"
    variant = project / "main-text-slice.gu"
    original.write_text(original_text, encoding="utf-8")
    variant.write_text(variant_text, encoding="utf-8")
    events = project / "events.jsonl"
    services = project / "services.txt"
    output = project / "summary.json"
    subprocess.run(
        [
            str(python_binary),
            str(GENERATOR),
            "--rows",
            str(rows),
            "--output",
            str(events),
            "--services",
            str(services),
        ],
        cwd=project,
        check=True,
        capture_output=True,
        text=True,
    )
    expected_path = project / "expected.json"
    subprocess.run(
        [str(python_binary), str(REFERENCE), "python", str(events), str(services), str(expected_path)],
        cwd=project,
        check=True,
        capture_output=True,
        text=True,
    )
    expected = runner.normalize_data(json.loads(expected_path.read_text(encoding="utf-8")))

    original_artifact = project / "main.gub"
    variant_artifact = project / "main-text-slice.gub"
    original_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(original), "--output", str(original_artifact)], project
    )
    variant_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(variant), "--output", str(variant_artifact)], project
    )
    data_args = ["--events", "events.jsonl", "--output", "summary.json"]
    commands = {
        "guion_original_source_uncached": runner.uncached_source_command(
            [str(binary), "run", "--json", str(original), *data_args]
        ),
        "guion_text_slice_source_uncached": runner.uncached_source_command(
            [str(binary), "run", "--json", str(variant), *data_args]
        ),
        "guion_original_artifact": [str(binary), "run", "--json", str(original_artifact), *data_args],
        "guion_text_slice_artifact": [str(binary), "run", "--json", str(variant_artifact), *data_args],
        "python_reference": [str(python_binary), str(REFERENCE), "python", str(events), str(services), str(output)],
    }
    samples, trial_metadata = runner.sample_candidates(
        f"jsonl-text-slice-{rows}",
        commands,
        project,
        expected,
        output_file=output,
        trials=trials,
    )
    comparisons = {
        "text_slice_source_over_original_source": paired_ratio(
            samples, "guion_text_slice_source_uncached", "guion_original_source_uncached"
        ),
        "text_slice_artifact_over_original_artifact": paired_ratio(
            samples, "guion_text_slice_artifact", "guion_original_artifact"
        ),
        "original_source_over_python": paired_ratio(samples, "guion_original_source_uncached", "python_reference"),
        "text_slice_source_over_python": paired_ratio(samples, "guion_text_slice_source_uncached", "python_reference"),
        "original_artifact_over_python": paired_ratio(samples, "guion_original_artifact", "python_reference"),
        "text_slice_artifact_over_python": paired_ratio(samples, "guion_text_slice_artifact", "python_reference"),
    }
    memory: dict[str, int] = {}
    for lane, command in commands.items():
        output.unlink(missing_ok=True)
        observed = runner.peak_memory(command, project)
        if observed is None:
            raise RuntimeError(f"peak RSS measurement failed for {lane}")
        actual = runner.normalize_data(json.loads(output.read_text(encoding="utf-8")))
        if actual != expected:
            raise RuntimeError(f"peak RSS output mismatch for {lane}")
        memory[lane] = observed

    return {
        "rows": rows,
        "correctness_status": "passed",
        "performance_status": {
            "source_text_slice_win": comparisons["text_slice_source_over_original_source"]["win"],
            "artifact_text_slice_win": comparisons["text_slice_artifact_over_original_artifact"]["win"],
        },
        "build_seconds": {
            "original_artifact": original_build_seconds,
            "text_slice_artifact": variant_build_seconds,
        },
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        "peak_rss_bytes": memory,
        "source_sha256": {
            "frozen_application": sha256_bytes(original_text.encode("utf-8")),
            "text_slice_variant": sha256_bytes(variant_text.encode("utf-8")),
        },
        "input_sha256": {
            "events": sha256_file(events),
            "services": sha256_file(services),
            "python_reference": sha256_file(REFERENCE),
            "generator": sha256_file(GENERATOR),
        },
        **trial_metadata,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--scratch-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rows", type=int, nargs="+", default=[20_000, 200_000])
    parser.add_argument("--trials", type=int, default=15)
    args = parser.parse_args()
    if args.trials < 2:
        parser.error("--trials must be at least 2")
    binary = args.binary.resolve(strict=True)
    python_binary = args.python.resolve(strict=True)
    args.scratch_dir.mkdir(parents=True, exist_ok=True)
    python_version = subprocess.run(
        [str(python_binary), "--version"], capture_output=True, text=True, check=True
    ).stdout.strip()
    workloads = {
        str(rows): measure_rows(binary, python_binary, args.scratch_dir, rows, args.trials)
        for rows in args.rows
    }
    report = {
        "schema_version": 1,
        "kind": "guion-jsonl-text-slice-source-variant",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "passed",
        "scope": "same binary; source-only application change from text_chars indexing to text_slice; original source remains unchanged",
        "binary_sha256": sha256_file(binary),
        "toolchain": {
            "python": python_version,
            "platform": platform.platform(),
            "architecture": platform.machine(),
        },
        "workloads": workloads,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": "passed", "output": str(args.output), "rows": list(workloads)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
