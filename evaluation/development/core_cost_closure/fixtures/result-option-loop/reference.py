#!/usr/bin/env python3
"""Python oracle for the deterministic Result/Option loop."""

ITERATIONS = 200_000


def main() -> None:
    checksum = rejected = absent = 0
    for value in range(1, ITERATIONS + 1):
        result = (False, "rejected") if value % 7 == 0 else (True, value)
        option = None if value % 11 == 0 else value
        if result[0]:
            checksum += result[1]
        else:
            rejected += 1
        if option is not None:
            checksum += option
        else:
            absent += 1
    print(checksum)
    print(rejected)
    print(absent)


if __name__ == "__main__":
    main()
