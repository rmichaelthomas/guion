"""Tests for build and workload comparability manifests."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import measure_costs


class BuildManifestTests(unittest.TestCase):
    def make_manifest(self, binary: Path, *, rustc: str = "rustc 1.97.1", input_hash: str = "a" * 64):
        return measure_costs.build_manifest(
            binary=binary,
            source_revision="41d3b48",
            source_patch_hash="0" * 64,
            toolchain={"rustc": rustc, "cargo": "cargo 1.97.1"},
            build_settings={"profile": "release", "target": "aarch64-unknown-linux-gnu", "locked": True, "offline": True},
            input_hashes={"events.jsonl": input_hash},
        )

    def test_matching_build_and_input_contract_is_comparable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            baseline = Path(directory) / "baseline"
            candidate = Path(directory) / "candidate"
            baseline.write_bytes(b"baseline executable")
            candidate.write_bytes(b"candidate executable")
            before = self.make_manifest(baseline)
            after = self.make_manifest(candidate)

            measure_costs.require_comparable(before, after)

            self.assertEqual(before["binary_sha256"], measure_costs.sha256_file(baseline))
            self.assertNotEqual(before["binary_sha256"], after["binary_sha256"])

    def test_mismatched_toolchain_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            baseline = Path(directory) / "baseline"
            candidate = Path(directory) / "candidate"
            baseline.write_bytes(b"baseline")
            candidate.write_bytes(b"candidate")

            with self.assertRaisesRegex(ValueError, "toolchain"):
                measure_costs.require_comparable(self.make_manifest(baseline), self.make_manifest(candidate, rustc="rustc 2.0"))

    def test_mismatched_input_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            baseline = Path(directory) / "baseline"
            candidate = Path(directory) / "candidate"
            baseline.write_bytes(b"baseline")
            candidate.write_bytes(b"candidate")

            with self.assertRaisesRegex(ValueError, "input"):
                measure_costs.require_comparable(self.make_manifest(baseline), self.make_manifest(candidate, input_hash="b" * 64))

    def test_manifest_rejects_a_binary_that_changed_after_build_recording(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "guion"
            binary.write_bytes(b"original executable")
            manifest = self.make_manifest(binary)
            binary.write_bytes(b"replacement executable")

            with self.assertRaisesRegex(ValueError, "SHA-256"):
                measure_costs.require_manifest_binary(manifest, binary, role="baseline")

    def test_matching_manifest_proves_the_binary_on_disk(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "guion"
            binary.write_bytes(b"recorded executable")
            manifest = self.make_manifest(binary)

            measure_costs.require_manifest_binary(manifest, binary, role="baseline")


if __name__ == "__main__":
    unittest.main()
