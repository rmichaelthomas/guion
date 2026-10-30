#!/usr/bin/env python3
"""Python oracle for deterministic count/min/max aggregate updates."""

ITERATIONS = 120_000
KEYS = tuple(
    f"service-{service}/date-{date}"
    for service in range(4)
    for date in range(5)
)


def main() -> None:
    groups = {key: [0, -1, 0] for key in KEYS}
    for index in range(ITERATIONS):
        key = KEYS[index % len(KEYS)]
        timestamp = index + 1
        summary = groups[key]
        summary[0] += 1
        if summary[1] < 0 or timestamp < summary[1]:
            summary[1] = timestamp
        if timestamp > summary[2]:
            summary[2] = timestamp
    for key in sorted(groups):
        print(key)
        for value in groups[key]:
            print(value)


if __name__ == "__main__":
    main()
