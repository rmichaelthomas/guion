# Wave 1 verification

Generated 2026-11-04T05:00:34Z at dd7f30c.

| Check | Result | Detail |
|---|---|---|
| release binary builds | PASS |  passed,  failed |
| B1 math, conversions, random | PASS | 3 passed, 0 failed |
| B2 encodings and URLs | PASS | 4 passed, 0 failed |
| B3 paths, filesystem, temp files | PASS | 5 passed, 0 failed |
| B3 filesystem grant denial (runtime) | PASS | 1 passed, 0 failed |
| B4 regex | PASS | 2 passed, 0 failed |
| B5 processes and signals (default-deny, limits) | PASS | 6 passed, 0 failed |
| B6 TCP and UDP (default-deny, limits) | PASS | 7 passed, 0 failed |
| B7 TLS (verification, rejection, grants) | PASS | 3 passed, 0 failed |
| B8 HTTP server, WebSockets, SHA-1 | PASS | 6 passed, 0 failed |
| artifact format gating (formats 52 to 59) | PASS | 46 passed, 0 failed |
| three-stage bootstrap | PASS | 5 passed, 0 failed |
| conformance (stage2, stage3) | PASS | 59 passed, 0 failed |
| clippy, warnings denied | PASS |  passed,  failed |
| full workspace tests (includes old-format fixtures) | PASS | 614 passed, 0 failed |
