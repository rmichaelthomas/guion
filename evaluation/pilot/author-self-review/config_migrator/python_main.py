#!/usr/bin/env python3
"""Python-side candidate using the same locked TOML library and helper."""
import argparse
from pathlib import Path
import sys

from config_support import inspect_and_migrate


def main() -> int:
    parser = argparse.ArgumentParser(prog="config-migrator")
    sub = parser.add_subparsers(dest="operation", required=True)
    for operation in ("check", "migrate"):
        command = sub.add_parser(operation)
        command.add_argument("--file", required=True)
    args = parser.parse_args()
    path = Path(args.file)
    try:
        source = path.read_text(encoding="utf-8")
    except OSError:
        print("cannot read configuration", file=sys.stderr)
        return 1
    result = inspect_and_migrate(source, args.operation)
    if not result["ok"]:
        print(result["error"], file=sys.stderr)
        return 1
    if args.operation == "migrate":
        temporary = path.with_name(f".{path.name}.veritas-tmp")
        try:
            temporary.write_text(result["updated"], encoding="utf-8")
            temporary.replace(path)
        except OSError:
            temporary.unlink(missing_ok=True)
            print("cannot atomically update configuration", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
