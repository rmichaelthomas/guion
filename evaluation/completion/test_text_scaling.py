#!/usr/bin/env python3
"""Exercise text splitting and joining on long Unicode and many-field inputs."""

from __future__ import annotations

import argparse
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="text-scaling-", dir=scratch) as directory:
        project = Path(directory)
        (project / "guion.toml").write_text(
            '[project]\nname = "text-scaling"\nversion = "1.0.0"\nentry = "main.gu"\n'
            '[runtime]\ninstruction_limit = 50000000\n', encoding="utf-8")
        long_line = "x" * 28000 + "🙂"
        source = f'''import "std:text"
fn main() -> Int:
    let fields = text_split("|{long_line}||é|", "|")
    assert(len(fields) == 5)
    assert(fields[0] == "")
    assert(len(fields[1]) == 28001)
    assert(fields[2] == "")
    assert(fields[3] == "é")
    assert(fields[4] == "")
    assert(text_split("abc", "")[0] == "abc")
    let overlapping = text_split("aaaa", "aa")
    assert(len(overlapping) == 3)
    assert(overlapping[0] == "" and overlapping[1] == "" and overlapping[2] == "")
    assert(text_join(text_split("α::🙂::", "::"), "|") == "α|🙂|")
    var pieces: List[Text] = []
    var i = 0
    while i < 3000:
        set pieces = list_push(pieces, "{'z' * 80}")
        set i = i + 1
    let joined = text_join(pieces, "|")
    assert(len(joined) == 242999)
    assert(text_join([], "|") == "")
    assert(text_join(["a", "", "é"], "|") == "a||é")
    var small: List[Text] = []
    var expected = ""
    var j = 0
    while j < 33:
        set small = list_push(small, "é")
        if j > 0:
            set expected = expected + "|"
        set expected = expected + "é"
        set j = j + 1
    assert(text_join(small, "|") == expected)
    print("text scaling passed")
    return 0
'''
        (project / "main.gu").write_text(source, encoding="utf-8")
        result = subprocess.run([str(binary), "run", "--no-cache", str(project)],
                                capture_output=True, text=True, timeout=180)
        if result.returncode or result.stdout != "text scaling passed\n":
            raise SystemExit(f"text scaling failed ({result.returncode}):\n{result.stdout}{result.stderr}")
        print(result.stdout, end="")


if __name__ == "__main__":
    main()
