"""Independent standard-library implementation of the task-ledger workload."""

import json
import sys
from pathlib import Path


def load_tasks(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return []


def save_tasks(path: Path, tasks):
    path.write_text(
        json.dumps(tasks, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8",
    )


def main(arguments):
    path = Path("tasks.json")
    if not arguments:
        print("Usage: veritas run . <list|summary|add TITLE NOTE|done TITLE|remove TITLE>")
        return 2
    tasks = load_tasks(path)
    command, *rest = arguments

    if command == "list":
        if rest:
            print("Usage: list")
            return 2
        if not tasks:
            print("No tasks.")
        for task in tasks:
            marker = "x" if task["done"] else " "
            print(f"[{marker}] {task['title']} — {task['note']}")
        return 0
    if command == "summary":
        if rest:
            print("Usage: summary")
            return 2
        done = sum(task["done"] for task in tasks)
        print("Total tasks:")
        print(len(tasks))
        print("Completed tasks:")
        print(done)
        print("Open tasks:")
        print(len(tasks) - done)
        return 0
    if command == "add":
        if len(rest) != 2:
            print("Usage: add TITLE NOTE")
            return 2
        title, note = rest
        if not title:
            print("Task title cannot be empty.")
            return 2
        if any(task["title"] == title for task in tasks):
            print("A task with that title already exists.")
            return 2
        tasks.append({"title": title, "note": note, "done": False})
        save_tasks(path, tasks)
        print(f"Added: {title}")
        return 0
    if command in {"done", "remove"}:
        if len(rest) != 1:
            print(f"Usage: {command} TITLE")
            return 2
        title = rest[0]
        if not any(task["title"] == title for task in tasks):
            print(f"Task not found: {title}")
            return 1
        if command == "done":
            for task in tasks:
                if task["title"] == title:
                    task["done"] = True
            save_tasks(path, tasks)
            print(f"Completed: {title}")
        else:
            save_tasks(path, [task for task in tasks if task["title"] != title])
            print(f"Removed: {title}")
        return 0

    print(f"Unknown command: {command}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
