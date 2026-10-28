#!/usr/bin/env python3
"""Verify the 100,000-row analysis as a managed, relocated Guion package."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/package_analysis"
WARMED = ROOT / "target/perf-session/managed-python-package"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_checked(
    label: str,
    command: list[str],
    cwd: Path,
    environment: dict[str, str],
    *,
    timeout: int = 900,
) -> tuple[float, subprocess.CompletedProcess[str]]:
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    elapsed = time.perf_counter() - started
    if result.returncode:
        raise RuntimeError(
            f"{label} failed ({result.returncode}):\n{result.stdout}\n{result.stderr}"
        )
    return elapsed, result


def directory_bytes(root: Path) -> int:
    total = 0
    for parent, directories, files in os.walk(root, followlinks=False):
        directories[:] = [name for name in directories if not (Path(parent) / name).is_symlink()]
        for name in files:
            path = Path(parent) / name
            if not path.is_symlink():
                total += path.stat().st_size
    return total


def make_dataset(path: Path) -> bytes:
    rows = [
        {
            "value": (index * 7919) % 50_003,
            "tag": f"é-{index % 13}",
            "department": f"dept-{index % 5}",
        }
        for index in range(100_000)
    ]
    encoded = json.dumps(
        {"rows": rows}, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    path.write_bytes(encoded)
    return encoded


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/guion")
    parser.add_argument("--warmed-dir", type=Path, default=WARMED)
    parser.add_argument("--uv", type=Path)
    parser.add_argument(
        "--python-install-dir",
        type=Path,
        help="preloaded CPython install directory when it is mounted separately from the uv cache",
    )
    parser.add_argument(
        "--vm-started",
        action="store_true",
        help="record that this acceptance run used a VM-hosted environment",
    )
    parser.add_argument(
        "--scratch-dir",
        type=Path,
        default=ROOT / "target/perf-session/managed-package-analysis",
        help="parent directory for temporary project, dataset, and package files",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT
        / "evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis.json",
    )
    args = parser.parse_args()

    binary = args.binary.resolve(strict=True)
    warmed = args.warmed_dir.resolve()
    uv = (args.uv or warmed / "toolchain/uv").resolve(strict=True)
    fixture_manifest = (APP / "veritas.toml").read_text(encoding="utf-8")
    manifest = fixture_manifest.replace('entry = "main.vr"', 'entry = "main.gu"')
    manifest = manifest.replace(
        'resources = ["analysis.py", "dataset.json"]',
        'resources = ["analysis.py"]',
    )
    if manifest == fixture_manifest:
        raise RuntimeError("package-analysis fixture no longer matches the expected legacy layout")
    project_configuration = tomllib.loads(manifest)
    expected_python_version = project_configuration["python"]["version"]
    expected_dependencies = project_configuration["python"]["dependencies"]
    expected_uv_version = tomllib.loads(
        (ROOT / "distribution/toolchains/manifest.toml").read_text(encoding="utf-8")
    )["uv_version"]
    cache = warmed / "cache"
    python_install = args.python_install_dir or warmed / "python"
    if not python_install.is_absolute():
        python_install = ROOT / python_install
    python_install = python_install.resolve()
    for required in (cache, python_install):
        if not required.is_dir():
            raise RuntimeError(f"warmed managed Python cache is missing: {required}")

    scratch_parent = args.scratch_dir
    if not scratch_parent.is_absolute():
        scratch_parent = ROOT / scratch_parent
    scratch_parent.mkdir(parents=True, exist_ok=True)
    uv_environment = os.environ.copy()
    uv_environment.update(
        {
            "GUION_UV": str(uv),
            "GUION_PYTHON_INSTALL_DIR": str(python_install),
            "GUION_PYTHON_CACHE_DIR": str(cache),
            "UV_CACHE_DIR": str(cache),
            "UV_PYTHON_INSTALL_DIR": str(python_install),
            "UV_OFFLINE": "1",
            "UV_NO_PROGRESS": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )

    with tempfile.TemporaryDirectory(
        prefix="guion-managed-analysis-", dir=scratch_parent
    ) as temporary:
        work = Path(temporary)
        run_tmp = work / "tmp"
        run_tmp.mkdir()
        uv_environment["TMPDIR"] = str(run_tmp)
        project = work / "analysis project"
        package = work / "first package"
        relocated = work / "relocated apps" / "Analysis Ω"
        project.mkdir()
        (project / ".guion/python-bridge").mkdir(parents=True)
        shutil.copyfile(APP / "main.vr", project / "main.gu")
        source = (project / "main.gu").read_text(encoding="utf-8")
        (project / "main.gu").write_text(
            source.replace("Usage: veritas run", "Usage: guion run"), encoding="utf-8"
        )
        shutil.copyfile(APP / "analysis.py", project / "analysis.py")
        shutil.copyfile(
            APP / ".veritas/python-bridge/uv.lock",
            project / ".guion/python-bridge/uv.lock",
        )
        shutil.copyfile(
            APP / ".veritas/python-bridge/pyproject.toml",
            project / ".guion/python-bridge/pyproject.toml",
        )
        (project / "guion.toml").write_text(manifest, encoding="utf-8")

        dataset_path = work / "dataset.json"
        dataset_bytes = make_dataset(dataset_path)
        reference_path = work / "reference-report.json"
        reference_elapsed, reference = run_checked(
            "independent standard-library reference",
            [
                sys.executable,
                "-B",
                str(APP / "reference.py"),
                str(dataset_path),
                str(reference_path),
                "tag",
            ],
            work,
            uv_environment,
        )
        expected_report = reference_path.read_bytes()
        expected_value = json.loads(expected_report)
        if expected_value.get("count") != 100_000 or len(expected_value.get("groups", {})) != 13:
            raise RuntimeError("reference did not produce the expected 100,000 rows and 13 groups")

        doctor_elapsed, doctor = run_checked(
            "managed-toolchain preflight",
            [str(binary), "doctor", "--json"],
            work,
            uv_environment,
            timeout=30,
        )
        doctor_report = json.loads(doctor.stdout)
        if not doctor_report.get("capabilities", {}).get("python_calls"):
            raise RuntimeError("selected Guion build does not enable managed Python calls")
        current_artifact_format = doctor_report.get("guion", {}).get("artifact_format")
        if not isinstance(current_artifact_format, int):
            raise RuntimeError("selected Guion build did not report its current artifact format")

        sync_elapsed, sync = run_checked(
            "offline managed dependency sync",
            [str(binary), "sync", "--update", str(project)],
            work,
            uv_environment,
        )
        if f"{len(expected_dependencies)} declared dependencies" not in sync.stdout:
            raise RuntimeError(f"sync did not report the two declared packages: {sync.stdout}")

        package_elapsed, packaged = run_checked(
            "managed package assembly",
            [str(binary), "package", str(project), "--output", str(package)],
            work,
            uv_environment,
        )
        if not (package / "run").is_file() or not (package / "app.gub").is_file():
            raise RuntimeError("package is missing its launcher or compiled artifact")
        if (package / "main.gu").exists() or (package / "dataset.json").exists():
            raise RuntimeError("package must contain neither source nor caller-provided data")

        _, inspected = run_checked(
            "artifact-format inspection",
            [str(binary), "inspect", "--json", str(package / "app.gub")],
            work,
            uv_environment,
            timeout=30,
        )
        inspection = json.loads(inspected.stdout)
        if inspection.get("artifact_format") != current_artifact_format:
            raise RuntimeError(
                f"package artifact format {inspection.get('artifact_format')} does not match the current CLI format {current_artifact_format}"
            )

        metadata = json.loads((package / "PACKAGE.json").read_text(encoding="utf-8"))
        python_metadata = metadata.get("python_runtime") or {}
        if python_metadata.get("uv_version") != expected_uv_version:
            raise RuntimeError("package did not bundle the pinned uv version")
        if python_metadata.get("version") != expected_python_version:
            raise RuntimeError("package did not bundle the pinned CPython version")
        dependency_report = json.loads(
            (package / "PYTHON-DEPENDENCIES.json").read_text(encoding="utf-8")
        )
        expected_dependency_versions = {}
        for dependency in expected_dependencies:
            name, separator, version = dependency.partition("==")
            if not separator:
                raise RuntimeError(f"dependency is not pinned: {dependency}")
            expected_dependency_versions[name] = version
        dependency_versions = {
            record["name"]: record["version"]
            for record in dependency_report.get("packages", [])
            if record.get("name") in expected_dependency_versions
        }
        if dependency_versions != expected_dependency_versions:
            raise RuntimeError(
                "managed package dependency versions differ from the project pins: "
                f"expected {expected_dependency_versions}, found {dependency_versions}"
            )

        package_bytes = directory_bytes(package)
        package_files = sum(1 for path in package.rglob("*") if path.is_file())
        package_python_runtime_bytes = python_metadata.get("python_runtime_size_bytes")
        package_python_cache_bytes = python_metadata.get("python_cache_size_bytes")
        if not isinstance(package_python_runtime_bytes, int) or package_python_runtime_bytes <= 0:
            raise RuntimeError("package metadata has no positive bundled runtime size")
        if not isinstance(package_python_cache_bytes, int) or package_python_cache_bytes <= 0:
            raise RuntimeError("package metadata has no positive offline package-cache size")

        verify_elapsed, verified = run_checked(
            "pre-relocation package verification",
            [str(binary), "verify-package", str(package)],
            work,
            uv_environment,
            timeout=60,
        )
        relocated.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(package), str(relocated))
        runtime_environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("UV_", "PYTHON", "PIP_"))
            and not key.startswith("GUION_PYTHON_")
            and key not in {"GUION_UV", "GUION_PYTHON", "VIRTUAL_ENV", "CONDA_PREFIX"}
        }
        runtime_environment["TMPDIR"] = str(run_tmp)
        expected_stdout = (
            f"Rows:\n{expected_value['count']}\nTotal:\n{expected_value['total']}\n"
        )
        launches: list[dict[str, Any]] = []
        for index in range(2):
            output_path = work / f"relocated-report-{index + 1}.json"
            elapsed, launched = run_checked(
                f"relocated package launch {index + 1}",
                [
                    str(relocated / "run"),
                    dataset_path.name,
                    output_path.name,
                    "tag",
                ],
                work,
                runtime_environment,
                timeout=180,
            )
            actual_report = output_path.read_bytes()
            if actual_report != expected_report:
                raise RuntimeError(f"relocated launch {index + 1} report differs from reference")
            if launched.stdout != expected_stdout or launched.stderr:
                raise RuntimeError(
                    f"relocated launch {index + 1} console output differs from reference:\n"
                    f"stdout={launched.stdout!r}\nstderr={launched.stderr!r}"
                )
            launches.append(
                {
                    "elapsed_seconds": elapsed,
                    "report_bytes": len(actual_report),
                    "report_sha256": sha256(actual_report),
                    "stdout_matches_reference": True,
                    "stderr_empty": True,
                }
            )

        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-current-format-managed-package-analysis",
            "status": "passed",
            "captured_at_utc": datetime.now(timezone.utc).isoformat(),
            "base_revision": revision,
            "host": platform.platform(),
            "guion_version": doctor_report["guion"]["version"],
            "artifact_format": inspection["artifact_format"],
            "managed_runtime": {
                "cpython": python_metadata["version"],
                "uv": python_metadata["uv_version"],
                "runtime_bytes": package_python_runtime_bytes,
                "offline_cache_bytes": package_python_cache_bytes,
                "dependencies": dependency_versions,
            },
            "workload": {
                "rows": expected_value["count"],
                "groups": len(expected_value["groups"]),
                "group_by": "tag",
                "total": expected_value["total"],
                "input_bytes": len(dataset_bytes),
                "input_sha256": sha256(dataset_bytes),
                "report_bytes": len(expected_report),
                "report_sha256": sha256(expected_report),
                "caller_data_embedded_in_package": False,
                "source_embedded_in_package": False,
            },
            "package": {
                "relocated_to_path_with_spaces_and_unicode": True,
                "files": package_files,
                "size_bytes": package_bytes,
                "verified_before_relocation": True,
                "launches_without_external_python_or_uv_settings": launches,
            },
            "timings_seconds": {
                "standard_library_reference": reference_elapsed,
                "managed_toolchain_preflight": doctor_elapsed,
                "offline_dependency_sync": sync_elapsed,
                "package_assembly": package_elapsed,
                "package_verification": verify_elapsed,
            },
            "correctness": {
                "all_relocated_report_bytes_match_reference": True,
                "all_console_output_matches_reference": True,
                "all_launches_succeeded": True,
                "expected_stdout": expected_stdout,
            },
            "cleanup": {
                "run_scratch_removed_on_exit": True,
                "warmed_toolchain_cache_preserved": str(warmed.relative_to(ROOT)),
                "vm_started": args.vm_started,
            },
        }

    output = args.output if args.output.is_absolute() else ROOT / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "status": report["status"],
                "artifact_format": report["artifact_format"],
                "input_bytes": report["workload"]["input_bytes"],
                "package_bytes": report["package"]["size_bytes"],
                "launches": len(report["package"]["launches_without_external_python_or_uv_settings"]),
                "report": str(output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
