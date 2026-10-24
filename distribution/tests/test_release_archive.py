from __future__ import annotations

from io import BytesIO
from pathlib import Path
import tarfile
import tempfile
import unittest

from distribution.verify_release_archive import ArchiveValidationError, verify_release_archive


class ReleaseArchiveValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="veritas archive validation ")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "release.tar.gz"

    def write_archive(self, entries):
        with tarfile.open(self.path, "w:gz") as archive:
            root = tarfile.TarInfo("veritas-test")
            root.type = tarfile.DIRTYPE
            archive.addfile(root)
            for name, kind, target in entries:
                member = tarfile.TarInfo(name)
                if kind == "dir":
                    member.type = tarfile.DIRTYPE
                    archive.addfile(member)
                elif kind == "symlink":
                    member.type = tarfile.SYMTYPE
                    member.linkname = target
                    archive.addfile(member)
                elif kind == "hardlink":
                    member.type = tarfile.LNKTYPE
                    member.linkname = target
                    archive.addfile(member)
                else:
                    payload = b"contents"
                    member.size = len(payload)
                    archive.addfile(member, fileobj=BytesIO(payload))

    def test_accepts_internal_relative_symlinks_and_regular_package_members(self):
        self.write_archive(
            [
                ("veritas-test/bin", "dir", ""),
                ("veritas-test/lib/runtime", "file", ""),
                ("veritas-test/bin/runtime", "symlink", "../lib/runtime"),
            ]
        )
        self.assertEqual(verify_release_archive(str(self.path), "veritas-test"), 4)

    def test_rejects_parent_traversal_and_absolute_paths(self):
        for name in ("veritas-test/../../outside", "/tmp/outside"):
            with self.subTest(name=name):
                self.write_archive([(name, "file", "")])
                with self.assertRaises(ArchiveValidationError):
                    verify_release_archive(str(self.path), "veritas-test")

    def test_rejects_absolute_and_escaping_symlinks(self):
        for target in ("/tmp/outside", "../../../../outside"):
            with self.subTest(target=target):
                self.write_archive(
                    [("veritas-test/bin", "dir", ""), ("veritas-test/bin/tool", "symlink", target)]
                )
                with self.assertRaises(ArchiveValidationError):
                    verify_release_archive(str(self.path), "veritas-test")

    def test_rejects_duplicate_members_and_children_beneath_symlinks(self):
        self.write_archive(
            [
                ("veritas-test/bin", "dir", ""),
                ("veritas-test/bin/tool", "file", ""),
                ("veritas-test/bin/tool", "file", ""),
            ]
        )
        with self.assertRaisesRegex(ArchiveValidationError, "duplicate"):
            verify_release_archive(str(self.path), "veritas-test")

        self.write_archive(
            [
                ("veritas-test/bin", "dir", ""),
                ("veritas-test/bin/link", "symlink", "../bin"),
                ("veritas-test/bin/link/child", "file", ""),
            ]
        )
        with self.assertRaisesRegex(ArchiveValidationError, "non-directory"):
            verify_release_archive(str(self.path), "veritas-test")


if __name__ == "__main__":
    unittest.main()
