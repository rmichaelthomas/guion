"""Parse and verify saved Codex thread-usage evidence."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any


TERMINAL_TURN_STATES = {"completed", "failed", "interrupted", "cancelled", "needs_attention"}
TOOL_ITEM_TYPES = {"commandExecution", "functionCallOutput"}
NON_TOOL_ITEM_TYPES = {"agentMessage", "reasoning"}


def _status_type(value: Any) -> Any:
    return value.get("type") if isinstance(value, dict) else value


def summarize_thread_snapshot(snapshot: Any, expected_session_id: str) -> dict[str, int]:
    """Return usage totals from the JSON payload returned by read_thread."""
    if not isinstance(snapshot, dict) or snapshot.get("schemaVersion") != 1:
        raise ValueError("usage snapshot must be a read_thread schemaVersion 1 object")
    thread = snapshot.get("thread")
    if not isinstance(thread, dict) or thread.get("id") != expected_session_id:
        raise ValueError("usage snapshot thread ID does not match the assigned session ID")
    turns = snapshot.get("turns")
    if not isinstance(turns, list) or not turns:
        raise ValueError("usage snapshot must contain at least one turn")
    page = snapshot.get("page")
    if not isinstance(page, dict) or not isinstance(page.get("hasMore"), bool):
        raise ValueError("usage snapshot must include read_thread page completeness metadata")
    if page.get("hasMore") or page.get("nextCursor") is not None:
        raise ValueError("usage snapshot is incomplete; fetch every cursor page before recording usage")
    limit = page.get("limit")
    if isinstance(limit, bool) or not isinstance(limit, int) or limit < 1 or len(turns) > limit:
        raise ValueError("usage snapshot page limit is missing or inconsistent with its turn count")

    total_ms = 0
    tool_ids: set[str] = set()
    turn_ids: set[str] = set()
    for index, turn in enumerate(turns):
        if not isinstance(turn, dict):
            raise ValueError(f"usage snapshot turn {index} must be an object")
        turn_id = turn.get("id")
        if not isinstance(turn_id, str) or not turn_id or turn_id in turn_ids:
            raise ValueError(f"usage snapshot turn {index} must have a unique ID")
        turn_ids.add(turn_id)
        state = _status_type(turn.get("status"))
        if state not in TERMINAL_TURN_STATES:
            raise ValueError(f"usage snapshot turn {index} must be terminal")
        duration = turn.get("durationMs")
        if isinstance(duration, bool) or not isinstance(duration, int) or duration < 0:
            raise ValueError(f"usage snapshot turn {index} durationMs must be a non-negative integer")
        total_ms += duration
        items = turn.get("items")
        if not isinstance(items, list):
            raise ValueError(f"usage snapshot turn {index} must include its items list")
        for item in items:
            if not isinstance(item, dict) or not isinstance(item.get("type"), str):
                raise ValueError(f"usage snapshot turn {index} contains an unrecognized item")
            item_type = item["type"]
            if item_type in NON_TOOL_ITEM_TYPES:
                continue
            if item_type not in TOOL_ITEM_TYPES:
                raise ValueError(f"usage snapshot contains unrecognized item type: {item_type}")
            # Thread creation is coordinator bookkeeping, not work performed by the session.
            if item.get("name") == "create_thread":
                continue
            item_id = item.get("id")
            if not isinstance(item_id, str) or not item_id or item_id in tool_ids:
                raise ValueError("usage snapshot tool items must have unique stable IDs")
            tool_ids.add(item_id)

    return {
        "active_milliseconds": total_ms,
        "active_seconds": (total_ms + 999) // 1000,
        "tool_calls": len(tool_ids),
    }


def read_thread_usage_snapshot(path: Path, expected_session_id: str) -> dict[str, int]:
    try:
        snapshot = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"usage snapshot is not readable JSON: {exc}") from exc
    return summarize_thread_snapshot(snapshot, expected_session_id)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def delivery_tree_sha256(root: Path) -> str:
    """Match the staged artifact digest used by case_runner._prepare_delivery."""
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        if path.is_symlink():
            payload = b"symlink:" + str(path.readlink()).encode("utf-8")
        elif path.is_file():
            payload = path.read_bytes()
        else:
            continue
        digest.update(relative + b"\0" + hashlib.sha256(payload).digest())
    return digest.hexdigest()
