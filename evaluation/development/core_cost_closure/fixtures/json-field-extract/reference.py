#!/usr/bin/env python3
"""Python oracle for typed extraction from already parsed JSON values."""

ITERATIONS = 120_000
ROWS = ({"count": 7}, {"count": "7"}, {"other": 7})


def main() -> None:
    checksum = invalid = 0
    for index in range(ITERATIONS):
        value = ROWS[index % len(ROWS)].get("count")
        if type(value) is int:
            checksum += value
        else:
            invalid += 1
    print(checksum)
    print(invalid)


if __name__ == "__main__":
    main()
