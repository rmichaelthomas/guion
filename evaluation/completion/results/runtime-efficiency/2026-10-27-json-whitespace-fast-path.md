# JSONL whitespace no-op fast path — 2026-10-27

The runtime now keeps the original immutable text value when Unicode whitespace
collapsing would leave it unchanged. The existing Unicode normalization path
still runs when the result differs.

## Result

The 20,000-row screen was inconclusive: the candidate/baseline median ratio was
**0.997** with a 95% interval of **0.984–1.024**. On 200,000 rows, the seven-pair
screen measured **0.995** (interval **0.983–1.004**). The 16-pair confirmation
measured **0.993** (interval **0.979–1.0002**), with medians of **3.789 s** for
the baseline and **3.771 s** for the candidate. This is a small observed
difference, but the interval includes parity, so it does **not** establish a
reliable latency improvement.

One guest-runtime profile estimated **4,009,001,609** gross allocation-budget
bytes for the baseline and **3,994,548,252** for the candidate, about **0.36%
lower**. These are VM budget estimates, not physical allocation counts, peak
memory, or host CPU attribution.

Every timed output matched the independent Python result. The JSONL application
acceptance run passed **11/11 cases** in both Guion and Python. The Guion runtime
suite passed **181 tests**, formatting passed, and the release CLI built.

The source cache was bypassed. OS file-cache state was not controlled. The exact
baseline and candidate binaries, randomized pairs, samples, and profile counters
are in the linked JSON evidence. The candidate was commit `1a0815d`.

## Evidence

- [20,000-row screen](2026-10-27-json-whitespace-fast-path-20k-screen.json)
- [200,000-row screen](2026-10-27-json-whitespace-fast-path-200k-screen.json)
- [200,000-row 16-pair confirmation](2026-10-27-json-whitespace-fast-path-200k-confirmation.json)
- [Guest-runtime profile](2026-10-27-json-whitespace-fast-path-200k-guest-profile.json)
- [JSONL acceptance results](../real-task-sprint/2026-10-27-json-whitespace-fast-path-jsonl-acceptance.json)
