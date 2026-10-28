#!/usr/bin/env python3
"""Verify opt-in phase profiling for repeated Python bridge calls."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "evaluation/development/guion_python_bridge_repeat_probe"
GUION = Path(os.environ.get("GUION_BINARY", ROOT / "target/release/guion")).resolve()
PYTHON = os.environ.get("GUION_PYTHON")
SCRATCH = ROOT / "target/perf-session"


class PythonBridgeProfileTests(unittest.TestCase):
    def test_opt_in_profile_reports_bridge_phases_for_every_call(self) -> None:
        if PYTHON is None:
            self.skipTest("set GUION_PYTHON to the pinned NumPy/orjson interpreter")
        self.assertTrue(GUION.is_file(), f"feature-enabled Guion CLI missing at {GUION}")
        SCRATCH.mkdir(parents=True, exist_ok=True)

        with tempfile.TemporaryDirectory(prefix="guion-bridge-profile-test-", dir=SCRATCH) as name:
            project = Path(name) / "project"
            shutil.copytree(APP, project)
            source = (project / "main.template.gu").read_text(encoding="utf-8")
            (project / "main.gu").write_text(
                source.replace("__CALL_COUNT__", "3"), encoding="utf-8"
            )
            (project / "payload.bin").write_bytes(b"x" * 64)
            profile_path = Path(name) / "profile.json"
            environment = os.environ.copy()
            environment.update(
                {
                    "GUION_PYTHON": str(Path(PYTHON).resolve()),
                    "GUION_PYTHON_BRIDGE_PROFILE_OUT": str(profile_path),
                    "PYTHONDONTWRITEBYTECODE": "1",
                }
            )
            result = subprocess.run(
                [
                    str(GUION),
                    "run",
                    "--no-cache",
                    "--file-root",
                    str(project),
                    "main.gu",
                    "payload.bin",
                ],
                cwd=project,
                env=environment,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "192\n")
            self.assertTrue(profile_path.is_file(), "opt-in profile file was not written")
            profile = json.loads(profile_path.read_text(encoding="utf-8"))
            self.assertEqual(profile["schema_version"], 1)
            self.assertEqual(profile["kind"], "guion-python-bridge-phase-profile")
            self.assertEqual(profile["calls"], 3)
            self.assertEqual(profile["python"]["calls"], 3)
            self.assertEqual(profile["python"].get("request_json_parser"), "orjson")
            self.assertEqual(profile["python"].get("response_json_encoder"), "orjson")
            self.assertGreater(profile["rust_first_call_ns"]["request_serialize"], 0)
            self.assertGreater(profile["rust_repeated_calls_ns"]["response_wait"], 0)
            self.assertGreater(
                profile["rust_repeated_calls_ns"]["response_first_byte_wait"], 0
            )
            self.assertGreater(
                profile["rust_repeated_calls_ns"]["reader_frame_after_first_byte"], 0
            )
            self.assertGreater(profile["rust_repeated_calls_ns"]["channel_delivery"], 0)
            self.assertGreater(profile["python"]["first_call_ns"]["module_dispatch"], 0)
            self.assertGreater(
                profile["python"]["first_call_ns"]["module_import_fallback"], 0
            )
            self.assertGreater(profile["python"]["repeated_calls_ns"]["function_call"], 0)
            self.assertEqual(
                profile["python"]["repeated_calls_ns"]["module_import_fallback"], 0
            )
            self.assertGreater(profile["python"]["repeated_calls_ns"]["request_frame_read"], 0)
            self.assertGreater(profile["python"]["repeated_calls_ns"]["response_json_encode"], 0)
            self.assertGreater(profile["python"]["repeated_calls_ns"]["response_write"], 0)
            self.assertGreater(
                profile["python"]["repeated_calls_ns"][
                    "request_to_response_first_write"
                ],
                0,
            )
            if sys.platform == "darwin":
                self.assertGreater(
                    profile["python"]["repeated_calls_ns"]["native_image_check"], 0
                )
            else:
                self.assertEqual(
                    profile["python"]["repeated_calls_ns"]["native_image_check"], 0
                )
            self.assertEqual(profile["python"]["repeated_calls_ns"]["native_image_snapshot"], 0)


if __name__ == "__main__":
    unittest.main()
