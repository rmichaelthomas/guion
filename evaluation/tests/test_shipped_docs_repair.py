"""Verify an installed release provides enough feedback and docs to repair a typo.

Run against an installed release, outside the source checkout:

    VERITAS_BINARY="/prefix/bin/veritas" \
    VERITAS_INSTALL_ROOT="/prefix/share/veritas/veritas-0.1.0-darwin-arm64" \
    python3 "$VERITAS_INSTALL_ROOT/evaluation/tests/test_shipped_docs_repair.py"
"""

from pathlib import Path
import json
import hashlib
import os
import re
import subprocess
import tempfile
import unittest


INSTALL_ROOT = Path(os.environ.get("VERITAS_INSTALL_ROOT", "."))
VERITAS = Path(os.environ.get("VERITAS_BINARY", INSTALL_ROOT / "veritas"))


class ShippedDocsRepairAcceptance(unittest.TestCase):
    def setUp(self):
        if "VERITAS_INSTALL_ROOT" not in os.environ:
            self.skipTest("requires an installed release with its shipped docs and examples")

    def test_machine_diagnostic_and_shipped_host_docs_repair_a_program(self):
        self.assertTrue(VERITAS.is_file(), f"installed Veritas binary missing: {VERITAS}")
        reference = INSTALL_ROOT / "docs/reference.md"
        host_api = INSTALL_ROOT / "language/host-api.md"
        self.assertTrue(reference.is_file(), f"shipped reference missing: {reference}")
        self.assertTrue(host_api.is_file(), f"shipped host API missing: {host_api}")
        self.assertIn("check", reference.read_text(encoding="utf-8"))
        self.assertIn("print(value: Any) -> Unit", host_api.read_text(encoding="utf-8"))

        with tempfile.TemporaryDirectory(prefix="veritas shipped docs repair ") as temp:
            project = Path(temp) / "repair sample"
            project.mkdir()
            source = project / "main.vr"
            source.write_text(
                'fn main() -> Int:\n    prnit("repaired")\n    return 0\n\n'
                "fn test_example() -> Unit:\n    assert(2 + 2 == 4)\n",
                encoding="utf-8",
            )
            broken = subprocess.run(
                [str(VERITAS), "check", str(source), "--json"],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(broken.returncode, 1, broken.stderr)
            diagnostic = json.loads(broken.stdout)
            self.assertEqual(diagnostic["kind"], "veritas_diagnostic")
            self.assertEqual(diagnostic["diagnostic"]["code"], "E_NAME_UNKNOWN")
            self.assertEqual(diagnostic["diagnostic"]["line"], 2)

            # Use the shipped host API's documented output function to repair the typo.
            source.write_text(
                source.read_text(encoding="utf-8").replace("prnit(", "print("),
                encoding="utf-8",
            )
            checked = subprocess.run(
                [str(VERITAS), "check", str(source), "--json"],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)
            self.assertEqual(json.loads(checked.stdout)["kind"], "veritas_check_report")

            run = subprocess.run(
                [str(VERITAS), "run", "--json", str(source)],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            run_report = json.loads(run.stdout)
            self.assertEqual(run_report["kind"], "veritas_run_report")
            self.assertEqual(run_report["program_stdout"], "repaired\n")

            tests = subprocess.run(
                [str(VERITAS), "test", str(source), "--json"],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(tests.returncode, 0, tests.stdout + tests.stderr)
            self.assertEqual(json.loads(tests.stdout)["summary"]["passed"], 1)

    def test_shipped_python_abi_matches_relocatable_package_behavior(self):
        abi = INSTALL_ROOT / "language/python-abi.md"
        self.assertTrue(abi.is_file(), f"shipped Python ABI guide missing: {abi}")
        text = abi.read_text(encoding="utf-8")
        self.assertNotIn("current bundle does not contain a relocatable CPython", text)
        self.assertIn("the bundle includes managed CPython", text)
        self.assertIn("immutable offline cache", text)

    def test_shipped_compatibility_guide_states_artifact_and_package_boundaries(self):
        guide = INSTALL_ROOT / "docs/compatibility-and-migrations.md"
        self.assertTrue(guide.is_file(), f"compatibility guide missing: {guide}")
        text = guide.read_text(encoding="utf-8")
        supported = re.search(
            r"artifact formats 14 through (\d+); the current compiler writes format (\d+)",
            text,
        )
        reference = (INSTALL_ROOT / "docs/reference.md").read_text(encoding="utf-8")
        current = re.search(r"The current format is (\d+)", reference)
        self.assertIsNotNone(supported, "compatibility guide must state the reader and writer formats")
        self.assertIsNotNone(current, "reference must state the current artifact format")
        self.assertEqual(supported.group(1), supported.group(2))
        self.assertEqual(supported.group(2), current.group(1))
        self.assertIn("macOS ARM64", text)
        self.assertIn("Debian 12-compatible GNU/Linux ARM64", text)
        self.assertIn("exact Python patch version", text)
        self.assertIn("no automatic application-data migration", text)

    def test_json_projection_runs_from_a_source_free_artifact(self):
        with tempfile.TemporaryDirectory(prefix="veritas shipped projection ") as temp:
            source = Path(temp) / "projection.vr"
            artifact = Path(temp) / "projection.vrb"
            source.write_text(
                r'''fn main() -> Int:
    let columns = unwrap(json_project_columns("{\"rows\":[{\"n\":40},{\"n\":2}]}", ["rows"], ["n"]))
    let values = json_as_int_list(json_get(columns, "n"))
    assert(values[0] == 40 and values[1] == 2)
    let duplicate = json_project_columns("{\"rows\":[{\"n\":1,\"ignored\":{\"x\":1,\"x\":2}}]}", ["rows"], ["n"])
    assert(not is_ok(duplicate))
    assert(json_error_code(error(duplicate)) == "R_JSON_DUPLICATE_KEY")
    return 0
''',
                encoding="utf-8",
            )
            built = subprocess.run(
                [str(VERITAS), "build", str(source), "-o", str(artifact)],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            self.assertRegex(built.stdout, r"artifact format \d+")
            source.unlink()
            run = subprocess.run(
                [str(VERITAS), "run", str(artifact)],
                cwd=temp,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_shipped_user_guidance_does_not_assume_a_python_command(self):
        readme = INSTALL_ROOT / "README.md"
        self.assertTrue(readme.is_file(), f"shipped README missing: {readme}")
        text = readme.read_text(encoding="utf-8")
        self.assertIn("commonly `python3`", text)
        self.assertIn("do not assume a `python` command exists", text)

    def test_shipped_package_notes_name_the_linux_dependency_audit(self):
        package_notes = INSTALL_ROOT / "PACKAGE.txt"
        self.assertTrue(package_notes.is_file(), f"package notes missing: {package_notes}")
        text = package_notes.read_text(encoding="utf-8")
        self.assertIn("Linux binary dependency audit: linux-dependencies.json", text)
        if "Host: linux-aarch64" in text:
            self.assertIn("Debian 12-compatible", text)
        else:
            self.assertIn("Host: darwin-arm64", text)

    def test_independent_briefs_and_case_manifest_are_shipped_as_frozen(self):
        brief_dir = INSTALL_ROOT / "evaluation/briefs/independent"
        self.assertEqual(len(list(brief_dir.glob("*.md"))), 9)
        manifest_path = INSTALL_ROOT / "evaluation/fixtures/independent-v1/manifest.json"
        self.assertTrue(manifest_path.is_file(), f"independent case manifest missing: {manifest_path}")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(manifest["status"], "frozen")
        self.assertEqual(manifest["task_count"], 9)
        review_packet = INSTALL_ROOT / "evaluation/review/independent-v1/reviewer-instructions.md"
        self.assertTrue(review_packet.is_file(), f"independent reviewer packet missing: {review_packet}")
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v10.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v11.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v12.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v13.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v14.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v15.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v16.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v17.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v18.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-v19.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/review/independent-v1/review-result-final-pass.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/prompts/recommendation-choice.md").is_file())
        sandbox = INSTALL_ROOT / "evaluation/sandbox"
        for name in ("Dockerfile", "build-images.sh", "run-case.sh", "candidate-python.json", "candidate-veritas.json"):
            self.assertTrue((sandbox / name).is_file(), f"restricted campaign runner file missing: {name}")
        self.assertTrue((sandbox / "dependencies/requirements.txt").is_file())
        self.assertTrue((sandbox / "dependencies/campaign-dependencies.tar.gz").is_file())
        product_bundle = INSTALL_ROOT / "evaluation/review/independent-v1/product-information"
        self.assertTrue((product_bundle / "python-baseline.md").is_file())
        self.assertTrue((product_bundle / "setup-guide.md").is_file())
        self.assertTrue((product_bundle / "language-spec.md").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/schedule.py").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/stage_delivery.py").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/render_choice_prompt.py").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/usage_evidence.py").is_file())
        self.assertTrue((INSTALL_ROOT / "evaluation/protocol/campaign-config.json").is_file())
        runner_guide = (INSTALL_ROOT / "evaluation/protocol/case-runner.md").read_text(encoding="utf-8")
        self.assertIn('"launch_command"', runner_guide)
        self.assertIn('"dependency_lock_path"', runner_guide)
        self.assertIn("127 cases total", runner_guide)
        self.assertFalse((INSTALL_ROOT / "evaluation/sandbox/campaign-lock.json").exists())
        hashes_path = INSTALL_ROOT / "evaluation/review/independent-v1/artifact-hashes.json"
        hashes = json.loads(hashes_path.read_text(encoding="utf-8"))
        self.assertEqual(hashes["status"], "frozen")
        for artifact in hashes["artifacts"]:
            if artifact.get("review_packet_only") or not artifact.get("distribution_include", True):
                continue
            path = INSTALL_ROOT / artifact["path"]
            self.assertTrue(path.is_file(), f"review artifact missing from package: {path}")
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), artifact["sha256"], artifact["path"])
            packaged_alias = artifact.get("packaged_alias")
            if packaged_alias:
                alias = INSTALL_ROOT / packaged_alias
                self.assertTrue(alias.is_file(), f"review artifact alias missing from package: {alias}")
                self.assertEqual(hashlib.sha256(alias.read_bytes()).hexdigest(), artifact["sha256"], packaged_alias)
        bundle_path = product_bundle / "bundle-manifest.json"
        bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
        canonical = json.dumps(bundle["artifacts"], sort_keys=True, separators=(",", ":")).encode("utf-8")
        self.assertEqual(hashlib.sha256(canonical).hexdigest(), bundle["bundle_sha256"])


if __name__ == "__main__":
    unittest.main()
