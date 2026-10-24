"""Independent end-to-end parity checks for the inventory reconciliation app."""

from pathlib import Path
from shutil import copytree, rmtree
import json
import os
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "evaluation/development/inventory_reconcile"
if not PROJECT.is_dir():
    PROJECT = ROOT / "examples/inventory_reconcile"
DEFAULT_VERITAS = ROOT / "veritas" if (ROOT / "veritas").is_file() else ROOT / "target/debug/veritas"
VERITAS = Path(os.environ.get("VERITAS_BINARY", DEFAULT_VERITAS))


class InventoryReconcileAcceptance(unittest.TestCase):
    def copy_project(self, parent):
        parent.mkdir(parents=True, exist_ok=True)
        project = parent / "inventory_reconcile"
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

    def compare(self, inventory, adjustments, expect_success=True):
        with tempfile.TemporaryDirectory(prefix="veritas-inventory-reconcile-") as directory:
            root = Path(directory)
            veritas_project = self.copy_project(root / "veritas")
            python_project = self.copy_project(root / "python")
            for project in (veritas_project, python_project):
                (project / "inventory.json").write_text(
                    json.dumps(inventory, ensure_ascii=False), encoding="utf-8"
                )
                (project / "adjustments.json").write_text(
                    json.dumps(adjustments, ensure_ascii=False), encoding="utf-8"
                )
                if not expect_success:
                    (project / "reconciled.json").write_bytes(b"previous result\n")
            arguments = ("inventory.json", "adjustments.json", "reconciled.json")
            veritas = self.run_veritas(veritas_project, *arguments)
            python = self.run_python(python_project, *arguments)
            self.assertEqual(veritas.returncode, python.returncode)
            self.assertEqual(veritas.stdout, python.stdout)
            self.assertEqual(veritas.stderr, python.stderr)
            self.assertEqual(veritas.returncode == 0, expect_success)
            veritas_result = veritas_project / "reconciled.json"
            python_result = python_project / "reconciled.json"
            self.assertEqual(veritas_result.exists(), python_result.exists())
            if veritas_result.exists():
                self.assertEqual(veritas_result.read_bytes(), python_result.read_bytes())
                if not expect_success:
                    self.assertEqual(veritas_result.read_bytes(), b"previous result\n")
                    return None
                return json.loads(veritas_result.read_text(encoding="utf-8"))
            return None

    def test_reconciles_duplicates_new_skus_and_unicode_deterministically(self):
        result = self.compare(
            {
                "items": [
                    {"sku": "A-1", "quantity": 10},
                    {"sku": "café-東京", "quantity": 4},
                ]
            },
            {
                "adjustments": [
                    {"sku": "A-1", "delta": -3, "reason": "damaged", "source": "cycle-count"},
                    {"sku": "NEW", "delta": 7, "reason": "initial stock", "source": "receiving"},
                    {"sku": "A-1", "delta": 2, "reason": "found units", "source": "cycle-count"},
                    {"sku": "café-東京", "delta": -4, "reason": "expired", "source": "inspection"},
                    {"sku": "NEW", "delta": 5, "reason": "late delivery", "source": "receiving"},
                ]
            },
        )
        self.assertEqual(
            result,
            {
                "items": {"A-1": 9, "NEW": 12, "café-東京": 0},
                "changes": [
                    {"sku": "A-1", "delta": -3, "reason": "damaged", "source": "cycle-count", "balance": 7},
                    {"sku": "NEW", "delta": 7, "reason": "initial stock", "source": "receiving", "balance": 7},
                    {"sku": "A-1", "delta": 2, "reason": "found units", "source": "cycle-count", "balance": 9},
                    {"sku": "café-東京", "delta": -4, "reason": "expired", "source": "inspection", "balance": 0},
                    {"sku": "NEW", "delta": 5, "reason": "late delivery", "source": "receiving", "balance": 12},
                ],
            },
        )

    def test_rejects_negative_inventory_without_publishing_partial_output(self):
        self.assertIsNone(
            self.compare(
                {"items": [{"sku": "A-1", "quantity": 2}]},
                {"adjustments": [{"sku": "A-1", "delta": -3, "reason": "damaged", "source": "inspection"}]},
                expect_success=False,
            )
        )

    def test_handles_generated_input_at_a_larger_size(self):
        inventory = {
            "items": [
                {"sku": f"SKU-{index:03}", "quantity": index + 10}
                for index in range(80)
            ]
        }
        adjustments = {
            "adjustments": [
                {
                    "sku": f"SKU-{(index * 37) % 80:03}",
                    "delta": (index % 7) - 3,
                    "reason": f"count-{index}",
                    "source": "cycle-count",
                }
                for index in range(600)
            ]
            + [
                {"sku": f"NEW-{index:03}", "delta": index + 1, "reason": "opening stock", "source": "receiving"}
                for index in range(20)
            ]
        }
        result = self.compare(inventory, adjustments)
        self.assertEqual(len(result["items"]), 100)
        self.assertEqual(len(result["changes"]), 620)
        self.assertEqual(result["changes"][-1]["balance"], 20)

    def test_current_artifact_reconciles_after_source_removal(self):
        with tempfile.TemporaryDirectory(prefix="veritas-inventory-artifact-") as directory:
            root = Path(directory)
            veritas_project = self.copy_project(root / "veritas")
            python_project = self.copy_project(root / "python")
            inventory = {"items": [{"sku": "A", "quantity": 4}]}
            adjustments = {
                "adjustments": [
                    {"sku": "A", "delta": 3, "reason": "counted", "source": "cycle-count"}
                ]
            }
            for project in (veritas_project, python_project):
                (project / "inventory.json").write_text(json.dumps(inventory), encoding="utf-8")
                (project / "adjustments.json").write_text(json.dumps(adjustments), encoding="utf-8")
            built = subprocess.run(
                [str(VERITAS), "build", str(veritas_project)],
                cwd=root / "veritas",
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            (veritas_project / "main.vr").unlink()
            rmtree(veritas_project / "src")
            arguments = ("inventory.json", "adjustments.json", "reconciled.json")
            veritas = subprocess.run(
                [str(VERITAS), "run", str(veritas_project / "main.vrb"), *arguments],
                cwd=root / "veritas",
                text=True,
                capture_output=True,
                timeout=30,
            )
            python = self.run_python(python_project, *arguments)
            self.assertEqual(veritas.returncode, python.returncode)
            self.assertEqual(veritas.stdout, python.stdout)
            self.assertEqual(veritas.stderr, python.stderr)
            self.assertEqual(
                (veritas_project / "reconciled.json").read_bytes(),
                (python_project / "reconciled.json").read_bytes(),
            )


if __name__ == "__main__":
    unittest.main()
