import datetime as dt


def _parse(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("RFC3339 timestamp must include a timezone")
    return parsed.astimezone(dt.timezone.utc)


def within_window(now, start, end):
    instant = _parse(now)
    return _parse(start) <= instant <= _parse(end)
