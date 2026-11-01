#!/usr/bin/env python3
"""Output-check and paired benchmark harness for the approved core-cost sprint."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402

CORE = ROOT / "evaluation/development/core_cost_closure"
FIXTURES = CORE / "fixtures"
JSONL_APP = ROOT / "evaluation/completion/applications/jsonl-summary"
GENERATOR = ROOT / "evaluation/completion/performance/inputs/generate_analysis_jsonl.py"
REFERENCE = ROOT / "evaluation/completion/performance/data_reference.py"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def command_output(command: list[str], cwd: Path) -> str:
    elapsed, raw = runner.execute(command, cwd)
    if raw.returncode:
        raise RuntimeError(
            f"command failed in {elapsed:.3f}s ({raw.returncode}): {command!r}\n"
            f"stdout:\n{raw.stdout}\nstderr:\n{raw.stderr}"
        )
    return raw.stdout


def guion_stdout(command: list[str], cwd: Path) -> str:
    _, raw, envelope = runner.guion_result(command, cwd)
    if envelope is None:
        raise RuntimeError("Guion command did not return the requested JSON envelope")
    return envelope["program_stdout"]


def build_app_project(project: Path, app_source: Path | None = None) -> Path:
    shutil.copytree(JSONL_APP, project, dirs_exist_ok=True)
    runner.prepare_guion_application(project)
    source = project / "main.gu"
    if app_source is not None:
        shutil.copy2(app_source, source)
    manifest = project / "guion.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8")
        + "\n[runtime]\ninstruction_limit = 1000000000\nallocation_limit_bytes = 8589934592\n",
        encoding="utf-8",
    )
    return source


def make_jsonl_project(project: Path, rows: int, app_source: Path | None) -> dict[str, Any]:
    source = build_app_project(project, app_source)
    generated = project / "performance-inputs"
    generated.mkdir(exist_ok=True)
    events = generated / "events.jsonl"
    services = generated / "services.txt"
    output = generated / "summary.json"
    command_output(
        [
            sys.executable,
            str(GENERATOR),
            "--rows",
            str(rows),
            "--output",
            str(events),
            "--services",
            str(services),
        ],
        project,
    )
    expected_path = project / "expected.json"
    command_output(
        [sys.executable, str(REFERENCE), "python", str(events), str(services), str(expected_path)],
        project,
    )
    expected = runner.normalize_data(json.loads(expected_path.read_text(encoding="utf-8")))
    sqlite_path = project / "expected-sqlite.json"
    command_output(
        [sys.executable, str(REFERENCE), "sqlite", str(events), str(services), str(sqlite_path)],
        project,
    )
    sqlite_expected = json.loads(sqlite_path.read_text(encoding="utf-8"))
    if runner.normalize_data(sqlite_expected) != expected:
        raise RuntimeError(f"Python and SQLite references disagree at {rows} rows")
    return {
        "source": source,
        "events": events,
        "services": services,
        "output": output,
        "expected": expected,
        "input_sha256": {"events": sha256(events), "services": sha256(services)},
    }


def validate_jsonl_failure_cases(
    binary: Path,
    project: Path,
    source: Path,
    artifact: Path,
    workload: dict[str, Any],
) -> dict[str, Any]:
    """Check strict, denied-path, and oversized-input behavior outside timed lanes."""
    if not workload["expected"].get("invalid"):
        raise RuntimeError("failure-case validation requires generated invalid JSONL rows")

    events = workload["events"]
    services = workload["services"]
    output_dir = workload["output"].parent
    oversized_events = output_dir / "oversized-events.jsonl"
    oversized_bytes = 16 * 1024 * 1024 + 1
    with oversized_events.open("wb") as handle:
        chunk = b"x" * (1024 * 1024)
        full_chunks, remainder = divmod(oversized_bytes, len(chunk))
        for _ in range(full_chunks):
            handle.write(chunk)
        if remainder:
            handle.write(chunk[:remainder])

    cases = (
        ("strict-invalid", events, True, "invalid JSONL event at physical line"),
        ("denied-outside-root", Path("/etc/passwd"), False, "cannot read events JSONL"),
        ("oversized-input", oversized_events, False, "cannot read events JSONL"),
    )
    results: dict[str, Any] = {
        "oversized_input_bytes": oversized_bytes,
        "oversized_input_sha256": sha256(oversized_events),
        "modes": {},
    }
    try:
        for mode, program in (("source", source), ("artifact", artifact)):
            mode_results = {}
            for case_name, events_arg, strict, expected_marker in cases:
                output = output_dir / f"{case_name}-{mode}.json"
                output.unlink(missing_ok=True)
                try:
                    events_value = str(events_arg.relative_to(project))
                except ValueError:
                    events_value = str(events_arg)
                command = [
                    str(binary),
                    "run",
                    "--json",
                    str(program),
                    "--events",
                    events_value,
                    "--output",
                    str(output.relative_to(project)),
                    "--services",
                    str(services.relative_to(project)),
                ]
                if strict:
                    command.append("--strict")
                if mode == "source":
                    command = runner.uncached_source_command(command)
                elapsed, result = runner.execute(command, project)
                diagnostic = result.stdout + result.stderr
                if result.returncode == 0:
                    raise RuntimeError(f"{mode} {case_name} unexpectedly succeeded")
                if expected_marker not in diagnostic:
                    raise RuntimeError(
                        f"{mode} {case_name} did not report {expected_marker!r}: {diagnostic!r}"
                    )
                if output.exists():
                    raise RuntimeError(f"{mode} {case_name} wrote output after rejecting the input")
                mode_results[case_name] = {
                    "exit_code": result.returncode,
                    "elapsed_seconds": elapsed,
                    "expected_diagnostic_seen": True,
                    "output_absent": True,
                }
            results["modes"][mode] = mode_results
    finally:
        oversized_events.unlink(missing_ok=True)
        for case_name, _, _, _ in cases:
            for mode in ("source", "artifact"):
                (output_dir / f"{case_name}-{mode}.json").unlink(missing_ok=True)
    return results


def run_jsonl_lane(
    binary: Path,
    project: Path,
    workload: dict[str, Any],
    trials: int,
    baseline_binary: Path | None = None,
    validate_failure_cases: bool = False,
) -> dict[str, Any]:
    source = workload["source"]
    output = workload["output"]
    events = workload["events"]
    services = workload["services"]
    expected = workload["expected"]
    candidate_artifact = project / "candidate.gub"
    candidate_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(source), "--output", str(candidate_artifact)], project
    )
    args = [
        "--events",
        str(events.relative_to(project)),
        "--output",
        str(output.relative_to(project)),
        "--services",
        str(services.relative_to(project)),
    ]
    commands = {
        "guion_candidate_source_uncached": runner.uncached_source_command(
            [str(binary), "run", "--json", str(source), *args]
        ),
        "guion_candidate_artifact": [str(binary), "run", "--json", str(candidate_artifact), *args],
        "python_reference": [
            sys.executable,
            str(REFERENCE),
            "python",
            str(events),
            str(services),
            str(output),
        ],
    }
    build_seconds = {"candidate": candidate_build_seconds}
    if baseline_binary is not None:
        baseline_artifact = project / "baseline.gub"
        baseline_build_seconds, _ = runner.require_ok(
            [str(baseline_binary), "build", str(source), "--output", str(baseline_artifact)], project
        )
        build_seconds["baseline"] = baseline_build_seconds
        commands["guion_baseline_source_uncached"] = runner.uncached_source_command(
            [str(baseline_binary), "run", "--json", str(source), *args]
        )
        commands["guion_baseline_artifact"] = [
            str(baseline_binary), "run", "--json", str(baseline_artifact), *args
        ]
    failure_case_checks = (
        validate_jsonl_failure_cases(binary, project, source, candidate_artifact, workload)
        if validate_failure_cases
        else None
    )
    samples, metadata = runner.sample_candidates(
        f"jsonl-{events.stat().st_size}",
        commands,
        project,
        expected,
        output_file=output,
        trials=trials,
    )
    source_name = "guion_candidate_source_uncached"
    artifact_name = "guion_candidate_artifact"
    comparisons = {
        "source_over_python": runner.paired_median_ci(
            [a / b for a, b in zip(samples[source_name]["samples_seconds"], samples["python_reference"]["samples_seconds"], strict=True)]
        ),
        "artifact_over_python": runner.paired_median_ci(
            [a / b for a, b in zip(samples[artifact_name]["samples_seconds"], samples["python_reference"]["samples_seconds"], strict=True)]
        ),
    }
    if baseline_binary is not None:
        comparisons.update(
            {
                "baseline_source_over_python": runner.paired_median_ci(
                    [
                        a / b
                        for a, b in zip(
                            samples["guion_baseline_source_uncached"]["samples_seconds"],
                            samples["python_reference"]["samples_seconds"],
                            strict=True,
                        )
                    ]
                ),
                "candidate_source_over_baseline": runner.paired_median_ci(
                    [
                        a / b
                        for a, b in zip(
                            samples["guion_candidate_source_uncached"]["samples_seconds"],
                            samples["guion_baseline_source_uncached"]["samples_seconds"],
                            strict=True,
                        )
                    ]
                ),
                "baseline_artifact_over_python": runner.paired_median_ci(
                    [
                        a / b
                        for a, b in zip(
                            samples["guion_baseline_artifact"]["samples_seconds"],
                            samples["python_reference"]["samples_seconds"],
                            strict=True,
                        )
                    ]
                ),
                "candidate_artifact_over_baseline": runner.paired_median_ci(
                    [
                        a / b
                        for a, b in zip(
                            samples["guion_candidate_artifact"]["samples_seconds"],
                            samples["guion_baseline_artifact"]["samples_seconds"],
                            strict=True,
                        )
                    ]
                ),
            }
        )
    profile = runner.profile_guion_run(
        commands[artifact_name], project, f"jsonl-{binary.stem}", expected, output_file=output,
        runtime_profile_enabled=True,
    )
    memory: dict[str, int] = {}
    for name, command in commands.items():
        output.unlink(missing_ok=True)
        observed = runner.peak_memory(command, project)
        if observed is None or not output.is_file():
            raise RuntimeError(f"memory sample failed for {name}")
        actual = runner.normalize_data(json.loads(output.read_text(encoding="utf-8")))
        if actual != expected:
            raise RuntimeError(f"memory sample output mismatch for {name}")
        memory[name] = observed
    return {
        "artifact_build_seconds": build_seconds,
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        "runtime_profile": profile,
        "peak_rss_bytes": memory,
        "failure_case_checks": failure_case_checks,
        **metadata,
    }


def run_date_extraction_ablation(
    binary: Path,
    scratch_dir: Path,
    rows: int,
    trials: int,
) -> dict[str, Any]:
    """Compare text_chars and text_slice with an identical nested-map application."""
    project = scratch_dir / f"jsonl-date-extraction-ablation-{rows}"
    project.mkdir(parents=True, exist_ok=True)
    workload = make_jsonl_project(
        project,
        rows,
        FIXTURES / "jsonl-streaming-summary/main.gu",
    )
    text_slice_source = workload["source"]
    text_chars_source = project / "text-chars.gu"
    shutil.copy2(FIXTURES / "jsonl-streaming-summary-text-chars/main.gu", text_chars_source)
    output = workload["output"]
    args = [
        "--events",
        str(workload["events"].relative_to(project)),
        "--output",
        str(output.relative_to(project)),
        "--services",
        str(workload["services"].relative_to(project)),
    ]
    artifacts = {
        "text_slice": project / "text-slice.gub",
        "text_chars": project / "text-chars.gub",
    }
    build_seconds = {}
    commands: dict[str, list[str]] = {}
    for name, source in (("text_slice", text_slice_source), ("text_chars", text_chars_source)):
        build_seconds[name], _ = runner.require_ok(
            [str(binary), "build", str(source), "--output", str(artifacts[name])], project
        )
        commands[f"guion_{name}_source_uncached"] = runner.uncached_source_command(
            [str(binary), "run", "--json", str(source), *args]
        )
        commands[f"guion_{name}_artifact"] = [
            str(binary), "run", "--json", str(artifacts[name]), *args
        ]
    commands["python_reference"] = [
        sys.executable,
        str(REFERENCE),
        "python",
        str(workload["events"]),
        str(workload["services"]),
        str(output),
    ]
    samples, metadata = runner.sample_candidates(
        f"jsonl-date-extraction-ablation-{rows}",
        commands,
        project,
        workload["expected"],
        output_file=output,
        trials=trials,
    )
    comparisons = {}
    for representation in ("source_uncached", "artifact"):
        chars = samples[f"guion_text_chars_{representation}"]["samples_seconds"]
        sliced = samples[f"guion_text_slice_{representation}"]["samples_seconds"]
        python = samples["python_reference"]["samples_seconds"]
        comparisons[f"text_chars_{representation}_over_text_slice"] = runner.paired_median_ci(
            [a / b for a, b in zip(chars, sliced, strict=True)]
        )
        comparisons[f"text_chars_{representation}_over_python"] = runner.paired_median_ci(
            [a / b for a, b in zip(chars, python, strict=True)]
        )
        comparisons[f"text_slice_{representation}_over_python"] = runner.paired_median_ci(
            [a / b for a, b in zip(sliced, python, strict=True)]
        )
    return {
        "rows": rows,
        "input_sha256": workload["input_sha256"],
        "reference_output_sha256": sha256(project / "expected.json"),
        "artifact_build_seconds": build_seconds,
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        **metadata,
    }


def run_focused(
    binary: Path,
    project: Path,
    fixture_dir: Path,
    filename: str,
    reference: Path,
    trials: int,
    baseline_binary: Path | None = None,
) -> dict[str, Any]:
    source = project / "main.gu"
    shutil.copy2(fixture_dir / filename, source)
    runner.write_manifest(project, "core-cost-focused", 1000000000)
    expected = runner.canonical_output(
        "python_reference", command_output([sys.executable, str(reference)], project), filename
    )
    candidate_artifact = project / "candidate.gub"
    candidate_build_seconds, _ = runner.require_ok(
        [str(binary), "build", str(source), "--output", str(candidate_artifact)], project
    )
    build_seconds = {"candidate": candidate_build_seconds}
    commands = {
        "guion_candidate_source_uncached": runner.uncached_source_command(
            [str(binary), "run", "--json", str(source)]
        ),
        "guion_candidate_artifact": [str(binary), "run", "--json", str(candidate_artifact)],
        "python_reference": [sys.executable, str(reference)],
    }
    if baseline_binary is not None:
        baseline_artifact = project / "baseline.gub"
        baseline_build_seconds, _ = runner.require_ok(
            [str(baseline_binary), "build", str(source), "--output", str(baseline_artifact)], project
        )
        build_seconds["baseline"] = baseline_build_seconds
        commands["guion_baseline_source_uncached"] = runner.uncached_source_command(
            [str(baseline_binary), "run", "--json", str(source)]
        )
        commands["guion_baseline_artifact"] = [
            str(baseline_binary), "run", "--json", str(baseline_artifact)
        ]
    samples, metadata = runner.sample_candidates(filename, commands, project, expected, trials=trials)
    comparisons = {}
    for lane in ("guion_candidate_source_uncached", "guion_candidate_artifact"):
        ratios = [a / b for a, b in zip(samples[lane]["samples_seconds"], samples["python_reference"]["samples_seconds"], strict=True)]
        comparisons[f"{lane}_over_python"] = runner.paired_median_ci(ratios)
    if baseline_binary is not None:
        for lane in ("guion_baseline_source_uncached", "guion_baseline_artifact"):
            ratios = [
                a / b
                for a, b in zip(
                    samples[lane]["samples_seconds"],
                    samples["python_reference"]["samples_seconds"],
                    strict=True,
                )
            ]
            comparisons[f"{lane}_over_python"] = runner.paired_median_ci(ratios)
        for candidate_lane, baseline_lane in (
            ("guion_candidate_source_uncached", "guion_baseline_source_uncached"),
            ("guion_candidate_artifact", "guion_baseline_artifact"),
        ):
            comparisons[f"{candidate_lane}_over_{baseline_lane}"] = runner.paired_median_ci(
                [
                    a / b
                    for a, b in zip(
                        samples[candidate_lane]["samples_seconds"],
                        samples[baseline_lane]["samples_seconds"],
                        strict=True,
                    )
                ]
            )
    profile = runner.profile_guion_run(
        commands["guion_candidate_artifact"], project, filename, expected, runtime_profile_enabled=True
    )
    return {
        "artifact_build_seconds": build_seconds,
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        "runtime_profile": profile,
        **metadata,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--baseline-binary", type=Path)
    parser.add_argument("--scratch-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--source-revision",
        help="record the source checkout revision when benchmarking a Git-less synced copy",
    )
    parser.add_argument(
        "--changed-source-path",
        type=Path,
        action="append",
        help="relative source path to hash when Git metadata is unavailable; repeat as needed",
    )
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--rows", type=int, nargs="+", default=[20000, 200000])
    parser.add_argument("--focused-only", action="store_true")
    parser.add_argument("--jsonl-only", action="store_true")
    parser.add_argument("--smoke", action="store_true", help="run one untimed warmup-free output-checked pass")
    args = parser.parse_args()
    if args.smoke:
        runner.WARMUPS = 0
        args.trials = 1
    binary = args.binary.resolve(strict=True)
    baseline_binary = args.baseline_binary.resolve(strict=True) if args.baseline_binary else None
    args.scratch_dir.mkdir(parents=True, exist_ok=True)
    source_revision = args.source_revision
    changed_source_paths = args.changed_source_path
    if source_revision is None:
        source_revision = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    if changed_source_paths is None:
        changed_source_paths = [
            Path(relative)
            for relative in subprocess.run(
                ["git", "-C", str(ROOT), "diff", "--name-only", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.splitlines()
        ]
    for relative in changed_source_paths:
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError(f"changed source path must stay under the checkout: {relative}")
    results: dict[str, Any] = {
        "schema_version": 1,
        "kind": "guion-core-cost-closure-benchmark",
        "binary": str(binary),
        "binary_sha256": sha256(binary),
        "baseline_binary": str(baseline_binary) if baseline_binary else None,
        "baseline_binary_sha256": sha256(baseline_binary) if baseline_binary else None,
        "source_revision": source_revision,
        "benchmark_harness_sha256": sha256(Path(__file__)),
        "working_tree_source_sha256": {
            str(relative): sha256(ROOT / relative) for relative in changed_source_paths
        },
        "source_sha256": {
            str(path.relative_to(ROOT)): sha256(path)
            for path in [
                FIXTURES / "result-option-loop/main.gu",
                FIXTURES / "result-option-loop/reference.py",
                FIXTURES / "json-field-extract/main.gu",
                FIXTURES / "json-field-extract/reference.py",
                FIXTURES / "aggregate-update/json-state.gu",
                FIXTURES / "aggregate-update/compact-record.gu",
                FIXTURES / "aggregate-update/mutable-record.gu",
                FIXTURES / "aggregate-update/reference.py",
                JSONL_APP / "main.vr",
                FIXTURES / "jsonl-summary-text-slice-compact-record/main.gu",
                FIXTURES / "jsonl-streaming-summary/main.gu",
                FIXTURES / "jsonl-streaming-summary-text-chars/main.gu",
                GENERATOR,
                REFERENCE,
            ]
        },
        "trials": args.trials,
        "rows": args.rows,
        "python": sys.version,
        "focused": {},
        "jsonl": {},
    }
    if not args.jsonl_only:
        focused_jobs = [
            ("result-option-loop", "main.gu", FIXTURES / "result-option-loop/reference.py"),
            ("json-field-extract", "main.gu", FIXTURES / "json-field-extract/reference.py"),
            ("aggregate-update", "json-state.gu", FIXTURES / "aggregate-update/reference.py"),
            ("aggregate-update", "compact-record.gu", FIXTURES / "aggregate-update/reference.py"),
            ("aggregate-update", "mutable-record.gu", FIXTURES / "aggregate-update/reference.py"),
        ]
        for index, (directory, source, reference) in enumerate(focused_jobs):
            project = args.scratch_dir / f"focused-{index}-{Path(source).stem}"
            project.mkdir(exist_ok=True)
            results["focused"][f"{directory}/{source}"] = run_focused(
                binary,
                project,
                FIXTURES / directory,
                source,
                reference,
                args.trials,
                baseline_binary if source != "mutable-record.gu" else None,
            )
    if not args.focused_only:
        lanes = {
            "original-json-state": None,
            "compact-record-text-slice": FIXTURES / "jsonl-summary-text-slice-compact-record/main.gu",
            "nested-map-record-text-slice": FIXTURES / "jsonl-streaming-summary/main.gu",
        }
        for rows in args.rows:
            for lane_name, lane_source in lanes.items():
                key = f"{lane_name}/{rows}"
                project = args.scratch_dir / f"jsonl-{lane_name}-{rows}"
                project.mkdir(exist_ok=True)
                workload = make_jsonl_project(project, rows, lane_source)
                results["jsonl"][key] = {
                    "input_sha256": workload["input_sha256"],
                    "result": run_jsonl_lane(
                        binary,
                        project,
                        workload,
                        args.trials,
                        baseline_binary if lane_name != "nested-map-record-text-slice" else None,
                        validate_failure_cases=(
                            lane_name == "nested-map-record-text-slice" and rows == args.rows[0]
                        ),
                    ),
                }
        results["jsonl"][f"text-date-extraction-ablation/{max(args.rows)}"] = (
            run_date_extraction_ablation(binary, args.scratch_dir, max(args.rows), args.trials)
        )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
