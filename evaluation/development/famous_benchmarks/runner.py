#!/usr/bin/env python3
"""Run the Mandelbrot workload in isolated scratch space and record timings."""

from __future__ import annotations

import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
WORKLOAD = Path(__file__).resolve().parent
VERITAS = Path(os.environ.get("VERITAS_BINARY", ROOT / "target/release/veritas")).resolve()
RESULT = ROOT / "evaluation/results/native-backend-probe-cranelift-0.135.0.json"
UPSTREAM_RESULT = ROOT / "evaluation/results/pyperformance-1.14.0-source-probes.json"
EXPECTED = "155052\n1608\n"
TRIALS = 15
WARMUPS = 2


def load_pyperformance_source_report() -> dict[str, object]:
    if not UPSTREAM_RESULT.is_file():
        raise RuntimeError(f"pinned pyperformance source report is missing: {UPSTREAM_RESULT}")
    report = json.loads(UPSTREAM_RESULT.read_text(encoding="utf-8"))
    if report.get("kind") != "pyperformance_pinned_source_probe" or report.get("status") != "passed":
        raise RuntimeError("pinned pyperformance source report is invalid")
    if report.get("source_commit") != "2ab96f98f98274d07ebfdfd66e4f021723179706":
        raise RuntimeError("pinned pyperformance source report has an unexpected source commit")
    return report


def invoke(command: list[str], cwd: Path) -> tuple[float, str]:
    start = time.perf_counter()
    completed = subprocess.run(
        command, cwd=cwd, check=True, capture_output=True, text=True
    )
    return time.perf_counter() - start, completed.stdout


def summarize(samples: list[float]) -> dict[str, float | list[float]]:
    ordered = sorted(samples)
    p95_index = max(0, math.ceil(0.95 * len(ordered)) - 1)
    return {
        "samples_seconds": samples,
        "median_seconds": statistics.median(samples),
        "p95_seconds_nearest_rank": ordered[p95_index],
        "max_seconds": max(samples),
    }


