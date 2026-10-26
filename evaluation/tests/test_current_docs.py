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
            ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu"],
        )
        evidence = ROOT / targets["evidence"]
        self.assertTrue(evidence.is_file(), f"missing target evidence: {evidence}")
        readiness = (ROOT / "docs/pre-release-readiness.md").read_text(
            encoding="utf-8"
        )
        status = (ROOT / "docs/status.md").read_text(encoding="utf-8")
        self.assertIn("matrix passed", readiness)
        self.assertIn("macOS ARM64", readiness)
        self.assertIn("Debian 12-compatible GNU/Linux ARM64", readiness)
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
        self.assertIn("Debian 12-compatible GNU/Linux ARM64", readme)
        start_here = (ROOT / "docs/agents/start-here.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("The default build works without Python", readme)
        self.assertIn("Python is not required to compile or run a core Guion program", start_here)
        self.assertIn("Linux distributions, and libc implementations remain unverified", compatibility)

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
