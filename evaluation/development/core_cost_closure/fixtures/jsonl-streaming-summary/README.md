# JSONL streaming summary with nested typed updates

This is a separately labeled full-application lane based on the measured compact-record/text-slice program. It keeps the same JSONL parsing, validation, allow-list behavior, report schema, and output construction. Existing (service, date) entries use nested-key map_record_get and map_record_set forms; new entries still use map_set. Runtime counters show whether updates stayed in place or copied shared containers.

The source is derived mechanically from jsonl-summary-text-slice-compact-record/main.gu; its only behavioral change is the aggregate update block. The core cost-closure harness checks output against the existing Python and SQLite references on the same generated normal, invalid, strict, and allow-list inputs, then measures source and artifact lanes. The historical original application and compact-record control remain unchanged.
