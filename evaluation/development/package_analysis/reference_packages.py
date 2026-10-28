"""Run the same NumPy/orjson analysis function without the Guion bridge."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile

from analysis import analyze_document


def main(args: list[str]) -> int:
    if len(args) not in (2, 3):
        print("Usage: python3 reference_packages.py INPUT.json REPORT.json [GROUP_BY]")
        return 2
    group_by = args[2] if len(args) == 3 else "tag"
    source = Path(args[0]).read_text(encoding="utf-8")
    report = analyze_document(source, group_by)
    encoded = json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    output = Path(args[1])
    output.parent.mkdir(parents=True, exist_ok=True)
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
