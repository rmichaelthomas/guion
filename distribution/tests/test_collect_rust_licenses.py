from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "collect_rust_licenses", ROOT / "distribution/collect-rust-licenses.py"
)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COLLECTOR)


class RustLicenseCollectionTests(unittest.TestCase):
    def test_license_collection_filters_metadata_for_the_requested_target(self):
        metadata = {"packages": []}
        for target in ("aarch64-apple-darwin", "aarch64-unknown-linux-gnu"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                destination = Path(temporary) / "licenses" / "rust"
                with patch.object(
                    COLLECTOR.subprocess,
                    "run",
                    return_value=type("Result", (), {"stdout": json.dumps(metadata)})(),
                ) as run:
                    COLLECTOR.main(destination, target)

                command = run.call_args.args[0]
                self.assertEqual(command[command.index("--filter-platform") + 1], target)
                inventory = (destination.parent / "RUST-DEPENDENCIES.md").read_text(
                    encoding="utf-8"
                )
                self.assertIn(f"Target: {target}", inventory)


if __name__ == "__main__":
    unittest.main()
