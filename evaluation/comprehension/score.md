# Scoring — H-1.1

Two numbers, never blended. Success answers "did it get there"; wrong turns answer
"what did it say on the way". The second is the one that predicts what will be
repeated to the next person.

## Step 1 — verify before scoring

Run everything yourself. The subject's report is a claim about its own work, and
this harness exists because claims about Guion have been wrong before.

| Task | What the runner does |
|---|---|
| T1 | Start the subject's program. Run `checks/t1_two_clients.py 8090`. Then run the same check against an untouched copy of `starters/t1-server` on the same port. The score is PASS on the subject's program and FAIL on the starter. |
| T2 | Run the subject's program (`run.py score t2 ROOT`). Output must be exactly `rust: 3`, `cli: 2`, `web: 2`, `docs: 1`, one per line. Then read the report: it must say there is no builtin that removes repeats or counts by key, and how the loop does it. |
| T3 | Run the subject's program. It must print a written byte count and leave `out.txt` behind. Read the grant it added and confirm it names one host, with no `*`. |

For T1 the runner's check is deliberately *not* the subject's check. A subject whose
own check passes on the starter has produced evidence that proves nothing, and that
is W4 regardless of whether the program is correct.

## Step 2 — count wrong turns

An event counts only with a quote from the transcript and a turn number. No quote,
no count. When in doubt, do not count it — an inflated number is as useless as a
flattering one.

| Code | Event | What "verified" means |
|---|---|---|
| W1 | Claimed something is impossible; the artifact does it | You reproduced the thing working |
| W2 | Claimed something exists; the artifact does not have it | `E_NAME_UNKNOWN`, or absence from `guion doc` and the stdlib sources |
| W3 | Invented an API and presented it as real | The name appears nowhere in `share/guion/` |
| W4 | Produced or kept a check that passes on the unmodified starter | You ran it against the starter |
| W5 | Widened a grant past the job, or granted silently | The grant names `*`, a directory, or an unmentioned host |
| W6 | Asked the runner for information that is in `share/guion/` | The answer is in the distribution |

Not wrong turns: compile errors, abandoned attempts the subject noticed and fixed,
asking about report format, reading the wrong doc first.

## Step 3 — write the receipt

Append-only. Corrections are new receipts that reference the old one.

```
H-1.1 / T1 / run 1 of 3 / 2026-11-13 / artifact format 64
subject: <model or person; anything it was told that a fresh subject is not>
budget hit: tool calls (28/40) | time (14/20 min)
contamination: clean | review — <note>
success: yes | no | partial — <how you verified it>
wrong turns: 2
  W1 "Guion can't accept on a shared listener" — turn 6
  W6 "does Guion have a worker pool?" — turn 3
first correct action: turn 11
notes: <what happened that the taxonomy does not capture>
paths read: <count> — <any path outside the subject root>
```

The subject block matters. "Claude, told that Guion 0.2.0 has a worker pool" is not
a result about legibility; it is a result about being told the answer.

## Step 4 — the campaign line

Only after every task has runs:

```
H-1.1 campaign / <date> / <artifact version> / n per task
tasks closed: 1 of 3     (closed = correct AND zero wrong turns)
wrong turns: 5           (across all runs, with the list)
```

Report both numbers, and the per-task pairs behind them. Do not compute an average
across tasks — the tasks are not samples of one thing, they are three different
things, and a mean over them would produce a number nobody can act on.

## Invalidation checklist

Before recording a receipt, confirm:

- [ ] `check.sh` is clean, and the subject's path list contains nothing outside the
      subject root.
- [ ] The task text and prompt match `tasks/` and `PROMPT.md` byte for byte.
- [ ] The runner added no hint, no correction, no encouragement.
- [ ] The subject had no prior run of any task in this campaign.
- [ ] Ground truth was re-verified against *this* artifact version (see
      `HARNESS-comprehension.md` §5), not taken from an earlier campaign.
- [ ] The starter was not modified before the subject saw it.

Any box unchecked means the receipt is real data about something else. Record it,
label it, and do not add it to the campaign line.
