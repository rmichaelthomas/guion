"""Output parity checks for the current-format package-analysis app."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "evaluation/development/guion_package_analysis"
GUION = Path(os.environ.get("GUION_BINARY", ROOT / "target/release/guion")).resolve()
PYTHON = os.environ.get("GUION_PYTHON")
SCRATCH = ROOT / "target/perf-session"


class GuionPackageAnalysisAcceptance(unittest.TestCase):
    def test_text_and_bytes_bridge_inputs_match_both_python_references(self):
        self.assertTrue(PROJECT.is_dir(), f"current Guion project missing at {PROJECT}")
        if PYTHON is None:
            self.skipTest("set GUION_PYTHON to the pinned NumPy/orjson interpreter")
        self.assertTrue(GUION.is_file(), f"feature-enabled Guion CLI missing at {GUION}")
        SCRATCH.mkdir(parents=True, exist_ok=True)

        with tempfile.TemporaryDirectory(prefix="guion-package-analysis-", dir=SCRATCH) as name:
            work = Path(name)
            project = work / "project"
            shutil.copytree(PROJECT, project)
            rows = [
                {
                    "value": (index * 7919) % 50_003,
                    "tag": f"é-{index % 13}",
                    "department": f"dept-{index % 5}",
                }
                for index in range(100_000)
            ]
            (project / "dataset.json").write_text(
                json.dumps({"rows": rows}, ensure_ascii=False, separators=(",", ":")),
                encoding="utf-8",
            )
            environment = os.environ.copy()
            environment.update(
                {
                    "GUION_BINARY": str(GUION),
                    "GUION_PYTHON": str(Path(PYTHON).resolve()),
                    "PYTHONDONTWRITEBYTECODE": "1",
                }
            )

            def run(command: list[str], cwd: Path = project) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    command,
                    cwd=cwd,
                    env=environment,
                    capture_output=True,
                    text=True,
                    timeout=90,
                    check=False,
                )

            outputs: dict[str, tuple[str, bytes]] = {}
            for mode in ("text", "bytes"):
                report_path = f"{mode}-report.json"
                guion = run(
                    [
                        str(GUION),
                        "run",
                        "--no-cache",
                        "--file-root",
                        str(project),
                        f"main-{mode}.gu",
                        "dataset.json",
                        report_path,
                        "tag",
                    ]
                )
                self.assertEqual(guion.returncode, 0, guion.stderr)
                outputs[mode] = (guion.stdout, (project / report_path).read_bytes())

            package_python = run(
                [str(Path(PYTHON).resolve()), "-B", "reference_packages.py", "dataset.json", "packages-report.json", "tag"]
            )
            self.assertEqual(package_python.returncode, 0, package_python.stderr)
            outputs["packages"] = (
                package_python.stdout,
                (project / "packages-report.json").read_bytes(),
            )

            stdlib_python = run(
                [str(Path(PYTHON).resolve()), "-B", "reference.py", "dataset.json", "stdlib-report.json", "tag"]
            )
            self.assertEqual(stdlib_python.returncode, 0, stdlib_python.stderr)
            outputs["stdlib"] = (stdlib_python.stdout, (project / "stdlib-report.json").read_bytes())

            expected = outputs["stdlib"]
            self.assertEqual(outputs["packages"], expected)
            self.assertEqual(outputs["text"], expected)
            self.assertEqual(outputs["bytes"], expected)
            decoded = json.loads(expected[1])
            self.assertEqual(decoded["count"], 100_000)
            self.assertEqual(decoded["total"], 2_500_116_287)
            self.assertEqual(len(decoded["groups"]), 13)


if __name__ == "__main__":
    unittest.main()
