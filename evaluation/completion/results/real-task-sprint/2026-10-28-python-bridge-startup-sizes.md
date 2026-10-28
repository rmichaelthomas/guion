# Python bridge empty and 5.1 MB no-op controls — 2026-10-28

## Result

This four-lane control compares one typed `Bytes` call returning only `len(source)` with either an empty value or the 5,100,867-byte application input. Both Guion and Python import NumPy and orjson, start a fresh process for every sample, and print the expected byte count.

| Payload | Guion median | Python median | Paired Guion/Python ratio (95% interval) | Guion slower in pairs |
| ---: | ---: | ---: | ---: | ---: |
| 0 bytes | 79.89 ms | 79.98 ms | 0.993 (0.979–1.019) | 5/15 |
| 5,100,867 bytes | 83.15 ms | 80.68 ms | 1.043 (1.004–1.050) | 14/15 |

The relative penalty as payload size grows was **1.045** (95% interval **1.001–1.088**). This points to work that scales with the bytes crossing the bridge. The zero-byte lane is at parity within measurement uncertainty; the 5.1 MB transfer is measurably slower in Guion. Every output matched the expected byte count and every stderr was empty.

This isolates the payload-dependent portion of the no-op path, but it does not split file read, pipe transfer, frame parsing, and attachment conversion into CPU costs. The next optimization targets one avoidable copy in the sidecar's frame reader.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- 15 randomized fresh-process pairs across four lanes: empty and 5.1 MB, each in Guion and Python. Every timed output was checked.
- The benchmark source hashes and all samples are in [`2026-10-28-python-bridge-startup-sizes.json`](2026-10-28-python-bridge-startup-sizes.json). The report records unrelated untracked site files; they were not used by the benchmark.
- OS file-cache state was uncontrolled. No VM or dependency download was used; temporary input files were removed automatically. No host CPU profile or whole-process peak-memory measurement is available.
