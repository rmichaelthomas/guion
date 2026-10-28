# Python bridge no-op call, 5.1 MB input — 2026-10-28

## Result

This probe removes application computation while retaining fresh-process startup, the Python sidecar, NumPy/orjson imports, a typed `Bytes` call, and the 5,100,867-byte input transfer. The Python function only returns `len(source)`; both programs print the expected byte count.

Across 15 randomized fresh-process pairs, Guion measured **89.22 ms** median and direct Python measured **81.99 ms**. The paired Guion/Python ratio was **1.078** (95% interval **1.045–1.121**); 14 of 15 pairs were slower in Guion. Every stdout matched the expected byte count and every stderr was empty.

This demonstrates a measurable cost before application computation on the payload-heavy path. It does not separate process startup from transferring the 5.1 MB attachment, nor does it provide CPU attribution. The next control compares the same call with an empty `Bytes` value so the payload-dependent portion can be separated.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- One fresh Guion or Python process per sample; 15 randomized pairs; all samples were output-checked.
- The benchmark source hashes and raw timings are in [`2026-10-28-python-bridge-startup-5mb.json`](2026-10-28-python-bridge-startup-5mb.json). The report records unrelated untracked site files in the checkout; they were not used by the probe.
- OS file-cache state was uncontrolled. No VM or dependency download was used; temporary input files were removed automatically. The benchmark does not provide host CPU attribution or whole-process peak memory.
