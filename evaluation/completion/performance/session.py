#!/usr/bin/env python3
"""Repository-local, reusable source-run performance session."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

import runner


ROOT = Path(__file__).resolve().parents[3]
SCRATCH = ROOT / "target/perf-session"
REPORTS = ROOT / "evaluation/completion/results/source-loop"
PINNED_PYTHON = "3.14.7"
MIN_FREE_BYTES = 2 * 1024**3
TRIALS = 7
WARMUPS = 2
TRIAL_SEED = 20261027
BUILD_MANIFEST = "build-identity.json"
ACTIVE_LOCK = "active-run.lock"
FROZEN_REPORT = ROOT / "evaluation/completion/results/guion-final/performance-guion.json"


class PreflightError(RuntimeError):
    """A preflight condition that must be resolved before work can run."""


def json_load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def _inside(root: Path, path: Path) -> Path:
    root = root.resolve()
    candidate = path.resolve(strict=False)
    try:
        candidate.relative_to(root)
    except ValueError as error:
        raise PreflightError(f"path must stay inside the repository: {path}") from error
    return candidate


def _python_version() -> str:
    result = subprocess.run([sys.executable, "-V"], capture_output=True, text=True, check=False)
    if result.returncode:
        raise PreflightError(f"could not identify Python: {result.stderr.strip()}")
    return result.stdout.strip().removeprefix("Python ") or result.stderr.strip().removeprefix("Python ")


def _offline_dependencies_available(root: Path) -> bool:
    result = subprocess.run(["cargo", "fetch", "--locked", "--offline"], cwd=root, capture_output=True, text=True, check=False)
    return result.returncode == 0


def preflight(
    *,
    root: Path = ROOT,
    cwd: Path | None = None,
    scratch: Path | None = None,
    python_version: str | None = None,
    free_bytes: int | None = None,
    offline_ok: bool | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    cwd = (cwd or Path.cwd()).resolve()
    scratch = _inside(root, scratch or (root / "target/perf-session"))
    if cwd != root:
        raise PreflightError(f"run from the repository root: {root}")
    version = python_version if python_version is not None else _python_version()
    if version != PINNED_PYTHON:
        raise PreflightError(f"Python {PINNED_PYTHON} is required; found {version}")
    if free_bytes is None:
        free_bytes = shutil.disk_usage(root).free
    if free_bytes < MIN_FREE_BYTES:
        raise PreflightError(f"at least {MIN_FREE_BYTES} bytes of free disk are required; found {free_bytes}")
    if offline_ok is None:
        offline_ok = _offline_dependencies_available(root)
    if not offline_ok:
        raise PreflightError("locked Cargo dependencies are not all available offline")
    if not (root / "Cargo.toml").is_file() or not (root / "Cargo.lock").is_file():
        raise PreflightError("repository root must contain Cargo.toml and Cargo.lock")
    if scratch.exists() and not scratch.is_dir():
        raise PreflightError(f"repo-local scratch is not an accessible directory: {scratch}")
    try:
        scratch.mkdir(parents=True, exist_ok=True)
        probe = scratch / ".preflight-write-check"
        probe.write_text("ok", encoding="utf-8")
        probe.unlink()
    except OSError as error:
        raise PreflightError(f"repo-local scratch is not writable: {scratch}: {error}") from error
    return {
        "repository_root": str(root),
        "working_directory": str(cwd),
        "scratch_directory": str(scratch),
        "python_version": version,
        "cargo_version": _tool_version("cargo"),
        "rustc_version": _tool_version("rustc"),
        "architecture": platform.machine(),
        "platform": platform.platform(),
        "free_disk_bytes": free_bytes,
        "offline_dependencies_available": True,
        "target": f"{platform.system().lower()}-{platform.machine().lower()}",
    }


def _tool_version(tool: str) -> str:
    result = subprocess.run([tool, "-V"], capture_output=True, text=True, check=False)
    if result.returncode:
        raise PreflightError(f"could not identify {tool}: {result.stderr.strip()}")
    return result.stdout.strip()


def _source_fingerprint(root: Path) -> str:
    digest = hashlib.sha256()
    inputs: list[Path] = []
    for name in ("Cargo.toml", "Cargo.lock", "build.rs"):
        path = root / name
        if path.is_file():
            inputs.append(path)
    for folder in ("crates", "language", "stdlib", "xtask"):
        path = root / folder
        if path.is_dir():
            inputs.extend(item for item in path.rglob("*") if item.is_file())
    for path in sorted(set(inputs)):
        relative_path = path.relative_to(root)
        if any(part in {"target", ".git", "__pycache__"} for part in relative_path.parts):
            continue
        relative = relative_path.as_posix().encode()
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        content = path.read_bytes()
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return digest.hexdigest()


def source_revision(root: Path) -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=False)
    if result.returncode:
        raise PreflightError(f"could not identify Git source revision: {result.stderr.strip()}")
    return result.stdout.strip()


def build_release_cli(root: Path, scratch: Path) -> Path:
    target_dir = _inside(root, scratch / "cargo")
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    result = subprocess.run(
        ["cargo", "build", "--release", "--locked", "--offline", "-p", "guion-cli"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(f"release build failed:\n{result.stdout}\n{result.stderr}")
    binary = target_dir / "release" / ("guion.exe" if os.name == "nt" else "guion")
    if not binary.is_file():
        raise RuntimeError(f"release build did not create the CLI: {binary}")
    return binary


def ensure_cli(
    root: Path,
    scratch: Path,
    revision: str | None = None,
    build_func: Callable[[Path, Path], Path] | None = None,
) -> tuple[Path, bool]:
    root = root.resolve()
    scratch = _inside(root, scratch)
    revision = revision or source_revision(root)
    identity = {
        "source_revision": revision,
        "source_fingerprint": _source_fingerprint(root),
        "cargo_version": _tool_version("cargo"),
        "rustc_version": _tool_version("rustc"),
        "target": f"{platform.system().lower()}-{platform.machine().lower()}",
    }
    manifest_path = scratch / BUILD_MANIFEST
    if manifest_path.is_file():
        try:
            manifest = json_load(manifest_path)
            binary = _inside(root, scratch / manifest["binary_relative_path"])
            if manifest.get("identity") == identity and binary.is_file() and hashlib.sha256(binary.read_bytes()).hexdigest() == manifest.get("binary_sha256"):
                return binary, True
        except (KeyError, OSError, ValueError, json.JSONDecodeError, PreflightError):
            pass
    binary = (build_func or build_release_cli)(root, scratch)
    binary = _inside(root, binary)
    if not binary.is_file():
        raise RuntimeError(f"release builder returned a missing binary: {binary}")
    payload = {
        "identity": identity,
        "binary_relative_path": binary.relative_to(scratch).as_posix(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "built_at_utc": datetime.now(timezone.utc).isoformat(),
    }
    _atomic_json(manifest_path, payload)
    return binary, False


def _atomic_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def save_report(reports: Path, payload: dict[str, Any], run_id: str | None = None) -> Path:
    reports.mkdir(parents=True, exist_ok=True)
    if run_id is None:
        run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    safe_id = "".join(character for character in run_id if character.isalnum() or character in "-_") or "run"
    candidate = reports / f"{safe_id}.json"
    suffix = 2
    while candidate.exists() or candidate.resolve() == FROZEN_REPORT.resolve():
        candidate = reports / f"{safe_id}-{suffix:02d}.json"
        suffix += 1
    with candidate.open("x", encoding="utf-8") as output:
        output.write(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return candidate


def _summary(samples: list[float]) -> dict[str, Any]:
    ordered = sorted(samples)
    return {
        "samples_seconds": samples,
        "median_seconds": statistics.median(samples),
        "p95_seconds_nearest_rank": ordered[max(0, int(0.95 * len(ordered) + 0.999999) - 1)],
    }


def _measure_launch(binary: Path) -> dict[str, Any]:
    samples: list[float] = []
    expected: str | None = None
    for _ in range(TRIALS):
        elapsed, result = runner.execute([str(binary), "--version"], ROOT)
        if result.returncode or not result.stdout.strip():
            raise RuntimeError(f"bare CLI launch failed: {result.stderr}")
        if expected is None:
            expected = result.stdout.strip()
        elif result.stdout.strip() != expected:
            raise RuntimeError("bare CLI version output changed between launches")
        samples.append(elapsed)
    return {**_summary(samples), "command": [str(binary), "--version"], "verified_stdout": expected}


def _python_app_script(kind: str) -> str:
    if kind == "file-data":
        return "import json\nfrom pathlib import Path\nprint(len(json.loads(Path('records.json').read_text())))\n"
    return (
        "import sqlite3\n"
        "db = sqlite3.connect('agent-recipes.sqlite3')\n"
        "db.execute('CREATE TABLE IF NOT EXISTS runs (name TEXT NOT NULL)')\n"
        "db.execute('INSERT INTO runs (name) VALUES (?)', ('Guion agents',))\n"
        "print(db.execute('SELECT name FROM runs ORDER BY rowid DESC LIMIT 1').fetchone()[0])\n"
        "db.commit()\n"
    )


def _checked_sample(name: str, command: list[str], project: Path, workload: str, expected: Any) -> tuple[float, str, str | None]:
    cache_status: str | None = None
    if name.startswith("guion_"):
        elapsed, output, envelope = runner.guion_result(command, project)
        if envelope:
            status = envelope.get("source_cache")
            cache_status = status if isinstance(status, str) else None
    else:
        elapsed, result = runner.execute(command, project)
        if result.returncode:
            raise RuntimeError(f"{name} failed ({result.returncode}): {result.stderr}")
        output = result.stdout
    actual = runner.canonical_output(name, output, workload)
    if not runner.equivalent(actual, expected, workload):
        raise RuntimeError(f"{name} output mismatch for {workload}: {actual!r} != {expected!r}")
    return elapsed, output, cache_status


def _measure_source_lanes(binary: Path, scratch: Path) -> dict[str, Any]:
    workload_files = {
        "richards-scheduler": ("richards.vr", "richards_reference.py", ["9297", "23246"], 30_000_000),
        "nbody-numeric": ("nbody.vr", "nbody.py", None, 300_000_000),
        "mandelbrot-image": ("mandelbrot.vr", "mandelbrot.py", ["155052", "1608"], 30_000_000),
    }
    run_help = subprocess.run([str(binary), "run", "--help"], capture_output=True, text=True, check=False)
    no_cache_available = run_help.returncode == 0 and "--no-cache" in run_help.stdout + run_help.stderr
    reports: dict[str, Any] = {}
    for workload, (source_name, python_name, expected_lines, instruction_limit) in workload_files.items():
        project = scratch / "source-loop" / workload
        project.mkdir(parents=True)
        source = project / "main.gu"
        shutil.copy2(runner.WORKLOADS / source_name, source)
        python_source = runner.WORKLOADS / python_name
        runner.write_manifest(project, workload, instruction_limit)
        artifact = project / "main.gub"
        runner.require_ok([str(binary), "build", str(source), "--output", str(artifact)], project)
        if expected_lines is None:
            _, raw = runner.require_ok([sys.executable, str(python_source)], project)
            expected: Any = [float(line) for line in raw.splitlines()]
        else:
            expected = expected_lines

        cold_command = [str(binary), "run", "--json"]
        if no_cache_available:
            cold_command.append("--no-cache")
        cold_command.append(str(source))
        cold_seconds, _, cold_cache_status = _checked_sample("guion_source", cold_command, project, workload, expected)
        first_python_seconds, _, _ = _checked_sample("python_reference", [sys.executable, str(python_source)], project, workload, expected)

        commands = {
            "guion_source": [str(binary), "run", "--json", str(source)],
            "guion_artifact": [str(binary), "run", "--json", str(artifact)],
            "python_reference": [sys.executable, str(python_source)],
        }
        for name, command in commands.items():
            _checked_sample(name, command, project, workload, expected)
        rng = random.Random(TRIAL_SEED)
        samples = {name: [] for name in commands}
        cache_states = {name: [] for name in commands}
        orders: list[list[str]] = []
        for _ in range(TRIALS):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for name in order:
                elapsed, _, cache_status = _checked_sample(name, commands[name], project, workload, expected)
                samples[name].append(elapsed)
                if cache_status is not None:
                    cache_states[name].append(cache_status)
        phase_profiles: dict[str, Any] = {}
        for profile_name, profile_command, profile_expected, profile_project in [
            ("uncached_source", cold_command, expected, project),
            ("prebuilt_artifact", [str(binary), "run", "--json", str(artifact)], expected, project),
        ]:
            elapsed, output, envelope = runner.guion_result(
                profile_command,
                profile_project,
                extra_env={"GUION_PHASE_TIMINGS": "1"},
            )
            actual = runner.canonical_output("guion_source", output, workload)
            if not runner.equivalent(actual, profile_expected, workload):
                raise RuntimeError(f"profiled {workload} output mismatch: {actual!r} != {profile_expected!r}")
            phase_profiles[profile_name] = {
                "elapsed_seconds_including_process_startup": elapsed,
                "cache_status": envelope.get("source_cache") if envelope else None,
                "reported_phase_timings_ns": envelope.get("phase_timings_ns") if envelope else None,
            }
        reports[workload] = {
            "cold_source_first_run": {"seconds": cold_seconds, "command": cold_command, "cache_bypassed": no_cache_available, "cache_status": cold_cache_status},
            "python_first_timed_run": {"seconds": first_python_seconds, "command": commands["python_reference"]},
            "repeat_and_artifact_lanes": {name: _summary(values) for name, values in samples.items()},
            "cache_status_by_lane": cache_states,
            "phase_profiles": phase_profiles,
            "paired_trial_order": orders,
            "cache_state": {
                "source_cache_available": no_cache_available,
                "source_repeat": "cache hit after first run" if no_cache_available else "compiler rerun each process",
                "prebuilt_artifact": "artifact load each process",
                "python_process": "fresh process each sample; OS file cache uncontrolled",
            },
        }
    return reports


def _measure_agent_examples(binary: Path, scratch: Path) -> dict[str, Any]:
    cases = {
        "file-data": ("file_data_cli.gu", ["records.json"], ["3"]),
        "sqlite-toml": ("sqlite_toml.gu", [], ["Guion agents"]),
    }
    outputs: dict[str, Any] = {}
    examples = ROOT / "docs/agents/examples"
    for name, (source_name, arguments, expected) in cases.items():
        project = scratch / "agent-examples" / name
        project.mkdir(parents=True)
        source = project / "main.gu"
        shutil.copy2(examples / source_name, source)
        runner.write_manifest(project, "source-loop-" + name, 10_000_000)
        if name == "file-data":
            shutil.copy2(examples / "records.json", project / "records.json")
        else:
            shutil.copy2(examples / "settings.toml", project / "settings.toml")
        artifact = project / "main.gub"
        runner.require_ok([str(binary), "build", str(source), "--output", str(artifact)], project)
        python_source = project / "reference.py"
        python_source.write_text(_python_app_script(name), encoding="utf-8")
        commands = {
            "guion_source": [str(binary), "run", "--json", str(source), *arguments],
            "guion_artifact": [str(binary), "run", "--json", str(artifact), *arguments],
            "python_reference": [sys.executable, str(python_source)],
        }
        samples, metadata = runner.sample_candidates(name, commands, project, expected, trials=TRIALS)
        peak_memory_bytes = {
            lane: runner.peak_memory(commands[lane], project)
            for lane in commands
        }
        outputs[name] = {
            "expected_output": expected,
            "cache_state": metadata.get("source_cache_states", {}),
            "peak_memory_bytes": peak_memory_bytes,
            "source_cache_bytes": runner.size_bytes(project / ".guion" / "cache"),
            "workloads": samples,
            **metadata,
        }
    return outputs


def run_benchmarks(binary: Path, scratch: Path) -> dict[str, Any]:
    source_lanes = _measure_source_lanes(binary, scratch)
    tiny_project = scratch / "source-loop" / "tiny-launch"
    tiny_project.mkdir(parents=True)
    tiny_source = tiny_project / "main.gu"
    tiny_source.write_text("fn main() -> Int:\n    print(\"tiny\")\n    return 0\n", encoding="utf-8")
    runner.write_manifest(tiny_project, "tiny-launch", 100_000)
    tiny_elapsed, tiny_output, tiny_envelope = runner.guion_result(
        [str(binary), "run", "--json", "--no-cache", str(tiny_source)],
        tiny_project,
        extra_env={"GUION_PHASE_TIMINGS": "1"},
    )
    if runner.canonical_output("guion_source", tiny_output, "tiny-launch") != ["tiny"]:
        raise RuntimeError("profiled tiny source program produced unexpected output")
    tiny_peak_memory = runner.peak_memory(
        [str(binary), "run", "--json", "--no-cache", str(tiny_source)],
        tiny_project,
    )
    suite_scratch = scratch / "existing-suite"
    suite_scratch.mkdir(parents=True, exist_ok=True)
    suite = runner.run_suite(binary, suite_scratch, output_path=scratch / "existing-suite.json")
    return {
        "source_run_lanes": source_lanes,
        "existing_correctness_checked_suite": suite,
        "tiny_cli_launch": _measure_launch(binary),
        "tiny_source_phase_profile": {
            "elapsed_seconds_including_process_startup": tiny_elapsed,
            "peak_memory_bytes": tiny_peak_memory,
            "cache_status": tiny_envelope.get("source_cache") if tiny_envelope else None,
            "reported_phase_timings_ns": tiny_envelope.get("phase_timings_ns") if tiny_envelope else None,
        },
        "agent_application_cases": _measure_agent_examples(binary, scratch),
        "cache_state": "cache implementation enabled; per-run states are recorded by workload",
    }


def _pid_is_running(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def clean(root: Path = ROOT, scratch: Path = SCRATCH) -> str:
    root = root.resolve()
    scratch = _inside(root, scratch)
    lock = scratch / ACTIVE_LOCK
    if lock.is_file():
        try:
            pid = int(lock.read_text(encoding="utf-8").strip())
        except (OSError, ValueError):
            raise RuntimeError(f"cannot confirm the owner of active session lock: {lock}")
        if _pid_is_running(pid):
            raise RuntimeError(f"performance session process {pid} is still running; scratch was preserved")
    if scratch.exists():
        shutil.rmtree(scratch)
    return str(scratch)


def baseline(
    *,
    root: Path = ROOT,
    cwd: Path | None = None,
    scratch: Path = SCRATCH,
    python_version: str | None = None,
    free_bytes: int | None = None,
    offline_ok: bool | None = None,
    report_dir: Path | None = None,
    run_id: str | None = None,
) -> Path:
    root = root.resolve()
    scratch = _inside(root, scratch)
    report_dir = _inside(root, report_dir or (root / "evaluation/completion/results/source-loop"))
    check = preflight(root=root, cwd=cwd, scratch=scratch, python_version=python_version, free_bytes=free_bytes, offline_ok=offline_ok)
    lock = scratch / ACTIVE_LOCK
    lock_created = False
    try:
        with lock.open("x", encoding="utf-8") as owner:
            lock_created = True
            owner.write(str(os.getpid()))
        requested_run_id = run_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
        safe_run_id = "".join(character for character in requested_run_id if character.isalnum() or character in "-_") or "run"
        run_scratch = scratch / "runs" / safe_run_id
        suffix = 2
        while run_scratch.exists():
            run_scratch = scratch / "runs" / f"{safe_run_id}-{suffix:02d}"
            suffix += 1
        run_scratch.mkdir(parents=True)
        report_id = run_scratch.name
        started = time.perf_counter()
        revision = source_revision(root)
        binary, reused = ensure_cli(root, scratch, revision)
        commands = [
            ["cargo", "build", "--release", "--locked", "--offline", "-p", "guion-cli"],
            [str(binary), "--version"],
            ["python3", str(root / "evaluation/completion/performance/runner.py"), "--installed", "--binary", str(binary)],
        ]
        outcomes = run_benchmarks(binary, run_scratch)
        manifest = {
            **check,
            "source_revision": revision,
            "source_fingerprint": _source_fingerprint(root),
            "release_binary": str(binary),
            "release_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "build_reused": reused,
            "target": {"os": platform.system(), "architecture": platform.machine()},
            "scratch_size_bytes": _tree_size(scratch),
            "commands": commands,
            "outcomes": {"status": "passed", "benchmark_groups": sorted(outcomes)},
            "duration_seconds": time.perf_counter() - started,
        }
        report = {"schema_version": 1, "kind": "guion-source-loop-performance-report", "status": "passed", "created_at_utc": datetime.now(timezone.utc).isoformat(), "manifest": manifest, **outcomes}
        return save_report(report_dir, report, report_id)
    finally:
        if lock_created:
            lock.unlink(missing_ok=True)


def _tree_size(path: Path) -> int:
    if not path.exists():
        return 0
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def compare(before: Path, after: Path) -> dict[str, Any]:
    left, right = json_load(before), json_load(after)
    result: dict[str, Any] = {"before": str(before), "after": str(after), "comparisons": {}}

    def record(name: str, old: float, new: float) -> None:
        result["comparisons"][name] = {
            "before_seconds": old,
            "after_seconds": new,
            "delta_seconds": new - old,
            "ratio": new / old if old else None,
        }

    for metric in ("median_seconds", "p95_seconds_nearest_rank"):
        if metric in left.get("tiny_cli_launch", {}) and metric in right.get("tiny_cli_launch", {}):
            record(f"tiny_cli_launch/{metric}", left["tiny_cli_launch"][metric], right["tiny_cli_launch"][metric])

    left_source = left.get("source_run_lanes", {})
    right_source = right.get("source_run_lanes", {})
    for workload, lanes in right_source.items():
        previous = left_source.get(workload, {})
        for lane in ("cold_source_first_run", "python_first_timed_run"):
            if lane in lanes and lane in previous:
                record(f"source/{workload}/{lane}/seconds", previous[lane]["seconds"], lanes[lane]["seconds"])
        old_repeats = previous.get("repeat_and_artifact_lanes", {})
        for candidate, metrics in lanes.get("repeat_and_artifact_lanes", {}).items():
            for metric in ("median_seconds", "p95_seconds_nearest_rank"):
                if candidate in old_repeats and metric in metrics and metric in old_repeats[candidate]:
                    record(f"source/{workload}/{candidate}/{metric}", old_repeats[candidate][metric], metrics[metric])

    left_apps = left.get("agent_application_cases", {})
    for application, app in right.get("agent_application_cases", {}).items():
        old_app = left_apps.get(application, {})
        old_lanes, new_lanes = old_app.get("workloads", {}), app.get("workloads", {})
        for candidate, metrics in new_lanes.items():
            for metric in ("median_seconds", "p95_seconds_nearest_rank"):
                if candidate in old_lanes and metric in metrics and metric in old_lanes[candidate]:
                    record(f"apps/{application}/{candidate}/{metric}", old_lanes[candidate][metric], metrics[metric])

    for workload, metrics in right.get("existing_correctness_checked_suite", {}).get("workloads", {}).items():
        old_metrics = left.get("existing_correctness_checked_suite", {}).get("workloads", {}).get(workload, {})
        for lane in ("guion_source", "guion_artifact", "python_reference", "python_warm"):
            if lane in metrics and lane in old_metrics:
                old = old_metrics[lane]["median_seconds"]
                new = metrics[lane]["median_seconds"]
                record(f"existing-suite/{workload}/{lane}/median_seconds", old, new)
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("preflight")
    subparsers.add_parser("baseline")
    compare_parser = subparsers.add_parser("compare")
    compare_parser.add_argument("before", type=Path)
    compare_parser.add_argument("after", type=Path)
    subparsers.add_parser("clean")
    args = parser.parse_args(argv)
    try:
        if args.command == "preflight":
            result = preflight()
        elif args.command == "baseline":
            path = baseline()
            result = {"report_path": str(path), "status": "passed"}
        elif args.command == "compare":
            result = compare(args.before, args.after)
        else:
            result = {"cleaned": clean()}
    except Exception as error:
        print(json.dumps({"status": "failed", "error": str(error)}), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
