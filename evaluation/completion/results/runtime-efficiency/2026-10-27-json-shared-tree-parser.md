# Direct shared-tree JSON parser — 2026-10-27

Guion now parses JSON directly into its immutable shared JSON representation.
The previous path first built a generic `serde_json::Value` tree, measured it,
then converted every node into Guion's shared tree. The new path creates the
shared nodes during deserialization and applies the same depth, size, integer,
duplicate-key, and collection checks.

## Large JSONL result

The comparison used 200,000 generated rows, uncached Guion source, randomized
baseline/candidate order, and output validation against an independent Python
result after every run. The saved baseline is commit `0380197`; it has the same
runtime source as `1a0815d`.

Across 32 confirmation pairs, the candidate's paired median ratio was **0.9885**
(95% interval **0.9779–0.9994**), about **1.2% less time**. Raw medians were
**3.739 s** for the baseline and **3.681 s** for the candidate. Eleven of the 32
pairs tied or regressed, so this is a small improvement with run-to-run noise,
not a large step. Including the initial seven-pair screen gives a ratio of
**0.9874** (95% interval **0.9734–0.9963**) across 39 pairs.

Estimated guest allocation-budget charges fell **2.1%** in one instrumented
run. This is a VM budget estimate, not a count of host allocations. Five peak
RSS samples per build showed medians of **14.29 MB** and **14.22 MB**; the
samples overlap, so this does not establish a memory reduction. Guest branch
counts were unchanged.

## Correctness and scope

The direct parser matched the prior parser for scalar values, nested objects,
Unicode strings, arbitrary-precision integers, reserved-looking user keys, and
numeric strings. It preserved malformed-JSON errors and the line and column
reported for escaped duplicate keys. The runtime suite passed **184/184 tests**.
Guion passed **127/127** frozen cases across the nine applications.

The Python reference passed **116/127** cases in that acceptance run. Its 11
calendar failures all reported `No module named 'icalendar'`; the pinned Python
reference dependency was absent from this environment. No Python calendar
results are counted as passing.

This result is specific to JSON-heavy work. It does not establish a general
speed advantage over Python. The OS file cache was uncontrolled, and the
profile's allocation values are not physical allocation counts or host CPU
attribution.

## Evidence

- Full paired timings, trial order, RSS samples, runtime profile, and the
  separate timestamp-formatting experiment are in
  [the evidence bundle](2026-10-27-jsonl-runtime-follow-ups.json).
- The complete nine-application acceptance record is in
  [the acceptance JSON](../real-task-sprint/2026-10-27-json-shared-tree-nine-task-acceptance.json).
