"""End-to-end state, command, and standard-library parity checks."""

from pathlib import Path
from shutil import copytree, rmtree
import os
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PROJECT = ROOT / "evaluation/development/task_ledger"
if not DEFAULT_PROJECT.is_dir():
    DEFAULT_PROJECT = ROOT / "examples/task_ledger"
PROJECT = Path(os.environ.get("GUION_TASK_LEDGER_PROJECT", DEFAULT_PROJECT))
GUION = Path(os.environ.get("GUION_BINARY", ROOT / "target/debug/guion"))


class TaskLedgerAcceptance(unittest.TestCase):
    def make_project(self, parent):
        project = parent / "task_ledger"
        copytree(PROJECT, project)
        return project

    def run_guion(self, project, *args):
        return subprocess.run(
            [str(GUION), "run", str(project), *args],
            cwd=project.parent,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def run_python(self, project, *args):
        return subprocess.run(
            ["python3", str(project / "python_baseline.py"), *args],
            cwd=project,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def compare(self, commands, missing_initial_state=False):
        with tempfile.TemporaryDirectory(prefix="guion-task-ledger-") as directory:
            root = Path(directory)
            (root / "guion").mkdir()
            (root / "python").mkdir()
            guion_project = self.make_project(root / "guion")
            python_project = self.make_project(root / "python")
            if missing_initial_state:
                (guion_project / "tasks.json").unlink(missing_ok=True)
                (python_project / "tasks.json").unlink(missing_ok=True)
            for arguments in commands:
                guion = self.run_guion(guion_project, *arguments)
                python = self.run_python(python_project, *arguments)
                self.assertEqual(guion.returncode, python.returncode, arguments)
                self.assertEqual(guion.stdout, python.stdout, arguments)
                self.assertEqual(guion.stderr, python.stderr, arguments)
                guion_state = guion_project / "tasks.json"
                python_state = python_project / "tasks.json"
                self.assertEqual(guion_state.exists(), python_state.exists(), arguments)
                if guion_state.exists():
                    self.assertEqual(guion_state.read_bytes(), python_state.read_bytes(), arguments)

    def test_persistence_and_all_mutations_match_python(self):
        self.compare(
            [
                ("list",),
                ("add", "Ship beta", "Check release notes"),
                ("add", "Café 東京", "Unicode survives storage"),
                ("summary",),
                ("done", "Ship beta"),
                ("list",),
                ("remove", "Café 東京"),
                ("summary",),
            ]
        )

    def test_first_run_creates_state_without_a_seed_file(self):
        self.compare(
            [("list",), ("add", "First task", "Created on first use"), ("list",)],
            missing_initial_state=True,
        )

    def test_duplicate_missing_and_bad_arguments_match_python(self):
        self.compare(
            [
                ("add", "same", "first"),
                ("add", "same", "second"),
                ("add", "", "empty title"),
                ("done", "missing"),
                ("remove", "missing"),
                ("add", "too", "many", "arguments"),
                (),
                ("unknown",),
            ]
        )

    def test_current_artifact_runs_without_source_and_keeps_state(self):
        with tempfile.TemporaryDirectory(prefix="guion-task-ledger-artifact-") as directory:
            root = Path(directory)
            (root / "guion").mkdir()
            (root / "python").mkdir()
            guion_project = self.make_project(root / "guion")
            python_project = self.make_project(root / "python")
            built = subprocess.run(
                [str(GUION), "build", str(guion_project)],
                cwd=root / "guion",
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            self.assertRegex(built.stdout, r"artifact format \d+")
            (guion_project / "main.gu").unlink()
            rmtree(guion_project / "src")

            for arguments in [
                ("add", "Built artifact", "No source files needed"),
                ("list",),
                ("done", "Built artifact"),
                ("summary",),
                ("remove", "Built artifact"),
            ]:
                guion = subprocess.run(
                    [str(GUION), "run", str(guion_project / "main.gub"), *arguments],
                    cwd=root / "guion",
                    text=True,
                    capture_output=True,
                    timeout=30,
                )
                python = self.run_python(python_project, *arguments)
                self.assertEqual(guion.returncode, python.returncode, arguments)
                self.assertEqual(guion.stdout, python.stdout, arguments)
                self.assertEqual(guion.stderr, python.stderr, arguments)
                self.assertEqual(
                    (guion_project / "tasks.json").read_bytes(),
                    (python_project / "tasks.json").read_bytes(),
                    arguments,
                )


if __name__ == "__main__":
    unittest.main()
