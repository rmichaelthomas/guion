"""NumPy/orjson analysis shared by the Guion Text and Bytes bridge paths."""

import numpy as np
import orjson


def _analyze_document(document: dict, group_by: str) -> dict:
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

    def summarize(items: list[int]) -> dict:
        array = np.fromiter(items, dtype=np.int64, count=len(items))
        total = sum(items)
        return {
            "count": int(array.size),
            "maximum": int(array.max()) if array.size else 0,
            "mean": round(total / len(items), 6) if items else 0.0,
            "minimum": int(array.min()) if array.size else 0,
            "total": total,
        }

    report = summarize(column)
    report["group_by"] = group_by
    report["groups"] = {key: summarize(items) for key, items in grouped.items()}
    return report


def analyze_document(source: str, group_by: str) -> dict:
    return _analyze_document(orjson.loads(source), group_by)


def analyze_document_bytes(source: bytes, group_by: str) -> dict:
    return _analyze_document(orjson.loads(source), group_by)
