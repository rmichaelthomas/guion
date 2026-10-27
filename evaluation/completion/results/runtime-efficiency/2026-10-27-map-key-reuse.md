# Reuse existing map-key storage

- Date: 2026-10-27
- Branch: `codex/guion-richards-gap-closure`
- Baseline revision: `709b2d5`
- Baseline binary SHA-256: `a8728d613aaf43c3f81ed0fedc5d59fa49ea38709da2067a264d8b2bad55e65d`
- Candidate binary SHA-256: `6be9feaa4b1c59ad5771007b0504d9a318ab3737c9736bc7dc7ac89a138e9a7e`
- Host: macOS ARM64

## Change

When `map_set` replaces the value for an existing key, the VM now updates that value in place within the map's owned storage instead of allocating a replacement `String` for the key. New keys still allocate their required owned key. Shared maps still take the existing copy-on-write path, preserve older aliases, and keep the existing allocation and collection-limit checks.

A VM-core regression test verifies that replacing a value preserves the existing key buffer. The map update behavior and aliasing are also covered by the runtime execution suite.

## Everyday task results

The direct comparison used seven randomized pairs and two warmup pairs for each selected app case. Each app output passed the frozen oracle on every run; Guion source caching was bypassed.

| Application | Candidate/previous ratio (95% interval) | Candidate wins |
| --- | ---: | ---: |
| SLA report | 0.986 (0.949–1.017) | 4/7 |
| JSONL summary | 1.034 (0.975–1.044) | 2/7 |
| Release notes | 0.954 (0.926–0.984) | 7/7 |
| Calendar conflicts | 0.996 (0.993–1.015) | 5/7 |
| Webhook replay | 0.998 (0.967–1.015) | 5/7 |
| Service health | 1.001 (0.985–1.169) | 3/7 |
| Config migrator | 1.019 (0.998–1.037) | 2/7 |
| Document index | 0.973 (0.946–1.007) | 5/7 |
| Lockfile audit | 0.978 (0.947–1.074) | 4/7 |

Release notes was **4.6% faster**, with its full paired interval below 1. The other eight intervals cross parity, so these samples establish neither broad gains nor broad regressions.

The 20,000-row JSONL comparison was effectively unchanged: **356.0 ms** before and **357.8 ms** after; paired ratio **1.005** (95% interval **0.990–1.025**), with 7/15 candidate wins. Every output matched the independent Python and SQLite expected results. This case is a useful guardrail, not a demonstrated gain for this map change. See the [JSONL result](2026-10-27-map-key-reuse-jsonl-head-to-head.json).

The complete nine-app acceptance rerun passed **127/127** cases in both Guion and Python. Full trial samples and acceptance records are in the [nine-app head-to-head](../real-task-sprint/2026-10-27-map-key-reuse-nine-task-head-to-head.json) and [acceptance result](../real-task-sprint/2026-10-27-map-key-reuse-nine-task-acceptance.json).

### Current comparison with Python

A separate seven-pair run measured this candidate against the existing Python reference on the same selected frozen case for each application. Every output passed its oracle. Guion/Python median ratios were: SLA report **0.173**, JSONL summary **0.165**, release notes **0.254**, calendar conflicts **0.660**, webhook replay **0.197**, service health **0.841**, config migrator **0.136**, document index **0.182**, and lockfile audit **0.167**. These ratios describe the named implementations and cases, not general language performance. Full timings and intervals are in the [current Python comparison](../real-task-sprint/2026-10-27-map-key-reuse-nine-task-python-comparison.json).

## Verification and limits

- `cargo test -p guion-vm-core --locked --offline --jobs 2`: passed, including the key-storage test and 7 portable execution tests.
- `cargo test -p guion-runtime --test execution --locked --offline --jobs 2`: passed, 117 tests.
- All comparisons used fresh processes with source caching disabled; OS file cache was uncontrolled.
- Guest allocation charges do not measure physical allocator calls. This report makes no physical allocation or RSS claim.

The release-notes gain supports retaining the small map update optimization as a targeted everyday-task improvement. It does not show that Guion is generally faster than Python.
