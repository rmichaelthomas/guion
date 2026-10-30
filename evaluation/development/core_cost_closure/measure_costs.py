#!/usr/bin/env python3
"""Build and validate reproducible Guion performance comparison manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path
from typing import Any

_SHA256 = re.compile(r"^[0-9a-f]{64}$")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_patch_sha256(repository: Path) -> str:
    """Hash tracked source/build changes relative to HEAD, excluding evaluation/docs."""
    patch = subprocess.run(
        [
            "git",
            "diff",
            "--binary",
            "HEAD",
            "--",
            ".",
            ":(exclude)evaluation",
            ":(exclude)docs",
            ":(exclude)README.md",
        ],
        cwd=repository,
        capture_output=True,
        check=True,
    ).stdout
    return hashlib.sha256(patch).hexdigest()


def build_manifest(
    *,
    binary: Path,
    source_revision: str,
    source_patch_hash: str,
    toolchain: dict[str, str],
    build_settings: dict[str, Any],
    input_hashes: dict[str, str],
) -> dict[str, Any]:
    binary = binary.resolve(strict=True)
    if not binary.is_file():
        raise ValueError(f"binary is not a file: {binary}")
    if not source_revision.strip():
        raise ValueError("source revision must not be empty")
    if not _SHA256.fullmatch(source_patch_hash):
        raise ValueError("source patch hash must be a lowercase SHA-256")
    if not toolchain or any(not str(value).strip() for value in toolchain.values()):
        raise ValueError("toolchain manifest must contain non-empty versions")
    if not build_settings:
        raise ValueError("build settings must not be empty")
    for name, digest in input_hashes.items():
        if not _SHA256.fullmatch(digest):
            raise ValueError(f"input hash for {name!r} is not a lowercase SHA-256")
    return {
        "schema_version": 1,
        "source_revision": source_revision.strip(),
        "source_patch_sha256": source_patch_hash,
        "binary_path": str(binary),
        "binary_sha256": sha256_file(binary),
        "toolchain": dict(sorted(toolchain.items())),
        "build_settings": build_settings,
        "input_sha256": dict(sorted(input_hashes.items())),
    }


def require_comparable(baseline: dict[str, Any], candidate: dict[str, Any]) -> None:
    """Reject unlike build/input cohorts before timing their binaries."""
    for field, label in (
        ("toolchain", "toolchain"),
        ("build_settings", "build settings"),
        ("input_sha256", "workload inputs"),
    ):
        if baseline.get(field) != candidate.get(field):
            raise ValueError(f"baseline and candidate {label} differ")
    for role, manifest in (("baseline", baseline), ("candidate", candidate)):
        if not _SHA256.fullmatch(str(manifest.get("binary_sha256", ""))):
            raise ValueError(f"{role} binary SHA-256 is missing or invalid")
        if not manifest.get("source_revision"):
            raise ValueError(f"{role} source revision is missing")
        if not manifest.get("input_sha256"):
            raise ValueError(f"{role} workload input hashes are missing")


def require_manifest_binary(
    manifest: dict[str, Any], binary: Path, *, role: str
) -> None:
    """Prove the binary about to run is the one recorded in its manifest."""
    binary = binary.resolve(strict=True)
    recorded_path = manifest.get("binary_path")
    if recorded_path and Path(str(recorded_path)).resolve(strict=True) != binary:
        raise ValueError(f"{role} manifest points at a different binary path")
    actual_hash = sha256_file(binary)
    if manifest.get("binary_sha256") != actual_hash:
        raise ValueError(f"{role} binary SHA-256 does not match its manifest")


def load_manifest(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"manifest must be a JSON object: {path}")
    return value


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--source-patch-sha256", required=True)
    parser.add_argument("--toolchain-json", required=True, type=Path)
    parser.add_argument("--build-settings-json", required=True, type=Path)
    parser.add_argument("--input-hashes-json", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    manifest = build_manifest(
        binary=args.binary,
        source_revision=args.source_revision,
        source_patch_hash=args.source_patch_sha256,
        toolchain=json.loads(args.toolchain_json.read_text(encoding="utf-8")),
        build_settings=json.loads(args.build_settings_json.read_text(encoding="utf-8")),
        input_hashes=json.loads(args.input_hashes_json.read_text(encoding="utf-8")),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": "recorded", "manifest": str(args.output), "binary_sha256": manifest["binary_sha256"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
