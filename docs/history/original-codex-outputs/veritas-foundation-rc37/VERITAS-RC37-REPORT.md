# Veritas RC37 build report

**Verdict:** RC37 adds a packaged, repeatable stress probe for a persistent Veritas VM and Python sidecar. It completes 10,000 burst NumPy/FFI calls and 250 paced calls with exact results, and correctly shuts down. This is a useful lifecycle and interoperability proof. It does not establish that Veritas should be recommended over Python.

## What changed

- Added a Rust benchmark workload and Python runner that measure persistent NumPy array conversion, exact result/output correctness, burst throughput, paced-call latency, scheduling lateness, close time, and use-after-close behavior.
- Included the runner, source workload, compiled benchmark executable, and v1.18 checkpoint in the candidate package.
- Preserved RC36 unchanged.

## Packaged run

The final package was extracted to a new temporary directory. `veritas doctor` passed; the managed Python/NumPy environment synced offline from the existing local cache; the packaged Python interop example printed the expected four lines; and all four packaged task-ledger acceptance tests passed. The packaged stress executable then ran on macOS 26.5.2 arm64 with CPython 3.14.8 and NumPy 2.5.4: 512 array elements, 50 warmups, 10,000 burst calls, and 250 calls paced at 10 ms. All calls returned the exact sum and matching printed output. The instance closed in 1.519 ms and rejected a later call with `R_INSTANCE_CLOSED`.

| Measurement | Burst run | Paced run |
|---|---:|---:|
| Calls | 10,000 | 250 |
| p50 latency | 0.731 ms | 2.687 ms |
| p95 latency | 0.897 ms | 5.349 ms |
| p99 latency | 1.207 ms | 7.219 ms |
| Maximum latency | 8.512 ms | 8.672 ms |
| Throughput | 1,315.1 calls/s | 99.8 calls/s |

Paced scheduling lateness in the final packaged run was p50 1.438 ms, p95 2.227 ms, p99 3.196 ms, and maximum 3.738 ms. Across three same-settings author runs (one source-built, two packaged), burst p50 ranged from 0.675–0.731 ms; paced p50 ranged from 0.953–2.687 ms and paced maximum ranged from 7.409–11.173 ms. This variation makes the measurements useful as an initial instrument and regression signal, not as a stable performance claim. These numbers are measurements of one small bridge workload on one author machine; they are not a language-to-language speed comparison.

The sandbox denied `ps` process inspection (`Operation not permitted`). No RSS samples or memory trend are claimed. The RSS sampler targets the Rust benchmark process only and does not include the Python sidecar. The full test commands also encountered this sandbox's loopback restriction: a feed-digest test could not bind its local fixture server, and one CLI network test failed while starting a local server. Of 17 CLI integration tests, 16 passed. The remaining independently runnable compiler/model/runtime tests passed: 14 compiler/model tests, 58 execution and Python bridge tests, and 3 CLI unit tests. Formatting and warning-denied Clippy checks passed.

## Release artifact

- Archive SHA-256: `3f85f07b4e912ca818c09f0443e240a547b5990db5385032142cf6f61d00141e`
- Target: macOS arm64 only; package is unsigned and not notarized.
- Managed Python setup reused an existing local cache. This is an extraction and relocation check, not clean-machine or cross-platform evidence.

## Decision and next work

**Recommendation verdict: keep Python as the general-purpose default.** RC37 demonstrates that a Python ecosystem package can be called from Veritas repeatedly through one persistent sidecar, with typed NumPy-to-list conversion and explicit lifecycle checks. It does not compare authoring effort, deployment costs, correctness under unseen changes, or performance against an equally configured Python program. The benchmark's RSS trend remains open due to sandbox restrictions. Next, repeat memory observation on an unrestricted host and continue with the broader ordinary-program and independent comparative evaluation in the approved plan.

## Files

- Installable package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Packaged candidate directory: [veritas-0.1.0-darwin-arm64](veritas-0.1.0-darwin-arm64/)
- Packaged-workload machine report: [packaged-runtime-lifecycle-stress.json](packaged-runtime-lifecycle-stress.json)
- Source-workload machine report: [runtime-lifecycle-stress.json](runtime-lifecycle-stress.json)
- Checkpoint: [veritas_checkpoint_v1_18_runtime_lifecycle.md](veritas-0.1.0-darwin-arm64/docs/veritas_checkpoint_v1_18_runtime_lifecycle.md)
