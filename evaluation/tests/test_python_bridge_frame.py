#!/usr/bin/env python3
"""Exercise rejection paths for the Python bridge's private binary frame."""

from __future__ import annotations

import ast
import io
import json
import struct
import subprocess
import sys
import tracemalloc
from types import SimpleNamespace
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


def read_exact_function(reader: io.BytesIO):
    module = ast.parse(SIDECAR.read_text(encoding="utf-8"))
    function = next(
        node
        for node in module.body
        if isinstance(node, ast.FunctionDef) and node.name == "read_exact"
    )
    namespace = {"sys": SimpleNamespace(stdin=SimpleNamespace(buffer=reader))}
    extracted = ast.Module(body=[function], type_ignores=[])
    exec(compile(extracted, str(SIDECAR), "exec"), namespace)
    return namespace["read_exact"]


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

    def test_attachment_extraction_uses_at_most_one_payload_copy(self) -> None:
        module = ast.parse(SIDECAR.read_text(encoding="utf-8"))
        parser = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name == "parse_request_frame"
        )
        namespace = {"BINARY_FRAME_MAGIC": MAGIC, "json": json, "struct": struct}
        extracted = ast.Module(body=[parser], type_ignores=[])
        exec(compile(extracted, str(SIDECAR), "exec"), namespace)

        attachment = b"x" * 5_100_867
        frame = bytearray(binary_frame(request([{"t": "bytes", "v": 0}]), (attachment,)))
        tracemalloc.start()
        parsed = namespace["parse_request_frame"](frame)
        _current, peak = tracemalloc.get_traced_memory()
        tracemalloc.stop()

        self.assertEqual(parsed[1], [attachment])
        self.assertIs(type(parsed[1][0]), bytes)
        self.assertLess(peak, len(attachment) * 1.5)

    def test_frame_read_uses_at_most_one_payload_allocation(self) -> None:
        payload = b"x" * 5_100_867
        read_exact = read_exact_function(io.BufferedReader(io.BytesIO(payload)))

        tracemalloc.start()
        try:
            frame = read_exact(len(payload))
            _current, peak = tracemalloc.get_traced_memory()
        finally:
            tracemalloc.stop()

        self.assertEqual(frame, payload)
        self.assertIs(type(frame), bytearray)
        self.assertLess(peak, len(payload) * 1.5)

    def test_frame_read_preserves_short_read_and_eof_behavior(self) -> None:
        class ShortReadBytesIO(io.BytesIO):
            def read(self, size: int = -1) -> bytes:
                return super().read(min(size, 2) if size >= 0 else 2)

            def readinto(self, buffer: bytearray | memoryview) -> int:
                return super().readinto(memoryview(buffer)[:2])

        reader = ShortReadBytesIO(b"hello")
        read_exact = read_exact_function(reader)
        self.assertEqual(read_exact(5), bytearray(b"hello"))
        self.assertEqual(read_exact(0), bytearray())
        self.assertIsNone(read_exact(1))

        partial = read_exact_function(ShortReadBytesIO(b"ab"))
        with self.assertRaisesRegex(EOFError, "truncated frame"):
            partial(3)


if __name__ == "__main__":
    unittest.main()
