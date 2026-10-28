# Current-format managed package for 100,000-row analysis — 2026-10-27

## Result

The actual NumPy/orjson analysis now passes as a source-free, managed Guion package on macOS ARM64, using artifact format 44. The package was built offline from the preloaded CPython and uv caches, moved to a path with spaces and Unicode, and launched twice without external Python or uv settings. Both launches produced the exact same report bytes and console output as the independent standard-library Python reference.

The package executable was built from the implementation in this PR; the raw JSON records `2405ba1` as the branch's starting revision.

This closes the managed-package and same-host relocation outcome for this workload. It does not close current-format Linux packaging, general native-wheel coverage, or the performance gap measured for the Python bridge.

## What ran

- Generated **100,000 rows** in a **5,100,867-byte** JSON input outside the package. The package does not contain the dataset or Guion source.
- Grouped the data into **13 categories**. The total was **2,500,116,287**.
- Compared the output report byte-for-byte with the standard-library Python reference. Both package launches wrote the same **1,256-byte** report with SHA-256 `86d880db3ae3bf9f1a7fa3f3e3b4f24b250bef4dc6f08e822b41ac977ab5d117` and printed:

  ```text
  Rows:
  100000
  Total:
  2500116287
  ```

- Bundled managed **CPython 3.14.8**, **uv 0.13.4**, and native **NumPy 2.5.4** and **orjson 3.13.0** wheels.
- Verified the package before moving it. Both relocated launches succeeded with `GUION_UV`, `GUION_PYTHON`, `GUION_PYTHON_INSTALL_DIR`, `GUION_PYTHON_CACHE_DIR`, and external `UV_*`/`PYTHON*` settings absent.

## Package size and time

The expanded app directory contained 3,802 files and occupied **157,585,367 bytes** (**150.3 MiB**) on disk. It included a **68,119,333-byte** managed Python runtime and a **27,172,671-byte** offline uv cache. The generated caller data was excluded from that size.

Offline dependency sync took **0.26 seconds** and the package assembly took **5.36 seconds**. The first relocated acceptance launch took **1.66 seconds** and the second **0.45 seconds**. These two correctness launches are not a performance benchmark and should not be compared with the separate Python bridge timing study. A current package directory size was measured; this run did not create or measure a compressed distribution archive. The separate 128 MiB limit applies to downloaded native-library package archives, not this application directory.

## Offline cache behavior

The first offline package attempt showed that Guion could reuse an installed CPython tree but was not copying the preloaded uv wheel cache into the new app. Package assembly now accepts `GUION_PYTHON_CACHE_DIR` and seeds the app's `.uv-cache` from that directory before syncing its locked Python environment. The regression test covers copying wheel payloads and their index records. The acceptance runner sets this alongside the existing `GUION_PYTHON_INSTALL_DIR`; all package operations used the one warmed cache under ignored `target/perf-session/managed-python-package`.

This copies the selected cache as a whole. Use a project-scoped cache when unused cached packages should not increase a bundle's size.

## Limits and next work

- This is one macOS ARM64 package run. Current artifact-format-44 package validation on Debian 12 ARM64 and x86_64 remains open.
- NumPy/orjson execution through the Guion bridge previously measured **173.5 ms**, versus **149.3 ms** for the same Python package function and **119.0 ms** for standard-library Python. Packaging this real task does not change those timings; see the [bridge measurement report](2026-10-27-python-bridge-package-analysis.md).
- Two launches confirm relocation and repeatability, not broad Python package compatibility, clean-machine downloads, or cross-host native-library behavior.

The reproducible acceptance runner is [`verify_managed_package_analysis.py`](../../performance/verify_managed_package_analysis.py). The raw machine-readable result, including input/output checksums and per-step timings, is [`2026-10-27-current-format-managed-package-analysis.json`](2026-10-27-current-format-managed-package-analysis.json).

## Resource cleanup

No VM was started. The generated dataset, temporary project, partial package attempts, and relocated package were confined to ignored repository scratch and removed on exit. The existing **124 MiB** warmed runtime/toolchain cache was retained for reuse; free disk space was **55 GiB** after the final run. The Guion and uv processes were synchronous children of the completed runner; no background service or VM was started.
