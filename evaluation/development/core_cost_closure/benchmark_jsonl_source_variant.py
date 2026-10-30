#!/usr/bin/env python3
"""Compare the frozen JSONL app with semantics-preserving source variants."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402

SOURCE_REPLACEMENT = (
    "                        let characters = text_chars(at)\n"
    "                        let date = characters[0] + characters[1] + characters[2] + characters[3] + characters[4] + characters[5] + characters[6] + characters[7] + characters[8] + characters[9]"
)
SLICE_REPLACEMENT = "                        let date = text_slice(at, 0, 10)"
UTC_DATE_REPLACEMENT = "                        let date = format_utc_date(instant)"
TYPED_RECORD_BLOCK = """                        var by_date: Map[Text, EventSummary] = {}
                        if map_has(groups, service):
                            set by_date = groups[service]
                        if map_has(by_date, date):
                            let previous = by_date[date]
                            var oldest = previous.oldest
                            var oldest_us = previous.oldest_us
                            if instant.unix_microseconds < oldest_us:
                                set oldest = at
                                set oldest_us = instant.unix_microseconds
                            var newest = previous.newest
                            var newest_us = previous.newest_us
                            if instant.unix_microseconds > newest_us:
                                set newest = at
                                set newest_us = instant.unix_microseconds
                            let updated = EventSummary(previous.count + 1, oldest, newest, oldest_us, newest_us)
                            set by_date = map_set(by_date, date, updated)
                        else:
                            let initial = EventSummary(1, at, at, instant.unix_microseconds, instant.unix_microseconds)
                            set by_date = map_set(by_date, date, initial)
                        set groups = map_set(groups, service, by_date)"""
TYPED_OUTPUT_BLOCK = """            let internal = by_date[date]
            var group = json_object()
            set group = json_set(group, "service", json_text(service))
            set group = json_set(group, "date", json_text(date))
            set group = json_set(group, "count", json_int(internal.count))
            set group = json_set(group, "oldest", json_text(internal.oldest))
            set group = json_set(group, "newest", json_text(internal.newest))
            set output_groups = json_push(output_groups, group)"""
COMPACT_RECORD_BLOCK = """                        var by_date: Map[Text, EventSummary] = {}
                        if map_has(groups, service):
                            set by_date = groups[service]
                        if map_has(by_date, date):
                            let previous = by_date[date]
                            var oldest_us = previous.oldest_us
                            if instant.unix_microseconds < oldest_us:
                                set oldest_us = instant.unix_microseconds
                            var newest_us = previous.newest_us
                            if instant.unix_microseconds > newest_us:
                                set newest_us = instant.unix_microseconds
                            let updated = EventSummary(previous.count + 1, oldest_us, newest_us)
                            set by_date = map_set(by_date, date, updated)
                        else:
                            let initial = EventSummary(1, instant.unix_microseconds, instant.unix_microseconds)
                            set by_date = map_set(by_date, date, initial)
                        set groups = map_set(groups, service, by_date)"""
COMPACT_OUTPUT_BLOCK = """            let internal = by_date[date]
            var group = json_object()
            set group = json_set(group, "service", json_text(service))
            set group = json_set(group, "date", json_text(date))
            set group = json_set(group, "count", json_int(internal.count))
            set group = json_set(group, "oldest", json_text(format_utc(Instant(internal.oldest_us))))
            set group = json_set(group, "newest", json_text(format_utc(Instant(internal.newest_us))))
            set output_groups = json_push(output_groups, group)"""


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def variant_source(original_text: str, variant: str) -> str:
    updated = original_text
    if variant in (
        "text-slice",
        "text-slice-and-typed-record",
        "text-slice-and-compact-record",
        "utc-date-and-compact-record",
    ):
        if original_text.count(SOURCE_REPLACEMENT) != 1:
            raise ValueError("frozen JSONL source no longer has the expected date-character extraction")
        updated = updated.replace(SOURCE_REPLACEMENT, SLICE_REPLACEMENT, 1)
    if variant == "utc-date-and-compact-record":
        if updated.count(SLICE_REPLACEMENT) != 1:
            raise ValueError("compact JSONL variant has no unique date slice to replace")
        updated = updated.replace(SLICE_REPLACEMENT, UTC_DATE_REPLACEMENT, 1)
        format_line = "                        let at = format_utc(instant)\n"
        if updated.count(format_line) != 1:
            raise ValueError("compact JSONL variant has no unique per-row timestamp formatting")
        updated = updated.replace(format_line, "", 1)
    if variant in (
        "typed-record",
        "text-slice-and-typed-record",
        "compact-record",
        "text-slice-and-compact-record",
        "utc-date-and-compact-record",
    ):
        start_marker = "                        var by_date: Map[Text, Json] = {}\n"
        end_marker = "                        set groups = map_set(groups, service, by_date)\n"
        if updated.count(start_marker) != 1 or updated.count(end_marker) != 1:
            raise ValueError("frozen JSONL source no longer has the expected aggregate update")
        start = updated.index(start_marker)
        end = updated.index(end_marker, start) + len(end_marker)
        record_block = COMPACT_RECORD_BLOCK if "compact-record" in variant else TYPED_RECORD_BLOCK
        output_block = COMPACT_OUTPUT_BLOCK if "compact-record" in variant else TYPED_OUTPUT_BLOCK
        updated = updated[:start] + record_block + "\n" + updated[end:]
        output_start = "            let internal = by_date[date]\n"
        output_end = "            set output_groups = json_push(output_groups, group)\n"
        if updated.count(output_start) != 1 or updated.count(output_end) != 1:
            raise ValueError("frozen JSONL source no longer has the expected output aggregation")
        start = updated.index(output_start)
        end = updated.index(output_end, start) + len(output_end)
        updated = updated[:start] + output_block + "\n" + updated[end:]
        groups_line = "    var groups: Map[Text, Map[Text, Json]] = {}"
        if updated.count(groups_line) != 1:
            raise ValueError("frozen JSONL source no longer has the expected groups declaration")
        updated = updated.replace(
            groups_line,
            "    var groups: Map[Text, Map[Text, EventSummary]] = {}",
            1,
        )
        main_marker = "fn main() -> Int:\n"
        if updated.count(main_marker) != 1:
            raise ValueError("frozen JSONL source has no unique main function")
        record_fields = (
            "record EventSummary:\n"
            "    count: Int\n"
            "    oldest_us: Int\n"
            "    newest_us: Int\n\n"
            if "compact-record" in variant
            else "record EventSummary:\n"
            "    count: Int\n"
            "    oldest: Text\n"
            "    newest: Text\n"
            "    oldest_us: Int\n"
            "    newest_us: Int\n\n"
        )
        return updated.replace(
            main_marker,
            record_fields + main_marker,
            1,
        )
    if variant not in (
        "text-slice",
        "typed-record",
        "text-slice-and-typed-record",
        "compact-record",
        "text-slice-and-compact-record",
        "utc-date-and-compact-record",
    ):
        raise ValueError(f"unsupported source variant: {variant}")
    return updated


def measure(
    binary: Path,
    scratch: Path,
    rows: int,
    trials: int,
    variant: str,
    control_binary: Path | None = None,
    comparison_variant: str | None = None,
) -> dict[str, Any]:
    workload_id = f"jsonl-summary-data-{rows}"
    project = scratch / workload_id
    project.mkdir(parents=True, exist_ok=True)
    runner.shutil.copytree(runner.APP, project, dirs_exist_ok=True)
    runner.prepare_guion_application(project)
    manifest = project / "guion.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8")
        + "\n[runtime]\ninstruction_limit = 1000000000\nallocation_limit_bytes = 8589934592\n",
        encoding="utf-8",
    )

    original_source = project / "main.gu"
    original_text = original_source.read_text(encoding="utf-8")
    variant_source_path = project / f"main.{variant}.gu"
    variant_text = variant_source(original_text, variant)
    variant_source_path.write_text(variant_text, encoding="utf-8")
    comparison_source_path = None
    if comparison_variant is not None:
        comparison_source_path = project / f"main.{comparison_variant}.gu"
        comparison_source_path.write_text(
            variant_source(original_text, comparison_variant), encoding="utf-8"
        )

    generated = project / "performance-inputs"
    events = generated / "events.jsonl"
    services = generated / "services.txt"
    output = generated / "summary.json"
    runner.require_ok(
        [
            sys.executable,
            str(runner.GENERATOR),
            "--rows",
            str(rows),
            "--output",
            str(events),
            "--services",
            str(services),
        ],
        project,
    )
    reference = project / "data_reference.py"
    runner.shutil.copy2(runner.DATA_REFERENCE, reference)
    expected_path = project / "expected.json"
    runner.require_ok(
        [sys.executable, str(reference), "python", str(events), str(services), str(expected_path)],
        project,
    )
    expected = runner.normalize_data(json.loads(expected_path.read_text(encoding="utf-8")))
    sqlite_expected = project / "expected-sqlite.json"
    runner.require_ok(
        [sys.executable, str(reference), "sqlite", str(events), str(services), str(sqlite_expected)],
        project,
    )
    if json.loads(sqlite_expected.read_text(encoding="utf-8")) != expected:
        raise RuntimeError(f"Python dictionary and native SQLite references disagree at {rows} rows")

    original_artifact = project / "original.gub"
    variant_artifact = project / f"{variant}.gub"
    comparison_artifact = (
        project / f"{comparison_variant}.gub" if comparison_variant is not None else None
    )
    original_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(original_source), "--output", str(original_artifact)], project
    )
    variant_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(variant_source_path), "--output", str(variant_artifact)], project
    )
    comparison_build_seconds = None
    if comparison_variant is not None:
        assert comparison_source_path is not None and comparison_artifact is not None
        comparison_build_seconds, _ = runner.require_ok(
            [str(binary), "build", str(comparison_source_path), "--output", str(comparison_artifact)],
            project,
        )
    args = ["--events", str(events.relative_to(project)), "--output", str(output.relative_to(project))]
    commands = {
        "guion_original_source": runner.uncached_source_command(
            [str(binary), "run", "--json", str(original_source), *args]
        ),
        "guion_variant_source": runner.uncached_source_command(
            [str(binary), "run", "--json", str(variant_source_path), *args]
        ),
        "guion_original_artifact": [str(binary), "run", "--json", str(original_artifact), *args],
        "guion_variant_artifact": [str(binary), "run", "--json", str(variant_artifact), *args],
        "python_data": [sys.executable, str(reference), "python", str(events), str(services), str(output)],
    }
    if comparison_variant is not None:
        assert comparison_source_path is not None and comparison_artifact is not None
        commands.update(
            {
                "guion_comparison_source": runner.uncached_source_command(
                    [str(binary), "run", "--json", str(comparison_source_path), *args]
                ),
                "guion_comparison_artifact": [
                    str(binary), "run", "--json", str(comparison_artifact), *args
                ],
            }
        )
    if control_binary is not None:
        commands.update(
            {
                "guion_before_original_source": runner.uncached_source_command(
                    [str(control_binary), "run", "--json", str(original_source), *args]
                ),
                "guion_before_variant_source": runner.uncached_source_command(
                    [str(control_binary), "run", "--json", str(variant_source_path), *args]
                ),
                "guion_before_original_artifact": [
                    str(control_binary), "run", "--json", str(original_artifact), *args
                ],
                "guion_before_variant_artifact": [
                    str(control_binary), "run", "--json", str(variant_artifact), *args
                ],
            }
        )
    runner.write_manifest(project, f"source-variant-{workload_id}", 1_000_000_000)
    samples, trial_metadata = runner.sample_candidates(
        workload_id,
        commands,
        project,
        expected,
        output_file=output,
        repetitions=1,
        trials=trials,
    )

    def paired_ratio(numerator: str, denominator: str) -> dict[str, Any]:
        ratios = [
            left / right
            for left, right in zip(
                samples[numerator]["samples_seconds"],
                samples[denominator]["samples_seconds"],
                strict=True,
            )
        ]
        return runner.paired_median_ci(ratios)

    comparisons = {
        "variant_source_over_original_source": paired_ratio("guion_variant_source", "guion_original_source"),
        "variant_artifact_over_original_artifact": paired_ratio("guion_variant_artifact", "guion_original_artifact"),
        "original_source_over_python": paired_ratio("guion_original_source", "python_data"),
        "variant_source_over_python": paired_ratio("guion_variant_source", "python_data"),
        "original_artifact_over_python": paired_ratio("guion_original_artifact", "python_data"),
        "variant_artifact_over_python": paired_ratio("guion_variant_artifact", "python_data"),
    }
    if comparison_variant is not None:
        comparisons.update(
            {
                "variant_source_over_comparison_source": paired_ratio(
                    "guion_variant_source", "guion_comparison_source"
                ),
                "variant_artifact_over_comparison_artifact": paired_ratio(
                    "guion_variant_artifact", "guion_comparison_artifact"
                ),
            }
        )
    if control_binary is not None:
        comparisons.update(
            {
                "after_original_source_over_before_original_source": paired_ratio(
                    "guion_original_source", "guion_before_original_source"
                ),
                "after_variant_source_over_before_variant_source": paired_ratio(
                    "guion_variant_source", "guion_before_variant_source"
                ),
                "after_original_artifact_over_before_original_artifact": paired_ratio(
                    "guion_original_artifact", "guion_before_original_artifact"
                ),
                "after_variant_artifact_over_before_variant_artifact": paired_ratio(
                    "guion_variant_artifact", "guion_before_variant_artifact"
                ),
            }
        )

    peak_rss_bytes: dict[str, int] = {}
    for name, command in commands.items():
        output.unlink(missing_ok=True)
        peak_rss = runner.peak_memory(command, project)
        if peak_rss is None or not output.is_file():
            raise RuntimeError(f"memory sample failed for {workload_id}/{name}")
        actual = runner.normalize_data(json.loads(output.read_text(encoding="utf-8")))
        if actual != expected:
            raise RuntimeError(f"memory-sampled output differs from oracle for {workload_id}/{name}")
        peak_rss_bytes[name] = peak_rss

    profile = runner.profile_guion_run(
        commands["guion_variant_artifact"],
        project,
        workload_id,
        expected,
        output_file=output,
        runtime_profile_enabled=True,
    )
    return {
        "rows": rows,
        "trials_per_lane": trials,
        "repetitions_per_process": 1,
        "generator_seed": 20261025,
        "application_source_sha256": {
            "original": digest(original_source),
            variant: digest(variant_source_path),
            **(
                {comparison_variant: digest(comparison_source_path)}
                if comparison_variant is not None and comparison_source_path is not None
                else {}
            ),
        },
        "artifact_sha256": {
            "original": digest(original_artifact),
            variant: digest(variant_artifact),
            **(
                {comparison_variant: digest(comparison_artifact)}
                if comparison_variant is not None and comparison_artifact is not None
                else {}
            ),
        },
        "build_seconds": {
            "original": original_build_seconds,
            variant: variant_build_seconds,
            **(
                {comparison_variant: comparison_build_seconds}
                if comparison_variant is not None and comparison_build_seconds is not None
                else {}
            ),
        },
        "samples": samples,
        "comparisons": comparisons,
        "peak_rss_bytes": peak_rss_bytes,
        "variant_profile": profile,
        **trial_metadata,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument(
        "--control-binary",
        type=Path,
        help="optional pre-change executable to interleave in the same paired cohort",
    )
    parser.add_argument("--scratch-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--variant",
        choices=(
            "text-slice",
            "typed-record",
            "text-slice-and-typed-record",
            "compact-record",
            "text-slice-and-compact-record",
            "utc-date-and-compact-record",
        ),
        default="text-slice",
    )
    parser.add_argument(
        "--comparison-variant",
        choices=(
            "text-slice",
            "typed-record",
            "text-slice-and-typed-record",
            "compact-record",
            "text-slice-and-compact-record",
            "utc-date-and-compact-record",
        ),
        help="sample a second source variant in the same randomized cohort for a paired comparison",
    )
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--rows", type=int, nargs="+", default=[20_000, 200_000])
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three paired trials are required")
    binary = args.binary.resolve(strict=True)
    control_binary = args.control_binary.resolve(strict=True) if args.control_binary else None
    workloads = {
        str(rows): measure(
            binary,
            args.scratch_dir.resolve(),
            rows,
            args.trials,
            args.variant,
            control_binary,
            args.comparison_variant,
        )
        for rows in args.rows
    }
    report = {
        "schema_version": 1,
        "kind": "guion-jsonl-source-variant-comparison",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": runner.subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip(),
        "binary": {
            "path": str(binary),
            "sha256": digest(binary),
        },
        "control_binary": (
            {"path": str(control_binary), "sha256": digest(control_binary)}
            if control_binary is not None
            else None
        ),
        "variant": {
            "name": args.variant,
            "description": {
                "text-slice": "Replace date character-list construction with text_slice(at, 0, 10).",
                "typed-record": "Store per-service/per-date aggregates in a typed EventSummary record and convert to JSON only when building the final report.",
                "text-slice-and-typed-record": "Apply text slicing and typed EventSummary aggregation together.",
                "compact-record": "Store only count and timestamp microseconds in a typed EventSummary record; format the retained timestamps when building final JSON.",
                "text-slice-and-compact-record": "Apply text slicing and compact typed EventSummary aggregation together.",
                "utc-date-and-compact-record": "Use the direct UTC date formatter for grouping, skip full timestamp formatting per input row, and format only retained extrema for final JSON.",
            }[args.variant],
            "equivalence_basis": {
                "text-slice": "format_utc produces a canonical ASCII UTC timestamp; its first ten Unicode scalars are YYYY-MM-DD.",
                "typed-record": "The record fields preserve the same count, earliest/latest timestamps and microsecond comparisons; the final JSON projection preserves the report schema.",
                "text-slice-and-typed-record": "The combined transformations preserve canonical date extraction, aggregate comparison semantics, and final report shape.",
                "compact-record": "The stored microsecond minima/maxima preserve the original comparisons; formatting them at output preserves the UTC strings.",
                "text-slice-and-compact-record": "Canonical date slicing and stored microsecond minima/maxima preserve grouping and report values.",
                "utc-date-and-compact-record": "The date operation derives the same UTC calendar date directly from each parsed instant; earliest and latest timestamps retain exact microseconds and are formatted canonically for the same output fields.",
            }[args.variant],
        },
        "comparison_variant": (
            {
                "name": args.comparison_variant,
                "description": "A second implementation sampled in the same randomized cohort for direct paired comparison.",
            }
            if args.comparison_variant is not None
            else None
        ),
        "workloads": workloads,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": "recorded", "output": str(args.output), "workloads": list(workloads)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
