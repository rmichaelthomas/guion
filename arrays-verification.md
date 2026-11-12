# Native arrays verification

Generated 2026-11-12T00:49:20Z at 1168a1d.

| Check | Result | Detail |
|---|---|---|
| release binary builds | PASS |  passed,  failed |
| model: element types, codes, format-62 gating | PASS | 7 passed, 0 failed |
| Rust compiler: array typing and diagnostics | PASS | 11 passed, 0 failed |
| catalog matches the model operand table | PASS | 1 passed, 0 failed |
| construction, access, shape, bytes | PASS | 8 passed, 0 failed |
| elementwise arithmetic, comparisons, math | PASS | 4 passed, 0 failed |
| reductions, sorting, linear algebra, random | PASS | 5 passed, 0 failed |
| self-hosted compiler parity | PASS | 2 passed, 0 failed |
| machine code on and off | PASS | 1 passed, 0 failed |
| shared catalog, both compilers | PASS | 42 passed, 0 failed |
| builtin catalog is valid | PASS |  passed,  failed |
| agent reference is current | PASS |  passed,  failed |
| three-stage bootstrap | PASS | 5 passed, 0 failed |
| conformance (stage2, stage3) | PASS | 59 passed, 0 failed |
| clippy, warnings denied | PASS |  passed,  failed |
| full workspace tests (includes old-format fixtures) | PASS | 713 passed, 0 failed |
