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

## Follow-up fixes (after review)

The first version of this report listed several defects as accepted shortfalls. They were worked on afterwards, in this order.

1. **Compiler bug (fixed, 0f56aa3).** A function returning a record that holds Text, called from another machine-code function (including `Err(helper(...))`), failed with `R_NATIVE_RESULT`. The caller assumed every record result was a block of numeric fields. It now checks the layout. A regression test covers direct, nested and `Err(...)` forms. No workaround remains in the stdlib; inline `std_error(...)` calls are ordinary code.
2. **Speed (partly fixed, d528f6c).** `Math`, `IntToFloat` and `FloatToInt` are now compiled to machine code. Errors replay in the VM, so the error raised is the VM's. Raw samples are in `wave-1-b1-math/samples.txt`.

| Benchmark | Guion before | Guion after | CPython |
|---|---|---|---|
| 2,000,000-iteration math loop | about 1.1 s | about 0.10 s | about 0.43 s |
| 100,000 base64 and hex round trips | not measured | about 0.18 s | about 0.10 s |

   The earlier "3.2 times slower" figure was an estimate; the measured gap was 2.5 times (1.1 s against 0.43 s). It is now about 4 times faster than CPython.
3. **Sockets and HTTP server (fixed).** Socket handles are shared between `parallel_map` workers, so several workers accept on one listener and serve connections at once. The HTTP server has keep-alive replies, pipelined requests and chunked request bodies. Tests cover concurrent keep-alive connections and request-smuggling refusals.

## Still open

- **Codec, regex, file, process and socket operations are not compiled to machine code.** A function using one runs on the VM. The codec loop above is about 1.8 times slower than CPython. Fixing it needs a generic bridge plus `Bytes` and `Result` handling in the machine-code type system. Not started.
- **`random` runs on the VM.** It needs unsigned 64-bit arithmetic; the machine-code integer is signed 64-bit.
- Floating-point results come from the platform libm and can differ in the last digit between targets. A Guion sum also associates differently from the equivalent Python expression (`a + b + c` against `a += b + c`), so printed totals differ in trailing digits.
- `guion run` grants cwd read and write by default.
- Random is seedable and deterministic only; there is no entropy grant. WebSocket client masks derive from a caller-supplied seed and the payload.
- `remove` and `rename` follow symlinks.
- The allocation budget caps quadratic string building at about 60 KB.
- Socket hostname grants match the name as written, before resolution.
- HTTP: no HTTP/2 and no `Expect: 100-continue`. Cancellation of a blocked call is bounded by its timeout.
- TLS: no client certificates, ALPN or session-resumption control. An abrupt peer close surfaces as `E_NET_IO`. Hostname mismatch is not tested against a real mismatching peer.
- SHA-1 is kept only for WebSocket and legacy use.
- `docs/status.md` and the docs-propagation pass are still owed.
