# Veritas RC27

RC27 adds a lossless binary data path through Veritas file operations, versioned artifacts, and Python `bytes` interop.

- `veritas-0.1.0-darwin-arm64.tar.gz` is the unsigned macOS ARM64 runnable bundle.
- `veritas-foundation-source-rc27.tar.gz` contains the complete source workspace, examples, tests, docs, and evaluation assets.
- `veritas_checkpoint_v1_10_bytes.md` records the implementation checkpoint.
- `veritas_rc27_milestone_report.md` records verification evidence, limits, and the next work.
- Each archive has a SHA-256 sidecar.

The bundle includes `language/examples/binary_copy.vr`. Place an `input.bin` file next to it and run `veritas run binary_copy.vr`; it writes `copy.bin` and prints the byte count.
