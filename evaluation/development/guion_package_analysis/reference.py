"""Independent standard-library implementation of the package-analysis task."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile


def summarize(values: list[int]) -> dict:
    count = len(values)
    total = sum(values)
    return {
        "count": count,
        "maximum": max(values) if values else 0,
        "mean": round(total / count, 6) if count else 0.0,
        "minimum": min(values) if values else 0,
        "total": total,
    }


def analyze_document(source: str, group_by: str) -> dict:
    rows = json.loads(source)["rows"]
    values = [row["value"] for row in rows]
    if any(type(value) is not int for value in values):
        raise ValueError("analysis values must be integers")
    grouped: dict[str, list[int]] = {}
    for row in rows:
        key = row[group_by]
        if not isinstance(key, str):
            raise ValueError(f"group values in {group_by!r} must be strings")
        grouped.setdefault(key, []).append(row["value"])
    report = summarize(values)
    report["group_by"] = group_by
    report["groups"] = {key: summarize(items) for key, items in grouped.items()}
    return report


def main(args: list[str]) -> int:
    if len(args) not in (2, 3):
        print("Usage: python3 reference.py INPUT.json REPORT.json [GROUP_BY]")
        return 2
    group_by = args[2] if len(args) == 3 else "tag"
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
