"""Tests for target validation in the Feed Digest package verifier."""

from __future__ import annotations

import importlib.util
import unittest
from argparse import Namespace
from pathlib import Path


VERIFIER_PATH = (
    Path(__file__).resolve().parents[1]
    / "completion/performance/verify_managed_feed_digest_package.py"
)
SPEC = importlib.util.spec_from_file_location("feed_digest_verifier", VERIFIER_PATH)
assert SPEC is not None and SPEC.loader is not None
VERIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFIER)


class ManagedFeedDigestTargetTests(unittest.TestCase):
    def test_target_description_names_debian_x86_64(self) -> None:
        label = VERIFIER.target_description(
            "Linux",
            "x86_64",
            {"id": "debian", "version": "12", "name": "Debian GNU/Linux 12 (bookworm)"},
        )

        self.assertEqual(label, "Debian GNU/Linux 12 (bookworm) x86_64")

    def test_target_description_keeps_macos_arm64_wording(self) -> None:
        label = VERIFIER.target_description(
            "Darwin", "arm64", {"id": "darwin", "version": "15.0", "name": "macOS"}
        )

        self.assertEqual(label, "macOS ARM64")

    def test_target_validation_accepts_the_requested_debian_target(self) -> None:
        args = Namespace(
            expected_os="linux",
            expected_arch="x86_64",
            expected_distribution="debian",
            expected_distribution_version="12",
            colima_vm_used=True,
            colima_vm_started=False,
            rosetta_used=True,
        )

        VERIFIER.validate_target(
            args,
            "Linux",
            "x86_64",
            {"id": "debian", "version": "12", "name": "Debian 12"},
        )

    def test_target_validation_rejects_wrong_architecture(self) -> None:
        args = Namespace(
            expected_os="linux",
            expected_arch="x86_64",
            expected_distribution="debian",
            expected_distribution_version="12",
            colima_vm_used=True,
            colima_vm_started=False,
            rosetta_used=True,
        )

        with self.assertRaisesRegex(RuntimeError, "expected architecture x86_64, found aarch64"):
            VERIFIER.validate_target(
                args,
                "Linux",
                "aarch64",
                {"id": "debian", "version": "12", "name": "Debian 12"},
            )

    def test_rosetta_requires_colima_record(self) -> None:
        args = Namespace(
            expected_os=None,
            expected_arch=None,
            expected_distribution=None,
            expected_distribution_version=None,
            colima_vm_used=False,
            colima_vm_started=False,
            rosetta_used=True,
        )

        with self.assertRaisesRegex(RuntimeError, "Rosetta use must be recorded with a Colima VM"):
            VERIFIER.validate_target(args, "Linux", "x86_64", {"id": "debian", "version": "12"})


if __name__ == "__main__":
    unittest.main()
