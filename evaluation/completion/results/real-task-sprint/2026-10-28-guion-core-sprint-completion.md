# Guion Core North-Star Sprint — Completion Report

Date: 2026-10-28
Branch: `codex/guion-core-sprint-spec`
Implementation source revision: `7c7b8a95c774da6b0d08d5f804e138440b2896d8`
Final release binary SHA-256: `d8d495abbe220e5602f4c63f940394b2b65cb6f186104fbee2cfdcc9312ffb6b`

## Outcome

The typed HTTP response work, capability audit, documentation updates, and implementation verification are complete. The final seven-workload Debian ARM64 run checked its outputs successfully. Its overall status is `failed` only because the preset speed-win gate was not met on JSONL; the two JSONL lanes remain substantially slower than the pinned Python data reference. No general speedup claim is made, and neither JSONL optimization candidate was retained.

Task 7's unfamiliar-builder packet is prepared but the challenge has not been run. No materials have been shared. It remains open until the user names an evaluator and approves the exact recipient and materials to transfer.

## Implementation

- Added `http_response_text(HttpResponse) -> Result[Text, HttpError]` with checked text-size and guest-allocation limits, recoverable UTF-8 errors, and unchanged HTTP status/body semantics.
- Added `std:http.http_response_json(HttpResponse) -> Result[Json, StdError]`, preserving established error codes and messages.
- Added append-only opcode tag 165 and artifact format 45. Format-44 artifacts remain readable, and format-44 rejects the new opcode.
- Updated compiler/runtime/model/CLI coverage, the builtin catalog and generated agent reference, the standard library, examples, conformance inventory, and compatibility documentation.
- Audited URL grants and redirects, file roots, environment grants, cancellation, resource ceilings, and static inspection. The audit names the DNS-rebinding and lack-of-OS-process-isolation limits.

## Final performance evidence

The pinned Debian 12 ARM64 container ran Linux `6.8.0-117-generic`, Rust/Cargo 1.97.1, and CPython 3.14.7 with the GIL enabled. The broad suite used 16 randomized trials per candidate and seed `20261025`; the short-source/HTTP pair used 15 trials, two warmups per arm, and 20,000 paired bootstrap resamples. Guion source compilation cache was bypassed where specified; operating-system file-cache state was uncontrolled. Timings are child-process wall time. The raw reports retain the trial order, intervals, phase profiles, input identity, and memory-method details.

| Workload | Final median | Reference median | Candidate/reference or candidate/base | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| Tiny source, final vs rebuilt base | 9.256 ms | 9.401 ms | 0.991× candidate/base | 0.955–1.028 |
| Local HTTP JSON (65,579-byte body) | 13.409 ms | 12.659 ms | 1.064× candidate/base | 1.006–1.210 |
| JSONL, 20,000 rows, artifact vs CPython data reference | 0.566 s | 0.121 s | 4.632× | 4.477–4.758 |
| JSONL, 200,000 rows, artifact vs CPython data reference | 5.879 s | 0.810 s | 7.113× | 6.684–7.384 |

The tiny-source pair is consistent with parity. The typed local HTTP path was about 6.4% slower than the legacy path in this measurement; its 51 expected fixture requests were all observed and its output matched. The JSONL speed gate failed at 20,000 rows, and 200,000 rows also missed the Python reference by a wide margin. The other required broad comparisons (Richards, N-body, Mandelbrot, and parallel CPU) met their named Python-reference gates. These are results for the named workloads and this target only.

The initial short-lane baseline executable (`f7d20351…`) had unusually low timings and did not match a rebuild from the frozen base with the same locked release toolchain; its build flags are not present in retained records. It was not used for the final paired conclusion. The final pair uses a fresh base build from `a1c0d40` (SHA-256 `d182bfffac526de8fe4828763558b523c14c1070af3c7c127863a16b9679ce92`) and the final candidate (SHA-256 above). The earlier comparisons remain preserved as diagnostics, not as authoritative speed evidence.

Peak RSS uses the runner's `resource.getrusage` child-process method. For process-pool measurements, the report records individual worker peaks and their sum as an upper bound, not a simultaneous process-tree peak. The target-specific memory method and limits are documented in the raw report.

- Broad raw samples and phase data: [`2026-10-28-linux-arm64-final.json`](2026-10-28-linux-arm64-final.json)
- Short-source and HTTP raw samples: [`2026-10-28-short-http-final.json`](2026-10-28-short-http-final.json)
- Earlier baseline-mismatch diagnostics: [`Linux`](2026-10-28-linux-arm64-before-dispatch-isolation.json), [`short-source and HTTP`](2026-10-28-short-http-before-dispatch-isolation.json)

## Verification

Successful checks in the pinned environment:

- `cargo xtask bootstrap` — self-host compiler parity passed (1 test), compiler seed regeneration passed, compiler-stage equality passed (1 test), and embedded compiler CLI checks passed (3 tests).
- `cargo test --locked --offline --workspace --release -- --skip package_launcher_uses_and_cleans_a_temporary_uv_cache_when_bundle_is_read_only` — passed. CLI tests passed 52/52 and compiler conformance passed 39/39. The root-only package-cache case was run separately as UID 65534 and passed 1/1.
- `cargo fmt --all -- --check`, `cargo xtask artifact-format-audit`, `cargo xtask conformance --inventory-only`, and `cargo xtask agent-reference --check` — passed.
- Agent index schema validation passed, and `scripts/verify-agent-examples.sh` passed 26/26.
- The final broad benchmark completed all seven workloads with checked outputs. Its reported failure is the preset JSONL speed gate, not a correctness or runtime failure.

No PR was created and no branch was merged. The final docs and raw results are committed on the named branch.
