"""Direct Python baseline for the one-call bridge startup probe."""

from pathlib import Path
import sys

from bridge_probe import payload_length


def main(args: list[str]) -> int:
    if len(args) != 1:
        print("Usage: python3 reference.py INPUT.json")
        return 2
    print(payload_length(Path(args[0]).read_bytes()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
