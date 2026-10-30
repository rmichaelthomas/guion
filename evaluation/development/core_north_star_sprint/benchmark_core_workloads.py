#!/usr/bin/env python3
"""Output-checked paired short-source and local HTTP benchmarks for the core sprint."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import threading
from collections import Counter
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402
sys.path.insert(0, str(ROOT / "evaluation/development/core_cost_closure"))
import measure_costs  # noqa: E402

HERE = Path(__file__).resolve().parent
BASELINE_HTTP_STDLIB = ROOT / "evaluation/development/core_cost_closure/fixtures/http-stdlib-before.gu"
HTTP_PAYLOAD_SIZES = (0, 1024, 65536, 1048576)
JSONL_ROWS = (20000, 200000)
BASELINE_SOURCE_REVISION = "41d3b48551ea735135ee8dc759bff513213a4d72"
TINY_SOURCE = 'fn main() -> Int:\n    print("ready")\n    return 0\n'
TINY_PYTHON = 'print("ready")\n'


class FixtureHandler(BaseHTTPRequestHandler):
    counts: Counter[str]
    counts_lock: threading.Lock
    payload: bytes

    def do_GET(self) -> None:
        if self.path != "/json":
            self.send_error(404)
            return
        with self.counts_lock:
            self.counts[self.path] += 1
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(self.payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(self.payload)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def frozen_stdlib_archive(revision: str) -> bytes:
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", f"{revision}^{{commit}}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    return subprocess.run(
        ["git", "archive", "--format=tar", resolved, "language/stdlib"],
        cwd=ROOT,
        capture_output=True,
        check=True,
    ).stdout


def prepare_frozen_stdlib(parent: Path, revision: str) -> Path:
    """Materialize the baseline standard library away from the live development tree."""
    root = parent / "baseline-stdlib"
    root.mkdir(parents=True, exist_ok=True)
    archive_bytes = frozen_stdlib_archive(revision)
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:") as archive:
        for member in archive.getmembers():
            parts = Path(member.name).parts
            if not member.isfile() or parts[:2] != ("language", "stdlib") or len(parts) < 3:
                continue
            destination = root.joinpath(*parts[2:])
            destination.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            if source is None:
                raise RuntimeError(f"could not read archived standard-library file {member.name}")
            destination.write_bytes(source.read())
    return root


def make_payload(payload_bytes: int) -> bytes:
    if payload_bytes < 0:
        raise ValueError("HTTP payload size cannot be negative")
    return json.dumps(
        {"count": 42, "message": "hello", "payload": "a" * payload_bytes},
        separators=(",", ":"),
    ).encode("utf-8")


def benchmark_input_hashes() -> dict[str, str]:
    """Inputs that must be identical in the before and candidate build cohorts."""
    return {
        "benchmark_script": digest(Path(__file__)),
        "manifest_tool": digest(Path(measure_costs.__file__)),
        "performance_runner": digest(Path(runner.__file__)),
        "tiny_guion_source": hashlib.sha256(TINY_SOURCE.encode("utf-8")).hexdigest(),
        "tiny_python_source": hashlib.sha256(TINY_PYTHON.encode("utf-8")).hexdigest(),
        "http_legacy_template": digest(HERE / "http_request_legacy.gu"),
        "http_typed_template": digest(HERE / "http_response_json.gu"),
        "http_typed_baseline_template": digest(HERE / "http_response_json_legacy_types.gu"),
        "http_baseline_stdlib_module": digest(BASELINE_HTTP_STDLIB),
        "baseline_stdlib_tree": hashlib.sha256(
            frozen_stdlib_archive(BASELINE_SOURCE_REVISION)
        ).hexdigest(),
        "http_python_reference": digest(HERE / "http_reference.py"),
        "jsonl_application_source": digest(runner.APP / "main.vr"),
        "jsonl_application_manifest": digest(runner.APP / "veritas.toml"),
        "jsonl_generator": digest(runner.GENERATOR),
        "jsonl_python_reference": digest(runner.DATA_REFERENCE),
        **{
            f"http_response_body_{payload_bytes}": hashlib.sha256(make_payload(payload_bytes)).hexdigest()
            for payload_bytes in HTTP_PAYLOAD_SIZES
        },
    }


def git_revision() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()


def guion_command(binary: Path, source: Path) -> list[str]:
    return [
        str(binary.resolve()), "run", "--json", "--no-cache",
        "--allow-http-host", "127.0.0.1", str(source),
    ]


def http_guion_command(binary: Path, source: Path, stdlib_root: Path, *, no_cache: bool) -> list[str]:
    command = [
        "env",
        f"GUION_STDLIB_ROOT={stdlib_root.resolve()}",
        str(binary.resolve()),
        "run",
        "--json",
    ]
    if no_cache:
        command.append("--no-cache")
    return [*command, "--allow-http-host", "127.0.0.1", str(source)]


def http_build_command(binary: Path, source: Path, output: Path, stdlib_root: Path) -> list[str]:
    return [
        "env",
        f"GUION_STDLIB_ROOT={stdlib_root.resolve()}",
        str(binary.resolve()),
        "build",
        str(source),
        "--output",
        str(output),
    ]


def prepare_http_baseline_stdlib(parent: Path) -> Path:
    """Materialize the standard library revision supported by the baseline binary."""
    root = prepare_frozen_stdlib(parent, BASELINE_SOURCE_REVISION)
    shutil.copyfile(BASELINE_HTTP_STDLIB, root / "http.gu")
    return root


def prepare_http_candidate_stdlib(parent: Path) -> Path:
    """Copy the candidate stdlib beside the baseline to keep filesystem I/O symmetric."""
    root = parent / "candidate-stdlib"
    shutil.copytree(ROOT / "language/stdlib", root, dirs_exist_ok=True)
    return root


def http_commands_for_binary(
    binary: Path,
    legacy_source: Path,
    typed_source: Path,
    cohort: str,
    stdlib_root: Path,
) -> dict[str, list[str]]:
    """Build same-executable HTTP arms so API cost is not confounded with binary changes."""
    return {
        f"guion_legacy_{cohort}": http_guion_command(binary, legacy_source, stdlib_root, no_cache=True),
        f"guion_typed_{cohort}": http_guion_command(binary, typed_source, stdlib_root, no_cache=True),
    }


def http_artifact_commands_for_binary(
    binary: Path,
    legacy_artifact: Path,
    typed_artifact: Path,
    cohort: str,
    stdlib_root: Path,
) -> dict[str, list[str]]:
    """Build artifact arms that measure execution without source compilation."""
    return {
        f"guion_legacy_{cohort}_artifact": [
            "env",
            f"GUION_STDLIB_ROOT={stdlib_root.resolve()}",
            str(binary.resolve()),
            "run",
            "--json",
            "--allow-http-host",
            "127.0.0.1",
            str(legacy_artifact),
        ],
        f"guion_typed_{cohort}_artifact": [
            "env",
            f"GUION_STDLIB_ROOT={stdlib_root.resolve()}",
            str(binary.resolve()),
            "run",
            "--json",
            "--allow-http-host",
            "127.0.0.1",
            str(typed_artifact),
        ],
    }


def http_cached_source_commands_for_binary(
    binary: Path,
    legacy_source: Path,
    typed_source: Path,
    cohort: str,
    stdlib_root: Path,
) -> dict[str, list[str]]:
    """Build normal source runs so warmups populate and measure Guion's source cache."""
    return {
        f"guion_legacy_{cohort}_cached_source": [
            *http_guion_command(binary, legacy_source, stdlib_root, no_cache=False),
        ],
        f"guion_typed_{cohort}_cached_source": [
            *http_guion_command(binary, typed_source, stdlib_root, no_cache=False),
        ],
    }


