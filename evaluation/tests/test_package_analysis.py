"""Large-data and relocatable native-wheel checks for package-backed analysis."""

from __future__ import annotations

import json
import os
from pathlib import Path
from shutil import copytree, ignore_patterns, move, rmtree
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "evaluation/development/package_analysis"
if not PROJECT.is_dir():
    PROJECT = ROOT / "examples/package_analysis"
DEFAULT_VERITAS = ROOT / "veritas" if (ROOT / "veritas").is_file() else ROOT / "target/debug/veritas"
VERITAS = Path(os.environ.get("VERITAS_BINARY", DEFAULT_VERITAS))
UV = os.environ.get("VERITAS_UV")
UV_CACHE = os.environ.get("UV_CACHE_DIR")
PACKAGE_CACHE = os.environ.get("VERITAS_PACKAGE_UV_CACHE", UV_CACHE)
PYTHON_INSTALL = os.environ.get("UV_PYTHON_INSTALL_DIR")


class PackageAnalysisAcceptance(unittest.TestCase):
    @unittest.skipUnless(UV and UV_CACHE and PYTHON_INSTALL, "requires the pinned managed Python toolchain")
    def test_generated_large_dataset_matches_python_across_repeat_and_relocation(self):
        with tempfile.TemporaryDirectory(prefix="veritas-package-analysis-") as directory:
            root = Path(directory)
            veritas_project = root / "veritas" / "package_analysis"
            python_project = root / "python" / "package_analysis"
            copytree(PROJECT, veritas_project, ignore=ignore_patterns("__pycache__", ".venv"))
            copytree(PROJECT, python_project, ignore=ignore_patterns("__pycache__", ".venv"))
            rows = [
                {
                    "value": (index * 7919) % 50_003,
                    "tag": f"é-{index % 13}",
                    "department": f"dept-{index % 5}",
                }
                for index in range(100_000)
            ]
            dataset = json.dumps({"rows": rows}, ensure_ascii=False, separators=(",", ":"))
            (veritas_project / "dataset.json").write_text(dataset, encoding="utf-8")
            (python_project / "dataset.json").write_text(dataset, encoding="utf-8")
            environment = os.environ.copy()
            environment.update({
                "VERITAS_UV": UV,
                "UV_CACHE_DIR": UV_CACHE,
                "UV_PYTHON_INSTALL_DIR": PYTHON_INSTALL,
            })

            synced = subprocess.run(
                [str(VERITAS), "sync", str(veritas_project)], cwd=root, env=environment,
                text=True, capture_output=True, timeout=120,
            )
            self.assertEqual(synced.returncode, 0, synced.stderr)
            environment["UV_OFFLINE"] = "1"
            run_command = [str(VERITAS), "run", str(veritas_project), "dataset.json", "report.json", "tag"]
            veritas = subprocess.run(run_command, cwd=root, env=environment, text=True, capture_output=True, timeout=60)
            python = subprocess.run(
                ["python3", str(python_project / "reference.py"), "dataset.json", "report.json", "tag"],
                cwd=python_project, text=True, capture_output=True, timeout=60,
            )
            self.assertEqual(veritas.returncode, 0, veritas.stderr)
            self.assertEqual(veritas.returncode, python.returncode)
            self.assertEqual(veritas.stdout, python.stdout)
            self.assertEqual(veritas.stderr, python.stderr)
            self.assertEqual((veritas_project / "report.json").read_bytes(), (python_project / "report.json").read_bytes())
            result = json.loads((veritas_project / "report.json").read_text())
            self.assertEqual(result["count"], 100_000)
            self.assertEqual(result["total"], sum(row["value"] for row in rows))
            self.assertEqual(len(result["groups"]), 13)
            self.assertEqual(sum(group["count"] for group in result["groups"].values()), 100_000)

            repeated = subprocess.run(run_command, cwd=root, env=environment, text=True, capture_output=True, timeout=60)
            self.assertEqual(repeated.returncode, 0, repeated.stderr)
            self.assertEqual((veritas_project / "report.json").read_bytes(), (python_project / "report.json").read_bytes())

            package = root / "first-package"
            environment["UV_CACHE_DIR"] = PACKAGE_CACHE
            environment.pop("UV_OFFLINE", None)
            packaged = subprocess.run(
                [str(VERITAS), "package", str(veritas_project), "--output", str(package)],
                cwd=root, env=environment, text=True, capture_output=True, timeout=300,
            )
            self.assertEqual(packaged.returncode, 0, packaged.stderr)
            relocated = root / "moved" / "package-analysis"
            relocated.parent.mkdir()
            move(str(package), str(relocated))
            rmtree(veritas_project)
            launched = subprocess.run(
                [str(relocated / "run"), "dataset.json", "report.json", "department"],
                cwd=root, env=environment, text=True, capture_output=True, timeout=120,
            )
            department_reference = subprocess.run(
                ["python3", str(python_project / "reference.py"), "dataset.json", "department-report.json", "department"],
                cwd=python_project, text=True, capture_output=True, timeout=60,
            )
            self.assertEqual(launched.returncode, 0, launched.stderr)
            self.assertEqual(department_reference.returncode, 0, department_reference.stderr)
            self.assertEqual(launched.stdout, department_reference.stdout)
            self.assertEqual((relocated / "report.json").read_bytes(), (python_project / "department-report.json").read_bytes())
            relocated_result = json.loads((relocated / "report.json").read_text())
            self.assertEqual(len(relocated_result["groups"]), 5)


if __name__ == "__main__":
    unittest.main()
