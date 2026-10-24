from __future__ import annotations

from datetime import date, datetime, time, timezone
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent / ".deps"))
from icalendar import Calendar


def _instant(value: str) -> datetime:
    parsed = datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ValueError("time must include a timezone")
    return parsed.astimezone(timezone.utc)


def _component_instant(value) -> datetime:
    if isinstance(value, datetime):
        parsed = value
    elif isinstance(value, date):
        parsed = datetime.combine(value, time.min, tzinfo=timezone.utc)
    else:
        raise ValueError("VEVENT start and end must be dates or date-times")
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _epoch_us(value: datetime) -> int:
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    delta = value - epoch
    return (delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds


def _utc_text(value: datetime) -> str:
    return value.isoformat(timespec="auto").replace("+00:00", "Z")


def parse_calendar(source: str, name: str, window_start: str, window_end: str, include_tentative: bool) -> dict[str, object]:
    try:
        start_limit = _instant(window_start)
        end_limit = _instant(window_end)
        if start_limit >= end_limit:
            raise ValueError("window start must be before window end")
        calendar = Calendar.from_ical(source)
        events = []
        for component in calendar.walk("VEVENT"):
            status = str(component.get("STATUS", "CONFIRMED")).upper()
            if status == "CANCELLED" or (status == "TENTATIVE" and not include_tentative):
                continue
            raw_start = component.decoded("DTSTART")
            raw_end = component.decoded("DTEND")
            event_start = _component_instant(raw_start)
            event_end = _component_instant(raw_end)
            if event_start >= event_end:
                raise ValueError("VEVENT end must be after start")
            if event_start >= end_limit or event_end <= start_limit:
                continue
            clipped_start = max(event_start, start_limit)
            clipped_end = min(event_end, end_limit)
            uid = component.decoded("UID")
            if isinstance(uid, bytes):
                uid = uid.decode("utf-8")
            if uid is None or not str(uid):
                raise ValueError("VEVENT requires a non-empty UID")
            events.append({
                "calendar": name,
                "id": str(uid),
                "start": _utc_text(clipped_start),
                "end": _utc_text(clipped_end),
                "start_us": _epoch_us(clipped_start),
                "end_us": _epoch_us(clipped_end),
            })
        return {"ok": True, "events": events}
    except Exception as exc:
        return {"ok": False, "error": str(exc), "events": []}


def sort_conflicts(values: list[dict[str, object]]) -> list[dict[str, object]]:
    ordered = sorted(values, key=lambda item: (item["start_us"], item["end_us"], item["event_ids"]))
    return [
        {key: value for key, value in item.items() if key not in ("start_us", "end_us")}
        for item in ordered
    ]
