#!/usr/bin/env python3
"""Text processing kernel for the Guion/Python comparison: split, trim, replace and find."""

WORDS = ["alpha", "beta", "café", "delta", "epsilon", "zeta", "eta", "theta"]
LINES = 40000


def main() -> None:
    lines = []
    for index in range(LINES):
        lines.append("  " + WORDS[index % 8] + ",{{name}}," + WORDS[(index * 3) % 8] + " likes {{thing}}  ")
    document = "\n".join(lines)
    rows = document.split("\n")
    total = 0
    for row in rows:
        fields = row.split(",")
        total += len(fields[0].strip()) + len(fields[2].strip())
    filled = document.replace("{{name}}", "Ada").replace("{{thing}}", "tea")
    found = 0
    position = filled.find("tea", 0)
    while position >= 0:
        found += 1
        position = filled.find("tea", position + 3)
    print(len(rows))
    print(total)
    print(len(filled))
    print(found)


if __name__ == "__main__":
    main()
