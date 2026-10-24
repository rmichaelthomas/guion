"""Canonical parity and native-path checks for Veritas Richards."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "evaluation/development/famous_benchmarks/richards.vr"
VERITAS = Path(os.environ.get("VERITAS_BINARY", ROOT / "target/release/veritas"))


class RichardsVeritasTests(unittest.TestCase):
    def test_veritas_richards_matches_pinned_counts_in_source_and_artifact(self):
        with tempfile.TemporaryDirectory(prefix="veritas-richards-test-") as temp:
            project = Path(temp)
            source = project / "richards.vr"
            shutil.copy2(SOURCE, source)
            (project / "veritas.toml").write_text(
                '[project]\nname = "richards-probe"\nversion = "0.1.0"\n'
                'entry = "richards.vr"\n\n[runtime]\ninstruction_limit = 30000000\n',
                encoding="utf-8",
            )
            build = subprocess.run(
                [str(VERITAS), "build", str(source)],
                cwd=project,
                text=True,
                capture_output=True,
                timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stderr)
            artifact = source.with_suffix(".vrb")

            source_report = self.run_json(project)
            source.unlink()
            artifact_report = self.run_json(artifact)
            for report in (source_report, artifact_report):
                self.assertEqual(report["status"], "passed", report)
                self.assertEqual(report["program_stdout"], "9297\n23246\n")
                native = report["native_backend"]
                self.assertGreater(native["calls"], 0, native)
                self.assertIn("next_worker_count", native["compiled_functions"])
                self.assertEqual(native["deoptimizations"], 0, native)

    def run_json(self, program: Path) -> dict[str, object]:
        result = subprocess.run(
            [str(VERITAS), "run", "--json", str(program)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)


if __name__ == "__main__":
    unittest.main()
