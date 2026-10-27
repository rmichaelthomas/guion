# Lockfile auditor scale follow-up — 2026-10-27

## Result

The lockfile auditor now completes the 8,192-package fixture and writes the exact expected report containing 16,384 sorted findings. The merged baseline failed this same case with `R_STEP_LIMIT` at Guion's 2,000,000-instruction default.

The lowest passing configured ceiling found by a boundary probe was 2,154,912 instructions. The application's `veritas.toml` now sets 2,250,000, leaving about 95,000 instructions of headroom. This is an application-level limit; Guion's global default is unchanged. No runtime or compiler code changed in this pass.

## Speed and memory

Seven randomized, output-checked fresh-process pairs compared uncached Guion source with the Python reference. Fixture generation was outside the timed interval, and OS file-cache state was uncontrolled.

| Packages | Guion median | Python median | Paired Guion/Python time | Guion peak RSS | Python peak RSS |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 4,096 | 76.2 ms | 114.5 ms | 0.666 (95% interval 0.653–0.670) | 31.3 MB | 38.0 MB |
| 8,192 | 131.1 ms | 136.3 ms | 0.974 (95% interval 0.960–0.992) | 51.2 MB | 42.2 MB |

At 4,096 packages, the candidate's timing remained statistically consistent with the pre-follow-up build (candidate/baseline ratio 0.992; 95% interval 0.976–0.996). At 8,192, Guion was about 2.6% faster by the paired median, while using about 21% more peak memory than Python.

## Verification

- The new 8,192-package regression test passed with exact sorted-output comparison.
- Lockfile application acceptance passed all 17 cases in Guion and all 17 in Python.
- The performance harness unit suite passed all 34 tests.
- The model artifact compatibility test passed.
- The release CLI build and calendar source check had already passed on the merged baseline before these manifest, harness, and documentation changes; no runtime or compiler implementation code changed.

The current evidence closes the 8,192-package correctness and budget boundary. It does not establish performance or memory behavior above 8,192 packages, and the memory gap remains a concrete follow-up target.

## Evidence

- [Raw timing and peak-memory samples](2026-10-27-lockfile-scale-postmerge-fix.json)
- [Lockfile acceptance results](2026-10-27-lockfile-acceptance-postmerge-fix.json)
- [Repeatable 8,192-package regression test](../../performance/test_lockfile_scale.py)
- [Previous scale checkpoint](2026-10-27-lockfile-auditor-scale.md)
