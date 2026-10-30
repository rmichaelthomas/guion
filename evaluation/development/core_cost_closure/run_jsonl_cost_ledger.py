#!/usr/bin/env python3
"""Run the frozen JSONL artifact with and without temporary VM cost timers."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--trials", type=int, default=5)
    args = parser.parse_args()
    summary = args.data_dir / "performance-inputs/summary.json"
    expected_hash = hashlib.sha256(summary.read_bytes()).hexdigest()
    rng = random.Random(20261029)
    order = []
    for pair_index in range(args.trials):
        pair = ["off", "on"]
        rng.shuffle(pair)
        order.extend((pair_index, mode) for mode in pair)
    records = []
    for pair_index, mode in order:
        env = os.environ.copy()
        env["GUION_PHASE_TIMINGS"] = "1"
        env.pop("GUION_RUNTIME_PROFILE", None)
        if mode == "on":
            env["GUION_COST_LEDGER"] = "1"
        else:
            env.pop("GUION_COST_LEDGER", None)
        process = subprocess.run(
            [str(args.binary), "run", "--json", str(args.artifact),
             "--events", "performance-inputs/events.jsonl",
             "--output", "performance-inputs/summary.json"],
            cwd=args.data_dir, env=env, capture_output=True, text=True, check=True,
        )
        report = json.loads(process.stdout)
        assert report["status"] == "passed", report
        output_hash = hashlib.sha256(summary.read_bytes()).hexdigest()
        assert output_hash == expected_hash, (output_hash, expected_hash)
        matches = re.findall(r"GUION_COST_LEDGER (\{[^\n]+\})", process.stderr)
        assert len(matches) == (1 if mode == "on" else 0), process.stderr
        cost = json.loads(matches[0]) if matches else None
        phase = report["phase_timings_ns"]
        execute_ns = phase["program_execute_ns"]
        if cost is not None:
            assert cost["run_ns"] <= execute_ns, (cost, execute_ns)
            assert cost["vm_segment_ns"] + cost["frame_start_ns"] <= cost["run_ns"], cost
            assert cost["host_ns"] <= cost["vm_segment_ns"], cost
            cost["outside_vm_ns"] = execute_ns - cost["run_ns"]
            assert sum(cost[k] for k in ("outside_vm_ns", "driver_ns", "frame_start_ns", "vm_guest_ns", "host_ns")) == execute_ns
        records.append({"pair": pair_index, "mode": mode, "program_execute_ns": execute_ns, "cost": cost})
        print(f"pair {pair_index + 1} {mode} {execute_ns / 1e9:.6f}s", flush=True)
    result = {
        "kind": "guion-jsonl-cost-ledger-trials",
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "artifact_sha256": hashlib.sha256(args.artifact.read_bytes()).hexdigest(),
        "input_sha256": hashlib.sha256((args.data_dir / "performance-inputs/events.jsonl").read_bytes()).hexdigest(),
        "output_sha256": expected_hash,
        "trials_per_mode": args.trials,
        "records": records,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
