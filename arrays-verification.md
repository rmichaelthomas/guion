# Native arrays verification

Generated 2026-11-12T15:59:31Z at 8d73df4.

| Check | Result | Detail |
|---|---|---|
| release binary builds | PASS |  passed,  failed |
| model: element types, codes, format-62 gating | PASS | 8 passed, 0 failed |
| Rust compiler: array typing and diagnostics | PASS | 13 passed, 0 failed |
| catalog matches the model operand table | PASS | 1 passed, 0 failed |
| construction, access, shape, bytes | PASS | 8 passed, 0 failed |
| elementwise arithmetic, comparisons, math | PASS | 8 passed, 0 failed |
| reductions, sorting, linear algebra, random | PASS | 5 passed, 0 failed |
| review edge cases (memory, overflow, non-finite) | PASS | 12 passed, 0 failed |
| self-hosted compiler parity | PASS | 5 passed, 0 failed |
| machine code on and off | PASS | 1 passed, 0 failed |
| shared catalog, both compilers | PASS | 42 passed, 0 failed |
| builtin catalog is valid | PASS |  passed,  failed |
| agent reference is current | PASS |  passed,  failed |
| three-stage bootstrap | PASS | 5 passed, 0 failed |
| conformance (stage2, stage3) | PASS | 59 passed, 0 failed |
| clippy, warnings denied | PASS |  passed,  failed |
| full workspace tests (includes old-format fixtures) | PASS | 803 passed, 0 failed |
