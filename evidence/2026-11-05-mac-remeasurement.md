# Mac re-measurement after the helper-call and Richards fixes (2026-11-05)

This replaces the numbers in six Mac claims on the site. Every figure below came from one set of runs on one Apple Silicon Mac running macOS 27 (27.0.1), with CPython 3.14.7. The Guion build is main plus the Richards fix, merged as pull request 39 (commit `484cefd`, artifact format 61). The earlier numbers were from commit `6b5e5aba` at artifact format 44.

## Why this was needed

Pull request 37 made loops that pass lists, maps and text through helper functions linear instead of quadratic. Its first version was too broad: it moved any function that calls a list-taking helper off machine code, and Richards ran at about 1.6 times Python's time. This re-measurement found that, and pull request 39 narrowed the rule. The numbers here are from after that fix.

## Richards

Starting from source with no compile cache, Guion finished the Richards benchmark in 28.03 ms. CPython took 64.70 ms. The ratio was 0.433 (0.426–0.440). Peak memory was 23.4 MiB for Guion and 18.2 MiB for Python. The run was 15 randomized trials. Free-threaded Python was unavailable on this host.

## Five short programs

Each program ran from source in 21 randomized pairs against Python, in fresh processes with outputs checked. The file-cache state was uncontrolled.

| Program | Ratio, Guion over Python (95% interval) |
|---|---|
| Tiny launch | 0.200 (0.198–0.204) |
| Arithmetic | 0.234 (0.232–0.239) |
| JSON file | 0.184 (0.179–0.192) |
| Imported helper | 0.232 (0.229–0.237) |
| SQLite and TOML | 0.231 (0.224–0.237) |

## Nine everyday applications

Each application ran in seven randomized timing pairs, in fresh processes, outputs checked against the expected results. Guion was faster in all 7 pairs on every application.

| Application | Guion vs Python (median) · ratio (95% interval) |
|---|---|
| SLA report | 14.5 ms vs 85.7 ms · 0.172 (0.165–0.174) |
| JSONL summary | 21.5 ms vs 86.0 ms · 0.250 (0.243–0.254) |
| Release notes | 25.3 ms vs 84.7 ms · 0.298 (0.290–0.304) |
| Calendar conflicts | 281.6 ms vs 408.0 ms · 0.697 (0.676–0.703) |
| Webhook replay | 159.1 ms vs 731.8 ms · 0.215 (0.213–0.231) |
| Service health | 558.2 ms vs 674.6 ms · 0.826 (0.799–0.831) |
| Config migrator | 12.5 ms vs 84.0 ms · 0.149 (0.146–0.151) |
| Document index | 45.1 ms vs 250.9 ms · 0.180 (0.176–0.182) |
| Lockfile auditor | 15.5 ms vs 83.5 ms · 0.183 (0.180–0.186) |

The ratios run from 0.149 to 0.826. The smallest gain was Service health, at 0.826 of Python's time. Broader task classes and carefully matched Python implementations are still needed.

## Conformance

Guion passed 127 of 127 cases on the frozen application suite and the independent Python version passed 127 of 127 (127/127 in each language). Every measured program output matched its expected output. This checkpoint is macOS ARM64 only.

## Lockfile auditing

Seven randomized fresh-process timing pairs per size, exact-output checks, Guion's source cache bypassed, one Apple Silicon host. It is not evidence that Guion is generally faster than Python.

| Packages | Guion | Python | Ratio (95% interval) | Peak memory, Guion / Python |
|---|---|---|---|---|
| 16,384 | 77.71 ms | 162.23 ms | 0.477× (0.471–0.479) | 29.7 / 46.7 MiB |
| 8,192 | 46.47 ms | 123.70 ms | 0.379× (0.368–0.384) | 20.7 / 39.3 MiB |
| 4,096 | 30.96 ms | 103.25 ms | 0.299× (0.286–0.301) | 16.2 / 35.5 MiB |

The 16,384-package case has 32,768 diagnostics. Peak memory there was 29.7 MiB for Guion and 46.7 MiB for Python. Frozen acceptance cases for the lockfile auditor: 17/17 in Guion and 17/17 in Python.

## Compute workloads

Uncached source against CPython, 15 randomized trials each.

| Workload | Ratio (95% interval) |
|---|---|
| N-body | 0.146 (0.145–0.147) |
| Mandelbrot | 0.279 (0.266–0.287) |
| Parallel CPU, 200,000 items, against serial Python | 0.145 (0.142–0.157) |

## Parallel HTTP cancellation

The focused test `parallel_worker_failure_cancels_in_flight_http_sibling_and_closes_socket` passed 1/1 on this build, on macOS ARM64. The test uses a local loopback fixture with two workers. They do not establish a general async runtime. The calendar, service-health and agent-example counts from the earlier report were not re-run here.
