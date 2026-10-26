from pathlib import Path
import re
import json
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CurrentDocumentationTests(unittest.TestCase):
    def test_agent_index_matches_current_release_target_evidence(self):
        index = json.loads(
            (ROOT / "docs/agents/index.json").read_text(encoding="utf-8")
        )
        targets = index["supported_targets"]
        self.assertEqual(targets["current_guion_validation"], "passed")
        self.assertEqual(
            targets["current_guion_targets"],
            [
                "aarch64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
            ],
        )
        evidence = ROOT / targets["evidence"]
        self.assertTrue(evidence.is_file(), f"missing target evidence: {evidence}")
        readiness = (ROOT / "docs/pre-release-readiness.md").read_text(
            encoding="utf-8"
        )
        status = (ROOT / "docs/status.md").read_text(encoding="utf-8")
        self.assertIn("matrix passed", readiness)
        self.assertIn("macOS ARM64", readiness)
        self.assertIn("Debian 12 GNU/Linux on ARM64 and x86_64", readiness)
        self.assertIn("matrix has passed", status)
        schema = json.loads(
            (ROOT / "docs/agents/index.schema.json").read_text(encoding="utf-8")
        )
        definition = schema["properties"]["supported_targets"]
        self.assertEqual(
            set(definition["required"]),
            {
                "source",
                "previously_validated_before_guion_rename",
                "current_guion_validation",
                "current_guion_targets",
                "evidence",
            },
        )

    def test_agent_start_here_explains_capability_inspection_and_result_syntax(self):
        start_here = (ROOT / "docs/agents/start-here.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("guion inspect --json", start_here)
        self.assertIn("statically reachable effects", start_here)
        self.assertIn("runtime-supplied URL", start_here)
        self.assertIn("?", start_here)
        self.assertIn("http_request_bytes", start_here)

    def test_agent_failure_recipe_is_indexed_and_sqlite_example_checks_results(self):
        index = json.loads(
            (ROOT / "docs/agents/index.json").read_text(encoding="utf-8")
        )
        failure = next(
            (example for example in index["examples"] if example["id"] == "agent_failure_paths"),
            None,
        )
        self.assertIsNotNone(failure)
        self.assertTrue((ROOT / failure["source"]).is_file())
        recipes = (ROOT / "docs/agents/recipes.md").read_text(encoding="utf-8")
        for case in ("missing-file", "denied-http", "sqlite-escape", "worker-failure"):
            self.assertIn(case, recipes)
        sqlite_example = (ROOT / "docs/agents/examples/sqlite_toml.gu").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("unwrap(", sqlite_example)

    def test_agent_inspection_report_schema_covers_grant_assessments(self):
        schema = json.loads(
            (ROOT / "docs/agents/cli-reports.schema.json").read_text(
                encoding="utf-8"
            )
        )
        report = schema["$defs"]["guion_inspection_report"]["allOf"][1]
        self.assertTrue(
            {
                "required_effects",
                "manifest_grants",
                "grant_assessment",
            }.issubset(report["required"])
        )
        statuses = report["properties"]["grant_assessment"]["properties"]
        self.assertIn(
            "present_but_target_dynamic",
            statuses["network_http"]["enum"],
        )
        self.assertIn(
            "unknown_external_runner",
            statuses["network_http"]["enum"],
        )
        index = json.loads(
            (ROOT / "docs/agents/index.json").read_text(encoding="utf-8")
        )
        inspect_report = next(
            report
            for report in index["cli_json_reports"]["reports"]
            if report["kind"] == "guion_inspection_report"
        )
        cases = {item["case"] for item in inspect_report["integration_tests"]}
        self.assertIn(
            "inspect_json_compares_reachable_effects_with_manifest_grants",
            cases,
        )

    @unittest.skipUnless(
        (ROOT / "crates/model/src/lib.rs").is_file(),
        "runtime constant comparison requires a source checkout",
    )
    def test_artifact_format_documentation_matches_the_runtime_constant(self):
        model = (ROOT / "crates/model/src/lib.rs").read_text(encoding="utf-8")
        match = re.search(r"pub const CURRENT_VERSION: u32 = (\d+);", model)
        self.assertIsNotNone(match, "current artifact format constant missing")
        current = match.group(1)
        compatibility = (ROOT / "docs/compatibility-and-migrations.md").read_text(
            encoding="utf-8"
        )
        reference = (ROOT / "docs/reference.md").read_text(encoding="utf-8")
        self.assertIn(f"runtime reads artifact formats 14 through {current}", compatibility)
        self.assertIn(f"current format is {current}", reference)

    def test_user_docs_state_verified_platform_scope_and_python_launcher(self):
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        compatibility = (ROOT / "docs/compatibility-and-migrations.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("macOS ARM64", readme)
        self.assertIn("Debian 12 GNU/Linux on ARM64 and x86_64", readme)
        start_here = (ROOT / "docs/agents/start-here.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("The default build works without Python", readme)
        self.assertIn("Python is not required to compile or run a core Guion program", start_here)
        self.assertIn(
            "Linux distributions beyond Debian 12, and libc implementations beyond glibc remain unverified",
            compatibility,
        )

    def test_linux_packaging_docs_name_the_elf_dependency_report(self):
        reference = (ROOT / "docs/reference.md").read_text(encoding="utf-8")
        notes_path = ROOT / "PACKAGE.txt"
        if not notes_path.is_file():
            notes_path = ROOT / "crates/cli/src/main.rs"
        package_notes = notes_path.read_text(encoding="utf-8")
        self.assertIn("linux-dependencies.json", reference)
        self.assertIn("linux-dependencies.json", package_notes)


if __name__ == "__main__":
    unittest.main()
