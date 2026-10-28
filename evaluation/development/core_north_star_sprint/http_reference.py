#!/usr/bin/env python3
"""Independent loopback HTTP and JSON oracle for the Guion core sprint."""

from __future__ import annotations

import json
import sys
from urllib.request import urlopen


def main() -> None:
    with urlopen(sys.argv[1], timeout=10) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        value = json.loads(response.read().decode("utf-8"))
    if value["count"] != 42 or len(value["payload"]) != 65536:
        raise RuntimeError("HTTP fixture body did not match the frozen oracle")
    print(value["message"])
    print(value["count"])
    print(len(value["payload"]))


if __name__ == "__main__":
    main()
