"""Array-backed analysis using two declared compiled Python packages."""

import numpy as np
import orjson


def summarize(column: list[int]) -> dict:
    values = np.fromiter(column, dtype=np.int64, count=len(column))
    total = sum(column)
    return {
        "count": int(values.size),
        "maximum": int(values.max()) if values.size else 0,
        "mean": round(total / len(column), 6) if column else 0.0,
        "minimum": int(values.min()) if values.size else 0,
        "total": total,
    }


def analyze_document(source: str, group_by: str) -> dict:
    document = orjson.loads(source)
    rows = document["rows"]
    column = [row["value"] for row in rows]
    if any(type(value) is not int or value < -(1 << 63) or value > (1 << 63) - 1 for value in column):
        raise ValueError("analysis values must fit signed 64-bit integers")
    grouped: dict[str, list[int]] = {}
    for row in rows:
        key = row[group_by]
        if not isinstance(key, str):
            raise ValueError(f"group values in {group_by!r} must be strings")
        grouped.setdefault(key, []).append(row["value"])
    report = summarize(column)
    report["group_by"] = group_by
    report["groups"] = {key: summarize(values) for key, values in grouped.items()}
    return report