def write_http_source_project(parent: Path, name: str, source: str) -> Path:
    """Give each source arm its own one-entry Guion cache root."""
    project = parent / name
    project.mkdir(parents=True, exist_ok=True)
    runner.write_manifest(project, f"sprint-http-{name}", 30_000_000)
    path = project / "main.gu"
    path.write_text(source, encoding="utf-8")
    return path


def write_http_cached_source_projects(
    parent: Path, cohort: str, legacy_source: str, typed_source: str
) -> tuple[Path, Path]:
    """Give each executable cohort isolated warm-source cache roots."""
    cohort_root = parent / cohort
    legacy = write_http_source_project(cohort_root, "legacy", legacy_source)
    typed = write_http_source_project(cohort_root, "typed", typed_source)
    return legacy, typed


def measure_short_source(
    binary: Path, candidate_binary: Path | None, python_binary: Path, project: Path, trials: int
) -> dict[str, object]:
    project.mkdir(parents=True, exist_ok=True)
    source = project / "tiny.gu"
    python_source = project / "tiny.py"
    source.write_text(TINY_SOURCE, encoding="utf-8")
    python_source.write_text(TINY_PYTHON, encoding="utf-8")
    runner.write_manifest(project, "sprint-tiny-source", 100_000)
    commands = {"guion_baseline": guion_command(binary, source)}
    if candidate_binary is not None:
        commands["guion_candidate"] = guion_command(candidate_binary, source)
    commands["python_reference"] = [str(python_binary), str(python_source)]
    samples, metadata = runner.sample_candidates(
        "sprint-tiny-source", commands, project, ["ready"], trials=trials
    )
    ratios = [
        guion / python
        for guion, python in zip(
            samples["guion_baseline"]["samples_seconds"],
            samples["python_reference"]["samples_seconds"],
            strict=True,
        )
    ]
    comparisons: dict[str, object] = {"baseline_over_python": runner.paired_median_ci(ratios)}
    if "guion_candidate" in samples:
        candidate_ratios = [
            candidate / baseline_sample
            for candidate, baseline_sample in zip(
                samples["guion_candidate"]["samples_seconds"],
                samples["guion_baseline"]["samples_seconds"],
                strict=True,
            )
        ]
        comparisons["candidate_over_baseline"] = runner.paired_median_ci(candidate_ratios)
    return {
        "commands": commands,
        "expected_output": ["ready"],
        "samples": samples,
        "comparisons": comparisons,
        **metadata,
    }


