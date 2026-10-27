#!/usr/bin/env python3
"""Check a pinned real AllHereHTML build without modifying its fixture."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "evaluation/completion/applications/allherehtml"
SCRATCH = ROOT / "target/perf-session"


def invoke(binary: Path, project: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), *args], cwd=project, text=True, capture_output=True, timeout=180
    )


def check(binary: Path) -> dict:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="allherehtml-", dir=SCRATCH) as temp:
        project = Path(temp) / "project"
        shutil.copytree(FIXTURE, project)
        expected = (project / "expected-guion-showcase.html").read_bytes()
        results = {}
        for name, args in (
            ("check", ("check", "AllHereHTML.gu")),
            ("tests", ("test", "AllHereHTML.gu")),
            ("build", ("run", "--no-cache", "AllHereHTML.gu")),
        ):
            completed = invoke(binary, project, *args)
            results[name] = {"exit_code": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr}
            if completed.returncode != 0:
                raise RuntimeError(f"{name} failed: {completed.stdout}\n{completed.stderr}")
        output = project / "guion-showcase.html"
        actual = output.read_bytes()
        if actual != expected:
            raise RuntimeError("generated page differs from the pinned expected page")
        results["output_sha256"] = hashlib.sha256(actual).hexdigest()

        template = project / "guion-site.template.html"
        original = template.read_text(encoding="utf-8")
        if "</body>" not in original:
            raise RuntimeError("fixture is missing the expected closing body tag")
        template.write_text(original.replace("</body>", "<script>bad()</script></body>", 1), encoding="utf-8")
        denied = invoke(binary, project, "run", "--no-cache", "AllHereHTML.gu")
        if denied.returncode == 0 or output.read_bytes() != expected:
            raise RuntimeError("unsafe HTML was accepted or the existing output was modified")
        results["unsafe_template"] = {"exit_code": denied.returncode, "output_unchanged": True}

        template.unlink()
        missing = invoke(binary, project, "run", "--no-cache", "AllHereHTML.gu")
        if missing.returncode == 0 or output.read_bytes() != expected:
            raise RuntimeError("missing template was accepted or the existing output was modified")
        results["missing_template"] = {"exit_code": missing.returncode, "output_unchanged": True}
        return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.binary.resolve()), indent=2))


if __name__ == "__main__":
    main()
