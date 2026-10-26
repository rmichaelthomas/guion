# Veritas RC28

RC28 adds source-level Python keyword arguments and lets omitted parameters use Python defaults.

- `veritas-0.1.0-darwin-arm64.tar.gz` is the unsigned macOS ARM64 runnable bundle.
- `veritas-foundation-source-rc28.tar.gz` contains the source workspace, docs, examples, tests, and evaluation assets.
- `veritas_checkpoint_v1_11_python_keyword_calls.md` records the implementation checkpoint.
- `veritas_rc28_milestone_report.md` records test and package evidence plus remaining north-star work.
- Each archive has a SHA-256 sidecar.

The bundle includes `language/examples/python_keywords.vr`. It calls Python's `pow` with `exp=10`, leaves `mod` to Python's default, and prints `1024`. Set `VERITAS_PYTHON` to an explicit interpreter path before running it.
