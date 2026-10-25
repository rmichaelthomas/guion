#!/usr/bin/env python3
"""Generate the fixed Task 10 JSONL input and service allow-list."""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path


SEED = 20261025
SERVICES = ("api", "billing", "search", "worker", "ignored")


def generate(rows: int, output: Path, services_output: Path) -> None:
    randomizer = random.Random(SEED)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8", newline="\n") as destination:
        for index in range(rows):
            if index and index % 211 == 0:
                destination.write("  \n")
            if index and index % 127 == 0:
                destination.write("{broken-json\n")
                continue
            service = randomizer.choice(SERVICES)
            day = randomizer.randrange(1, 29)
            hour = randomizer.randrange(24)
            minute = randomizer.randrange(60)
            second = randomizer.randrange(60)
            record = {
                "service": service,
                "at": f"2026-07-{day:02d}T{hour:02d}:{minute:02d}:{second:02d}Z",
                "event_id": f"evt-{index:07d}",
            }
            if index and index % 97 == 0:
                record["service"] = 17
            destination.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
    services_output.parent.mkdir(parents=True, exist_ok=True)
    services_output.write_text("".join(service + "\n" for service in SERVICES), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rows", type=int, default=20_000)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--services", type=Path, required=True)
    args = parser.parse_args()
    if args.rows < 1:
        parser.error("--rows must be positive")
    generate(args.rows, args.output, args.services)
    print(json.dumps({"seed": SEED, "rows": args.rows, "jsonl": str(args.output), "services": str(args.services)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
