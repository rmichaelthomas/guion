from __future__ import annotations

import hashlib
import hmac
import json
import sqlite3


_ASCII_WHITESPACE = " \t\r\n\v\f"


def check_signature(secret_bytes: bytes, body: bytes, signature: str) -> str:
    try:
        secret = secret_bytes.decode("utf-8").strip(_ASCII_WHITESPACE).encode("utf-8")
    except UnicodeDecodeError:
        return "invalid_secret"
    if not secret:
        return "invalid_secret"
    expected = hmac.new(secret, body, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(expected, signature.lower()):
        return "invalid_signature"
    return "ok"


def decode_event(body: bytes) -> dict[str, object]:
    try:
        event = json.loads(body.decode("utf-8"))
        if not isinstance(event, dict) or not isinstance(event.get("id"), str) or not event["id"] or "payload" not in event:
            return {"ok": False, "event": {}}
        return {"ok": True, "event": event}
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {"ok": False, "event": {}}


def _connect(db_path: str) -> sqlite3.Connection:
    return sqlite3.connect(db_path, timeout=5)


def initialize(db_path: str) -> bool:
    with _connect(db_path) as db:
        db.execute(
            "CREATE TABLE IF NOT EXISTS events ("
            "id TEXT PRIMARY KEY, body BLOB NOT NULL, state TEXT NOT NULL, "
            "attempts INTEGER NOT NULL DEFAULT 0, category TEXT)"
        )
    return True


def insert_event(db_path: str, event_id: str, body: bytes) -> bool:
    try:
        with _connect(db_path) as db:
            db.execute("INSERT INTO events(id,body,state) VALUES(?,?,?)", (event_id, body, "pending"))
        return True
    except sqlite3.IntegrityError:
        return False


def pending_events(db_path: str) -> list[dict[str, object]]:
    with _connect(db_path) as db:
        rows = db.execute("SELECT id,body,attempts FROM events WHERE state='pending' ORDER BY id").fetchall()
    return [{"id": row[0], "body": bytes(row[1]).decode("utf-8"), "attempts": row[2]} for row in rows]


def record_attempt(db_path: str, event_id: str, delivered: bool, max_attempts: int) -> str:
    with _connect(db_path) as db:
        row = db.execute("SELECT attempts FROM events WHERE id=? AND state='pending'", (event_id,)).fetchone()
        if row is None:
            raise ValueError(f"event is not pending: {event_id}")
        attempts = row[0] + 1
        if delivered:
            state, category = "delivered", None
        elif max_attempts > 0 and attempts >= max_attempts:
            state, category = "dead_letter", "http_failure"
        else:
            state, category = "pending", None
        db.execute("UPDATE events SET attempts=?,state=?,category=? WHERE id=?", (attempts, state, category, event_id))
    return {"delivered": "delivered", "pending": "pending", "dead_letter": "dead_lettered"}[state]


def status_data(db_path: str) -> dict[str, object]:
    with _connect(db_path) as db:
        pending = [row[0] for row in db.execute("SELECT id FROM events WHERE state='pending' ORDER BY id")]
        delivered = [row[0] for row in db.execute("SELECT id FROM events WHERE state='delivered' ORDER BY id")]
    return {"pending": pending, "delivered": delivered}


def dead_letters_data(db_path: str) -> list[dict[str, object]]:
    with _connect(db_path) as db:
        rows = db.execute(
            "SELECT id,category,attempts FROM events WHERE state='dead_letter' ORDER BY id"
        ).fetchall()
    return [{"id": row[0], "category": row[1], "attempts": row[2]} for row in rows]
