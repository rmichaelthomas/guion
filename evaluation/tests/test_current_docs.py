from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CurrentDocumentationTests(unittest.TestCase):
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
        self.assertIn(f"artifact formats 14 through {current}", compatibility)
        self.assertIn(f"current format is {current}", reference)

    def test_user_docs_state_verified_platform_scope_and_python_launcher(self):
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        compatibility = (ROOT / "docs/compatibility-and-migrations.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("macOS ARM64", readme)
        self.assertIn("Debian 12-compatible GNU/Linux ARM64", readme)
        self.assertIn("do not assume a `python` command exists", readme)
        self.assertIn("python3", readme)
        self.assertIn("Other Linux distributions, libc implementations", compatibility)

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
