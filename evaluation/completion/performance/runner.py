#!/usr/bin/env python3
"""Run the reproducible Task 10 performance suite against an installed release."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
WORKLOADS = ROOT / "evaluation/development/famous_benchmarks"
APP = ROOT / "evaluation/completion/applications/jsonl-summary"
DATA_REFERENCE = Path(__file__).with_name("data_reference.py")
GENERATOR = Path(__file__).parent / "inputs/generate_analysis_jsonl.py"
TRIALS = 15
DATA_TRIALS = 16
WARMUPS = 2
WARM_REPETITIONS = 5
RICHARDS_PROFILE_REPETITIONS = 25
TRIAL_SEED = 20261025
BOOTSTRAP_SEED = 20261026
BOOTSTRAP_SAMPLES = 20_000
PINNED_PYTHON = "3.14.7"
RESUMABLE_BASELINE_WORKLOADS = (
    "richards-scheduler",
    "nbody-numeric",
    "mandelbrot-image",
    "parallel-cpu-20000",
    "parallel-cpu-200000",
    "jsonl-summary-data",
)
MEMORY_MEASUREMENT = {
    "method": "resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss",
    "single_process_scope": "direct child process",
    "darwin_unit": "bytes",
    "other_platform_unit": "kilobytes, converted to bytes",
    "process_pool_scope": "individual worker peaks; their sum is an upper bound, not a simultaneous process-tree peak",
}


def execute(
    command: list[str], cwd: Path, extra_env: dict[str, str] | None = None
) -> tuple[float, subprocess.CompletedProcess[str]]:
    start = time.perf_counter()
    environment = None
    if Path(command[0]).name == "guion":
        environment = os.environ.copy()
        environment["GUION_FILE_ROOT"] = str(cwd.resolve())
    if extra_env:
        environment = environment or os.environ.copy()
        environment.update(extra_env)
    result = subprocess.run(command, cwd=cwd, env=environment, capture_output=True, text=True, check=False)
    return time.perf_counter() - start, result


def require_ok(command: list[str], cwd: Path) -> tuple[float, str]:
    elapsed, result = execute(command, cwd)
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {command!r}\n{result.stdout}\n{result.stderr}")
    return elapsed, result.stdout


def parse_program_output(name: str, raw: str) -> str:
    if name.startswith("guion_"):
        report = json.loads(raw)
        if report.get("status") != "passed":
            raise RuntimeError(f"Guion invocation failed: {report}")
        return report["program_stdout"]
    return raw


def canonical_output(name: str, raw: str, workload_id: str) -> Any:
    output = parse_program_output(name, raw)
    if workload_id == "nbody-numeric":
        try:
            return [float(line) for line in output.splitlines()]
        except ValueError as error:
            raise RuntimeError(f"invalid N-body output {output!r}") from error
    return output.strip().splitlines()


def equivalent(left: Any, right: Any, workload: str) -> bool:
    if workload == "nbody-numeric":
        return len(left) == len(right) and len(left) >= 2 and all(math.isclose(a, b, rel_tol=0, abs_tol=1e-12) for a, b in zip(left, right, strict=True))
    return left == right


def summary(samples: list[float]) -> dict[str, Any]:
    ordered = sorted(samples)
    return {
        "samples_seconds": samples,
        "median_seconds": statistics.median(samples),
        "p95_seconds_nearest_rank": ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)],
    }


def paired_median_ci(ratios: list[float]) -> dict[str, Any]:
    rng = random.Random(BOOTSTRAP_SEED)
    medians = []
    for _ in range(BOOTSTRAP_SAMPLES):
        medians.append(statistics.median(ratios[rng.randrange(len(ratios))] for _ in ratios))
    medians.sort()
    low = medians[int(0.025 * (len(medians) - 1))]
    high = medians[int(0.975 * (len(medians) - 1))]
    return {"median_ratio": statistics.median(ratios), "ci95_percentile": [low, high], "win": high < 1.0, "ties_or_regressions": sum(value >= 1.0 for value in ratios)}


def guion_result(
    command: list[str], cwd: Path, extra_env: dict[str, str] | None = None
) -> tuple[float, str, dict[str, Any] | None]:
    elapsed, result = execute(command, cwd, extra_env)
    if result.returncode:
        raise RuntimeError(f"Guion failed ({result.returncode}): {command!r}\n{result.stdout}\n{result.stderr}")
    envelope = json.loads(result.stdout) if "--json" in command else None
    if envelope and envelope.get("status") != "passed":
        raise RuntimeError(f"Guion execution reported failure: {envelope}")
    return elapsed, result.stdout, envelope


def uncached_source_command(command: list[str]) -> list[str]:
    """Return the same source-run command with Guion's compilation cache bypassed."""
    result = list(command)
    try:
        run_index = result.index("run")
    except ValueError as error:
        raise ValueError("uncached source command must contain the Guion run subcommand") from error
    if "--no-cache" in result:
        return result
    source_index = next((index for index in range(run_index + 1, len(result)) if not result[index].startswith("-")), None)
    if source_index is None:
        raise ValueError("uncached source command must identify a source file")
    result.insert(source_index, "--no-cache")
    return result


def profile_guion_run(
    command: list[str],
    cwd: Path,
    workload_id: str,
    expected: Any,
    *,
    output_file: Path | None = None,
    runtime_profile_enabled: bool = False,
) -> dict[str, Any]:
    """Collect phase telemetry in an untimed lane and verify the workload output."""
    if output_file is not None:
        output_file.unlink(missing_ok=True)
    profile_environment = {"GUION_PHASE_TIMINGS": "1"}
    if runtime_profile_enabled:
        profile_environment["GUION_RUNTIME_PROFILE"] = "1"
    elapsed, output, envelope = guion_result(
        command,
        cwd,
        extra_env=profile_environment,
    )
    if envelope is None:
        raise RuntimeError("phase profile requires Guion JSON output")
    if output_file is not None:
        if not output_file.is_file():
            raise RuntimeError(f"profiled Guion run did not create {output_file}")
        actual = normalize_data(json.loads(output_file.read_text(encoding="utf-8")))
        if actual != normalize_data(expected):
            raise RuntimeError(f"profiled {workload_id} data output differs from independent oracle")
    else:
        actual = canonical_output("guion_source", output, workload_id)
        if not equivalent(actual, expected, workload_id):
            raise RuntimeError(f"profiled {workload_id} output mismatch: {actual!r} != {expected!r}")
    profile = {
        "elapsed_seconds_including_process_startup": elapsed,
        "cache_status": envelope.get("source_cache"),
        "reported_phase_timings_ns": envelope.get("phase_timings_ns"),
    }
    if "runtime_profile" in envelope:
        profile["runtime_profile"] = envelope["runtime_profile"]
    if runtime_profile_enabled and "runtime_profile" not in profile:
        raise RuntimeError("runtime profile was requested but Guion returned no runtime_profile data")
    return profile


def native_compilation_profile_total_ns(profile: dict[str, Any]) -> int:
    phases = profile.get("reported_phase_timings_ns") or {}
    native = phases.get("native_compilation") or {}
    total = int(native.get("finalize_nanoseconds", 0))
    for function in native.get("functions", []):
        total += sum(
            int(function.get(field, 0))
            for field in ("lowering_nanoseconds", "verification_nanoseconds", "define_nanoseconds")
        )
    return total


