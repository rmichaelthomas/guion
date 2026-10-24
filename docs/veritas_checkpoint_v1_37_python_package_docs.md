# Veritas checkpoint v1.37 — Python package documentation alignment

Status: implemented and validated in the RC60 installed release.

The Python ABI guide previously said that current bundles did not contain relocatable CPython, contradicting the package implementation and successful same-target offline relocation evidence. It now distinguishes the development `sync` step from app packaging: `sync` provisions the managed project environment, while `package` copies managed CPython, the locked bridge environment and offline cache into the app bundle. The guide scopes relocation evidence to macOS ARM64 and names the tested `feedparser`, `orjson` and NumPy cases.

The reference guide now identifies artifact format 22 as current instead of describing format 16 metadata as the current artifact. The README explains the development-sync and app-bundling sequence.

The shipped acceptance kit includes a check that protects the Python ABI packaging description against the old contradictory claim. The Rust workspace has 118 passing tests with no failures (from RC59; this update changes docs and Python acceptance code only). The packaged RC60 suite has 22 passing acceptance tests and no failures. The package installer, relocation, integrity, and repeated-launch checks passed.
