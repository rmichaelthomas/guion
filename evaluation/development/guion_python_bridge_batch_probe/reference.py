"""Direct-Python reference for the one-call batched bridge workload."""

import sys

import numpy
import orjson

_PACKAGE_IMPORTS = (numpy, orjson)


def main() -> int:
    if len(sys.argv) != 3:
        print("Usage: reference.py INPUT.bin COUNT")
        return 2
    with open(sys.argv[1], "rb") as source_file:
        source = source_file.read()
    count = int(sys.argv[2])
    total = 0
    for _ in range(count):
        total += len(source)
    print(total)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
