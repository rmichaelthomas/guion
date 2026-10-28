"""Direct Python baseline for the repeated-call bridge probe."""

from pathlib import Path
import sys

from bridge_probe import payload_length


def main(args: list[str]) -> int:
    if len(args) != 2:
        print("Usage: python3 reference.py INPUT.bin CALL_COUNT")
        return 2
    source = Path(args[0]).read_bytes()
    count = int(args[1])
    total = 0
    for _ in range(count):
        total += payload_length(source)
    print(total)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
