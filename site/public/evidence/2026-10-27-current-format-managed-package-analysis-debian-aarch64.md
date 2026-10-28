# Current-format managed analysis package on Debian ARM64 — 2026-10-27

## Result

**Passed.** The real 100,000-row NumPy/orjson analysis ran from a source-free Guion format-44 package on Debian 12 ARM64 (glibc 2.36). The acceptance run had networking disabled and used preloaded CPython, uv, and dependency caches.

The package was verified, moved to a path containing spaces and Unicode, and launched twice with external Python and uv settings removed. Both launches succeeded, printed the expected row count and total, and produced a report identical byte-for-byte to the standard-library Python reference. The source program and input data were not included in the package.

## Measured package contents

- Uncompressed package directory: **244,199,858 bytes** (about **232.9 MiB**, 6,731 files). This is an on-disk directory size, not a compressed download size.
- Managed CPython 3.14.8: **81,670,318 bytes** (about **77.9 MiB**).
- Offline NumPy/orjson cache: **60,936,598 bytes** (about **58.1 MiB**).
- Workload input: **5,100,867 bytes**, generated outside the package.
- Input SHA-256: `7c34a5aeb714026e2624d64980999cc40612b6f230843516d5b6b9f59279a40b`.
- Workload: **100,000 rows**, 13 groups, total **2,500,116,287**.
- Reference output: **1,256 bytes**, SHA-256 `86d880db3ae3bf9f1a7fa3f3e3b4f24b250bef4dc6f08e822b41ac977ab5d117`.

The two correctness launches took 0.6145 seconds and 0.4654 seconds. Those times verify repeatability; they are not a benchmark or a speed comparison. Offline dependency sync took 0.899 seconds, package assembly 2.697 seconds, and package verification 1.180 seconds in this run.

## What this establishes

This closes the named format-44 managed-package analysis outcome on Debian ARM64, alongside the existing macOS ARM64 result. It shows that the package can carry its required Python runtime and native dependencies, relocate, and run offline for this workload.

It does **not** establish a speed advantage, broad Python package compatibility, or general Linux support. Format-44 managed-package validation on Debian x86_64 remains open; the older three-target validation used artifact format 39.

## Run hygiene

The acceptance container had no network access. Its temporary work directory was removed on exit. The ARM64 cache is retained only until this result is committed, then removed to reclaim space. The x86_64 run needs architecture-matched runtime and dependency caches; those will be prepared once before its offline acceptance run in the same VM. The VM remains running for that follow-up.

## Evidence

- Raw result: [`2026-10-27-current-format-managed-package-analysis-debian-aarch64.json`](2026-10-27-current-format-managed-package-analysis-debian-aarch64.json)
- Shared runner: [`verify_managed_package_analysis.py`](../../performance/verify_managed_package_analysis.py)
- Base revision: `ba2790e3f643ec101978492fbc1a5c4f50144dc5`
- Artifact format: 44
