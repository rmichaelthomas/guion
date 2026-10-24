from __future__ import annotations

from datetime import datetime, timezone
import json
import re


_RFC3339 = re.compile(
    r"^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})$"
)


def parse_events(source: str) -> dict[str, object]:
    """Decode JSONL records and normalize timestamps, leaving grouping to Veritas."""
    events: list[dict[str, object]] = []
    invalid: list[dict[str, int]] = []
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    for line_number, line in enumerate(source.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
            if not isinstance(record, dict):
                raise ValueError("record must be an object")
            service = record.get("service")
            raw_at = record.get("at")
            if not isinstance(service, str) or not service or not isinstance(raw_at, str) or not _RFC3339.fullmatch(raw_at):
                raise ValueError("record must have string service and RFC3339 at fields")
            parsed_at = datetime.fromisoformat(raw_at[:-1] + "+00:00" if raw_at[-1:] in ("Z", "z") else raw_at)
            if parsed_at.tzinfo is None or parsed_at.utcoffset() is None:
                raise ValueError("timestamp must include a timezone")
            utc_at = parsed_at.astimezone(timezone.utc)
            normalized = utc_at.isoformat(timespec="auto").replace("+00:00", "Z")
            delta = utc_at - epoch
            epoch_us = (delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds
            events.append({"service": service, "date": utc_at.date().isoformat(), "at": normalized, "epoch_us": epoch_us})
        except (json.JSONDecodeError, TypeError, ValueError, OverflowError):
            invalid.append({"line": line_number})
    return {"events": events, "invalid": invalid}


def parse_services(source: str) -> list[str]:
    return [line.removesuffix("\r") for line in source.splitlines() if line.removesuffix("\r")]