def measure_http(
    baseline_binary: Path,
    candidate_binary: Path | None,
    python_binary: Path,
    project: Path,
    trials: int,
    payload_bytes: int = 65536,
) -> dict[str, object]:
    project.mkdir(parents=True, exist_ok=True)
    baseline_stdlib_root = prepare_http_baseline_stdlib(project / "stdlib-snapshots")
    candidate_stdlib_root = prepare_http_candidate_stdlib(project / "stdlib-snapshots")
    payload = make_payload(payload_bytes)
    expected = ["hello", "42", str(payload_bytes)]
    FixtureHandler.payload = payload
    counts: Counter[str] = Counter()
    lock = threading.Lock()
    FixtureHandler.counts = counts
    FixtureHandler.counts_lock = lock
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, name="sprint-http-fixture", daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}/json"
    legacy_program = (
        (HERE / "http_request_legacy.gu")
        .read_text(encoding="utf-8")
        .replace("__URL__", url)
        .replace("__PAYLOAD_LENGTH__", str(payload_bytes))
    )
    reference = HERE / "http_reference.py"
    typed_baseline_program = (
        (HERE / "http_response_json_legacy_types.gu")
        .read_text(encoding="utf-8")
        .replace("__URL__", url)
        .replace("__PAYLOAD_LENGTH__", str(payload_bytes))
    )
    typed_candidate_program = (
        (HERE / "http_response_json.gu")
        .read_text(encoding="utf-8")
        .replace("__URL__", url)
        .replace("__PAYLOAD_LENGTH__", str(payload_bytes))
    )
    legacy_source = write_http_source_project(project, "legacy", legacy_program)
    baseline_typed_source = write_http_source_project(
        project, "typed-baseline", typed_baseline_program
    )
    baseline_cached_sources = write_http_cached_source_projects(
        project / "cached-source", "baseline", legacy_program, typed_baseline_program
    )
    legacy_artifact = project / "http_legacy.gub"
    typed_artifact = project / "http_typed.gub"
    legacy_build_seconds, _ = runner.require_ok(
        http_build_command(baseline_binary, legacy_source, legacy_artifact, baseline_stdlib_root),
        project,
    )
    typed_build_seconds, _ = runner.require_ok(
        http_build_command(baseline_binary, baseline_typed_source, typed_artifact, baseline_stdlib_root),
        project,
    )
    commands = http_commands_for_binary(
        baseline_binary, legacy_source, baseline_typed_source, "baseline", baseline_stdlib_root
    )
    commands.update(
        http_cached_source_commands_for_binary(
            baseline_binary, *baseline_cached_sources, "baseline", baseline_stdlib_root
        )
    )
    commands.update(
        http_artifact_commands_for_binary(
            baseline_binary, legacy_artifact, typed_artifact, "baseline", baseline_stdlib_root
        )
    )
    commands["python_reference"] = [str(python_binary), str(reference), url, str(payload_bytes)]
    source_hashes = {"legacy": digest(legacy_source), "python_reference": digest(reference)}
    source_hashes["typed_baseline"] = digest(baseline_typed_source)
    candidate_build_seconds: dict[str, float] = {}
    if candidate_binary is not None:
        candidate_typed_source = write_http_source_project(
            project, "typed-candidate", typed_candidate_program
        )
        candidate_cached_sources = write_http_cached_source_projects(
            project / "cached-source", "candidate", legacy_program, typed_candidate_program
        )
        commands.update(
            http_commands_for_binary(
                candidate_binary, legacy_source, candidate_typed_source, "candidate", candidate_stdlib_root
            )
        )
        source_hashes["typed_candidate"] = digest(candidate_typed_source)
        commands.update(
            http_cached_source_commands_for_binary(
                candidate_binary, *candidate_cached_sources, "candidate", candidate_stdlib_root
            )
        )
        candidate_legacy_artifact = project / "candidate_http_legacy.gub"
        candidate_typed_artifact = project / "candidate_http_typed.gub"
        candidate_build_seconds["legacy"] = runner.require_ok(
            http_build_command(
                candidate_binary, legacy_source, candidate_legacy_artifact, candidate_stdlib_root
            ),
            project,
        )[0]
        candidate_build_seconds["typed"] = runner.require_ok(
            http_build_command(
                candidate_binary, candidate_typed_source, candidate_typed_artifact, candidate_stdlib_root
            ),
            project,
        )[0]
        commands.update(
            http_artifact_commands_for_binary(
                candidate_binary,
                candidate_legacy_artifact,
                candidate_typed_artifact,
                "candidate",
                candidate_stdlib_root,
            )
        )
    runner.write_manifest(project, "sprint-http-local-json", 30_000_000)
    phase_profiles: dict[str, object] = {}
    try:
        samples, metadata = runner.sample_candidates(
            f"sprint-http-local-json-{payload_bytes}", commands, project, expected, trials=trials
        )
        for lane in (
            "guion_legacy_baseline",
            "guion_typed_baseline",
            "guion_legacy_baseline_artifact",
            "guion_typed_baseline_artifact",
            "guion_legacy_candidate",
            "guion_typed_candidate",
            "guion_legacy_candidate_artifact",
            "guion_typed_candidate_artifact",
            "guion_legacy_baseline_cached_source",
            "guion_typed_baseline_cached_source",
            "guion_legacy_candidate_cached_source",
            "guion_typed_candidate_cached_source",
        ):
            if lane in commands:
                phase_profiles[lane] = runner.profile_guion_run(
                    commands[lane],
                    project,
                    f"sprint-http-local-json-{payload_bytes}",
                    expected,
                    runtime_profile_enabled=True,
                )
                if "cached_source" in lane and phase_profiles[lane]["cache_status"] != "hit":
                    raise RuntimeError(f"{lane} did not use a warm Guion source-cache entry")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    expected_count = (runner.WARMUPS + trials) * len(commands) + len(phase_profiles)
    actual_count = counts["/json"]
    if actual_count != expected_count:
        raise RuntimeError(f"fixture observed {actual_count} /json requests; expected {expected_count}")
    comparisons: dict[str, object] = {}
    python_samples = samples["python_reference"]["samples_seconds"]
    for lane in (
        "guion_legacy_baseline",
        "guion_typed_baseline",
        "guion_legacy_baseline_artifact",
        "guion_typed_baseline_artifact",
        "guion_legacy_baseline_cached_source",
        "guion_typed_baseline_cached_source",
        "guion_legacy_candidate",
        "guion_typed_candidate",
        "guion_legacy_candidate_artifact",
        "guion_typed_candidate_artifact",
        "guion_legacy_candidate_cached_source",
        "guion_typed_candidate_cached_source",
    ):
        if lane not in samples:
            continue
        ratios = [
            guion / python
            for guion, python in zip(samples[lane]["samples_seconds"], python_samples, strict=True)
        ]
        comparisons[lane + "_over_python"] = runner.paired_median_ci(ratios)
    pairings = [
        (
            "typed_baseline_source_over_legacy_baseline_source",
            "guion_typed_baseline",
            "guion_legacy_baseline",
        ),
        (
            "typed_baseline_artifact_over_legacy_baseline_artifact",
            "guion_typed_baseline_artifact",
            "guion_legacy_baseline_artifact",
        ),
        (
            "typed_baseline_cached_source_over_legacy_baseline_cached_source",
            "guion_typed_baseline_cached_source",
            "guion_legacy_baseline_cached_source",
        ),
    ]
    if candidate_binary is not None:
        pairings.extend(
            (
                ("legacy_candidate_over_baseline", "guion_legacy_candidate", "guion_legacy_baseline"),
                ("typed_candidate_over_legacy_candidate", "guion_typed_candidate", "guion_legacy_candidate"),
                ("typed_candidate_over_typed_baseline", "guion_typed_candidate", "guion_typed_baseline"),
                (
                    "typed_candidate_artifact_over_legacy_candidate_artifact",
                    "guion_typed_candidate_artifact",
                    "guion_legacy_candidate_artifact",
                ),
                (
                    "typed_candidate_cached_source_over_legacy_candidate_cached_source",
                    "guion_typed_candidate_cached_source",
                    "guion_legacy_candidate_cached_source",
                ),
                (
                    "legacy_candidate_artifact_over_baseline_artifact",
                    "guion_legacy_candidate_artifact",
                    "guion_legacy_baseline_artifact",
                ),
                (
                    "typed_candidate_artifact_over_typed_baseline_artifact",
                    "guion_typed_candidate_artifact",
                    "guion_typed_baseline_artifact",
                ),
            )
        )
    for comparison, numerator, denominator in pairings:
        ratios = [
            candidate / baseline
            for candidate, baseline in zip(
                samples[numerator]["samples_seconds"],
                samples[denominator]["samples_seconds"],
                strict=True,
            )
        ]
        comparisons[comparison] = runner.paired_median_ci(ratios)
    return {
        "url_host_grant": "127.0.0.1",
        "fixture_url_path": "/json",
        "fixture_response_status": 200,
        "fixture_response_payload_bytes": payload_bytes,
        "fixture_response_body_bytes": len(payload),
        "fixture_response_body_sha256": hashlib.sha256(payload).hexdigest(),
        "fixture_request_count": actual_count,
        "expected_fixture_request_count": expected_count,
        "expected_output": expected,
        "source_sha256": source_hashes,
        "artifact_build_seconds": {
            "baseline_legacy": legacy_build_seconds,
            "baseline_typed": typed_build_seconds,
            **(
                {
                    "candidate_legacy": candidate_build_seconds["legacy"],
                    "candidate_typed": candidate_build_seconds["typed"],
                }
                if candidate_build_seconds
                else {}
            ),
        },
        "source_cache_states": metadata["source_cache_states"],
        "commands": commands,
        "stdlib_sha256": {
            "baseline": digest(baseline_stdlib_root / "http.gu"),
            "candidate": digest(candidate_stdlib_root / "http.gu"),
        },
        "samples": samples,
        "comparisons": comparisons,
        "phase_profiles": phase_profiles,
        **metadata,
    }


