#!/usr/bin/env python3
"""Measure clean local installation and staging for matched applications."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.compare_real_tasks import APP_IDS, APPS, PYTHON_REFERENCE  # noqa: E402


def timed(command: list[str], *, env: dict[str, str] | None = None) -> float:
    started = time.perf_counter()
    subprocess.run(command, env=env, capture_output=True, text=True, check=True, timeout=180)
    return time.perf_counter() - started


def file_bytes(root: Path) -> int:
    return sum(path.stat().st_size for path in root.rglob("*") if path.is_file() and not path.is_symlink())


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--wheels", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    archive = args.archive.resolve(strict=True)
    wheels = args.wheels.resolve(strict=True)
    scratch = ROOT / "target/perf-session"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="task-setup-", dir=scratch) as tmp:
        work = Path(tmp)
        extract = work / "extract"
        extract.mkdir()
        environment = os.environ.copy()
        environment["PATH"] = "/usr/bin:/bin"
        archive_seconds = timed(["/usr/bin/tar", "-xzf", str(archive), "-C", str(extract)], env=environment)
        packages = list(extract.iterdir())
        if len(packages) != 1:
            raise RuntimeError("release archive must contain exactly one root directory")
        prefix = work / "guion-install"
        install_seconds = timed(["/bin/sh", str(packages[0] / "install.sh"), str(prefix)], env=environment)
        timed([str(prefix / "bin/guion"), "--version"], env=environment)
        venv = work / "python-venv"
        venv_seconds = timed([sys.executable, "-m", "venv", str(venv)])
        pip_seconds = timed([str(venv / "bin/python3"), "-m", "pip", "install", "--no-index", "--find-links",
                             str(wheels), "-r", str(ROOT / "evaluation/completion/requirements-real-task.txt")])
        results = {"schema_version": 1, "method": "one warm-OS-cache local observation; network download excluded",
                   "guion_archive_bytes": archive.stat().st_size,
                   "guion_archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                   "guion_extract_seconds": archive_seconds, "guion_install_seconds": install_seconds,
                   "guion_installed_file_bytes": file_bytes(prefix),
                   "python_venv_create_seconds": venv_seconds, "python_calendar_dependencies_install_seconds": pip_seconds,
                   "python_venv_file_bytes_including_calendar_dependencies": file_bytes(venv),
                   "python_interpreter_assumed_preinstalled": sys.executable, "applications": {}}
        for name in APP_IDS:
            staged = work / (name + "-guion")
            started = time.perf_counter()
            staged.mkdir()
            for filename in ("main.vr", "veritas.toml", "veritas.lock"):
                shutil.copy2(APPS / name / filename, staged / filename)
            guion_seconds = time.perf_counter() - started
            python_staged = work / (name + "-python")
            started = time.perf_counter()
            python_staged.mkdir()
            shutil.copy2(PYTHON_REFERENCE / "main.py", python_staged / "main.py")
            python_seconds = time.perf_counter() - started
            application_package = work / (name + "-package")
            package_seconds = timed([str(prefix / "bin/guion"), "package", str(staged),
                                     "--output", str(application_package)], env=environment)
            timed([str(prefix / "bin/guion"), "verify-package", str(application_package)], env=environment)
            results["applications"][name] = {
                "guion_staging_seconds": guion_seconds, "guion_source_bytes": file_bytes(staged),
                "python_staging_seconds": python_seconds, "python_reference_bytes": file_bytes(python_staged),
                "guion_package_seconds": package_seconds, "guion_package_file_bytes": file_bytes(application_package),
                "guion_package_verified": True,
                "additional_dependency": "icalendar and its dependencies" if name == "calendar-conflicts" else None,
            }
            shutil.rmtree(application_package)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({key: value for key, value in results.items() if key != "applications"}, indent=2))


if __name__ == "__main__":
    main()
