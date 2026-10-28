"""Tiny Python target that keeps the bridge's package-import cost in scope."""

import numpy
import orjson

_PACKAGE_IMPORTS = (numpy, orjson)


def payload_length(source: bytes) -> int:
    """Return the input size without parsing or transforming the payload."""
    return len(source)
