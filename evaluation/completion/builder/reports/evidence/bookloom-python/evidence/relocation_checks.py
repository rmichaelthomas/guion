from __future__ import annotations

import hashlib
import json
import shlex
import sqlite3
import statistics
import subprocess
import time
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "📚 Bookloom – bibliothèque"
DB = ROOT / "caller-data" / "bookloom.sqlite3"
events: list[dict[str, object]] = []


def run(*args: str, expect: int = 0, contains: str = "") -> subprocess.CompletedProcess[str]:
    command = ["./bookloom", *args]
    started = time.perf_counter()
    result = subprocess.run(command, cwd=PROJECT, text=True, capture_output=True)
    elapsed = time.perf_counter() - started
    output = result.stdout + result.stderr
    events.append({
        "command": shlex.join(command),
        "cwd": str(PROJECT),
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


if PROJECT.resolve() == DB.parent.resolve() or DB.is_relative_to(PROJECT.resolve()):
    raise AssertionError("database is not separate from relocated program directory")
if DB.exists():
    DB.unlink()
run("add", "--id", "rel-2", "--title", "A Book in the Café", "--author", "M. Étoile", contains="Added")
run("add", "--id", "rel-1", "--title", "Paper Moons", "--author", "N. Rivers", contains="Added")
run("checkout", "--id", "rel-2", "--borrower", "Zoë Martin", contains="Checked out")
checked_out = run("list", "checked-out", contains="rel-2 | A Book in the Café")
if "Zoë Martin" not in checked_out.stdout:
    raise AssertionError("Unicode borrower name was not preserved")
today = date.today().isoformat()
due = run("due-soon", "--as-of", today, contains="rel-2")
if "rel-2" not in due.stdout:
    raise AssertionError("checked-out book due inside checkout window was not listed")
run("return", "--id", "rel-2", contains="Returned")
available = run("list", "available", contains="rel-1 | Paper Moons")
if available.stdout.find("rel-1") > available.stdout.find("rel-2"):
    raise AssertionError("available list is not stable by book ID")

before_hash = hashlib.sha256(DB.read_bytes()).hexdigest()
with sqlite3.connect(DB) as connection:
    before_rows = connection.execute(
        "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
        "FROM books ORDER BY book_id"
    ).fetchall()
run("due-soon", "--as-of", "2026-02-30", expect=2, contains="invalid date")
after_hash = hashlib.sha256(DB.read_bytes()).hexdigest()
with sqlite3.connect(DB) as connection:
    after_rows = connection.execute(
        "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
        "FROM books ORDER BY book_id"
    ).fetchall()
if after_hash != before_hash or after_rows != before_rows:
    raise AssertionError("invalid date changed relocated SQLite state")
if not DB.exists() or not DB.is_file():
    raise AssertionError("database was not created in the caller-data directory")

launch_seconds = []
for _ in range(5):
    launch = run("list", "available")
    if launch.returncode != 0:
        raise AssertionError("installed-launch command failed")
    launch_seconds.append(events[-1]["elapsed_seconds"])

with sqlite3.connect(DB) as connection:
    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
if integrity != "ok":
    raise AssertionError(f"SQLite integrity_check returned {integrity!r}")

output = ROOT / "evidence" / "relocation_checks.json"
output.write_text(json.dumps({
    "status": "passed",
    "project_path": str(PROJECT.resolve()),
    "project_path_has_unicode_and_spaces": any(ord(ch) > 127 for ch in PROJECT.name) and " " in PROJECT.name,
    "database_path": str(DB.resolve()),
    "database_separate_from_project": not DB.is_relative_to(PROJECT.resolve()),
    "invalid_date_hash_unchanged": True,
    "invalid_date_rows_unchanged": True,
    "installed_launch_seconds": launch_seconds,
    "installed_launch_median_seconds": statistics.median(launch_seconds),
    "integrity_check": integrity,
    "events": events,
}, indent=2, ensure_ascii=False) + "\n")
print(
    f"relocation checks passed; median launch {statistics.median(launch_seconds):.6f}s; "
    f"logs: {output}"
)
