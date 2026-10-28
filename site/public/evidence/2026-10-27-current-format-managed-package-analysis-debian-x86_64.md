# Current-format managed package result — Debian 12 x86_64

**Result: passed.** The current artifact-format-44 CLI packaged the 100,000-row NumPy/orjson analysis as a source-free managed application on Debian 12 x86_64 with glibc 2.36. The full machine-readable record is in the adjacent [raw JSON report](2026-10-27-current-format-managed-package-analysis-debian-x86_64.json).

The input file stayed outside the package. Its output contained 13 groups and a total of 2,500,116,287. After the package was moved to a path containing spaces and Unicode, two launches succeeded with external Python and uv settings removed. Both produced the same 1,256-byte report as the independent standard-library Python reference; console output also matched and stderr was empty.

The uncompressed package directory measured **274,853,607 bytes** (about **262.1 MiB**) across 6,545 files. It includes CPython 3.14.8 (101,961,520 bytes) and an offline NumPy 2.5.4/orjson 3.13.0 cache (59,505,505 bytes). uv 0.13.4 performed the dependency setup. The 5,100,867-byte caller input was not embedded in the package.

The two launch times were 2.273 and 2.009 seconds under Rosetta translation. They are correctness and repeatability observations, **not a performance comparison**. Package assembly took 12.784 seconds in this run. The acceptance container had no network access and was limited to two CPUs and 6 GiB of memory. The run's temporary project, dataset, and relocated package were removed when the runner exited.

The first attempt stopped during `ldd` inspection under QEMU and produced no package result; the [attempt record](2026-10-27-current-format-managed-package-analysis-debian-x86_64-attempt.md) preserves that history. The acceptance passed after enabling Rosetta in the same existing Colima VM and reusing the already-built CLI and offline dependency cache. This closes the named format-44 analysis-package outcome on Debian 12 x86_64. It does not establish broad Python package compatibility or general performance.
