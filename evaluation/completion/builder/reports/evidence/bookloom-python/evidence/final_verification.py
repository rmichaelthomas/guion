from __future__ import annotations

import hashlib
import json
import shlex
import sqlite3
import subprocess
import time
import tomllib
from datetime import date, timedelta
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "📚 Bookloom – bibliothèque"
CONFIG = PROJECT / "bookloom.toml"
with CONFIG.open("rb") as config_file:
    config = tomllib.load(config_file)
DB = Path(config["database_path"])
if not DB.is_absolute():
    DB = CONFIG.parent / DB
DB = DB.resolve()
AS_OF = date(2026, 10, 25)
WINDOW = config["checkout_days"]
events: list[dict[str, object]] = []


def run(*args: str, expect: int = 0, contains: str = "") -> subprocess.CompletedProcess[str]:
    command = ["./bookloom", *args]
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
for book_id in ("bk2", "bk1", "edge", "outside", "overdue"):
    run("add", "--id", book_id, "--title", f"Title {book_id}", "--author", "Test Author", contains="Added")
for book_id in ("bk2", "bk1", "edge", "outside", "overdue"):
    run("checkout", "--id", book_id, "--borrower", "Test Borrower", contains="Checked out")

available = run("list", "available", contains="No available books")
checked = run("list", "checked-out", contains="bk1 | Title bk1")
if checked.stdout.find("bk1 |") > checked.stdout.find("bk2 |"):
    raise AssertionError("fresh-process checked-out list was not in stable ID order")

def snapshot() -> list[tuple[object, ...]]:
    with sqlite3.connect(DB) as connection:
        return connection.execute(
            "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
            "FROM books ORDER BY book_id"
        ).fetchall()


def failed_without_change(*args: str, message: str) -> None:
    before = snapshot()
    run(*args, expect=2, contains=message)
    if snapshot() != before:
        raise AssertionError(f"failed command changed SQLite rows: {args}")


failed_without_change("add", "--id", "bk1", "--title", "Duplicate", message="already exists")
failed_without_change("checkout", "--id", "missing", "--borrower", "Test Borrower", message="unknown book ID")
failed_without_change("checkout", "--id", "bk1", "--borrower", "Different Borrower", message="already checked out")
failed_without_change("return", "--id", "missing", message="unknown book ID")

fixture_dates = {
    "bk1": AS_OF + timedelta(days=2),
    "bk2": AS_OF + timedelta(days=2),
    "edge": AS_OF + timedelta(days=WINDOW),
    "outside": AS_OF + timedelta(days=WINDOW + 1),
    "overdue": AS_OF - timedelta(days=1),
}
with sqlite3.connect(DB) as connection:
    for book_id, due_on in fixture_dates.items():
        connection.execute("UPDATE books SET due_on = ? WHERE book_id = ?", (due_on.isoformat(), book_id))
due = run("due-soon", "--as-of", AS_OF.isoformat())
due_ids = [line.split(" | ")[1] for line in due.stdout.splitlines()]
if due_ids != ["bk1", "bk2", "edge"]:
    raise AssertionError(f"unexpected due-soon order/window: {due_ids}")

before_hash = hashlib.sha256(DB.read_bytes()).hexdigest()
before_rows = snapshot()
for invalid_date in ("2026-02-30", "2026-2-3"):
    run("due-soon", "--as-of", invalid_date, expect=2, contains="invalid date")
    if hashlib.sha256(DB.read_bytes()).hexdigest() != before_hash or snapshot() != before_rows:
        raise AssertionError(f"invalid date {invalid_date} changed state")

run("return", "--id", "bk2", contains="Returned")
failed_without_change("return", "--id", "bk2", message="not checked out")
available = run("list", "available", contains="bk2 | Title bk2")
if "bk1 | Title bk1" in available.stdout:
    raise AssertionError("checked-out book appeared in available list")
with sqlite3.connect(DB) as connection:
    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
if integrity != "ok":
    raise AssertionError(f"SQLite integrity_check returned {integrity!r}")

output = ROOT / "evidence" / "final_verification.json"
output.write_text(json.dumps({
    "status": "passed",
    "project_path": str(PROJECT.resolve()),
    "database_path": str(DB.resolve()),
    "database_outside_project": not DB.is_relative_to(PROJECT.resolve()),
    "fresh_process_persistence": True,
    "failed_operations_left_rows_unchanged": True,
    "due_soon_order": due_ids,
    "due_soon_window_inclusive": [AS_OF.isoformat(), (AS_OF + timedelta(days=WINDOW)).isoformat()],
    "invalid_dates_left_hash_and_rows_unchanged": True,
    "integrity_check": integrity,
    "events": events,
}, indent=2, ensure_ascii=False) + "\n")
print(f"final acceptance verification passed; {len(events)} CLI invocations logged to {output}")
