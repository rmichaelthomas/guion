# Aggregate update forms

These three Guion programs and the Python reference process the same 120,000 deterministic `(service, date, timestamp)` events over 20 keys and emit an identical sorted count/min/max report.

- `json-state.gu` keeps each aggregate in a JSON object and updates its fields with `json_set`.
- `compact-record.gu` uses a typed `EventSummary` value and replaces the map entry for each event.
- `mutable-record.gu` uses typed map-record field access/update operations. Runtime profile counters report reads, updates, in-place writes, copy-on-write copies, and estimated copied bytes.
- `reference.py` is the output oracle and timing comparison.

The source ablation from `text_chars` to `text_slice` is already measured separately in the historical application fixture and is not repeated here. The core cost-closure harness records each source and oracle hash before timing.
