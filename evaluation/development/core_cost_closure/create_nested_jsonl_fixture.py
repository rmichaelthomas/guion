#!/usr/bin/env python3
"""Rebuild the nested-map JSONL lane from the frozen compact-record fixture."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / "evaluation/development/core_cost_closure/fixtures/jsonl-summary-text-slice-compact-record/main.gu"
OUTPUT = ROOT / "evaluation/development/core_cost_closure/fixtures/jsonl-streaming-summary"
TEXT_CHARS_OUTPUT = ROOT / "evaluation/development/core_cost_closure/fixtures/jsonl-streaming-summary-text-chars"

OLD = (
    "                        var by_date: Map[Text, EventSummary] = {}\n"
    "                        if map_has(groups, service):\n"
    "                            set by_date = groups[service]\n"
    "                        if map_has(by_date, date):\n"
    "                            let previous = by_date[date]\n"
    "                            var oldest_us = previous.oldest_us\n"
    "                            if instant.unix_microseconds < oldest_us:\n"
    "                                set oldest_us = instant.unix_microseconds\n"
    "                            var newest_us = previous.newest_us\n"
    "                            if instant.unix_microseconds > newest_us:\n"
    "                                set newest_us = instant.unix_microseconds\n"
    "                            let updated = EventSummary(previous.count + 1, oldest_us, newest_us)\n"
    "                            set by_date = map_set(by_date, date, updated)\n"
    "                        else:\n"
    "                            let initial = EventSummary(1, instant.unix_microseconds, instant.unix_microseconds)\n"
    "                            set by_date = map_set(by_date, date, initial)\n"
    "                        set groups = map_set(groups, service, by_date)"
)
NEW = (
    "                        if map_has(groups, service) and map_has(groups[service], date):\n"
    "                            let count = map_record_get(groups, service, date, \"count\")\n"
    "                            var oldest_us = map_record_get(groups, service, date, \"oldest_us\")\n"
    "                            if instant.unix_microseconds < oldest_us:\n"
    "                                set oldest_us = instant.unix_microseconds\n"
    "                            var newest_us = map_record_get(groups, service, date, \"newest_us\")\n"
    "                            if instant.unix_microseconds > newest_us:\n"
    "                                set newest_us = instant.unix_microseconds\n"
    "                            set groups = map_record_set(groups, service, date, \"count\", count + 1)\n"
    "                            set groups = map_record_set(groups, service, date, \"oldest_us\", oldest_us)\n"
    "                            set groups = map_record_set(groups, service, date, \"newest_us\", newest_us)\n"
    "                        else:\n"
    "                            var by_date: Map[Text, EventSummary] = {}\n"
    "                            if map_has(groups, service):\n"
    "                                set by_date = groups[service]\n"
    "                            let initial = EventSummary(1, instant.unix_microseconds, instant.unix_microseconds)\n"
    "                            set by_date = map_set(by_date, date, initial)\n"
    "                            set groups = map_set(groups, service, by_date)"
)
DATE_SLICE = "                        let date = text_slice(at, 0, 10)"
DATE_CHARS = (
    "                        let timestamp_characters = text_chars(at)\n"
    "                        let date = timestamp_characters[0] + timestamp_characters[1] + timestamp_characters[2] + timestamp_characters[3] + timestamp_characters[4] + timestamp_characters[5] + timestamp_characters[6] + timestamp_characters[7] + timestamp_characters[8] + timestamp_characters[9]"
)


def main() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    if source.count(OLD) != 1:
        raise SystemExit("compact-record fixture no longer has the expected update block")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    nested_source = source.replace(OLD, NEW, 1)
    if nested_source.count(DATE_SLICE) != 1:
        raise SystemExit("nested-map fixture no longer has the expected text_slice date expression")
    (OUTPUT / "main.gu").write_text(nested_source, encoding="utf-8")
    (OUTPUT / "README.md").write_text(
        "# JSONL streaming summary with nested typed updates\n\n"
        "This is a separately labeled full-application lane based on the measured compact-record/text-slice program. It keeps the same JSONL parsing, validation, allow-list behavior, report schema, and output construction. Existing (service, date) entries use nested-key map_record_get and map_record_set forms; new entries still use map_set. Runtime counters show whether updates stayed in place or copied shared containers.\n\n"
        "The source is derived mechanically from jsonl-summary-text-slice-compact-record/main.gu; its only behavioral change is the aggregate update block. The core cost-closure harness checks output against the existing Python and SQLite references on the same generated normal, invalid, strict, and allow-list inputs, then measures source and artifact lanes. The historical original application and compact-record control remain unchanged.\n",
        encoding="utf-8",
    )
    TEXT_CHARS_OUTPUT.mkdir(parents=True, exist_ok=True)
    (TEXT_CHARS_OUTPUT / "main.gu").write_text(
        nested_source.replace(DATE_SLICE, DATE_CHARS, 1), encoding="utf-8"
    )
    (TEXT_CHARS_OUTPUT / "README.md").write_text(
        "# JSONL streaming summary with text_chars date extraction\n\n"
        "This is a controlled ablation of jsonl-streaming-summary/main.gu. It changes only the UTC-date extraction expression from text_slice to text_chars plus indexed character concatenation. Parsing, validation, aggregation representation, MapRecord updates, output construction, and input are identical. The paired benchmark compares this lane with the text_slice source to isolate the date-extraction contribution.\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
