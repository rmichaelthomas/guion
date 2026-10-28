"""Direct NumPy/orjson reference matching the Guion bridge analysis."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile

from analysis import analyze_document, analyze_document_bytes


def main(args: list[str]) -> int:
    use_bytes = args[-1:] == ["--bytes"]
    if use_bytes:
        args = args[:-1]
    if len(args) not in (2, 3):
        print("Usage: python3 reference_packages.py INPUT.json REPORT.json [GROUP_BY] [--bytes]")
        return 2
    group_by = args[2] if len(args) == 3 else "tag"
    if use_bytes:
        source = Path(args[0]).read_bytes()
        report = analyze_document_bytes(source, group_by)
    else:
        source = Path(args[0]).read_text(encoding="utf-8")
        report = analyze_document(source, group_by)
    encoded = json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    output = Path(args[1])
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=output.parent, delete=False) as handle:
        handle.write(encoded)
        staged = Path(handle.name)
    staged.replace(output)
    print("Rows:")
    print(report["count"])
    print("Total:")
    print(report["total"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
