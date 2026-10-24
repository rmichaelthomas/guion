"""Independent standard-library reference for package-backed analysis."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile


def main(args: list[str]) -> int:
    if len(args) not in (2, 3):
        print("Usage: python3 reference.py INPUT.json REPORT.json [GROUP_BY]")
        return 2
    group_by = args[2] if len(args) == 3 else "tag"
    try:
        with open(args[0], encoding="utf-8") as source:
            rows = json.load(source)["rows"]
            values = [row["value"] for row in rows]
            grouped: dict[str, list[int]] = {}
            for row in rows:
                key = row[group_by]
                if not isinstance(key, str):
                    raise ValueError(f"group values in {group_by!r} must be strings")
                grouped.setdefault(key, []).append(row["value"])
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError) as error:
        print(f"Cannot analyze input: {type(error).__name__}")
        return 1
    total = sum(values)
    report = {
        "count": len(values),
        "maximum": max(values, default=0),
        "mean": round(total / len(values), 6) if values else 0.0,
        "minimum": min(values, default=0),
        "total": total,
        "group_by": group_by,
        "groups": {
            key: {
                "count": len(group_values),
                "maximum": max(group_values, default=0),
                "mean": round(sum(group_values) / len(group_values), 6) if group_values else 0.0,
                "minimum": min(group_values, default=0),
                "total": sum(group_values),
            }
            for key, group_values in grouped.items()
        },
    }
    encoded = json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    output = Path(args[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=output.parent, delete=False) as handle:
        handle.write(encoded)
        staged = Path(handle.name)
    staged.replace(output)
    print("Rows:")
    print(len(values))
    print("Total:")
    print(total)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
