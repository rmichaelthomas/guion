# Standard library Wave 1 report

Date: 2026-11-03. Branch `feat/stdlib-wave-1`. Artifact format 51 to 59. Verification table: `wave-1-verification.md` (produced by `scripts/verify-wave-1.sh`).

## What landed

| Stage | Commit | Contents | Format |
|---|---|---|---|
| B1 | d13a955 | conversions, `std:math`, `std:random` (seeded, deterministic) | 52 |
| B2 | 9c30a0b | base64, hex, percent-encoding, `std:url` | 53 |
| B3 | ab46dcb | filesystem operations, `std:path`, temp files | 54 |
| B4 | 484a672 | `std:regex` | 55 |
| B5 | c56ccca | `std:process` (command grant, timeouts, signal polling) | 56 |
| B6 | 5f58aec | `std:net` TCP and UDP (host:port grants) | 57 |
| B7 | f6cd773 | TLS client and server in `std:net` | 58 |
| B8 | 4f9ca29 | `std:http_server`, `std:websocket`, SHA-1, byte XOR | 59 |

## Verification (local, at 4f9ca29)

Full workspace: 608 tests passed, 0 failed. Clippy with warnings denied is clean. Three-stage bootstrap and conformance (stage2, stage3) pass. Format gating tests and old-format fixtures pass. Every new capability (process, connect, bind) is default-deny and has a denial test.

## Measurements

- B1 math loop: Guion is about 3.2 times slower than CPython. Cause: the new ops are not JIT-native, so functions containing them run on the VM.
- B4 regex: Guion was faster than CPython `re`. Raw paired samples are in `evaluation/completion/results/wave-1-b4-regex/`.
- Stages B5 to B8 were verified for correctness and limits, not benchmarked.

## Shortfalls

1. New math, codec, fs, regex, process, net and tls ops are not JIT-native (see above).
2. Pre-existing JIT bug, not fixed: a helper call inside `Err(...)` in a stdlib wrapper raises `R_NATIVE_RESULT`. Workaround (inline `Err(std_error(a, b))`) applied in several stdlib files.
3. `guion run` grants cwd read/write by default.
4. Random is seedable and deterministic only; there is no entropy grant. WebSocket client masks derive from a caller-supplied seed and the payload.
5. Float last digits can differ across targets; tests compare to tolerance.
6. Remove and rename follow symlinks.
7. The allocation budget caps quadratic string building at about 60 KB.
8. Socket hostname grants match the name as written, before resolution.
9. Socket handles cannot be shared across `parallel_map` workers, so servers handle one connection at a time. Cancellation latency is bounded by call timeouts.
10. HTTP server: no keep-alive, chunked bodies or HTTP/2. One request per connection.
11. TLS: no client certificates, ALPN or session-resumption control. An abrupt peer close surfaces as `E_NET_IO`. Hostname mismatch is not exercised against a real mismatching peer.
12. SHA-1 is kept only for WebSocket and legacy use.

## Not done

Nothing pushed, no PR, no workflow run. `docs/status.md` not updated. Docs-propagation pass (stale format numbers, v0.2 design links) still owed.
