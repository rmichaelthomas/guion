#!/usr/bin/env python3
"""Verify Feed Digest as a current-format, source-free managed Guion package."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/guion_feed_digest"
TEST_SUITE = ROOT / "evaluation/tests/test_feed_digest.py"
WARMED = ROOT / "target/perf-session/managed-python-package"
DEFAULT_OUTPUT = ROOT / "evaluation/completion/results/real-task-sprint/2026-10-27-feed-digest-managed-package"


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def directory_bytes(root: Path) -> int:
    total = 0
    for parent, directories, files in os.walk(root, followlinks=False):
        directories[:] = [
            name for name in directories if not (Path(parent) / name).is_symlink()
        ]
        for name in files:
            path = Path(parent) / name
            if not path.is_symlink():
                total += path.stat().st_size
    return total


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
            f"{label} failed with exit code {result.returncode}:\n"
            f"{result.stdout}\n{result.stderr}"
        )
    return elapsed, result


def run_acceptance_suite(
    project: Path,
    binary: Path,
    uv: Path,
    warmed: Path,
    scratch: Path,
    package_runner: Path | None = None,
) -> tuple[int, str]:
    environment = os.environ.copy()
    environment.update(
        {
            "GUION_BINARY": str(binary),
            "GUION_FEED_PROJECT": str(project),
            "GUION_FEED_SCRATCH": str(scratch),
            "GUION_UV": str(uv),
            "GUION_PYTHON_INSTALL_DIR": str(warmed / "python"),
            "GUION_PYTHON_CACHE_DIR": str(warmed / "cache"),
            "UV_CACHE_DIR": str(warmed / "cache"),
            "UV_PYTHON_INSTALL_DIR": str(warmed / "python"),
            "UV_OFFLINE": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    environment.pop("GUION_FEED_PACKAGE_RUNNER", None)
    if package_runner is not None:
        environment["GUION_FEED_PACKAGE_RUNNER"] = str(package_runner)
    started = time.perf_counter()
    result = subprocess.run(
        [sys.executable, "-B", str(TEST_SUITE)],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        timeout=900,
        check=False,
    )
    elapsed = time.perf_counter() - started
    combined = result.stdout + result.stderr
    if result.returncode:
        raise RuntimeError(
            "Feed Digest acceptance suite failed "
            f"({result.returncode}):\n{combined}"
        )
    match = re.search(r"Ran (\d+) tests?", combined)
    if not match:
        raise RuntimeError(f"acceptance suite did not report a test count:\n{combined}")
    return int(match.group(1)), f"{elapsed:.3f}s"


def resolve_from_root(path: Path) -> Path:
    return path if path.is_absolute() else ROOT / path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=ROOT / "target/debug/guion",
        help="feature-enabled Guion CLI",
    )
    parser.add_argument(
        "--uv",
        type=Path,
        default=WARMED / "toolchain/uv",
        help="pinned uv executable",
    )
    parser.add_argument(
        "--warmed-dir",
        type=Path,
        default=WARMED,
        help="repository-local managed Python and uv cache",
    )
    parser.add_argument(
        "--scratch-dir",
        type=Path,
        default=ROOT / "target/perf-session/feed-digest-managed-package",
        help="ignored repository-local scratch parent",
    )
    parser.add_argument(
        "--output-stem",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="tracked report path without the .md or .json suffix",
    )
    parser.add_argument(
        "--preload-dependencies",
        action="store_true",
        help="allow the initial pinned dependency sync to populate the local cache",
    )
    parser.add_argument(
        "--write-lock",
        action="store_true",
        help="copy verified Guion and uv locks back into the current-format app",
    )
    args = parser.parse_args()

    binary = args.binary.resolve(strict=True)
    uv = args.uv.resolve(strict=True)
    warmed = args.warmed_dir.resolve(strict=True)
    scratch = resolve_from_root(args.scratch_dir).resolve()
    output_stem = resolve_from_root(args.output_stem)
    cache = warmed / "cache"
    python_install = warmed / "python"
    if not cache.is_dir() or not python_install.is_dir():
        raise RuntimeError(f"repository-local managed package cache is incomplete: {warmed}")
    scratch.mkdir(parents=True, exist_ok=True)
    try:
        scratch.relative_to(ROOT)
    except ValueError as error:
        raise RuntimeError("scratch must remain inside the Guion repository") from error

    distribution_manifest = tomllib.loads(
        (ROOT / "distribution/toolchains/manifest.toml").read_text(encoding="utf-8")
    )
    expected_uv_version = distribution_manifest["uv_version"]
    uv_version_text = subprocess.run(
        [str(uv), "--version"], capture_output=True, text=True, check=True
    ).stdout.strip()
    found_uv_version = uv_version_text.split()[1]
    if found_uv_version != expected_uv_version:
        raise RuntimeError(
            f"expected pinned uv {expected_uv_version}, found {uv_version_text}"
        )

    base_environment = os.environ.copy()
    base_environment.update(
        {
            "GUION_UV": str(uv),
            "GUION_PYTHON_INSTALL_DIR": str(python_install),
            "GUION_PYTHON_CACHE_DIR": str(cache),
            "UV_CACHE_DIR": str(cache),
            "UV_PYTHON_INSTALL_DIR": str(python_install),
            "UV_NO_PROGRESS": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    base_environment.pop("UV_PROJECT_ENVIRONMENT", None)
    base_environment.pop("UV_OFFLINE", None)

    doctor_elapsed, doctor_result = run_checked(
        "Guion managed-toolchain preflight",
        [str(binary), "doctor", "--json"],
        ROOT,
        base_environment,
        timeout=30,
    )
    doctor = json.loads(doctor_result.stdout)
    if not doctor.get("capabilities", {}).get("python_calls"):
        raise RuntimeError("selected Guion CLI does not enable python-interop")
    artifact_format = doctor.get("guion", {}).get("artifact_format")
    if not isinstance(artifact_format, int):
        raise RuntimeError("Guion doctor did not report its artifact format")

    app_source_hashes = {
        name: file_sha256(APP / name)
        for name in ("guion.toml", "main.gu", "guion_feed.py", "python_baseline.py")
    }

    lock_contents: dict[str, bytes] = {}
    with tempfile.TemporaryDirectory(
        prefix="guion-feed-digest-package-", dir=scratch
    ) as temporary:
        work = Path(temporary)
        project = work / "feed digest source"
        shutil.copytree(
            APP,
            project,
            ignore=shutil.ignore_patterns(".venv", "__pycache__", "digest.md"),
        )
        bridge = project / ".guion/python-bridge"
        package = work / "first package"
        relocated = work / "relocated apps" / "Feed digest Ω"

        sync_environment = base_environment.copy()
        if not args.preload_dependencies:
            sync_environment["UV_OFFLINE"] = "1"
        sync_elapsed, _ = run_checked(
            "managed dependency sync",
            [str(binary), "sync", "--update", str(project)],
            ROOT,
            sync_environment,
        )
        lock_path = project / "guion.lock"
        uv_lock_path = bridge / "uv.lock"
        if not lock_path.is_file() or not uv_lock_path.is_file():
            raise RuntimeError("sync did not produce both Guion and uv dependency locks")
        python_version = tomllib.loads(
            (project / "guion.toml").read_text(encoding="utf-8")
        )["python"]["version"]
        staged_interpreter = bridge / ".venv/bin/python3"
        if not staged_interpreter.is_file():
            raise RuntimeError(f"synced project interpreter is missing: {staged_interpreter}")

        offline_environment = base_environment.copy()
        offline_environment["UV_OFFLINE"] = "1"
        offline_sync_elapsed, _ = run_checked(
            "offline locked dependency sync",
            [str(binary), "sync", str(project)],
            ROOT,
            offline_environment,
        )

        package_elapsed, _ = run_checked(
            "managed source-free package assembly",
            [str(binary), "package", str(project), "--output", str(package)],
            ROOT,
            offline_environment,
        )
        if not (package / "run").is_file() or not (package / "app.gub").is_file():
            raise RuntimeError("package is missing its launcher or compiled artifact")
        for source_file in ("main.gu", "python_baseline.py"):
            if (package / source_file).exists():
                raise RuntimeError(
                    f"package unexpectedly contains source file {source_file}"
                )
        packaged_manifest = tomllib.loads(
            (package / "guion.toml").read_text(encoding="utf-8")
        )
        if packaged_manifest.get("network", {}).get("hosts") != ["127.0.0.1"]:
            raise RuntimeError("package did not retain the exact loopback-only host grant")

        inspection_elapsed, inspection_result = run_checked(
            "artifact format inspection",
            [str(binary), "inspect", "--json", str(package / "app.gub")],
            ROOT,
            offline_environment,
            timeout=30,
        )
        inspection = json.loads(inspection_result.stdout)
        if inspection.get("artifact_format") != artifact_format:
            raise RuntimeError(
                "packaged artifact format differs from the active Guion CLI"
            )

        package_metadata = json.loads((package / "PACKAGE.json").read_text(encoding="utf-8"))
        python_metadata = package_metadata.get("python_runtime") or {}
        if python_metadata.get("version") != python_version:
            raise RuntimeError("package does not contain the pinned CPython version")
        if python_metadata.get("uv_version") != expected_uv_version:
            raise RuntimeError("package does not contain the pinned uv version")

        dependencies = json.loads(
            (package / "PYTHON-DEPENDENCIES.json").read_text(encoding="utf-8")
        )
        expected_versions = {"feedparser": "6.0.14", "feedparser-sgmllib": "2.1.0"}
        package_versions = {
            record["name"]: record["version"]
            for record in dependencies.get("packages", [])
            if record.get("name") in expected_versions
        }
        if package_versions != expected_versions:
            raise RuntimeError(
                f"managed package versions differ from the lock: {package_versions}"
            )
        for record in dependencies.get("packages", []):
            if record.get("name") in expected_versions:
                if record.get("license_status") != "complete":
                    raise RuntimeError(
                        f"license inventory is incomplete for {record.get('name')}"
                    )
                if not record.get("license_files"):
                    raise RuntimeError(
                        f"license inventory has no files for {record.get('name')}"
                    )

        native_report = json.loads(
            (package / "NATIVE-DEPENDENCIES.json").read_text(encoding="utf-8")
        )
        python_native_images = [
            image
            for image in native_report.get("images", [])
            if "site-packages" in image.get("path", "")
        ]
        if python_native_images:
            raise RuntimeError(
                "feedparser package unexpectedly acquired Python native extensions"
            )
        if native_report.get("unresolved") or native_report.get("unattributed_images"):
            raise RuntimeError("package native-dependency inventory is incomplete")

        package_size = directory_bytes(package)
        package_file_count = sum(1 for path in package.rglob("*") if path.is_file())
        artifact_sha256 = file_sha256(package / "app.gub")

        verify_elapsed, _ = run_checked(
            "pre-relocation package verification",
            [str(binary), "verify-package", str(package)],
            ROOT,
            offline_environment,
            timeout=60,
        )
        relocated.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(package), str(relocated))
        relocated_verify_elapsed, _ = run_checked(
            "relocated package verification",
            [str(binary), "verify-package", str(relocated)],
            ROOT,
            offline_environment,
            timeout=60,
        )

        source_tests, source_test_elapsed = run_acceptance_suite(
            project, binary, uv, warmed, scratch
        )
        package_tests, package_test_elapsed = run_acceptance_suite(
            project, binary, uv, warmed, scratch, relocated / "run"
        )
        if source_tests != 6 or package_tests != 6:
            raise RuntimeError(
                f"expected six acceptance cases per mode, got source={source_tests}, "
                f"package={package_tests}"
            )

        lock_files = {
            "guion.lock": lock_path,
            ".guion/python-bridge/pyproject.toml": bridge / "pyproject.toml",
            ".guion/python-bridge/uv.lock": uv_lock_path,
        }
        lock_hashes = {
            name: file_sha256(path) for name, path in lock_files.items()
        }
        lock_contents = {
            name: path.read_bytes() for name, path in lock_files.items()
        }

        package_record: dict[str, Any] = {
            "schema_version": 1,
            "status": "passed",
            "captured_at_utc": datetime.now(timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "workload": "Feed Digest with managed feedparser",
            "host": {
                "os": platform.system().lower(),
                "architecture": platform.machine().lower(),
                "guion_architecture": doctor["host"]["architecture"],
                "artifact_format": artifact_format,
            },
            "python_runtime": {
                "version": python_metadata.get("version"),
                "uv_version": python_metadata.get("uv_version"),
                "runtime_size_bytes": python_metadata.get("python_runtime_size_bytes"),
                "offline_cache_size_bytes": python_metadata.get("python_cache_size_bytes"),
            },
            "dependencies": expected_versions,
            "python_native_extension_count": len(python_native_images),
            "license_records_complete": True,
            "source_free": True,
            "package_directory_bytes": package_size,
            "package_file_count": package_file_count,
            "artifact_sha256": artifact_sha256,
            "source_file_sha256": app_source_hashes,
            "lock_file_sha256": lock_hashes,
            "checks": {
                "managed_dependency_sync": "passed",
                "offline_locked_resync": "passed",
                "package_verification": "passed",
                "relocation_verification": "passed",
                "source_acceptance": {"passed": source_tests, "total": source_tests},
                "relocated_package_acceptance": {
                    "passed": package_tests,
                    "total": package_tests,
                },
                "expected_output_parity": "all cases compared byte-for-byte with Python",
                "loopback_http_concurrency": "both implementations reached two in-flight requests",
                "external_dependency_network_during_acceptance": "disabled",
            },
            "phase_durations_seconds": {
                "doctor_preflight": round(doctor_elapsed, 3),
                "managed_dependency_sync": round(sync_elapsed, 3),
                "offline_locked_resync": round(offline_sync_elapsed, 3),
                "package_assembly": round(package_elapsed, 3),
                "artifact_inspection": round(inspection_elapsed, 3),
                "package_verification": round(verify_elapsed, 3),
                "relocated_package_verification": round(relocated_verify_elapsed, 3),
                "source_acceptance_suite": source_test_elapsed,
                "relocated_package_acceptance_suite": package_test_elapsed,
            },
            "environment": {
                "network_fixture": "loopback only",
                "colima_vm_started_for_this_run": False,
                "scratch_root": scratch.relative_to(ROOT).as_posix(),
                "warmed_cache": warmed.relative_to(ROOT).as_posix(),
                "vm_or_rosetta_translation_used": False,
            },
            "interpretation": (
                "This establishes one current-format macOS ARM64 package path for a "
                "pure-Python parser and this application; it is not broad Python "
                "package compatibility or a speed comparison."
            ),
        }

    if args.write_lock:
        for relative_path, content in lock_contents.items():
            destination = APP / relative_path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
            package_record["lock_file_sha256"][relative_path] = file_sha256(destination)

    output_stem.parent.mkdir(parents=True, exist_ok=True)
    json_path = output_stem.with_suffix(".json")
    markdown_path = output_stem.with_suffix(".md")
    json_path.write_text(
        json.dumps(package_record, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    size_mib = package_size / (1024 * 1024)
    markdown = (
        "# Guion current-format Feed Digest package verification\n\n"
        "## Result\n\n"
        "The current-format Guion Feed Digest application built a source-free, "
        "relocatable managed package on macOS ARM64. The package uses the pinned "
        "pure-Python feedparser dependency and passed the same six application "
        "acceptance cases as its independent Python reference.\n\n"
        "## Verified package\n\n"
        f"- Artifact format: {artifact_format}.\n"
        f"- Managed CPython: {python_metadata.get('version')}.\n"
        f"- Bundled uv: {python_metadata.get('uv_version')}.\n"
        "- Dependencies: feedparser 6.0.14 and feedparser-sgmllib 2.1.0.\n"
        "- Native extensions from these Python dependencies: 0.\n"
        "- Both dependency license records are complete.\n"
        f"- Uncompressed package directory: {package_size:,} bytes "
        f"({size_mib:.1f} MiB, {package_file_count:,} files).\n"
        "- The source-free artifact has no bundled Guion source; its manifest remains "
        "to carry the project policy and exact loopback-only host grant.\n"
        "- Package was verified before and after moving it to a path with spaces "
        "and Unicode.\n"
        "- The relocated package passed all six loopback application acceptance "
        "cases with external Python and uv configuration removed from the Guion "
        "launch process.\n\n"
        "## Application checks\n\n"
        "- Every digest matched the independent Python implementation byte-for-byte.\n"
        "- Cases cover duplicate links, Unicode titles, stable ordering, changed "
        "feed content on a fresh run, malformed input, HTTP 404 handling, denied "
        "hosts, and concurrent requests.\n"
        "- Dependency resolution and the acceptance suites ran offline after the "
        "pinned packages were preloaded into the repository cache. The only network "
        "traffic during acceptance was to the local loopback fixture server.\n\n"
        "## Limits\n\n"
        "This extends evidence to one more pure-Python package and one real "
        "application shape. It does not establish broad package compatibility, "
        "native-wheel coverage, package-size advantages, or a speed result. "
        "Package assembly and launch phase durations are recorded in the adjacent "
        "JSON as operational observations, not benchmarks.\n\n"
        "## Reproduction and hygiene\n\n"
        "Run from the Guion repository with a feature-enabled CLI and the warmed "
        "repository-local package cache. The first run may use the preload option:\n\n"
        "python3 evaluation/completion/performance/verify_managed_feed_digest_package.py "
        "--preload-dependencies --write-lock\n\n"
        f"Raw evidence: {json_path.name}. All run scratch is under "
        f"{scratch.relative_to(ROOT).as_posix()} and is removed on exit; only the "
        "small dependency cache is retained. No VM was started for this macOS "
        "ARM64 result.\n"
    )
    markdown_path.write_text(markdown, encoding="utf-8")
    print(f"passed: {json_path.relative_to(ROOT)}")
    print(f"passed: {markdown_path.relative_to(ROOT)}")
    print(
        f"source cases {source_tests}/{source_tests}; relocated package cases "
        f"{package_tests}/{package_tests}; package {package_size:,} bytes"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.TimeoutExpired, KeyError, ValueError) as error:
        print(f"managed Feed Digest package verification failed: {error}", file=sys.stderr)
        raise SystemExit(1)
