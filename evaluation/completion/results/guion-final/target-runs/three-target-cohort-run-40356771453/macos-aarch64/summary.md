# Guion release-target verification

Source revision: `51aeb93a93724b4aa81171250837cca04efcb383`
Target: `macos-aarch64`
Host: macOS 15.7.9, arm64,  

| Gate | Result | Exit | Target |
| --- | --- | ---: | --- |
| `conformance-inventory` | PASS | 0 | macos-aarch64 |
| `bootstrap` | PASS | 0 | both |
| `conformance` | PASS | 0 | both |
| `stdlib-data` | PASS | 0 | both |
| `stdlib-services` | PASS | 0 | both |
| `runtime` | PASS | 0 | both |
| `product-dependencies` | PASS | 0 | both |
| `product-lifecycle` | PASS | 0 | both |
| `product-applications` | PASS | 0 | both |
| `release` | PASS | 0 | both |
| `application reports` | PASS (18/18) | — | both |

Target result: **PASS**.
Three-target aggregates remain pending the remaining target runners.
