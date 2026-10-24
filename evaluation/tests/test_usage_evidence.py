"""Checks for measured Codex session usage evidence."""

from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest

from evaluation.protocol.usage_evidence import read_thread_usage_snapshot, summarize_thread_snapshot


class UsageEvidenceTests(unittest.TestCase):
    def test_sums_completed_turn_durations_and_tool_invocations(self):
        snapshot = {
            "schemaVersion": 1,
            "thread": {"id": "session-001"},
            "page": {"limit": 10, "nextCursor": None, "hasMore": False},
            "turns": [
                {
                    "id": "turn-1",
                    "status": {"type": "completed"},
                    "durationMs": 1250,
                    "items": [
                        {"type": "commandExecution", "id": "exec-1"},
                        {"type": "functionCallOutput", "id": "fco-1", "name": "read_file"},
                        {"type": "agentMessage", "id": "msg-1"},
                    ],
                },
                {
                    "id": "turn-2",
                    "status": "completed",
                    "durationMs": 500,
                    "items": [{"type": "functionCallOutput", "id": "fco-2", "name": "create_thread"}],
                },
            ],
        }
        self.assertEqual(
            summarize_thread_snapshot(snapshot, "session-001"),
            {"active_milliseconds": 1750, "active_seconds": 2, "tool_calls": 2},
        )

    def test_rejects_wrong_session_and_nonterminal_turns(self):
        snapshot = {
            "schemaVersion": 1,
            "thread": {"id": "session-001"},
            "page": {"limit": 10, "nextCursor": None, "hasMore": False},
            "turns": [{"id": "turn-1", "status": "in_progress", "durationMs": 12, "items": []}],
        }
        with self.assertRaisesRegex(ValueError, "session ID"):
            summarize_thread_snapshot(snapshot, "other-session")
        with self.assertRaisesRegex(ValueError, "terminal"):
            summarize_thread_snapshot(snapshot, "session-001")

    def test_rejects_pagination_gaps_missing_items_and_unknown_item_kinds(self):
        snapshot = {
            "schemaVersion": 1,
            "thread": {"id": "session-001"},
            "page": {"limit": 1, "nextCursor": "older-page", "hasMore": True},
            "turns": [{"id": "turn-1", "status": "completed", "durationMs": 1, "items": []}],
        }
        with self.assertRaisesRegex(ValueError, "incomplete"):
            summarize_thread_snapshot(snapshot, "session-001")

        snapshot["page"] = {"limit": 10, "nextCursor": None, "hasMore": False}
        snapshot["turns"][0].pop("items")
        with self.assertRaisesRegex(ValueError, "items list"):
            summarize_thread_snapshot(snapshot, "session-001")

        snapshot["turns"][0]["items"] = [{"type": "newToolType", "id": "tool-1"}]
        with self.assertRaisesRegex(ValueError, "unrecognized item type"):
            summarize_thread_snapshot(snapshot, "session-001")

    def test_usage_evidence_file_must_be_valid_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "snapshot.json"
            path.write_text("not json", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "readable JSON"):
                read_thread_usage_snapshot(path, "session-001")


if __name__ == "__main__":
    unittest.main()
