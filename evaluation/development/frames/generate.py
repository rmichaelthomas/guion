"""Writes the seeded benchmark dataset: programs/data/rows.csv (1,000,000 rows) and programs/data/lookup.csv.

Usage: python3 generate.py [rows]

Columns of rows.csv: id (i64), group (text, 1,000 values), x and y (f64, about 1% missing),
flag (bool). lookup.csv has one row per group: group, weight (f64), label (text).
"""

import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "programs", "data")


def main():
    rows = int(sys.argv[1]) if len(sys.argv) > 1 else 1_000_000
    rng = random.Random(2026)
    os.makedirs(DATA, exist_ok=True)
    with open(os.path.join(DATA, "rows.csv"), "w", newline="") as out:
        out.write("id,group,x,y,flag\n")
        for row in range(rows):
            group = f"g{rng.randrange(1000):03d}"
            x = "" if rng.random() < 0.01 else repr(round(rng.random(), 6))
            y = "" if rng.random() < 0.01 else repr(round(rng.gauss(0.0, 10.0), 6))
            flag = "true" if rng.random() < 0.5 else "false"
            out.write(f"{row},{group},{x},{y},{flag}\n")
    with open(os.path.join(DATA, "lookup.csv"), "w", newline="") as out:
        out.write("group,weight,label\n")
        for group in range(1000):
            out.write(f"g{group:03d},{repr(round(rng.random() * 2.0, 6))},L{group % 17}\n")


if __name__ == "__main__":
    main()
