import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: reference.py <sales.json>", file=sys.stderr)
        return 2
    document = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    sales = document["sales"]
    print(len(sales))
    print(sum(int(sale["amount"]) for sale in sales))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
