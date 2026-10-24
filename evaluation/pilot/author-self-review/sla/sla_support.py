import csv
import datetime as dt
import io


def parse_timestamp(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timestamp must include a timezone")
    return int(parsed.timestamp())


def parse_tickets(value):
    rows = list(csv.DictReader(io.StringIO(value, newline="")))
    required = ("id", "team", "priority", "status", "created_at")
    ids = set()
    for row in rows:
        if not row or any(row.get(key) is None for key in required):
            raise ValueError("malformed CSV row")
        if row["id"] in ids:
            raise ValueError("duplicate ticket id: " + row["id"])
        ids.add(row["id"])
    rows.sort(key=lambda row: (row["team"], row["id"]))
    return rows


def minutes_since(as_of, created_at):
    end = dt.datetime.fromisoformat(as_of.replace("Z", "+00:00"))
    start = dt.datetime.fromisoformat(created_at.replace("Z", "+00:00"))
    if end.tzinfo is None or start.tzinfo is None:
        raise ValueError("timestamp must include a timezone")
    return int((end.astimezone(dt.timezone.utc) - start.astimezone(dt.timezone.utc)).total_seconds() // 60)


def write_stderr(value):
    import sys
    return sys.stderr.write(value)
