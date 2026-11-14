# Comprehension harness — H-1.0

**Frozen:** 2026-11-12, against Guion 0.2.0, artifact format 64.
**Question it answers:** can a competent outsider, given only what ships in
`share/guion/`, get three ordinary things right — and how many wrong turns does it
take?

This is not a performance benchmark. The project has several of those. It has none
for the thing that produced the last four findings: a capability was present and
correct, and a careful reader could not find it, so they said it was missing. That
is a legibility defect, and it is invisible to every timing instrument.

## Why two numbers and not one

Every task reports **success** and **wrong turns** separately, and they are never
combined into a score.

A run that produces the right program after three false claims that the language
cannot do it is worse for the project than a run that fails cleanly, because the
false claims are what get repeated in advice. Success alone rewards the outcome;
wrong turns measure the cost of getting there. Blending them would hide exactly the
asymmetry this harness exists to expose.

The project-level line is therefore two numbers: **tasks closed** (0–3) and
**wrong turns** (a count, with quotes). A run closes a task only when it reaches
the right answer *and* records zero wrong turns on that task.

## Blinding

The subject is an agent that has never seen this project.

- It may read only `share/guion/` inside the distribution, plus the starter folder
  it was given. Nothing else on disk. The prompt names this as the rule.
- It may run `guion` commands. The artifact is the evidence; the docs are claims
  about the artifact, and both are in scope.
- It is told nothing about which features exist, which are missing, or which
  answer is expected.
- It must list every path it opened. The runner diffs that list.

**Contamination markers.** `harness/check.sh` greps the transcript for the
evaluation artifacts (`RECOMMENDATION-`, `BRIEF-guion-frameworks`, `PROBE-sqlite`,
`VERSIONING`, `B14`, `B13`). Any hit invalidates the run — a paraphrased reading is
not detected by grep, which is why the path list is also required and reviewed.

## The three tasks

Each targets a different comprehension failure. Together they cover the three ways
an outsider gets Guion wrong, whether or not the project's own claims are accurate.

| Task | Axis | The failure it catches |
|---|---|---|
| T1 — make a serial server serve two clients | **Buried capability** | Saying "impossible" when the artifact does it |
| T2 — sort records by a text field | **Absent capability** | Saying "use `sort`" when 0.2.0 has none, or "impossible" when a workaround exists |
| T3 — make a denied program run | **Gated capability** | Saying "impossible" instead of granting, or widening the grant past what the job needs |

Task text lives in `tasks/` and is frozen. Starters live in `starters/` and are
frozen. Rewording a task after seeing a result is the one thing that would make
this instrument useless, so any change is a new version and a new receipt.

## Ground truth (re-verified against format 64 before H-1.0 was frozen)

| Task | Verified fact | Evidence |
|---|---|---|
| T1 | Possible. One listener in the parent, passed into `parallel_map` so several workers wait in `tcp_accept` on it. Ceiling equals worker count. | `probe/srv1`, `probe/srv8`; 1 worker 905.9 ms vs 4 workers 3.8 ms on the same held-connection test |
| T2 | No text-key sort. `sort` → `E_NAME_UNKNOWN`; `sort_by_int_key` with a `Text` key → `E_SORT_KEY`. A correct sort *is* expressible: `Text` `<` compares by code point, so a 20-line insertion sort works. | `probe/T2-sort-text.gu` prints `Apple, apple, banana, fig, pear` |
| T3 | Refused by default, and the refusal is specific: absolute paths outside the granted root → `R_HOST_DENIED`; ungranted host → `R_NETWORK_DENIED`. One exact host grant is enough (`--allow-http-host example.com` → 577 bytes). Absolute writing outside the project is refused *by design*, not by configuration. | `probe/T3-capability.gu` |

T1's ground truth also carries a second, subtler fact: the pool multiplies waiting,
not computing (`PROBE-sqlite-workers.md`). A subject that finds the pattern and
flags that limit has understood the feature better than a subject that only makes
the test pass. That is recorded as an observation, not as a scoring event.

## Success, defined per task

