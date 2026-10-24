"""Report ELF dependencies and the required glibc floor for a Linux package."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


NEEDED = re.compile(r"Shared library: \[([^]]+)\]")
GLIBC = re.compile(r"Name: GLIBC_([0-9]+(?:\.[0-9]+)+)")


class AuditError(RuntimeError):
    pass


def _version(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in value.split("."))


def audit_tree(
    root: Path,
    *,
    readelf: str = "readelf",
    ldd: str = "ldd",
    max_glibc: str = "2.36",
) -> dict[str, Any]:
    """Inspect every ELF payload and fail for unresolved libraries or a newer glibc."""
    if not root.is_dir():
        raise AuditError(f"package tree does not exist: {root}")
    entries = []
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        try:
            with path.open("rb") as source:
                if source.read(4) != b"\x7fELF":
                    continue
        except OSError as error:
            raise AuditError(f"cannot inspect {path}: {error}") from error

        dynamic = subprocess.run(
            [readelf, "-dW", str(path)], capture_output=True, text=True, check=False
        )
        if dynamic.returncode:
            raise AuditError(f"readelf failed for {path}: {dynamic.stderr.strip()}")
        versions = subprocess.run(
            [readelf, "--version-info", "--wide", str(path)],
            capture_output=True,
            text=True,
            check=False,
        )
        if versions.returncode:
            raise AuditError(f"readelf failed for {path}: {versions.stderr.strip()}")
        requirements = sorted(
            {match.group(1) for match in GLIBC.finditer(versions.stdout)}, key=_version
        )
        if requirements and _version(requirements[-1]) > _version(max_glibc):
            raise AuditError(
                f"{path.relative_to(root)} requires GLIBC_{requirements[-1]}, "
                f"above supported glibc {max_glibc}"
            )

        dependencies = sorted(set(NEEDED.findall(dynamic.stdout)))
        resolution = subprocess.run(
            [ldd, str(path)], capture_output=True, text=True, check=False
        )
        resolution_text = resolution.stdout + resolution.stderr
        if "not found" in resolution_text:
            missing = [line.strip() for line in resolution_text.splitlines() if "not found" in line]
            raise AuditError(f"unresolved dependencies for {path.relative_to(root)}: {missing}")
        if resolution.returncode and "not a dynamic executable" not in resolution_text:
            raise AuditError(f"ldd failed for {path}: {resolution_text.strip()}")

        entries.append(
            {
                "path": path.relative_to(root).as_posix(),
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "needed": dependencies,
                "required_glibc": requirements[-1] if requirements else None,
            }
        )
    if not entries:
        raise AuditError(f"no ELF files found under {root}")
    return {
        "schema_version": 1,
        "target": "linux-aarch64-gnu",
        "glibc_maximum": max_glibc,
        "elf_files": entries,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package_root", type=Path)
    parser.add_argument("--max-glibc", default="2.36")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        report = audit_tree(args.package_root, max_glibc=args.max_glibc)
    except (AuditError, OSError) as error:
        parser.exit(1, f"Linux dependency audit failed: {error}\n")
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
