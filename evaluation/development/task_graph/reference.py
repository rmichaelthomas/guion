#!/usr/bin/env python3
"""Standard-library reference for the task graph planner."""

import json
import sys
from pathlib import Path


USAGE = "Usage: veritas run . INPUT.json OUTPUT.json [--priority]"


def plan(document, priority_mode=False):
    tasks = document.get("tasks")
    if not isinstance(tasks, list):
        raise ValueError("missing tasks array")

    by_id = {}
    for task in tasks:
        task_id = task.get("id")
        if not isinstance(task_id, str) or not task_id:
            raise ValueError("task id cannot be empty")
        if task_id in by_id:
            raise ValueError(f"duplicate task id {task_id}")
        dependencies = task.get("depends_on")
        if not isinstance(dependencies, list) or any(not isinstance(dep, str) for dep in dependencies):
            raise ValueError(f"invalid dependencies for {task_id}")
        by_id[task_id] = task

    for task_id, task in by_id.items():
        for dependency in task["depends_on"]:
            if dependency not in by_id:
                raise ValueError(f"unknown dependency {dependency} for {task_id}")

    complete = set()
    order = []
    while len(complete) < len(by_id):
        ready = [
            task_id
            for task_id, task in by_id.items()
            if task_id not in complete and all(dep in complete for dep in task["depends_on"])
        ]
        if not ready:
            raise ValueError("dependency cycle")
        if priority_mode:
            ready.sort(key=lambda task_id: (-int(by_id[task_id].get("priority", 0)), task_id))
        else:
            ready.sort()
        selected = ready[0]
        complete.add(selected)
        order.append(selected)
    return {"order": order, "count": len(order)}


def main(arguments):
    if len(arguments) not in (2, 3) or (len(arguments) == 3 and arguments[2] != "--priority"):
        print(USAGE)
        return 2
    priority_mode = len(arguments) == 3
    try:
        document = json.loads(Path(arguments[0]).read_text(encoding="utf-8"))
        result = plan(document, priority_mode)
        Path(arguments[1]).write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    except (OSError, json.JSONDecodeError, AttributeError, TypeError, ValueError) as exc:
        message = str(exc)
        if isinstance(exc, json.JSONDecodeError):
            print(f"Invalid JSON: R_JSON_SYNTAX — {exc.msg}")
        elif isinstance(exc, OSError):
            print(f"Cannot read input: R_FILE_NOT_FOUND — {message}")
        elif message.startswith("task id") or message.startswith("duplicate task id") or message.startswith("unknown dependency") or message == "dependency cycle" or message == "missing tasks array":
            print(f"Invalid task graph: {message}")
        else:
            print(f"Invalid task graph: {message}")
        return 1
    print("Planned tasks:")
    print(result["count"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
