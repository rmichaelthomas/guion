from __future__ import annotations

import hashlib
import json
import shlex
import sqlite3
import subprocess
import sys
import time
import tomllib
from datetime import date, timedelta
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "initial-project"
CONFIG = PROJECT / "bookloom-due-soon-test.toml"
with CONFIG.open("rb") as config_file:
    CONFIG_DATA = tomllib.load(config_file)
DB = Path(CONFIG_DATA["database_path"])
if not DB.is_absolute():
    DB = CONFIG.parent / DB
AS_OF = date(2026, 10, 25)
CHECKOUT_DAYS = CONFIG_DATA["checkout_days"]
events: list[dict[str, object]] = []


def run(*args: str, config: Path = CONFIG, expect: int = 0, contains: str = "") -> subprocess.CompletedProcess[str]:
    command = ["./bookloom", "--config", str(config), *args]
    started = time.perf_counter()
    result = subprocess.run(command, cwd=PROJECT, text=True, capture_output=True)
    elapsed = time.perf_counter() - started
    output = result.stdout + result.stderr
    events.append({
        "command": shlex.join(command),
        "returncode": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "elapsed_seconds": round(elapsed, 6),
    })
    if result.returncode != expect:
        raise AssertionError(f"{command}: expected {expect}, got {result.returncode}: {output}")
    if contains and contains not in output:
        raise AssertionError(f"{command}: missing {contains!r}: {output}")
    return result


if DB.exists():
    DB.unlink()
fixture_books = ["due-today", "due-a", "due-z", "edge", "outside", "overdue"]
for book_id in fixture_books:
    run("add", "--id", book_id, "--title", book_id.title(), "--author", "Fixture Author", contains="Added")
    run("checkout", "--id", book_id, "--borrower", "Fixture Borrower", contains="Checked out")

fixtures = {
    "due-today": AS_OF,
    "due-a": AS_OF + timedelta(days=2),
    "due-z": AS_OF + timedelta(days=2),
    "edge": AS_OF + timedelta(days=CHECKOUT_DAYS),
    "outside": AS_OF + timedelta(days=CHECKOUT_DAYS + 1),
    "overdue": AS_OF - timedelta(days=1),
}
with sqlite3.connect(DB) as connection:
    for book_id, due_on in fixtures.items():
        connection.execute("UPDATE books SET due_on = ? WHERE book_id = ?", (due_on.isoformat(), book_id))

listing = run("due-soon", "--as-of", AS_OF.isoformat())
ids = [line.split(" | ")[1] for line in listing.stdout.splitlines()]
expected_ids = ["due-today", "due-a", "due-z", "edge"]
if ids != expected_ids:
    raise AssertionError(f"due-soon ordering/window mismatch: expected {expected_ids}, got {ids}")
if "outside" in listing.stdout or "overdue" in listing.stdout:
    raise AssertionError("due-soon included an out-of-window or overdue book")

def digest() -> str:
    return hashlib.sha256(DB.read_bytes()).hexdigest()


before_hash = digest()
with sqlite3.connect(DB) as connection:
    before_rows = connection.execute(
        "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
        "FROM books ORDER BY book_id"
    ).fetchall()
for invalid_date in ("2026-02-30", "2026-2-3"):
    run("due-soon", "--as-of", invalid_date, expect=2, contains="invalid date")
    if digest() != before_hash:
        raise AssertionError(f"invalid date {invalid_date} changed SQLite file bytes")
    with sqlite3.connect(DB) as connection:
        after_rows = connection.execute(
            "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
            "FROM books ORDER BY book_id"
        ).fetchall()
    if after_rows != before_rows:
        raise AssertionError(f"invalid date {invalid_date} changed database rows")

missing_config = PROJECT / "bookloom-missing-db-test.toml"
missing_data_dir = ROOT / "must-not-be-created-for-invalid-date"
missing_config.write_text(
    f'checkout_days = 14\ndatabase_path = "../{missing_data_dir.name}/catalog.sqlite3"\n'
)
if missing_data_dir.exists():
    raise AssertionError("missing-db fixture path already exists")
run("due-soon", "--as-of", "2026-02-30", config=missing_config, expect=2, contains="invalid date")
if missing_data_dir.exists():
    raise AssertionError("invalid date caused database directory creation")

with sqlite3.connect(DB) as connection:
    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
if integrity != "ok":
    raise AssertionError(f"SQLite integrity_check returned {integrity!r}")

output = ROOT / "evidence" / "followup_due_soon_checks.json"
output.write_text(json.dumps({
    "status": "passed",
    "as_of": AS_OF.isoformat(),
    "checkout_days": CHECKOUT_DAYS,
    "window_inclusive": [AS_OF.isoformat(), (AS_OF + timedelta(days=CHECKOUT_DAYS)).isoformat()],
    "fixture_due_dates": {key: value.isoformat() for key, value in fixtures.items()},
    "expected_order": expected_ids,
    "invalid_date_hash_unchanged": True,
    "invalid_date_rows_unchanged": True,
    "invalid_date_did_not_create_database_directory": True,
    "integrity_check": integrity,
    "fixture_method": "Books were added and checked out through the CLI; due_on values were then seeded with SQL to exercise deterministic date-window boundaries and tie ordering.",
    "events": events,
}, indent=2) + "\n")
print(f"follow-up due-soon checks passed; {len(events)} CLI invocations logged to {output}")
