# Veritas checkpoint v1.36 — structured environment readiness

Status: implemented and validated in the RC59 installed release.

`veritas doctor --json` emits a versioned report with host and artifact identity, native VM availability, Python sidecar mode, managed `uv` readiness, and capability flags for pure Veritas programs and Python calls. Missing optional Python tooling leaves pure-Veritas readiness available while Python interoperability is reported separately.

Tests cover a missing managed resolver with no explicit Python and an explicit Python path when the managed resolver is absent. The report is for environment discovery; an explicit interpreter path is reported as configured and is not independently executed or version-validated by `doctor`. The complete Rust workspace reports 118 passed tests and 0 failures; all 21 bundled Python acceptance tests pass in the installed release. Formatting, strict Clippy, archive checksum, installer relocation, and repeat-launch checks also pass.

Cross-platform releases, dynamic native provenance, and the independent assistant recommendation evaluation remain open.
