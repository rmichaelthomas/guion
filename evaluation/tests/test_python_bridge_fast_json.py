#!/usr/bin/env python3
"""Verify the Python bridge's optional JSON encoder fallback."""

from __future__ import annotations

import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SIDECAR = ROOT / "crates/runtime/python/sidecar.py"
SCRATCH = ROOT / "target/perf-session"


class PythonBridgeFastJsonTests(unittest.TestCase):
    def test_unsupported_fast_encoder_falls_back_and_preserves_large_json_integer(self) -> None:
        SCRATCH.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="guion-bridge-fast-json-", dir=SCRATCH) as name:
            project = Path(name)
            (project / "probe.py").write_text(
                "import orjson\n\n"
                "def large_integer():\n"
                "    return 2**100\n",
                encoding="utf-8",
            )
            (project / "orjson.py").write_text(
                "def dumps(value):\n"
                "    raise TypeError('simulated fast encoder range limit')\n",
                encoding="utf-8",
            )
            request = {
                "protocol": 2,
                "id": 1,
                "kind": "call",
                "module": "probe",
                "function": "large_integer",
                "args": [],
                "kwargs": {},
                "json_result": True,
                "numpy_list_result": False,
            }
            payload = json.dumps(request, separators=(",", ":")).encode("utf-8")
            framed_request = struct.pack(">I", len(payload)) + payload
            environment = os.environ.copy()
            environment.update(
                {
                    "GUION_PYTHON_BRIDGE_PROFILE": "1",
                    "PYTHONPATH": str(project),
                    "PYTHONDONTWRITEBYTECODE": "1",
                }
            )
            result = subprocess.run(
                [sys.executable, "-B", str(SIDECAR)],
                cwd=project,
                env=environment,
                input=framed_request,
                capture_output=True,
                timeout=10,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            self.assertGreaterEqual(len(result.stdout), 4)
            response_size = struct.unpack(">I", result.stdout[:4])[0]
            self.assertEqual(len(result.stdout), response_size + 4)
            response = json.loads(result.stdout[4:])
            self.assertTrue(response["ok"], response)
            self.assertEqual(response["value"]["v"], 2**100)
            self.assertEqual(
                response["_bridge_profile"].get("response_json_encoder"), "stdlib_fallback"
            )


if __name__ == "__main__":
    unittest.main()
