"""Seeded multi-size application inputs with independent expected results."""

from __future__ import annotations

import csv
from datetime import datetime, timedelta, timezone
import hashlib
import hmac
import io
import json
import random

SEED = 20261024
SIZES = (24, 128)
APPLICATIONS = (
    "sla-report",
    "jsonl-summary",
    "release-notes",
    "calendar-conflicts",
    "webhook-replay",
    "health-digest",
    "config-migrator",
    "doc-index",
    "lockfile-auditor",
)


def _rng(application: str, size: int) -> random.Random:
    return random.Random(SEED + APPLICATIONS.index(application) * 100_003 + size)


def _check(kind: str, file: str, value: object, path: str | None = None) -> dict:
    item = {"kind": kind, "file": file, "value": value}
    if path is not None:
        item["path"] = path
    return item


def _sla(size: int, rng: random.Random) -> dict:
    as_of = datetime(2025, 1, 2, 12, 0, tzinfo=timezone.utc)
    teams = ("api", "core", "docs,west", "ops")
    limits = {"high": 60, "low": 120}
    rows = []
    for index in range(size):
        priority = rng.choice(("high", "low"))
        age = rng.randrange(0, 241)
        rows.append(
            {
                "id": f"T-{index:04d}",
                "team": rng.choice(teams),
                "priority": priority,
                "status": rng.choice(("open", "closed")),
                "created_at": (as_of - timedelta(minutes=age)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "age_minutes": age,
                "limit_minutes": limits[priority],
            }
        )
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(("id", "team", "priority", "status", "created_at"))
    for row in rows:
        writer.writerow(tuple(row[key] for key in ("id", "team", "priority", "status", "created_at")))
    ordered = sorted(rows, key=lambda row: (row["team"], row["id"]))
    tickets = []
    breached = 0
    for row in ordered:
        is_breached = row["status"] == "open" and row["age_minutes"] > row["limit_minutes"]
        breached += int(is_breached)
        tickets.append(
            {
                "id": row["id"],
                "team": row["team"],
                "priority": row["priority"],
                "status": row["status"],
                "age_minutes": row["age_minutes"],
                "limit_minutes": row["limit_minutes"],
                "breached": is_breached,
            }
        )
    return {
        "id": f"generated-sla-{size}",
        "files": {
            "tickets.csv": output.getvalue(),
            "policy.json": json.dumps({"response_minutes": limits}),
        },
        "command_args": ["--tickets", "tickets.csv", "--policy", "policy.json", "--as-of", as_of.strftime("%Y-%m-%dT%H:%M:%SZ"), "--output", "report.json"],
        "expected_exit_code": 0,
        "checks": [_check("json_equals", "report.json", {"as_of": as_of.strftime("%Y-%m-%dT%H:%M:%SZ"), "tickets": tickets, "summary": {"total": size, "breached": breached}})],
    }


def _jsonl(size: int, rng: random.Random) -> dict:
    lines: list[str] = []
    groups: dict[tuple[str, str], list[datetime]] = {}
    invalid_lines: list[dict[str, int]] = []
    for index in range(size):
        if index % 31 == 7:
            lines.append("not-json")
            invalid_lines.append({"line": len(lines)})
        if index % 19 == 4:
            lines.append("   ")
        service = rng.choice(("api", "worker", "indexer"))
        utc = datetime(2025, 1, 1, tzinfo=timezone.utc) + timedelta(days=rng.randrange(4), minutes=rng.randrange(1440))
        local = utc.astimezone(timezone(timedelta(hours=2, minutes=30)))
        record = {"service": service, "at": local.isoformat(timespec="seconds")}
        lines.append(json.dumps(record, separators=(",", ":")))
        groups.setdefault((service, utc.strftime("%Y-%m-%d")), []).append(utc)
    expected_groups = []
    for (service, day), timestamps in sorted(groups.items()):
        expected_groups.append(
            {
                "service": service,
                "date": day,
                "count": len(timestamps),
                "oldest": min(timestamps).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "newest": max(timestamps).strftime("%Y-%m-%dT%H:%M:%SZ"),
            }
        )
    return {
        "id": f"generated-jsonl-{size}",
        "files": {"events.jsonl": "\n".join(lines) + "\n"},
        "command_args": ["--events", "events.jsonl", "--output", "report.json"],
        "expected_exit_code": 0,
        "checks": [_check("json_equals", "report.json", {"groups": expected_groups, "invalid": invalid_lines, "ignored_count": 0})],
    }


def _release_notes(size: int, _rng: random.Random) -> dict:
    files: dict[str, str] = {"release.json": json.dumps({"date": "2025-01-02", "category_order": ["Added"]})}
    releases = []
    markdown = ["# Release notes (2025-01-02)", ""]
    for patch in range(size - 1, -1, -1):
        version = f"1.0.{patch}"
        files[f"changes/{version}.json"] = json.dumps({"id": f"change-{patch:04d}", "category": "Added", "text": f"Generated item {patch:04d}"})
        markdown.extend((f"## {version}", "", "### Added", f"- Generated item {patch:04d}", ""))
        releases.append({"version": version, "entry_count": 1})
    return {
        "id": f"generated-release-notes-{size}",
        "files": files,
        "command_args": ["--changes", "changes", "--config", "release.json", "--markdown", "notes.md", "--summary", "summary.json"],
        "expected_exit_code": 0,
        "checks": [_check("file_equals", "notes.md", "\n".join(markdown)), _check("json_equals", "summary.json", {"releases": releases})],
    }


def _ics(name: str, prefix: str, size: int, offset_minutes: int) -> tuple[str, list[dict]]:
    base = datetime(2025, 1, 1, tzinfo=timezone.utc)
    lines = ["BEGIN:VCALENDAR", "VERSION:2.0"]
    events = []
    for index in range(size):
        start = base + timedelta(minutes=4 * index + offset_minutes)
        duration = 3 if prefix == "A" else 2
        end = start + timedelta(minutes=duration)
        event_id = f"{prefix}{index:04d}"
        lines.extend(("BEGIN:VEVENT", f"UID:{event_id}", f"DTSTART:{start.strftime('%Y%m%dT%H%M%SZ')}", f"DTEND:{end.strftime('%Y%m%dT%H%M%SZ')}", "STATUS:CONFIRMED", "END:VEVENT"))
        events.append({"id": event_id, "start": start, "end": end, "calendar": name})
    lines.append("END:VCALENDAR")
    return "\n".join(lines) + "\n", events


def _calendar(size: int, _rng: random.Random) -> dict:
    alpha_body, alpha = _ics("alpha", "A", size, 0)
    beta_body, beta = _ics("beta", "B", size, 2)
    conflicts = []
    for left, right in zip(alpha, beta):
        start = max(left["start"], right["start"])
        end = min(left["end"], right["end"])
        conflicts.append(
            {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "end": end.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "event_ids": sorted((left["id"], right["id"])),
                "calendars": ["alpha", "beta"],
            }
        )
    source = {"calendars": [{"name": "alpha", "url": "${http:calendar}/alpha.ics"}, {"name": "beta", "url": "${http:calendar}/beta.ics"}]}
    return {
        "id": f"generated-calendar-{size}",
        "files": {"sources.json": json.dumps(source)},
        "http_fixtures": {"calendar": {"/alpha.ics": {"status": 200, "body": alpha_body}, "/beta.ics": {"status": 200, "body": beta_body}}},
        "command_args": ["--sources", "sources.json", "--window-start", "2025-01-01T00:00:00Z", "--window-end", "2025-01-02T00:00:00Z", "--output", "report.json", "--concurrency", "2", "--timeout-ms", "2000"],
        "expected_exit_code": 0,
        "checks": [_check("json_equals", "report.json", {"conflicts": conflicts, "source_errors": []}), {"kind": "http_request_count_equals", "fixture": "calendar", "value": 2}],
    }


def _webhook(size: int, _rng: random.Random) -> dict:
    secret = f"generated-secret-{SEED}"
    files = {"secret.txt": secret, "accepted/.keep": ""}
    http_responses = []
    steps = [
        {"command_args": ["init", "--db", "state.sqlite", "--secret-file", "secret.txt"], "expected_exit_code": 0, "checks": [{"kind": "file_exists", "file": "state.sqlite"}]}
    ]
    ids = []
    for index in range(size):
        event_id = f"generated-{index:04d}"
        ids.append(event_id)
        body = f'{{"id": "{event_id}", "payload": {{"index": {index}, "seed": {SEED}}}}}\n'
        files[f"events/{index:04d}.json"] = body
        signature = hmac.new(secret.encode(), body.encode(), hashlib.sha256).hexdigest()
        steps.append(
            {
                "command_args": ["ingest", "--db", "state.sqlite", "--secret-file", "secret.txt", "--body", f"events/{index:04d}.json", "--signature", signature, "--result", f"accepted/{index:04d}.json"],
                "expected_exit_code": 0,
                "checks": [_check("json_equals", f"accepted/{index:04d}.json", {"accepted": True, "id": event_id})],
            }
        )
        http_responses.append({"status": 204, "body": ""})
    ordered_ids = sorted(ids)
    steps.append(
        {
            "command_args": ["deliver", "--db", "state.sqlite", "--target", "${http:downstream}/events", "--result", "delivered.json"],
            "expected_exit_code": 0,
            "checks": [_check("json_equals", "delivered.json", {"pending": [], "delivered": ordered_ids}), {"kind": "http_request_count_equals", "fixture": "downstream", "path": "/events", "value": size}],
        }
    )
    steps.append(
        {
            "command_args": ["status", "--db", "state.sqlite", "--output", "status.json"],
            "expected_exit_code": 0,
            "checks": [_check("json_equals", "status.json", {"pending": [], "delivered": ordered_ids})],
        }
    )
    return {"id": f"generated-webhook-{size}", "files": files, "http_fixtures": {"downstream": {"/events": http_responses}}, "steps": steps, "redact": [secret], "expected_exit_code": 0}


def _health(size: int, rng: random.Random) -> dict:
    names = [f"service-{index:04d}" for index in range(size)]
    shuffled = names[:]
    rng.shuffle(shuffled)
    services = []
    fixture = {}
    expected = []
    for index, name in enumerate(shuffled):
        path = f"/probe/{index:04d}"
        services.append({"name": name, "url": "${http:health}" + path})
        if index % 3 == 0:
            body, state, raw = {"status": "ok"}, "healthy", "ok"
        elif index % 3 == 1:
            body, state, raw = {"status": "degraded"}, "unhealthy", "degraded"
        else:
            body, state, raw = "not-json", "invalid_response", None
        fixture[path] = {"status": 200, "body": body}
        expected.append({"service": name, "status": state, "raw_status": raw})
    expected.sort(key=lambda row: row["service"].casefold())
    digest = "".join(f"{row['service']} {row['status']}\n" for row in expected)
    return {
        "id": f"generated-health-{size}",
        "files": {"services.json": json.dumps({"services": services})},
        "http_fixtures": {"health": fixture},
        "command_args": ["--config", "services.json", "--json", "report.json", "--text", "digest.txt", "--concurrency", "2", "--timeout-ms", "2000"],
        "expected_exit_code": 0,
        "checks": [_check("json_equals", "report.json", {"results": expected}), _check("file_equals", "digest.txt", digest), {"kind": "http_request_count_equals", "fixture": "health", "value": size}, {"kind": "http_max_in_flight_at_most", "fixture": "health", "value": 2}],
    }


def _config(size: int, _rng: random.Random) -> dict:
    custom = {f"key_{index:04d}": f"value-{SEED}-{index:04d}" for index in range(size)}
    lines = ["# generated by fixed seed", "version = 1 # preserve this inline note", 'name = "generated"', "", "[custom]"]
    lines.extend(f'{key} = "{value}"' for key, value in custom.items())
    expected = {"version": 2, "name": "generated", "custom": custom, "sync": {"timeout_seconds": 30}}
    return {
        "id": f"generated-config-{size}",
        "files": {"config.toml": "\n".join(lines) + "\n"},
        "command_args": ["migrate", "--file", "config.toml"],
        "expected_exit_code": 0,
        "checks": [_check("toml_equals", "config.toml", expected), _check("file_contains", "config.toml", "version = 2 # preserve this inline note"), _check("file_contains", "config.toml", "timeout_seconds = 30")],
    }


def _doc_index(size: int, _rng: random.Random) -> dict:
    files = {}
    paths = [f"Note-{index:04d}.md" for index in range(size)]
    title = lambda index: f"Café Note {index:04d}"
    for index, path in enumerate(paths):
        target = "" if index == 0 else f"See [[{title(0)}]]."
        files[f"docs/{path}"] = f'---\naliases: ["Alias {index:04d}"]\n---\n# {title(index)}\n{target}\n'
    expected_backlinks = sorted(paths[1:], key=lambda path: (path.casefold(), path))
    return {
        "id": f"generated-doc-index-{size}",
        "files": files,
        "steps": [
            {"command_args": ["build", "--root", "docs", "--database", "index.sqlite"], "expected_exit_code": 0, "checks": [{"kind": "file_exists", "file": "index.sqlite"}]},
            {"command_args": ["check", "--database", "index.sqlite", "--json", "report.json"], "expected_exit_code": 0, "checks": [_check("json_equals", "report.json", {"missing": [], "ambiguous": []})]},
            {"command_args": ["backlinks", "--database", "index.sqlite", "--json", "backlinks.json", title(0)], "expected_exit_code": 0, "checks": [_check("json_equals", "backlinks.json", {"backlinks": expected_backlinks})]},
        ],
    }


def _lockfile(size: int, _rng: random.Random) -> dict:
    packages = []
    denied = []
    minimum = {}
    maximum = {}
    violations = []
    for index in range(size):
        name = f"pkg-{index:04d}"
        version = "1.0.0"
        source = None if index % 11 == 0 else "registry"
        checksum = None if index % 13 == 0 else f"sha256:{index:064x}"
        if index % 17 == 0:
            denied.append(name)
            violations.append({"name": name, "rule": "denied"})
        if index % 7 == 0:
            minimum[name] = "2.0.0"
            violations.append({"name": name, "rule": "below_minimum"})
        if index % 19 == 0:
            maximum[name] = "0.9.0"
            violations.append({"name": name, "rule": "above_maximum"})
        if source is None:
            violations.append({"name": name, "rule": "missing_source"})
        if checksum is None:
            violations.append({"name": name, "rule": "missing_checksum"})
        packages.append({"name": name, "version": version, "source": source, "checksum": checksum})
    violations.sort(key=lambda row: (row["name"], row["rule"]))
    policy = {"deny": denied, "minimum_versions": minimum, "maximum_versions": maximum, "require_source": True, "require_checksum": True}
    return {
        "id": f"generated-lockfile-{size}",
        "files": {"lock.json": json.dumps({"packages": packages}), "policy.json": json.dumps(policy)},
        "command_args": ["--lockfile", "lock.json", "--policy", "policy.json", "--output", "report.json"],
        "expected_exit_code": 0,
        "checks": [_check("json_equals", "report.json", {"violations": violations, "warnings": []})],
    }


_GENERATORS = {
    "sla-report": _sla,
    "jsonl-summary": _jsonl,
    "release-notes": _release_notes,
    "calendar-conflicts": _calendar,
    "webhook-replay": _webhook,
    "health-digest": _health,
    "config-migrator": _config,
    "doc-index": _doc_index,
    "lockfile-auditor": _lockfile,
}


def generated_cases(application: str) -> list[dict]:
    generator = _GENERATORS[application]
    return [generator(size, _rng(application, size)) for size in SIZES]
