# JSON field extraction

This workload parses three fixed JSON values once, then repeatedly extracts `count` from an already parsed value. It covers an integer field, a field with the wrong type, and a missing field. File I/O and aggregation are excluded. Guion and Python emit the same checksum and invalid-field count.

The Guion helper path uses `json_get_safe` and `json_int_safe`, so telemetry can distinguish host JSON operations from native Result/Option checks. `main.gu` and `reference.py` are hashed by the core cost-closure benchmark harness.
