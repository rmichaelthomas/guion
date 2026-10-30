# Result and Option loop

This workload isolates repeated `Result` and `Option` construction, tag checks, payload extraction, branching, and function return. It does no file, JSON, or collection work. Both programs run 200,000 deterministic values and emit the same checksum and failure counts. The Python implementation is an oracle and timing reference, not a claim that tuple-based Result emulation is Python's ideal representation.

`main.gu` and `reference.py` are hashed by the core cost-closure benchmark harness. Keep their iteration count and arithmetic identical when revising the workload.
