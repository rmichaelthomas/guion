"""Task graph planning acceptance and follow-up parity checks."""

from pathlib import Path
from shutil import copytree, rmtree
import json
import os
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "evaluation/development/task_graph"
if not PROJECT.is_dir():
    PROJECT = ROOT / "examples/task_graph"
VERITAS = Path(os.environ.get("VERITAS_BINARY", ROOT / "target/debug/veritas"))


class TaskGraphAcceptance(unittest.TestCase):
    def make_project(self, parent, name):
        project = parent / name
        copytree(PROJECT, project)
        return project

    def run_veritas(self, project, *args):
        return subprocess.run(
            [str(VERITAS), "run", str(project), *args],
            cwd=project.parent,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def run_python(self, project, *args):
        return subprocess.run(
            ["python3", str(project / "reference.py"), *args],
            cwd=project,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def compare(self, tasks, priority=False, expected_order=None, output_exists=True):
        with tempfile.TemporaryDirectory(prefix="veritas-task-graph-") as directory:
            root = Path(directory)
            veritas = self.make_project(root / "veritas", "app")
            python = self.make_project(root / "python", "app")
            input_value = {"tasks": tasks}
            (veritas / "tasks.json").write_text(json.dumps(input_value), encoding="utf-8")
            (python / "tasks.json").write_text(json.dumps(input_value), encoding="utf-8")
            arguments = ("tasks.json", "plan.json", "--priority") if priority else ("tasks.json", "plan.json")
            actual = self.run_veritas(veritas, *arguments)
            expected = self.run_python(python, *arguments)
            self.assertEqual(actual.returncode, expected.returncode)
            self.assertEqual(actual.stdout, expected.stdout)
            self.assertEqual(actual.stderr, expected.stderr)
            actual_output = veritas / "plan.json"
            expected_output = python / "plan.json"
            self.assertEqual(actual_output.exists(), output_exists)
            self.assertEqual(expected_output.exists(), output_exists)
            if output_exists:
                actual_plan = json.loads(actual_output.read_text(encoding="utf-8"))
                expected_plan = json.loads(expected_output.read_text(encoding="utf-8"))
                self.assertEqual(actual_plan, expected_plan)
                if expected_order is not None:
                    self.assertEqual(actual_plan["order"], expected_order)
            return actual

    def test_dependencies_are_ordered_deterministically(self):
        self.compare(
            [
                {"id": "ship", "depends_on": ["package"]},
                {"id": "test", "depends_on": ["compile"]},
                {"id": "docs", "depends_on": []},
                {"id": "package", "depends_on": ["test"]},
                {"id": "compile", "depends_on": []},
            ],
            expected_order=["compile", "docs", "test", "package", "ship"],
        )

    def test_priority_follow_up_changes_only_ready_task_tie_breaking(self):
        self.compare(
            [
                {"id": "ship", "depends_on": ["package"], "priority": 10},
                {"id": "test", "depends_on": ["compile"], "priority": 8},
                {"id": "docs", "depends_on": [], "priority": 4},
                {"id": "package", "depends_on": ["test"], "priority": 2},
                {"id": "compile", "depends_on": [], "priority": 1},
            ],
            priority=True,
            expected_order=["docs", "compile", "test", "package", "ship"],
        )

    def test_duplicate_missing_and_cyclic_dependencies_fail_without_output(self):
        cases = [
            ([{"id": "same", "depends_on": []}, {"id": "same", "depends_on": []}], "duplicate task id"),
            ([{"id": "compile", "depends_on": ["absent"]}], "unknown dependency"),
            ([{"id": "a", "depends_on": ["b"]}, {"id": "b", "depends_on": ["a"]}], "dependency cycle"),
        ]
        for tasks, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic):
                result = self.compare(tasks, output_exists=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(diagnostic, result.stdout)

    def test_artifact_runs_after_source_removal(self):
        with tempfile.TemporaryDirectory(prefix="veritas-task-graph-artifact-") as directory:
            root = Path(directory)
            project = self.make_project(root / "veritas", "app")
            (project / "tasks.json").write_text(
                json.dumps({"tasks": [{"id": "b", "depends_on": ["a"]}, {"id": "a", "depends_on": []}]}),
                encoding="utf-8",
            )
            built = subprocess.run(
                [str(VERITAS), "build", str(project)],
                cwd=root,
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            (project / "main.vr").unlink()
            result = subprocess.run(
                [str(VERITAS), "run", str(project / "main.vrb"), "tasks.json", "plan.json"],
                cwd=root,
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((project / "plan.json").read_text())["order"], ["a", "b"])


if __name__ == "__main__":
    unittest.main()
