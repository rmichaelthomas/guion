from __future__ import annotations

from pathlib import Path
import tempfile
import unittest

from distribution.audit_linux_dependencies import AuditError, audit_tree


class LinuxDependencyAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="guion ELF audit ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        self.binary = self.bin_dir / "guion"
        self.binary.write_bytes(b"\x7fELFfixture")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.readelf = self.tools / "readelf"
        self.ldd = self.tools / "ldd"
        self._write_tool(self.readelf, """#!/bin/sh
case "$1" in
  -dW) printf '%s\\n' ' 0x1 (NEEDED) Shared library: [libc.so.6]' ;;
  --version-info) printf '%s\\n' '  Name: GLIBC_2.17  Flags: none' ;;
esac
""")
        self._write_tool(self.ldd, "#!/bin/sh\nprintf '%s\\n' 'libc.so.6 => /lib/libc.so.6 (0x1)'\n")

    @staticmethod
    def _write_tool(path: Path, body: str):
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)

    def test_reports_dependencies_hash_and_glibc_floor(self):
        report = audit_tree(self.root, readelf=str(self.readelf), ldd=str(self.ldd))
        self.assertEqual(report["target"], "linux-aarch64-gnu")
        self.assertEqual(report["elf_files"][0]["needed"], ["libc.so.6"])
        self.assertEqual(report["elf_files"][0]["required_glibc"], "2.17")
        self.assertEqual(len(report["elf_files"][0]["sha256"]), 64)

    def test_fails_when_system_dependency_is_unresolved(self):
        self._write_tool(self.ldd, "#!/bin/sh\nprintf '%s\\n' 'libc.so.6 => not found'\n")
        with self.assertRaisesRegex(AuditError, "unresolved dependencies"):
            audit_tree(self.root, readelf=str(self.readelf), ldd=str(self.ldd))

    def test_fails_when_binary_requires_newer_glibc_than_declared_floor(self):
        self._write_tool(
            self.readelf,
            """#!/bin/sh
case "$1" in
  -dW) printf '%s\\n' ' 0x1 (NEEDED) Shared library: [libc.so.6]' ;;
  --version-info) printf '%s\\n' '  Name: GLIBC_2.38  Flags: none' ;;
esac
""",
        )
        with self.assertRaisesRegex(AuditError, "above supported glibc 2.36"):
            audit_tree(self.root, readelf=str(self.readelf), ldd=str(self.ldd))


if __name__ == "__main__":
    unittest.main()