The runner re-runs everything. A subject's claim that its check passes is not
evidence; the runner executes the check against **both** the original starter and
the subject's version.

- **T1 closed** when: the subject's program serves a second client while the first
  is held, *and* the same check fails against the untouched starter. A check that
  passes on the starter proves nothing and is counted as a wrong turn (W4).
- **T2 closed** when: the program's output is actually sorted by title in code-point
  order, and the written report states both halves of the truth — no text-key
  `sort` builtin, and the workaround that does exist.
- **T3 closed** when: the program fetches and writes, the grant names exactly one
  host, and there is no wildcard.

## Wrong turns, countable and quotable

An event counts only with a quote and a transcript location. No quote, no count.

| Code | Event |
|---|---|
| W1 | Claimed something is impossible; the artifact does it |
| W2 | Claimed something exists; the artifact has no such call |
| W3 | Invented an API and presented it as real |
| W4 | Wrote or kept a check that passes on the unmodified starter |
| W5 | Widened a grant past the job, or granted silently without saying so |
| W6 | Asked the runner for information that is in `share/guion/` |

W6 is deliberately not "asked a question". Asking how the subject should format its
report is fine. Asking whether Guion can do X, when the docs answer it, is the turn
being counted.

Three zips is fine, so to speak: reverted attempts, compile failures, and dead ends
that the subject notices and corrects are **not** wrong turns. Only statements that
are false about the artifact, or actions that make the test meaningless.

## Running it

One task = one fresh subject, one prompt, no follow-ups except the ones allowed
below.

1. Run `harness/setup.sh tN <run-number>`. It copies the whole Guion distribution
   and one starter into an isolated root, so the subject's world contains no
   evaluation notes — blinding by directory layout, not by instruction.
2. Give the subject the exact contents of `harness/PROMPT.md` with `{{TASK}}`
   replaced by `tasks/tN.md` and `{{ROOT}}` by the copy's path.
3. Budget: 40 tool calls or 20 minutes, whichever comes first. Record which was
   hit. Hitting the budget is not a failure; it is a data point.
4. If the subject asks anything, reply with the task's required output restated.
   Nothing more. Record the exchange.
5. Save the full transcript next to the receipt.
6. Run `harness/check.sh <transcript>`, then score with `score.md`.

**n matters.** Three runs per task is the target. One run is a pilot and must be
labelled `n=1` — it shows the instrument works, not what the number is. Do not
average across tasks; report three pairs.

## Receipt format

Receipts are append-only. Never edit one after it is written; a correction is a new
receipt that references the old one.

```
H-1.0 / T1 / run 1 of 3 / 2026-11-13 / artifact format 64
subject: <model or person, and anything it was told that a fresh subject is not>
budget hit: tool calls (28/40) | time (14/20 min)
success: yes | no | partial — <one line, how it was verified>
wrong turns: 2
  W1 "Guion can't accept on a shared listener" — turn 6
  W6 "does Guion have a worker pool?" — turn 3
first correct action: turn 11
notes: <what the subject did that no taxonomy captures>
paths read: <count, and any path outside share/guion/>
```

## What this harness cannot tell you

- **Why** a task failed. It measures comprehension, not cause. A W1 on T1 is consistent
  with a missing doc, a buried doc, and a misleading doc; finding out which is a
  separate read.
- **Whether a human would do better or worse.** The subject is an agent. Agents
  differ from people in exactly the ways that matter here (they read the whole
  spec; people do not).
- **Anything about scale, performance, or correctness.** Those have their own
  instruments.
- **A trend from one run.** The number is only meaningful as repeated runs, on the
  same frozen task set, across releases. The point of freezing is that H-1.0 run on
  0.3.0 is comparable to H-1.0 run on 0.2.0.

## Freeze rule

H-1.0 is the tasks, the starters, the prompt, the taxonomy, and this document. Change
any of them and it is H-1.1; the old receipts stay labelled H-1.0 and remain
comparable only to each other. Re-verify ground truth against the artifact version
in use before every campaign — a harness scored against a stale answer key reports
false wrong turns, which is worse than reporting nothing.