def profile_richards(binary: Path, scratch: Path) -> dict[str, Any]:
    """Measure one-shot and repeated Richards source runs with separate diagnostics."""
    binary = binary.resolve()
    scratch = scratch.resolve()
    if not binary.is_file():
        raise FileNotFoundError(f"release Guion binary does not exist: {binary}")
    scratch.mkdir(parents=True, exist_ok=True)
    source_revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    runtime_source_hashes = {
        relative: hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()
        for relative in ("crates/runtime/src/lib.rs", "crates/runtime/tests/execution.rs")
    }
    binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    expected = ["9297", "23246"]
    with tempfile.TemporaryDirectory(prefix="richards-profile-", dir=scratch) as directory:
        project = Path(directory) / "project"
        project.mkdir(parents=True)
        source = project / "main.gu"
        shutil.copy2(WORKLOADS / "richards.vr", source)
        warm_source = project / "main-warm.gu"
        make_warm_source(source, warm_source, RICHARDS_PROFILE_REPETITIONS)
        write_manifest(project, "richards-runtime-profile", 30_000_000 * RICHARDS_PROFILE_REPETITIONS)
        commands = {
            "guion_source_uncached": uncached_source_command(
                [str(binary), "run", "--json", str(source)]
            ),
            "guion_source_warm_25": uncached_source_command(
                [str(binary), "run", "--json", str(warm_source)]
            ),
        }
        timed_lanes, trial_metadata = sample_candidates(
            "richards-scheduler",
            commands,
            project,
            expected,
            repetitions=RICHARDS_PROFILE_REPETITIONS,
            trials=TRIALS,
        )
        phase_profiles = {
            "one_shot": profile_guion_run(
                commands["guion_source_uncached"],
                project,
                "richards-scheduler",
                expected,
                runtime_profile_enabled=True,
            ),
            "repeated_25": profile_guion_run(
                commands["guion_source_warm_25"],
                project,
                "richards-scheduler",
                expected * RICHARDS_PROFILE_REPETITIONS,
                runtime_profile_enabled=True,
            ),
        }
    for profile in phase_profiles.values():
        profile["native_compilation_profile_total_ns"] = native_compilation_profile_total_ns(profile)
        profile["program_execute_ns_includes_native_compilation"] = True
    copy_size_distribution = {}
    for lane, profile in phase_profiles.items():
        list_set = profile["runtime_profile"]["list_set"]
        buckets = list_set["copy_size_histogram"]["buckets"]
        small_copies = buckets["0-8"]["copies"]
        small_copy_bytes = buckets["0-8"]["estimated_copy_bytes"]
        total_copies = list_set["copy_on_write_copies"]
        total_copy_bytes = list_set["estimated_copy_bytes"]
        gross_guest_bytes = profile["runtime_profile"]["estimated_guest_allocation_budget_charges"][
            "estimated_gross_bytes"
        ]
        copy_size_distribution[lane] = {
            "copies_0_to_8_elements": small_copies,
            "total_copy_on_write_copies": total_copies,
            "share_of_copies_percent": (small_copies / total_copies * 100) if total_copies else 0.0,
            "estimated_copy_bytes_0_to_8_elements": small_copy_bytes,
            "total_estimated_copy_bytes": total_copy_bytes,
            "share_of_estimated_copy_bytes_percent": (small_copy_bytes / total_copy_bytes * 100) if total_copy_bytes else 0.0,
            "total_estimated_gross_guest_allocation_bytes": gross_guest_bytes,
            "copy_bytes_share_of_gross_guest_allocation_percent": (total_copy_bytes / gross_guest_bytes * 100) if gross_guest_bytes else 0.0,
        }
    return {
        "schema_version": 1,
        "kind": "guion-richards-runtime-profile",
        "status": "passed",
        "host": {
            "platform": platform.platform(),
            "architecture": platform.machine(),
            "python_version": platform.python_version(),
        },
        "source_revision": source_revision,
        "runtime_source_sha256": runtime_source_hashes,
        "release_binary_sha256": binary_sha256,
        "repetitions_in_repeated_source_lane": RICHARDS_PROFILE_REPETITIONS,
        "timed_trials_per_lane": TRIALS,
        "expected_output_per_execution": expected,
        "cache_conditions": {
            "source_compile_cache": "bypassed with --no-cache",
            "os_file_cache": "uncontrolled; not a cold-disk measurement",
        },
        "timed_uninstrumented_samples_per_logical_execution": timed_lanes,
        "trial_metadata": trial_metadata,
        "untimed_phase_profiles": phase_profiles,
        "copy_size_distribution": copy_size_distribution,
        "metric_interpretation": {
            "allocation": "estimated guest-data budget charges, not host allocator calls",
            "branches": "dynamic guest conditional executions, not hardware branch-misprediction counts",
            "copy_size_histogram": "copy-on-write list lengths in elements; estimated bytes follow the existing guest-copy estimate and are not allocator traffic",
            "warm_lane": "25 executions in one process; reported timing is elapsed process time divided by 25",
        },
    }


def compare_richards_cohort(
    baseline_binary: Path,
    candidate_binary: Path,
    scratch: Path,
    *,
    baseline_revision: str,
    candidate_revision: str,
    python_executable: str = sys.executable,
    trials: int = TRIALS,
) -> dict[str, Any]:
    """Compare both Guion builds with Python in one randomized, output-checked cohort."""
    baseline_binary = baseline_binary.resolve()
    candidate_binary = candidate_binary.resolve()
    scratch = scratch.resolve()
    for name, binary in (("baseline", baseline_binary), ("candidate", candidate_binary)):
        if not binary.is_file():
            raise FileNotFoundError(f"{name} release Guion binary does not exist: {binary}")
    if trials < 2:
        raise ValueError("a paired Richards cohort requires at least two trials")

    python_version = subprocess.run(
        [python_executable, "--version"], capture_output=True, text=True, check=True
    ).stdout.strip().removeprefix("Python ")
    if python_version != PINNED_PYTHON:
        raise RuntimeError(f"Richards comparison requires Python {PINNED_PYTHON}, found {python_version}")

    expected = ["9297", "23246"]
    source_bytes = (WORKLOADS / "richards.vr").read_bytes()
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="richards-followup-", dir=scratch) as directory:
        project = Path(directory) / "project"
        project.mkdir(parents=True)
        source = project / "main.gu"
        source.write_bytes(source_bytes)
        write_manifest(project, "richards-scheduler-followup", 30_000_000)
        commands = {
            "guion_baseline_source_uncached": uncached_source_command(
                [str(baseline_binary), "run", "--json", str(source)]
            ),
            "guion_candidate_source_uncached": uncached_source_command(
                [str(candidate_binary), "run", "--json", str(source)]
            ),
            "python_reference": [python_executable, str(WORKLOADS / "richards_reference.py")],
        }
        samples, metadata = sample_candidates(
            "richards-scheduler", commands, project, expected, trials=trials
        )

    candidate_times = samples["guion_candidate_source_uncached"]["samples_seconds"]
    baseline_times = samples["guion_baseline_source_uncached"]["samples_seconds"]
    python_times = samples["python_reference"]["samples_seconds"]
    paired_ratios = {
        "candidate_over_python": paired_median_ci(
            [candidate / python for candidate, python in zip(candidate_times, python_times, strict=True)]
        ),
        "baseline_over_python": paired_median_ci(
            [baseline / python for baseline, python in zip(baseline_times, python_times, strict=True)]
        ),
        "candidate_over_baseline": paired_median_ci(
            [candidate / baseline for candidate, baseline in zip(candidate_times, baseline_times, strict=True)]
        ),
    }
    return {
        "schema_version": 1,
        "kind": "guion-richards-python-comparison",
        "status": "passed",
        "workload": "richards-scheduler",
        "baseline_revision": baseline_revision,
        "candidate_revision": candidate_revision,
        "baseline_binary_sha256": hashlib.sha256(baseline_binary.read_bytes()).hexdigest(),
        "candidate_binary_sha256": hashlib.sha256(candidate_binary.read_bytes()).hexdigest(),
        "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
        "host": {
            "platform": platform.platform(),
            "architecture": platform.machine(),
            "python_version": python_version,
        },
        "measurement_design": {
            "sampled_arms": list(commands),
            "trials_per_arm": trials,
            "randomized_paired_order": True,
            "warmup_trials_per_arm": WARMUPS,
            "guion_cache_condition": "--no-cache bypasses Guion's source cache",
            "os_file_cache": "uncontrolled",
            "expected_stdout_per_execution": expected,
            "output_checked_on_every_warmup_and_trial": True,
        },
        "commands": commands,
        "raw_samples_seconds": {
            name: result["samples_seconds"] for name, result in samples.items()
        },
        "arm_medians_seconds": {
            name: result["median_seconds"] for name, result in samples.items()
        },
        "comparisons": paired_ratios,
        "paired_trial_order": metadata["paired_trial_order"],
    }


