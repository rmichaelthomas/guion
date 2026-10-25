from __future__ import annotations

import json
import shlex
import sqlite3
import subprocess
import sys
import time
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "initial-project"
CONFIG = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else PROJECT / "bookloom.toml"
with CONFIG.open("rb") as config_file:
    database_value = tomllib.load(config_file)["database_path"]
DB = Path(database_value)
if not DB.is_absolute():
    DB = CONFIG.parent / DB
LOG = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else ROOT / "evidence" / "initial_checks.json"
events: list[dict[str, object]] = []


def snapshot() -> list[tuple[object, ...]]:
    if not DB.exists():
        return []
    with sqlite3.connect(DB) as connection:
        return connection.execute(
            "SELECT book_id, title, author, checked_out_to, checked_out_on, due_on "
            "FROM books ORDER BY book_id"
        ).fetchall()


def run(*args: str, expect: int = 0, contains: str = "") -> subprocess.CompletedProcess[str]:
    command = ["./bookloom", "--config", str(CONFIG), *args]
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


def failed_without_change(*args: str, contains: str) -> None:
    before = snapshot()
    run(*args, expect=2, contains=contains)
    after = snapshot()
    if after != before:
        raise AssertionError(f"failed command changed rows: {args}")


run("add", "--id", "bk-2", "--title", "Piranesi", "--author", "Susanna Clarke", contains="Added")
run("add", "--id", "bk-1", "--title", "Kindred", "--author", "Octavia E. Butler", contains="Added")
run("list", "available", contains="bk-1 | Kindred")
run("checkout", "--id", "bk-2", "--borrower", "Morgan Lee", contains="Checked out")
run("checkout", "--id", "bk-1", "--borrower", "Sam Rivera", contains="Checked out")
checked_out = run("list", "checked-out", contains="bk-1 | Kindred")
if checked_out.stdout.find("bk-1") > checked_out.stdout.find("bk-2"):
    raise AssertionError("checked-out list is not in stable ID order")
run("list", "checked-out", contains="bk-2 | Piranesi")  # fresh process proves persistence

failed_without_change("add", "--id", "bk-1", "--title", "Duplicate", contains="already exists")
failed_without_change("checkout", "--id", "missing", "--borrower", "Morgan Lee", contains="unknown book ID")
failed_without_change("checkout", "--id", "bk-1", "--borrower", "Different Person", contains="already checked out")
failed_without_change("return", "--id", "missing", contains="unknown book ID")

run("return", "--id", "bk-2", contains="Returned")
failed_without_change("return", "--id", "bk-2", contains="not checked out")
available = run("list", "available", contains="bk-2 | Piranesi")
if "bk-1 | Kindred" in available.stdout:
    raise AssertionError("still checked-out book appeared in available list")
run("list", "checked-out", contains="bk-1 | Kindred")
with sqlite3.connect(DB) as connection:
    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
if integrity != "ok":
    raise AssertionError(f"SQLite integrity_check returned {integrity!r}")

LOG.write_text(json.dumps({"status": "passed", "events": events, "integrity_check": integrity}, indent=2) + "\n")
print(f"initial checks passed; {len(events)} CLI invocations logged to {LOG}")
