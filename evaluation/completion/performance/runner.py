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
TRIAL_SEED = 20261025
BOOTSTRAP_SEED = 20261026
BOOTSTRAP_SAMPLES = 20_000
PINNED_PYTHON = "3.14.7"


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
            wanted = expected * WARM_REPETITIONS if "warm" in name else expected
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


def peak_memory(command: list[str], cwd: Path) -> int | None:
    scale = 1 if platform.system() == "Darwin" else 1024
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
        return None
    for line in result.stderr.splitlines():
        if line.startswith("GUION_PEAK_RSS="):
            try:
                return int(line.split("=", 1)[1]) * scale
            except (ValueError, IndexError):
                return None
    return None


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


def run_suite(binary: Path, scratch: Path, output_path: Path | None = None) -> dict[str, Any]:
    reports: dict[str, Any] = {"workloads": {}, "build_seconds": {}, "artifact_bytes": {}, "peak_memory_bytes": {}}
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
            "guion_artifact": ["guion_artifact", "python_reference"],
            "guion_warm_artifact": ["guion_warm_artifact", "python_warm"],
        }
        # Warm outputs contain five repetitions of the canonical workload.
        warm_expected = expected * WARM_REPETITIONS if workload_id != "nbody-numeric" else expected * WARM_REPETITIONS
        original_expected = expected
        results, meta = sample_candidates(workload_id, commands, project, original_expected, repetitions=WARM_REPETITIONS)
        reports["workloads"][workload_id] = {**results, "comparison": {key: ci_for(results, pair[1], [pair[0]])[pair[0] + "_over_" + pair[1]] for key, pair in comparisons.items()}, **meta}
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
        comparisons["guion_warm_over_python_serial"] = ci_for(results, "python_warm", ["guion_warm_artifact"])["guion_warm_artifact_over_python_warm"]
        reports["workloads"][workload_id] = {**results, "comparison": comparisons, **meta}
        for name, command in commands.items():
            memory = peak_memory(command, project)
            if memory is not None:
                reports["peak_memory_bytes"][workload_id + "/" + name] = memory
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
        "guion_artifact_over_python": ci_for(results, "python_data", ["guion_artifact"])["guion_artifact_over_python_data"],
        "guion_artifact_over_sqlite_native": ci_for(results, "python_sqlite_native", ["guion_artifact"])["guion_artifact_over_python_sqlite_native"],
    }
    reports["workloads"][workload_id] = {**results, "comparison": comparisons, "rows": 20000, "timed_trials_per_candidate": DATA_TRIALS, "generator_seed": 20261025, **meta}
    reports["peak_memory_bytes"][workload_id + "/guion_artifact"] = peak_memory(commands["guion_artifact"], project)
    save_progress(reports, output_path)

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
    reports["resource_policy"] = {"instruction_limits_are_workload_specific": True, "warm_instruction_budget_multiplier": WARM_REPETITIONS, "allocation_limit_bytes": 8589934592, "reason": "same declared ceiling for each paired candidate and repeated in-process warm run; peak resident memory is measured separately"}
    release_directory = binary.parents[2]
    archive = release_directory / f"guion-{release_directory.name}.tar.gz"
    reports["package_size_bytes"] = size_bytes(binary.parent.parent)
    reports["package_archive_size_bytes"] = archive.stat().st_size if archive.is_file() else None
    reports["package_archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest() if archive.is_file() else None
    return reports


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
