"""Standard-library adapter for the Veritas support SLA CLI."""

from __future__ import annotations

import argparse
import calendar
import csv
import json
import os
import re
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any


CSV_FIELDS = ("id", "team", "priority", "status", "created_at")
RFC3339 = re.compile(
    r"(?P<date>\d{4}-\d{2}-\d{2})T"
    r"(?P<time>\d{2}:\d{2}:\d{2})"
    r"(?:\.(?P<fraction>\d+))?"
    r"(?P<offset>Z|[+-]\d{2}:\d{2})"
)


@dataclass(frozen=True)
class SlaError(Exception):
    code: str
    message: str

    def __str__(self) -> str:
        return f"{self.code}: {self.message}"


@dataclass(frozen=True)
class Instant:
    whole_seconds: int
    fraction: Decimal
    utc_datetime: datetime


def _parse_rfc3339(value: str, label: str) -> Instant:
    match = RFC3339.fullmatch(value)
    if match is None:
        raise SlaError("invalid_timestamp", f"{label} is not RFC3339: {value!r}")

    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)
    except ValueError as exc:
        raise SlaError("invalid_timestamp", f"{label} is invalid: {value!r}") from exc

    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise SlaError("invalid_timestamp", f"{label} must include a UTC offset")

    utc = parsed.astimezone(timezone.utc)
    second = calendar.timegm(utc.replace(microsecond=0).timetuple())
    fraction_text = match.group("fraction") or ""
    fraction = Decimal("0." + fraction_text) if fraction_text else Decimal(0)
    return Instant(second, fraction, utc)


def _format_utc(instant: Instant) -> str:
    base = instant.utc_datetime.strftime("%Y-%m-%dT%H:%M:%S")
    fraction = format(instant.fraction, "f").partition(".")[2].rstrip("0")
    if fraction:
        base += "." + fraction
    return base + "Z"


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def _load_policy(path: str) -> dict[str, int]:
    try:
        with open(path, "r", encoding="utf-8", newline="") as handle:
            raw = json.load(handle, object_pairs_hook=_reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as exc:
        raise SlaError("invalid_policy", f"cannot read policy JSON: {exc}") from exc

    if not isinstance(raw, dict) or not isinstance(raw.get("response_minutes"), dict):
        raise SlaError("invalid_policy", "policy must contain a response_minutes object")

    limits: dict[str, int] = {}
    for priority, value in raw["response_minutes"].items():
        if not isinstance(priority, str) or not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise SlaError("invalid_policy", "response_minutes values must be nonnegative integers")
        limits[priority] = value
    return limits


def _read_tickets(path: str) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    try:
        with open(path, "r", encoding="utf-8", newline="") as handle:
            reader = csv.reader(handle, strict=True)
            try:
                header = next(reader)
            except StopIteration as exc:
                raise SlaError("malformed_row", "ticket CSV is empty") from exc
            if tuple(header) != CSV_FIELDS:
                raise SlaError(
                    "malformed_row",
                    "ticket CSV header must be id,team,priority,status,created_at in that order",
                )

            for row_number, values in enumerate(reader, start=2):
                if len(values) != len(CSV_FIELDS):
                    raise SlaError("malformed_row", f"ticket row {row_number} has {len(values)} fields")
                row = dict(zip(CSV_FIELDS, values, strict=True))
                if any(not value or not value.strip() for value in row.values()):
                    raise SlaError("malformed_row", f"ticket row {row_number} contains an empty field")
                rows.append(row)
    except SlaError:
        raise
    except (OSError, UnicodeError, csv.Error) as exc:
        raise SlaError("malformed_row", f"cannot read ticket CSV: {exc}") from exc
    return rows


def _build_report(tickets_path: str, policy_path: str, as_of_text: str) -> dict[str, Any]:
    as_of = _parse_rfc3339(as_of_text, "--as-of")
    policy = _load_policy(policy_path)
    rows = _read_tickets(tickets_path)

    seen_ids: set[str] = set()
    report_tickets: list[dict[str, Any]] = []
    for row_number, row in enumerate(rows, start=2):
        ticket_id = row["id"]
        priority = row["priority"]
        status = row["status"]

        if ticket_id in seen_ids:
            raise SlaError("duplicate_id", f"duplicate ticket id {ticket_id!r} on row {row_number}")
        seen_ids.add(ticket_id)

        if priority not in ("high", "low"):
            raise SlaError("unknown_priority", f"unknown priority {priority!r} on row {row_number}")
        if priority not in policy:
            raise SlaError("missing_policy_entry", f"no response_minutes entry for {priority!r}")
        if status not in ("open", "closed"):
            raise SlaError("invalid_status", f"invalid status {status!r} on row {row_number}")

        created = _parse_rfc3339(row["created_at"], f"created_at on row {row_number}")
        if (created.whole_seconds, created.fraction) > (as_of.whole_seconds, as_of.fraction):
            raise SlaError("future_timestamp", f"created_at on row {row_number} is later than --as-of")

        elapsed_seconds = as_of.whole_seconds - created.whole_seconds
        if as_of.fraction < created.fraction:
            elapsed_seconds -= 1
        age_minutes = elapsed_seconds // 60
        limit_minutes = policy[priority]
        breached = status == "open" and age_minutes > limit_minutes
        report_tickets.append(
            {
                "id": ticket_id,
                "team": row["team"],
                "priority": priority,
                "status": status,
                "age_minutes": age_minutes,
                "limit_minutes": limit_minutes,
                "breached": breached,
            }
        )

    report_tickets.sort(key=lambda ticket: (ticket["team"], ticket["id"]))
    return {
        "as_of": _format_utc(as_of),
        "tickets": report_tickets,
        "summary": {
            "total": len(report_tickets),
            "breached": sum(1 for ticket in report_tickets if ticket["breached"]),
        },
    }


def _write_report_atomic(path: str, report: dict[str, Any]) -> None:
    target = Path(path)
    parent = target.parent if str(target.parent) else Path(".")
    temp_path: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            dir=parent,
            prefix=f".{target.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            temp_path = handle.name
            json.dump(report, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, target)
        temp_path = None
    finally:
        if temp_path is not None:
            try:
                os.unlink(temp_path)
            except FileNotFoundError:
                pass


def run_cli(arguments: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="sla-report")
    parser.add_argument("--tickets", required=True, metavar="PATH")
    parser.add_argument("--policy", required=True, metavar="PATH")
    parser.add_argument("--as-of", required=True, metavar="RFC3339")
    parser.add_argument("--output", required=True, metavar="PATH")
    try:
        options = parser.parse_args(arguments)
    except SystemExit as exc:
        return int(exc.code or 0)

    try:
        report = _build_report(options.tickets, options.policy, options.as_of)
        _write_report_atomic(options.output, report)
    except SlaError as exc:
        print(str(exc), file=sys.stderr)
        return 2
    except OSError as exc:
        print(f"io_error: {exc}", file=sys.stderr)
        return 2
    return 0
