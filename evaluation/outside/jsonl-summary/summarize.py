import json
from collections import Counter

by_class = Counter()
by_path = Counter()
requests = 0
bad_lines = 0
failures = 0

handle = open("access.jsonl")
for line in handle:
    try:
        row = json.loads(line)
    except json.JSONDecodeError:
        bad_lines += 1
        continue
    status = row["status"]
    requests += 1
    by_class[str(status // 100) + "xx"] += 1
    by_path[row["path"]] += 1
    if status >= 400:
        failures += 1
handle.close()

print("requests: " + str(requests))
print("bad lines: " + str(bad_lines))
for name in sorted(by_class):
    print(name + ": " + str(by_class[name]))
rate = (failures * 100) // requests if requests else 0
print("error rate: " + str(rate) + "%")
top = "-"
top_count = 0
for path in sorted(by_path):
    if by_path[path] > top_count:
        top = path
        top_count = by_path[path]
print("top path: " + top + " " + str(top_count))
