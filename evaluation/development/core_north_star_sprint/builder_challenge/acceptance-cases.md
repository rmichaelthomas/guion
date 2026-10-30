# Queue Digest acceptance cases

These cases are frozen before the evaluation. The evaluator receives these cases with the brief; no implementation or hidden solution is included.

## Case 1: grouped summary and tie ordering

Input JSONL:

```jsonl
{"id":"A-10","team":"billing","state":"open","priority":"high","labels":["payments","api"]}
{"id":"A-11","team":"platform","state":"closed","priority":"low","labels":["api"]}
{"id":"A-12","team":"billing","state":"open","priority":"normal","labels":["payments","on-call"]}
{"id":"A-13","team":"mobile","state":"open","priority":"high","labels":[]}
```

The output JSON must have total 4; state counts `open: 3`, `closed: 1`; priority counts `high: 2`, `normal: 1`, `low: 1`; team counts `billing: 2`, `platform: 1`, `mobile: 1`; and top labels `api: 2`, `payments: 2`, `on-call: 1` in that order. JSON object key order and whitespace are not significant.

## Case 2: empty queue

An empty file exits successfully and prints a valid summary with total 0, all known state and priority counts equal to 0, no team counts, and an empty `top_labels` list.

## Case 3: malformed second line

Use a valid first record followed by `{` on line 2. The program exits nonzero, reports line 2 and a useful parse error, and emits no partial summary to standard output.

## Case 4: invalid or duplicate record

Try a record with an unknown priority, then a pair of records with the same `id`. Each run exits nonzero, identifies the bad line, and emits no partial summary.

## Case 5: command-line use

No path or more than one path produces a concise usage message and a nonzero exit status. A valid input path under the project root is readable without adding a file-root grant.

## Acceptance record

For each case, record command, exit status, stdout, stderr, whether the output oracle passed, elapsed wall time, and any retry. Record the first documentation page or command used for each obstacle. Preserve the source, input files, output, and final acceptance record.
