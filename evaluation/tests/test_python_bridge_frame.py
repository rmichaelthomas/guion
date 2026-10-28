#!/usr/bin/env python3
"""Exercise rejection paths for the Python bridge's private binary frame."""

from __future__ import annotations

import json
import struct
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SIDECAR = ROOT / "crates/runtime/python/sidecar.py"
MAX_FRAME = 24 * 1024 * 1024
MAGIC = b"GBIN"


def binary_frame(metadata: dict[str, object], attachments: tuple[bytes, ...] = ()) -> bytes:
    encoded_metadata = json.dumps(metadata, separators=(",", ":")).encode("utf-8")
    frame = bytearray(MAGIC)
    frame.extend(struct.pack(">I", len(encoded_metadata)))
    frame.extend(encoded_metadata)
    frame.extend(struct.pack(">I", len(attachments)))
    for attachment in attachments:
        frame.extend(struct.pack(">I", len(attachment)))
        frame.extend(attachment)
    return bytes(frame)


def request(args: list[dict[str, object]]) -> dict[str, object]:
    return {
        "protocol": 2,
        "id": 7,
        "kind": "call",
        "module": "builtins",
        "function": "len",
        "args": args,
        "kwargs": {},
    }


def run_frame(payload: bytes, declared_size: int | None = None) -> subprocess.CompletedProcess[bytes]:
    outer_size = len(payload) if declared_size is None else declared_size
    return subprocess.run(
        [sys.executable, "-I", "-B", "-u", str(SIDECAR)],
        input=struct.pack(">I", outer_size) + payload,
        cwd=ROOT,
        capture_output=True,
        timeout=5,
        check=False,
    )


def response_for(result: subprocess.CompletedProcess[bytes]) -> dict[str, object]:
    if result.returncode != 0:
        raise AssertionError(f"sidecar exited {result.returncode}: {result.stderr!r}")
    if len(result.stdout) < 4:
        raise AssertionError("sidecar did not return a framed error response")
    size = struct.unpack(">I", result.stdout[:4])[0]
    if len(result.stdout) != size + 4:
        raise AssertionError("sidecar response frame length is inconsistent")
    return json.loads(result.stdout[4:])


class PythonBridgeFrameRejectionTests(unittest.TestCase):
    def assert_error_response(self, payload: bytes) -> None:
        response = response_for(run_frame(payload))
        self.assertFalse(response["ok"])

    def test_rejects_truncated_binary_metadata(self) -> None:
        self.assert_error_response(MAGIC + struct.pack(">I", 8) + b"{}")

    def test_rejects_truncated_attachment(self) -> None:
        metadata = request([{"t": "bytes", "v": 0}])
        payload = binary_frame(metadata, (b"ab",))[:-1]
        self.assert_error_response(payload)

    def test_rejects_out_of_range_attachment_index(self) -> None:
        payload = binary_frame(request([{"t": "bytes", "v": 0}]))
        self.assert_error_response(payload)

    def test_rejects_trailing_binary_frame_data(self) -> None:
        self.assert_error_response(binary_frame(request([])) + b"x")

    def test_rejects_oversized_outer_frame_without_response(self) -> None:
        result = run_frame(b"", MAX_FRAME + 1)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")


if __name__ == "__main__":
    unittest.main()
