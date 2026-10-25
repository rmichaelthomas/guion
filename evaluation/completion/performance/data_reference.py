#!/usr/bin/env python3
"""Independent standard-library JSONL and SQLite baselines for Task 10."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sqlite3
from pathlib import Path
from typing import Any


def parse_event(value: Any) -> tuple[str, str] | None:
    if not isinstance(value, dict):
        return None
    service = value.get("service")
    stamp = value.get("at")
    if not isinstance(service, str) or not service or not isinstance(stamp, str):
        return None
    if len(stamp) < 20 or stamp[-1] != "Z" or "T" not in stamp:
        return None
    try:
        parsed = dt.datetime.fromisoformat(stamp[:-1] + "+00:00")
    except ValueError:
        return None
    if parsed.tzinfo is None or parsed.second >= 60:
        return None
    return service, parsed.astimezone(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def read_events(path: Path, allowed: set[str]) -> tuple[list[tuple[str, str, str]], list[dict[str, int]], int]:
    accepted: list[tuple[str, str, str]] = []
    invalid: list[dict[str, int]] = []
    ignored = 0
    with path.open(encoding="utf-8", newline="") as source:
        for number, raw in enumerate(source, start=1):
            if not raw.strip():
                continue
            try:
                event = parse_event(json.loads(raw))
            except (json.JSONDecodeError, UnicodeDecodeError):
                event = None
            if event is None:
                invalid.append({"line": number})
            elif event[0] not in allowed:
                ignored += 1
            else:
                accepted.append((event[0], event[1][:10], event[1]))
    return accepted, invalid, ignored


def report_from_groups(groups: list[tuple[str, str, int, str, str]], invalid: list[dict[str, int]], ignored: int) -> dict[str, Any]:
    return {
        "groups": [
            {"service": service, "date": day, "count": count, "oldest": oldest, "newest": newest}
            for service, day, count, oldest, newest in sorted(groups)
        ],
        "invalid": invalid,
        "ignored_count": ignored,
    }


def analyze_python(events: Path, services: Path) -> dict[str, Any]:
    allowed = {line.rstrip("\r\n") for line in services.read_text(encoding="utf-8").splitlines()}
    accepted, invalid, ignored = read_events(events, allowed)
    grouped: dict[tuple[str, str], list[Any]] = {}
    for service, day, instant in accepted:
        key = (service, day)
        if key not in grouped:
            grouped[key] = [0, instant, instant]
        value = grouped[key]
        value[0] += 1
        value[1] = min(value[1], instant)
        value[2] = max(value[2], instant)
    groups = [(service, day, values[0], values[1], values[2]) for (service, day), values in grouped.items()]
    return report_from_groups(groups, invalid, ignored)


def analyze_sqlite(events: Path, services: Path) -> dict[str, Any]:
    allowed = {line.rstrip("\r\n") for line in services.read_text(encoding="utf-8").splitlines()}
    accepted, invalid, ignored = read_events(events, allowed)
    with sqlite3.connect(":memory:") as database:
        database.execute("CREATE TABLE events(service TEXT NOT NULL, day TEXT NOT NULL, instant TEXT NOT NULL)")
        database.executemany("INSERT INTO events VALUES(?,?,?)", accepted)
        groups = database.execute(
            "SELECT service,day,count(*),min(instant),max(instant) FROM events GROUP BY service,day ORDER BY service,day"
        ).fetchall()
    return report_from_groups(groups, invalid, ignored)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("python", "sqlite"))
    parser.add_argument("events", type=Path)
    parser.add_argument("services", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = analyze_python(args.events, args.services) if args.mode == "python" else analyze_sqlite(args.events, args.services)
    args.output.write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