def main() -> int:
    if not VERITAS.is_file():
        raise SystemExit(f"Veritas executable not found: {VERITAS}")

    with tempfile.TemporaryDirectory(prefix="veritas-famous-benchmark-") as scratch:
        project = Path(scratch)
        for name in ("mandelbrot.vr", "mandelbrot.py", "mandelbrot.c", "veritas.toml"):
            shutil.copy2(WORKLOAD / name, project / name)

        invoke([str(VERITAS), "check", str(project / "mandelbrot.vr")], project)
        build_seconds, _ = invoke(
            [str(VERITAS), "build", str(project / "mandelbrot.vr")], project
        )
        artifact = project / "mandelbrot.vrb"
        if not artifact.is_file():
            raise RuntimeError("veritas build did not produce mandelbrot.vrb")

        clang = shutil.which("clang")
        c_compiler = None
        c_compile_seconds = None
        native_c = None
        if clang:
            c_compiler = subprocess.run(
                [clang, "--version"], check=True, capture_output=True, text=True
            ).stdout.splitlines()[0]
            c_compile_seconds, _ = invoke(
                [clang, "-O3", str(project / "mandelbrot.c"), "-o", str(project / "mandelbrot-c")],
                project,
            )
            native_c = str(project / "mandelbrot-c")

        cases = {
            "veritas_source_run": [str(VERITAS), "run", "--json", str(project / "mandelbrot.vr")],
            "veritas_bytecode_artifact_run": [str(VERITAS), "run", "--json", str(artifact)],
            "python3_reference": [sys.executable, str(project / "mandelbrot.py")],
        }
        if native_c:
            cases["native_c_o3_reference"] = [native_c]
        results: dict[str, dict[str, float | list[float]]] = {}
        native_backend_reports: dict[str, dict[str, object]] = {}
        for name, command in cases.items():
            for _ in range(WARMUPS):
                _, output = invoke(command, project)
                parsed = json.loads(output) if name.startswith("veritas_") else None
                actual = parsed["program_stdout"] if parsed else output
                if actual != EXPECTED:
                    raise RuntimeError(f"{name} output mismatch: {actual!r}")
            samples = []
            for _ in range(TRIALS):
                elapsed, output = invoke(command, project)
                parsed = json.loads(output) if name.startswith("veritas_") else None
                actual = parsed["program_stdout"] if parsed else output
                if actual != EXPECTED:
                    raise RuntimeError(f"{name} output mismatch: {actual!r}")
                if parsed:
                    if parsed["native_backend"]["calls"] == 0:
                        raise RuntimeError(f"{name} did not execute native code")
                    compiled = parsed["native_backend"]["compiled_functions"]
                    if not {"calculate", "escape_steps"}.issubset(compiled):
                        raise RuntimeError(f"{name} did not compile the complete numeric kernel")
                    if parsed["native_backend"]["calls"] < 6145:
                        raise RuntimeError(f"{name} did not execute native function calls")
                    native_backend_reports[name] = parsed["native_backend"]
                samples.append(elapsed)
            results[name] = summarize(samples)

        for name in ("parallel_cpu.vr", "parallel_cpu.py"):
            shutil.copy2(WORKLOAD / name, project / name)
        invoke([str(VERITAS), "check", str(project / "parallel_cpu.vr")], project)
        parallel_build_seconds, _ = invoke(
            [str(VERITAS), "build", str(project / "parallel_cpu.vr")], project
        )
        parallel_artifact = project / "parallel_cpu.vrb"
        expected_parallel = "959289\n" * 4

        parallel_cases = {
            "veritas_parallel_map_source": [
                str(VERITAS), "run", "--json", str(project / "parallel_cpu.vr")
            ],
            "veritas_parallel_map_artifact": [
                str(VERITAS), "run", "--json", str(parallel_artifact)
            ],
            "python_serial": [sys.executable, str(project / "parallel_cpu.py"), "serial"],
            "python_threads": [sys.executable, str(project / "parallel_cpu.py"), "threads"],
            "python_processes": [sys.executable, str(project / "parallel_cpu.py"), "processes"],
        }

        parallel_native_reports: dict[str, dict[str, object]] = {}

        def checked_parallel_output(
            name: str, raw_output: str, expected: str = expected_parallel
        ) -> None:
            parsed = json.loads(raw_output) if name.startswith("veritas_") else None
            actual = parsed["program_stdout"] if parsed else raw_output
            if actual != expected:
                raise RuntimeError(f"{name} output mismatch: {actual!r}")
            if parsed and parsed["native_backend"]["calls"] == 0:
                raise RuntimeError(f"{name} did not execute any native functions")
            if parsed:
                parallel_native_reports[name] = parsed["native_backend"]

        parallel_results: dict[str, dict[str, float | list[float]]] = {}
        for name, command in parallel_cases.items():
            for _ in range(1):
                _, output = invoke(command, project)
                checked_parallel_output(name, output)
            samples = []
            for _ in range(3):
                elapsed, output = invoke(command, project)
                checked_parallel_output(name, output)
                samples.append(elapsed)
            parallel_results[name] = summarize(samples)
        small_native_reports = dict(parallel_native_reports)

        gil_enabled = None
        gil_probe = subprocess.run(
            [sys.executable, "-c", "import sys; print(sys._is_gil_enabled())"],
            capture_output=True,
            text=True,
        )
        if gil_probe.returncode == 0:
            gil_enabled = gil_probe.stdout.strip() == "True"

        large_source = project / "parallel_cpu_large.vr"
        large_source.write_text(
            (project / "parallel_cpu.vr").read_text().replace("20000", "200000"),
            encoding="utf-8",
        )
        invoke([str(VERITAS), "check", str(large_source)], project)
        large_build_seconds, _ = invoke(
            [str(VERITAS), "build", str(large_source)], project
        )
        large_artifact = project / "parallel_cpu_large.vrb"
        large_result = sum(index % 97 for index in range(200_000))
        expected_large = f"{large_result}\n" * 4
        large_cases = {
            "veritas_parallel_map_source": [
                str(VERITAS), "run", "--json", str(large_source)
            ],
            "veritas_parallel_map_artifact": [
                str(VERITAS), "run", "--json", str(large_artifact)
            ],
            "python_serial": [
                sys.executable, str(project / "parallel_cpu.py"), "serial", "200000"
            ],
            "python_threads": [
                sys.executable, str(project / "parallel_cpu.py"), "threads", "200000"
            ],
            "python_processes": [
                sys.executable, str(project / "parallel_cpu.py"), "processes", "200000"
            ],
        }
        large_parallel_results: dict[str, dict[str, float | list[float]]] = {}
        for name, command in large_cases.items():
            _, output = invoke(command, project)
            checked_parallel_output(name, output, expected_large)
            samples = []
            for _ in range(3):
                elapsed, output = invoke(command, project)
                checked_parallel_output(name, output, expected_large)
                samples.append(elapsed)
            large_parallel_results[name] = summarize(samples)

        nbody_project = project / "nbody-project"
        nbody_project.mkdir()
        for name in ("nbody.vr", "nbody.py"):
            shutil.copy2(WORKLOAD / name, nbody_project / name)
        (nbody_project / "veritas.toml").write_text(
            "[project]\nname = \"nbody-probe\"\nversion = \"0.1.0\"\nentry = \"nbody.vr\"\n\n"
            "[runtime]\ninstruction_limit = 300000000\n",
            encoding="utf-8",
        )
        invoke([str(VERITAS), "check", str(nbody_project / "nbody.vr")], nbody_project)
        nbody_build_seconds, _ = invoke(
            [str(VERITAS), "build", str(nbody_project / "nbody.vr")], nbody_project
        )
        nbody_artifact = nbody_project / "nbody.vrb"
        nbody_cases = {
            "python3_reference": [sys.executable, str(nbody_project / "nbody.py")],
            "veritas_source": [str(VERITAS), "run", "--json", str(nbody_project / "nbody.vr")],
            "veritas_artifact": [str(VERITAS), "run", "--json", str(nbody_artifact)],
        }
        nbody_reference: list[float] | None = None
        nbody_results: dict[str, dict[str, float | list[float]]] = {}
        nbody_native_reports: dict[str, dict[str, object]] = {}
        for name, command in nbody_cases.items():
            samples = []
            for trial in range(6):
                elapsed, output = invoke(command, project)
                if name.startswith("veritas_"):
                    parsed = json.loads(output)
                    if parsed["status"] != "passed":
                        raise RuntimeError(f"{name} failed: {parsed}")
                    raw_values = parsed["program_stdout"].splitlines()
                    native_report = parsed["native_backend"]
                    nbody_native_reports[name] = native_report
                    if not {"energy", "advance"}.issubset(native_report["compiled_functions"]):
                        raise RuntimeError(f"{name} did not compile the N-body energy and advance functions")
                    if native_report["calls"] < 3 or native_report["deoptimizations"] != 0:
                        raise RuntimeError(f"{name} did not complete the N-body work natively: {native_report}")
                else:
                    raw_values = output.splitlines()
                values = [float(value) for value in raw_values]
                if len(values) != 2 or not all(math.isfinite(value) for value in values):
                    raise RuntimeError(f"{name} produced invalid energies: {values!r}")
                if nbody_reference is None and name == "python3_reference":
                    nbody_reference = values
                if nbody_reference is not None and any(
                    abs(actual - expected) > 1e-12
                    for actual, expected in zip(values, nbody_reference, strict=True)
                ):
                    raise RuntimeError(
                        f"{name} output mismatch: {values!r} versus {nbody_reference!r}"
                    )
                if trial > 0:
                    samples.append(elapsed)
            nbody_results[name] = summarize(samples)
        if nbody_reference is None:
            raise RuntimeError("Python N-body reference did not run")

        richards_project = project / "richards-project"
        richards_project.mkdir()
        richards_source = richards_project / "richards.vr"
        shutil.copy2(WORKLOAD / "richards.vr", richards_source)
        (richards_project / "veritas.toml").write_text(
            '[project]\nname = "richards-probe"\nversion = "0.1.0"\n'
            'entry = "richards.vr"\n\n[runtime]\ninstruction_limit = 30000000\n',
            encoding="utf-8",
        )
        invoke([str(VERITAS), "check", str(richards_project)], richards_project)
        richards_build_seconds, _ = invoke(
            [str(VERITAS), "build", str(richards_project)], richards_project
        )
        richards_artifact = richards_project / "richards.vrb"
        richards_cases = {
            "veritas_source": [str(VERITAS), "run", "--json", str(richards_project)],
            "veritas_artifact": [str(VERITAS), "run", "--json", str(richards_artifact)],
            "official_python_source": [
                sys.executable,
                str(WORKLOAD / "richards_reference.py"),
            ],
        }
        richards_expected = "9297\n23246\n"
        richards_results: dict[str, dict[str, float | list[float]]] = {}
        richards_native_reports: dict[str, dict[str, object]] = {}
        richards_trials = 7
        richards_warmups = 1
        for name, command in richards_cases.items():
            samples = []
            for trial in range(richards_warmups + richards_trials):
                elapsed, output = invoke(command, richards_project)
                if name.startswith("veritas_"):
                    parsed = json.loads(output)
                    if parsed["status"] != "passed":
                        raise RuntimeError(f"{name} Richards failed: {parsed}")
                    actual = parsed["program_stdout"]
                    native = parsed["native_backend"]
                    if native["calls"] <= 0 or native["deoptimizations"] != 0:
                        raise RuntimeError(f"{name} Richards native report is invalid: {native}")
                    required_richards_functions = {
                        "next_worker_count",
                        "xor_d008",
                        "route_packet",
                        "main",
                    }
                    if not required_richards_functions.issubset(native["compiled_functions"]):
                        raise RuntimeError(
                            f"{name} Richards scheduler was not fully compiled: {native}"
                        )
                    richards_native_reports[name] = native
                else:
                    actual = output
                if actual != richards_expected:
                    raise RuntimeError(f"{name} Richards output mismatch: {actual!r}")
                if trial >= richards_warmups:
                    samples.append(elapsed)
            richards_results[name] = summarize(samples)

        upstream_report = load_pyperformance_source_report()
        upstream_nbody_median = upstream_report["nbody"]["python_median_seconds"]
        upstream_richards_median = upstream_report["richards"]["python_median_seconds"]
        report = {
            "schema_version": 1,
            "kind": "veritas_famous_benchmark_report",
            "status": "passed",
            "workload": "Mandelbrot escape-time kernel, 96x64 grid, 80 iteration ceiling",
            "correct_output": EXPECTED.splitlines(),
            "timing_scope": (
                "Each sample is a fresh process and includes process startup. "
                "Veritas source samples also include parsing, typechecking and compilation; "
                "artifact samples load the current bytecode format; Python samples parse and execute source. "
                "All include the same arithmetic workload and captured output."
            ),
            "warmups_per_case": WARMUPS,
            "timed_trials_per_case": TRIALS,
            "artifact_build_seconds_separate": build_seconds,
            "artifact_size_bytes": artifact.stat().st_size,
            "native_c_compiler": c_compiler,
            "native_c_compile_seconds_separate": c_compile_seconds,
            "parallel_map_probe": {
                "workload": "four independent integer loops of 20,000 iterations each",
                "correct_output_per_task": 959289,
                "timing_scope": "Each sample is a fresh process. Python process-pool samples include worker startup; Veritas samples include JIT initialization and worker-thread startup, with a fresh isolated VM frame per task.",
                "warmups_per_case": 1,
                "timed_trials_per_case": 3,
                "python_gil_enabled": gil_enabled,
                "build_seconds_separate": parallel_build_seconds,
                "results": parallel_results,
                "native_backend_reports": small_native_reports,
                "allocation_budget": "Default 256 MiB cumulative estimate budget; no per-workload override.",
            },
            "large_parallel_map_probe": {
                "workload": "four independent integer loops of 200,000 iterations each",
                "correct_output_per_task": large_result,
                "timing_scope": "Each sample is a fresh process. Python process-pool samples include worker startup; Veritas samples include JIT initialization and worker-thread startup, with a fresh isolated VM frame per task.",
                "allocation_limit_bytes": 268435456,
                "allocation_budget": "Default 256 MiB cumulative estimate budget; no per-workload override.",
                "warmups_per_case": 1,
                "timed_trials_per_case": 3,
                "build_seconds_separate": large_build_seconds,
                "results": large_parallel_results,
                "native_backend_reports": parallel_native_reports,
            },
            "nbody_probe": {
                "workload": "Five-body, 20,000-step symplectic N-body energy workload using standard benchmark initial conditions and the pyperformance default iteration count.",
                "correct_output_energy_values": nbody_reference,
                "absolute_tolerance": 1e-12,
                "timing_scope": "Each sample is a fresh process; source parsing/compilation and artifact loading are included. One warmup and five timed trials per case.",
                "warmups_per_case": 1,
                "timed_trials_per_case": 5,
                "allocation_limit_bytes": 268435456,
                "allocation_limit_note": "The optimized same-local list_set assignment stays within the default 256 MiB allocation ceiling.",
                "instruction_limit_note": "A 300,000,000 instruction ceiling is declared for this 20,000-step workload; the default 10,000,000 ceiling is too low.",
                "build_seconds_separate": nbody_build_seconds,
                "artifact_size_bytes": nbody_artifact.stat().st_size,
                "results": nbody_results,
                "native_backend_reports": nbody_native_reports,
                "official_pyperformance_python_median_seconds": upstream_nbody_median,
                "veritas_source_to_official_python_ratio": nbody_results["veritas_source"]["median_seconds"] / upstream_nbody_median,
                "native_backend_scope": "The complete 20,000-step loop inside advance, including typed record/list access and same-local list updates, plus energy run in Cranelift native code. The VM handles entry, output and the two energy calls.",
                "full_default_workload_note": "The 20,000-step pyperformance-default workload completes and matches the Python reference within 1e-12 under the default 256 MiB allocation ceiling. It requires a declared 300,000,000 instruction ceiling; the standard 10,000,000 ceiling is too low.",
            },
            "richards_probe": {
                "workload": "One iteration of the pinned Richards task scheduler benchmark, with six task slots, linked packet queues, task state, and the canonical hold/packet checks.",
                "correct_output": ["9297", "23246"],
                "canonical_hold_count": 9297,
                "canonical_packet_count": 23246,
                "timing_scope": "Each sample is a fresh process. Veritas source samples parse, typecheck and compile; artifact samples load bytecode; Python samples import and execute the pinned upstream workload. One warmup and seven timed trials per case.",
                "warmups_per_case": richards_warmups,
                "timed_trials_per_case": richards_trials,
                "instruction_limit": 30000000,
                "build_seconds_separate": richards_build_seconds,
                "artifact_size_bytes": richards_artifact.stat().st_size,
                "results": richards_results,
                "native_backend_reports": richards_native_reports,
                "official_python_median_seconds": richards_results["official_python_source"]["median_seconds"],
                "pinned_source_probe_python_median_seconds": upstream_richards_median,
                "veritas_source_to_official_python_ratio": richards_results["veritas_source"]["median_seconds"] / richards_results["official_python_source"]["median_seconds"],
                "native_backend_scope": "The complete Veritas Richards port, including main, route_packet, xor_d008, and next_worker_count, executes in Cranelift with zero deoptimizations. This is one translated implementation on one host, not a general performance claim.",
            },
            "official_pyperformance_source_probe": upstream_report,
            "veritas_binary": str(VERITAS),
            "python_version": sys.version,
            "platform": platform.platform(),
            "architecture": platform.machine(),
            "results": results,
            "native_backend_reports": native_backend_reports,
            "limitations": [
                "This is a dependency-free representative translation, not a run of the Computer Language Benchmarks Game source file.",
                "Mandelbrot's Float64 escape kernel and the bounded parallel integer functions exercise different supported Cranelift subsets; other functions continue on the VM.",
                "The C reference is an independently compiled baseline, not a Veritas-generated backend or FFI call.",
                "One host and one numeric workload do not establish general performance or recommendation superiority.",
                "The bounded parallel workload validates output and measures available modes; its process-pool timings include worker startup and cannot establish cross-host speedup.",
                "The Veritas N-body workload is a translation, not an unmodified pyperformance program. A separate pinned-source probe validates the official Python workload and canonical Richards counts; Veritas-to-Python timing remains a fresh-process translated-workload comparison.",
                "The Veritas Richards port matches canonical counts and all four functions execute natively with zero deoptimizations; this one-host run remains slower than pinned Python and does not support a general speed claim.",
            ],
        }
        RESULT.parent.mkdir(parents=True, exist_ok=True)
        RESULT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))
        print(f"saved {RESULT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
