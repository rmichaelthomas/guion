# Python bridge empty and 5.1 MB no-op controls — 2026-10-28

## Result

This four-lane control compares one typed `Bytes` call returning only `len(source)` with either an empty value or the 5,100,867-byte application input. Both Guion and Python import NumPy and orjson, start a fresh process for every sample, and print the expected byte count.

| Payload | Guion median | Python median | Paired Guion/Python ratio (95% interval) | Guion slower in pairs |
| ---: | ---: | ---: | ---: | ---: |
| 0 bytes | 79.89 ms | 79.98 ms | 0.993 (0.979–1.019) | 5/15 |
| 5,100,867 bytes | 83.15 ms | 80.68 ms | 1.043 (1.004–1.050) | 14/15 |

The relative penalty as payload size grows was **1.045** (95% interval **1.001–1.088**). This points to work that scales with the bytes crossing the bridge. The zero-byte lane is at parity within measurement uncertainty; the 5.1 MB transfer is measurably slower in Guion. Every output matched the expected byte count and every stderr was empty.

This isolates the payload-dependent portion of the no-op path, but it does not split file read, pipe transfer, frame parsing, and attachment conversion into CPU costs.

## Follow-up on the current PR binary

The same control was rerun against the current PR binary (`296c750`) in 15 randomized pairs, preserving the earlier cohort and report. The empty-payload ratio was **1.017** (95% interval **0.993–1.065**), which does not distinguish Guion from Python. At 5,100,867 bytes, the ratio was **1.108** (**1.045–1.155**); the relative payload penalty was **1.055** (**1.007–1.210**). All outputs matched. This is weaker evidence than the 30-pair batch comparison and has a broad interval; it keeps large-payload transfer on the open list but does not localize its CPU cost. Raw samples and binary identity are in [`2026-10-28-python-bridge-startup-sizes-current-binary.json`](2026-10-28-python-bridge-startup-sizes-current-binary.json).

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- 15 randomized fresh-process pairs across four lanes: empty and 5.1 MB, each in Guion and Python. Every timed output was checked.
- The benchmark source hashes and all samples are in [`2026-10-28-python-bridge-startup-sizes.json`](2026-10-28-python-bridge-startup-sizes.json). The report records unrelated untracked site files; they were not used by the benchmark.
- OS file-cache state was uncontrolled. No VM or dependency download was used; temporary input files were removed automatically. No host CPU profile or whole-process peak-memory measurement is available.
