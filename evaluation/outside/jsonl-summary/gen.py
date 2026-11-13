#!/usr/bin/env python3
"""Deterministic access-log generator: gen.py ROWS OUTPUT_PATH.

Writes ROWS valid JSON lines followed by one malformed line, using a fixed
seed, so the same row count always gives byte-identical output.
"""
import json
import random
import sys


def generate(size, out):
    rng = random.Random(7)
    paths = ["/login", "/search", "/cart", "/checkout", "/health"]
    rows = []
    for i in range(size):
        status = rng.choice([200, 200, 200, 201, 304, 404, 500] if i % 97 else [500])
        rows.append(json.dumps({
            "ts": f"2026-11-03T{i % 24:02d}:{i % 60:02d}:00Z",
            "status": status,
            "path": rng.choice(paths),
            "ms": rng.randint(1, 900),
        }))
    rows.append("{not json at all")
    with open(out, "w") as f:
        f.write("\n".join(rows) + "\n")
    return len(rows)


if __name__ == "__main__":
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 5000
    path = sys.argv[2] if len(sys.argv) > 2 else "access.jsonl"
    print("wrote", generate(n, path), "lines to", path)
