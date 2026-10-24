"""Small standard-library adapters used by the Veritas service health CLI."""

from __future__ import annotations

from datetime import date
from decimal import Decimal
import json
import re


_RFC3339 = re.compile(
    r"^(?P<year>[0-9]{4})-(?P<month>0[1-9]|1[0-2])-(?P<day>0[1-9]|[12][0-9]|3[01])"
    r"[Tt](?P<hour>[01][0-9]|2[0-3]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9]|60)"
    r"(?:\.(?P<fraction>[0-9]+))?"
    r"(?P<zone>[Zz]|[+-](?:[01][0-9]|2[0-3]):[0-5][0-9])$"
)
_EPOCH_ORDINAL = date(1970, 1, 1).toordinal()


def _parse_instant(value: str) -> tuple[int, Decimal]:
    match = _RFC3339.fullmatch(value)
    if match is None:
        raise ValueError("invalid RFC3339 instant")

    year = int(match.group("year"))
    month = int(match.group("month"))
    day = int(match.group("day"))
    # date() rejects impossible Gregorian dates such as February 30.
    ordinal = date(year, month, day).toordinal()
    hour = int(match.group("hour"))
    minute = int(match.group("minute"))
    second = int(match.group("second"))
    fraction = match.group("fraction") or "0"
    zone = match.group("zone")

    offset_seconds = 0
    if zone not in ("Z", "z"):
        sign = 1 if zone[0] == "+" else -1
        offset_seconds = sign * (int(zone[1:3]) * 3600 + int(zone[4:6]) * 60)

    whole_seconds = (ordinal - _EPOCH_ORDINAL) * 86400
    whole_seconds += hour * 3600 + minute * 60 + min(second, 59)
    whole_seconds -= offset_seconds
    if second == 60:
        whole_seconds += 1
    return whole_seconds, Decimal("0." + fraction)


def valid_rfc3339(value: str) -> bool:
    try:
        _parse_instant(value)
        return True
    except (ValueError, OverflowError):
        return False


def maintenance_window_valid(start: str, end: str) -> bool:
    try:
        return _parse_instant(start) <= _parse_instant(end)
    except (ValueError, OverflowError):
        return False


def in_maintenance(now: str, start: str, end: str) -> bool:
    current = _parse_instant(now)
    return _parse_instant(start) <= current <= _parse_instant(end)


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def _reject_constant(value: str) -> None:
    raise ValueError(f"invalid JSON constant: {value}")


def classify_health_body(body: str) -> dict[str, object]:
    try:
        value = json.loads(
            body,
            object_pairs_hook=_unique_object,
            parse_constant=_reject_constant,
        )
    except (json.JSONDecodeError, ValueError, RecursionError):
        return {"status": "invalid_response", "raw_status": None}

    if not isinstance(value, dict):
        return {"status": "invalid_response", "raw_status": None}
    raw_status = value.get("status")
    if not isinstance(raw_status, str):
        return {"status": "invalid_response", "raw_status": None}

    try:
        raw_status.encode("utf-8", "strict")
    except UnicodeEncodeError:
        return {"status": "invalid_response", "raw_status": None}

    status = "healthy" if raw_status == "ok" else "unhealthy"
    return {"status": status, "raw_status": raw_status}
