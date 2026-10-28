"""Batch-call target for measuring the Python bridge's amortized cost."""

import numpy
import orjson

_PACKAGE_IMPORTS = (numpy, orjson)


def sum_repeated_lengths(source: bytes, count: int) -> int:
    """Repeat the same small operation inside Python after one bridge call."""
    total = 0
    for _ in range(count):
        total += len(source)
    return total