def ru_maxrss_to_bytes(value: int, *, system: str | None = None) -> int:
    system = system or platform.system()
    return value if system == "Darwin" else value * 1024


def make_warm_source(source: Path, destination: Path, repeats: int) -> None:
    text = source.read_text(encoding="utf-8")
    signature = "fn main() -> Int:"
    if text.count(signature) != 1:
        raise RuntimeError(f"expected one benchmark entry in {source}")
    text = text.replace(signature, "fn benchmark_once() -> Int:", 1)
    text += f"\nfn main() -> Int:\n    var pass = 0\n    while pass < {repeats}:\n        let result = benchmark_once()\n        if result != 0:\n            return result\n        set pass = pass + 1\n    return 0\n"
    destination.write_text(text, encoding="utf-8")


def warm_python_source(path: Path, destination: Path, mode: str, count: int, repeats: int) -> None:
    destination.write_text(
        "import runpy, sys\n"
        f"sys.path.insert(0, {str(path.parent)!r})\n"
        f"path = {str(path)!r}\n"
        f"for _ in range({repeats}):\n"
        "    module = runpy.run_path(path, run_name='warm_benchmark')\n"
        f"    module['main']({mode!r}, {count})\n" if mode else
        "import runpy, sys\n"
        f"sys.path.insert(0, {str(path.parent)!r})\n"
        f"path = {str(path)!r}\n"
        f"for _ in range({repeats}):\n"
        "    module = runpy.run_path(path, run_name='warm_benchmark')\n"
        "    module['main']()\n",
        encoding="utf-8",
    )


def write_manifest(project: Path, name: str, instruction_limit: int) -> None:
    (project / "guion.toml").write_text(
        f'[project]\nname = "{name}"\nversion = "0.1.0"\nentry = "main.gu"\n\n[runtime]\ninstruction_limit = {instruction_limit}\nallocation_limit_bytes = 8589934592\n',
        encoding="utf-8",
    )


def prepare_guion_application(project: Path) -> None:
    """Convert the frozen Veritas fixture names in a temporary benchmark copy."""
    source = project / "main.vr"
    manifest = project / "veritas.toml"
    if not source.is_file() or not manifest.is_file():
        raise FileNotFoundError("frozen JSONL application source or manifest is missing")

    manifest_text, replacements = re.subn(
        r'(?m)^entry\s*=\s*"main\.vr"\s*$',
        'entry = "main.gu"',
        manifest.read_text(encoding="utf-8"),
        count=1,
    )
    if replacements != 1:
        raise ValueError("frozen JSONL application manifest has no main.vr entry")

    (project / "main.gu").write_bytes(source.read_bytes())
    (project / "guion.toml").write_text(manifest_text, encoding="utf-8")
    source.unlink()
    manifest.unlink()
    for legacy in ("veritas.lock", "delivery.json"):
        (project / legacy).unlink(missing_ok=True)


def sample_candidates(
    workload_id: str,
    commands: dict[str, list[str]],
    cwd: Path,
    expected: Any,
    *,
    output_file: Path | None = None,
    repetitions: int = 1,
    trials: int = TRIALS,
) -> tuple[dict[str, dict[str, Any]], dict[str, Any]]:
    rng = random.Random(TRIAL_SEED)
    samples = {name: [] for name in commands}
    native_reports: dict[str, Any] = {}
    source_cache_states: dict[str, list[str]] = {}
    order_log: list[list[str]] = []
    def check_one(name: str, command: list[str]) -> tuple[float, dict[str, Any] | None]:
        if name.startswith("guion_"):
            elapsed, output, envelope = guion_result(command, cwd)
        else:
            elapsed, result = execute(command, cwd)
            if result.returncode:
                raise RuntimeError(f"Python baseline failed: {command!r}\n{result.stderr}")
            output, envelope = result.stdout, None
        if output_file is not None:
            if not output_file.is_file():
                raise RuntimeError(f"{name} did not create its result file")
            actual = normalize_data(json.loads(output_file.read_text(encoding="utf-8")))
            if actual != normalize_data(expected):
                raise RuntimeError(f"{name} data output differs from independent oracle")
        else:
            actual = canonical_output(name, output, workload_id)
            wanted = expected * repetitions if "warm" in name else expected
            if not equivalent(actual, wanted, workload_id):
                raise RuntimeError(f"{name} output mismatch for {workload_id}: {actual!r} != {wanted!r}")
        if envelope:
            native_reports[name] = envelope.get("native_backend", {})
            if "source_cache" in envelope:
                source_cache_states.setdefault(name, []).append(envelope["source_cache"])
        return elapsed, envelope

    for _ in range(WARMUPS):
        order = list(commands)
        rng.shuffle(order)
        for name in order:
            if output_file is not None:
                try:
                    output_file.unlink()
                except FileNotFoundError:
                    pass
            check_one(name, commands[name])
    for trial in range(trials):
        order = list(commands)
        rng.shuffle(order)
        order_log.append(order)
        for name in order:
            if output_file is not None:
                try:
                    output_file.unlink()
                except FileNotFoundError:
                    pass
            elapsed, envelope = check_one(name, commands[name])
            divisor = repetitions if "warm" in name else 1
            samples[name].append(elapsed / divisor)
    return {name: summary(values) for name, values in samples.items()}, {
        "paired_trial_order": order_log,
        "native_backend_reports": native_reports,
        "source_cache_states": source_cache_states,
    }


def normalize_data(value: dict[str, Any]) -> dict[str, Any]:
    return {**value, "groups": sorted(value.get("groups", []), key=lambda item: (item["service"], item["date"]))}


