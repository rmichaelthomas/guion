# Guion release-target verification

Source revision: `0fecfbf1694474b5e85bebb8bc4f87db29275f53`
Target: `debian-x86_64`
Host: Debian 12.15, x86_64, glibc 2.36

| Gate | Result | Exit | Target |
| --- | --- | ---: | --- |
| `conformance-inventory` | PASS | 0 | debian-x86_64 |
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
| `three-target aggregate` | PASS | — | all targets |

**Overall: PASS.**
