from __future__ import annotations

from pathlib import Path
import re
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


class PlatformManifestTests(unittest.TestCase):
    def test_macos_and_linux_manifests_pin_matching_uv_versions(self):
        mac_path = ROOT / "distribution/toolchains/manifest.toml"
        linux_path = ROOT / "distribution/toolchains/manifest-linux-aarch64.toml"
        x86_linux_path = ROOT / "distribution/toolchains/manifest-linux-x86_64.toml"
        self.assertTrue(linux_path.is_file(), "Linux ARM64 must have a pinned toolchain manifest")
        self.assertTrue(x86_linux_path.is_file(), "Linux x86_64 must have a pinned toolchain manifest")
        with mac_path.open("rb") as source:
            mac = tomllib.load(source)
        with linux_path.open("rb") as source:
            linux = tomllib.load(source)
        with x86_linux_path.open("rb") as source:
            x86_linux = tomllib.load(source)

        self.assertEqual(mac["target"], "macos-aarch64")
        self.assertEqual(linux["target"], "linux-aarch64-gnu")
        self.assertEqual(linux["uv_version"], mac["uv_version"])
        self.assertIn("aarch64-unknown-linux-gnu", linux["archive_url"])
        self.assertRegex(linux["archive_sha256"], re.compile(r"^[0-9a-f]{64}$"))
        self.assertRegex(linux["binary_sha256"], re.compile(r"^[0-9a-f]{64}$"))
        self.assertEqual(linux["architecture"], "aarch64")
        self.assertEqual(linux["libc"], "glibc")
        self.assertEqual(x86_linux["target"], "linux-x86_64-gnu")
        self.assertEqual(x86_linux["uv_version"], mac["uv_version"])
        self.assertIn("x86_64-unknown-linux-gnu", x86_linux["archive_url"])
        self.assertRegex(x86_linux["archive_sha256"], re.compile(r"^[0-9a-f]{64}$"))
        self.assertRegex(x86_linux["binary_sha256"], re.compile(r"^[0-9a-f]{64}$"))
        self.assertEqual(x86_linux["architecture"], "x86_64")
        self.assertEqual(x86_linux["libc"], "glibc")

    def test_host_package_script_routes_linux_arm64_to_its_manifest(self):
        script = (ROOT / "distribution/package-current-host.sh").read_text(encoding="utf-8")
        self.assertIn("linux-aarch64", script)
        self.assertIn("manifest-linux-aarch64.toml", script)
        self.assertIn("linux-x86_64", script)
        self.assertIn("manifest-linux-x86_64.toml", script)
        self.assertIn("audit_linux_dependencies.py", script)
        self.assertIn("*_bundle) continue", script)
        self.assertIn('rm -rf "$PYTHON_ENV"', script)


if __name__ == "__main__":
    unittest.main()