def ci_for(workload: dict[str, Any], reference: str, candidates: list[str]) -> dict[str, Any]:
    result = {}
    for candidate in candidates:
        ratios = [a / b for a, b in zip(workload[candidate]["samples_seconds"], workload[reference]["samples_seconds"], strict=True)]
        result[candidate + "_over_" + reference] = paired_median_ci(ratios)
    return result


def peak_memory(
    command: list[str],
    cwd: Path,
    *,
    workload_id: str | None = None,
    expected: Any | None = None,
) -> int | None:
    if (workload_id is None) != (expected is None):
        raise ValueError("workload_id and expected output must be provided together")
    environment = None
    if Path(command[0]).name == "guion":
        environment = os.environ.copy()
        environment["GUION_FILE_ROOT"] = str(cwd.resolve())
    wrapper = (
        "import resource, subprocess, sys\n"
        "result = subprocess.run(sys.argv[1:], capture_output=True)\n"
        "sys.stdout.buffer.write(result.stdout)\n"
        "sys.stderr.buffer.write(result.stderr)\n"
        "usage = resource.getrusage(resource.RUSAGE_CHILDREN)\n"
        "sys.stderr.write(f'\\nGUION_PEAK_RSS={usage.ru_maxrss}\\n')\n"
        "raise SystemExit(result.returncode)\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", wrapper, *command],
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        if workload_id is not None:
            raise RuntimeError(
                f"memory sample command failed ({result.returncode}): {command!r}\n"
                f"{result.stdout}\n{result.stderr}"
            )
        return None
    if workload_id is not None:
        lane = "guion_source" if Path(command[0]).name == "guion" else "python_reference"
        actual = canonical_output(lane, result.stdout, workload_id)
        if not equivalent(actual, expected, workload_id):
            raise RuntimeError(f"memory sample output mismatch for {workload_id}: {actual!r} != {expected!r}")
    for line in result.stderr.splitlines():
        if line.startswith("GUION_PEAK_RSS="):
            try:
                return ru_maxrss_to_bytes(int(line.split("=", 1)[1]))
            except (ValueError, IndexError):
                return None
    return None


def richards_peak_memory(
    baseline_binary: Path,
    candidate_binary: Path,
    scratch: Path,
    *,
    baseline_revision: str,
    candidate_revision: str,
    runs: int = 5,
) -> dict[str, Any]:
    """Compare output-checked peak RSS for the two Guion builds in both Richards lanes."""
    baseline_binary = baseline_binary.resolve()
    candidate_binary = candidate_binary.resolve()
    scratch = scratch.resolve()
    if not baseline_binary.is_file() or not candidate_binary.is_file():
        raise FileNotFoundError("both Richards RSS comparison binaries must exist")
    if runs < 1:
        raise ValueError("RSS comparison runs must be positive")
    scratch.mkdir(parents=True, exist_ok=True)
    binaries = {"baseline": baseline_binary, "candidate": candidate_binary}
    binary_hashes = {
        name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()
    }
    expected = ["9297", "23246"]
    lane_configs = {
        "fresh_source": (1, expected),
        "repeated_source_25": (RICHARDS_PROFILE_REPETITIONS, expected * RICHARDS_PROFILE_REPETITIONS),
    }
    lane_reports: dict[str, Any] = {}
    with tempfile.TemporaryDirectory(prefix="richards-rss-", dir=scratch) as directory:
        project = Path(directory) / "project"
        project.mkdir(parents=True)
        source = project / "main.gu"
        shutil.copy2(WORKLOADS / "richards.vr", source)
        runner_source = source
        warm_source = project / "main-warm.gu"
        make_warm_source(runner_source, warm_source, RICHARDS_PROFILE_REPETITIONS)
        write_manifest(project, "richards-peak-rss", 30_000_000 * RICHARDS_PROFILE_REPETITIONS)
        for lane, (repetitions, lane_expected) in lane_configs.items():
            lane_source = source if repetitions == 1 else warm_source
            samples: dict[str, list[int]] = {}
            commands: dict[str, list[str]] = {}
            for name, binary in binaries.items():
                command = uncached_source_command([str(binary), "run", "--json", str(lane_source)])
                commands[name] = command
                values: list[int] = []
                for _ in range(runs):
                    peak = peak_memory(
                        command,
                        project,
                        workload_id="richards-scheduler",
                        expected=lane_expected,
                    )
                    if peak is None:
                        raise RuntimeError(f"peak RSS could not be read for {name} in {lane}")
                    values.append(peak)
                samples[name] = values
            baseline_median = statistics.median(samples["baseline"])
            candidate_median = statistics.median(samples["candidate"])
            delta_bytes = candidate_median - baseline_median
            delta_percent = (delta_bytes / baseline_median * 100) if baseline_median else None
            lane_reports[lane] = {
                "repetitions_per_process": repetitions,
                "commands": commands,
                "baseline": {
                    "samples_peak_rss_bytes": samples["baseline"],
                    "median_peak_rss_bytes": baseline_median,
                },
                "candidate": {
                    "samples_peak_rss_bytes": samples["candidate"],
                    "median_peak_rss_bytes": candidate_median,
                },
                "candidate_minus_baseline_bytes": delta_bytes,
                "candidate_minus_baseline_percent": delta_percent,
            }
    open_findings = [
        {
            "lane": lane,
            "candidate_over_baseline_percent": values["candidate_minus_baseline_percent"],
            "threshold_percent": 5.0,
            "finding": "candidate peak RSS exceeds baseline by more than 5%",
        }
        for lane, values in lane_reports.items()
        if values["candidate_minus_baseline_percent"] is not None
        and values["candidate_minus_baseline_percent"] > 5.0
    ]
    return {
        "status": "passed",
        "method": MEMORY_MEASUREMENT["method"],
        "runs_per_binary_per_lane": runs,
        "output_checked_each_run": True,
        "metric_scope": MEMORY_MEASUREMENT["single_process_scope"],
        "host": {"platform": platform.platform(), "architecture": platform.machine()},
        "baseline_revision": baseline_revision,
        "candidate_revision": candidate_revision,
        "baseline_binary_sha256": binary_hashes["baseline"],
        "candidate_binary_sha256": binary_hashes["candidate"],
        "lanes": lane_reports,
        "open_findings": open_findings,
    }


