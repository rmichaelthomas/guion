#!/usr/bin/env python3
"""Compare uncached AllHereHTML source runs in randomized, output-checked pairs."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import random
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "evaluation/completion/applications/allherehtml"
SCRATCH = ROOT / "target/perf-session"
SEED = 20261027
SCALES = (1, 2, 4)


def bootstrap_ratio(ratios: list[float], rng: random.Random) -> dict[str, object]:
    medians = sorted(
        statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(2_000)
    )
    return {
        "median_ratio": statistics.median(ratios),
        "ci95_percentile": [medians[49], medians[1_949]],
        "win": medians[1_949] < 1.0,
        "ties_or_regressions": sum(ratio >= 1.0 for ratio in ratios),
    }


def prepare_project(root: Path, scale: int) -> tuple[Path, str, int]:
    project = root / str(scale)
    shutil.copytree(FIXTURE, project)
    template = project / "guion-site.template.html"
    expected = project / "expected-guion-showcase.html"
    source_text = template.read_text(encoding="utf-8")
    expected_text = expected.read_text(encoding="utf-8")
    added = "x" * (len(source_text) * (scale - 1))
    extra = "" if scale == 1 else "<!--\n" + "\n".join(
        added[index:index + 80] for index in range(0, len(added), 80)
    ) + "\n-->"
    template.write_text(source_text.replace("</body>", extra + "</body>", 1), encoding="utf-8")
    expected.write_text(expected_text.replace("</body>", extra + "</body>", 1), encoding="utf-8")
    manifest = project / "guion.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            "instruction_limit = 20000000", f"instruction_limit = {20000000 * scale}"
        ),
        encoding="utf-8",
    )
    return project, hashlib.sha256(expected.read_bytes()).hexdigest(), template.stat().st_size


def run_once(binary: Path, project: Path, expected_sha256: str) -> float:
    started = time.perf_counter()
    result = subprocess.run(
        [str(binary), "run", "--no-cache", "AllHereHTML.gu"],
        cwd=project,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    elapsed = time.perf_counter() - started
    output = project / "guion-showcase.html"
    if result.returncode or not output.is_file():
        raise RuntimeError(f"{binary} failed ({result.returncode}): {result.stdout}\n{result.stderr}")
    observed_sha256 = hashlib.sha256(output.read_bytes()).hexdigest()
    if observed_sha256 != expected_sha256:
        raise RuntimeError(f"{binary} produced an incorrect AllHereHTML output")
    return elapsed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--warmups", type=int, default=2)
    args = parser.parse_args()
    if args.trials < 3 or args.warmups < 1:
        parser.error("at least three paired trials and one warmup pair are required")
    baseline = args.baseline.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    rng = random.Random(SEED)
    report: dict[str, object] = {
        "schema_version": 1,
        "kind": "allherehtml-randomized-build-comparison",
        "host": {"platform": platform.platform(), "architecture": platform.machine()},
        "baseline_sha256": hashlib.sha256(baseline.read_bytes()).hexdigest(),
        "candidate_sha256": hashlib.sha256(candidate.read_bytes()).hexdigest(),
        "source_sha256": hashlib.sha256((FIXTURE / "AllHereHTML.gu").read_bytes()).hexdigest(),
        "trials_per_arm_per_scale": args.trials,
        "warmup_pairs_per_scale": args.warmups,
        "measurement": "Fresh process, source cache bypassed, output SHA-256 checked after every run; OS file cache uncontrolled.",
        "scales": {},
    }
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="allhere-paired-", dir=SCRATCH) as tmp:
        for scale in SCALES:
            project, expected_sha256, template_bytes = prepare_project(Path(tmp), scale)
            arms = {"baseline": baseline, "candidate": candidate}
            samples = {name: [] for name in arms}
            orders: list[list[str]] = []
            schedule = ["warmup"] * args.warmups + ["trial"] * args.trials
            for phase in schedule:
                order = list(arms)
                rng.shuffle(order)
                observations = {name: run_once(arms[name], project, expected_sha256) for name in order}
                if phase == "trial":
                    orders.append(order)
                    for name in arms:
                        samples[name].append(observations[name])
            ratios = [candidate_time / baseline_time for candidate_time, baseline_time in zip(
                samples["candidate"], samples["baseline"], strict=True
            )]
            scale_report = {
                "template_bytes": template_bytes,
                "expected_output_sha256": expected_sha256,
                "output_checked_each_warmup_and_trial": True,
                "arm_medians_seconds": {name: statistics.median(values) for name, values in samples.items()},
                "candidate_over_baseline": bootstrap_ratio(ratios, rng),
                "samples_seconds": samples,
                "paired_trial_order": orders,
            }
            report["scales"][str(scale)] = scale_report
            print(json.dumps({"scale": scale, "candidate_over_baseline": scale_report["candidate_over_baseline"]}), flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