def measure_jsonl_comparison(
    baseline_binary: Path,
    candidate_binary: Path,
    python_binary: Path,
    project: Path,
    rows: int,
    trials: int,
) -> dict[str, object]:
    """Pair unchanged JSONL source/artifact runs against both builds and Python."""
    workload_name = f"jsonl-summary-data-{rows}"
    project.mkdir(parents=True, exist_ok=True)
    runner.shutil.copytree(runner.APP, project, dirs_exist_ok=True)
    runner.prepare_guion_application(project)
    manifest = project / "guion.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8")
        + "\n[runtime]\ninstruction_limit = 1000000000\nallocation_limit_bytes = 8589934592\n",
        encoding="utf-8",
    )
    generated = project / "performance-inputs"
    events = generated / "events.jsonl"
    services = generated / "services.txt"
    output = generated / "summary.json"
    runner.require_ok(
        [
            str(python_binary),
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
        [str(python_binary), str(reference), "python", str(events), str(services), str(expected_path)],
        project,
    )
    expected = runner.normalize_data(json.loads(expected_path.read_text(encoding="utf-8")))
    sqlite_expected = project / "expected-sqlite.json"
    runner.require_ok(
        [str(python_binary), str(reference), "sqlite", str(events), str(services), str(sqlite_expected)],
        project,
    )
    if json.loads(sqlite_expected.read_text(encoding="utf-8")) != expected:
        raise RuntimeError(f"Python dictionary and native SQLite references disagree at {rows} rows")

    source = project / "main.gu"
    before_artifact = project / "before.gub"
    candidate_artifact = project / "candidate.gub"
    before_build_seconds, _ = runner.require_ok(
        [str(baseline_binary), "build", str(source), "--output", str(before_artifact)], project
    )
    candidate_build_seconds, _ = runner.require_ok(
        [str(candidate_binary), "build", str(source), "--output", str(candidate_artifact)], project
    )
    args = ["--events", str(events.relative_to(project)), "--output", str(output.relative_to(project))]
    commands = {
        "guion_before_source_uncached": runner.uncached_source_command(
            [str(baseline_binary), "run", "--json", str(source), *args]
        ),
        "guion_candidate_source_uncached": runner.uncached_source_command(
            [str(candidate_binary), "run", "--json", str(source), *args]
        ),
        "guion_before_artifact": [str(baseline_binary), "run", "--json", str(before_artifact), *args],
        "guion_candidate_artifact": [
            str(candidate_binary),
            "run",
            "--json",
            str(candidate_artifact),
            *args,
        ],
        "python_data": [str(python_binary), str(reference), "python", str(events), str(services), str(output)],
        "python_sqlite_native": [
            str(python_binary),
            str(reference),
            "sqlite",
            str(events),
            str(services),
            str(output),
        ],
    }
    runner.write_manifest(project, f"sprint-{workload_name}", 1_000_000_000)
    samples, trial_metadata = runner.sample_candidates(
        workload_name,
        commands,
        project,
        expected,
        output_file=output,
        repetitions=1,
        trials=trials,
    )

    def paired_ratio(numerator: str, denominator: str) -> dict[str, object]:
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
        "before_source_over_python": paired_ratio("guion_before_source_uncached", "python_data"),
        "candidate_source_over_python": paired_ratio("guion_candidate_source_uncached", "python_data"),
        "candidate_source_over_before_source": paired_ratio(
            "guion_candidate_source_uncached", "guion_before_source_uncached"
        ),
        "before_artifact_over_python": paired_ratio("guion_before_artifact", "python_data"),
        "candidate_artifact_over_python": paired_ratio("guion_candidate_artifact", "python_data"),
        "candidate_artifact_over_before_artifact": paired_ratio(
            "guion_candidate_artifact", "guion_before_artifact"
        ),
    }
    profiles = {
        lane: runner.profile_guion_run(
            commands[lane],
            project,
            workload_name,
            expected,
            output_file=output,
            runtime_profile_enabled=True,
        )
        for lane in ("guion_candidate_source_uncached", "guion_candidate_artifact")
    }
    memory_bytes: dict[str, int] = {}
    for lane, command in commands.items():
        output.unlink(missing_ok=True)
        observed = runner.peak_memory(command, project)
        if observed is None:
            raise RuntimeError(f"peak RSS sample failed for {workload_name}/{lane}")
        actual = runner.normalize_data(json.loads(output.read_text(encoding="utf-8")))
        if actual != expected:
            raise RuntimeError(f"peak RSS output mismatch for {workload_name}/{lane}")
        memory_bytes[lane] = observed
    return {
        "rows": rows,
        "generator_seed": 20261025,
        "trials_per_lane": trials,
        "repetitions_per_process": 1,
        "build_seconds": {
            "before_artifact": before_build_seconds,
            "candidate_artifact": candidate_build_seconds,
        },
        "commands": commands,
        "samples": samples,
        "comparisons": comparisons,
        "phase_profiles": profiles,
        "peak_rss_bytes": memory_bytes,
        **trial_metadata,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print-input-hashes", action="store_true")
    parser.add_argument("--jsonl-only", action="store_true")
    parser.add_argument("--baseline-binary", type=Path)
    parser.add_argument("--candidate-binary", type=Path)
    parser.add_argument("--baseline-manifest", type=Path)
    parser.add_argument("--candidate-manifest", type=Path)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--scratch-dir", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--trials", type=int, default=15)
    args = parser.parse_args()
    if args.print_input_hashes:
        print(json.dumps(benchmark_input_hashes(), indent=2, sort_keys=True))
        return
    for name in ("baseline_binary", "baseline_manifest", "scratch_dir", "output"):
        if getattr(args, name) is None:
            parser.error(f"--{name.replace('_', '-')} is required unless --print-input-hashes is used")
    if args.candidate_binary is not None and args.candidate_manifest is None:
        parser.error("--candidate-manifest is required with --candidate-binary")
    if args.candidate_manifest is not None and args.candidate_binary is None:
        parser.error("--candidate-binary is required with --candidate-manifest")
    if args.trials < 3:
        parser.error("at least three paired trials are required")
    baseline = args.baseline_binary.resolve(strict=True)
    candidate = args.candidate_binary.resolve(strict=True) if args.candidate_binary else None
    baseline_manifest = measure_costs.load_manifest(args.baseline_manifest)
    measure_costs.require_manifest_binary(baseline_manifest, baseline, role="baseline")
    expected_inputs = benchmark_input_hashes()
    if baseline_manifest.get("input_sha256") != expected_inputs:
        raise ValueError("baseline manifest workload inputs do not match this benchmark")
    candidate_manifest = None
    if candidate is not None:
        candidate_manifest = measure_costs.load_manifest(args.candidate_manifest)
        measure_costs.require_manifest_binary(candidate_manifest, candidate, role="candidate")
        measure_costs.require_comparable(baseline_manifest, candidate_manifest)
        if candidate_manifest.get("input_sha256") != expected_inputs:
            raise ValueError("candidate manifest workload inputs do not match this benchmark")
    python_binary = args.python.resolve(strict=True)
    scratch = args.scratch_dir.resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    baseline_stdlib_root = prepare_frozen_stdlib(
        scratch, baseline_manifest["source_revision"]
    )
    os.environ["GUION_STDLIB_ROOT"] = str(baseline_stdlib_root)
    project = scratch / "core-workloads"
    project.mkdir(parents=True, exist_ok=True)
    if args.jsonl_only:
        if candidate is None or candidate_manifest is None:
            parser.error("--jsonl-only requires both baseline and candidate binaries with manifests")
        workloads = {
            str(rows): measure_jsonl_comparison(
                baseline,
                candidate,
                python_binary,
                project / f"jsonl-summary-data-{rows}",
                rows,
                args.trials,
            )
            for rows in JSONL_ROWS
        }
        report = {
            "schema_version": 1,
            "kind": "guion-core-sprint-jsonl-comparison",
            "created_at_utc": datetime.now(timezone.utc).isoformat(),
            "source_revision": git_revision(),
            "python": {
                "version": subprocess.run(
                    [str(python_binary), "--version"], capture_output=True, text=True, check=True
                ).stdout.strip(),
                "executable": str(python_binary),
            },
            "binary": {
                "baseline_manifest": baseline_manifest,
                "candidate_manifest": candidate_manifest,
            },
            "standard_library": {
                "source_revision": baseline_manifest["source_revision"],
                "archive_sha256": hashlib.sha256(
                    frozen_stdlib_archive(baseline_manifest["source_revision"])
                ).hexdigest(),
            },
            "measurement": {
                "trials_per_lane": args.trials,
                "warmups_per_lane": runner.WARMUPS,
                "trial_seed": runner.TRIAL_SEED,
                "bootstrap_seed": runner.BOOTSTRAP_SEED,
                "bootstrap_resamples": runner.BOOTSTRAP_SAMPLES,
                "order": "randomized per paired trial by the existing performance runner",
                "timing": "uncached source and prebuilt artifact timed separately; output oracle checked for every trial",
                "runtime_profiles": "untimed candidate-only attribution profiles",
                "peak_rss": runner.MEMORY_MEASUREMENT,
            },
            "workloads": workloads,
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "status": "recorded",
                    "output": str(args.output),
                    "comparisons": {
                        rows: workload["comparisons"] for rows, workload in workloads.items()
                    },
                }
            )
        )
        return
    short = measure_short_source(baseline, candidate, python_binary, project / "short-source", args.trials)
    http_payload_matrix = {
        str(payload_bytes): measure_http(
            baseline,
            candidate,
            python_binary,
            project / "http" / str(payload_bytes),
            args.trials,
            payload_bytes,
        )
        for payload_bytes in HTTP_PAYLOAD_SIZES
    }
    report = {
        "schema_version": 1,
        "kind": "guion-core-sprint-short-and-http-benchmarks",
        "status": "passed",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": git_revision(),
        "python": {
            "version": subprocess.run(
                [str(python_binary), "--version"], capture_output=True, text=True, check=True
            ).stdout.strip(),
            "executable": str(python_binary),
        },
        "platform": {"platform": platform.platform(), "architecture": platform.machine()},
        "measurement": {
            "trials_per_arm": args.trials,
            "warmups_per_arm": runner.WARMUPS,
            "trial_seed": runner.TRIAL_SEED,
            "bootstrap_seed": runner.BOOTSTRAP_SEED,
            "bootstrap_resamples": runner.BOOTSTRAP_SAMPLES,
            "order": "randomized per paired trial by the existing performance runner",
            "timing": "child-process wall time from startup through exit; fixture startup and post-run output-oracle validation excluded",
            "cache": "Guion source cache bypassed with --no-cache; OS cache uncontrolled",
        },
        "binary": {
            "baseline_sha256": digest(baseline),
            "candidate_sha256": digest(candidate) if candidate else None,
            "baseline_manifest": baseline_manifest,
            "candidate_manifest": candidate_manifest,
        },
        "standard_library": {
            "source_revision": baseline_manifest["source_revision"],
            "archive_sha256": hashlib.sha256(
                frozen_stdlib_archive(baseline_manifest["source_revision"])
            ).hexdigest(),
        },
        "workloads": {"tiny-source": short, "http-payload-matrix": http_payload_matrix},
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "output": str(args.output), "http_requests_by_payload": {size: lane["fixture_request_count"] for size, lane in http_payload_matrix.items()}}))


if __name__ == "__main__":
    main()