def sample_richards(
    binary: Path,
    scratch: Path,
    *,
    duration_seconds: int = 3,
    sampling_interval_ms: int = 1,
) -> dict[str, Any]:
    """Sample a profiled 25-execution Richards process with macOS's built-in sampler."""
    binary = binary.resolve()
    scratch = scratch.resolve()
    sample_tool = Path("/usr/bin/sample")
    if not binary.is_file():
        raise FileNotFoundError(f"release Guion binary does not exist: {binary}")
    if not sample_tool.is_file():
        raise FileNotFoundError("macOS /usr/bin/sample is unavailable")
    scratch.mkdir(parents=True, exist_ok=True)
    source_revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    source_hash = hashlib.sha256((WORKLOADS / "richards.vr").read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="richards-sample-", dir=scratch) as directory:
        project = Path(directory) / "project"
        project.mkdir(parents=True)
        source = project / "main.gu"
        shutil.copy2(WORKLOADS / "richards.vr", source)
        warm_source = project / "main-warm.gu"
        make_warm_source(source, warm_source, RICHARDS_PROFILE_REPETITIONS)
        write_manifest(project, "richards-sampled-attribution", 30_000_000 * RICHARDS_PROFILE_REPETITIONS)
        command = uncached_source_command([str(binary), "run", "--json", str(warm_source)])
        environment = os.environ.copy()
        environment["GUION_FILE_ROOT"] = str(project.resolve())
        environment["GUION_RUNTIME_PROFILE"] = "1"
        process = subprocess.Popen(
            command,
            cwd=project,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        sample_path = project / "sample-report.txt"
        sample_command = [
            str(sample_tool),
            str(process.pid),
            str(duration_seconds),
            str(sampling_interval_ms),
            "-mayDie",
            "-file",
            str(sample_path),
        ]
        sampler_returncode: int | None = None
        sampler_stdout = ""
        sampler_stderr = ""
        sampler_error: str | None = None
        try:
            sampled = subprocess.run(
                sample_command,
                cwd=project,
                capture_output=True,
                text=True,
                check=False,
                timeout=duration_seconds + 15,
            )
            sampler_returncode = sampled.returncode
            sampler_stdout = sampled.stdout
            sampler_stderr = sampled.stderr
        except subprocess.TimeoutExpired as error:
            sampler_error = f"sample timed out after {error.timeout} seconds"
        stdout, stderr = process.communicate(timeout=duration_seconds + 30)
        if process.returncode != 0:
            raise RuntimeError(f"sampled Richards program failed ({process.returncode}): {stdout}\n{stderr}")
        envelope = json.loads(stdout)
        if envelope.get("status") != "passed":
            raise RuntimeError(f"sampled Richards invocation reported failure: {envelope}")
        actual = canonical_output("guion_source", stdout, "richards-scheduler")
        expected = ["9297", "23246"] * RICHARDS_PROFILE_REPETITIONS
        if not equivalent(actual, expected, "richards-scheduler"):
            raise RuntimeError("sampled Richards process output did not match the 25-execution oracle")
        sample_report_text = sample_path.read_text(encoding="utf-8", errors="replace") if sample_path.is_file() else ""
    sample_report_bytes = sample_report_text.encode("utf-8")
    return {
        "status": "passed" if sampler_returncode == 0 and sample_report_text else "sample_unavailable",
        "source_revision": source_revision,
        "source_sha256": source_hash,
        "release_binary_sha256": binary_sha256,
        "program_repetitions": RICHARDS_PROFILE_REPETITIONS,
        "program_output_checked": True,
        "command": command,
        "sampler": {
            "command": [str(sample_tool), "<pid>", str(duration_seconds), str(sampling_interval_ms), "-mayDie", "-file", "<repo-local-temp>/sample-report.txt"],
            "returncode": sampler_returncode,
            "stdout": sampler_stdout,
            "stderr": sampler_stderr,
            "error": sampler_error,
            "sample_report_sha256": hashlib.sha256(sample_report_bytes).hexdigest() if sample_report_text else None,
            "sample_report_bytes": len(sample_report_bytes),
            "sample_report_text": sample_report_text,
        },
        "runtime_profile": envelope.get("runtime_profile"),
        "program_stderr": stderr,
    }


def parallel_worker_memory_report(command: list[str], cwd: Path) -> dict[str, Any]:
    """Run the process-pool workload separately and collect per-worker RSS peaks."""
    result = subprocess.run(
        [*command, "--report-memory"],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(f"parallel worker memory report failed: {result.stderr}")
    marker = "GUION_WORKER_MEMORY="
    line = next((line for line in reversed(result.stderr.splitlines()) if line.startswith(marker)), None)
    if line is None:
        raise RuntimeError("parallel worker memory report was not emitted")
    report = json.loads(line.removeprefix(marker))
    if not report.get("workers") or report.get("sum_peak_rss_bytes_upper_bound", 0) <= 0:
        raise RuntimeError("parallel worker memory report contains no positive worker peaks")
    return report


def size_bytes(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def save_progress(reports: dict[str, Any], output_path: Path | None = None) -> None:
    destination = output_path or ROOT / "evaluation/completion/results/performance-guion.json"
    path = destination.with_name(destination.stem + ".partial" + destination.suffix)
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = {
        **reports,
        "schema_version": 1,
        "kind": "guion-installed-performance-report",
        "status": "in_progress",
        "completed_workloads": list(reports["workloads"]),
    }
    temporary = path.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(partial, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def jsonl_scale_commands(binary: Path, project: Path) -> dict[str, list[str]]:
    source = project / "main.gu"
    artifact = project / "main.gub"
    reference_script = project / "data_reference.py"
    events = project / "performance-inputs" / "events.jsonl"
    services = project / "performance-inputs" / "services.txt"
    result_path = project / "performance-inputs" / "summary.json"
    args = ["--events", str(events.relative_to(project)), "--output", str(result_path.relative_to(project))]
    return {
        "guion_source": [str(binary), "run", "--json", str(source), *args],
        "guion_source_uncached": uncached_source_command([str(binary), "run", "--json", str(source), *args]),
        "guion_artifact": [str(binary), "run", "--json", str(artifact), *args],
        "python_data": [sys.executable, str(reference_script), "python", str(events), str(services), str(result_path)],
        "python_sqlite_native": [sys.executable, str(reference_script), "sqlite", str(events), str(services), str(result_path)],
    }


def load_resumable_suite_report(path: Path) -> dict[str, Any]:
    partial = json.loads(path.read_text(encoding="utf-8"))
    completed = set(partial.get("completed_workloads", []))
    workloads = partial.get("workloads", {})
    missing = set(RESUMABLE_BASELINE_WORKLOADS) - completed.intersection(workloads)
    if missing:
        raise ValueError(f"cannot resume from an incomplete benchmark report: {', '.join(sorted(missing))}")
    return {
        key: value
        for key, value in partial.items()
        if key not in {"schema_version", "kind", "status", "completed_workloads"}
    }


def measure_jsonl_scale(binary: Path, project: Path, rows: int) -> tuple[dict[str, Any], dict[str, int]]:
    workload_id = f"jsonl-summary-data-{rows}"
    generated = project / "performance-inputs"
    events, services, result_path = generated / "events.jsonl", generated / "services.txt", generated / "summary.json"
    reference_script = project / "data_reference.py"
    require_ok([sys.executable, str(GENERATOR), "--rows", str(rows), "--output", str(events), "--services", str(services)], project)
    py_output = generated / "expected.json"
    require_ok([sys.executable, str(reference_script), "python", str(events), str(services), str(py_output)], project)
    expected = normalize_data(json.loads(py_output.read_text(encoding="utf-8")))
    sqlite_output = generated / "expected-sqlite.json"
    require_ok([sys.executable, str(reference_script), "sqlite", str(events), str(services), str(sqlite_output)], project)
    if json.loads(sqlite_output.read_text(encoding="utf-8")) != expected:
        raise RuntimeError(f"Python dictionary and native SQLite data references disagree at {rows:,} rows")

    commands = jsonl_scale_commands(binary, project)
    results, meta = sample_candidates(
        workload_id,
        commands,
        project,
        expected,
        output_file=result_path,
        repetitions=1,
        trials=DATA_TRIALS,
    )
    comparisons = {
        "guion_source_over_python": ci_for(results, "python_data", ["guion_source"])["guion_source_over_python_data"],
        "guion_source_uncached_over_python": ci_for(results, "python_data", ["guion_source_uncached"])["guion_source_uncached_over_python_data"],
        "guion_artifact_over_python": ci_for(results, "python_data", ["guion_artifact"])["guion_artifact_over_python_data"],
        "guion_artifact_over_sqlite_native": ci_for(results, "python_sqlite_native", ["guion_artifact"])["guion_artifact_over_python_sqlite_native"],
    }
    profiles = {
        profile_name: profile_guion_run(command, project, workload_id, expected, output_file=result_path)
        for profile_name, command in (
            ("uncached_source", commands["guion_source_uncached"]),
            ("prebuilt_artifact", commands["guion_artifact"]),
        )
    }
    memory: dict[str, int] = {}
    for name, command in commands.items():
        result_path.unlink(missing_ok=True)
        observed = peak_memory(command, project)
        if observed is None:
            raise RuntimeError(f"peak RSS sample failed for {workload_id}/{name}")
        if not result_path.is_file() or normalize_data(json.loads(result_path.read_text(encoding="utf-8"))) != expected:
            raise RuntimeError(f"peak RSS sample output mismatch for {workload_id}/{name}")
        memory[name] = observed

    return (
        {
            **results,
            "comparison": comparisons,
            "rows": rows,
            "timed_trials_per_candidate": DATA_TRIALS,
            "repetitions_per_process": 1,
            "generator_seed": 20261025,
            "phase_profiles": profiles,
            "cache_policy": {
                "uncached_source": "--no-cache bypasses the Guion compilation cache; OS file cache uncontrolled",
                "cached_source": "fresh Guion process; source cache warmed by correctness-checked calls",
            },
            **meta,
        },
        memory,
    )


def finalize_suite_report(reports: dict[str, Any], binary: Path) -> dict[str, Any]:
    gil_enabled: bool | None = None
    probe = subprocess.run([sys.executable, "-c", "import sys; print(sys._is_gil_enabled())"], capture_output=True, text=True, check=False)
    if probe.returncode == 0:
        gil_enabled = probe.stdout.strip().lower() == "true"
    free_threaded = shutil.which("python3.14t") or shutil.which("python3t")
    if free_threaded is None:
        reports["free_threaded_python"] = {"available": False, "status": "unverified", "reason": "No python3.14t or python3t executable is installed; free-threaded concurrency comparison cannot be measured on this host."}
    else:
        probe = subprocess.run([free_threaded, "-c", "import sys; print(sys._is_gil_enabled())"], capture_output=True, text=True, check=False)
        reports["free_threaded_python"] = {"available": probe.returncode == 0 and probe.stdout.strip() == "False", "status": "verified" if probe.returncode == 0 and probe.stdout.strip() == "False" else "unverified", "executable": free_threaded}
    reports["python"] = {"version": sys.version, "executable": sys.executable, "pinned_version": PINNED_PYTHON, "matches_pinned": sys.version.startswith(PINNED_PYTHON), "gil_enabled": gil_enabled}
    reports["platform"] = {"platform": platform.platform(), "architecture": platform.machine(), "release_binary": str(binary), "release_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    reports["fixed_inputs"] = {"seed": TRIAL_SEED, "bootstrap_seed": BOOTSTRAP_SEED, "bootstrap_resamples": BOOTSTRAP_SAMPLES, "candidate_order": "seeded shuffle per paired trial", "compute_trials": TRIALS, "data_trials": DATA_TRIALS, "warmups": WARMUPS, "warm_repetitions": WARM_REPETITIONS}
    reports["resource_policy"] = {"instruction_limits_are_workload_specific": True, "warm_instruction_budget_multiplier": WARM_REPETITIONS, "allocation_limit_bytes": 8589934592, "reason": "same declared ceiling for each paired candidate; 200k JSONL scaling uses one workload pass per process to stay within the existing allocation ceiling; peak resident memory is measured separately"}
    release_directory = binary.parents[2]
    archive = release_directory / f"guion-{release_directory.name}.tar.gz"
    reports["package_size_bytes"] = size_bytes(binary.parent.parent)
    reports["package_archive_size_bytes"] = archive.stat().st_size if archive.is_file() else None
    reports["package_archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest() if archive.is_file() else None
    return reports


def run_suite(
    binary: Path,
    scratch: Path,
    output_path: Path | None = None,
    *,
    resume_from: Path | None = None,
) -> dict[str, Any]:
    if resume_from is not None:
        reports = load_resumable_suite_report(resume_from)
        project = scratch / "jsonl-summary-data"
        required_files = (project / "main.gu", project / "main.gub", project / "data_reference.py")
        if not all(path.is_file() for path in required_files):
            raise ValueError("cannot resume the 200k JSONL point because the prior workload workspace is incomplete")
        print("resuming at jsonl-summary-data-200000", file=sys.stderr, flush=True)
        workload, memory = measure_jsonl_scale(binary, project, 200000)
        reports["workloads"]["jsonl-summary-data-200000"] = workload
        reports["peak_memory_bytes"].update(
            {"jsonl-summary-data-200000/" + name: value for name, value in memory.items()}
        )
        save_progress(reports, output_path)
        return finalize_suite_report(reports, binary)

    reports: dict[str, Any] = {
        "workloads": {},
        "build_seconds": {},
        "artifact_bytes": {},
        "peak_memory_bytes": {},
        "memory_measurement": MEMORY_MEASUREMENT,
        "process_pool_worker_memory": {},
    }
    python_cases = {
        "richards-scheduler": ("richards.vr", "richards_reference.py", ["9297", "23246"], 30_000_000),
        "nbody-numeric": ("nbody.vr", "nbody.py", None, 300_000_000),
        "mandelbrot-image": ("mandelbrot.vr", "mandelbrot.py", ["155052", "1608"], 30_000_000),
    }
    for workload_id, (source_name, python_name, expected, instruction_limit) in python_cases.items():
        print(f"measuring {workload_id}", file=sys.stderr, flush=True)
        project = scratch / workload_id
        project.mkdir()
        source = project / "main.gu"
        reference = WORKLOADS / python_name
        shutil.copy2(WORKLOADS / source_name, source)
        write_manifest(project, workload_id, instruction_limit * WARM_REPETITIONS)
        artifact = project / "main.gub"
        build_seconds, _ = require_ok([str(binary), "build", str(source), "--output", str(artifact)], project)
        reports["build_seconds"][workload_id] = build_seconds
        reports["artifact_bytes"][workload_id] = artifact.stat().st_size
        warm_source = project / "warm.gu"
        make_warm_source(source, warm_source, WARM_REPETITIONS)
        warm_artifact = project / "warm.gub"
        warm_build_seconds, _ = require_ok([str(binary), "build", str(warm_source), "--output", str(warm_artifact)], project)
        reports["build_seconds"][workload_id + "_warm"] = warm_build_seconds
        python_warm = project / "warm_python.py"
        warm_python_source(reference, python_warm, "", 0, WARM_REPETITIONS)
        commands = {
            "guion_source": [str(binary), "run", "--json", str(source)],
            "guion_source_uncached": uncached_source_command([str(binary), "run", "--json", str(source)]),
            "guion_artifact": [str(binary), "run", "--json", str(artifact)],
            "python_reference": [sys.executable, str(reference)],
            "guion_warm_artifact": [str(binary), "run", "--json", str(warm_artifact)],
            "python_warm": [sys.executable, str(python_warm)],
        }
        if expected is None:
            _, reference_output = require_ok(commands["python_reference"], project)
            expected = [float(line) for line in reference_output.splitlines()]
        else:
            expected = expected
        comparisons = {
            "guion_source": ["guion_source", "python_reference"],
            "guion_source_uncached": ["guion_source_uncached", "python_reference"],
            "guion_artifact": ["guion_artifact", "python_reference"],
            "guion_warm_artifact": ["guion_warm_artifact", "python_warm"],
        }
        # Warm outputs contain five repetitions of the canonical workload.
        warm_expected = expected * WARM_REPETITIONS if workload_id != "nbody-numeric" else expected * WARM_REPETITIONS
        original_expected = expected
        results, meta = sample_candidates(workload_id, commands, project, original_expected, repetitions=WARM_REPETITIONS)
        phase_profiles = {
            profile_name: profile_guion_run(command, project, workload_id, original_expected)
            for profile_name, command in (
                ("uncached_source", commands["guion_source_uncached"]),
                ("prebuilt_artifact", commands["guion_artifact"]),
            )
        }
        reports["workloads"][workload_id] = {
            **results,
            "comparison": {key: ci_for(results, pair[1], [pair[0]])[pair[0] + "_over_" + pair[1]] for key, pair in comparisons.items()},
            "phase_profiles": phase_profiles,
            "cache_policy": {
                "uncached_source": "--no-cache bypasses the Guion compilation cache; OS file cache uncontrolled",
                "cached_source": "fresh Guion process; source cache warmed by prior correctness-checked calls",
            },
            **meta,
        }
        for name, command in commands.items():
            memory = peak_memory(command, project)
            if memory is not None:
                reports["peak_memory_bytes"][workload_id + "/" + name] = memory
        save_progress(reports, output_path)

    # Both established parallel sizes; Python serial/thread/process modes are measured independently.
    for count in (20_000, 200_000):
        workload_id = "parallel-cpu-" + str(count)
        print(f"measuring {workload_id} against serial, threads, and processes", file=sys.stderr, flush=True)
        project = scratch / workload_id
        project.mkdir()
        source_text = (WORKLOADS / "parallel_cpu.vr").read_text(encoding="utf-8").replace("20000", str(count))
        source = project / "main.gu"
        source.write_text(source_text, encoding="utf-8")
        py_source = project / "parallel_cpu.py"
        shutil.copy2(WORKLOADS / "parallel_cpu.py", py_source)
        write_manifest(project, workload_id, 1_000_000_000 * WARM_REPETITIONS)
        artifact = project / "main.gub"
        build_seconds, _ = require_ok([str(binary), "build", str(source), "--output", str(artifact)], project)
        reports["build_seconds"][workload_id] = build_seconds
        reports["artifact_bytes"][workload_id] = artifact.stat().st_size
        warm_source = project / "warm.gu"
        make_warm_source(source, warm_source, WARM_REPETITIONS)
        warm_artifact = project / "warm.gub"
        require_ok([str(binary), "build", str(warm_source), "--output", str(warm_artifact)], project)
        warm_python = project / "warm_parallel.py"
        warm_python_source(py_source, warm_python, "serial", count, WARM_REPETITIONS)
        total = sum(i % 97 for i in range(count))
        expected = [str(total)] * 4
        commands = {
            "guion_source": [str(binary), "run", "--json", str(source)],
            "guion_source_uncached": uncached_source_command([str(binary), "run", "--json", str(source)]),
            "guion_artifact": [str(binary), "run", "--json", str(artifact)],
            "python_serial": [sys.executable, str(py_source), "serial", str(count)],
            "python_threads": [sys.executable, str(py_source), "threads", str(count)],
            "python_processes": [sys.executable, str(py_source), "processes", str(count)],
            "guion_warm_artifact": [str(binary), "run", "--json", str(warm_artifact)],
            "python_warm": [sys.executable, str(warm_python)],
        }
        results, meta = sample_candidates(workload_id, commands, project, expected, repetitions=WARM_REPETITIONS)
        references = ["python_serial", "python_threads", "python_processes"]
        comparisons = {f"guion_artifact_over_{name}": ci_for(results, name, ["guion_artifact"])["guion_artifact_over_" + name] for name in references}
        comparisons["guion_source_over_python_serial"] = ci_for(results, "python_serial", ["guion_source"])["guion_source_over_python_serial"]
        comparisons["guion_source_uncached_over_python_serial"] = ci_for(results, "python_serial", ["guion_source_uncached"])["guion_source_uncached_over_python_serial"]
        comparisons["guion_warm_over_python_serial"] = ci_for(results, "python_warm", ["guion_warm_artifact"])["guion_warm_artifact_over_python_warm"]
        phase_profiles = {
            profile_name: profile_guion_run(command, project, workload_id, expected)
            for profile_name, command in (
                ("uncached_source", commands["guion_source_uncached"]),
                ("prebuilt_artifact", commands["guion_artifact"]),
            )
        }
        reports["workloads"][workload_id] = {
            **results,
            "comparison": comparisons,
            "phase_profiles": phase_profiles,
            "cache_policy": {
                "uncached_source": "--no-cache bypasses the Guion compilation cache; OS file cache uncontrolled",
                "cached_source": "fresh Guion process; source cache warmed by prior correctness-checked calls",
            },
            **meta,
        }
        for name, command in commands.items():
            memory = peak_memory(command, project)
            if memory is not None:
                reports["peak_memory_bytes"][workload_id + "/" + name] = memory
        reports["process_pool_worker_memory"][workload_id] = parallel_worker_memory_report(commands["python_processes"], project)
        save_progress(reports, output_path)

    # Real shipped Task 9 JSONL application with generated, validated input and independent baselines.
    workload_id = "jsonl-summary-data"
    print(f"measuring {workload_id} on generated JSONL with Python and SQLite baselines", file=sys.stderr, flush=True)
    project = scratch / workload_id
    shutil.copytree(APP, project)
    prepare_guion_application(project)
    project_manifest = project / "guion.toml"
    project_manifest.write_text(project_manifest.read_text(encoding="utf-8") + f"\n[runtime]\ninstruction_limit = {1000000000 * WARM_REPETITIONS}\nallocation_limit_bytes = 8589934592\n", encoding="utf-8")
    generated = project / "performance-inputs"
    events, services, result_path = generated / "events.jsonl", generated / "services.txt", generated / "summary.json"
    require_ok([sys.executable, str(GENERATOR), "--rows", "20000", "--output", str(events), "--services", str(services)], project)
    reference_script = project / "data_reference.py"
    shutil.copy2(DATA_REFERENCE, reference_script)
    py_output = project / "expected.json"
    require_ok([sys.executable, str(reference_script), "python", str(events), str(services), str(py_output)], project)
    expected = normalize_data(json.loads(py_output.read_text(encoding="utf-8")))
    sqlite_output = project / "expected-sqlite.json"
    require_ok([sys.executable, str(reference_script), "sqlite", str(events), str(services), str(sqlite_output)], project)
    if json.loads(sqlite_output.read_text(encoding="utf-8")) != expected:
        raise RuntimeError("Python dictionary and native SQLite data references disagree")
    source = project / "main.gu"
    artifact = project / "main.gub"
    build_seconds, _ = require_ok([str(binary), "build", str(source), "--output", str(artifact)], project)
    reports["build_seconds"][workload_id] = build_seconds
    reports["artifact_bytes"][workload_id] = artifact.stat().st_size
    warm_source = project / "warm.gu"
    make_warm_source(source, warm_source, WARM_REPETITIONS)
    warm_artifact = project / "warm.gub"
    require_ok([str(binary), "build", str(warm_source), "--output", str(warm_artifact)], project)
    warm_python = project / "warm_data.py"
    warm_python.write_text(
        "import json, sys\nfrom pathlib import Path\n"
        f"sys.path.insert(0, {str(project)!r})\nfrom data_reference import analyze_python\n"
        f"events = Path({str(events)!r})\nservices = Path({str(services)!r})\noutput = Path({str(result_path)!r})\n"
        f"for _ in range({WARM_REPETITIONS}):\n    value = analyze_python(events, services)\n    output.write_text(json.dumps(value, sort_keys=True) + '\\n', encoding='utf-8')\n",
        encoding="utf-8",
    )
    # The shipped allow-list branch is covered by Task 9 acceptance. This measured path
    # exercises the full stream/parse/group/write pipeline and uses all generated services.
    # Guion file effects are scoped to GUION_FILE_ROOT. Keep these arguments
    # relative to the workload directory, matching the shipped application's
    # relocated acceptance runs.
    args = ["--events", str(events.relative_to(project)), "--output", str(result_path.relative_to(project))]
    commands = {
        "guion_source": [str(binary), "run", "--json", str(source), *args],
        "guion_source_uncached": uncached_source_command([str(binary), "run", "--json", str(source), *args]),
        "guion_artifact": [str(binary), "run", "--json", str(artifact), *args],
        "python_data": [sys.executable, str(reference_script), "python", str(events), str(services), str(result_path)],
        "python_sqlite_native": [sys.executable, str(reference_script), "sqlite", str(events), str(services), str(result_path)],
        "guion_warm_artifact": [str(binary), "run", "--json", str(warm_artifact), *args],
        "python_warm": [sys.executable, str(warm_python)],
    }
    results, meta = sample_candidates(workload_id, commands, project, expected, output_file=result_path, repetitions=WARM_REPETITIONS, trials=DATA_TRIALS)
    # CSV/JSON output ordering is compared as parsed structures, so paths and map order do not bias timing.
    comparisons = {
        "guion_source_over_python": ci_for(results, "python_data", ["guion_source"])["guion_source_over_python_data"],
        "guion_source_uncached_over_python": ci_for(results, "python_data", ["guion_source_uncached"])["guion_source_uncached_over_python_data"],
        "guion_artifact_over_python": ci_for(results, "python_data", ["guion_artifact"])["guion_artifact_over_python_data"],
        "guion_artifact_over_sqlite_native": ci_for(results, "python_sqlite_native", ["guion_artifact"])["guion_artifact_over_python_sqlite_native"],
    }
    phase_profiles = {
        profile_name: profile_guion_run(command, project, workload_id, expected, output_file=result_path)
        for profile_name, command in (
            ("uncached_source", commands["guion_source_uncached"]),
            ("prebuilt_artifact", commands["guion_artifact"]),
        )
    }
    reports["workloads"][workload_id] = {
        **results,
        "comparison": comparisons,
        "rows": 20000,
        "timed_trials_per_candidate": DATA_TRIALS,
        "generator_seed": 20261025,
        "phase_profiles": phase_profiles,
        "cache_policy": {
            "uncached_source": "--no-cache bypasses the Guion compilation cache; OS file cache uncontrolled",
            "cached_source": "fresh Guion process; source cache warmed by prior correctness-checked calls",
        },
        **meta,
    }
    for name, command in commands.items():
        memory = peak_memory(command, project)
        if memory is not None:
            reports["peak_memory_bytes"][workload_id + "/" + name] = memory
    save_progress(reports, output_path)

    # Add a larger data point with single-execution lanes and the existing allocation ceiling.
    workload_id = "jsonl-summary-data-200000"
    print(f"measuring {workload_id} on generated JSONL with Python and SQLite baselines", file=sys.stderr, flush=True)
    scale_workload, scale_memory = measure_jsonl_scale(binary, project, 200000)
    reports["workloads"][workload_id] = scale_workload
    reports["peak_memory_bytes"].update({workload_id + "/" + name: value for name, value in scale_memory.items()})
    save_progress(reports, output_path)

    return finalize_suite_report(reports, binary)


def required_speed_wins(report: dict[str, Any]) -> dict[str, Any]:
    workloads = report["workloads"]
    comparisons: dict[str, dict[str, bool]] = {}
    for workload_id in ["richards-scheduler", "nbody-numeric", "mandelbrot-image"]:
        comparisons[workload_id] = {
            "python_reference": workloads[workload_id]["comparison"]["guion_artifact"]["win"]
        }
    parallel = workloads["parallel-cpu-200000"]["comparison"]
    comparisons["parallel-cpu-200000"] = {
        "python_serial": parallel["guion_artifact_over_python_serial"]["win"],
        "python_threads": parallel["guion_artifact_over_python_threads"]["win"],
        "python_processes": parallel["guion_artifact_over_python_processes"]["win"],
    }
    data = workloads["jsonl-summary-data"]["comparison"]
    comparisons["jsonl-summary-data"] = {
        "python_data": data["guion_artifact_over_python"]["win"]
    }
    wins = [won for workload in comparisons.values() for won in workload.values()]
    return {
        "all_required_wins_observed": all(wins),
        "required_workloads": list(comparisons),
        "comparisons": comparisons,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installed", action="store_true", required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--scratch-dir", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    binary = (args.binary or Path(os.environ.get("GUION_BINARY", ROOT / "target/completion/releases/macos-aarch64/current/bin/guion"))).resolve()
    if not binary.is_file():
        raise SystemExit(f"installed release binary does not exist: {binary}")

    def execute_report(scratch: Path) -> tuple[dict[str, Any], Path]:
        try:
            report = run_suite(binary, scratch, args.output)
        except Exception as error:
            destination = args.output or ROOT / "evaluation/completion/results/performance-guion.json"
            partial_path = destination.with_name(destination.stem + ".partial" + destination.suffix)
            if partial_path.is_file():
                report = json.loads(partial_path.read_text(encoding="utf-8"))
            else:
                report = {"python": {"version": sys.version}, "platform": platform.platform()}
            report.update({"status": "failed", "error": str(error)})
        else:
            speed = required_speed_wins(report)
            report["status"] = "passed" if report["python"]["matches_pinned"] and speed["all_required_wins_observed"] else "failed"
            report["required_speed_wins"] = speed
        report["schema_version"] = 1
        report["kind"] = "guion-installed-performance-report"
        path = args.output or ROOT / "evaluation/completion/results/performance-guion.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        path.with_name(path.stem + ".partial" + path.suffix).unlink(missing_ok=True)
        return report, path

    if args.scratch_dir:
        scratch = args.scratch_dir.resolve()
        scratch.mkdir(parents=True, exist_ok=True)
        report, path = execute_report(scratch)
    else:
        with tempfile.TemporaryDirectory(prefix="guion-performance-") as directory:
            report, path = execute_report(Path(directory))
    print(json.dumps({"status": report.get("status"), "report_path": str(path), "workloads": len(report.get("workloads", {}))}))
    return 0 if report.get("status") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
