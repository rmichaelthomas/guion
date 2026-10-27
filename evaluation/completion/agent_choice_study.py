#!/usr/bin/env python3
"""Run ephemeral, sequential post-trial agent choices without app chats."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BRIEFS = {
    "health-digest": "Build an internal inventory of device endpoints, querying each concurrently under a hard deadline and writing a human-readable health digest. It will run repeatedly on one controlled host.",
    "lockfile-auditor": "Build a command-line audit of a software bill of materials against a team policy, reporting disallowed packages, version limits, and exceptions in deterministic JSON.",
    "webhook-replay": "Build a small durable notification relay that validates incoming payloads, records their state, retries delivery, and reports failures to an operator.",
}
SOURCE = {
    "health-digest": "evaluation/completion/applications/health-digest/main.vr",
    "lockfile-auditor": "evaluation/completion/applications/lockfile-auditor/main.vr",
    "webhook-replay": "evaluation/completion/applications/webhook-replay/main.vr",
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", choices=sorted(BRIEFS), action="append")
    args = parser.parse_args()
    evidence = ROOT / "evaluation/completion/results/real-task-sprint/agent-choice"
    scratch = ROOT / "target/perf-session/agent-choice"
    evidence.mkdir(parents=True, exist_ok=True)
    scratch.mkdir(parents=True, exist_ok=True)
    schema = scratch / "choice-schema.json"
    schema.write_text(json.dumps({"type": "object", "additionalProperties": False,
        "properties": {"choice": {"type": "string", "enum": ["Guion", "Python", "Either"]},
                       "rationale": {"type": "string"}, "trial_observation": {"type": "string"}},
        "required": ["choice", "rationale", "trial_observation"]}), encoding="utf-8")
    for name in args.application or list(BRIEFS):
        acceptance = scratch / f"{name}-acceptance.json"
        final = scratch / f"{name}-choice.json"
        acceptance.unlink(missing_ok=True)
        final.unlink(missing_ok=True)
        prompt = f'''You are evaluating two programming languages after trying working examples. Work only in this repository. Keep any session contract and all temporary files inside {scratch}; do not inspect unrelated historical session contracts. Do not change project files or create chats.

First run this exact acceptance command to try both implementations:
PYTHONPATH="{ROOT}/target/perf-session/python-deps" python3 evaluation/completion/compare_real_tasks.py --binary target/release/guion --application {name} --output {acceptance}

Read the acceptance result, inspect the Guion source at {SOURCE[name]} and Python reference at evaluation/calibration/python-reference/main.py, and inspect this application's measurements in evaluation/completion/results/real-task-sprint/nine-task-timings-final.json. The timings measure the named frozen case and include oracle-check time. Decide which language you would use for the following related, new task and explain the reasons based on what you observed. You may choose either language; do not optimize for a particular result.

New task: {BRIEFS[name]}

Return the required JSON including your choice, concise rationale, and what happened when you ran the two implementations.'''
        (evidence / f"{name}-prompt.txt").write_text(prompt, encoding="utf-8")
        command = ["codex", "exec", "--ephemeral", "--json", "--sandbox", "workspace-write",
                   "--config", "sandbox_workspace_write.network_access=true",
                   "--skip-git-repo-check", "-C", str(ROOT), "--output-schema", str(schema),
                   "--output-last-message", str(final), "-"]
        print(f"Running ephemeral post-trial evaluation: {name}", flush=True)
        environment = os.environ.copy()
        environment["TMPDIR"] = str(scratch)
        environment["LIMINATE_CONTRACTS_DIR"] = str(scratch / "internal-records")
        result = subprocess.run(command, input=prompt, text=True, capture_output=True, cwd=ROOT,
                                env=environment, timeout=300)
        raw = scratch / "raw-events"
        raw.mkdir(exist_ok=True)
        (raw / f"{name}-events.jsonl").write_text(result.stdout, encoding="utf-8")
        (raw / f"{name}-stderr.txt").write_text(result.stderr, encoding="utf-8")
        if result.returncode or not final.is_file() or not acceptance.is_file():
            raise RuntimeError(f"{name} evaluator did not finish or did not run acceptance: {result.stderr}")
        choice = json.loads(final.read_text(encoding="utf-8"))
        verified = json.loads(acceptance.read_text(encoding="utf-8"))
        if not verified["applications"][name]["passed"]:
            raise RuntimeError(f"{name} evaluator's acceptance run failed")
        events = [json.loads(line) for line in result.stdout.splitlines() if line]
        commands = [event["item"] for event in events if event.get("type") == "item.completed"
                    and event.get("item", {}).get("type") == "command_execution"]
        acceptance_runs = [item for item in commands if "compare_real_tasks.py" in item.get("command", "")]
        if not any(item.get("exit_code") == 0 for item in acceptance_runs):
            raise RuntimeError(f"{name} trace does not show a successful acceptance command")
        trace = {"command_count": len(commands), "nonzero_commands": sum(
            item.get("exit_code") not in (0, None) for item in commands),
            "acceptance_exit_codes": [item.get("exit_code") for item in acceptance_runs]}
        (evidence / f"{name}-trace.json").write_text(json.dumps(trace, indent=2) + "\n", encoding="utf-8")
        (evidence / f"{name}-choice.json").write_text(json.dumps(choice, indent=2) + "\n", encoding="utf-8")
        (evidence / f"{name}-acceptance.json").write_text(json.dumps(verified, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"application": name, "choice": choice["choice"]}), flush=True)


if __name__ == "__main__":
    main()
